//! Platform-agnostic rendering abstraction layer.
//!
//! This module defines the data types and builder function that convert engine
//! state into a `ScreenLayout` — the shared contract between the GTK/Cairo
//! backend and any future TUI backend.
//!
//! **Critical:** No GTK, Cairo, Pango, or Relm4 dependencies are allowed here.
//! All types must be plain Rust structs with no platform coupling.

// Many public fields and methods are part of the rendering API consumed by the
// Cairo backend and reserved for the future TUI backend; dead_code warnings
// are expected for unused-in-this-binary items.
#![allow(dead_code)]

use crate::core::buffer::Buffer;
use crate::core::dap::DapVariable;
use crate::core::engine::{AlignedDiffEntry, DiffLine, Engine, PanelChromeDesc, SearchDirection};
pub use crate::core::engine::{BottomPanelKind, DebugSidebarSection};
use crate::core::lsp::SignatureHelpData;
use crate::core::settings::LineNumberMode;
use crate::core::view::View;
use crate::core::window::{GroupDivider, GroupId, SplitDirection, WindowDivider};
use crate::core::{Cursor, GitLineStatus, Mode, WindowId, WindowRect};
use crate::icons;

// ─── Color ───────────────────────────────────────────────────────────────────

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse a `#rrggbb` hex string. Panics on invalid input (all callers use
    /// compile-time constants so this is acceptable).
    pub fn from_hex(s: &str) -> Self {
        let s = s.trim_start_matches('#');
        assert!(s.len() == 6, "Color::from_hex expects #rrggbb");
        let r = u8::from_str_radix(&s[0..2], 16).expect("invalid hex");
        let g = u8::from_str_radix(&s[2..4], 16).expect("invalid hex");
        let b = u8::from_str_radix(&s[4..6], 16).expect("invalid hex");
        Self { r, g, b }
    }

    /// Try to parse a hex colour string. Accepts `#rrggbb`, `#rrggbbaa`
    /// (alpha is discarded), and `#rgb` shorthand. Returns `None` on failure.
    pub fn try_from_hex(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('#');
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
        Some(Self { r, g, b })
    }

    /// Parse `#rrggbbaa` and alpha-blend against `bg`. If no alpha component
    /// is present, behaves identically to `try_from_hex`.
    pub fn try_from_hex_over(s: &str, bg: Color) -> Option<Self> {
        let s = s.trim_start_matches('#');
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
                Some(Self {
                    r: blend(r, bg.r),
                    g: blend(g, bg.g),
                    b: blend(b, bg.b),
                })
            }
            _ => Self::try_from_hex(s),
        }
    }

    /// Blend this colour toward white by `amount` (0.0 = unchanged, 1.0 = white).
    pub fn lighten(self, amount: f64) -> Self {
        let f = amount.clamp(0.0, 1.0);
        Self {
            r: (self.r as f64 + (255.0 - self.r as f64) * f) as u8,
            g: (self.g as f64 + (255.0 - self.g as f64) * f) as u8,
            b: (self.b as f64 + (255.0 - self.b as f64) * f) as u8,
        }
    }

    /// Blend this colour toward black by `amount` (0.0 = unchanged, 1.0 = black).
    pub fn darken(self, amount: f64) -> Self {
        let f = 1.0 - amount.clamp(0.0, 1.0);
        Self {
            r: (self.r as f64 * f) as u8,
            g: (self.g as f64 * f) as u8,
            b: (self.b as f64 * f) as u8,
        }
    }

    /// Derive a subtle cursorline background from this colour.
    /// Dark backgrounds get lightened; light backgrounds get darkened.
    pub fn cursorline_tint(self) -> Self {
        let lum = 0.299 * self.r as f64 + 0.587 * self.g as f64 + 0.114 * self.b as f64;
        if lum < 128.0 {
            self.lighten(0.06)
        } else {
            self.darken(0.04)
        }
    }

    /// Derive a subtle colorcolumn background from this colour.
    /// Slightly less prominent than cursorline — a gentle column tint.
    pub fn colorcolumn_tint(self) -> Self {
        let lum = 0.299 * self.r as f64 + 0.587 * self.g as f64 + 0.114 * self.b as f64;
        if lum < 128.0 {
            self.lighten(0.08)
        } else {
            self.darken(0.06)
        }
    }

    /// Normalise to the (0.0..=1.0, 0.0..=1.0, 0.0..=1.0) triple expected by
    /// Cairo's `set_source_rgb` / `set_source_rgba`.
    pub fn to_cairo(self) -> (f64, f64, f64) {
        (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
        )
    }

    /// Normalise to `(f32, f32, f32, f32)` RGBA with full opacity.
    /// Used by Direct2D (`D2D1_COLOR_F`) and Core Graphics (`CGColor`).
    pub fn to_f32_rgba(self) -> (f32, f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            1.0,
        )
    }

    /// Format as a CSS `#rrggbb` hex string.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Expand to the 16-bit (0..65535) values expected by Pango attribute
    /// constructors (`AttrColor::new_foreground` etc.).
    pub fn to_pango_u16(self) -> (u16, u16, u16) {
        (
            self.r as u16 * 257,
            self.g as u16 * 257,
            self.b as u16 * 257,
        )
    }
}

/// Strip `//` and `/* */` comments from JSON-with-comments (JSONC), as used
/// by VSCode theme files. Preserves newlines so error positions stay valid.
fn strip_json_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'"' {
            // String literal — copy verbatim until closing quote
            out.push('"');
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    out.push(bytes[i] as char);
                    out.push(bytes[i + 1] as char);
                    i += 2;
                } else if bytes[i] == b'"' {
                    out.push('"');
                    i += 1;
                    break;
                } else {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Line comment — skip until newline
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Block comment — skip until */
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2; // skip */
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
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
    /// Display label, e.g. `" 1: main.rs "`.
    pub name: String,
    /// Whether this is the currently active tab.
    pub active: bool,
    /// Whether the buffer has unsaved changes.
    pub dirty: bool,
    /// Whether the buffer is in preview mode.
    pub preview: bool,
}

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
    /// Pre-computed click regions for this tab bar, in char-cell units
    /// relative to the tab bar's left edge (column 0 = left edge of group bounds).
    pub hit_regions: Vec<(
        crate::core::engine::TabBarHitRegion,
        crate::core::engine::TabBarClickTarget,
    )>,
    /// Pre-built quadraui `TabBar` primitive — backends draw this directly.
    pub bar: quadraui::TabBar,
}

// ── Tab bar hit region constants (char-cell units) ──────────────────────────

/// Columns used by each tab's close button (the × itself + trailing space).
pub const TAB_CLOSE_COLS: u16 = 2;
/// Columns occupied by each split button (1 space + 2-wide glyph).
const TAB_SPLIT_BTN_COLS: u16 = 3;
/// Total columns reserved for both split buttons (right + down).
const TAB_SPLIT_BOTH_COLS: u16 = TAB_SPLIT_BTN_COLS * 2;
/// Columns for the editor action menu button ("…").
const TAB_ACTION_BTN_COLS: u16 = 3;
/// Columns per diff toolbar button (1 space + 1 char + 1 space).
const DIFF_BTN_COLS: u16 = 3;
/// Total columns for all three diff toolbar buttons.
const DIFF_TOOLBAR_BTN_COLS: u16 = DIFF_BTN_COLS * 3;

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
/// [`compute_tab_bar_hit_regions`], so nothing else measures a tab.
///
/// A tab named `" 1: 日本語.rs "` (11 chars, 14 columns) is now both measured
/// and painted 14 cells wide, so the next tab starts at cell 14 and every hit
/// box lands on the glyph it covers. Reverting this to `.chars().count()`
/// against a post-#554 quadraui shifts every hit box *left* of what is drawn
/// — the mirror image of the pre-#554 hazard, and caught by
/// `tui_main::render_impl::tests::
/// tab_hit_regions_match_painted_columns_for_wide_names`, which reads the
/// expected columns out of the rendered buffer.
fn tab_hit_width(t: &TabInfo) -> usize {
    quadraui::tui::display_width(&t.name) + TAB_CLOSE_COLS as usize
}

/// Compute hit regions for a group's tab bar.
///
/// Layout (left to right):
/// `[tab0][tab1]...[tabN]  [diff_toolbar?] [split_btns?] [action_btn]`
///
/// All positions are in char-cell columns relative to the tab bar left edge.
///
/// Per D6: layout math lives in `quadraui::TabBar::layout()`. This
/// function builds the TabBar primitive, asks it for a layout, and
/// converts the layout's `hit_regions` into the engine's legacy
/// `(TabBarHitRegion, TabBarClickTarget)` shape. Until TUI / GTK /
/// Win-GUI migrate to consume `TabBarLayout` directly, this shim
/// is the bridge — but the layout math itself has only one
/// source of truth now.
pub fn compute_tab_bar_hit_regions(
    tabs: &[TabInfo],
    tab_scroll_offset: usize,
    bar_width: u16,
    has_diff_toolbar: bool,
    diff_label_cols: u16,
    has_split_buttons: bool,
) -> Vec<(
    crate::core::engine::TabBarHitRegion,
    crate::core::engine::TabBarClickTarget,
)> {
    use crate::core::engine::{TabBarClickTarget, TabBarHitRegion};

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
    let tab_widths: Vec<usize> = tabs.iter().map(tab_hit_width).collect();

    let layout = primitive.layout(
        bar_width as f32,
        1.0,
        0.0, // scroll arrows disabled — matches existing TUI behaviour
        |i| quadraui::TabMeasure::new(tab_widths[i] as f32, TAB_CLOSE_COLS as f32),
        |i| {
            // TabBarSegment.width_cells is pre-computed by build_tab_bar_primitive
            // in legacy char-cell units, which is exactly what we want here.
            quadraui::SegmentMeasure::new(primitive.right_segments[i].width_cells as f32)
        },
    );

    // Convert layout hit regions → legacy (TabBarHitRegion, TabBarClickTarget).
    // Order preserved from the layout: close regions before tab bodies,
    // and segments (which are disjoint from tab regions) appended at the end.
    let mut regions = Vec::new();
    for (rect, hit) in &layout.hit_regions {
        let col = rect.x.round() as u16;
        let width = rect.width.round() as u16;
        let target = match hit {
            quadraui::TabBarHit::Tab(i) => Some(TabBarClickTarget::Tab(*i)),
            quadraui::TabBarHit::TabClose(i) => Some(TabBarClickTarget::CloseTab(*i)),
            quadraui::TabBarHit::RightSegment(id) => match id.as_str() {
                "tab:split_right" => Some(TabBarClickTarget::SplitRight),
                "tab:split_down" => Some(TabBarClickTarget::SplitDown),
                "tab:diff_prev" => Some(TabBarClickTarget::DiffPrev),
                "tab:diff_next" => Some(TabBarClickTarget::DiffNext),
                "tab:diff_toggle" => Some(TabBarClickTarget::DiffToggle),
                "tab:action_menu" => Some(TabBarClickTarget::ActionMenu),
                _ => None,
            },
            // Scroll arrows / Empty don't exist in the legacy enum — skipped.
            quadraui::TabBarHit::ScrollLeft
            | quadraui::TabBarHit::ScrollRight
            | quadraui::TabBarHit::Empty => None,
        };
        if let Some(t) = target {
            regions.push((TabBarHitRegion { col, width }, t));
        }
    }
    regions
}

/// Resolve a column position (in char cells, relative to the tab bar left edge)
/// to a `TabBarClickTarget` by walking the hit region list.
pub fn resolve_tab_bar_click(
    hit_regions: &[(
        crate::core::engine::TabBarHitRegion,
        crate::core::engine::TabBarClickTarget,
    )],
    col: u16,
) -> Option<crate::core::engine::TabBarClickTarget> {
    for (region, target) in hit_regions {
        if col >= region.col && col < region.col + region.width {
            return Some(*target);
        }
    }
    None
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
    let bg = to_quadraui_color(theme.breadcrumb_bg);
    let normal_fg = to_quadraui_color(theme.breadcrumb_fg);
    let active_fg = to_quadraui_color(theme.breadcrumb_active_fg);

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
pub fn breadcrumb_draw_targets(
    screen: &ScreenLayout,
    terminal_maximized: bool,
    line_height: f64,
) -> Vec<BreadcrumbDrawTarget<'_>> {
    if terminal_maximized {
        return Vec::new();
    }
    screen
        .breadcrumbs
        .iter()
        .filter(|bc| !bc.segments.is_empty() && bc.bounds.width > 0.0)
        .map(|bc| BreadcrumbDrawTarget {
            rect: quadraui::Rect::new(
                bc.bounds.x as f32,
                bc.bounds.y as f32,
                bc.bounds.width as f32,
                line_height as f32,
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
/// message handler (`Msg::CacheFontMetrics`) that stopped firing after the
/// #540 ShellApp migration, silently freezing the GTK backend's nerd-fonts
/// flag at its default (`false`) forever.
pub fn sync_nerd_fonts(b: &mut dyn quadraui::Backend, engine: &Engine) {
    b.set_nerd_fonts(engine.settings.use_nerd_fonts);
}

/// One tab bar ready to be painted via `Surface::TabBar` (GTK) or
/// `render_tab_bar` (TUI).
pub struct TabBarDrawTarget<'a> {
    pub rect: quadraui::Rect,
    pub bar: &'a quadraui::TabBar,
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
        .filter(|gtb| !engine.is_tab_bar_hidden(gtb.group_id) && gtb.bounds.width > 0.0)
        .map(|gtb| TabBarDrawTarget {
            rect: quadraui::Rect::new(
                gtb.bounds.x as f32,
                (gtb.bounds.y - reserved_h) as f32,
                gtb.bounds.width as f32,
                tab_row_h as f32,
            ),
            bar: &gtb.bar,
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

// ─── Per-window status line ──────────────────────────────────────────────────

// Re-export from core for use by backends.
pub use crate::core::engine::StatusAction;

/// A styled segment of a per-window status line (e.g. mode badge, filename, cursor position).
#[derive(Debug, Clone)]
pub struct StatusSegment {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    /// Action triggered when this segment is clicked, or `None` for non-interactive segments.
    pub action: Option<StatusAction>,
}

/// Per-window status line data (Vim-style). Active windows get a rich,
/// colorful bar; inactive windows get a dimmed minimal bar.
#[derive(Debug, Clone)]
pub struct WindowStatusLine {
    pub left_segments: Vec<StatusSegment>,
    pub right_segments: Vec<StatusSegment>,
}

// ─── RenderedWindow ───────────────────────────────────────────────────────────

/// All data needed to render one editor window (pane).
#[derive(Debug)]
pub struct RenderedWindow {
    pub window_id: WindowId,
    /// Pixel-space rectangle for the GTK backend (ignored by TUI).
    pub rect: WindowRect,
    /// Visible lines, one per row.
    pub lines: Vec<RenderedLine>,
    /// Cursor position + shape, or `None` if the cursor is scrolled off-screen.
    pub cursor: Option<(CursorPos, CursorShape)>,
    /// Secondary cursor positions (multi-cursor Alt-D). Rendered as dimmed blocks.
    pub extra_cursors: Vec<CursorPos>,
    /// Active visual selection, or `None`.
    pub selection: Option<SelectionRange>,
    /// Extra selections for Ctrl+D multi-cursor word selections.
    pub extra_selections: Vec<SelectionRange>,
    /// Index of the first visible buffer line.
    pub scroll_top: usize,
    /// Number of character columns scrolled horizontally.
    pub scroll_left: usize,
    /// Total lines in the buffer (for scrollbar calculation).
    pub total_lines: usize,
    /// Width of the line-number gutter in *character cells* (0 = no gutter).
    /// GTK backend multiplies by `char_width` to get pixels.
    pub gutter_char_width: usize,
    /// Exact number of text columns visible (rect width minus gutter minus
    /// scrollbar, divided by char_width). Backends should feed this back
    /// to `Engine::set_viewport_for_window` so `ensure_cursor_visible`
    /// uses accurate geometry.
    pub text_viewport_cols: usize,
    /// Whether this is the focused window.
    pub is_active: bool,
    /// Whether to render with the slightly-different active-window background
    /// (only true when `is_active` AND there are multiple windows).
    pub show_active_bg: bool,
    /// Whether the buffer has git diff data (controls git column in gutter).
    pub has_git_diff: bool,
    /// Whether to show the breakpoint gutter column (any breakpoint set for
    /// this file, or a DAP session is active).
    pub has_breakpoints: bool,
    /// Maximum line length across the whole buffer (character cells, excluding
    /// trailing newline).  Used by backends to size the horizontal scrollbar.
    pub max_col: usize,
    /// Per-line worst diagnostic severity (line index → severity). Used for gutter icons.
    pub diagnostic_gutter: std::collections::HashMap<usize, crate::core::lsp::DiagnosticSeverity>,
    /// Lines that have available LSP code actions (for lightbulb gutter icon).
    pub code_action_lines: std::collections::HashSet<usize>,
    /// Transient yank-highlight region (flashes briefly after a yank). `None` if no active highlight.
    pub yank_highlight: Option<SelectionRange>,
    /// Bracket pair positions to highlight (cursor bracket + matching bracket).
    /// Each entry is (view_line, col). Up to 2 entries.
    pub bracket_match_positions: Vec<(usize, usize)>,
    /// The indent guide column that should be highlighted as "active" (cursor's scope).
    pub active_indent_col: Option<usize>,
    /// Tab stop width for expanding `\t` to spaces in TUI rendering.
    pub tabstop: usize,
    /// Whether to draw cursorline highlight (from `settings.cursorline`).
    pub cursorline: bool,
    /// Per-window status line (Vim-style), or `None` when the setting is off.
    pub status_line: Option<WindowStatusLine>,
}

// ─── CommandLineData ──────────────────────────────────────────────────────────

/// Data needed to render the command / message line.
#[derive(Debug, Clone)]
pub struct CommandLineData {
    /// Text to display.
    pub text: String,
    /// When `true`, right-align the text (used for count prefix display).
    pub right_align: bool,
    /// When `true`, draw an insert cursor at the end of `cursor_anchor_text`.
    pub show_cursor: bool,
    /// Text whose rendered pixel-width determines the cursor's x position.
    /// Often equal to `text`, but may differ (e.g. history-search display).
    pub cursor_anchor_text: String,
}

// ─── WildmenuData ─────────────────────────────────────────────────────────────

/// Data for the command-line wildmenu (Tab completion bar above the status line).
#[derive(Debug, Clone)]
pub struct WildmenuData {
    /// Display labels shown in the bar (may be shortened, e.g. just the argument).
    pub items: Vec<String>,
    /// Currently highlighted item index, or `None` for common-prefix mode.
    pub selected: Option<usize>,
}

/// Convert wildmenu data to a quadraui `StatusBar` for shared rendering.
pub fn wildmenu_to_status_bar(wm: &WildmenuData, theme: &Theme) -> quadraui::StatusBar {
    let fg = quadraui::Color::rgb(
        theme.wildmenu_fg.r,
        theme.wildmenu_fg.g,
        theme.wildmenu_fg.b,
    );
    let bg = quadraui::Color::rgb(
        theme.wildmenu_bg.r,
        theme.wildmenu_bg.g,
        theme.wildmenu_bg.b,
    );
    let sel_fg = quadraui::Color::rgb(
        theme.wildmenu_sel_fg.r,
        theme.wildmenu_sel_fg.g,
        theme.wildmenu_sel_fg.b,
    );
    let sel_bg = quadraui::Color::rgb(
        theme.wildmenu_sel_bg.r,
        theme.wildmenu_sel_bg.g,
        theme.wildmenu_sel_bg.b,
    );
    let segments = wm
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_sel = wm.selected == Some(i);
            quadraui::StatusBarSegment {
                text: format!(" {} ", item),
                fg: if is_sel { sel_fg } else { fg },
                bg: if is_sel { sel_bg } else { bg },
                bold: is_sel,
                action_id: None,
            }
        })
        .collect();
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("wildmenu"),
        left_segments: segments,
        right_segments: vec![],
    }
}

// ─── CompletionMenu ────────────────────────────────────────────────────────────

/// Data needed to render the word-completion popup in insert mode.
#[derive(Debug, Clone)]
pub struct CompletionMenu {
    /// Sorted list of candidates.
    pub candidates: Vec<String>,
    /// Index of the currently highlighted candidate.
    pub selected_idx: usize,
    /// Length (in chars) of the longest candidate — used for popup width.
    pub max_width: usize,
}

/// Convert a render-side `CompletionMenu` into a `quadraui::Completions`
/// for backend rasterisation via the D6 layout pipeline.
///
/// vimcode's completion menu is string-only at this stage — no LSP
/// `CompletionKind` metadata — so every item ships as
/// `CompletionKind::Text`. A richer adapter lands when LSP
/// `CompletionItemKind` threads through the engine.
pub fn completion_menu_to_quadraui_completions(menu: &CompletionMenu) -> quadraui::Completions {
    let items = menu
        .candidates
        .iter()
        .map(|c| quadraui::CompletionItem {
            label: quadraui::StyledText::plain(c.clone()),
            detail: None,
            documentation: None,
            kind: quadraui::CompletionKind::Text,
            icon: None,
        })
        .collect();
    quadraui::Completions {
        id: quadraui::WidgetId::new("completions"),
        items,
        selected_idx: menu.selected_idx,
        scroll_offset: 0,
        has_focus: true,
    }
}

// ─── HoverPopup ──────────────────────────────────────────────────────────────

/// Convert an engine `HoverPopup` + on-screen anchor cell into a fully
/// resolved `quadraui::Tooltip` and its `TooltipLayout`.
///
/// `anchor_x` / `anchor_y` are the screen cell at the requested symbol
/// (cursor position, already resolved for scroll + gutter). The popup's
/// width is sized to the longest text line + 4 cells of padding /
/// border, and the height is the line count clamped to 20.
///
/// Placement is `Top` with fallback `Bottom` via the Tooltip primitive's
/// own viewport-fit logic. The anchor rectangle is given
/// `width = popup_width` so the primitive's horizontal-centering math
/// aligns the popup's left edge with the cursor cell (as the legacy
/// hover popup did). `margin=0` matches the legacy 0-cell gap above /
/// 0-cell gap below the cursor line.
/// Build a `quadraui::Tooltip` from the two fields every vimcode call
/// site must supply — `id` and `text` — with every other field at its
/// behaviour-preserving default (`styled_lines: None`, `placement:
/// Bottom`, `bg: None`, `fg: None`). Callers that need a non-default
/// then assign the public field directly.
///
/// # Why a local helper and not `quadraui::Tooltip::new` (#661)
///
/// quadraui#541 added exactly this constructor upstream, as
/// `Tooltip::new(id, text)` plus `.with_styled_lines/.with_placement/
/// .with_bg/.with_fg`. It is **not callable from vimcode yet**: it
/// landed on quadraui `develop` *after* the rev this repo is pinned to
/// (`quadraui-pin.txt` = `f6d27c2`), and `build.rs` enforces that pin.
/// Calling it compiles only once the pin moves, which per
/// `quadraui-pin.txt` is its own deliberate, `cargo test`-verified
/// commit — ~70 quadraui commits ride along in `f6d27c2..develop`,
/// including tooltip-box and tab-label paint changes that restate this
/// repo's snapshots.
///
/// Routing through this helper still buys #661's actual goal today:
/// vimcode names `Tooltip`'s field set in exactly **one** place instead
/// of seven exhaustive literals across three modules, so an upstream
/// field addition is a one-line fix here rather than seven `E0063`s.
/// When the pin does move, this body becomes a single delegation to
/// `quadraui::Tooltip::new(id, text)` and every call site is untouched.
pub fn quadraui_tooltip(id: quadraui::WidgetId, text: impl Into<String>) -> quadraui::Tooltip {
    quadraui::Tooltip {
        id,
        text: text.into(),
        styled_lines: None,
        placement: quadraui::TooltipPlacement::default(),
        bg: None,
        fg: None,
    }
}

/// `unit_w` / `unit_h` scale a single character cell / text row into the
/// caller's coordinate space — `1.0, 1.0` for TUI (already cell-native) or
/// `char_width, line_height` in pixels for GTK (#669). `anchor_x`/`anchor_y`
/// must already be expressed in that same space. `Tooltip::layout`'s own
/// anchor/viewport/measure arithmetic is unit-agnostic (plain `Rect` math),
/// so scaling only the chars/rows-derived sizes here is sufficient — no
/// backend-specific geometry needed at call sites.
pub fn hover_popup_to_quadraui_tooltip(
    hover: &HoverPopup,
    anchor_x: f32,
    anchor_y: f32,
    viewport: quadraui::Rect,
    unit_w: f32,
    unit_h: f32,
) -> (quadraui::Tooltip, quadraui::TooltipLayout) {
    let text_lines: Vec<&str> = hover.text.lines().take(20).collect();
    let num_lines = text_lines.len().max(1) as f32;
    let max_len = text_lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(10);
    // +4: 1 left border + 1 left pad + 1 right pad + 1 right border.
    let width = ((max_len + 4) as f32).max(12.0) * unit_w;
    let height = num_lines * unit_h;
    let mut tooltip = quadraui_tooltip(quadraui::WidgetId::new("lsp_hover"), hover.text.clone());
    tooltip.placement = quadraui::TooltipPlacement::Top;
    // anchor.width = popup width so the primitive's center-on-anchor x
    // math collapses to left-align with the cursor cell.
    let anchor = quadraui::Rect::new(anchor_x, anchor_y, width, unit_h);
    let measure = quadraui::TooltipMeasure::new(width, height);
    let layout = tooltip.layout(anchor, viewport, measure, 0.0);
    (tooltip, layout)
}

/// Data needed to render the LSP hover popup.
#[derive(Debug, Clone)]
pub struct HoverPopup {
    /// Text content to display.
    pub text: String,
    /// Buffer line where the hover was requested (for positioning).
    pub anchor_line: usize,
    /// Buffer column where the hover was requested.
    pub anchor_col: usize,
}

/// Data for rendering an editor hover popup with rich markdown content.
#[derive(Debug, Clone)]
pub struct EditorHoverPopupData {
    /// Rendered markdown content.
    pub rendered: crate::core::markdown::MdRendered,
    /// Clickable link regions: (line_idx, start_byte, end_byte, url).
    pub links: Vec<(usize, usize, usize, String)>,
    /// Buffer line where the hover is anchored (0-indexed).
    pub anchor_line: usize,
    /// Buffer column where the hover is anchored (0-indexed).
    pub anchor_col: usize,
    /// Scroll offset for long content.
    pub scroll_top: usize,
    /// Currently focused link index (for keyboard navigation).
    pub focused_link: Option<usize>,
    /// Whether the popup currently has keyboard focus (clicked or keyboard-triggered).
    pub has_focus: bool,
    /// Fixed popup width in characters, computed once when first shown.
    pub popup_width: usize,
    /// Frozen scroll offsets — used so the popup stays at a fixed screen position.
    pub frozen_scroll_top: usize,
    pub frozen_scroll_left: usize,
    /// Normalized text selection: (start_line, start_col, end_line, end_col).
    pub selection: Option<(usize, usize, usize, usize)>,
}

/// Maximum number of editor hover popup rows shown at once. Both the
/// TUI and GTK rasterisers obey this cap (longer content scrolls).
pub const EDITOR_HOVER_MAX_ROWS: usize = 20;

/// Geometry + drag-math inputs for a popup's scrollbar, captured by
/// the renderer so click/drag handlers don't have to recompute the
/// layout. Used by both backends for #215. Native units: cells (TUI)
/// or pixels (GTK) — matches whatever `RichTextPopupLayout` was built
/// with.
#[derive(Debug, Clone, Copy)]
pub struct PopupScrollbarHit {
    pub track: quadraui::Rect,
    pub thumb: quadraui::Rect,
    /// Number of content rows fitting in the viewport.
    pub visible_rows: usize,
    /// Total number of content rows in the popup.
    pub total: usize,
}

/// Flatten a `MdRendered` block into per-line `quadraui::StyledText` +
/// per-line heading font scale. Shared by every rich-text hover/popup
/// builder (editor hover, panel-item hover) so markdown → styled-span
/// conversion lives in exactly one place.
fn markdown_rendered_to_quadraui_lines(
    rendered: &crate::core::markdown::MdRendered,
    theme: &Theme,
) -> (Vec<quadraui::StyledText>, Vec<f32>) {
    let mut q_lines: Vec<quadraui::StyledText> = Vec::with_capacity(rendered.lines.len());
    let mut line_scales: Vec<f32> = Vec::with_capacity(rendered.lines.len());
    for (line_idx, line_text) in rendered.lines.iter().enumerate() {
        let md_spans = rendered.spans.get(line_idx);
        let code_hl = rendered.code_highlights.get(line_idx);
        q_lines.push(hover_line_to_styled_text(
            line_text,
            md_spans.map(|v| v.as_slice()).unwrap_or(&[]),
            code_hl.map(|v| v.as_slice()).unwrap_or(&[]),
            theme,
        ));
        // Heading rows render at a larger font scale (matches the
        // legacy `font_scale` on the render-side StyledSpan).
        let heading_level = md_spans
            .and_then(|spans| {
                spans.iter().find_map(|s| match s.style {
                    crate::core::markdown::MdStyle::Heading(n) => Some(n),
                    _ => None,
                })
            })
            .unwrap_or(0);
        let scale = match heading_level {
            1 => 1.4,
            2 => 1.2,
            3..=6 => 1.1,
            _ => 1.0,
        };
        line_scales.push(scale);
    }
    (q_lines, line_scales)
}

/// Convert `(line, start_byte, end_byte, url)` link tuples (the shape
/// shared by `EditorHoverPopupData` and `PanelHoverPopupData`) into
/// `quadraui::RichTextLink`s.
fn md_links_to_quadraui_rich_text_links(
    links: &[(usize, usize, usize, String)],
) -> Vec<quadraui::RichTextLink> {
    links
        .iter()
        .map(|(line, s, e, url)| quadraui::RichTextLink {
            line: *line,
            start_byte: *s,
            end_byte: *e,
            url: url.clone(),
        })
        .collect()
}

/// Convert an `EditorHoverPopupData` into a `quadraui::RichTextPopup`
/// for the D6 layout pipeline. Markdown style spans + tree-sitter code
/// highlights collapse into per-character `StyledSpan`s in
/// `quadraui::StyledText`; selection, focus, scroll, and link state
/// transfer 1:1.
pub fn editor_hover_to_quadraui_rich_text(
    eh: &EditorHoverPopupData,
    theme: &Theme,
) -> quadraui::RichTextPopup {
    let (q_lines, line_scales) = markdown_rendered_to_quadraui_lines(&eh.rendered, theme);
    let q_links = md_links_to_quadraui_rich_text_links(&eh.links);

    let q_selection = eh
        .selection
        .map(|(sl, sc, el, ec)| quadraui::TextSelection {
            start_line: sl,
            start_col: sc,
            end_line: el,
            end_col: ec,
        });

    quadraui::RichTextPopup {
        id: quadraui::WidgetId::new("editor_hover"),
        lines: q_lines,
        line_text: eh.rendered.lines.clone(),
        line_scales,
        scroll_top: eh.scroll_top,
        max_visible_rows: EDITOR_HOVER_MAX_ROWS,
        has_focus: eh.has_focus,
        selection: q_selection,
        links: q_links,
        focused_link: eh.focused_link,
        placement: quadraui::PopupPlacement::Above,
        padding: 0.0,
        fg: Some(to_quadraui_color(theme.hover_fg)),
        bg: Some(to_quadraui_color(theme.hover_bg)),
    }
}

/// Build, rasterise and hit-region-extract the editor hover popup via the
/// `quadraui::RichTextPopup` primitive. Shared by both backends' paint
/// paths (#669) — GTK previously duplicated this in the now-dead
/// `src/gtk/draw.rs::draw_editor_hover_popup`, with an added Pango-exact
/// link-width measure; that precision isn't reachable from
/// `render_content`'s `&mut dyn Backend`-only signature (no raw
/// `pango::Layout`, same class of gap TUI's own `render_editor_hover_popup`
/// hit for the raw `Frame` — see `PLAN.md`), so both backends now use the
/// same char-count-based `link_widths` closure, scaled by `unit_w`. This
/// only affects link *hit-region* precision, not paint — the rasteriser
/// re-measures glyphs itself when drawing.
///
/// `unit_w` / `unit_h` are `1.0, 1.0` for TUI (cell-native) or
/// `char_width, line_height` in pixels for GTK. `popup_x` / `popup_y` /
/// `viewport` must already be expressed in that same space.
///
/// Returns `(link_rects, popup_bounds, scrollbar_hit)` in the caller's
/// units, for mouse hit-testing — mirrors
/// `tui_main::panels::render_editor_hover_popup`'s return shape.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn editor_hover_popup_paint(
    backend: &mut dyn quadraui::Backend,
    eh: &EditorHoverPopupData,
    popup_x: f32,
    popup_y: f32,
    viewport: quadraui::Rect,
    theme: &Theme,
    unit_w: f32,
    unit_h: f32,
) -> (
    Vec<(f32, f32, f32, f32, String)>,
    Option<(f32, f32, f32, f32)>,
    Option<PopupScrollbarHit>,
) {
    if eh.rendered.lines.is_empty() {
        return (vec![], None, None);
    }
    let popup = editor_hover_to_quadraui_rich_text(eh, theme);
    let content_w = ((eh.popup_width as f32) * unit_w)
        .max(10.0 * unit_w)
        .min((viewport.width - 4.0 * unit_w).max(10.0 * unit_w));
    let measure = quadraui::RichTextPopupMeasure::new(content_w, unit_h);
    let layout = popup.layout(
        popup_x,
        popup_y,
        viewport,
        measure,
        |line_idx, start_byte, end_byte| {
            popup
                .line_text
                .get(line_idx)
                .map(|t| {
                    t[start_byte.min(t.len())..end_byte.min(t.len())]
                        .chars()
                        .count() as f32
                })
                .unwrap_or(0.0)
                * unit_w
        },
    );

    backend.draw_rich_text_popup(&popup, &layout);

    let link_rects: Vec<(f32, f32, f32, f32, String)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (rect.x, rect.y, rect.width, rect.height, url)
        })
        .collect();

    let popup_rect = Some((
        layout.bounds.x,
        layout.bounds.y,
        layout.bounds.width,
        layout.bounds.height,
    ));
    let scrollbar_hit = layout.scrollbar.map(|sb| PopupScrollbarHit {
        track: sb.track,
        thumb: sb.thumb,
        visible_rows: EDITOR_HOVER_MAX_ROWS,
        total: popup.lines.len(),
    });
    (link_rects, popup_rect, scrollbar_hit)
}

/// Flatten one rendered hover line (text + markdown spans + tree-sitter
/// code highlights) into a `quadraui::StyledText` whose spans correspond
/// to contiguous runs sharing fg/bold/italic.
fn hover_line_to_styled_text(
    line_text: &str,
    md_spans: &[crate::core::markdown::MdSpan],
    code_highlights: &[crate::core::markdown::MdCodeHighlight],
    theme: &Theme,
) -> quadraui::StyledText {
    use crate::core::markdown::MdStyle;
    if line_text.is_empty() {
        return quadraui::StyledText::default();
    }

    let default_fg = to_quadraui_color(theme.hover_fg);
    let h1_fg = to_quadraui_color(theme.md_heading1);
    let h2_fg = to_quadraui_color(theme.md_heading2);
    let h3_fg = to_quadraui_color(theme.md_heading3);
    let code_fg = to_quadraui_color(theme.md_code);
    let link_fg = to_quadraui_color(theme.md_link);

    // Style at byte position. Code highlights take priority on lines
    // that have any (matching the TUI rasteriser's behaviour).
    let style_at = |byte_pos: usize| -> (quadraui::Color, bool, bool) {
        if !code_highlights.is_empty() {
            for h in code_highlights {
                if byte_pos >= h.start_byte && byte_pos < h.end_byte {
                    return (to_quadraui_color(theme.scope_color(&h.scope)), false, false);
                }
            }
            return (code_fg, false, false);
        }
        for span in md_spans {
            if byte_pos >= span.start_byte && byte_pos < span.end_byte {
                return match span.style {
                    MdStyle::Heading(1) => (h1_fg, true, false),
                    MdStyle::Heading(2) => (h2_fg, true, false),
                    MdStyle::Heading(_) => (h3_fg, true, false),
                    MdStyle::Bold => (default_fg, true, false),
                    MdStyle::Italic => (default_fg, false, true),
                    MdStyle::BoldItalic => (default_fg, true, true),
                    MdStyle::Code | MdStyle::CodeBlock => (code_fg, false, false),
                    MdStyle::Link | MdStyle::LinkUrl => (link_fg, false, false),
                    MdStyle::BlockQuote => (h3_fg, false, true),
                    MdStyle::ListBullet => (h1_fg, true, false),
                    MdStyle::HorizontalRule | MdStyle::Image => (link_fg, false, true),
                };
            }
        }
        (default_fg, false, false)
    };

    let mut spans: Vec<quadraui::StyledSpan> = Vec::new();
    let mut byte_pos: usize = 0;
    let mut current_text = String::new();
    let mut current_style: Option<(quadraui::Color, bool, bool)> = None;

    for ch in line_text.chars() {
        let s = style_at(byte_pos);
        match current_style {
            Some(prev) if prev == s => {
                current_text.push(ch);
            }
            _ => {
                if !current_text.is_empty() {
                    let st = current_style.unwrap();
                    spans.push(quadraui::StyledSpan {
                        text: std::mem::take(&mut current_text),
                        fg: Some(st.0),
                        bg: None,
                        bold: st.1,
                        italic: st.2,
                        underline: false,
                    });
                }
                current_text.push(ch);
                current_style = Some(s);
            }
        }
        byte_pos += ch.len_utf8();
    }
    if !current_text.is_empty() {
        let st = current_style.unwrap_or((default_fg, false, false));
        spans.push(quadraui::StyledSpan {
            text: current_text,
            fg: Some(st.0),
            bg: None,
            bold: st.1,
            italic: st.2,
            underline: false,
        });
    }
    quadraui::StyledText { spans }
}

// ─── SignatureHelp ────────────────────────────────────────────────────────────

/// Data needed to render the signature help popup (shown above cursor in insert mode).
#[derive(Debug, Clone)]
pub struct SignatureHelp {
    /// The full signature label, e.g. `fn foo(a: i32, b: &str) -> bool`
    pub label: String,
    /// Byte-offset ranges of each parameter within `label`.
    pub params: Vec<(usize, usize)>,
    /// Index of the currently active parameter (0-based), if known.
    pub active_param: Option<usize>,
    /// Buffer line where the call was started (for positioning above cursor).
    pub anchor_line: usize,
    /// Buffer column of the opening `(`.
    pub anchor_col: usize,
}

/// Convert a `SignatureHelp` + on-screen anchor cell into a fully
/// resolved `quadraui::Tooltip` and its `TooltipLayout`.
///
/// The label is rendered as a single-line styled tooltip: text before
/// and after the active parameter use the theme hover-fg; the active
/// parameter is highlighted in the theme keyword colour.
///
/// Placement is `Top` with fallback `Bottom`. The anchor rectangle is
/// given `width = popup_width` so the primitive's horizontal-centering
/// math aligns the popup's left edge with the cursor cell (matching
/// legacy behavior).
/// `unit_w` / `unit_h` scale cell/row-derived sizes into the caller's
/// coordinate space — see [`hover_popup_to_quadraui_tooltip`]'s doc for why
/// this is enough to make the one adapter serve both backends (#669).
pub fn signature_help_to_quadraui_tooltip(
    sig: &SignatureHelp,
    anchor_x: f32,
    anchor_y: f32,
    viewport: quadraui::Rect,
    theme: &Theme,
    unit_w: f32,
    unit_h: f32,
) -> (quadraui::Tooltip, quadraui::TooltipLayout) {
    let label = &sig.label;
    // Display adds a leading + trailing space inside the border, so
    // `display_len` is `label_chars + 2`.
    let label_chars = label.chars().count();
    let display_len = label_chars + 2;
    // +2 for the two side borders.
    let width = ((display_len + 2) as f32).max(12.0) * unit_w;

    // Build styled spans. The label's active parameter (if any) is
    // highlighted in theme.keyword. Offsets in `sig.params` are byte
    // offsets into `label` — convert to char-based splits.
    let fg = to_q_color(theme.hover_fg);
    let kw = to_q_color(theme.keyword);

    let active_byte_range: Option<(usize, usize)> = sig
        .active_param
        .and_then(|idx| sig.params.get(idx).copied());

    let mut spans: Vec<quadraui::StyledSpan> = Vec::new();
    // Leading space inside the border.
    spans.push(quadraui::StyledSpan::with_fg(" ", fg));
    match active_byte_range {
        Some((start, end)) if start < end && end <= label.len() => {
            let pre = &label[..start];
            let active = &label[start..end];
            let post = &label[end..];
            if !pre.is_empty() {
                spans.push(quadraui::StyledSpan::with_fg(pre, fg));
            }
            spans.push(quadraui::StyledSpan::with_fg(active, kw));
            if !post.is_empty() {
                spans.push(quadraui::StyledSpan::with_fg(post, fg));
            }
        }
        _ => {
            spans.push(quadraui::StyledSpan::with_fg(label, fg));
        }
    }
    // Trailing space inside the border.
    spans.push(quadraui::StyledSpan::with_fg(" ", fg));

    let mut tooltip =
        quadraui_tooltip(quadraui::WidgetId::new("lsp_signature_help"), String::new());
    tooltip.styled_lines = Some(vec![quadraui::StyledText { spans }]);
    tooltip.placement = quadraui::TooltipPlacement::Top;
    // anchor.width = popup width so centering math left-aligns popup
    // with the cursor cell.
    let anchor = quadraui::Rect::new(anchor_x, anchor_y, width, unit_h);
    let measure = quadraui::TooltipMeasure::new(width, unit_h);
    let layout = tooltip.layout(anchor, viewport, measure, 0.0);
    (tooltip, layout)
}

// ─── PickerPanel (unified) ─────────────────────────────────────────────────

/// A single item in the unified picker display.
#[derive(Debug, Clone)]
pub struct PickerPanelItem {
    /// Text shown in the result list.
    pub display: String,
    /// Right-aligned hint (shortcut, line number, etc.).
    pub detail: Option<String>,
    /// Byte positions in `display` that matched the query (for highlight).
    pub match_positions: Vec<usize>,
    /// Tree nesting depth (0 = top-level).
    pub depth: usize,
    /// Whether this item has children (shows expand arrow).
    pub expandable: bool,
    /// Whether this item's children are currently visible.
    pub expanded: bool,
}

/// Data needed to render the unified picker modal.
#[derive(Debug, Clone)]
pub struct PickerPanel {
    /// Title shown in the header bar.
    pub title: String,
    /// Current query typed by the user.
    pub query: String,
    /// Filtered items to display.
    pub items: Vec<PickerPanelItem>,
    /// Index of the currently highlighted item.
    pub selected_idx: usize,
    /// Scroll offset into the filtered list.
    pub scroll_top: usize,
    /// Total number of source items (for the "N/M" counter).
    pub total_count: usize,
    /// Preview lines: (1-based line number, text, is_highlighted).
    /// When `Some`, the picker is rendered in two-pane mode.
    pub preview: Option<Vec<(usize, String, bool)>>,
    /// Scroll offset for the preview pane.
    pub preview_scroll: usize,
}

// ─── PickerGeometry ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct PickerSizing {
    pub min_w: (f32, f32),
    pub min_h: (f32, f32),
    pub left_pane_ratio: f32,
    pub header_h: f32,
    pub line_h: f32,
}

pub const TUI_PICKER_SIZING: PickerSizing = PickerSizing {
    min_w: (55.0, 60.0),
    min_h: (16.0, 18.0),
    left_pane_ratio: 0.35,
    header_h: 4.0,
    line_h: 1.0,
};

pub fn gtk_picker_sizing(line_height: f32) -> PickerSizing {
    PickerSizing {
        min_w: (500.0, 600.0),
        min_h: (350.0, 400.0),
        left_pane_ratio: 0.40,
        header_h: 2.0 * line_height + 2.0,
        line_h: line_height,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PickerGeometry {
    pub popup_x: f32,
    pub popup_y: f32,
    pub popup_w: f32,
    pub popup_h: f32,
    pub left_pane_w: f32,
    pub visible_rows: usize,
}

impl PickerGeometry {
    pub fn compute(
        viewport_w: f32,
        viewport_h: f32,
        has_preview: bool,
        sizing: &PickerSizing,
    ) -> Self {
        let popup_w = if has_preview {
            (viewport_w * 0.8).max(sizing.min_w.1)
        } else {
            (viewport_w * 0.55).max(sizing.min_w.0)
        };
        let popup_h = if has_preview {
            (viewport_h * 0.65).max(sizing.min_h.1)
        } else {
            (viewport_h * 0.60).max(sizing.min_h.0)
        };
        let popup_x = (viewport_w - popup_w) / 2.0;
        let popup_y = (viewport_h - popup_h) / 2.0;
        let left_pane_w = if has_preview {
            popup_w * sizing.left_pane_ratio
        } else {
            0.0
        };
        let results_h = (popup_h - sizing.header_h).max(0.0);
        let visible_rows = (results_h / sizing.line_h) as usize;
        PickerGeometry {
            popup_x,
            popup_y,
            popup_w,
            popup_h,
            left_pane_w,
            visible_rows,
        }
    }
}

// ─── TabSwitcherPanel ─────────────────────────────────────────────────────

/// Data needed to render the tab switcher popup (Ctrl+Tab MRU list).
#[derive(Debug, Clone)]
pub struct TabSwitcherPanel {
    /// MRU-ordered items: (filename, full_path, is_dirty).
    pub items: Vec<(String, String, bool)>,
    /// Index of the currently highlighted item.
    pub selected_idx: usize,
}

/// Convert a `TabSwitcherPanel` into a bordered `quadraui::ListView`.
///
/// Each item carries the filename (with a trailing `●` when dirty)
/// and uses the full path as the right-aligned `detail`. The list is
/// bordered with the title `" Open Tabs "` overlayed on the top
/// border. `scroll_offset` is set so the selected item is always
/// visible inside `max_visible` rows.
pub fn tab_switcher_to_quadraui_list_view(
    ts: &TabSwitcherPanel,
    max_visible: usize,
) -> quadraui::ListView {
    use quadraui::{ListItem, ListView, StyledText, WidgetId};

    let items: Vec<ListItem> = ts
        .items
        .iter()
        .map(|(name, path, dirty)| {
            let label = if *dirty {
                format!("{} ●", name)
            } else {
                name.clone()
            };
            ListItem {
                text: StyledText::plain(label),
                icon: None,
                detail: if path.is_empty() {
                    None
                } else {
                    Some(StyledText::plain(path.clone()))
                },
                decoration: quadraui::Decoration::Normal,
            }
        })
        .collect();

    // Scroll so the selected item is on screen. Window is `max_visible`
    // items tall; scroll forward by enough to keep selected_idx in view.
    let scroll_offset = if ts.selected_idx >= max_visible {
        ts.selected_idx + 1 - max_visible
    } else {
        0
    };

    ListView {
        id: WidgetId::new("tab_switcher"),
        title: Some(StyledText::plain("Open Tabs")),
        items,
        selected_idx: ts.selected_idx,
        scroll_offset,
        has_focus: true,
        bordered: true,
        h_scroll: 0,
        max_content_width: None,
        show_v_scrollbar: false,
    }
}

// ─── QuickfixPanel ────────────────────────────────────────────────────────────

/// Data needed to render the quickfix bottom panel.
#[derive(Debug, Clone)]
pub struct QuickfixPanel {
    /// Formatted display strings: "file.rs:12: line text"
    pub items: Vec<String>,
    /// Currently selected item index.
    pub selected_idx: usize,
    /// Total number of items in the list.
    pub total_items: usize,
    /// Whether the quickfix panel has keyboard focus.
    pub has_focus: bool,
}

/// A single item rendered in the debug sidebar. Used by win-gui;
/// TUI/GTK use `SidebarSystem` with `TreeRow` directly via
/// `populate_dap_sidebar_system()`.
#[derive(Debug, Clone)]
pub struct DebugSidebarItem {
    pub text: String,
    pub indent: u8,
    pub is_selected: bool,
}

// ─── SourceControlData ────────────────────────────────────────────────────────

/// A single file-change item in the Source Control panel.
#[derive(Debug, Clone)]
pub struct ScFileItem {
    pub path: String,
    /// Single-char status label: A / M / D / R / ?
    pub status_char: char,
    pub is_staged: bool,
}

/// A single worktree item in the Source Control panel.
#[derive(Debug, Clone)]
pub struct ScWorktreeItem {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_main: bool,
}

/// A single git log entry in the Source Control panel.
#[derive(Debug, Clone)]
pub struct ScLogItem {
    /// Short (abbreviated) commit hash.
    pub hash: String,
    /// Commit subject line.
    pub message: String,
}

/// Rendering data for the Source Control panel sidebar.
#[derive(Debug, Clone)]
pub struct SourceControlData {
    /// Current git branch name (e.g. "main").
    pub branch: String,
    /// Number of commits ahead of the upstream.
    pub ahead: u32,
    /// Number of commits behind the upstream.
    pub behind: u32,
    /// Staged files (index changes).
    pub staged: Vec<ScFileItem>,
    /// Unstaged / untracked files (working-tree changes).
    pub unstaged: Vec<ScFileItem>,
    /// Git worktrees.
    pub worktrees: Vec<ScWorktreeItem>,
    /// Recent git log entries.
    pub log: Vec<ScLogItem>,
    /// Which sections are expanded: [staged, unstaged, worktrees, log].
    pub sections_expanded: [bool; 4],
    /// Flat selection index.
    pub selected: usize,
    /// Whether the panel currently has keyboard focus.
    pub has_focus: bool,
    /// Commit message being typed in the input row.
    pub commit_message: String,
    /// Byte-offset cursor position within the commit message.
    pub commit_cursor: usize,
    /// True when the commit input row is in edit mode.
    pub commit_input_active: bool,
    /// Which action button is keyboard-focused (0=Commit 1=Push 2=Pull 3=Sync), or None.
    pub button_focused: Option<usize>,
    /// Which action button the mouse is hovering over, or None.
    pub button_hovered: Option<usize>,
    /// Branch picker popup data (None when closed).
    pub branch_picker: Option<BranchPickerData>,
    /// SC help dialog visible.
    pub help_open: bool,
    /// Y coordinate (in native units) where the sections area begins —
    /// the top of `SidebarPanelLayout.content_bounds` from the last paint
    /// (#509). TUI: terminal rows. GTK: pixels. `None` until first paint.
    pub sc_sections_start_y: Option<f32>,
}

/// Data for the branch picker / create popup in the SC panel.
#[derive(Debug, Clone)]
pub struct BranchPickerData {
    pub query: String,
    /// (branch_name, is_current)
    pub results: Vec<(String, bool)>,
    pub selected: usize,
    /// When true, the popup is in "create new branch" mode.
    pub create_mode: bool,
    /// The new branch name being typed (only in create mode).
    pub create_input: String,
}

// ─── ExtSidebarData ───────────────────────────────────────────────────────────

/// A single extension item in the Extensions sidebar.
#[derive(Debug, Clone)]
pub struct ExtSidebarItem {
    pub name: String,
    pub display_name: String,
    pub description: String,
    /// LSP binary name (empty string if none).
    pub lsp_binary: String,
    /// DAP adapter name (empty string if none).
    pub dap_adapter: String,
    /// Number of bundled Lua scripts.
    pub script_count: usize,
    pub installed: bool,
    /// True when a newer version is available in the registry.
    pub update_available: bool,
}

/// Rendering data for the Extensions sidebar panel.
#[derive(Debug, Clone)]
pub struct ExtSidebarData {
    /// Installed extensions (filtered by query).
    pub items_installed: Vec<ExtSidebarItem>,
    /// Available (not yet installed) extensions (filtered by query).
    pub items_available: Vec<ExtSidebarItem>,
    /// Whether each section is expanded: [installed, available].
    pub sections_expanded: [bool; 2],
    /// Flat selection index (installed items first, then available).
    pub selected: usize,
    /// Whether the panel currently has keyboard focus.
    pub has_focus: bool,
    /// Current search query string.
    pub query: String,
    /// Whether the search input is in active edit mode.
    pub input_active: bool,
    /// True while a background registry fetch is in-flight.
    pub fetching: bool,
    /// Vertical scroll offset of the panel content in main-axis units
    /// (cells / pixels). Drives `MultiSectionView::panel_scroll` (#293).
    pub panel_scroll: f32,
}

// ─── ExtPanelData (extension-provided sidebar panels) ────────────────────────

/// Rendering data for a single extension-provided sidebar panel.
#[derive(Debug, Clone)]
pub struct ExtPanelData {
    pub name: String,
    pub title: String,
    pub sections: Vec<ExtPanelSectionData>,
    pub selected: usize,
    pub has_focus: bool,
    pub scroll_top: usize,
    pub input_text: String,
    pub input_active: bool,
    pub help_open: bool,
    pub help_bindings: Vec<(String, String)>,
}

/// A single section within an extension panel.
#[derive(Debug, Clone)]
pub struct ExtPanelSectionData {
    pub name: String,
    pub items: Vec<crate::core::plugin::ExtPanelItem>,
    pub expanded: bool,
}

// ─── PanelHoverPopupData ──────────────────────────────────────────────────────

/// Rendering data for a sidebar panel hover popup (rendered markdown).
#[derive(Debug, Clone)]
pub struct PanelHoverPopupData {
    /// Rendered markdown content.
    pub rendered: crate::core::markdown::MdRendered,
    /// Clickable link regions: (line_idx, start_byte, end_byte, url).
    pub links: Vec<(usize, usize, usize, String)>,
    /// Flat item index being hovered (for positioning relative to panel).
    pub item_index: usize,
    /// The panel this hover belongs to (e.g. "source_control", ext panel name).
    pub panel_name: String,
}

/// Maximum number of sidebar-item hover popup rows shown at once
/// (matches the legacy `MAX_HEIGHT` constant it replaces — no
/// scrolling for this popup, content beyond this is truncated).
pub const PANEL_HOVER_MAX_ROWS: usize = 20;

/// Convert a `PanelHoverPopupData` into a `quadraui::RichTextPopup` for
/// the D6 layout pipeline, mirroring `editor_hover_to_quadraui_rich_text`.
/// The sidebar-item hover is read-only (no scroll, focus, selection, or
/// keyboard-link-nav state), so those fields are fixed defaults.
///
/// Placement is `Below`: callers pass `anchor_y = desired_top_row -
/// 1.0` (one row height) so the popup's top border lands exactly on
/// the row the legacy hand-rolled renderer used.
pub fn panel_hover_to_quadraui_rich_text(
    ph: &PanelHoverPopupData,
    theme: &Theme,
) -> quadraui::RichTextPopup {
    let (q_lines, line_scales) = markdown_rendered_to_quadraui_lines(&ph.rendered, theme);
    let q_links = md_links_to_quadraui_rich_text_links(&ph.links);

    quadraui::RichTextPopup {
        id: quadraui::WidgetId::new("panel_hover"),
        lines: q_lines,
        line_text: ph.rendered.lines.clone(),
        line_scales,
        scroll_top: 0,
        max_visible_rows: PANEL_HOVER_MAX_ROWS,
        has_focus: false,
        selection: None,
        links: q_links,
        focused_link: None,
        placement: quadraui::PopupPlacement::Below,
        padding: 0.0,
        fg: Some(to_quadraui_color(theme.hover_fg)),
        bg: Some(to_quadraui_color(theme.hover_bg)),
    }
}

// ─── AiPanelData ─────────────────────────────────────────────────────────────

/// A single message in the AI conversation history, pre-formatted for rendering.
#[derive(Debug, Clone)]
pub struct AiPanelMessage {
    /// "user" or "assistant"
    pub role: String,
    /// Message text (may be multi-line)
    pub content: String,
}

/// Rendering data for the AI assistant sidebar panel.
#[derive(Debug, Clone)]
pub struct AiPanelData {
    pub messages: Vec<AiPanelMessage>,
    /// Current input being composed.
    pub input: String,
    /// Whether the panel has keyboard focus.
    pub has_focus: bool,
    /// Whether the text input box is in active edit mode.
    pub input_active: bool,
    /// True while waiting for an AI response.
    pub streaming: bool,
    /// Scroll offset into the messages list.
    pub scroll_top: usize,
    /// Cursor position within `input` (char index).
    pub input_cursor: usize,
}

// ─── SettingDef ───────────────────────────────────────────────────────────────

// SettingType, SettingDef, and SETTING_DEFS are defined in settings.rs and
// re-exported at the top of this file for backward compatibility.

/// Always present in `ScreenLayout`; each section may be empty.
#[derive(Debug, Clone)]
pub struct DebugSidebarData {
    pub session_active: bool,
    pub stopped: bool,
    pub variables: Vec<DebugSidebarItem>,
    pub watch: Vec<DebugSidebarItem>,
    pub frames: Vec<DebugSidebarItem>,
    pub breakpoints: Vec<DebugSidebarItem>,
    pub active_section: DebugSidebarSection,
    pub sidebar_selected: usize,
    pub has_focus: bool,
    pub launch_config_name: Option<String>,
    pub debug_output_lines: Vec<String>,
    pub eval_result: Option<String>,
    pub scroll_offsets: [usize; 4],
    pub section_heights: [u16; 4],
}

/// The two bottom panel tabs: Terminal and Debug Output.
#[derive(Debug)]
pub struct BottomPanelTabs {
    /// Which tab is currently active.
    pub active: BottomPanelKind,
    /// Terminal panel data (always built if terminal is open, regardless of active tab).
    pub terminal: Option<TerminalPanel>,
    /// Debug output lines for the Debug Output tab.
    pub output_lines: Vec<String>,
}

// ─── TerminalPanel ────────────────────────────────────────────────────────────

/// Data needed to render the integrated terminal bottom panel.
#[derive(Debug)]
pub struct TerminalPanel {
    /// Rendered cell grid: `rows[content_row][col]` — quadraui cells with all
    /// overlay flags (cursor, selection, find-match) already applied.
    pub rows: Vec<Vec<quadraui::TerminalCell>>,
    /// Number of content rows (excluding toolbar).
    pub content_rows: u16,
    /// Number of columns.
    pub content_cols: u16,
    /// Whether the terminal panel has keyboard focus.
    pub has_focus: bool,
    /// Rows scrolled up into scrollback (0 = live view).
    pub scroll_offset: usize,
    /// Number of scrollback rows stored in the VT100 parser buffer.
    pub scrollback_rows: usize,
    /// Total number of terminal tabs.
    pub tab_count: usize,
    /// Index of the currently active tab.
    pub active_tab: usize,
    /// Whether the inline find bar is open.
    pub find_active: bool,
    /// Current find query string.
    pub find_query: String,
    /// Total number of matches found.
    pub find_match_count: usize,
    /// Index (0-based) of the currently highlighted match.
    pub find_selected_idx: usize,
    /// In split view: cell grid for the LEFT pane (pane[0]).
    /// When `Some`, the main `rows` field represents the RIGHT pane (pane[1]).
    /// `None` in normal (non-split) mode.
    pub split_left_rows: Option<Vec<Vec<quadraui::TerminalCell>>>,
    /// Column count of the left pane in split view.
    pub split_left_cols: u16,
    /// Which pane has keyboard focus in split view: 0 = left, 1 = right.
    pub split_focus: u8,
    /// Whether the panel is currently maximized (fills editor area).
    /// Backends can render a different icon glyph based on this.
    pub maximized: bool,
}

/// Terminal scrollbar thumb position as fractions of track height.
/// Both backends use this for painting and `SurfaceScrollbar` registration.
#[derive(Debug, Clone, Copy)]
pub struct TerminalScrollbarGeom {
    pub thumb_top_frac: f64,
    pub thumb_height_frac: f64,
    pub total_items: usize,
    pub visible_items: usize,
}

/// Returns `None` when there's no scrollback (thumb fills entire track).
pub fn terminal_scrollbar_geometry(
    panel: &TerminalPanel,
    visible_rows: usize,
) -> Option<TerminalScrollbarGeom> {
    if panel.scrollback_rows == 0 {
        return None;
    }
    let total = panel.scrollback_rows + visible_rows;
    let thumb_frac = (visible_rows as f64 / total as f64).max(0.01);
    let max_off = panel.scrollback_rows as f64;
    let frac = if panel.scroll_offset == 0 {
        1.0
    } else {
        1.0 - (panel.scroll_offset as f64 / max_off).min(1.0)
    };
    let thumb_top_frac = frac * (1.0 - thumb_frac);
    Some(TerminalScrollbarGeom {
        thumb_top_frac,
        thumb_height_frac: thumb_frac,
        total_items: total,
        visible_items: visible_rows,
    })
}

/// Pre-built terminal primitives ready for `Backend::draw_terminal`.
/// Both backends call `build_terminal_draw_data` and then just do the
/// backend-specific drawing (clear background, enter frame scope, divider).
pub struct TerminalDrawData {
    pub single: Option<quadraui::Terminal>,
    pub left: Option<quadraui::Terminal>,
    pub right: Option<quadraui::Terminal>,
    pub split: Option<quadraui::TerminalSplitLayout>,
}

pub fn build_terminal_draw_data(
    panel: &TerminalPanel,
    area: quadraui::Rect,
    cell_width: f32,
    cell_height: f32,
    visible_rows: usize,
    sb_width: Option<u16>,
) -> TerminalDrawData {
    let sb = Some(quadraui::TerminalScrollbar {
        total_lines: panel.scrollback_rows + visible_rows,
        visible_lines: visible_rows,
        scroll_offset: panel.scroll_offset,
        inverted: true,
        width: sb_width,
    });
    if let Some(ref left_rows) = panel.split_left_rows {
        let sb_px = sb_width.unwrap_or(0) as f32;
        let split = quadraui::TerminalSplitLayout::new(
            area,
            panel.split_left_cols as usize,
            cell_width,
            cell_height,
            sb_px,
        );
        let left = quadraui::Terminal {
            id: quadraui::WidgetId::new("terminal:left"),
            cells: left_rows.clone(),
            scrollbar: None,
        };
        let right = quadraui::Terminal {
            id: quadraui::WidgetId::new("terminal:right"),
            cells: panel.rows.clone(),
            scrollbar: sb,
        };
        TerminalDrawData {
            single: None,
            left: Some(left),
            right: Some(right),
            split: Some(split),
        }
    } else {
        let term = quadraui::Terminal {
            id: quadraui::WidgetId::new("terminal:pane"),
            cells: panel.rows.clone(),
            scrollbar: sb,
        };
        TerminalDrawData {
            single: Some(term),
            left: None,
            right: None,
            split: None,
        }
    }
}

// ─── Menu bar / debug toolbar ─────────────────────────────────────────────────

/// One item in a menu dropdown.
#[derive(Debug, Clone)]
pub struct MenuItemData {
    /// Display label shown in the dropdown (e.g. "Save").
    pub label: &'static str,
    /// Right-aligned keyboard shortcut hint in Vim mode (e.g. "u" for Undo).
    pub shortcut: &'static str,
    /// Right-aligned keyboard shortcut hint in VSCode mode (e.g. "Ctrl+Z" for Undo).
    /// Empty string means fall back to `shortcut`.
    pub vscode_shortcut: &'static str,
    /// Command string dispatched to the engine when activated (e.g. "w").
    /// Empty string means no action (for separators).
    pub action: &'static str,
    /// Whether this item is currently enabled.
    pub enabled: bool,
    /// If true, render as a horizontal divider line instead of a regular item.
    pub separator: bool,
}

/// One button in the debug toolbar strip.
#[derive(Debug, Clone)]
pub struct DebugButton {
    /// Nerd Font glyph string.
    pub icon: &'static str,
    /// Short label shown next to the icon.
    pub label: &'static str,
    /// Key hint shown in the button (e.g. "F5").
    pub key_hint: &'static str,
    /// Command string passed to `engine.execute_command()` when the button is clicked.
    pub action: &'static str,
    /// Whether this button is currently clickable.
    pub enabled: bool,
}

/// Data for the debug toolbar strip.
#[derive(Debug)]
pub struct DebugToolbarData {
    /// Buttons to render (in order, with a `│` separator after index 3).
    pub buttons: Vec<DebugButton>,
    /// True when a DAP session is active; drives future enabled/greyed-out state.
    pub session_active: bool,
}

/// Build the debug action-button toolbar as a `quadraui::Toolbar` (#510).
/// Button ids come from [`crate::core::engine::DEBUG_BUTTON_IDS`] so
/// click dispatch can map the hit-test result back to a button index and
/// action string. A `ToolbarButton::Separator` is inserted between the
/// Restart (index 3) and Step Over (index 4) buttons.
///
/// `enabled` state follows the per-button DAP rules:
/// - Continue / Step Over / Step Into / Step Out: `dap_session_active && dap_stopped_thread.is_some()`
/// - Pause: `dap_session_active && dap_stopped_thread.is_none()`
/// - Stop / Restart: `dap_session_active`
///
/// Both backends call this and hand the result to `Backend::draw_toolbar`.
pub fn debug_toolbar(engine: &Engine) -> quadraui::Toolbar {
    use crate::core::engine::DEBUG_BUTTON_IDS;
    use crate::icons;
    use quadraui::{Toolbar, ToolbarButton, WidgetId};

    let session = engine.dap_session_active;
    let stopped = engine.dap_stopped_thread.is_some();

    let action = |idx: usize, label: &str, icon: &str, key_hint: Option<&str>, enabled: bool| {
        ToolbarButton::Action {
            id: WidgetId::new(DEBUG_BUTTON_IDS[idx]),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            key_hint: key_hint.map(|s| s.to_string()),
            enabled,
            is_active: false,
            tooltip: String::new(),
        }
    };

    Toolbar {
        id: WidgetId::new("debug:toolbar"),
        bg: None,
        focused_index: None,
        buttons: vec![
            // 0: Continue — enabled when session active and stopped
            action(
                0,
                "Continue",
                icons::DBG_CONTINUE.fallback,
                Some("F5"),
                session && stopped,
            ),
            // 1: Pause — enabled when session active and running (not stopped)
            action(
                1,
                "Pause",
                icons::DBG_PAUSE.fallback,
                Some("F6"),
                session && !stopped,
            ),
            // 2: Stop — enabled when session active
            action(2, "Stop", icons::DBG_STOP.fallback, Some("⇧F5"), session),
            // 3: Restart — enabled when session active
            action(
                3,
                "Restart",
                icons::DBG_RESTART.fallback,
                Some("^⇧F5"),
                session,
            ),
            // Separator between restart and step controls
            ToolbarButton::Separator,
            // 4: Step Over — enabled when session active and stopped
            action(
                4,
                "Step Over",
                icons::DBG_STEP_OVER.fallback,
                Some("F10"),
                session && stopped,
            ),
            // 5: Step Into — enabled when session active and stopped
            action(
                5,
                "Step Into",
                icons::DBG_RESTART.fallback,
                Some("F11"),
                session && stopped,
            ),
            // 6: Step Out — enabled when session active and stopped
            action(
                6,
                "Step Out",
                icons::DBG_STEP_OUT.fallback,
                Some("⇧F11"),
                session && stopped,
            ),
        ],
    }
}

/// Draw the debug action-button toolbar through backend `b` and cache its
/// layout on `engine` for click/hover dispatch (#510). Both backends call
/// this inside their frame scope; the only per-backend input is `rect`
/// (cell units for TUI, pixels for GTK). Mouse hover → `debug_button_hovered`,
/// visual press → `debug_button_pressed`, both read from the engine.
pub fn draw_debug_toolbar(b: &mut dyn quadraui::Backend, engine: &Engine, rect: quadraui::Rect) {
    use crate::core::engine::Engine;
    let bar = debug_toolbar(engine);
    let hovered = engine
        .debug_button_hovered
        .and_then(Engine::debug_button_id);
    let pressed = engine
        .debug_button_pressed
        .and_then(Engine::debug_button_id);
    let layout = b.draw_toolbar(rect, &bar, hovered.as_ref(), pressed.as_ref());
    engine.debug_toolbar_layout.replace(Some(layout));
}

/// Build two `StatusBar` rows for the debug sidebar chrome:
/// row 0 = title ("DEBUG | config_name"), row 1 = action button (Continue/Stop/Start).
pub fn debug_sidebar_chrome_to_status_bars(
    sidebar: &DebugSidebarData,
    theme: &Theme,
) -> (quadraui::StatusBar, quadraui::StatusBar) {
    let bg = to_quadraui_color(theme.status_bg);
    let fg = to_quadraui_color(theme.status_fg);
    let green = to_quadraui_color(theme.git_added);
    let red = to_quadraui_color(theme.diagnostic_error);

    let cfg_name = sidebar.launch_config_name.as_deref().unwrap_or("no config");
    let title = quadraui::StatusBar {
        id: quadraui::WidgetId::new("debug_sidebar_title"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: format!("  {} DEBUG  |  {cfg_name}", icons::DEBUG.s()),
            fg,
            bg,
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    };

    let action_id = Some(quadraui::WidgetId::new("debug_sidebar:action"));
    let (icon, label, icon_fg) = if sidebar.session_active && sidebar.stopped {
        (icons::DBG_PLAY.s(), "  Continue", green)
    } else if sidebar.session_active {
        (icons::DBG_STOP_ALT.s(), "  Stop", red)
    } else {
        (icons::DBG_PLAY.s(), "  Start Debugging", green)
    };
    let action = quadraui::StatusBar {
        id: quadraui::WidgetId::new("debug_sidebar_action"),
        left_segments: vec![
            quadraui::StatusBarSegment {
                text: icon.to_string(),
                fg: icon_fg,
                bg,
                bold: false,
                action_id: action_id.clone(),
            },
            quadraui::StatusBarSegment {
                text: label.to_string(),
                fg,
                bg,
                bold: false,
                action_id,
            },
        ],
        right_segments: Vec::new(),
    };

    (title, action)
}

/// Returns `true` if `id` matches the debug sidebar action button.
pub fn is_debug_sidebar_action(id: &quadraui::WidgetId) -> bool {
    id.as_str() == "debug_sidebar:action"
}

/// `action_id` for each inline window-control button drawn by
/// [`window_controls_status_bar`]. Shared with the GTK click handler so the
/// two sides can't drift.
pub const WINDOW_MINIMIZE_ACTION: &str = "window:minimize";
pub const WINDOW_MAXIMIZE_ACTION: &str = "window:maximize";
pub const WINDOW_CLOSE_ACTION: &str = "window:close";

/// Build the inline minimize/maximize/close window-control buttons for the
/// GTK client-side titlebar (#552).
///
/// quadraui's `run_with_shell` GTK runner creates an undecorated-chrome-free
/// window with no native titlebar hosting (single-DA architecture, #217) —
/// GTK draws its own CSD-style controls at the right edge of the menu-bar
/// row using the same `StatusBar` primitive already used for the debug
/// sidebar's action row, so the click hit-testing reuses the existing
/// `StatusBarHit::Segment` mechanism rather than any new backend API.
///
/// TUI has no window-chrome equivalent (a terminal has no window to
/// minimize/maximize) — this is GTK-only, called from `src/gtk/mod.rs`.
pub fn window_controls_status_bar(theme: &Theme, maximized: bool) -> quadraui::StatusBar {
    let bg = to_quadraui_color(theme.tab_bar_bg);
    // `tab_inactive_fg` — NOT `status_fg` — pairs with `tab_bar_bg` by theme
    // design (it's what `draw_menu_bar` already uses for the File/Edit/...
    // labels painted immediately to the left, against this exact
    // background). `status_fg` is paired with `status_bg` (the bottom
    // status line's own background) instead; at least one shipped theme
    // (`vs_light`: `tab_bar_bg` #ececec, `status_fg` #ffffff) renders
    // near-invisible white-on-near-white glyphs with that mismatched
    // pairing — a real, reproducible cause of the #552 round-2 "buttons
    // render with zero visible pixels" report.
    let fg = to_quadraui_color(theme.tab_inactive_fg);
    let maximize_icon = if maximized {
        icons::WINDOW_RESTORE.s()
    } else {
        icons::WINDOW_MAXIMIZE.s()
    };
    let seg = |text: String, action: &str| quadraui::StatusBarSegment {
        text,
        fg,
        bg,
        bold: false,
        action_id: Some(quadraui::WidgetId::new(action)),
    };
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("window_controls"),
        left_segments: Vec::new(),
        right_segments: vec![
            seg(
                format!("  {}  ", icons::WINDOW_MINIMIZE.s()),
                WINDOW_MINIMIZE_ACTION,
            ),
            seg(format!("  {maximize_icon}  "), WINDOW_MAXIMIZE_ACTION),
            seg(
                format!("  {}  ", icons::WINDOW_CLOSE.s()),
                WINDOW_CLOSE_ACTION,
            ),
        ],
    }
}

/// Build a `TextDisplay` for the debug output panel.
pub fn debug_output_to_text_display(
    output_lines: &[String],
    scroll_offset: usize,
    auto_scroll: bool,
) -> quadraui::TextDisplay {
    let lines: Vec<quadraui::TextDisplayLine> = output_lines
        .iter()
        .map(|line| quadraui::TextDisplayLine {
            spans: vec![quadraui::StyledSpan {
                text: format!("  {line}"),
                fg: None,
                bg: None,
                bold: false,
                italic: false,
                underline: false,
            }],
            decoration: quadraui::Decoration::Normal,
            timestamp: None,
        })
        .collect();

    quadraui::TextDisplay {
        id: quadraui::WidgetId::new("debug_output"),
        lines,
        scroll_offset,
        auto_scroll,
        max_lines: 0,
        has_focus: false,
        title: None,
        show_scrollbar: true,
    }
}

// ─── Static menu structure ────────────────────────────────────────────────────

/// Static description of every top-level menu and its items.
/// Layout: (menu_name, alt_key_char, items).
/// Used by both backends to render the menu bar and by the engine to dispatch actions.
pub static MENU_STRUCTURE: &[(&str, char, &[MenuItemData])] = &[
    (
        "File",
        'f',
        &[
            MenuItemData {
                label: "New Tab",
                shortcut: "Ctrl+T",
                vscode_shortcut: "",
                action: "tabnew",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Open File…",
                shortcut: "",
                vscode_shortcut: "",
                action: "open_file_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Open Folder…",
                shortcut: "",
                vscode_shortcut: "",
                action: "open_folder_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Open Recent…",
                shortcut: "",
                vscode_shortcut: "",
                action: "openrecent",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Open Workspace From File…",
                shortcut: "",
                vscode_shortcut: "",
                action: "open_workspace_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Save Workspace As…",
                shortcut: "",
                vscode_shortcut: "",
                action: "save_workspace_as_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Save",
                shortcut: "Ctrl+S",
                vscode_shortcut: "",
                action: "w",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Save As",
                shortcut: "",
                vscode_shortcut: "",
                action: "saveas",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Quit",
                shortcut: "",
                vscode_shortcut: "Ctrl+Q",
                action: "quit_menu",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Edit",
        'e',
        &[
            MenuItemData {
                label: "Undo",
                shortcut: "u",
                vscode_shortcut: "Ctrl+Z",
                action: "undo",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Redo",
                shortcut: "Ctrl+R",
                vscode_shortcut: "Ctrl+Y",
                action: "redo",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Cut",
                shortcut: "",
                vscode_shortcut: "Ctrl+X",
                action: "cut",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Copy",
                shortcut: "",
                vscode_shortcut: "Ctrl+C",
                action: "clipboard_copy",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Paste",
                shortcut: "",
                vscode_shortcut: "Ctrl+V",
                action: "paste",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Find",
                shortcut: "Ctrl+F",
                vscode_shortcut: "",
                action: "find",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Replace",
                shortcut: "",
                vscode_shortcut: "Ctrl+H",
                action: "replace",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "View",
        'v',
        &[
            MenuItemData {
                label: "Toggle Sidebar",
                shortcut: "Ctrl+B",
                vscode_shortcut: "",
                action: "sidebar",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Toggle Terminal",
                shortcut: "Ctrl+T",
                vscode_shortcut: "",
                action: "terminal",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Zoom In",
                shortcut: "Ctrl++",
                vscode_shortcut: "",
                action: "zoomin",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Zoom Out",
                shortcut: "Ctrl+-",
                vscode_shortcut: "",
                action: "zoomout",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Command Palette",
                shortcut: "Ctrl+Shift+P",
                vscode_shortcut: "",
                action: "palette",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Split Editor Right",
                shortcut: "Ctrl+\\",
                vscode_shortcut: "",
                action: "EditorGroupSplit",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Split Editor Down",
                shortcut: "Ctrl-W E",
                vscode_shortcut: "",
                action: "EditorGroupSplitDown",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Close Editor Group",
                shortcut: "",
                vscode_shortcut: "",
                action: "EditorGroupClose",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Word Wrap",
                shortcut: "",
                vscode_shortcut: "Alt+Z",
                action: "set_wrap_toggle",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Go",
        'g',
        &[
            MenuItemData {
                label: "Go to File",
                shortcut: "Ctrl+P",
                vscode_shortcut: "",
                action: "fuzzy",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Go to Line",
                shortcut: "",
                vscode_shortcut: "",
                action: "goto",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Go to Definition",
                shortcut: "gd",
                vscode_shortcut: "F12",
                action: "def",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Find References",
                shortcut: "gr",
                vscode_shortcut: "Shift+F12",
                action: "refs",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Back",
                shortcut: "Ctrl+O",
                vscode_shortcut: "",
                action: "back",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Forward",
                shortcut: "Ctrl+I",
                vscode_shortcut: "",
                action: "fwd",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Run",
        'r',
        &[
            MenuItemData {
                label: "Start Debugging",
                shortcut: "F5",
                vscode_shortcut: "",
                action: "debug",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Continue",
                shortcut: "F5",
                vscode_shortcut: "",
                action: "continue",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Pause",
                shortcut: "F6",
                vscode_shortcut: "",
                action: "pause",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Stop",
                shortcut: "Shift+F5",
                vscode_shortcut: "",
                action: "stop",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Step Over",
                shortcut: "F10",
                vscode_shortcut: "",
                action: "stepover",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Step Into",
                shortcut: "F11",
                vscode_shortcut: "",
                action: "stepin",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Step Out",
                shortcut: "Shift+F11",
                vscode_shortcut: "",
                action: "stepout",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Toggle Breakpoint",
                shortcut: "F9",
                vscode_shortcut: "",
                action: "brkpt",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Terminal",
        't',
        &[
            MenuItemData {
                label: "New Terminal",
                shortcut: "",
                vscode_shortcut: "",
                action: "terminal",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Close Terminal",
                shortcut: "",
                vscode_shortcut: "",
                action: "termkill",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Help",
        'h',
        &[
            MenuItemData {
                label: "Key Bindings",
                shortcut: "",
                vscode_shortcut: "",
                action: "Keybindings",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "About",
                shortcut: "",
                vscode_shortcut: "",
                action: "about",
                enabled: true,
                separator: false,
            },
        ],
    ),
];

/// Build `Vec<MenuDef>` from `MENU_STRUCTURE` for `quadraui::MenuSystem`.
/// `is_vscode_mode` selects which shortcut variant to display.
pub fn build_menu_defs(is_vscode_mode: bool) -> Vec<quadraui::MenuDef> {
    MENU_STRUCTURE
        .iter()
        .map(|(name, _alt, items)| quadraui::MenuDef {
            id: quadraui::WidgetId::new(*name),
            label: format!("&{name}"),
            disabled: false,
            items: items
                .iter()
                .map(|item| {
                    if item.separator {
                        return quadraui::ContextMenuItem::default();
                    }
                    let shortcut = if is_vscode_mode && !item.vscode_shortcut.is_empty() {
                        item.vscode_shortcut
                    } else {
                        item.shortcut
                    };
                    quadraui::ContextMenuItem {
                        id: Some(quadraui::WidgetId::new(item.action)),
                        label: quadraui::StyledText::plain(item.label.to_string()),
                        detail: if shortcut.is_empty() {
                            None
                        } else {
                            Some(quadraui::StyledText::plain(shortcut.to_string()))
                        },
                        disabled: !item.enabled,
                        ..Default::default()
                    }
                })
                .collect(),
        })
        .collect()
}

/// Static debug toolbar button definitions.
/// Icons use the Unicode fallback glyphs (▶ ⏸ ⏹ ↻ etc.) which render
/// correctly in both TUI (any font) and GTK (no Nerd Font subset needed).
pub static DEBUG_BUTTONS: &[DebugButton] = &[
    DebugButton {
        icon: icons::DBG_CONTINUE.fallback,
        label: "Continue",
        key_hint: "F5",
        action: "continue",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_PAUSE.fallback,
        label: "Pause",
        key_hint: "F6",
        action: "pause",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_STOP.fallback,
        label: "Stop",
        key_hint: "Shift+F5",
        action: "stop",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_RESTART.fallback,
        label: "Restart",
        key_hint: "Ctrl+Shift+F5",
        action: "restart",
        enabled: true,
    },
    // separator goes here (rendered between index 3 and 4)
    DebugButton {
        icon: icons::DBG_STEP_OVER.fallback,
        label: "Step Over",
        key_hint: "F10",
        action: "stepover",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_RESTART.fallback,
        label: "Step Into",
        key_hint: "F11",
        action: "stepin",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_STEP_OUT.fallback,
        label: "Step Out",
        key_hint: "Shift+F11",
        action: "stepout",
        enabled: true,
    },
];

// ─── Backend Parity Harness ───────────────────────────────────────────────────

/// A UI element that a backend is expected to render from a [`ScreenLayout`].
/// Used by the parity harness to verify all three backends handle the same set
/// of elements.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UiElement {
    /// Menu bar strip (File / Edit / View / …).
    MenuBar,
    /// Open menu dropdown overlay.
    MenuDropdown,
    /// Single-group tab bar (uses `ScreenLayout.tab_bar`).
    TabBar,
    /// Per-group tab bar in a multi-group split.
    GroupTabBar { group_idx: usize },
    /// Group divider lines between editor groups.
    GroupDividers,
    /// Breadcrumb bar for a group.
    Breadcrumbs { group_idx: usize },
    /// An editor window/pane.
    EditorWindow { window_idx: usize },
    /// Per-window status line (Vim-style).
    WindowStatusLine { window_idx: usize },
    /// Global status bar (when per-window status lines are off).
    GlobalStatusBar,
    /// Separated status line (above terminal panel).
    SeparatedStatusLine,
    /// Command line (always present).
    CommandLine,
    /// Completion popup (autocomplete).
    CompletionPopup,
    /// Hover popup (LSP documentation).
    HoverPopup,
    /// Rich editor hover popup (markdown, triggered by `gh` or mouse dwell).
    EditorHoverPopup,
    /// Signature help popup (function parameter hints).
    SignatureHelp,
    /// Wildmenu bar (Tab completion in command mode).
    Wildmenu,
    /// Quickfix bottom panel.
    QuickfixPanel,
    /// Debug toolbar strip.
    DebugToolbar,
    /// Terminal panel (bottom).
    TerminalPanel,
    /// Unified picker modal (fuzzy finder / command palette).
    PickerPopup,
    /// Tab switcher popup (Ctrl+Tab MRU list).
    TabSwitcher,
    /// Context menu popup (right-click).
    ContextMenu,
    /// Modal dialog popup.
    Dialog,
    /// Diff peek popup (inline git hunk preview).
    DiffPeekPopup,
    /// Panel hover popup (sidebar item hover).
    PanelHoverPopup,
    /// Tab hover tooltip.
    TabTooltip,
    /// Diff toolbar (change navigation buttons in tab bar).
    DiffToolbar,
    /// Activity bar (sidebar icon strip) — built by `render::build_activity_bar()` and
    /// painted by `quadraui::{tui,gtk}::draw_activity_bar`; not stored in `ScreenLayout`.
    ActivityBar,
    /// Sidebar panel content — rendered by backends from ScreenLayout sidebar data.
    Sidebar,
}

// ─── Phase 2c: Action / click-handler parity ────────────────────────────────

/// A user-triggered action that each backend must handle.
/// This is the **source of truth** for click/mouse/interaction parity.
///
/// Each variant documents: the trigger, the correct engine method to call,
/// and any draw-order requirements.  Backends that are missing a handler
/// will fail the parity test.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UiAction {
    // ── Explorer interactions ─────────────────────────────────────────
    /// Single-click on a file in the explorer tree.
    /// Must call: `engine.open_file_preview(&path)`
    ExplorerSingleClickFile,
    /// Double-click on a file in the explorer tree.
    /// Must call: `engine.open_file_in_tab(&path)`
    ExplorerDoubleClickFile,
    /// Enter/Return key on a file in the explorer.
    /// Must call: `engine.open_file_in_tab(&path)`
    ExplorerEnterOnFile,
    /// Right-click on explorer item → open context menu.
    /// Must call: `engine.open_explorer_context_menu(path, is_dir, x, y)`
    ExplorerRightClick,

    // ── Context menu ─────────────────────────────────────────────────
    /// Click inside an open context menu → select item and execute.
    /// Must call: `engine.context_menu_confirm()` then dispatch action.
    /// Must be checked BEFORE explorer/editor click handlers.
    ContextMenuClickInside,
    /// Click outside an open context menu → dismiss.
    /// Must call: `engine.close_context_menu()`
    ContextMenuClickOutside,

    // ── Tab bar ──────────────────────────────────────────────────────
    /// Click on a tab → switch to it.
    /// Must call: `engine.goto_tab(idx)`
    TabClick,
    /// Click on tab close button.
    /// Must call: `engine.goto_tab(idx)` then `engine.close_tab()`
    TabCloseClick,
    /// Right-click on tab → open tab context menu.
    /// Must call: `engine.open_tab_context_menu(group_id, tab_idx, x, y)`
    TabRightClick,
    /// Drag a tab → reorder or move between groups.
    /// Handled by `TabGroupController::handle_tab_drag_start/move/drop`.
    TabDragDrop,

    // ── Editor ───────────────────────────────────────────────────────
    /// Right-click in editor → open editor context menu.
    /// Must call: `engine.open_editor_context_menu(x, y)`
    EditorRightClick,
    /// Double-click in editor → word select.
    /// Must call: `engine.mouse_double_click(wid, line, col)`
    EditorDoubleClick,
    /// Scroll wheel in editor → scroll viewport.
    EditorScroll,

    // ── Popup interactions ───────────────────────────────────────────
    /// Click on editor hover popup → focus it.
    /// Must call: `engine.editor_hover_focus()`
    EditorHoverClick,
    /// Click outside editor hover popup → dismiss.
    /// Must call: `engine.dismiss_editor_hover()`
    EditorHoverDismiss,
    /// Scroll wheel on editor hover popup → scroll content.
    /// Must call: `engine.editor_hover_scroll(delta)`
    EditorHoverScroll,
    /// Click on debug toolbar button → execute command.
    /// Must call: `engine.execute_command(&btn.action)`
    DebugToolbarButtonClick,

    // ── Terminal ─────────────────────────────────────────────────────
    /// Click terminal split button.
    /// Must call: `engine.terminal_toggle_split(cols, rows)`
    TerminalSplitButton,
    /// Click terminal add (+) button.
    /// Must call: `engine.terminal_new_tab(cols, rows)`
    TerminalAddButton,
    /// Click terminal close (×) button.
    /// Must call: `engine.terminal_close_active_tab()`
    TerminalCloseButton,
    /// Click terminal maximize (□) button.
    /// Must call: `engine.toggle_terminal_maximize(target_rows)`
    /// followed by `engine.terminal_resize(cols, engine.session.terminal_panel_rows)`.
    TerminalMaximizeButton,
    /// Click in split terminal pane → switch focus.
    /// Must set: `engine.terminal_active = 0 or 1`
    TerminalSplitPaneClick,

    // ── Activity bar ─────────────────────────────────────────────────
    /// Click on activity bar icon → toggle sidebar panel.
    ActivityBarClick,
    /// Click on settings gear icon → open settings panel.
    ActivityBarSettingsClick,

    // ── Draw order requirements ──────────────────────────────────────
    /// Context menu must be drawn AFTER sidebar (higher z-order).
    DrawOrderContextMenuAboveSidebar,
    /// Dialog must be drawn AFTER context menu and sidebar.
    DrawOrderDialogOnTop,
    /// Menu dropdown must be drawn AFTER sidebar.
    DrawOrderMenuDropdownAboveSidebar,
}

/// Return the full set of [`UiAction`]s that every backend must handle.
/// This is the canonical contract — if a backend doesn't handle one of these,
/// users will experience broken interactions.
pub fn all_required_ui_actions() -> Vec<UiAction> {
    vec![
        UiAction::ExplorerSingleClickFile,
        UiAction::ExplorerDoubleClickFile,
        UiAction::ExplorerEnterOnFile,
        UiAction::ExplorerRightClick,
        UiAction::ContextMenuClickInside,
        UiAction::ContextMenuClickOutside,
        UiAction::TabClick,
        UiAction::TabCloseClick,
        UiAction::TabRightClick,
        UiAction::TabDragDrop,
        UiAction::EditorRightClick,
        UiAction::EditorDoubleClick,
        UiAction::EditorScroll,
        UiAction::EditorHoverClick,
        UiAction::EditorHoverDismiss,
        UiAction::EditorHoverScroll,
        UiAction::DebugToolbarButtonClick,
        UiAction::TerminalSplitButton,
        UiAction::TerminalAddButton,
        UiAction::TerminalCloseButton,
        UiAction::TerminalMaximizeButton,
        UiAction::TerminalSplitPaneClick,
        UiAction::ActivityBarClick,
        UiAction::ActivityBarSettingsClick,
        UiAction::DrawOrderContextMenuAboveSidebar,
        UiAction::DrawOrderDialogOnTop,
        UiAction::DrawOrderMenuDropdownAboveSidebar,
    ]
}

/// Collect the [`UiAction`]s that the **TUI** backend handles.
/// This is the reference implementation — all actions should be present.
pub fn collect_ui_actions_tui() -> Vec<UiAction> {
    // TUI is the reference backend — it handles all actions.
    // Each entry below is verified by the corresponding code location:
    vec![
        // mouse.rs:1914 — open_file_preview for single click
        UiAction::ExplorerSingleClickFile,
        // mouse.rs:1913 — open_file_in_tab for double click
        UiAction::ExplorerDoubleClickFile,
        // mod.rs key handler — open_file_in_tab for Enter
        UiAction::ExplorerEnterOnFile,
        // mouse.rs:898 — open_explorer_context_menu
        UiAction::ExplorerRightClick,
        // mouse.rs:984-1036 — context_menu click inside/outside
        UiAction::ContextMenuClickInside,
        UiAction::ContextMenuClickOutside,
        // mouse.rs tab click handlers
        UiAction::TabClick,
        UiAction::TabCloseClick,
        UiAction::TabRightClick,
        UiAction::TabDragDrop,
        // mouse.rs:977 — open_editor_context_menu
        UiAction::EditorRightClick,
        // mouse.rs — mouse_double_click
        UiAction::EditorDoubleClick,
        UiAction::EditorScroll,
        // mouse.rs — editor_hover_focus, dismiss, scroll
        UiAction::EditorHoverClick,
        UiAction::EditorHoverDismiss,
        UiAction::EditorHoverScroll,
        // mouse.rs — debug toolbar button handling
        UiAction::DebugToolbarButtonClick,
        // mouse.rs:1639 — terminal_toggle_split
        UiAction::TerminalSplitButton,
        // mouse.rs — terminal_new_tab
        UiAction::TerminalAddButton,
        // mouse.rs — terminal_close_active_tab
        UiAction::TerminalCloseButton,
        // mouse.rs — toggle_terminal_maximize button on toolbar
        UiAction::TerminalMaximizeButton,
        // mouse.rs:1650 — terminal split pane click
        UiAction::TerminalSplitPaneClick,
        // panels.rs — activity bar icon click
        UiAction::ActivityBarClick,
        UiAction::ActivityBarSettingsClick,
        // render_impl.rs — draw order: popups after terminal, picker on top
        UiAction::DrawOrderContextMenuAboveSidebar,
        UiAction::DrawOrderDialogOnTop,
        UiAction::DrawOrderMenuDropdownAboveSidebar,
    ]
}

/// Collect the [`UiAction`]s that the **Win-GUI** backend handles.
/// Update this list as handlers are added to `src/win_gui/`.
pub fn collect_ui_actions_wingui() -> Vec<UiAction> {
    vec![
        // mod.rs:2253 — open_file_preview for single click
        UiAction::ExplorerSingleClickFile,
        // mod.rs:2945 — open_file_in_tab for double click
        UiAction::ExplorerDoubleClickFile,
        // mod.rs:1535 — open_file_in_tab for Enter/Right/l
        UiAction::ExplorerEnterOnFile,
        // mod.rs:3015 — open_explorer_context_menu
        UiAction::ExplorerRightClick,
        // mod.rs:2331-2416 — context menu click inside/outside
        UiAction::ContextMenuClickInside,
        UiAction::ContextMenuClickOutside,
        // mod.rs:2420-2440 — tab click + close
        UiAction::TabClick,
        UiAction::TabCloseClick,
        // mod.rs:2981 — open_tab_context_menu
        UiAction::TabRightClick,
        // mod.rs — tab drag begin/drop
        UiAction::TabDragDrop,
        // mod.rs:3037 — open_editor_context_menu
        UiAction::EditorRightClick,
        // mod.rs:2955 — mouse_double_click
        UiAction::EditorDoubleClick,
        // mod.rs:3043+ — scroll handler
        UiAction::EditorScroll,
        // mod.rs — editor_hover_focus, dismiss_editor_hover, editor_hover_scroll
        UiAction::EditorHoverClick,
        UiAction::EditorHoverDismiss,
        UiAction::EditorHoverScroll,
        // mod.rs — debug toolbar button execute_command
        UiAction::DebugToolbarButtonClick,
        // mod.rs — terminal_toggle_split
        UiAction::TerminalSplitButton,
        // mod.rs — terminal_new_tab
        UiAction::TerminalAddButton,
        // mod.rs — terminal_close_active_tab
        UiAction::TerminalCloseButton,
        // mod.rs — toggle_terminal_maximize button on toolbar
        UiAction::TerminalMaximizeButton,
        // mod.rs — terminal_active = 0/1
        UiAction::TerminalSplitPaneClick,
        // mod.rs — sidebar panel toggle
        UiAction::ActivityBarClick,
        UiAction::ActivityBarSettingsClick,
        // on_paint draw order: draw_frame → sidebar → context menu → dialog → notifications
        UiAction::DrawOrderContextMenuAboveSidebar,
        UiAction::DrawOrderDialogOnTop,
        UiAction::DrawOrderMenuDropdownAboveSidebar,
    ]
}

/// Walk a [`ScreenLayout`] and collect every [`UiElement`] that a backend is
/// expected to render.  This is the **source of truth** for the parity harness.
pub fn collect_expected_ui_elements(layout: &ScreenLayout) -> Vec<UiElement> {
    let mut elems = Vec::new();

    // Menu bar
    if layout.menu_bar_visible {
        elems.push(UiElement::MenuBar);
        if layout.menu_dropdown_open {
            elems.push(UiElement::MenuDropdown);
        }
    }

    // Tab bar(s)
    if layout.editor_group_split.is_some() {
        for (i, _gtb) in layout.group_tab_bars.iter().enumerate() {
            elems.push(UiElement::GroupTabBar { group_idx: i });
        }
        elems.push(UiElement::GroupDividers);
    } else {
        elems.push(UiElement::TabBar);
    }

    // Diff toolbar (single-group)
    if layout.diff_toolbar.is_some() {
        elems.push(UiElement::DiffToolbar);
    }
    // Diff toolbar (per-group)
    if layout.editor_group_split.is_some() {
        for gtb in &layout.group_tab_bars {
            if gtb.diff_toolbar.is_some() {
                elems.push(UiElement::DiffToolbar);
                break; // one element is enough to flag presence
            }
        }
    }

    // Breadcrumbs
    for (i, bc) in layout.breadcrumbs.iter().enumerate() {
        if !bc.segments.is_empty() {
            elems.push(UiElement::Breadcrumbs { group_idx: i });
        }
    }

    // Editor windows + per-window status lines
    for (i, rw) in layout.windows.iter().enumerate() {
        elems.push(UiElement::EditorWindow { window_idx: i });
        if rw.status_line.is_some() {
            elems.push(UiElement::WindowStatusLine { window_idx: i });
        }
    }

    // Global status bar (only when per-window status lines are off)
    let any_per_window_status = layout.windows.iter().any(|w| w.status_line.is_some());
    if !any_per_window_status {
        elems.push(UiElement::GlobalStatusBar);
    }

    // Separated status line (above terminal)
    if layout.separated_status_line.is_some() {
        elems.push(UiElement::SeparatedStatusLine);
    }

    // Command line (always rendered)
    elems.push(UiElement::CommandLine);

    // Popups & overlays (conditional)
    if layout.completion.is_some() {
        elems.push(UiElement::CompletionPopup);
    }
    if layout.hover.is_some() {
        elems.push(UiElement::HoverPopup);
    }
    if layout.editor_hover.is_some() {
        elems.push(UiElement::EditorHoverPopup);
    }
    if layout.signature_help.is_some() {
        elems.push(UiElement::SignatureHelp);
    }
    if layout.wildmenu.is_some() {
        elems.push(UiElement::Wildmenu);
    }
    if layout.quickfix.is_some() {
        elems.push(UiElement::QuickfixPanel);
    }
    if layout.debug_toolbar.is_some() {
        elems.push(UiElement::DebugToolbar);
    }
    if layout.bottom_tabs.terminal.is_some() {
        elems.push(UiElement::TerminalPanel);
    }
    if layout.picker.is_some() {
        elems.push(UiElement::PickerPopup);
    }
    if layout.tab_switcher.is_some() {
        elems.push(UiElement::TabSwitcher);
    }
    if layout.context_menu.is_some() {
        elems.push(UiElement::ContextMenu);
    }
    if layout.dialog.is_some() {
        elems.push(UiElement::Dialog);
    }
    if layout.diff_peek.is_some() {
        elems.push(UiElement::DiffPeekPopup);
    }
    if layout.panel_hover.is_some() {
        elems.push(UiElement::PanelHoverPopup);
    }
    if layout.tab_tooltip.is_some() {
        elems.push(UiElement::TabTooltip);
    }

    // Activity bar + sidebar — always expected (backends render these from
    // engine state / ScreenLayout sidebar fields).
    elems.push(UiElement::ActivityBar);
    if layout.source_control.is_some()
        || layout.ext_sidebar.is_some()
        || layout.ai_panel.is_some()
        || layout.ext_panel.is_some()
        || layout.debug_sidebar.session_active
    {
        elems.push(UiElement::Sidebar);
    }

    elems.sort();
    elems
}

/// Simulate the Win-GUI backend's `draw_frame()` + `on_paint()` branching logic
/// to collect which [`UiElement`]s it would render.  This mirrors the actual
/// rendering code in `src/win_gui/draw.rs` without requiring Direct2D.
pub fn collect_ui_elements_wingui(layout: &ScreenLayout) -> Vec<UiElement> {
    let mut elems = Vec::new();

    // draw_frame(): menu bar
    if layout.menu_bar_visible {
        elems.push(UiElement::MenuBar);
    }

    // draw_frame(): tab bar(s)
    if layout.editor_group_split.is_some() {
        for (i, _gtb) in layout.group_tab_bars.iter().enumerate() {
            elems.push(UiElement::GroupTabBar { group_idx: i });
        }
        elems.push(UiElement::GroupDividers);
    } else {
        elems.push(UiElement::TabBar);
    }

    // draw_frame(): breadcrumbs
    for (i, bc) in layout.breadcrumbs.iter().enumerate() {
        if !bc.segments.is_empty() {
            elems.push(UiElement::Breadcrumbs { group_idx: i });
        }
    }

    // draw_frame(): editor windows
    for (i, rw) in layout.windows.iter().enumerate() {
        elems.push(UiElement::EditorWindow { window_idx: i });
        if rw.status_line.is_some() {
            elems.push(UiElement::WindowStatusLine { window_idx: i });
        }
    }

    // draw_frame(): status bar (global, only when separated_status_line is None)
    if layout.separated_status_line.is_none() {
        let any_per_window = layout.windows.iter().any(|w| w.status_line.is_some());
        if !any_per_window {
            elems.push(UiElement::GlobalStatusBar);
        }
    }

    // draw_frame(): command line
    elems.push(UiElement::CommandLine);

    // draw_frame(): tab tooltip
    if layout.tab_tooltip.is_some() {
        elems.push(UiElement::TabTooltip);
    }

    // draw_frame(): completion popup
    if layout.completion.is_some() {
        elems.push(UiElement::CompletionPopup);
    }

    // draw_frame(): hover popup
    if layout.hover.is_some() {
        elems.push(UiElement::HoverPopup);
    }

    // draw_frame(): editor hover (rich markdown)
    if layout.editor_hover.is_some() {
        elems.push(UiElement::EditorHoverPopup);
    }

    // draw_frame(): diff peek popup
    if layout.diff_peek.is_some() {
        elems.push(UiElement::DiffPeekPopup);
    }

    // draw_frame(): signature help
    if layout.signature_help.is_some() {
        elems.push(UiElement::SignatureHelp);
    }

    // draw_frame(): wildmenu
    if layout.wildmenu.is_some() {
        elems.push(UiElement::Wildmenu);
    }

    // draw_frame(): quickfix
    if layout.quickfix.is_some() {
        elems.push(UiElement::QuickfixPanel);
    }

    // draw_frame(): separated status line
    if layout.separated_status_line.is_some() {
        elems.push(UiElement::SeparatedStatusLine);
    }

    // draw_frame(): debug toolbar
    if layout.debug_toolbar.is_some() {
        elems.push(UiElement::DebugToolbar);
    }

    // draw_frame(): terminal
    if layout.bottom_tabs.terminal.is_some() {
        elems.push(UiElement::TerminalPanel);
    }

    // draw_frame(): panel hover popup
    if layout.panel_hover.is_some() {
        elems.push(UiElement::PanelHoverPopup);
    }

    // draw_frame(): picker
    if layout.picker.is_some() {
        elems.push(UiElement::PickerPopup);
    }

    // draw_frame(): tab switcher
    if layout.tab_switcher.is_some() {
        elems.push(UiElement::TabSwitcher);
    }

    // draw_frame(): context menu
    if layout.context_menu.is_some() {
        elems.push(UiElement::ContextMenu);
    }

    // draw_frame(): dialog
    if layout.dialog.is_some() {
        elems.push(UiElement::Dialog);
    }

    // on_paint(): sidebar (always rendered after draw_frame)
    elems.push(UiElement::ActivityBar);
    if layout.source_control.is_some()
        || layout.ext_sidebar.is_some()
        || layout.ai_panel.is_some()
        || layout.ext_panel.is_some()
        || layout.debug_sidebar.session_active
    {
        elems.push(UiElement::Sidebar);
    }

    // on_paint(): menu dropdown (rendered after sidebar for z-order)
    if layout.menu_dropdown_open {
        elems.push(UiElement::MenuDropdown);
    }

    // draw_tab_bar() / draw_group_tab_bar(): diff toolbar
    if layout.diff_toolbar.is_some() {
        elems.push(UiElement::DiffToolbar);
    }
    if layout.editor_group_split.is_some() {
        for gtb in &layout.group_tab_bars {
            if gtb.diff_toolbar.is_some() {
                elems.push(UiElement::DiffToolbar);
                break;
            }
        }
    }

    elems.sort();
    elems
}

/// Simulate the TUI backend's `draw_frame()` branching logic to collect which
/// [`UiElement`]s it would render.
pub fn collect_ui_elements_tui(layout: &ScreenLayout) -> Vec<UiElement> {
    let mut elems = Vec::new();

    // Menu bar
    if layout.menu_bar_visible {
        elems.push(UiElement::MenuBar);
    }

    // Activity bar (always rendered)
    elems.push(UiElement::ActivityBar);

    // Sidebar
    if layout.source_control.is_some()
        || layout.ext_sidebar.is_some()
        || layout.ai_panel.is_some()
        || layout.ext_panel.is_some()
        || layout.debug_sidebar.session_active
    {
        elems.push(UiElement::Sidebar);
    }

    // Tab bar(s)
    if layout.editor_group_split.is_some() {
        for (i, _gtb) in layout.group_tab_bars.iter().enumerate() {
            elems.push(UiElement::GroupTabBar { group_idx: i });
        }
        elems.push(UiElement::GroupDividers);
    } else {
        elems.push(UiElement::TabBar);
    }

    // Diff toolbar (single-group, rendered as part of tab bar)
    if layout.diff_toolbar.is_some() {
        elems.push(UiElement::DiffToolbar);
    }
    // Diff toolbar (per-group)
    if layout.editor_group_split.is_some() {
        for gtb in &layout.group_tab_bars {
            if gtb.diff_toolbar.is_some() {
                elems.push(UiElement::DiffToolbar);
                break;
            }
        }
    }

    // Breadcrumbs
    for (i, bc) in layout.breadcrumbs.iter().enumerate() {
        if !bc.segments.is_empty() {
            elems.push(UiElement::Breadcrumbs { group_idx: i });
        }
    }

    // Editor windows
    for (i, rw) in layout.windows.iter().enumerate() {
        elems.push(UiElement::EditorWindow { window_idx: i });
        if rw.status_line.is_some() {
            elems.push(UiElement::WindowStatusLine { window_idx: i });
        }
    }

    // Tab tooltip
    if layout.tab_tooltip.is_some() {
        elems.push(UiElement::TabTooltip);
    }

    // Completion popup
    if layout.completion.is_some() {
        elems.push(UiElement::CompletionPopup);
    }

    // Hover popup
    if layout.hover.is_some() {
        elems.push(UiElement::HoverPopup);
    }

    // Editor hover popup (rich markdown)
    if layout.editor_hover.is_some() {
        elems.push(UiElement::EditorHoverPopup);
    }

    // Diff peek popup
    if layout.diff_peek.is_some() {
        elems.push(UiElement::DiffPeekPopup);
    }

    // Signature help
    if layout.signature_help.is_some() {
        elems.push(UiElement::SignatureHelp);
    }

    // Quickfix
    if layout.quickfix.is_some() {
        elems.push(UiElement::QuickfixPanel);
    }

    // Separated status line
    if layout.separated_status_line.is_some() {
        elems.push(UiElement::SeparatedStatusLine);
    }

    // Bottom panel (terminal / debug output)
    if layout.bottom_tabs.terminal.is_some() {
        elems.push(UiElement::TerminalPanel);
    }

    // Debug toolbar
    if layout.debug_toolbar.is_some() {
        elems.push(UiElement::DebugToolbar);
    }

    // Wildmenu
    if layout.wildmenu.is_some() {
        elems.push(UiElement::Wildmenu);
    }

    // Global status bar (when per-window status is off)
    let any_per_window = layout.windows.iter().any(|w| w.status_line.is_some());
    if !any_per_window {
        elems.push(UiElement::GlobalStatusBar);
    }

    // Command line
    elems.push(UiElement::CommandLine);

    // Panel hover popup
    if layout.panel_hover.is_some() {
        elems.push(UiElement::PanelHoverPopup);
    }

    // Picker
    if layout.picker.is_some() {
        elems.push(UiElement::PickerPopup);
    }

    // Tab switcher
    if layout.tab_switcher.is_some() {
        elems.push(UiElement::TabSwitcher);
    }

    // Context menu
    if layout.context_menu.is_some() {
        elems.push(UiElement::ContextMenu);
    }

    // Dialog
    if layout.dialog.is_some() {
        elems.push(UiElement::Dialog);
    }

    // Menu dropdown (rendered last for z-order)
    if layout.menu_dropdown_open {
        elems.push(UiElement::MenuDropdown);
    }

    elems.sort();
    elems
}

// ─── ScreenLayout ─────────────────────────────────────────────────────────────

/// The complete, platform-agnostic description of one editor frame.
/// Build it with [`build_screen_layout`], then hand it to the backend renderer.
#[derive(Debug)]
pub struct ScreenLayout {
    pub tab_bar: Vec<TabInfo>,
    pub windows: Vec<RenderedWindow>,
    /// Global status bar (when per-window status lines are disabled).
    pub global_status_bar: Option<quadraui::StatusBar>,
    pub command: CommandLineData,
    /// Wildmenu bar (Tab completion in command mode), or `None` when inactive.
    pub wildmenu: Option<WildmenuData>,
    pub active_window_id: WindowId,
    /// Completion popup to show, or `None` when inactive.
    pub completion: Option<CompletionMenu>,
    /// Hover information popup, or `None` when inactive.
    pub hover: Option<HoverPopup>,
    /// Quickfix bottom panel, or `None` when closed.
    pub quickfix: Option<QuickfixPanel>,
    /// Bottom panel tabs (Terminal / Debug Output) — always present.
    pub bottom_tabs: BottomPanelTabs,
    /// Signature help popup (shown in insert mode after `(` or `,`), or `None`.
    pub signature_help: Option<SignatureHelp>,
    /// Menu bar strip data, or `None` when the bar is hidden.
    pub menu_bar_visible: bool,
    pub menu_dropdown_open: bool,
    /// Debug toolbar strip data, or `None` when hidden and no active session.
    pub debug_toolbar: Option<DebugToolbarData>,
    /// Debug sidebar data — always present (sections may be empty).
    pub debug_sidebar: DebugSidebarData,
    /// Source Control panel data — `Some` when the SC panel is the active sidebar panel.
    pub source_control: Option<SourceControlData>,
    /// Unified picker modal — `Some` when open.
    pub picker: Option<PickerPanel>,
    /// Tab switcher popup (Ctrl+Tab MRU list) — `Some` when open.
    pub tab_switcher: Option<TabSwitcherPanel>,
    /// Marker for "the editor area holds 2 or more groups", carrying the
    /// focused group + group count. `None` in the default single-group mode.
    /// The per-group chrome it used to own lives on `group_tab_bars` /
    /// `group_dividers` below, which are populated for *every* group count
    /// (#551).
    pub editor_group_split: Option<EditorGroupSplitData>,
    /// Tab bar + bounds for every editor group, in tree traversal order.
    /// Always populated — a single group is a split of one, so this holds
    /// exactly one entry in the default unsplit case rather than being empty
    /// with a parallel single-group field. Backends iterate it unconditionally
    /// (via `tab_bar_draw_targets`) instead of carrying a hand-written
    /// "exactly one group" draw path beside the generic N-group one (#551).
    pub group_tab_bars: Vec<GroupTabBar>,
    /// Divider lines *between* editor groups (`Ctrl+W v` / `Ctrl+W s`
    /// boundaries), in tree traversal order. Naturally empty when there is
    /// only one group — `GroupLayout::Leaf::dividers()` returns `vec![]` — so
    /// backends can paint it unconditionally (#551). Distinct from
    /// `window_dividers`, which are the `:split`/`:vsplit` boundaries *within*
    /// each group.
    pub group_dividers: Vec<GroupDivider>,
    /// Extensions sidebar data — `Some` when the Extensions panel is the active sidebar panel.
    pub ext_sidebar: Option<ExtSidebarData>,
    /// AI assistant panel data — `Some` when the AI panel is the active sidebar panel.
    pub ai_panel: Option<AiPanelData>,
    /// Extension-provided panel data — `Some` when an extension panel is the active sidebar panel.
    pub ext_panel: Option<ExtPanelData>,
    /// Breadcrumb bars for each editor group (empty when breadcrumbs are disabled).
    pub breadcrumbs: Vec<BreadcrumbBar>,
    /// Panel hover popup — `Some` when hovering over a sidebar panel item.
    pub panel_hover: Option<PanelHoverPopupData>,
    /// Editor hover popup — `Some` when hovering over an editor element (diagnostic, annotation, etc.).
    pub editor_hover: Option<EditorHoverPopupData>,
    /// Git diff peek popup — `Some` when the user is previewing a diff hunk.
    pub diff_peek: Option<DiffPeekPopup>,
    /// Diff toolbar data for the single-group tab bar.
    pub diff_toolbar: Option<DiffToolbarData>,
    /// Modal dialog popup — `Some` when a dialog is open.
    pub dialog: Option<DialogPanel>,
    /// Inline find/replace overlay — `Some` when the find/replace popup is open.
    pub find_replace: Option<FindReplacePanel>,
    /// Context menu popup — `Some` when an engine context menu is open.
    pub context_menu: Option<ContextMenuPanel>,
    /// Tab hover tooltip: shortened file path to display near the hovered tab.
    pub tab_tooltip: Option<String>,
    /// Tab scroll offset for the single-group tab bar.
    pub tab_scroll_offset: usize,
    /// Pre-built quadraui `TabBar` primitive for the single-group tab bar.
    pub tab_bar_primitive: quadraui::TabBar,
    /// Hit regions (char-cell columns, relative to the tab bar's left edge) for
    /// the single-group / active tab bar drawn from `tab_bar_primitive`. Empty in
    /// multi-group mode (each group carries its own `hit_regions` on its
    /// `GroupTabBar`). Lets backends resolve tab-bar clicks through the shared
    /// `resolve_tab_bar_click` path instead of per-backend pixel maps. (#515)
    pub tab_bar_hit_regions: Vec<(
        crate::core::engine::TabBarHitRegion,
        crate::core::engine::TabBarClickTarget,
    )>,
    /// When `status_line_above_terminal` is OFF and the terminal panel is open,
    /// this carries the active window's status line to render as a dedicated row
    /// above the terminal panel. When `Some`, per-window `status_line` fields on
    /// individual `RenderedWindow`s are `None`.
    /// (Setting name is historical — the UI labels it "Status Line Inside Window";
    /// `true` keeps the bar inside each editor window, `false` extracts it.)
    pub separated_status_line: Option<WindowStatusLine>,
    /// Window-split (`:split`/`:vsplit`) dividers across all editor groups'
    /// active tabs. Independent of `editor_group_split` — window splits exist
    /// regardless of how many editor groups are open (#582).
    pub window_dividers: Vec<WindowDivider>,
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

    let anchor_x = panel.screen_col as f64 * char_width;
    let anchor_y = panel.screen_row as f64 * line_height;
    let trigger_height_px = panel.trigger_height as f64 * line_height;
    let item_height = |_i: usize| quadraui::ContextMenuItemMeasure::new(line_height as f32);

    let layout = menu.layout_at(
        quadraui::Rect::new(
            anchor_x as f32,
            anchor_y as f32,
            0.0,
            trigger_height_px as f32,
        ),
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
    /// Each button is `(formatted_label, is_selected)`.
    pub buttons: Vec<(String, bool)>,
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
        .map(|(i, (label, is_selected))| quadraui::DialogButton {
            id: quadraui::WidgetId::new(format!("dialog:btn:{i}")),
            label: label.clone(),
            is_default: *is_selected,
            is_cancel: false,
            tint: None,
        })
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
        .map(|(lbl, _)| lbl.chars().count() + 4)
        .max()
        .unwrap_or(0);
    let btn_row_len: usize = if panel.vertical_buttons {
        btn_max_label + 2
    } else {
        panel
            .buttons
            .iter()
            .map(|(lbl, _)| lbl.chars().count() + 4)
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
    let fg = to_q_color(theme.hover_fg);
    let added = to_q_color(theme.git_added);
    let deleted = to_q_color(theme.git_deleted);

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

// ─── Theme ────────────────────────────────────────────────────────────────────

/// All colours used by the editor UI.
/// Derive new themes by constructing a `Theme` with different field values.
pub struct Theme {
    // Editor background
    pub background: Color,
    /// Slightly lighter background for the active window when splits exist.
    pub active_background: Color,
    /// Default text foreground.
    pub foreground: Color,

    // Syntax highlighting
    pub keyword: Color,
    pub string_lit: Color,
    pub comment: Color,
    pub function: Color,
    pub type_name: Color,
    pub variable: Color,
    pub number: Color,
    pub control_flow: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub macro_call: Color,
    pub attribute: Color,
    pub lifetime: Color,
    pub constant: Color,
    pub escape: Color,
    pub boolean: Color,
    pub property: Color,
    pub parameter: Color,
    pub module: Color,
    /// Fallback foreground for unrecognised scopes.
    pub default_fg: Color,

    // Visual selection (alpha handled separately in Cairo)
    pub selection: Color,
    pub selection_alpha: f64,

    // Cursor
    pub cursor: Color,
    pub cursor_normal_alpha: f64,

    // Search match highlights
    pub search_match_bg: Color,
    pub search_current_match_bg: Color,
    pub search_match_fg: Color,

    // Yank highlight flash
    pub yank_highlight_bg: Color,
    pub yank_highlight_alpha: f64,

    // Virtual text / line annotations (e.g. git blame inline)
    pub annotation_fg: Color,

    // AI ghost text (inline completions)
    pub ghost_text_fg: Color,

    // Tab bar
    pub tab_bar_bg: Color,
    pub tab_active_bg: Color,
    pub tab_active_fg: Color,
    pub tab_inactive_fg: Color,
    pub tab_preview_active_fg: Color,
    pub tab_preview_inactive_fg: Color,
    /// Accent line color for the active tab in the focused editor group.
    pub tab_active_accent: Color,

    // Status line
    pub status_bg: Color,
    pub status_fg: Color,

    // Per-window status line mode text tints
    pub status_mode_normal_bg: Color,
    pub status_mode_insert_bg: Color,
    pub status_mode_visual_bg: Color,
    pub status_mode_replace_bg: Color,
    pub status_inactive_bg: Color,
    pub status_inactive_fg: Color,

    // Wildmenu (command Tab completion bar)
    pub wildmenu_bg: Color,
    pub wildmenu_fg: Color,
    pub wildmenu_sel_bg: Color,
    pub wildmenu_sel_fg: Color,

    // Command / message line
    pub command_bg: Color,
    pub command_fg: Color,

    // Line numbers
    pub line_number_fg: Color,
    pub line_number_active_fg: Color,

    // Window separator
    pub separator: Color,

    // Git diff gutter markers
    pub git_added: Color,
    pub git_modified: Color,
    pub git_deleted: Color,

    // Completion popup
    pub completion_bg: Color,
    pub completion_selected_bg: Color,
    pub completion_fg: Color,
    pub completion_border: Color,

    // Diagnostic colours
    pub diagnostic_error: Color,
    pub diagnostic_warning: Color,
    pub diagnostic_info: Color,
    pub diagnostic_hint: Color,

    // Spell checking
    pub spell_error: Color,

    // Code action lightbulb
    pub lightbulb: Color,

    // Hover popup
    pub hover_bg: Color,
    pub hover_fg: Color,
    pub hover_border: Color,

    // Fuzzy file-picker modal
    pub fuzzy_bg: Color,
    pub fuzzy_selected_bg: Color,
    pub fuzzy_fg: Color,
    pub fuzzy_query_fg: Color,
    pub fuzzy_border: Color,
    pub fuzzy_title_fg: Color,
    /// Highlight color for fuzzy-match character positions.
    pub fuzzy_match_fg: Color,

    // Two-way diff background colours
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_padding_bg: Color,

    // DAP stopped-line highlight
    pub dap_stopped_bg: Color,

    // Cursor line highlight (subtle background for the current line).
    // Derived from `background` by default; overridden by VSCode theme
    // `editor.lineHighlightBackground`.
    pub cursorline_bg: Color,

    // Markdown preview colours
    pub md_heading1: Color,
    pub md_heading2: Color,
    pub md_heading3: Color,
    pub md_code: Color,
    pub md_link: Color,

    // Sidebar selection
    /// Background for the selected row when the sidebar has keyboard focus.
    pub sidebar_sel_bg: Color,
    /// Background for the selected row when the sidebar does NOT have focus.
    pub sidebar_sel_bg_inactive: Color,

    // LSP semantic token colours (overlay on tree-sitter)
    pub semantic_parameter: Color,
    pub semantic_property: Color,
    pub semantic_namespace: Color,
    pub semantic_enum_member: Color,
    pub semantic_interface: Color,
    pub semantic_type_parameter: Color,
    pub semantic_decorator: Color,
    pub semantic_macro: Color,

    // Breadcrumb bar
    pub breadcrumb_bg: Color,
    pub breadcrumb_fg: Color,
    pub breadcrumb_active_fg: Color,

    // Indent guides
    pub indent_guide_fg: Color,
    pub indent_guide_active_fg: Color,

    // Color column (`:set colorcolumn=80`)
    pub colorcolumn_bg: Color,

    // Bracket match highlight
    pub bracket_match_bg: Color,

    // Explorer sidebar (TUI)
    /// Foreground for directory names in the file explorer.
    pub explorer_dir_fg: Color,
    /// Foreground for file names in the file explorer (muted grey).
    pub explorer_file_fg: Color,
    /// Background tint for rows whose file is open in a buffer.
    pub explorer_active_bg: Color,

    // Scrollbar
    /// Scrollbar thumb (draggable part).
    pub scrollbar_thumb: Color,
    /// Scrollbar track (gutter behind thumb).
    pub scrollbar_track: Color,

    // Integrated terminal
    /// Default background for the integrated terminal pane.
    pub terminal_bg: Color,

    // Activity bar
    /// Foreground for activity bar icons.
    pub activity_bar_fg: Color,
}

impl Theme {
    /// The OneDark-inspired colour scheme currently used by VimCode.
    /// All values are derived directly from the Cairo RGB tuples in the
    /// original `draw_*` functions.
    pub fn onedark() -> Self {
        let bg = Color::from_hex("#1a1a1a");
        Self {
            // (0.1, 0.1, 0.1)
            background: bg,
            // (0.12, 0.12, 0.12)
            active_background: Color::from_hex("#1e1e1e"),
            // (0.9, 0.9, 0.9)
            foreground: Color::from_hex("#e5e5e5"),

            keyword: Color::from_hex("#c678dd"),
            control_flow: Color::from_hex("#c678dd"),
            string_lit: Color::from_hex("#98c379"),
            comment: Color::from_hex("#5c6370"),
            function: Color::from_hex("#61afef"),
            type_name: Color::from_hex("#e5c07b"),
            variable: Color::from_hex("#e06c75"),
            number: Color::from_hex("#d19a66"),
            operator: Color::from_hex("#56b6c2"),
            punctuation: Color::from_hex("#abb2bf"),
            macro_call: Color::from_hex("#61afef"),
            attribute: Color::from_hex("#e5c07b"),
            lifetime: Color::from_hex("#e06c75"),
            constant: Color::from_hex("#d19a66"),
            escape: Color::from_hex("#56b6c2"),
            boolean: Color::from_hex("#d19a66"),
            property: Color::from_hex("#e06c75"),
            parameter: Color::from_hex("#e06c75"),
            module: Color::from_hex("#e5c07b"),
            default_fg: Color::from_hex("#abb2bf"),

            // (0.3, 0.5, 0.7) with alpha 0.3
            selection: Color::from_hex("#4c7fb2"),
            selection_alpha: 0.3,

            // (1.0, 1.0, 1.0) with alpha 0.5 in Normal/Visual
            cursor: Color::from_hex("#ffffff"),
            cursor_normal_alpha: 0.5,

            // Pango 16-bit: (180*256, 150*256, 0) → RGB(180, 150, 0)
            search_match_bg: Color::from_hex("#b49600"),
            // Pango 16-bit: (255*256, 200*256, 0) → RGB(255, 200, 0)
            search_current_match_bg: Color::from_hex("#ffc800"),
            search_match_fg: Color::from_hex("#000000"),

            // (0.15, 0.15, 0.2)
            tab_bar_bg: Color::from_hex("#262633"),
            // (0.25, 0.25, 0.35)
            tab_active_bg: Color::from_hex("#3f3f59"),
            // (1.0, 1.0, 1.0)
            tab_active_fg: Color::from_hex("#ffffff"),
            // (0.7, 0.7, 0.7)
            tab_inactive_fg: Color::from_hex("#b2b2b2"),
            // (0.8, 0.8, 0.8)
            tab_preview_active_fg: Color::from_hex("#cccccc"),
            // (0.5, 0.5, 0.5)
            tab_preview_inactive_fg: Color::from_hex("#7f7f7f"),
            tab_active_accent: Color::from_hex("#61afef"),

            status_bg: Color::from_hex("#33334c"),
            status_fg: Color::from_hex("#e5e5e5"),

            status_mode_normal_bg: Color::from_hex("#61afef"),
            status_mode_insert_bg: Color::from_hex("#98c379"),
            status_mode_visual_bg: Color::from_hex("#c678dd"),
            status_mode_replace_bg: Color::from_hex("#e06c75"),
            status_inactive_bg: Color::from_hex("#262626"),
            status_inactive_fg: Color::from_hex("#808080"),

            wildmenu_bg: Color::from_hex("#33334c"),
            wildmenu_fg: Color::from_hex("#abb2bf"),
            wildmenu_sel_bg: Color::from_hex("#e5c07b"),
            wildmenu_sel_fg: Color::from_hex("#282c34"),

            // (0.1, 0.1, 0.1)
            command_bg: Color::from_hex("#1a1a1a"),
            // (0.9, 0.9, 0.9)
            command_fg: Color::from_hex("#e5e5e5"),

            // (0.7, 0.7, 0.7)
            line_number_fg: Color::from_hex("#b2b2b2"),
            // (0.9, 0.9, 0.5)
            line_number_active_fg: Color::from_hex("#e5e57f"),

            // (0.3, 0.3, 0.4)
            separator: Color::from_hex("#4c4c66"),

            // Git diff gutter markers
            git_added: Color::from_hex("#98c379"),    // green
            git_modified: Color::from_hex("#e5c07b"), // yellow
            git_deleted: Color::from_hex("#e06c75"),  // red

            // Completion popup (OneDark palette)
            completion_bg: Color::from_hex("#282c34"),
            completion_selected_bg: Color::from_hex("#3e4451"),
            completion_fg: Color::from_hex("#abb2bf"),
            completion_border: Color::from_hex("#528bff"),

            // Diagnostic colours
            diagnostic_error: Color::from_hex("#e06c75"), // red
            diagnostic_warning: Color::from_hex("#e5c07b"), // yellow
            diagnostic_info: Color::from_hex("#61afef"),  // blue
            diagnostic_hint: Color::from_hex("#5c6370"),  // grey
            spell_error: Color::from_hex("#56b6c2"),      // cyan
            lightbulb: Color::from_hex("#e5c07b"),        // yellow

            // Hover popup
            hover_bg: Color::from_hex("#21252b"),
            hover_fg: Color::from_hex("#abb2bf"),
            hover_border: Color::from_hex("#528bff"),

            // Fuzzy file-picker modal (OneDark palette)
            fuzzy_bg: Color::from_hex("#21252b"),
            fuzzy_selected_bg: Color::from_hex("#2c313c"),
            fuzzy_fg: Color::from_hex("#abb2bf"),
            fuzzy_query_fg: Color::from_hex("#61afef"),
            fuzzy_border: Color::from_hex("#528bff"),
            fuzzy_title_fg: Color::from_hex("#e5c07b"),
            fuzzy_match_fg: Color::from_hex("#61afef"),

            // Two-way diff backgrounds — must be clearly green/red in terminals
            diff_added_bg: Color::from_hex("#14541a"),
            diff_removed_bg: Color::from_hex("#541a1a"),
            diff_padding_bg: Color::from_hex("#2d2d2d"),

            // DAP stopped-line (dark amber)
            dap_stopped_bg: Color::from_hex("#3a3000"),

            // Cursor line highlight (subtle lightening of background)
            cursorline_bg: Color::from_hex("#1a1a1a").cursorline_tint(), // derived from background

            // Yank highlight flash (green, matching Neovim default)
            yank_highlight_bg: Color::from_hex("#57d45e"),
            yank_highlight_alpha: 0.35,

            // Virtual text annotations (muted grey — matches comment colour)
            annotation_fg: Color::from_hex("#5c6370"),

            // AI ghost text (inline completions) — slightly lighter than annotation
            ghost_text_fg: Color::from_hex("#4b5263"),

            // Markdown preview
            md_heading1: Color::from_hex("#e5c07b"), // gold
            md_heading2: Color::from_hex("#61afef"), // blue
            md_heading3: Color::from_hex("#c678dd"), // purple
            md_code: Color::from_hex("#98c379"),     // green (string-like)
            md_link: Color::from_hex("#61afef"),     // blue

            sidebar_sel_bg: Color::from_hex("#373d4a"), // focused: visible highlight
            sidebar_sel_bg_inactive: Color::from_hex("#21252b"), // unfocused: very faint
            semantic_parameter: Color::from_hex("#c8ae9d"), // warm sandy (distinct from variable red)
            semantic_property: Color::from_hex("#d19a66"),  // orange
            semantic_namespace: Color::from_hex("#e5c07b"), // gold
            semantic_enum_member: Color::from_hex("#56b6c2"), // cyan
            semantic_interface: Color::from_hex("#e5c07b"), // gold (like type)
            semantic_type_parameter: Color::from_hex("#e5c07b"), // gold
            semantic_decorator: Color::from_hex("#c678dd"), // purple (like keyword)
            semantic_macro: Color::from_hex("#56b6c2"),     // cyan

            breadcrumb_bg: Color::from_hex("#21252b"),
            breadcrumb_fg: Color::from_hex("#7f848e"),
            breadcrumb_active_fg: Color::from_hex("#abb2bf"),

            indent_guide_fg: Color::from_hex("#404040"),
            indent_guide_active_fg: Color::from_hex("#606060"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: Color::from_hex("#3a3d41"),

            explorer_dir_fg: Color::from_hex("#61afef"), // function blue
            explorer_file_fg: Color::from_hex("#aab1be"), // muted grey (matches OneDark sidebar)
            explorer_active_bg: Color::from_hex("#333842"), // current-file tint

            scrollbar_thumb: Color::from_hex("#5a5a5a"),
            scrollbar_track: Color::from_hex("#1a1a1a"),
            terminal_bg: Color::from_hex("#1e1e1e"),
            activity_bar_fg: Color::from_hex("#c8c8d2"),
        }
    }

    /// Gruvbox Dark colour scheme.
    pub fn gruvbox_dark() -> Self {
        let bg = Color::from_hex("#282828");
        Self {
            background: bg,
            active_background: Color::from_hex("#32302f"),
            foreground: Color::from_hex("#ebdbb2"),

            keyword: Color::from_hex("#fb4934"),
            control_flow: Color::from_hex("#fb4934"),
            string_lit: Color::from_hex("#b8bb26"),
            comment: Color::from_hex("#928374"),
            function: Color::from_hex("#8ec07c"),
            type_name: Color::from_hex("#fabd2f"),
            variable: Color::from_hex("#83a598"),
            number: Color::from_hex("#d3869b"),
            operator: Color::from_hex("#8ec07c"),
            punctuation: Color::from_hex("#ebdbb2"),
            macro_call: Color::from_hex("#8ec07c"),
            attribute: Color::from_hex("#fabd2f"),
            lifetime: Color::from_hex("#fb4934"),
            constant: Color::from_hex("#d3869b"),
            escape: Color::from_hex("#8ec07c"),
            boolean: Color::from_hex("#d3869b"),
            property: Color::from_hex("#83a598"),
            parameter: Color::from_hex("#83a598"),
            module: Color::from_hex("#fabd2f"),
            default_fg: Color::from_hex("#ebdbb2"),

            selection: Color::from_hex("#458588"),
            selection_alpha: 0.4,

            cursor: Color::from_hex("#ebdbb2"),
            cursor_normal_alpha: 0.6,

            search_match_bg: Color::from_hex("#d65d0e"),
            search_current_match_bg: Color::from_hex("#fe8019"),
            search_match_fg: Color::from_hex("#1d2021"),

            tab_bar_bg: Color::from_hex("#3c3836"),
            tab_active_bg: Color::from_hex("#504945"),
            tab_active_fg: Color::from_hex("#ebdbb2"),
            tab_inactive_fg: Color::from_hex("#a89984"),
            tab_preview_active_fg: Color::from_hex("#d5c4a1"),
            tab_preview_inactive_fg: Color::from_hex("#7c6f64"),
            tab_active_accent: Color::from_hex("#d65d0e"),

            status_bg: Color::from_hex("#504945"),
            status_fg: Color::from_hex("#ebdbb2"),

            status_mode_normal_bg: Color::from_hex("#83a598"),
            status_mode_insert_bg: Color::from_hex("#b8bb26"),
            status_mode_visual_bg: Color::from_hex("#d3869b"),
            status_mode_replace_bg: Color::from_hex("#fb4934"),
            status_inactive_bg: Color::from_hex("#303030"),
            status_inactive_fg: Color::from_hex("#808080"),

            wildmenu_bg: Color::from_hex("#504945"),
            wildmenu_fg: Color::from_hex("#ebdbb2"),
            wildmenu_sel_bg: Color::from_hex("#fabd2f"),
            wildmenu_sel_fg: Color::from_hex("#282828"),

            command_bg: Color::from_hex("#282828"),
            command_fg: Color::from_hex("#ebdbb2"),

            line_number_fg: Color::from_hex("#7c6f64"),
            line_number_active_fg: Color::from_hex("#fabd2f"),

            separator: Color::from_hex("#665c54"),

            git_added: Color::from_hex("#b8bb26"),
            git_modified: Color::from_hex("#fabd2f"),
            git_deleted: Color::from_hex("#fb4934"),

            completion_bg: Color::from_hex("#32302f"),
            completion_selected_bg: Color::from_hex("#504945"),
            completion_fg: Color::from_hex("#ebdbb2"),
            completion_border: Color::from_hex("#458588"),

            diagnostic_error: Color::from_hex("#fb4934"),
            diagnostic_warning: Color::from_hex("#fabd2f"),
            diagnostic_info: Color::from_hex("#83a598"),
            diagnostic_hint: Color::from_hex("#928374"),
            spell_error: Color::from_hex("#8ec07c"),
            lightbulb: Color::from_hex("#fabd2f"),

            hover_bg: Color::from_hex("#32302f"),
            hover_fg: Color::from_hex("#ebdbb2"),
            hover_border: Color::from_hex("#458588"),

            fuzzy_bg: Color::from_hex("#32302f"),
            fuzzy_selected_bg: Color::from_hex("#504945"),
            fuzzy_fg: Color::from_hex("#ebdbb2"),
            fuzzy_query_fg: Color::from_hex("#8ec07c"),
            fuzzy_border: Color::from_hex("#458588"),
            fuzzy_title_fg: Color::from_hex("#fabd2f"),
            fuzzy_match_fg: Color::from_hex("#83a598"),

            // (bg #282828)
            diff_added_bg: Color::from_hex("#1e5e24"),
            diff_removed_bg: Color::from_hex("#5e2424"),
            diff_padding_bg: Color::from_hex("#333333"),

            dap_stopped_bg: Color::from_hex("#3a3000"),

            cursorline_bg: Color::from_hex("#282828").cursorline_tint(), // derived from background

            yank_highlight_bg: Color::from_hex("#b8bb26"),
            yank_highlight_alpha: 0.35,

            annotation_fg: Color::from_hex("#928374"),
            ghost_text_fg: Color::from_hex("#7c6f64"),

            md_heading1: Color::from_hex("#fabd2f"),
            md_heading2: Color::from_hex("#83a598"),
            md_heading3: Color::from_hex("#d3869b"),
            md_code: Color::from_hex("#b8bb26"),
            md_link: Color::from_hex("#83a598"),

            sidebar_sel_bg: Color::from_hex("#504945"), // focused: visible highlight
            sidebar_sel_bg_inactive: Color::from_hex("#32302f"), // unfocused
            semantic_parameter: Color::from_hex("#83a598"), // blue
            semantic_property: Color::from_hex("#d3869b"), // purple-pink
            semantic_namespace: Color::from_hex("#fabd2f"), // yellow
            semantic_enum_member: Color::from_hex("#8ec07c"), // aqua
            semantic_interface: Color::from_hex("#fabd2f"), // yellow
            semantic_type_parameter: Color::from_hex("#fabd2f"),
            semantic_decorator: Color::from_hex("#fb4934"), // red
            semantic_macro: Color::from_hex("#8ec07c"),     // aqua

            breadcrumb_bg: Color::from_hex("#32302f"),
            breadcrumb_fg: Color::from_hex("#a89984"),
            breadcrumb_active_fg: Color::from_hex("#ebdbb2"),

            indent_guide_fg: Color::from_hex("#3c3836"),
            indent_guide_active_fg: Color::from_hex("#504945"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: Color::from_hex("#504945"),

            explorer_dir_fg: Color::from_hex("#83a598"), // gruvbox blue
            explorer_file_fg: Color::from_hex("#bdae93"), // gruvbox muted
            explorer_active_bg: Color::from_hex("#45403d"), // current-file tint

            scrollbar_thumb: Color::from_hex("#665c54"),
            scrollbar_track: Color::from_hex("#282828"),
            terminal_bg: Color::from_hex("#282828"),
            activity_bar_fg: Color::from_hex("#bdae93"),
        }
    }

    /// Tokyo Night colour scheme.
    pub fn tokyo_night() -> Self {
        let bg = Color::from_hex("#1a1b26");
        Self {
            background: bg,
            active_background: Color::from_hex("#1f2335"),
            foreground: Color::from_hex("#c0caf5"),

            keyword: Color::from_hex("#bb9af7"),
            control_flow: Color::from_hex("#bb9af7"),
            string_lit: Color::from_hex("#9ece6a"),
            comment: Color::from_hex("#565f89"),
            function: Color::from_hex("#7aa2f7"),
            type_name: Color::from_hex("#e0af68"),
            variable: Color::from_hex("#f7768e"),
            number: Color::from_hex("#ff9e64"),
            operator: Color::from_hex("#89ddff"),
            punctuation: Color::from_hex("#a9b1d6"),
            macro_call: Color::from_hex("#7aa2f7"),
            attribute: Color::from_hex("#e0af68"),
            lifetime: Color::from_hex("#f7768e"),
            constant: Color::from_hex("#ff9e64"),
            escape: Color::from_hex("#89ddff"),
            boolean: Color::from_hex("#ff9e64"),
            property: Color::from_hex("#73daca"),
            parameter: Color::from_hex("#e0af68"),
            module: Color::from_hex("#e0af68"),
            default_fg: Color::from_hex("#a9b1d6"),

            selection: Color::from_hex("#364a82"),
            selection_alpha: 0.5,

            cursor: Color::from_hex("#c0caf5"),
            cursor_normal_alpha: 0.5,

            search_match_bg: Color::from_hex("#3d59a1"),
            search_current_match_bg: Color::from_hex("#ff9e64"),
            search_match_fg: Color::from_hex("#c0caf5"),

            tab_bar_bg: Color::from_hex("#16161e"),
            tab_active_bg: Color::from_hex("#292e42"),
            tab_active_fg: Color::from_hex("#c0caf5"),
            tab_inactive_fg: Color::from_hex("#545c7e"),
            tab_preview_active_fg: Color::from_hex("#a9b1d6"),
            tab_preview_inactive_fg: Color::from_hex("#3b4261"),
            tab_active_accent: Color::from_hex("#7aa2f7"),

            status_bg: Color::from_hex("#292e42"),
            status_fg: Color::from_hex("#c0caf5"),

            status_mode_normal_bg: Color::from_hex("#7aa2f7"),
            status_mode_insert_bg: Color::from_hex("#9ece6a"),
            status_mode_visual_bg: Color::from_hex("#bb9af7"),
            status_mode_replace_bg: Color::from_hex("#f7768e"),
            status_inactive_bg: Color::from_hex("#262626"),
            status_inactive_fg: Color::from_hex("#808080"),

            wildmenu_bg: Color::from_hex("#292e42"),
            wildmenu_fg: Color::from_hex("#c0caf5"),
            wildmenu_sel_bg: Color::from_hex("#e0af68"),
            wildmenu_sel_fg: Color::from_hex("#1a1b26"),

            command_bg: Color::from_hex("#1a1b26"),
            command_fg: Color::from_hex("#c0caf5"),

            line_number_fg: Color::from_hex("#3b4261"),
            line_number_active_fg: Color::from_hex("#e0af68"),

            separator: Color::from_hex("#292e42"),

            git_added: Color::from_hex("#9ece6a"),
            git_modified: Color::from_hex("#e0af68"),
            git_deleted: Color::from_hex("#f7768e"),

            completion_bg: Color::from_hex("#1f2335"),
            completion_selected_bg: Color::from_hex("#364a82"),
            completion_fg: Color::from_hex("#c0caf5"),
            completion_border: Color::from_hex("#7aa2f7"),

            diagnostic_error: Color::from_hex("#f7768e"),
            diagnostic_warning: Color::from_hex("#e0af68"),
            diagnostic_info: Color::from_hex("#7aa2f7"),
            diagnostic_hint: Color::from_hex("#565f89"),
            spell_error: Color::from_hex("#7dcfff"),
            lightbulb: Color::from_hex("#e0af68"),

            hover_bg: Color::from_hex("#1f2335"),
            hover_fg: Color::from_hex("#c0caf5"),
            hover_border: Color::from_hex("#7aa2f7"),

            fuzzy_bg: Color::from_hex("#1f2335"),
            fuzzy_selected_bg: Color::from_hex("#364a82"),
            fuzzy_fg: Color::from_hex("#c0caf5"),
            fuzzy_query_fg: Color::from_hex("#7aa2f7"),
            fuzzy_border: Color::from_hex("#7aa2f7"),
            fuzzy_title_fg: Color::from_hex("#e0af68"),
            fuzzy_match_fg: Color::from_hex("#7aa2f7"),

            // (bg #1a1b26)
            diff_added_bg: Color::from_hex("#14541a"),
            diff_removed_bg: Color::from_hex("#541a28"),
            diff_padding_bg: Color::from_hex("#252530"),

            dap_stopped_bg: Color::from_hex("#2a2500"),

            cursorline_bg: Color::from_hex("#1a1b26").cursorline_tint(), // derived from background

            yank_highlight_bg: Color::from_hex("#9ece6a"),
            yank_highlight_alpha: 0.35,

            annotation_fg: Color::from_hex("#565f89"),
            ghost_text_fg: Color::from_hex("#414868"),

            md_heading1: Color::from_hex("#e0af68"),
            md_heading2: Color::from_hex("#7aa2f7"),
            md_heading3: Color::from_hex("#bb9af7"),
            md_code: Color::from_hex("#9ece6a"),
            md_link: Color::from_hex("#7aa2f7"),

            sidebar_sel_bg: Color::from_hex("#33395a"), // focused: visible highlight
            sidebar_sel_bg_inactive: Color::from_hex("#1f2335"), // unfocused
            semantic_parameter: Color::from_hex("#e0af68"), // orange-gold
            semantic_property: Color::from_hex("#73daca"), // teal
            semantic_namespace: Color::from_hex("#2ac3de"), // cyan
            semantic_enum_member: Color::from_hex("#ff9e64"), // orange
            semantic_interface: Color::from_hex("#2ac3de"), // cyan
            semantic_type_parameter: Color::from_hex("#e0af68"),
            semantic_decorator: Color::from_hex("#bb9af7"), // purple
            semantic_macro: Color::from_hex("#2ac3de"),     // cyan

            breadcrumb_bg: Color::from_hex("#1f2335"),
            breadcrumb_fg: Color::from_hex("#565f89"),
            breadcrumb_active_fg: Color::from_hex("#c0caf5"),

            indent_guide_fg: Color::from_hex("#292e42"),
            indent_guide_active_fg: Color::from_hex("#3b4261"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: Color::from_hex("#364a82"),

            explorer_dir_fg: Color::from_hex("#7aa2f7"), // tokyo blue
            explorer_file_fg: Color::from_hex("#a9b1d6"), // tokyo muted
            explorer_active_bg: Color::from_hex("#2f3550"), // current-file tint

            scrollbar_thumb: Color::from_hex("#565f89"),
            scrollbar_track: Color::from_hex("#1a1b26"),
            terminal_bg: Color::from_hex("#1a1b26"),
            activity_bar_fg: Color::from_hex("#a9b1d6"),
        }
    }

    /// Solarized Dark colour scheme.
    pub fn solarized_dark() -> Self {
        let bg = Color::from_hex("#002b36");
        Self {
            background: bg,
            active_background: Color::from_hex("#073642"),
            foreground: Color::from_hex("#839496"),

            keyword: Color::from_hex("#859900"),
            control_flow: Color::from_hex("#859900"),
            string_lit: Color::from_hex("#2aa198"),
            comment: Color::from_hex("#586e75"),
            function: Color::from_hex("#268bd2"),
            type_name: Color::from_hex("#b58900"),
            variable: Color::from_hex("#dc322f"),
            number: Color::from_hex("#2aa198"),
            operator: Color::from_hex("#859900"),
            punctuation: Color::from_hex("#93a1a1"),
            macro_call: Color::from_hex("#268bd2"),
            attribute: Color::from_hex("#b58900"),
            lifetime: Color::from_hex("#dc322f"),
            constant: Color::from_hex("#2aa198"),
            escape: Color::from_hex("#cb4b16"),
            boolean: Color::from_hex("#2aa198"),
            property: Color::from_hex("#268bd2"),
            parameter: Color::from_hex("#93a1a1"),
            module: Color::from_hex("#b58900"),
            default_fg: Color::from_hex("#93a1a1"),

            selection: Color::from_hex("#073642"),
            selection_alpha: 0.6,

            cursor: Color::from_hex("#93a1a1"),
            cursor_normal_alpha: 0.6,

            search_match_bg: Color::from_hex("#cb4b16"),
            search_current_match_bg: Color::from_hex("#d33682"),
            search_match_fg: Color::from_hex("#fdf6e3"),

            tab_bar_bg: Color::from_hex("#073642"),
            tab_active_bg: Color::from_hex("#0d4a5a"),
            tab_active_fg: Color::from_hex("#93a1a1"),
            tab_inactive_fg: Color::from_hex("#586e75"),
            tab_preview_active_fg: Color::from_hex("#839496"),
            tab_preview_inactive_fg: Color::from_hex("#4a6570"),
            tab_active_accent: Color::from_hex("#268bd2"),

            status_bg: Color::from_hex("#073642"),
            status_fg: Color::from_hex("#93a1a1"),

            status_mode_normal_bg: Color::from_hex("#268bd2"),
            status_mode_insert_bg: Color::from_hex("#859900"),
            status_mode_visual_bg: Color::from_hex("#6c71c4"),
            status_mode_replace_bg: Color::from_hex("#dc322f"),
            status_inactive_bg: Color::from_hex("#121212"),
            status_inactive_fg: Color::from_hex("#6c6c6c"),

            wildmenu_bg: Color::from_hex("#073642"),
            wildmenu_fg: Color::from_hex("#93a1a1"),
            wildmenu_sel_bg: Color::from_hex("#b58900"),
            wildmenu_sel_fg: Color::from_hex("#002b36"),

            command_bg: Color::from_hex("#002b36"),
            command_fg: Color::from_hex("#839496"),

            line_number_fg: Color::from_hex("#586e75"),
            line_number_active_fg: Color::from_hex("#b58900"),

            separator: Color::from_hex("#073642"),

            git_added: Color::from_hex("#859900"),
            git_modified: Color::from_hex("#b58900"),
            git_deleted: Color::from_hex("#dc322f"),

            completion_bg: Color::from_hex("#073642"),
            completion_selected_bg: Color::from_hex("#0d4a5a"),
            completion_fg: Color::from_hex("#839496"),
            completion_border: Color::from_hex("#268bd2"),

            diagnostic_error: Color::from_hex("#dc322f"),
            diagnostic_warning: Color::from_hex("#b58900"),
            diagnostic_info: Color::from_hex("#268bd2"),
            diagnostic_hint: Color::from_hex("#586e75"),
            spell_error: Color::from_hex("#2aa198"),
            lightbulb: Color::from_hex("#b58900"),

            hover_bg: Color::from_hex("#073642"),
            hover_fg: Color::from_hex("#93a1a1"),
            hover_border: Color::from_hex("#268bd2"),

            fuzzy_bg: Color::from_hex("#073642"),
            fuzzy_selected_bg: Color::from_hex("#0d4a5a"),
            fuzzy_fg: Color::from_hex("#839496"),
            fuzzy_query_fg: Color::from_hex("#268bd2"),
            fuzzy_border: Color::from_hex("#268bd2"),
            fuzzy_title_fg: Color::from_hex("#b58900"),
            fuzzy_match_fg: Color::from_hex("#268bd2"),

            // (bg #002b36)
            diff_added_bg: Color::from_hex("#005e30"),
            diff_removed_bg: Color::from_hex("#5e1a28"),
            diff_padding_bg: Color::from_hex("#0a3545"),

            dap_stopped_bg: Color::from_hex("#2b2000"),

            cursorline_bg: Color::from_hex("#002b36").cursorline_tint(), // derived from background

            yank_highlight_bg: Color::from_hex("#859900"),
            yank_highlight_alpha: 0.35,

            annotation_fg: Color::from_hex("#586e75"),
            ghost_text_fg: Color::from_hex("#4a5e68"),

            md_heading1: Color::from_hex("#b58900"),
            md_heading2: Color::from_hex("#268bd2"),
            md_heading3: Color::from_hex("#6c71c4"),
            md_code: Color::from_hex("#859900"),
            md_link: Color::from_hex("#268bd2"),

            sidebar_sel_bg: Color::from_hex("#0a4a5a"), // focused: visible highlight
            sidebar_sel_bg_inactive: Color::from_hex("#002b36"), // unfocused (base03)
            semantic_parameter: Color::from_hex("#268bd2"), // blue
            semantic_property: Color::from_hex("#2aa198"), // cyan
            semantic_namespace: Color::from_hex("#b58900"), // yellow
            semantic_enum_member: Color::from_hex("#cb4b16"), // orange
            semantic_interface: Color::from_hex("#b58900"), // yellow
            semantic_type_parameter: Color::from_hex("#b58900"),
            semantic_decorator: Color::from_hex("#6c71c4"), // violet
            semantic_macro: Color::from_hex("#d33682"),     // magenta

            breadcrumb_bg: Color::from_hex("#073642"),
            breadcrumb_fg: Color::from_hex("#586e75"),
            breadcrumb_active_fg: Color::from_hex("#93a1a1"),

            indent_guide_fg: Color::from_hex("#073642"),
            indent_guide_active_fg: Color::from_hex("#0d4a5a"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: Color::from_hex("#0d4a5a"),

            explorer_dir_fg: Color::from_hex("#268bd2"), // solarized blue
            explorer_file_fg: Color::from_hex("#93a1a1"), // solarized base1
            explorer_active_bg: Color::from_hex("#0a4050"), // current-file tint

            scrollbar_thumb: Color::from_hex("#586e75"),
            scrollbar_track: Color::from_hex("#002b36"),
            terminal_bg: Color::from_hex("#002b36"),
            activity_bar_fg: Color::from_hex("#93a1a1"),
        }
    }

    /// VSCode Dark+ colour scheme.
    pub fn vscode_dark() -> Self {
        let bg = Color::from_hex("#1e1e1e");
        Self {
            background: bg,
            active_background: Color::from_hex("#252526"),
            foreground: Color::from_hex("#d4d4d4"),

            keyword: Color::from_hex("#569cd6"), // blue (storage: let, fn, struct)
            control_flow: Color::from_hex("#c586c0"), // purple (if, else, for, return)
            string_lit: Color::from_hex("#ce9178"), // salmon
            comment: Color::from_hex("#6a9955"), // green
            function: Color::from_hex("#dcdcaa"), // yellow
            type_name: Color::from_hex("#4ec9b0"), // teal
            variable: Color::from_hex("#9cdcfe"), // light blue
            number: Color::from_hex("#b5cea8"),  // light green
            operator: Color::from_hex("#d4d4d4"),
            punctuation: Color::from_hex("#d4d4d4"),
            macro_call: Color::from_hex("#dcdcaa"),
            attribute: Color::from_hex("#4ec9b0"),
            lifetime: Color::from_hex("#569cd6"),
            constant: Color::from_hex("#4fc1ff"),
            escape: Color::from_hex("#d7ba7d"),
            boolean: Color::from_hex("#569cd6"),
            property: Color::from_hex("#9cdcfe"),
            parameter: Color::from_hex("#9cdcfe"),
            module: Color::from_hex("#4ec9b0"),
            default_fg: Color::from_hex("#d4d4d4"),

            selection: Color::from_hex("#264f78"),
            selection_alpha: 0.6,

            cursor: Color::from_hex("#aeafad"),
            cursor_normal_alpha: 0.6,

            search_match_bg: Color::from_hex("#515c6a"),
            search_current_match_bg: Color::from_hex("#613214"),
            search_match_fg: Color::from_hex("#d4d4d4"),

            tab_bar_bg: Color::from_hex("#252526"),
            tab_active_bg: Color::from_hex("#1e1e1e"),
            tab_active_fg: Color::from_hex("#ffffff"),
            tab_inactive_fg: Color::from_hex("#969696"),
            tab_preview_active_fg: Color::from_hex("#cccccc"),
            tab_preview_inactive_fg: Color::from_hex("#7f7f7f"),
            tab_active_accent: Color::from_hex("#007acc"),

            status_bg: Color::from_hex("#007acc"),
            status_fg: Color::from_hex("#ffffff"),

            status_mode_normal_bg: Color::from_hex("#007acc"),
            status_mode_insert_bg: Color::from_hex("#16825d"),
            status_mode_visual_bg: Color::from_hex("#68217a"),
            status_mode_replace_bg: Color::from_hex("#c72e0f"),
            status_inactive_bg: Color::from_hex("#262626"),
            status_inactive_fg: Color::from_hex("#808080"),

            wildmenu_bg: Color::from_hex("#252526"),
            wildmenu_fg: Color::from_hex("#d4d4d4"),
            wildmenu_sel_bg: Color::from_hex("#04395e"),
            wildmenu_sel_fg: Color::from_hex("#ffffff"),

            command_bg: Color::from_hex("#1e1e1e"),
            command_fg: Color::from_hex("#d4d4d4"),

            line_number_fg: Color::from_hex("#858585"),
            line_number_active_fg: Color::from_hex("#c6c6c6"),

            separator: Color::from_hex("#414141"),

            git_added: Color::from_hex("#587c0c"),
            git_modified: Color::from_hex("#0c7d9d"),
            git_deleted: Color::from_hex("#94151b"),

            completion_bg: Color::from_hex("#252526"),
            completion_selected_bg: Color::from_hex("#04395e"),
            completion_fg: Color::from_hex("#d4d4d4"),
            completion_border: Color::from_hex("#454545"),

            diagnostic_error: Color::from_hex("#f14c4c"),
            diagnostic_warning: Color::from_hex("#cca700"),
            diagnostic_info: Color::from_hex("#3794ff"),
            diagnostic_hint: Color::from_hex("#858585"),
            spell_error: Color::from_hex("#4fc1ff"),
            lightbulb: Color::from_hex("#cca700"),

            hover_bg: Color::from_hex("#252526"),
            hover_fg: Color::from_hex("#d4d4d4"),
            hover_border: Color::from_hex("#454545"),

            fuzzy_bg: Color::from_hex("#252526"),
            fuzzy_selected_bg: Color::from_hex("#04395e"),
            fuzzy_fg: Color::from_hex("#d4d4d4"),
            fuzzy_query_fg: Color::from_hex("#0097fb"),
            fuzzy_border: Color::from_hex("#007acc"),
            fuzzy_title_fg: Color::from_hex("#dcdcaa"),
            fuzzy_match_fg: Color::from_hex("#0097fb"),

            // (bg #1e1e1e)
            diff_added_bg: Color::from_hex("#14541a"),
            diff_removed_bg: Color::from_hex("#541a1a"),
            diff_padding_bg: Color::from_hex("#2d2d2d"),

            dap_stopped_bg: Color::from_hex("#3a3000"),

            cursorline_bg: Color::from_hex("#1e1e1e").cursorline_tint(), // derived from background

            yank_highlight_bg: Color::from_hex("#dcdcaa"),
            yank_highlight_alpha: 0.25,

            annotation_fg: Color::from_hex("#858585"),
            ghost_text_fg: Color::from_hex("#5a5a5a"),

            md_heading1: Color::from_hex("#dcdcaa"),
            md_heading2: Color::from_hex("#569cd6"),
            md_heading3: Color::from_hex("#c586c0"),
            md_code: Color::from_hex("#ce9178"),
            md_link: Color::from_hex("#3794ff"),

            sidebar_sel_bg: Color::from_hex("#04395e"), // focused: visible blue highlight
            sidebar_sel_bg_inactive: Color::from_hex("#2a2d2e"),
            semantic_parameter: Color::from_hex("#9cdcfe"), // light blue
            semantic_property: Color::from_hex("#9cdcfe"),  // light blue
            semantic_namespace: Color::from_hex("#4ec9b0"), // teal
            semantic_enum_member: Color::from_hex("#4fc1ff"), // bright blue
            semantic_interface: Color::from_hex("#4ec9b0"), // teal
            semantic_type_parameter: Color::from_hex("#4ec9b0"),
            semantic_decorator: Color::from_hex("#dcdcaa"), // yellow
            semantic_macro: Color::from_hex("#dcdcaa"),     // yellow

            breadcrumb_bg: Color::from_hex("#1e1e1e"),
            breadcrumb_fg: Color::from_hex("#858585"),
            breadcrumb_active_fg: Color::from_hex("#d4d4d4"),

            indent_guide_fg: Color::from_hex("#404040"),
            indent_guide_active_fg: Color::from_hex("#707070"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: Color::from_hex("#3a3d41"),

            explorer_dir_fg: Color::from_hex("#dcdcaa"), // warm yellow (like function names)
            explorer_file_fg: Color::from_hex("#bbbbbb"), // VSCode default sidebar fg
            explorer_active_bg: Color::from_hex("#2a2d3e"), // current-file tint

            scrollbar_thumb: Color::from_hex("#5a5a5a"),
            scrollbar_track: Color::from_hex("#1e1e1e"),
            terminal_bg: Color::from_hex("#1e1e1e"),
            activity_bar_fg: Color::from_hex("#c8c8d2"),
        }
    }

    /// VS Code Light+ (Default Light+) colour scheme.
    pub fn vscode_light() -> Self {
        let bg = Color::from_hex("#ffffff");
        Self {
            background: bg,
            active_background: Color::from_hex("#f3f3f3"),
            foreground: Color::from_hex("#333333"),

            keyword: Color::from_hex("#0000ff"), // blue (storage)
            control_flow: Color::from_hex("#af00db"), // purple (if, else, for, return)
            string_lit: Color::from_hex("#a31515"), // red
            comment: Color::from_hex("#008000"), // green
            function: Color::from_hex("#795e26"), // brown
            type_name: Color::from_hex("#267f99"), // teal
            variable: Color::from_hex("#001080"), // dark blue
            number: Color::from_hex("#098658"),  // green
            operator: Color::from_hex("#333333"),
            punctuation: Color::from_hex("#333333"),
            macro_call: Color::from_hex("#795e26"),
            attribute: Color::from_hex("#267f99"),
            lifetime: Color::from_hex("#0000ff"),
            constant: Color::from_hex("#0070c1"),
            escape: Color::from_hex("#ee0000"),
            boolean: Color::from_hex("#0000ff"),
            property: Color::from_hex("#001080"),
            parameter: Color::from_hex("#001080"),
            module: Color::from_hex("#267f99"),
            default_fg: Color::from_hex("#333333"),

            selection: Color::from_hex("#add6ff"),
            selection_alpha: 0.6,

            cursor: Color::from_hex("#000000"),
            cursor_normal_alpha: 0.6,

            search_match_bg: Color::from_hex("#e8be5a"),
            search_current_match_bg: Color::from_hex("#a8ac94"),
            search_match_fg: Color::from_hex("#000000"),

            tab_bar_bg: Color::from_hex("#ececec"),
            tab_active_bg: Color::from_hex("#ffffff"),
            tab_active_fg: Color::from_hex("#333333"),
            tab_inactive_fg: Color::from_hex("#8e8e8e"),
            tab_preview_active_fg: Color::from_hex("#555555"),
            tab_preview_inactive_fg: Color::from_hex("#999999"),
            tab_active_accent: Color::from_hex("#005fb8"),

            status_bg: Color::from_hex("#007acc"),
            status_fg: Color::from_hex("#ffffff"),

            status_mode_normal_bg: Color::from_hex("#007acc"),
            status_mode_insert_bg: Color::from_hex("#16825d"),
            status_mode_visual_bg: Color::from_hex("#68217a"),
            status_mode_replace_bg: Color::from_hex("#c72e0f"),
            status_inactive_bg: Color::from_hex("#e0e0e0"),
            status_inactive_fg: Color::from_hex("#666666"),

            wildmenu_bg: Color::from_hex("#f3f3f3"),
            wildmenu_fg: Color::from_hex("#333333"),
            wildmenu_sel_bg: Color::from_hex("#0060c0"),
            wildmenu_sel_fg: Color::from_hex("#ffffff"),

            command_bg: Color::from_hex("#ffffff"),
            command_fg: Color::from_hex("#333333"),

            line_number_fg: Color::from_hex("#237893"),
            line_number_active_fg: Color::from_hex("#0b216f"),

            separator: Color::from_hex("#d4d4d4"),

            git_added: Color::from_hex("#48985e"),
            git_modified: Color::from_hex("#2090d0"),
            git_deleted: Color::from_hex("#e51400"),

            completion_bg: Color::from_hex("#f3f3f3"),
            completion_selected_bg: Color::from_hex("#0060c0"),
            completion_fg: Color::from_hex("#333333"),
            completion_border: Color::from_hex("#c8c8c8"),

            diagnostic_error: Color::from_hex("#e51400"),
            diagnostic_warning: Color::from_hex("#bf8803"),
            diagnostic_info: Color::from_hex("#1a85ff"),
            diagnostic_hint: Color::from_hex("#6c6c6c"),
            spell_error: Color::from_hex("#1a85ff"),
            lightbulb: Color::from_hex("#ddb100"),

            hover_bg: Color::from_hex("#f3f3f3"),
            hover_fg: Color::from_hex("#333333"),
            hover_border: Color::from_hex("#c8c8c8"),

            fuzzy_bg: Color::from_hex("#ffffff"),
            fuzzy_selected_bg: Color::from_hex("#0060c0"),
            fuzzy_fg: Color::from_hex("#333333"),
            fuzzy_query_fg: Color::from_hex("#0066bf"),
            fuzzy_border: Color::from_hex("#007acc"),
            fuzzy_title_fg: Color::from_hex("#795e26"),
            fuzzy_match_fg: Color::from_hex("#0066bf"),

            diff_added_bg: Color::from_hex("#dfffdf"),
            diff_removed_bg: Color::from_hex("#ffdede"),
            diff_padding_bg: Color::from_hex("#f0f0f0"),

            dap_stopped_bg: Color::from_hex("#ffffcc"),

            cursorline_bg: Color::from_hex("#ffffff").cursorline_tint(), // derived from background

            yank_highlight_bg: Color::from_hex("#795e26"),
            yank_highlight_alpha: 0.2,

            annotation_fg: Color::from_hex("#8e8e8e"),
            ghost_text_fg: Color::from_hex("#b0b0b0"),

            md_heading1: Color::from_hex("#795e26"),
            md_heading2: Color::from_hex("#0000ff"),
            md_heading3: Color::from_hex("#af00db"),
            md_code: Color::from_hex("#a31515"),
            md_link: Color::from_hex("#0066bf"),

            sidebar_sel_bg: Color::from_hex("#b4d9ff"), // focused: visible blue highlight
            sidebar_sel_bg_inactive: Color::from_hex("#e4e6f1"),
            semantic_parameter: Color::from_hex("#001080"), // dark blue
            semantic_property: Color::from_hex("#001080"),  // dark blue
            semantic_namespace: Color::from_hex("#267f99"), // teal
            semantic_enum_member: Color::from_hex("#0070c1"), // blue
            semantic_interface: Color::from_hex("#267f99"), // teal
            semantic_type_parameter: Color::from_hex("#267f99"),
            semantic_decorator: Color::from_hex("#795e26"), // brown
            semantic_macro: Color::from_hex("#795e26"),     // brown

            breadcrumb_bg: Color::from_hex("#ffffff"),
            breadcrumb_fg: Color::from_hex("#8e8e8e"),
            breadcrumb_active_fg: Color::from_hex("#333333"),

            indent_guide_fg: Color::from_hex("#d3d3d3"),
            indent_guide_active_fg: Color::from_hex("#939393"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: Color::from_hex("#dddddd"),

            explorer_dir_fg: Color::from_hex("#795e26"), // warm brown dirs
            explorer_file_fg: Color::from_hex("#3b3b3b"), // VSCode light sidebar fg
            explorer_active_bg: Color::from_hex("#dce5f0"), // current-file tint

            scrollbar_thumb: Color::from_hex("#b0b0b0"),
            scrollbar_track: Color::from_hex("#f3f3f3"),
            terminal_bg: Color::from_hex("#ffffff"),
            activity_bar_fg: Color::from_hex("#646e6e"),
        }
    }

    /// Return a theme by name. Falls back to `onedark` for unknown names.
    pub fn from_name(name: &str) -> Self {
        match name {
            "gruvbox" | "gruvbox-dark" => Self::gruvbox_dark(),
            "tokyo-night" | "tokyonight" => Self::tokyo_night(),
            "solarized" | "solarized-dark" => Self::solarized_dark(),
            "vscode-dark" | "vscode" | "dark+" => Self::vscode_dark(),
            "vscode-light" | "light+" => Self::vscode_light(),
            "onedark" => Self::onedark(),
            _ => {
                // Try loading a VSCode theme from ~/.config/vimcode/themes/
                if let Some(theme) = Self::load_vscode_theme(name) {
                    theme
                } else {
                    Self::onedark()
                }
            }
        }
    }

    /// Returns `true` when the theme has a light background (relative luminance > 0.5).
    pub fn is_light(&self) -> bool {
        let (r, g, b) = (
            self.background.r as f64 / 255.0,
            self.background.g as f64 / 255.0,
            self.background.b as f64 / 255.0,
        );
        // Perceptual luminance (sRGB)
        0.299 * r + 0.587 * g + 0.114 * b > 0.5
    }

    /// Return the list of all built-in theme names.
    pub fn available_names() -> Vec<String> {
        let mut names: Vec<String> = vec![
            "onedark".into(),
            "gruvbox-dark".into(),
            "tokyo-night".into(),
            "solarized-dark".into(),
            "vscode-dark".into(),
            "vscode-light".into(),
        ];
        // Append custom VSCode themes from ~/.config/vimcode/themes/
        if let Some(dir) = Self::themes_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            names.push(stem.to_string());
                        }
                    }
                }
            }
        }
        names
    }

    /// The directory where custom VSCode theme JSON files are stored.
    fn themes_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config/vimcode/themes"))
    }

    /// Try to load a VSCode-format `.json` theme file by name.
    /// Looks in `~/.config/vimcode/themes/<name>.json`.
    pub fn load_vscode_theme(name: &str) -> Option<Self> {
        let dir = Self::themes_dir()?;
        let path = dir.join(format!("{name}.json"));
        Self::from_vscode_json(&path)
    }

    /// Parse a VSCode theme JSON file and map its colours to a `Theme`.
    /// Falls back to OneDark defaults for any missing keys.
    pub fn from_vscode_json(path: &std::path::Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        // VSCode themes often have comments — strip them
        let data = strip_json_comments(&data);
        let val: serde_json::Value = serde_json::from_str(&data).ok()?;
        let colors = val.get("colors");
        let token_colors = val.get("tokenColors");

        // Start from OneDark and override what the theme provides
        let mut theme = Self::onedark();

        // Helper: get a color from the "colors" object
        let color = |key: &str| -> Option<Color> {
            colors?.get(key)?.as_str().and_then(Color::try_from_hex)
        };

        // ── Editor core ───────────────────────────────────────────────────
        if let Some(c) = color("editor.background") {
            theme.background = c;
            theme.active_background = c.lighten(0.02);
            theme.command_bg = c;
            theme.cursorline_bg = c.cursorline_tint();
        }
        if let Some(c) = color("editor.foreground") {
            theme.foreground = c;
            theme.default_fg = c;
            theme.command_fg = c;
        }

        // ── Selection / cursor ────────────────────────────────────────────
        if let Some(c) = color("editor.selectionBackground") {
            theme.selection = c;
        }
        if let Some(c) = color("editorCursor.foreground") {
            theme.cursor = c;
        }

        // ── Cursor line highlight ─────────────────────────────────────────
        if let Some(c) = color("editor.lineHighlightBackground") {
            theme.cursorline_bg = c;
        }
        if let Some(c) = color("editorRuler.foreground") {
            theme.colorcolumn_bg = c;
        }

        // ── Search ────────────────────────────────────────────────────────
        if let Some(c) = color("editor.findMatchBackground") {
            theme.search_current_match_bg = c;
        }
        if let Some(c) = color("editor.findMatchHighlightBackground") {
            theme.search_match_bg = c;
        }

        // ── Line numbers ──────────────────────────────────────────────────
        if let Some(c) = color("editorLineNumber.foreground") {
            theme.line_number_fg = c;
        }
        if let Some(c) = color("editorLineNumber.activeForeground") {
            theme.line_number_active_fg = c;
        }

        // ── Tab bar ───────────────────────────────────────────────────────
        if let Some(c) = color("editorGroupHeader.tabsBackground") {
            theme.tab_bar_bg = c;
        }
        if let Some(c) = color("tab.activeBackground") {
            theme.tab_active_bg = c;
        }
        if let Some(c) = color("tab.activeForeground") {
            theme.tab_active_fg = c;
        }
        if let Some(c) = color("tab.inactiveForeground") {
            theme.tab_inactive_fg = c;
            theme.tab_preview_inactive_fg = c.darken(0.3);
            theme.tab_preview_active_fg = c.lighten(0.2);
        }
        if let Some(c) = color("tab.activeBorderTop") {
            theme.tab_active_accent = c;
        }

        // ── Status bar ────────────────────────────────────────────────────
        if let Some(c) = color("statusBar.background") {
            theme.status_bg = c;
        }
        if let Some(c) = color("statusBar.foreground") {
            theme.status_fg = c;
        }

        // ── Wildmenu (derive from status bar) ─────────────────────────────
        if let Some(c) = color("statusBar.background") {
            theme.wildmenu_bg = c;
        }
        if let Some(c) = color("statusBar.foreground") {
            theme.wildmenu_fg = c;
        }

        // ── Separator ─────────────────────────────────────────────────────
        if let Some(c) = color("editorGroup.border") {
            theme.separator = c;
        }

        // ── Widgets (completion, hover, fuzzy) ────────────────────────────
        if let Some(c) = color("editorWidget.background") {
            theme.completion_bg = c;
            theme.hover_bg = c;
            theme.fuzzy_bg = c;
        }
        if let Some(c) = color("editorWidget.border") {
            theme.completion_border = c;
            theme.hover_border = c;
            theme.fuzzy_border = c;
        }
        if let Some(c) = color("editorSuggestWidget.selectedBackground") {
            theme.completion_selected_bg = c;
            theme.fuzzy_selected_bg = c;
        }
        if let Some(c) = color("editorWidget.foreground").or_else(|| color("editor.foreground")) {
            theme.completion_fg = c;
            theme.hover_fg = c;
            theme.fuzzy_fg = c;
        }

        // ── Sidebar ──────────────────────────────────────────────────────
        if let Some(c) = color("list.activeSelectionBackground") {
            theme.sidebar_sel_bg = c;
        }
        if let Some(c) = color("list.inactiveSelectionBackground") {
            theme.sidebar_sel_bg_inactive = c;
            theme.explorer_active_bg = c;
        }
        if let Some(c) = color("sideBar.foreground") {
            theme.explorer_file_fg = c;
        }

        // ── Scrollbar / terminal / activity bar ─────────────────────────
        if let Some(c) = color("scrollbarSlider.background") {
            theme.scrollbar_thumb = c;
            // VSCode doesn't have a separate track colour; derive from background
            theme.scrollbar_track = theme.background;
        }
        if let Some(c) = color("terminal.background") {
            theme.terminal_bg = c;
        }
        if let Some(c) = color("activityBar.foreground") {
            theme.activity_bar_fg = c;
        }

        // ── Breadcrumbs ──────────────────────────────────────────────────
        if let Some(c) = color("breadcrumb.background") {
            theme.breadcrumb_bg = c;
        }
        if let Some(c) = color("breadcrumb.foreground") {
            theme.breadcrumb_fg = c;
        }
        if let Some(c) = color("breadcrumb.focusForeground")
            .or_else(|| color("breadcrumb.activeSelectionForeground"))
        {
            theme.breadcrumb_active_fg = c;
        }

        // ── Git gutter ────────────────────────────────────────────────────
        if let Some(c) = color("editorGutter.addedBackground")
            .or_else(|| color("gitDecoration.addedResourceForeground"))
        {
            theme.git_added = c;
        }
        if let Some(c) = color("editorGutter.modifiedBackground")
            .or_else(|| color("gitDecoration.modifiedResourceForeground"))
        {
            theme.git_modified = c;
        }
        if let Some(c) = color("editorGutter.deletedBackground")
            .or_else(|| color("gitDecoration.deletedResourceForeground"))
        {
            theme.git_deleted = c;
        }

        // ── Diagnostics ──────────────────────────────────────────────────
        if let Some(c) = color("editorError.foreground") {
            theme.diagnostic_error = c;
        }
        if let Some(c) = color("editorWarning.foreground") {
            theme.diagnostic_warning = c;
        }
        if let Some(c) = color("editorInfo.foreground") {
            theme.diagnostic_info = c;
        }
        if let Some(c) = color("editorHint.foreground") {
            theme.diagnostic_hint = c;
        }
        if let Some(c) = color("editorSpellChecker.foreground") {
            theme.spell_error = c;
        }

        // ── Diff ─────────────────────────────────────────────────────────
        // Alpha-blend diff backgrounds against the editor background so that
        // `#rrggbbaa` values (common in VSCode themes) produce correct results.
        if let Some(s) = colors
            .and_then(|c| c.get("diffEditor.insertedTextBackground"))
            .and_then(|v| v.as_str())
        {
            if let Some(c) = Color::try_from_hex_over(s, theme.background) {
                theme.diff_added_bg = c;
            }
        }
        if let Some(s) = colors
            .and_then(|c| c.get("diffEditor.removedTextBackground"))
            .and_then(|v| v.as_str())
        {
            if let Some(c) = Color::try_from_hex_over(s, theme.background) {
                theme.diff_removed_bg = c;
            }
        }

        // ── Annotations / ghost text ─────────────────────────────────────
        if let Some(c) = color("editorGhostText.foreground") {
            theme.ghost_text_fg = c;
        }

        // ── Token colours (syntax highlighting) ──────────────────────────
        if let Some(tc) = token_colors.and_then(|v| v.as_array()) {
            for entry in tc {
                let settings = match entry.get("settings") {
                    Some(s) => s,
                    None => continue,
                };
                let fg = settings
                    .get("foreground")
                    .and_then(|v| v.as_str())
                    .and_then(Color::try_from_hex);
                let fg = match fg {
                    Some(c) => c,
                    None => continue,
                };
                let scopes = match entry.get("scope") {
                    Some(serde_json::Value::String(s)) => {
                        s.split(',').map(|s| s.trim()).collect::<Vec<_>>()
                    }
                    Some(serde_json::Value::Array(arr)) => {
                        arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                    }
                    _ => continue,
                };
                for scope in &scopes {
                    match *scope {
                        "keyword" | "storage" | "storage.type" | "storage.modifier" => {
                            theme.keyword = fg;
                        }
                        "keyword.control"
                        | "keyword.control.flow"
                        | "keyword.control.conditional"
                        | "keyword.control.loop"
                        | "keyword.control.trycatch"
                        | "keyword.control.import" => {
                            theme.control_flow = fg;
                        }
                        "string"
                        | "string.quoted"
                        | "string.quoted.double"
                        | "string.quoted.single" => {
                            theme.string_lit = fg;
                        }
                        "comment" | "comment.line" | "comment.block" => {
                            theme.comment = fg;
                            theme.annotation_fg = fg;
                        }
                        "entity.name.function" | "support.function" | "meta.function-call" => {
                            theme.function = fg;
                        }
                        "entity.name.type"
                        | "support.type"
                        | "support.class"
                        | "entity.name.class"
                        | "entity.name.type.class" => {
                            theme.type_name = fg;
                            theme.semantic_namespace = fg;
                            theme.semantic_interface = fg;
                            theme.semantic_type_parameter = fg;
                        }
                        "variable" | "variable.other" | "variable.language" => {
                            theme.variable = fg;
                        }
                        "constant.numeric"
                        | "constant.numeric.integer"
                        | "constant.numeric.float" => {
                            theme.number = fg;
                        }
                        "entity.name.tag" => {
                            theme.semantic_decorator = fg;
                        }
                        "variable.parameter" | "variable.parameter.function" => {
                            theme.semantic_parameter = fg;
                        }
                        "variable.other.property" | "support.type.property-name" => {
                            theme.semantic_property = fg;
                        }
                        "variable.other.enummember" | "constant.other.enum" => {
                            theme.semantic_enum_member = fg;
                        }
                        "entity.name.function.macro" | "support.function.macro" => {
                            theme.semantic_macro = fg;
                            theme.macro_call = fg;
                        }
                        "keyword.operator"
                        | "keyword.operator.expression"
                        | "keyword.operator.logical" => {
                            theme.operator = fg;
                        }
                        "punctuation"
                        | "punctuation.definition"
                        | "punctuation.bracket"
                        | "punctuation.separator" => {
                            theme.punctuation = fg;
                        }
                        "entity.other.attribute-name" | "meta.attribute" => {
                            theme.attribute = fg;
                        }
                        "storage.modifier.lifetime" | "punctuation.definition.lifetime" => {
                            theme.lifetime = fg;
                        }
                        "constant" | "constant.language" | "constant.other" => {
                            theme.constant = fg;
                            theme.boolean = fg;
                        }
                        "constant.character.escape" => {
                            theme.escape = fg;
                        }
                        "entity.name.namespace" | "entity.name.module" => {
                            theme.module = fg;
                        }
                        _ => {}
                    }
                }
            }
        }

        // ── Derive remaining colours from the base palette ───────────────
        // Fuzzy finder query/title inherit from syntax colours if not set
        theme.fuzzy_query_fg = theme.function;
        theme.fuzzy_title_fg = theme.type_name;

        // Markdown headings from syntax palette
        theme.md_heading1 = theme.type_name;
        theme.md_heading2 = theme.function;
        theme.md_heading3 = theme.keyword;
        theme.md_code = theme.string_lit;
        theme.md_link = theme.function;

        Some(theme)
    }

    /// Return the foreground colour for a Tree-sitter scope name.
    pub fn scope_color(&self, scope: &str) -> Color {
        match scope {
            "keyword" => self.keyword,
            "keyword.control" => self.control_flow,
            "operator" => self.operator,
            "string" => self.string_lit,
            "comment" => self.comment,
            "function" | "function.call" | "method" | "method.call" => self.function,
            "type" | "class" | "struct" | "enum" | "interface" => self.type_name,
            "variable" => self.variable,
            "number" => self.number,
            "boolean" => self.boolean,
            "constant" => self.constant,
            "punctuation"
            | "punctuation.bracket"
            | "punctuation.delimiter"
            | "punctuation.special" => self.punctuation,
            "macro" | "macro_call" => self.macro_call,
            "attribute" => self.attribute,
            "lifetime" => self.lifetime,
            "escape" => self.escape,
            "module" | "namespace" => self.module,
            "parameter" => self.parameter,
            "property" | "field" => self.property,
            _ => self.default_fg,
        }
    }

    /// Map an LSP semantic token type + modifiers to a style.
    /// Returns `None` for unknown/unmapped token types (preserves tree-sitter coloring).
    pub fn semantic_token_style(&self, token_type: &str, modifiers: &[String]) -> Option<Style> {
        let fg = match token_type {
            "parameter" => self.semantic_parameter,
            "property" => self.semantic_property,
            "namespace" => self.semantic_namespace,
            "enumMember" => self.semantic_enum_member,
            "interface" => self.semantic_interface,
            "typeParameter" => self.semantic_type_parameter,
            "decorator" => self.semantic_decorator,
            "macro" => self.semantic_macro,
            // Reuse existing syntax colors for standard token types
            "keyword" | "modifier" => {
                // rust-analyzer sends "controlFlow" modifier for if/else/for/while/return etc.
                if modifiers.iter().any(|m| m == "controlFlow") {
                    self.control_flow
                } else {
                    self.keyword
                }
            }
            "function" | "method" => self.function,
            "type" | "class" | "struct" | "enum" => self.type_name,
            "variable" => self.variable,
            "string" | "regexp" => self.string_lit,
            "comment" => self.comment,
            "number" => self.number,
            "operator" => self.operator,
            "boolean" => self.boolean,
            "lifetime" => self.lifetime,
            "attribute" | "attributeBracket" => self.attribute,
            "builtinType" => self.type_name,
            _ => return None,
        };
        let bold = modifiers
            .iter()
            .any(|m| m == "declaration" || m == "definition");
        let italic = modifiers
            .iter()
            .any(|m| m == "readonly" || m == "static" || m == "deprecated");
        Some(Style {
            fg,
            bg: None,
            bold,
            italic,
            font_scale: 1.0,
        })
    }
}

// ─── build_screen_layout ──────────────────────────────────────────────────────

/// Build a complete `ScreenLayout` from current engine state.
///
/// # Parameters
/// - `engine` — current editor state (no GTK types)
/// - `theme` — colour scheme
/// - `window_rects` — pixel-space rects for each window in the current tab,
///   as returned by `engine.calculate_group_window_rects()`
/// - `line_height` — pixel height of one text line (from Pango font metrics)
/// - `char_width` — pixel width of one character (from Pango font metrics),
///   used to compute gutter width
///
/// This function is intentionally *pure* — no side effects, no GTK/Cairo calls.
pub fn build_screen_layout(
    engine: &Engine,
    theme: &Theme,
    window_rects: &[(WindowId, WindowRect)],
    line_height: f64,
    char_width: f64,
    color_headings: bool,
) -> ScreenLayout {
    let active_window_id = engine.active_window_id();
    let multi_window = engine.windows.len() > 1;

    let tab_bar = build_tab_bar(engine);

    let per_window_status = engine.settings.window_status_line;
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    // When status_line_above_terminal is OFF and the terminal is open, extract the
    // active window's status into a separated bar rendered above the terminal.
    // When the setting is ON (default), per-window status bars stay inside each
    // window — they're naturally above the terminal by being part of the editor area.
    let separate_status =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open;

    // Window-split dividers (#582) — independent of the `n >= 2` editor-group
    // check below, since `:split`/`:vsplit` panes exist within a single group.
    let window_dividers = engine.calculate_window_dividers(window_rects);

    let windows = window_rects
        .iter()
        .map(|(window_id, rect)| {
            let mut visible_lines = (rect.height / line_height).floor() as usize;
            if per_window_status && !separate_status && visible_lines > 1 {
                visible_lines -= 1; // reserve bottom row for per-window status bar
            }
            let is_active = *window_id == active_window_id;
            let mut rw = build_rendered_window(
                engine,
                theme,
                *window_id,
                rect,
                visible_lines,
                char_width,
                is_active,
                multi_window,
                color_headings,
            );
            if per_window_status && !separate_status {
                rw.status_line = Some(build_window_status_line(
                    engine, theme, *window_id, is_active,
                ));
            }
            engine
                .paint_viewport_cols
                .borrow_mut()
                .insert(*window_id, rw.text_viewport_cols);
            rw
        })
        .collect();

    let separated_status_line = if separate_status {
        Some(build_window_status_line(
            engine,
            theme,
            active_window_id,
            true,
        ))
    } else {
        None
    };

    let global_status_bar = if per_window_status {
        None
    } else {
        Some(build_global_status_bar(engine, theme))
    };
    let command = build_command_line(engine);

    let wildmenu = if engine.wildmenu_items.is_empty() {
        None
    } else {
        // For argument completions (e.g. "set wrap"), display only the last word
        let display_items: Vec<String> = engine
            .wildmenu_items
            .iter()
            .map(|item| {
                item.rsplit_once(' ')
                    .map(|(_, arg)| arg.to_string())
                    .unwrap_or_else(|| item.clone())
            })
            .collect();
        Some(WildmenuData {
            items: display_items,
            selected: engine.wildmenu_selected,
        })
    };

    let completion = engine.completion_idx.map(|idx| {
        let max_width = engine
            .completion_candidates
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or(0);
        CompletionMenu {
            candidates: engine.completion_candidates.clone(),
            selected_idx: idx,
            max_width,
        }
    });

    let hover = engine.lsp_hover_text.as_ref().map(|text| HoverPopup {
        text: text.clone(),
        anchor_line: engine.view().cursor.line,
        anchor_col: engine.view().cursor.col,
    });

    let quickfix = (engine.quickfix_open && !engine.quickfix_items.is_empty()).then(|| {
        let items = engine
            .quickfix_items
            .iter()
            .map(|m| {
                let f = m.file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                let snippet: String = m.line_text.trim().chars().take(80).collect();
                format!("{}:{}: {}", f, m.line + 1, snippet)
            })
            .collect();
        QuickfixPanel {
            items,
            selected_idx: engine.quickfix_selected,
            total_items: engine.quickfix_items.len(),
            has_focus: engine.quickfix_has_focus,
        }
    });

    let signature_help = engine
        .lsp_signature_help
        .as_ref()
        .map(|sh: &SignatureHelpData| SignatureHelp {
            label: sh.label.clone(),
            params: sh.params.clone(),
            active_param: sh.active_param,
            anchor_line: engine.view().cursor.line,
            anchor_col: engine.view().cursor.col,
        });

    let menu_bar_visible = engine.menu_bar_visible;
    let menu_dropdown_open = engine.menu_system.borrow().is_open();

    let debug_toolbar = engine.debug_toolbar_visible.then(|| DebugToolbarData {
        buttons: DEBUG_BUTTONS.to_vec(),
        session_active: engine.dap_session_active,
    });

    // Build the debug sidebar data (always present).
    let debug_sidebar = {
        let selected = engine.dap_sidebar_selected;
        let active_section = engine.dap_sidebar_section;

        // Variables section: flat tree with ▶/▼ prefixes, recursive expansion.
        let mut var_items: Vec<DebugSidebarItem> = Vec::new();
        let mut flat_idx = 0usize;
        #[allow(clippy::too_many_arguments)]
        fn build_var_tree(
            items: &mut Vec<DebugSidebarItem>,
            vars: &[DapVariable],
            depth: u8,
            flat_idx: &mut usize,
            expanded: &std::collections::HashSet<u64>,
            children_map: &std::collections::HashMap<u64, Vec<DapVariable>>,
            active_section: &DebugSidebarSection,
            selected: usize,
        ) {
            for v in vars {
                let prefix = if v.var_ref > 0 {
                    if expanded.contains(&v.var_ref) {
                        icons::EXPAND_DOWN.nerd
                    } else {
                        icons::COLLAPSE_RIGHT.nerd
                    }
                } else {
                    "  "
                };
                items.push(DebugSidebarItem {
                    text: if v.value.is_empty() {
                        format!("{}{}", prefix, v.name)
                    } else {
                        format!("{}{} = {}", prefix, v.name, v.value)
                    },
                    indent: depth,
                    is_selected: *active_section == DebugSidebarSection::Variables
                        && *flat_idx == selected,
                });
                *flat_idx += 1;
                if v.var_ref > 0 && expanded.contains(&v.var_ref) {
                    if let Some(child_vars) = children_map.get(&v.var_ref) {
                        build_var_tree(
                            items,
                            child_vars,
                            depth + 1,
                            flat_idx,
                            expanded,
                            children_map,
                            active_section,
                            selected,
                        );
                    }
                }
            }
        }
        if engine.dap_primary_scope_ref > 0 {
            // Primary scope header (e.g. "▼ Locals").
            let expanded = engine
                .dap_expanded_vars
                .contains(&engine.dap_primary_scope_ref);
            let prefix = if expanded {
                icons::EXPAND_DOWN.nerd
            } else {
                icons::COLLAPSE_RIGHT.nerd
            };
            var_items.push(DebugSidebarItem {
                text: format!("{prefix}{}", engine.dap_primary_scope_name),
                indent: 0,
                is_selected: active_section == DebugSidebarSection::Variables
                    && flat_idx == selected,
            });
            flat_idx += 1;
            if expanded {
                build_var_tree(
                    &mut var_items,
                    &engine.dap_variables,
                    1,
                    &mut flat_idx,
                    &engine.dap_expanded_vars,
                    &engine.dap_child_variables,
                    &active_section,
                    selected,
                );
            }
        } else {
            // No scope info (e.g. tests): show variables at root level.
            build_var_tree(
                &mut var_items,
                &engine.dap_variables,
                0,
                &mut flat_idx,
                &engine.dap_expanded_vars,
                &engine.dap_child_variables,
                &active_section,
                selected,
            );
        }

        // Additional scope groups (e.g. "Statics", "Registers") as expandable headers.
        for (scope_name, var_ref) in &engine.dap_scope_groups {
            let expanded = engine.dap_expanded_vars.contains(var_ref);
            let prefix = if expanded {
                icons::EXPAND_DOWN.nerd
            } else {
                icons::COLLAPSE_RIGHT.nerd
            };
            var_items.push(DebugSidebarItem {
                text: format!("{prefix}{scope_name}"),
                indent: 0,
                is_selected: active_section == DebugSidebarSection::Variables
                    && flat_idx == selected,
            });
            flat_idx += 1;
            if expanded {
                if let Some(child_vars) = engine.dap_child_variables.get(var_ref) {
                    build_var_tree(
                        &mut var_items,
                        child_vars,
                        1,
                        &mut flat_idx,
                        &engine.dap_expanded_vars,
                        &engine.dap_child_variables,
                        &active_section,
                        selected,
                    );
                }
            }
        }

        // Watch section: expressions with their evaluated values.
        let watch_items: Vec<DebugSidebarItem> = engine
            .dap_watch_expressions
            .iter()
            .zip(engine.dap_watch_values.iter())
            .enumerate()
            .map(|(i, (expr, val))| {
                let val_str = val.as_deref().unwrap_or(if engine.dap_session_active {
                    "…"
                } else {
                    "(not running)"
                });
                DebugSidebarItem {
                    text: format!("{expr} = {val_str}"),
                    indent: 0,
                    is_selected: active_section == DebugSidebarSection::Watch && i == selected,
                }
            })
            .collect();

        // Call Stack section.
        let frame_items: Vec<DebugSidebarItem> = engine
            .dap_stack_frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let src = f
                    .source
                    .as_deref()
                    .and_then(|p| std::path::Path::new(p).file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                let prefix = if i == engine.dap_active_frame {
                    icons::COLLAPSE_RIGHT.nerd
                } else {
                    "  "
                };
                DebugSidebarItem {
                    text: format!("{}{} ({}:{})", prefix, f.name, src, f.line),
                    indent: 0,
                    is_selected: active_section == DebugSidebarSection::CallStack && i == selected,
                }
            })
            .collect();

        // Breakpoints section: all breakpoints across all files.
        let mut bp_items: Vec<DebugSidebarItem> = Vec::new();
        let mut sorted_bp: Vec<_> = engine.dap_breakpoints.iter().collect();
        sorted_bp.sort_by_key(|(path, _)| path.as_str());
        let mut bp_global_idx = 0usize;
        for (path, bps) in &sorted_bp {
            let file_name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            for bp in *bps {
                let suffix = if let Some(cond) = &bp.condition {
                    format!(" [if {cond}]")
                } else if let Some(hc) = &bp.hit_condition {
                    format!(" [hits {hc}]")
                } else if let Some(msg) = &bp.log_message {
                    format!(" [log: {msg}]")
                } else {
                    String::new()
                };
                let symbol = if bp.condition.is_some() || bp.hit_condition.is_some() {
                    "\u{25c6}" // ◆ conditional
                } else {
                    icons::DBG_BREAKPOINTS.nerd
                };
                bp_items.push(DebugSidebarItem {
                    text: format!("{} {}:{}{}", symbol, file_name, bp.line, suffix),
                    indent: 0,
                    is_selected: active_section == DebugSidebarSection::Breakpoints
                        && bp_global_idx == selected,
                });
                bp_global_idx += 1;
            }
        }

        // Output lines for the Debug Output tab (up to 200, oldest-first).
        let debug_output_lines: Vec<String> = engine
            .dap_output_lines
            .iter()
            .rev()
            .take(200)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let launch_config_name = engine
            .dap_launch_configs
            .get(engine.dap_selected_launch_config)
            .map(|c| c.name.clone());

        DebugSidebarData {
            session_active: engine.dap_session_active,
            stopped: engine.dap_stopped_thread.is_some(),
            variables: var_items,
            watch: watch_items,
            frames: frame_items,
            breakpoints: bp_items,
            active_section,
            sidebar_selected: selected,
            has_focus: engine.dap_sidebar_has_focus,
            launch_config_name,
            debug_output_lines,
            eval_result: engine.dap_eval_result.clone(),
            scroll_offsets: engine.dap_sidebar_scroll,
            section_heights: engine.dap_sidebar_section_heights,
        }
    };

    // Build bottom panel tabs.
    let terminal = build_terminal_panel(engine);
    let bottom_tabs = BottomPanelTabs {
        active: engine.bottom_panel_kind.clone(),
        output_lines: debug_sidebar.debug_output_lines.clone(),
        terminal,
    };

    // Build Source Control panel data (populated when the panel is visible).
    let source_control = build_source_control_data(engine);

    let tab_switcher = engine.tab_switcher_open.then(|| TabSwitcherPanel {
        items: engine.tab_switcher_items(),
        selected_idx: engine.tab_switcher_selected,
    });

    let n = engine.group_layout.leaf_count();
    // ── Per-group chrome, built uniformly for EVERY group count (#551) ────────
    // `group_tab_bars` and `group_dividers` used to live inside an
    // `if n >= 2 { .. } else { None }` block, which forced every backend to
    // carry a parallel hand-written "exactly one group" draw path beside the
    // generic N-group one. A single group is just a split of one: the same
    // bounding-box math produces the identical full-width tab bar rect, and
    // `GroupLayout::Leaf::dividers()` already returns `vec![]`, so the generic
    // path covers N=1 with no special case. `editor_group_split` below is now
    // only a *marker* for "2 or more groups" (it still gates the hit-test
    // paths that legitimately differ), and no longer the storage for this data
    // — one source of truth, so a single-group calculation can't silently
    // drift from the N-group one the way #547's breadcrumb y-offset did.
    let group_ids = engine.group_layout.group_ids();
    // Compute group bounds from the window_rects: each group's bounds is
    // the bounding box of its windows (the tab bar is drawn just above it).
    let group_tab_bars: Vec<GroupTabBar> = group_ids
        .iter()
        .map(|&gid| {
            let tabs = build_tab_bar_for_group_by_id(engine, gid);
            // Find bounding rect for all windows in this group
            let mut min_x = f64::MAX;
            let mut min_y = f64::MAX;
            let mut max_x = f64::MIN;
            let mut max_y = f64::MIN;
            if let Some(group) = engine.editor_groups.get(&gid) {
                for wr in window_rects {
                    if group.active_tab().layout.window_ids().contains(&wr.0) {
                        min_x = min_x.min(wr.1.x);
                        min_y = min_y.min(wr.1.y);
                        max_x = max_x.max(wr.1.x + wr.1.width);
                        max_y = max_y.max(wr.1.y + wr.1.height);
                    }
                }
            }
            if min_x == f64::MAX {
                min_x = 0.0;
                min_y = 0.0;
                max_x = 0.0;
                max_y = 0.0;
            }
            let bounds = WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y);
            // Populate diff toolbar if this group contains a diff window.
            let diff_toolbar = if engine.is_in_diff_view() {
                if let Some((a, b)) = engine.diff_window_pair {
                    let group = engine.editor_groups.get(&gid);
                    let has_diff_win = group.is_some_and(|g| {
                        let wids = g.active_tab().layout.window_ids();
                        wids.contains(&a) || wids.contains(&b)
                    });
                    if has_diff_win {
                        let (_, total) = engine.diff_unified_regions();
                        let change_label = engine
                            .diff_current_change_index()
                            .map(|(c, t)| format!("{c} of {t}"));
                        Some(DiffToolbarData {
                            change_label,
                            total_changes: total,
                            unchanged_hidden: engine.diff_unchanged_hidden,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let tab_scroll_offset = engine
                .editor_groups
                .get(&gid)
                .map(|g| g.tab_scroll_offset)
                .unwrap_or(0);
            // Hit regions are expressed in char-CELLS so they are
            // backend-neutral. TUI passes char_width=1.0 (bounds already in
            // cells); GTK passes pixel bounds + real char_width, so divide to
            // recover cells. Without this, GTK's right-aligned button regions
            // (split/diff/action) would land at pixel columns and never match
            // a cell-converted click. (#515)
            let bar_width = (bounds.width / char_width).round() as u16;
            let has_diff_toolbar = diff_toolbar.is_some();
            let diff_label_cols = diff_toolbar
                .as_ref()
                .and_then(|dt| dt.change_label.as_ref())
                .map(|l| l.len() as u16 + 1)
                .unwrap_or(0);
            let is_active = gid == engine.active_group;
            let has_split = is_active || engine.is_in_diff_view();
            let hit_regions = compute_tab_bar_hit_regions(
                &tabs,
                tab_scroll_offset,
                bar_width,
                has_diff_toolbar,
                diff_label_cols,
                has_split,
            );
            let accent = if is_active {
                Some(to_quadraui_color(theme.tab_active_accent))
            } else {
                None
            };
            let bar = build_tab_bar_primitive(
                &tabs,
                has_split,
                diff_toolbar.as_ref(),
                tab_scroll_offset,
                accent,
            );
            GroupTabBar {
                group_id: gid,
                tabs,
                bounds,
                diff_toolbar,
                tab_scroll_offset,
                hit_regions,
                bar,
            }
        })
        .collect();
    // Collect dividers — use the total content bounds from window_rects.
    // `GroupLayout::Leaf::dividers()` returns an empty vec, so this is
    // naturally empty in single-group mode (#551).
    let content_bounds = if !window_rects.is_empty() {
        let min_x = window_rects.iter().map(|r| r.1.x).fold(f64::MAX, f64::min);
        let min_y = window_rects
            .iter()
            .map(|r| r.1.y - line_height)
            .fold(f64::MAX, f64::min);
        let max_x = window_rects
            .iter()
            .map(|r| r.1.x + r.1.width)
            .fold(f64::MIN, f64::max);
        let max_y = window_rects
            .iter()
            .map(|r| r.1.y + r.1.height)
            .fold(f64::MIN, f64::max);
        WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    } else {
        WindowRect::new(0.0, 0.0, 0.0, 0.0)
    };
    let group_dividers = engine.group_layout.dividers(content_bounds, &mut 0);
    let editor_group_split = (n >= 2).then_some(EditorGroupSplitData {
        active_group: engine.active_group,
        num_groups: n,
    });

    let ext_sidebar = build_ext_sidebar_data(engine);
    let ai_panel = build_ai_panel_data(engine);

    // Build breadcrumbs for each editor group
    let breadcrumbs = if engine.settings.breadcrumbs {
        let group_ids = engine.group_layout.group_ids();
        group_ids
            .iter()
            .map(|&gid| {
                let segments = build_breadcrumbs_for_group(engine, gid);
                // Compute bounds from the group's windows
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                if let Some(group) = engine.editor_groups.get(&gid) {
                    for wr in window_rects {
                        if group.active_tab().layout.window_ids().contains(&wr.0) {
                            min_x = min_x.min(wr.1.x);
                            min_y = min_y.min(wr.1.y);
                            max_x = max_x.max(wr.1.x + wr.1.width);
                        }
                    }
                }
                if min_x == f64::MAX {
                    min_x = 0.0;
                    min_y = 0.0;
                    max_x = 0.0;
                }
                // Place bounds at the actual breadcrumb row (one line_height
                // above the window content top).
                let bc_y = (min_y - line_height).max(0.0);
                let bounds = WindowRect::new(min_x, bc_y, max_x - min_x, line_height);
                let bar = breadcrumbs_to_quadraui_status_bar(
                    &segments,
                    theme,
                    engine.breadcrumb_focus,
                    engine.breadcrumb_selected,
                );
                BreadcrumbBar {
                    group_id: gid,
                    segments,
                    bounds,
                    bar,
                    draw_layout: std::cell::RefCell::new(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    // Compute diff toolbar for single-group mode (multi-group has it on GroupTabBar).
    let diff_toolbar = if n < 2 && engine.is_in_diff_view() {
        let (_, total) = engine.diff_unified_regions();
        let change_label = engine
            .diff_current_change_index()
            .map(|(c, t)| format!("{c} of {t}"));
        Some(DiffToolbarData {
            change_label,
            total_changes: total,
            unchanged_hidden: engine.diff_unchanged_hidden,
        })
    } else {
        None
    };

    let tab_scroll_offset_single = engine
        .editor_groups
        .get(&engine.active_group)
        .map(|g| g.tab_scroll_offset)
        .unwrap_or(0);
    let tab_bar_primitive = build_tab_bar_primitive(
        &tab_bar,
        true,
        diff_toolbar.as_ref(),
        tab_scroll_offset_single,
        Some(to_quadraui_color(theme.tab_active_accent)),
    );

    // Hit regions for the single-group / active tab bar, in char-cells. The bar
    // spans the full editor content width (bounding box of all window rects);
    // divide by char_width so the result is backend-neutral (TUI char_width=1.0).
    // `has_split_buttons = true` mirrors the `true` passed to build_tab_bar_primitive
    // above. Empty in multi-group mode (handled per-group on each GroupTabBar). (#515)
    let tab_bar_hit_regions = if n >= 2 || window_rects.is_empty() {
        Vec::new()
    } else {
        let min_x = window_rects
            .iter()
            .map(|(_, r)| r.x)
            .fold(f64::MAX, f64::min);
        let max_r = window_rects
            .iter()
            .map(|(_, r)| r.x + r.width)
            .fold(f64::MIN, f64::max);
        let bar_width_cells = ((max_r - min_x) / char_width).round().max(0.0) as u16;
        let diff_label_cols = diff_toolbar
            .as_ref()
            .and_then(|dt| dt.change_label.as_ref())
            .map(|l| l.len() as u16 + 1)
            .unwrap_or(0);
        compute_tab_bar_hit_regions(
            &tab_bar,
            tab_scroll_offset_single,
            bar_width_cells,
            diff_toolbar.is_some(),
            diff_label_cols,
            true,
        )
    };

    ScreenLayout {
        tab_bar,
        tab_bar_hit_regions,
        windows,
        global_status_bar,
        command,
        wildmenu,
        active_window_id,
        completion,
        hover,
        quickfix,
        bottom_tabs,
        signature_help,
        menu_bar_visible,
        menu_dropdown_open,
        debug_toolbar,
        debug_sidebar,
        source_control,
        picker: engine.picker_open.then(|| {
            use crate::core::engine::PickerSource;
            let has_preview = matches!(
                engine.picker_source,
                PickerSource::Files | PickerSource::Grep
            );
            PickerPanel {
                title: engine.picker_title.clone(),
                query: engine.picker_query.clone(),
                items: engine
                    .picker_items
                    .iter()
                    .map(|item| PickerPanelItem {
                        display: item.display.clone(),
                        detail: item.detail.clone(),
                        match_positions: item.match_positions.clone(),
                        depth: item.depth,
                        expandable: item.expandable,
                        expanded: item.expanded,
                    })
                    .collect(),
                selected_idx: engine.picker_selected,
                scroll_top: engine.picker_scroll_top,
                total_count: if engine.picker_source == PickerSource::Grep {
                    engine.picker_items.len()
                } else {
                    engine.picker_all_items.len()
                },
                preview: if has_preview {
                    engine
                        .picker_preview
                        .as_ref()
                        .map(|p| p.lines.clone())
                        .or_else(|| Some(Vec::new()))
                } else {
                    None
                },
                preview_scroll: engine.picker_preview_scroll,
            }
        }),
        tab_switcher,
        editor_group_split,
        group_tab_bars,
        group_dividers,
        window_dividers,
        ext_sidebar,
        ai_panel,
        ext_panel: build_ext_panel_data(engine),
        breadcrumbs,
        diff_peek: engine.diff_peek.as_ref().map(|dp| DiffPeekPopup {
            anchor_line: dp.anchor_line,
            hunk_lines: dp.hunk_lines.clone(),
        }),
        diff_toolbar,
        panel_hover: engine.panel_hover.as_ref().map(|ph| PanelHoverPopupData {
            rendered: ph.rendered.clone(),
            links: ph.links.clone(),
            item_index: ph.item_index,
            panel_name: ph.panel_name.clone(),
        }),
        editor_hover: engine.editor_hover.as_ref().map(|eh| EditorHoverPopupData {
            rendered: eh.rendered.clone(),
            links: eh.links.clone(),
            anchor_line: eh.anchor_line,
            anchor_col: eh.anchor_col,
            scroll_top: eh.scroll_top,
            focused_link: eh.focused_link,
            has_focus: engine.editor_hover_has_focus,
            popup_width: eh.popup_width,
            frozen_scroll_top: eh.frozen_scroll_top,
            frozen_scroll_left: eh.frozen_scroll_left,
            selection: eh.selection.as_ref().map(|s| s.normalized()),
        }),
        dialog: engine.dialog.as_ref().map(|d| DialogPanel {
            title: d.title.clone(),
            body: d.body.clone(),
            buttons: d
                .buttons
                .iter()
                .enumerate()
                .map(|(i, btn)| (format_button_label(&btn.label, btn.hotkey), i == d.selected))
                .collect(),
            input: d.input.as_ref().map(|inp| DialogInputPanel {
                display: if inp.is_password {
                    format!("{}|", "*".repeat(inp.value.len()))
                } else {
                    format!("{}|", inp.value)
                },
            }),
            vertical_buttons: d.tag == "code_actions",
        }),
        context_menu: engine.context_menu.as_ref().map(|cm| ContextMenuPanel {
            items: cm
                .items
                .iter()
                .map(|item| ContextMenuRenderItem {
                    label: item.label.clone(),
                    shortcut: item.shortcut.clone(),
                    separator_after: item.separator_after,
                    enabled: item.enabled,
                })
                .collect(),
            selected_idx: cm.selected,
            screen_col: cm.screen_x,
            screen_row: cm.screen_y,
            trigger_height: cm.trigger_height,
        }),
        find_replace: if engine.find_replace_open {
            let match_info = if engine.search_matches.is_empty() {
                if engine.find_replace_query.is_empty() {
                    String::new()
                } else {
                    "No results".to_string()
                }
            } else {
                match engine.search_index {
                    Some(idx) => format!("{} of {}", idx + 1, engine.search_matches.len()),
                    None => format!("{} matches", engine.search_matches.len()),
                }
            };
            // Compute active group bounds from window rects
            let active_group_bounds = {
                let active_group = &engine.active_group;
                let group_window_ids: Vec<_> = engine
                    .editor_groups
                    .get(active_group)
                    .map(|g| g.active_tab().layout.window_ids())
                    .unwrap_or_default();
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = 0.0f64;
                let mut max_y = 0.0f64;
                for (wid, rect) in window_rects {
                    if group_window_ids.contains(wid) {
                        min_x = min_x.min(rect.x);
                        min_y = min_y.min(rect.y);
                        max_x = max_x.max(rect.x + rect.width);
                        max_y = max_y.max(rect.y + rect.height);
                    }
                }
                if min_x < f64::MAX {
                    WindowRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
                } else {
                    // Fallback: use first window rect or zero
                    window_rects
                        .first()
                        .map(|(_, r)| *r)
                        .unwrap_or_else(|| WindowRect::new(0.0, 0.0, 800.0, 600.0))
                }
            };
            let panel_w = FR_PANEL_WIDTH;
            let (hit_regions, _input_w) = compute_find_replace_hit_regions(
                panel_w,
                engine.find_replace_show_replace,
                &match_info,
            );
            // Convert vimcode's f64 WindowRect to quadraui::Rect (f32).
            let qr = quadraui::Rect::new(
                active_group_bounds.x as f32,
                active_group_bounds.y as f32,
                active_group_bounds.width as f32,
                active_group_bounds.height as f32,
            );
            Some(FindReplacePanel {
                query: engine.find_replace_query.clone(),
                replacement: engine.find_replace_replacement.clone(),
                show_replace: engine.find_replace_show_replace,
                focus: engine.find_replace_focus,
                cursor: engine.find_replace_cursor,
                sel_anchor: engine.find_replace_sel_anchor,
                match_info,
                case_sensitive: engine.find_replace_options.case_sensitive,
                whole_word: engine.find_replace_options.whole_word,
                use_regex: engine.find_replace_options.use_regex,
                preserve_case: engine.find_replace_options.preserve_case,
                in_selection: engine.find_replace_options.in_selection,
                group_bounds: qr,
                panel_width: panel_w,
                replace_one_glyph: crate::icons::FIND_REPLACE.s().to_string(),
                replace_all_glyph: crate::icons::FIND_REPLACE_ALL.s().to_string(),
                hit_regions,
            })
        } else {
            None
        },
        tab_tooltip: engine.tab_hover_tooltip.clone(),
        tab_scroll_offset: tab_scroll_offset_single,
        tab_bar_primitive,
        separated_status_line,
    }
}

fn build_source_control_data(engine: &Engine) -> Option<SourceControlData> {
    // Only populate when the engine has been sc_refresh()ed at least once.
    // We always build it so both GTK and TUI backends can check sc_has_focus.
    let branch = engine
        .git_branch
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());

    let staged: Vec<ScFileItem> = engine
        .sc_file_statuses
        .iter()
        .filter_map(|f| {
            f.staged.map(|s| ScFileItem {
                path: f.path.clone(),
                status_char: s.label(),
                is_staged: true,
            })
        })
        .collect();

    let unstaged: Vec<ScFileItem> = engine
        .sc_file_statuses
        .iter()
        .filter_map(|f| {
            f.unstaged.map(|s| ScFileItem {
                path: f.path.clone(),
                status_char: s.label(),
                is_staged: false,
            })
        })
        .collect();

    let worktrees: Vec<ScWorktreeItem> = engine
        .sc_worktrees
        .iter()
        .map(|wt| ScWorktreeItem {
            path: wt.path.display().to_string(),
            branch: wt.branch.clone().unwrap_or_else(|| "HEAD".to_string()),
            is_current: wt.is_current,
            is_main: wt.is_main,
        })
        .collect();

    let log: Vec<ScLogItem> = engine
        .sc_log
        .iter()
        .map(|e| ScLogItem {
            hash: e.hash.clone(),
            message: e.message.clone(),
        })
        .collect();

    Some(SourceControlData {
        branch,
        ahead: engine.sc_ahead,
        behind: engine.sc_behind,
        staged,
        unstaged,
        worktrees,
        log,
        sections_expanded: engine.sc_sections_expanded,
        selected: engine.sc_selected,
        has_focus: engine.sc_has_focus,
        commit_message: engine.sc_commit_message.clone(),
        commit_cursor: engine.sc_commit_cursor,
        commit_input_active: engine.sc_commit_input_active,
        button_focused: engine.sc_button_focused,
        button_hovered: engine.sc_button_hovered,
        branch_picker: if engine.sc_branch_picker_open {
            let filtered = engine.sc_branch_picker_filtered();
            let results = filtered
                .iter()
                .map(|&(i, _)| {
                    let b = &engine.sc_branch_picker_branches[i];
                    (b.name.clone(), b.is_current)
                })
                .collect();
            Some(BranchPickerData {
                query: engine.sc_branch_picker_query.clone(),
                results,
                selected: engine.sc_branch_picker_selected,
                create_mode: false,
                create_input: String::new(),
            })
        } else if engine.sc_branch_create_mode {
            Some(BranchPickerData {
                query: String::new(),
                results: Vec::new(),
                selected: 0,
                create_mode: true,
                create_input: engine.sc_branch_create_input.clone(),
            })
        } else {
            None
        },
        help_open: engine.sc_help_open,
        sc_sections_start_y: engine
            .sc_panel_layout
            .borrow()
            .as_ref()
            .map(|l| l.content_bounds.y),
    })
}

/// Convert vimcode's internal `Color` to quadraui's `Color`. Alpha is fully opaque.
fn to_q_color(c: Color) -> quadraui::Color {
    quadraui::Color::rgb(c.r, c.g, c.b)
}

/// Build the Source Control action-button row as a `quadraui::Toolbar`
/// (#505). Commit carries its label + `(c)` key hint and is disabled while
/// the commit message is empty; Push/Pull/Sync are icon-only. Button ids
/// come from [`crate::core::engine::SC_BUTTON_IDS`] so click dispatch can
/// map the hit-test result back to a button index. Both backends call this
/// and hand the result to `Backend::draw_toolbar`.
pub fn sc_button_toolbar(sc: &SourceControlData) -> quadraui::Toolbar {
    use crate::core::engine::SC_BUTTON_IDS;
    use crate::icons;
    use quadraui::{Toolbar, ToolbarButton, WidgetId};

    let action = |idx: usize, label: &str, icon: &str, key_hint: Option<&str>, enabled: bool| {
        ToolbarButton::Action {
            id: WidgetId::new(SC_BUTTON_IDS[idx]),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            key_hint: key_hint.map(|s| s.to_string()),
            enabled,
            is_active: false,
            tooltip: String::new(),
        }
    };

    let commit_enabled = !sc.commit_message.trim().is_empty();
    Toolbar {
        id: WidgetId::new("sc:buttons"),
        bg: None,
        focused_index: None,
        buttons: vec![
            action(
                0,
                "Commit",
                icons::GIT_COMMIT.s(),
                Some("c"),
                commit_enabled,
            ),
            action(1, "", icons::GIT_PUSH.s(), None, true),
            action(2, "", icons::GIT_PULL.s(), None, true),
            action(3, "", icons::GIT_SYNC.s(), None, true),
        ],
    }
}

/// Build the SC `SidebarPanel` — a `quadraui::SidebarPanel` wrapping the
/// action-button toolbar as its header slot (#509). `toolbar_height: None`
/// defers to each backend's idiomatic default (1 cell TUI, `line_height` GTK).
pub fn sc_sidebar_panel(sc: &SourceControlData) -> quadraui::SidebarPanel {
    use quadraui::{SidebarPanel, WidgetId};
    SidebarPanel {
        id: WidgetId::new("sc:panel"),
        toolbar: Some(sc_button_toolbar(sc)),
        toolbar_height: None,
    }
}

/// Draw the SC bottom slab (toolbar + sections) through backend `b` and
/// cache the full `SidebarPanelLayout` on `engine` for click/hover dispatch
/// (#509). `rect` covers the "bottom slab" — from just below the commit input
/// to the bottom of the panel. Both backends call this inside their frame
/// scope; section rendering then reads `content_bounds` from the cached
/// layout. Keyboard focus → pressed_id, mouse hover → hovered_id.
pub fn draw_sc_sidebar_panel(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    sc: &SourceControlData,
    rect: quadraui::Rect,
) {
    let panel = sc_sidebar_panel(sc);
    let hovered = sc.button_hovered.and_then(Engine::sc_button_id);
    let pressed = sc.button_focused.and_then(Engine::sc_button_id);
    let layout = b.draw_sidebar_panel(rect, &panel, hovered.as_ref(), pressed.as_ref());
    engine.sc_panel_layout.replace(Some(layout));
}

/// Format the SC panel's header row text: branch name + ahead/behind
/// counts when present. Shared by both backends so the header text can't
/// drift between TUI and GTK renderers (#480).
pub fn sc_header_text(sc: &SourceControlData) -> String {
    if sc.ahead > 0 || sc.behind > 0 {
        format!(
            "  \u{e702} SOURCE CONTROL  {}  \u{2191}{} \u{2193}{}",
            sc.branch, sc.ahead, sc.behind
        )
    } else {
        format!("  \u{e702} SOURCE CONTROL  {}", sc.branch)
    }
}

/// Build the SC panel's header row as a single-segment `quadraui::StatusBar`
/// (#480). GTK paints the header through this — TUI keeps its existing
/// direct `set_cell` text row (both read the same [`sc_header_text`]
/// string, so the two can't show different branch info even though the
/// paint mechanism differs).
pub fn sc_header_status_bar(sc: &SourceControlData, theme: &Theme) -> quadraui::StatusBar {
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("sc:header"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: sc_header_text(sc),
            fg: to_quadraui_color(theme.status_fg),
            bg: to_quadraui_color(theme.status_bg),
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    }
}

/// Number of text rows in the SC commit message (at least 1, even when
/// empty). Shared raw line count — both backends derive their own
/// border/line-height-aware box height from this (#480).
pub fn sc_commit_input_row_count(commit_message: &str) -> u16 {
    commit_message.split('\n').count().max(1) as u16
}

/// Height in *rows* of the SC commit-input box on TUI, including the
/// `TextInput` primitive's 1-cell border on top and bottom (#480). TUI's
/// native unit is one screen cell, so the border costs exactly 2 whole
/// rows — this is the single source of truth shared by TUI's paint code
/// (`panels.rs`) and its click hit-test math (`mouse.rs`), so the two
/// can't drift out of sync the way the pre-migration hand-rolled geometry
/// did. GTK's native unit is pixels, where the same 1-*pixel* border is
/// negligible next to a `line_height` row — GTK computes its box height
/// directly from [`sc_commit_input_row_count`] instead of this function.
pub fn sc_commit_input_box_height(commit_message: &str) -> u16 {
    sc_commit_input_row_count(commit_message) + 2
}

/// The three fixed bands the git ("source control") sidebar stacks inside its
/// content area, top to bottom: the header status bar, the commit-message
/// `TextInput` box, and the toolbar-slab + section list that fills the rest.
///
/// Returned by [`sc_sidebar_bands`] so a backend's *painter* and its *click
/// router* read one derivation instead of two. Both used to inline this
/// arithmetic separately, which is exactly how the pre-#544 GTK click path
/// ended up hit-testing against DrawingArea-local `y` (`0` at the panel top)
/// while the ShellApp painter drew at absolute window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScSidebarBands {
    /// Header row (branch name + summary).
    pub header: quadraui::Rect,
    /// Commit-message input box, including its border.
    pub commit_input: quadraui::Rect,
    /// Everything below: the toolbar slab and the change sections.
    pub slab: quadraui::Rect,
}

/// Split a git-sidebar content rect into its [`ScSidebarBands`].
///
/// `row_height` is one text row in the caller's native unit (pixels on GTK,
/// cells on TUI). `commit_border` is what the `TextInput` primitive's 1-unit
/// border on top *and* bottom costs in that same unit — 2.0 px on GTK, 2.0
/// rows on TUI (see [`sc_commit_input_box_height`], which is the row-unit
/// spelling of the same constant).
pub fn sc_sidebar_bands(
    commit_message: &str,
    rect: quadraui::Rect,
    row_height: f32,
    commit_border: f32,
) -> ScSidebarBands {
    let header_h = row_height;
    let commit_h = sc_commit_input_row_count(commit_message) as f32 * row_height + commit_border;
    let slab_y = rect.y + header_h + commit_h;
    ScSidebarBands {
        header: quadraui::Rect::new(rect.x, rect.y, rect.width, header_h),
        commit_input: quadraui::Rect::new(rect.x, rect.y + header_h, rect.width, commit_h),
        slab: quadraui::Rect::new(
            rect.x,
            slab_y,
            rect.width,
            (rect.y + rect.height - slab_y).max(0.0),
        ),
    }
}

/// Adapt the SC commit-message state into a `quadraui::TextInput` (#480,
/// migrating the hand-rolled `set_cell` commit-row painter to the shared
/// primitive shipped in quadraui#222).
///
/// Converts the engine's byte-offset cursor (`sc.commit_cursor`, an index
/// into the flat `\n`-joined `commit_message` string) into the
/// primitive's `(cursor_line, cursor_col)` char-column coordinates.
/// Render-only: the engine's `handle_sc_commit_input_key` remains the sole
/// owner of edit logic — this function only builds a paint-time snapshot.
pub fn sc_commit_message_to_text_input(sc: &SourceControlData) -> quadraui::TextInput {
    use quadraui::{TextInput, WidgetId};

    let byte_cursor = sc.commit_cursor.min(sc.commit_message.len());
    let before = &sc.commit_message[..byte_cursor];
    let cursor_line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let cursor_col = before[line_start..].chars().count();

    let lines: Vec<String> = if sc.commit_message.is_empty() {
        vec![String::new()]
    } else {
        sc.commit_message.split('\n').map(str::to_string).collect()
    };

    TextInput {
        id: WidgetId::new("sc:commit_input"),
        lines,
        cursor_line,
        cursor_col,
        // Only shown while not actively editing an empty message — matches
        // the pre-migration behaviour of hiding the prompt text as soon as
        // the cursor is live in an empty input.
        placeholder: if sc.commit_input_active {
            None
        } else {
            Some("Message (press c)".to_string())
        },
        scroll_offset: 0,
        scroll_col: 0,
        has_focus: sc.commit_input_active,
    }
}

/// Adapt the SC branch-picker popup state into a dual-mode
/// `quadraui::Palette` (#480, migrating the hand-rolled popup to the
/// primitive shipped in quadraui#224). `create_mode` maps to
/// `PaletteMode::Input` (free-text new-branch name); otherwise
/// `PaletteMode::List` with the fuzzy-filtered branch results, current
/// branch marked with a leading bullet.
///
/// Render-only, same as [`sc_commit_message_to_text_input`]: query/cursor
/// editing and selection remain owned by `Engine::handle_sc_branch_picker_key`
/// / `handle_sc_branch_create_key` — this is purely a paint-time snapshot,
/// not an adoption of `DualModePaletteController`'s own (would-be
/// duplicate) key-handling state machine.
pub fn sc_branch_picker_to_palette(bp: &BranchPickerData) -> quadraui::Palette {
    use quadraui::{Palette, PaletteItem, PaletteMode, StyledText, WidgetId};

    if bp.create_mode {
        return Palette {
            id: WidgetId::new("sc:branch_picker"),
            title: "New Branch".to_string(),
            query: bp.create_input.clone(),
            query_cursor: bp.create_input.len(),
            items: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            total_count: 0,
            has_focus: true,
            show_query: true,
            create_label: None,
            preview: None,
            mode: PaletteMode::Input,
        };
    }

    let items: Vec<PaletteItem> = bp
        .results
        .iter()
        .map(|(name, is_current)| PaletteItem {
            text: StyledText::plain(if *is_current {
                format!("\u{25cf} {name}")
            } else {
                format!("  {name}")
            }),
            detail: None,
            icon: None,
            match_positions: Vec::new(),
            depth: 0,
            expandable: false,
            expanded: false,
        })
        .collect();

    Palette {
        id: WidgetId::new("sc:branch_picker"),
        title: "Switch Branch".to_string(),
        query: bp.query.clone(),
        query_cursor: bp.query.len(),
        items,
        selected_idx: bp.selected,
        scroll_offset: 0,
        total_count: 0,
        has_focus: true,
        show_query: true,
        create_label: None,
        preview: None,
        mode: PaletteMode::List,
    }
}

/// Static keybindings table for the SC help dialog (#480, migrating the
/// hand-rolled 2-column popup to `Dialog` + `DialogTable`, shipped in
/// quadraui#225). Shared by both backends so the bindings list has one
/// source of truth.
pub fn sc_help_dialog() -> quadraui::Dialog {
    use quadraui::{Dialog, DialogButton, DialogTable, StyledText, WidgetId};

    const BINDINGS: &[(&str, &str)] = &[
        ("j/k", "Navigate"),
        ("s", "Stage / unstage"),
        ("S", "Stage all"),
        ("d", "Discard file"),
        ("D", "Discard all unstaged"),
        ("c", "Commit message"),
        ("b", "Switch branch"),
        ("B", "Create branch"),
        ("p", "Push"),
        ("P", "Pull"),
        ("f", "Fetch"),
        ("r", "Refresh"),
        ("Tab", "Expand / collapse"),
        ("Enter", "Open file"),
        ("q/Esc", "Close panel"),
    ];

    Dialog {
        id: WidgetId::new("sc:help"),
        title: StyledText::plain("Keybindings"),
        body: Vec::new(),
        buttons: vec![DialogButton {
            id: WidgetId::new("sc:help:close"),
            label: "Close".to_string(),
            is_default: true,
            is_cancel: true,
            tint: None,
        }],
        severity: None,
        vertical_buttons: false,
        table: Some(DialogTable {
            headers: Some(vec!["Key".to_string(), "Action".to_string()]),
            rows: BINDINGS
                .iter()
                .map(|(k, d)| vec![k.to_string(), d.to_string()])
                .collect(),
            column_widths: None,
        }),
        input: None,
    }
}

/// Compute the `DialogLayout` for [`sc_help_dialog`] from generic
/// char-cell/pixel metrics (TUI: `1.0, 1.0`; GTK: real `char_width`/
/// `line_height`, #546-style dual-backend convention). Mirrors
/// `dialog_generic_layout`'s char-cell approximation formula, but sized
/// from the table's own `tui_total_width`/`tui_total_height` helpers
/// since this dialog has no body text driving its width.
pub fn sc_help_dialog_layout(
    viewport: quadraui::Rect,
    char_width: f32,
    line_height: f32,
) -> (quadraui::Dialog, quadraui::DialogLayout) {
    let dialog = sc_help_dialog();
    let table = dialog
        .table
        .as_ref()
        .expect("sc_help_dialog always sets `table`");

    let table_h = table.tui_total_height() as f32 * line_height;
    let table_w = table.tui_total_width() as f32 * char_width + char_width * 2.0;

    let min_w = char_width * 30.0;
    let max_w = char_width * 60.0;
    let default_w = (viewport.width * 0.5).clamp(min_w, max_w);
    let width = default_w
        .max(table_w)
        .min(viewport.width - char_width * 4.0);

    let measure = quadraui::DialogMeasure {
        width,
        title_height: line_height,
        body_height: 0.0,
        table_height: table_h,
        input_height: 0.0,
        button_row_height: line_height,
        button_width: char_width * 8.0,
        button_gap: char_width * 2.0,
        padding: line_height,
    };
    let layout = dialog.layout(viewport, measure, |_| {
        quadraui::ToolbarItemMeasure::new(0.0)
    });
    (dialog, layout)
}

/// Adapt a `SourceControlData` (vimcode's internal representation) into a
/// generic `quadraui::TreeView` that backends can render through the shared
/// tree-primitive drawing path.
///
/// Scope: covers the four expandable sections only — Staged, Changes,
/// Worktrees, and Recent Commits. The header row, commit input, branch
/// picker, help dialog, and action button row are built by their own
/// dedicated adapters (`sc_header_text`, `sc_commit_message_to_text_input`,
/// `sc_branch_picker_to_palette`, `sc_help_dialog`, `sc_button_toolbar`).
///
/// Row order mirrors `render_source_control()` in the TUI so `sc.selected`
/// (a flat row index within the sections area) maps one-to-one onto the
/// returned `TreeView.rows`.
pub fn source_control_to_tree_view(sc: &SourceControlData, theme: &Theme) -> quadraui::TreeView {
    use quadraui::{
        Badge, Decoration, SelectionMode, StyledSpan, StyledText, TreeRow, TreeStyle, TreeView,
        WidgetId,
    };

    let mut rows: Vec<TreeRow> = Vec::new();

    let add_fg = to_q_color(theme.git_added);
    let del_fg = to_q_color(theme.git_deleted);
    let mod_fg = to_q_color(theme.git_modified);
    let dim_fg = to_q_color(theme.status_inactive_fg);
    let show_worktrees = sc.worktrees.len() > 1;

    // Section order: 0=Staged, 1=Changes, 2=Worktrees (conditional), 3=Log.
    // Matches `render_source_control()` in tui_main/panels.rs.
    // Labels include Nerd Font glyphs; backends that don't have the icon font
    // will still show the text portion.
    let sections: [(u16, &str, usize); 4] = [
        (0, "\u{f055} STAGED CHANGES", sc.staged.len()),
        (1, "\u{f02b} CHANGES", sc.unstaged.len()),
        (2, "\u{e702} WORKTREES", sc.worktrees.len()),
        (3, "\u{f417} RECENT COMMITS", sc.log.len()),
    ];

    for (sec_idx, label, count) in sections {
        if sec_idx == 2 && !show_worktrees {
            continue;
        }
        let is_expanded = sc.sections_expanded[sec_idx as usize];

        // Section header row (branch in tree terms).
        let badge = if count > 0 {
            Some(Badge::plain(format!("({})", count)))
        } else {
            None
        };
        rows.push(TreeRow {
            path: vec![sec_idx],
            indent: 0,
            icon: None,
            text: StyledText::plain(label),
            badge,
            is_expanded: Some(is_expanded),
            decoration: Decoration::Header,
            edit: None,
        });

        if !is_expanded {
            continue;
        }

        match sec_idx {
            0 | 1 => {
                // NOTE: no "(no changes)" placeholder row — adding one would
                // shift the flat row count away from
                // `engine.sc_flat_to_section_idx`, which breaks the
                // `sc.selected` → `selected_path` mapping and causes Tab /
                // Enter / staging to act on the wrong section.
                let items = if sec_idx == 0 {
                    &sc.staged
                } else {
                    &sc.unstaged
                };
                for (i, fi) in items.iter().enumerate() {
                    let status_color = match fi.status_char {
                        'A' => add_fg,
                        'D' => del_fg,
                        _ => mod_fg,
                    };
                    rows.push(TreeRow {
                        path: vec![sec_idx, i as u16],
                        indent: 1,
                        icon: None,
                        text: StyledText {
                            spans: vec![
                                StyledSpan::with_fg(fi.status_char.to_string(), status_color),
                                StyledSpan::plain(format!(" {}", fi.path)),
                            ],
                        },
                        badge: None,
                        is_expanded: None,
                        decoration: Decoration::Normal,
                        edit: None,
                    });
                }
            }
            2 => {
                for (i, wt) in sc.worktrees.iter().enumerate() {
                    let check = if wt.is_current { "\u{2713} " } else { "  " };
                    let main_marker = if wt.is_main { " [main]" } else { "" };
                    let text = format!("{}{} {}{}", check, wt.branch, wt.path, main_marker);
                    rows.push(TreeRow {
                        path: vec![sec_idx, i as u16],
                        indent: 1,
                        icon: None,
                        text: StyledText::plain(text),
                        badge: None,
                        is_expanded: None,
                        decoration: Decoration::Normal,
                        edit: None,
                    });
                }
            }
            3 => {
                for (i, entry) in sc.log.iter().enumerate() {
                    rows.push(TreeRow {
                        path: vec![sec_idx, i as u16],
                        indent: 1,
                        icon: None,
                        text: StyledText {
                            spans: vec![
                                StyledSpan::with_fg(entry.hash.clone(), dim_fg),
                                StyledSpan::plain(format!(" {}", entry.message)),
                            ],
                        },
                        badge: None,
                        is_expanded: None,
                        decoration: Decoration::Muted,
                        edit: None,
                    });
                }
            }
            _ => {}
        }
    }

    // Map flat `sc.selected` → `selected_path`. When selected is out of range
    // (e.g. sections collapsed), we fall back to no selection.
    let selected_path = rows.get(sc.selected).map(|r| r.path.clone());

    TreeView {
        id: WidgetId::new("sc-tree"),
        rows,
        selection_mode: SelectionMode::Single,
        selected_path,
        scroll_offset: 0,
        style: TreeStyle::default(),
        has_focus: sc.has_focus,
    }
}

/// Populate the `SidebarSystem` on `engine.dap_sidebar_system` with
/// current row data for all 4 debug sidebar sections. Call once per
/// frame before `sidebar_system.render()` or `.handle()`.
pub fn populate_dap_sidebar_system(engine: &Engine) {
    let session_active = engine.dap_session_active;

    // ── Variables section ──
    let var_rows = build_dap_var_rows(engine, session_active);
    // ── Watch section ──
    let watch_rows = build_dap_watch_rows(engine, session_active);
    // ── Call Stack section ──
    let stack_rows = build_dap_stack_rows(engine, session_active);
    // ── Breakpoints section ──
    let bp_rows = build_dap_bp_rows(engine, session_active);

    let mut sidebar = engine.dap_sidebar_system.borrow_mut();
    sidebar.set_has_focus(engine.dap_sidebar_has_focus);
    if engine.dap_sidebar_has_focus && sidebar.active_section().is_none() {
        sidebar.set_active_section(Some(0));
    }
    sidebar.set_rows(0, var_rows);
    sidebar.set_rows(1, watch_rows);
    sidebar.set_rows(2, stack_rows);
    sidebar.set_rows(3, bp_rows);
}

pub fn populate_ext_sidebar_system(engine: &Engine) {
    engine.populate_ext_sidebar_system();
}

/// Populate the `SidebarSystem` on `engine.sc_sidebar_system` with current
/// row data for all 4 SC sections. Call once per frame before
/// `sidebar_system.render()` or `.handle_cached()`.
pub fn populate_sc_sidebar_system(engine: &Engine, theme: &Theme) {
    use quadraui::{Decoration, StyledSpan, StyledText, TreeRow};

    let add_fg = to_q_color(theme.git_added);
    let del_fg = to_q_color(theme.git_deleted);
    let mod_fg = to_q_color(theme.git_modified);
    let dim_fg = to_q_color(theme.status_inactive_fg);

    let staged: Vec<_> = engine
        .sc_file_statuses
        .iter()
        .filter(|f| f.staged.is_some())
        .collect();
    let unstaged: Vec<_> = engine
        .sc_file_statuses
        .iter()
        .filter(|f| f.unstaged.is_some())
        .collect();
    let show_worktrees = engine.sc_worktrees.len() > 1;

    let file_row = |i: usize, f: &crate::core::git::FileStatus, is_staged: bool| {
        let kind = if is_staged { f.staged } else { f.unstaged };
        let ch = kind.map(|k| k.label()).unwrap_or('?');
        let color = match ch {
            'A' => add_fg,
            'D' => del_fg,
            _ => mod_fg,
        };
        TreeRow {
            path: vec![i as u16],
            indent: 0,
            icon: None,
            text: StyledText {
                spans: vec![
                    StyledSpan::with_fg(ch.to_string(), color),
                    StyledSpan::plain(format!(" {}", f.path)),
                ],
            },
            badge: None,
            is_expanded: None,
            decoration: Decoration::Normal,
            edit: None,
        }
    };

    let staged_rows: Vec<TreeRow> = staged
        .iter()
        .enumerate()
        .map(|(i, f)| file_row(i, f, true))
        .collect();

    let unstaged_rows: Vec<TreeRow> = unstaged
        .iter()
        .enumerate()
        .map(|(i, f)| file_row(i, f, false))
        .collect();

    let worktree_rows: Vec<TreeRow> = engine
        .sc_worktrees
        .iter()
        .enumerate()
        .map(|(i, wt)| {
            let check = if wt.is_current { "\u{2713} " } else { "  " };
            let branch = wt.branch.as_deref().unwrap_or("HEAD");
            let main_marker = if wt.is_main { " [main]" } else { "" };
            let text = format!("{}{} {}{}", check, branch, wt.path.display(), main_marker);
            TreeRow {
                path: vec![i as u16],
                indent: 0,
                icon: None,
                text: StyledText::plain(text),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Normal,
                edit: None,
            }
        })
        .collect();

    let log_rows: Vec<TreeRow> = engine
        .sc_log
        .iter()
        .enumerate()
        .map(|(i, entry)| TreeRow {
            path: vec![i as u16],
            indent: 0,
            icon: None,
            text: StyledText {
                spans: vec![
                    StyledSpan::with_fg(entry.hash.clone(), dim_fg),
                    StyledSpan::plain(format!(" {}", entry.message)),
                ],
            },
            badge: None,
            is_expanded: None,
            decoration: Decoration::Muted,
            edit: None,
        })
        .collect();

    let mut sidebar = engine.sc_sidebar_system.borrow_mut();
    sidebar.set_has_focus(engine.sc_has_focus);
    if engine.sc_has_focus && sidebar.active_section().is_none() {
        sidebar.set_active_section(Some(0));
    }

    let badge = |n: usize| {
        if n > 0 {
            Some(StyledText::plain(format!("({})", n)))
        } else {
            None
        }
    };
    sidebar.set_section_badge(0, badge(staged.len()));
    sidebar.set_section_badge(1, badge(unstaged.len()));
    sidebar.set_section_badge(2, badge(engine.sc_worktrees.len()));
    sidebar.set_section_badge(3, badge(engine.sc_log.len()));
    sidebar.set_section_visible(2, show_worktrees);

    sidebar.set_rows(0, staged_rows);
    sidebar.set_rows(1, unstaged_rows);
    sidebar.set_rows(2, worktree_rows);
    sidebar.set_rows(3, log_rows);
}

/// Populate the Search panel's `SidebarSystem` with current form + tree
/// data. Section 0 is the Form (query/replace/toggles/buttons/status);
/// Section 1 is the TreeView (results grouped by file). Call once per
/// frame before `sidebar.render()`.
pub fn populate_search_sidebar_system(engine: &Engine, root: &std::path::Path) {
    use quadraui::primitives::form::{ButtonRowItem, FieldKind, ToggleGroupItem};
    use quadraui::{Badge, Decoration, Form, FormField, StyledSpan, StyledText, TreeRow, WidgetId};

    let opts = &engine.project_search_options;
    let results = &engine.project_search_results;

    // ── Section 0: Form (search chrome) ─────────────────────────────────
    let form_focus = engine.search_panel_form_focus.borrow();
    let query_focused = form_focus.as_deref() == Some("search:query");
    let replace_focused = form_focus.as_deref() == Some("search:replace");

    let form = Form {
        id: WidgetId::new("search-form"),
        fields: vec![
            FormField {
                id: WidgetId::new("search:query"),
                label: StyledText::default(),
                kind: FieldKind::TextInput {
                    value: engine.project_search_query.clone(),
                    placeholder: "Search…".to_string(),
                    cursor: if query_focused {
                        Some(engine.search_query_caret.get())
                    } else {
                        None
                    },
                    selection_anchor: None,
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:replace"),
                label: StyledText::default(),
                kind: FieldKind::TextInput {
                    value: engine.project_replace_text.clone(),
                    placeholder: "Replace…".to_string(),
                    cursor: if replace_focused {
                        Some(engine.replace_text_caret.get())
                    } else {
                        None
                    },
                    selection_anchor: None,
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:toggles"),
                label: StyledText::default(),
                kind: FieldKind::ToggleGroup {
                    toggles: vec![
                        ToggleGroupItem {
                            id: WidgetId::new("search:case"),
                            label: "Aa".to_string(),
                            value: opts.case_sensitive,
                        },
                        ToggleGroupItem {
                            id: WidgetId::new("search:word"),
                            label: "Ab|".to_string(),
                            value: opts.whole_word,
                        },
                        ToggleGroupItem {
                            id: WidgetId::new("search:regex"),
                            label: ".*".to_string(),
                            value: opts.use_regex,
                        },
                    ],
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:buttons"),
                label: StyledText::default(),
                kind: FieldKind::ButtonRow {
                    buttons: vec![
                        ButtonRowItem {
                            id: WidgetId::new("search:find_next"),
                            label: "Find".to_string(),
                            disabled: engine.project_search_query.is_empty(),
                            icon: None,
                        },
                        ButtonRowItem {
                            id: WidgetId::new("search:replace_next"),
                            label: "Repl".to_string(),
                            disabled: results.is_empty(),
                            icon: None,
                        },
                        ButtonRowItem {
                            id: WidgetId::new("search:replace_all"),
                            label: "All".to_string(),
                            disabled: results.is_empty(),
                            icon: None,
                        },
                    ],
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:status"),
                label: StyledText::default(),
                kind: FieldKind::ReadOnly {
                    value: StyledText::plain(if results.is_empty() {
                        if engine.project_search_query.is_empty() {
                            "Type to search, Enter to run".to_string()
                        } else if engine.project_search_status.is_empty() {
                            String::new()
                        } else {
                            engine.project_search_status.clone()
                        }
                    } else {
                        engine.project_search_status.clone()
                    }),
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
        ],
        focused_field: form_focus.as_deref().map(WidgetId::new),
        scroll_offset: 0,
        has_focus: query_focused || replace_focused,
    };

    // ── Section 1: TreeView (results grouped by file) ───────────────────
    let collapsed = engine.search_collapsed_files.borrow();
    let mut tree_rows: Vec<TreeRow> = Vec::new();
    let mut file_idx: usize = 0;
    let mut last_file: Option<&std::path::Path> = None;
    let mut match_within_file: usize = 0;
    let mut file_match_count: usize = 0;

    for m in results.iter() {
        if last_file != Some(m.file.as_path()) {
            if let Some(prev_header) = tree_rows.iter_mut().rev().find(|r| r.path.len() == 1) {
                prev_header.badge = Some(Badge::plain(format!("({})", file_match_count)));
            }
            if last_file.is_some() {
                file_idx += 1;
            }
            last_file = Some(m.file.as_path());
            match_within_file = 0;
            file_match_count = 0;

            let expanded = !collapsed.contains(&file_idx);
            let rel = m.file.strip_prefix(root).unwrap_or(&m.file);
            tree_rows.push(TreeRow {
                path: vec![file_idx as u16],
                indent: 0,
                icon: None,
                text: StyledText {
                    spans: vec![StyledSpan::plain(rel.display().to_string())],
                },
                badge: None,
                is_expanded: Some(expanded),
                decoration: Decoration::Header,
                edit: None,
            });
        }

        let expanded = !collapsed.contains(&file_idx);
        if expanded {
            let line_prefix = format!("{:>4}: ", m.line + 1);
            tree_rows.push(TreeRow {
                path: vec![file_idx as u16, match_within_file as u16],
                indent: 1,
                icon: None,
                text: StyledText {
                    spans: vec![
                        StyledSpan {
                            text: line_prefix,
                            fg: Some(quadraui::Color::rgb(100, 100, 100)),
                            bg: None,
                            bold: false,
                            italic: false,
                            underline: false,
                        },
                        StyledSpan::plain(m.line_text.trim().to_string()),
                    ],
                },
                badge: None,
                is_expanded: None,
                decoration: Decoration::Normal,
                edit: None,
            });
        }
        match_within_file += 1;
        file_match_count += 1;
    }
    if let Some(prev_header) = tree_rows.iter_mut().rev().find(|r| r.path.len() == 1) {
        prev_header.badge = Some(Badge::plain(format!("({})", file_match_count)));
    }

    let mut sidebar = engine.search_sidebar_system.borrow_mut();
    sidebar.set_has_focus(engine.search_has_focus);
    sidebar.set_form(0, form);
    sidebar.set_rows(1, tree_rows);

    if engine.search_has_focus && sidebar.active_section().is_none() {
        sidebar.set_active_section(Some(0));
    }
}

/// Populate the `TreeController` on `engine.explorer_tree` with current
/// row data. Call once per frame before `tree_controller.render()` or
/// `.handle()`.
pub fn populate_explorer_tree_controller(engine: &Engine, theme: &Theme) {
    let mut tree = engine.explorer_tree.borrow_mut();
    tree.set_has_focus(engine.explorer_has_focus);
    let tree_rows = build_explorer_tree_rows(&engine.explorer_rows, engine, theme);
    tree.set_rows(tree_rows);
}

fn build_explorer_tree_rows(
    rows: &[ExplorerRow],
    engine: &Engine,
    theme: &Theme,
) -> Vec<quadraui::TreeRow> {
    use quadraui::{Badge, Decoration, Icon as QIcon, StyledText, TreeRow};

    let (git_statuses, diag_counts) = engine.explorer_indicators();
    let err_fg = to_quadraui_color(theme.diagnostic_error);
    let warn_fg = to_quadraui_color(theme.diagnostic_warning);

    let mut out: Vec<TreeRow> = Vec::with_capacity(rows.len());
    for (row_idx, row) in rows.iter().enumerate() {
        let canon = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());

        let diag = diag_counts.get(&canon).copied();
        let git_label = git_statuses.get(&canon).copied();

        let decoration = match diag {
            Some((e, _)) if e > 0 => Decoration::Error,
            Some((_, w)) if w > 0 => Decoration::Warning,
            _ if git_label.is_some() => Decoration::Modified,
            _ => Decoration::Normal,
        };

        let badge = if let Some((errors, warnings)) = diag {
            if errors > 0 {
                Some(Badge::colored(
                    if errors > 9 {
                        "9+".to_string()
                    } else {
                        errors.to_string()
                    },
                    err_fg,
                ))
            } else if warnings > 0 {
                Some(Badge::colored(
                    if warnings > 9 {
                        "9+".to_string()
                    } else {
                        warnings.to_string()
                    },
                    warn_fg,
                ))
            } else {
                git_label.map(|label| Badge::plain(label.to_string()))
            }
        } else {
            git_label.map(|label| Badge::plain(label.to_string()))
        };

        let icon = if row.is_dir {
            Some(QIcon::new(
                icons::FOLDER.nerd.to_string(),
                icons::FOLDER.fallback.to_string(),
            ))
        } else {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let glyph = icons::file_icon(ext).to_string();
            Some(QIcon::new(glyph, ".".to_string()))
        };

        out.push(TreeRow {
            path: vec![row_idx as u16],
            indent: row.depth as u16,
            icon,
            text: StyledText::plain(&row.name),
            badge,
            is_expanded: if row.is_dir {
                Some(row.is_expanded)
            } else {
                None
            },
            decoration,
            edit: None,
        });
    }
    out
}

fn empty_placeholder_row(session_active: bool) -> quadraui::TreeRow {
    use quadraui::{Decoration, StyledText, TreeRow};
    let hint = if session_active {
        "(empty)"
    } else {
        "(not running)"
    };
    TreeRow {
        path: vec![u16::MAX],
        indent: 0,
        icon: None,
        text: StyledText::plain(hint.to_string()),
        badge: None,
        is_expanded: None,
        decoration: Decoration::Muted,
        edit: None,
    }
}

fn build_dap_var_rows(engine: &Engine, session_active: bool) -> Vec<quadraui::TreeRow> {
    use quadraui::{Decoration, StyledText, TreeRow};

    let mut rows: Vec<TreeRow> = Vec::new();
    let mut flat_idx: u16 = 0;

    fn push_var_tree(
        rows: &mut Vec<TreeRow>,
        vars: &[crate::core::dap::DapVariable],
        depth: u16,
        flat_idx: &mut u16,
        expanded: &std::collections::HashSet<u64>,
        children_map: &std::collections::HashMap<u64, Vec<crate::core::dap::DapVariable>>,
    ) {
        for v in vars {
            let prefix = if v.var_ref > 0 {
                if expanded.contains(&v.var_ref) {
                    icons::EXPAND_DOWN.nerd
                } else {
                    icons::COLLAPSE_RIGHT.nerd
                }
            } else {
                "  "
            };
            let text = if v.value.is_empty() {
                format!("{}{}", prefix, v.name)
            } else {
                format!("{}{} = {}", prefix, v.name, v.value)
            };
            rows.push(TreeRow {
                path: vec![*flat_idx],
                indent: depth,
                icon: None,
                text: StyledText::plain(text),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Normal,
                edit: None,
            });
            *flat_idx += 1;
            if v.var_ref > 0 && expanded.contains(&v.var_ref) {
                if let Some(child_vars) = children_map.get(&v.var_ref) {
                    push_var_tree(
                        rows,
                        child_vars,
                        depth + 1,
                        flat_idx,
                        expanded,
                        children_map,
                    );
                }
            }
        }
    }

    if engine.dap_primary_scope_ref > 0 {
        let expanded = engine
            .dap_expanded_vars
            .contains(&engine.dap_primary_scope_ref);
        let prefix = if expanded {
            icons::EXPAND_DOWN.nerd
        } else {
            icons::COLLAPSE_RIGHT.nerd
        };
        rows.push(TreeRow {
            path: vec![flat_idx],
            indent: 0,
            icon: None,
            text: StyledText::plain(format!("{prefix}{}", engine.dap_primary_scope_name)),
            badge: None,
            is_expanded: None,
            decoration: Decoration::Normal,
            edit: None,
        });
        flat_idx += 1;
        if expanded {
            push_var_tree(
                &mut rows,
                &engine.dap_variables,
                1,
                &mut flat_idx,
                &engine.dap_expanded_vars,
                &engine.dap_child_variables,
            );
        }
    } else {
        push_var_tree(
            &mut rows,
            &engine.dap_variables,
            0,
            &mut flat_idx,
            &engine.dap_expanded_vars,
            &engine.dap_child_variables,
        );
    }

    for (scope_name, var_ref) in &engine.dap_scope_groups {
        let expanded = engine.dap_expanded_vars.contains(var_ref);
        let prefix = if expanded {
            icons::EXPAND_DOWN.nerd
        } else {
            icons::COLLAPSE_RIGHT.nerd
        };
        rows.push(TreeRow {
            path: vec![flat_idx],
            indent: 0,
            icon: None,
            text: StyledText::plain(format!("{prefix}{scope_name}")),
            badge: None,
            is_expanded: None,
            decoration: Decoration::Normal,
            edit: None,
        });
        flat_idx += 1;
        if expanded {
            if let Some(child_vars) = engine.dap_child_variables.get(var_ref) {
                push_var_tree(
                    &mut rows,
                    child_vars,
                    1,
                    &mut flat_idx,
                    &engine.dap_expanded_vars,
                    &engine.dap_child_variables,
                );
            }
        }
    }

    if rows.is_empty() {
        vec![empty_placeholder_row(session_active)]
    } else {
        rows
    }
}

fn build_dap_watch_rows(engine: &Engine, session_active: bool) -> Vec<quadraui::TreeRow> {
    use quadraui::{Decoration, StyledText, TreeRow};

    if engine.dap_watch_expressions.is_empty() {
        return vec![empty_placeholder_row(session_active)];
    }

    engine
        .dap_watch_expressions
        .iter()
        .zip(engine.dap_watch_values.iter())
        .enumerate()
        .map(|(i, (expr, val))| {
            let val_str = val.as_deref().unwrap_or(if session_active {
                "\u{2026}" // …
            } else {
                "(not running)"
            });
            TreeRow {
                path: vec![i as u16],
                indent: 0,
                icon: None,
                text: StyledText::plain(format!("{expr} = {val_str}")),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Normal,
                edit: None,
            }
        })
        .collect()
}

fn build_dap_stack_rows(engine: &Engine, session_active: bool) -> Vec<quadraui::TreeRow> {
    use quadraui::{Decoration, StyledText, TreeRow};

    if engine.dap_stack_frames.is_empty() {
        return vec![empty_placeholder_row(session_active)];
    }

    engine
        .dap_stack_frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let src = f
                .source
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("?");
            let prefix = if i == engine.dap_active_frame {
                icons::COLLAPSE_RIGHT.nerd
            } else {
                "  "
            };
            TreeRow {
                path: vec![i as u16],
                indent: 0,
                icon: None,
                text: StyledText::plain(format!("{}{} ({}:{})", prefix, f.name, src, f.line)),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Normal,
                edit: None,
            }
        })
        .collect()
}

fn build_dap_bp_rows(engine: &Engine, session_active: bool) -> Vec<quadraui::TreeRow> {
    use quadraui::{Decoration, StyledText, TreeRow};

    let mut sorted_bp: Vec<_> = engine.dap_breakpoints.iter().collect();
    sorted_bp.sort_by_key(|(path, _)| path.as_str());

    let mut rows = Vec::new();
    let mut flat_idx: u16 = 0;
    for (path, bps) in &sorted_bp {
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        for bp in *bps {
            let suffix = if let Some(cond) = &bp.condition {
                format!(" [if {cond}]")
            } else if let Some(hc) = &bp.hit_condition {
                format!(" [hits {hc}]")
            } else if let Some(msg) = &bp.log_message {
                format!(" [log: {msg}]")
            } else {
                String::new()
            };
            let symbol = if bp.condition.is_some() || bp.hit_condition.is_some() {
                "\u{25c6}" // ◆ conditional
            } else {
                icons::DBG_BREAKPOINTS.nerd
            };
            rows.push(TreeRow {
                path: vec![flat_idx],
                indent: 0,
                icon: None,
                text: StyledText::plain(format!("{} {}:{}{}", symbol, file_name, bp.line, suffix)),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Normal,
                edit: None,
            });
            flat_idx += 1;
        }
    }

    if rows.is_empty() {
        vec![empty_placeholder_row(session_active)]
    } else {
        rows
    }
}

// ─── Activity bar adapter (#133) ─────────────────────────────────────────────

/// Build a `quadraui::ActivityBar` primitive from engine state.
///
/// Both backends call this once per frame, then delegate to
/// `quadraui::{tui,gtk}::draw_activity_bar`.
///
/// * `include_hamburger` — `true` for TUI (no native menu bar so the
///   hamburger item at keyboard-index 0 provides keyboard access to the
///   menu); `false` for GTK (the menu bar is a native GTK widget).
/// * `active_ext_panel` — name of the currently-active extension panel, if
///   any. GTK passes `engine.ext_panel_active.as_deref()`; TUI passes
///   `sidebar.ext_panel_name.as_deref()`.
///
/// Icons use `icons::*.s()` which respects the thread-local `USE_NERD_FONTS`
/// flag (see `icons` module docs) set at startup — both GTK and TUI set it
/// from `settings.use_nerd_fonts` on their own (single) render thread.
pub fn build_activity_bar(
    engine: &Engine,
    theme: &Theme,
    include_hamburger: bool,
    active_ext_panel: Option<&str>,
) -> quadraui::ActivityBar {
    use crate::core::engine::sidebar::{
        ext_panel_id, HAMBURGER_PANEL_ID, PANEL_AI, PANEL_DEBUG, PANEL_EXPLORER, PANEL_EXTENSIONS,
        PANEL_GIT, PANEL_SEARCH, PANEL_SETTINGS,
    };

    // #536: the keyboard ring is matched by *panel id*, not by re-deriving each
    // item's numeric toolbar index at the paint site. `Engine` owns the single
    // index↔id table (`activity_bar_item_id`), and the stepping itself is
    // quadraui's `AppShell` cursor (quadraui#386) — so the painted order and
    // the navigable order cannot drift apart here.
    let kbd_sel_id = if engine.activity_bar_focused {
        engine.activity_bar_selected_item_id()
    } else {
        None
    };
    let kbd_sel = |panel_id: &str| kbd_sel_id.as_deref() == Some(panel_id);
    let sb_visible = engine.app_shell.sidebar_visible();
    let has_ext = active_ext_panel.is_some();
    let active_id = engine.app_shell.active_panel_id().map(|w| w.as_str());

    let mut top = Vec::new();

    if include_hamburger {
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(HAMBURGER_PANEL_ID),
            icon: icons::HAMBURGER.s().to_string(),
            tooltip: "Menu".to_string(),
            is_active: false,
            is_keyboard_selected: kbd_sel(HAMBURGER_PANEL_ID),
        });
    }

    // (panel_id, icon, tooltip, activity_id)
    let fixed: [(&str, &str, &str, &str); 6] = [
        (
            PANEL_EXPLORER,
            icons::EXPLORER.s(),
            "Explorer (Ctrl+Shift+E)",
            "activity:explorer",
        ),
        (
            PANEL_SEARCH,
            icons::SEARCH.s(),
            "Search (Ctrl+Shift+F)",
            "activity:search",
        ),
        (PANEL_DEBUG, icons::DEBUG.s(), "Debug", "activity:debug"),
        (
            PANEL_GIT,
            icons::GIT_BRANCH.s(),
            "Source Control",
            "activity:git",
        ),
        (
            PANEL_EXTENSIONS,
            icons::EXTENSIONS.s(),
            "Extensions",
            "activity:extensions",
        ),
        (PANEL_AI, icons::AI_CHAT.s(), "AI Assistant", "activity:ai"),
    ];

    // #635 (Stage 6b): `tui_main::shell_app::TuiShellApp::shell_config` derives
    // its own middle-panel `PanelDefinition` order from
    // `sidebar::FIXED_ACTIVITY_PANEL_IDS` rather than re-reading this array (it
    // can't — `fixed` is a local, and the two arrays carry different metadata
    // shapes), so this assertion is what actually keeps them from drifting
    // apart: a reordering here without updating the shared constant now trips
    // in any test that exercises `build_activity_bar` (every one does).
    debug_assert_eq!(
        fixed.map(|(panel_id, _, _, _)| panel_id),
        crate::core::engine::sidebar::FIXED_ACTIVITY_PANEL_IDS,
        "build_activity_bar's `fixed` panel-id order must match \
         sidebar::FIXED_ACTIVITY_PANEL_IDS"
    );

    for (panel_id, icon, tooltip, activity_id) in fixed {
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(activity_id),
            icon: icon.to_string(),
            tooltip: tooltip.to_string(),
            is_active: sb_visible && !has_ext && active_id == Some(panel_id),
            is_keyboard_selected: kbd_sel(panel_id),
        });
    }

    // Dynamic extension panels, sorted by name — the same order
    // `Engine::ext_activity_panels` (and therefore the keyboard ring) uses.
    let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
    ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
    for panel in ext_panels.iter() {
        let is_active = sb_visible && active_ext_panel == Some(panel.name.as_str());
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(format!("activity:ext:{}", panel.name)),
            icon: panel.resolved_icon().to_string(),
            tooltip: panel.title.clone(),
            is_active,
            is_keyboard_selected: kbd_sel(&ext_panel_id(&panel.name)),
        });
    }

    let bottom = vec![quadraui::ActivityItem {
        id: quadraui::WidgetId::new("activity:settings"),
        icon: icons::SETTINGS.s().to_string(),
        tooltip: "Settings".to_string(),
        is_active: sb_visible && !has_ext && active_id == Some(PANEL_SETTINGS),
        is_keyboard_selected: kbd_sel(PANEL_SETTINGS),
    }];

    quadraui::ActivityBar {
        id: quadraui::WidgetId::new("activity-bar"),
        top_items: top,
        bottom_items: bottom,
        active_accent: Some(quadraui::Color::rgb(
            theme.cursor.r,
            theme.cursor.g,
            theme.cursor.b,
        )),
        selection_bg: Some(quadraui::Color::rgb(
            theme.cursor.r,
            theme.cursor.g,
            theme.cursor.b,
        )),
        // Signals to the quadraui backend that this bar owns the keyboard so
        // it intercepts KeyPressed as ActivityBarEvent::KeyPressed (Q#368).
        is_keyboard_focused: engine.activity_bar_focused,
    }
}

/// Adapt one section of the debug sidebar (`Variables` / `Watch` /
/// `Call Stack` / `Breakpoints`) into a `quadraui::TreeView` for the
/// shared `draw_tree` primitive (#281).
///
/// Adapt the engine-side `ExtSidebarData` into a `quadraui::TreeView`
/// for the shared `draw_tree` primitive (#280).
///
/// Tree shape:
/// - Path `[0]` — "INSTALLED (n)" header (`Decoration::Header`, badge
///   carries the count).
/// - Path `[0, i]` — installed item `i` (icon = filled circle / nerd-font
///   `\u{f4d0}`; badge `[d]remove` or `[u]update` shown only on the
///   selected row; trailing `\u{2191}` (`↑`) on the label when an
///   update is available).
/// - Path `[1]` — "AVAILABLE (m)" header.
/// - Path `[1, i]` — available item `i` (hollow circle icon; badge
///   `[i]install` on selected row only).
/// - Empty-state placeholders (`(none installed)`, `(all installed)`,
///   `Fetching registry…`) appear as `Decoration::Muted` rows that are
///   intentionally not in the selection mapping.
///
/// `selected: usize` in `ExtSidebarData` is a flat index across items
/// (installed first, then available). This maps to `selected_path`
/// = `[0, selected]` while `selected < installed_count` else
/// `[1, selected - installed_count]`.
pub fn ext_sidebar_to_tree_view(ext: &ExtSidebarData) -> quadraui::TreeView {
    use quadraui::{
        Badge, Decoration, SelectionMode, StyledText, TreeRow, TreeStyle, TreeView, WidgetId,
    };

    let mut rows: Vec<TreeRow> = Vec::new();

    let installed_count = ext.items_installed.len();
    let available_count = ext.items_available.len();

    // Map flat selected → (section, item_idx) for badge gating.
    let (sel_section, sel_item) = if ext.selected < installed_count {
        (0u16, ext.selected)
    } else {
        (1u16, ext.selected.saturating_sub(installed_count))
    };

    // ── Section 0: INSTALLED ─────────────────────────────────────────────────
    rows.push(TreeRow {
        path: vec![0],
        indent: 0,
        icon: None,
        text: StyledText::plain(format!("INSTALLED ({})", installed_count)),
        badge: None,
        is_expanded: Some(ext.sections_expanded[0]),
        decoration: Decoration::Header,
        edit: None,
    });

    if ext.sections_expanded[0] {
        if installed_count == 0 {
            rows.push(TreeRow {
                path: vec![0, u16::MAX],
                indent: 1,
                icon: None,
                text: StyledText::plain("(none installed)".to_string()),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Muted,
                edit: None,
            });
        } else {
            for (i, item) in ext.items_installed.iter().enumerate() {
                let is_sel = ext.has_focus && sel_section == 0 && sel_item == i;
                let label = if item.update_available {
                    format!("\u{25cf} {} \u{2191}", item.display_name)
                } else {
                    format!("\u{25cf} {}", item.display_name)
                };
                let badge = if is_sel {
                    let hint = if item.update_available {
                        "[u]update"
                    } else {
                        "[d]remove"
                    };
                    Some(Badge::plain(hint.to_string()))
                } else {
                    None
                };
                rows.push(TreeRow {
                    path: vec![0, i as u16],
                    indent: 1,
                    icon: None,
                    text: StyledText::plain(label),
                    badge,
                    is_expanded: None,
                    decoration: Decoration::Normal,
                    edit: None,
                });
            }
        }
    }

    // ── Section 1: AVAILABLE ─────────────────────────────────────────────────
    rows.push(TreeRow {
        path: vec![1],
        indent: 0,
        icon: None,
        text: StyledText::plain(format!("AVAILABLE ({})", available_count)),
        badge: None,
        is_expanded: Some(ext.sections_expanded[1]),
        decoration: Decoration::Header,
        edit: None,
    });

    if ext.sections_expanded[1] {
        if available_count == 0 {
            let msg = if ext.fetching {
                "Fetching registry\u{2026}"
            } else {
                "(all installed)"
            };
            rows.push(TreeRow {
                path: vec![1, u16::MAX],
                indent: 1,
                icon: None,
                text: StyledText::plain(msg.to_string()),
                badge: None,
                is_expanded: None,
                decoration: Decoration::Muted,
                edit: None,
            });
        } else {
            for (i, item) in ext.items_available.iter().enumerate() {
                let is_sel = ext.has_focus && sel_section == 1 && sel_item == i;
                let badge = if is_sel {
                    Some(Badge::plain("[i]install".to_string()))
                } else {
                    None
                };
                rows.push(TreeRow {
                    path: vec![1, i as u16],
                    indent: 1,
                    icon: None,
                    text: StyledText::plain(format!("\u{25cb} {}", item.display_name)),
                    badge,
                    is_expanded: None,
                    decoration: Decoration::Normal,
                    edit: None,
                });
            }
        }
    }

    // Compute the selected_path matching the flat `ext.selected`. Skip
    // when the selection points outside the visible items (e.g. all
    // sections collapsed).
    let selected_path = if ext.has_focus {
        if sel_section == 0 && sel_item < installed_count && ext.sections_expanded[0] {
            Some(vec![0u16, sel_item as u16])
        } else if sel_section == 1 && sel_item < available_count && ext.sections_expanded[1] {
            Some(vec![1u16, sel_item as u16])
        } else {
            None
        }
    } else {
        None
    };

    TreeView {
        id: WidgetId::new("ext-sidebar-tree"),
        rows,
        selection_mode: SelectionMode::Single,
        selected_path,
        scroll_offset: 0,
        style: TreeStyle::default(),
        has_focus: ext.has_focus,
    }
}

/// Adapt the engine-side `ExtPanelData` (extension-provided sidebar
/// panel) into a `quadraui::TreeView` for rendering via the shared
/// `draw_tree` primitive (#476).
///
/// Tree shape:
/// - Path `[s]` — section `s` header (`Decoration::Header`, chevron
///   reflects `section.expanded`).
/// - Path `[s, i]` — visible item `i` in section `s`. `indent` mirrors
///   `ExtPanelItem.indent + 1` so children of section headers start at
///   indent 1 (matching the legacy renderer). Tree-expandable items
///   carry `is_expanded: Some(item.expanded)` so the primitive draws
///   the chevron; non-expandable items leave `is_expanded` as `None`.
///
/// `ExtPanelStyle` maps to `Decoration` as:
/// - `Header → Decoration::Header`
/// - `Dim → Decoration::Muted`
/// - `Normal → Decoration::Normal`
/// - `Accent → Decoration::Normal` with the row text wrapped in a
///   `StyledSpan` coloured by `theme.keyword` (no first-class accent
///   decoration on the primitive).
///
/// Badges and action labels are concatenated into a single
/// right-aligned `Badge`, matching the legacy `[badge] ⟨action⟩ hint`
/// hint format. Separator rows (`item.is_separator`) become a single
/// `Decoration::Muted` row with a `─` glyph; the primitive doesn't
/// have a first-class separator decoration so this is a visual
/// approximation rather than a full-width rule.
///
/// `panel.selected` is a flat row index across visible rows (section
/// headers count, items in collapsed sections do not). The matching
/// `selected_path` is computed by walking the same flat enumeration
/// while emitting rows.
///
/// Tree-item expansion state (`engine.ext_panel_tree_expanded`) is
/// expected to have already been resolved into `item.expanded` by
/// `build_ext_panel_data` before this function is called.
pub fn ext_panel_to_tree_view(panel: &ExtPanelData, theme: &Theme) -> quadraui::TreeView {
    use crate::core::plugin::ExtPanelStyle;
    use quadraui::{
        Badge, Decoration, SelectionMode, StyledSpan, StyledText, TreeRow, TreeStyle, TreeView,
        WidgetId,
    };

    let accent_color = to_quadraui_color(theme.keyword);
    let mut rows: Vec<TreeRow> = Vec::new();
    let mut selected_path: Option<Vec<u16>> = None;
    let mut flat_idx = 0usize;

    for (s, section) in panel.sections.iter().enumerate() {
        if panel.has_focus && flat_idx == panel.selected {
            selected_path = Some(vec![s as u16]);
        }
        rows.push(TreeRow {
            path: vec![s as u16],
            indent: 0,
            icon: None,
            text: StyledText::plain(section.name.clone()),
            badge: None,
            is_expanded: Some(section.expanded),
            decoration: Decoration::Header,
            edit: None,
        });
        flat_idx += 1;

        if !section.expanded {
            continue;
        }

        for (i, item) in section.items.iter().enumerate() {
            let row_path = vec![s as u16, i as u16];
            if panel.has_focus && flat_idx == panel.selected {
                selected_path = Some(row_path.clone());
            }

            if item.is_separator {
                rows.push(TreeRow {
                    path: row_path,
                    indent: 0,
                    icon: None,
                    text: StyledText::plain("\u{2500}".to_string()),
                    badge: None,
                    is_expanded: None,
                    decoration: Decoration::Muted,
                    edit: None,
                });
                flat_idx += 1;
                continue;
            }

            let (decoration, text) = match item.style {
                ExtPanelStyle::Header => (Decoration::Header, StyledText::plain(item.text.clone())),
                ExtPanelStyle::Dim => (Decoration::Muted, StyledText::plain(item.text.clone())),
                ExtPanelStyle::Accent => (
                    Decoration::Normal,
                    StyledText {
                        spans: vec![StyledSpan::with_fg(item.text.clone(), accent_color)],
                    },
                ),
                ExtPanelStyle::Normal => (Decoration::Normal, StyledText::plain(item.text.clone())),
            };

            let mut parts: Vec<String> = Vec::new();
            for badge in &item.badges {
                parts.push(format!("[{}]", badge.text));
            }
            for action in &item.actions {
                parts.push(format!("\u{27e8}{}\u{27e9}", action.label));
            }
            if !item.hint.is_empty() {
                parts.push(item.hint.clone());
            }
            let badge = if parts.is_empty() {
                None
            } else {
                Some(Badge::plain(parts.join(" ")))
            };

            let icon = if item.icon.is_empty() {
                None
            } else {
                Some(quadraui::Icon::new(item.icon.clone(), item.icon.clone()))
            };

            let is_expanded = if item.expandable {
                Some(item.expanded)
            } else {
                None
            };

            rows.push(TreeRow {
                path: row_path,
                indent: item.indent as u16 + 1,
                icon,
                text,
                badge,
                is_expanded,
                decoration,
                edit: None,
            });
            flat_idx += 1;
        }
    }

    TreeView {
        id: WidgetId::new("ext-panel-tree"),
        rows,
        selection_mode: SelectionMode::Single,
        selected_path,
        scroll_offset: panel.scroll_top,
        style: TreeStyle::default(),
        has_focus: panel.has_focus,
    }
}

/// Adapt the engine-side `ExtSidebarData` into a `quadraui::MultiSectionView`
/// (#293).
///
/// Two sections — `"installed"` and `"available"` — each carries its own
/// `TreeView` body of item rows only (the section title is rendered as
/// the section header, not as a tree-row). Both sections size as
/// `EqualShare` and scroll independently (`ScrollMode::PerSection`).
///
/// Selection mapping: `ExtSidebarData.selected` is a flat index across
/// installed-then-available items. Within a section, each TreeView's
/// `selected_path` is `vec![item_idx as u16]` for that section's items.
///
/// Empty-state rows (`(none installed)` / `(all installed)` /
/// `Fetching registry…`) appear as `Decoration::Muted` rows in the
/// section's tree, intentionally not in the selection mapping.
pub fn ext_sidebar_to_multi_section_view(ext: &ExtSidebarData) -> quadraui::MultiSectionView {
    use quadraui::{
        Badge, Decoration, EmptyBody, MsvAxis, MultiSectionView, ScrollMode, Section, SectionBody,
        SectionHeader, SectionSize, SelectionMode, StyledText, TreeRow, TreeStyle, TreeView,
        WidgetId,
    };

    let installed_count = ext.items_installed.len();
    let available_count = ext.items_available.len();

    let (sel_section, sel_item) = if ext.selected < installed_count {
        (0u16, ext.selected)
    } else {
        (1u16, ext.selected.saturating_sub(installed_count))
    };

    // ── Build INSTALLED tree ─────────────────────────────────────────
    let installed_rows: Vec<TreeRow> = if installed_count == 0 {
        Vec::new()
    } else {
        ext.items_installed
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_sel = ext.has_focus && sel_section == 0 && sel_item == i;
                let label = if item.update_available {
                    format!("\u{25cf} {} \u{2191}", item.display_name)
                } else {
                    format!("\u{25cf} {}", item.display_name)
                };
                let badge = if is_sel {
                    let hint = if item.update_available {
                        "[u]update"
                    } else {
                        "[d]remove"
                    };
                    Some(Badge::plain(hint.to_string()))
                } else {
                    None
                };
                TreeRow {
                    path: vec![i as u16],
                    indent: 0,
                    icon: None,
                    text: StyledText::plain(label),
                    badge,
                    is_expanded: None,
                    decoration: Decoration::Normal,
                    edit: None,
                }
            })
            .collect()
    };

    let installed_selected_path = if ext.has_focus && sel_section == 0 && sel_item < installed_count
    {
        Some(vec![sel_item as u16])
    } else {
        None
    };

    let installed_body = if installed_rows.is_empty() {
        SectionBody::Empty(EmptyBody {
            icon: None,
            text: StyledText::plain("(none installed)".to_string()),
            hint: None,
            action: None,
        })
    } else {
        SectionBody::Tree(TreeView {
            id: WidgetId::new("ext-sidebar-installed-tree"),
            rows: installed_rows,
            selection_mode: SelectionMode::Single,
            selected_path: installed_selected_path,
            scroll_offset: 0,
            style: TreeStyle::default(),
            has_focus: ext.has_focus && sel_section == 0,
        })
    };

    // ── Build AVAILABLE tree ─────────────────────────────────────────
    let available_rows: Vec<TreeRow> = if available_count == 0 {
        Vec::new()
    } else {
        ext.items_available
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_sel = ext.has_focus && sel_section == 1 && sel_item == i;
                let badge = if is_sel {
                    Some(Badge::plain("[i]install".to_string()))
                } else {
                    None
                };
                TreeRow {
                    path: vec![i as u16],
                    indent: 0,
                    icon: None,
                    text: StyledText::plain(format!("\u{25cb} {}", item.display_name)),
                    badge,
                    is_expanded: None,
                    decoration: Decoration::Normal,
                    edit: None,
                }
            })
            .collect()
    };

    let available_selected_path = if ext.has_focus && sel_section == 1 && sel_item < available_count
    {
        Some(vec![sel_item as u16])
    } else {
        None
    };

    let available_body = if available_rows.is_empty() {
        let msg = if ext.fetching {
            "Fetching registry\u{2026}"
        } else {
            "(all installed)"
        };
        SectionBody::Empty(EmptyBody {
            icon: None,
            text: StyledText::plain(msg.to_string()),
            hint: None,
            action: None,
        })
    } else {
        SectionBody::Tree(TreeView {
            id: WidgetId::new("ext-sidebar-available-tree"),
            rows: available_rows,
            selection_mode: SelectionMode::Single,
            selected_path: available_selected_path,
            scroll_offset: 0,
            style: TreeStyle::default(),
            has_focus: ext.has_focus && sel_section == 1,
        })
    };

    let installed_section = Section {
        id: "installed".to_string(),
        header: SectionHeader {
            icon: None,
            title: StyledText::plain("INSTALLED".to_string()),
            badge: Some(StyledText::plain(format!("({installed_count})"))),
            actions: Vec::new(),
            show_chevron: true,
        },
        body: installed_body,
        aux: None,
        size: SectionSize::EqualShare,
        collapsed: !ext.sections_expanded[0],
        min_size: None,
        max_size: None,
    };

    let available_section = Section {
        id: "available".to_string(),
        header: SectionHeader {
            icon: None,
            title: StyledText::plain("AVAILABLE".to_string()),
            badge: Some(StyledText::plain(format!("({available_count})"))),
            actions: Vec::new(),
            show_chevron: true,
        },
        body: available_body,
        aux: None,
        size: SectionSize::EqualShare,
        collapsed: !ext.sections_expanded[1],
        min_size: None,
        max_size: None,
    };

    MultiSectionView {
        id: WidgetId::new("ext-sidebar-msv"),
        sections: vec![installed_section, available_section],
        active_section: Some(sel_section as usize),
        axis: MsvAxis::Vertical,
        allow_resize: false,
        allow_collapse: true,
        // WholePanel mode: sections size to their own content height
        // and stack at deterministic positions. The panel scrolls as a
        // unit when total content exceeds the visible body area (matches
        // VSCode Extensions panel UX: INSTALLED grows with item count,
        // AVAILABLE flows below it). Critical for click hit-testing —
        // section boundaries don't depend on the bounds.height the
        // backend passes, so paint and click see the same layout
        // regardless of which area each measures.
        scroll_mode: ScrollMode::WholePanel,
        has_focus: ext.has_focus,
        panel_scroll: ext.panel_scroll,
    }
}

pub use crate::core::engine::ExplorerRow;

/// Adapt a flat explorer row list into a `quadraui::TreeView` for the
/// shared `draw_tree` primitive. Each backend drives its own flat-row
/// model (GTK via `ExplorerState`, Win-GUI via `WinSidebar`) and calls
/// this adapter on every draw.
///
/// Overlays per-row git status letters and LSP diagnostic counts via
/// `engine.explorer_indicators()` — the cached indicator map keyed by
/// canonical path. Directories get a folder glyph; files get the
/// extension-based icon from `icons::file_icon`.
/// Adapt the picker panel's `PickerPanel` render data into a generic
/// `quadraui::Palette` for rendering through the shared primitive.
///
/// Phase A.4 scope: flat-list palettes only. Returns `None` when the
/// caller should fall through to the legacy renderer:
/// - `preview.is_some()` — file / symbol picker with right-side preview pane
/// - any item has `depth > 0` or `expandable` — tree-structured picker
///
/// When `Some(Palette)` is returned, the backend can render the full
/// modal via `quadraui_tui::draw_palette` (TUI) or `quadraui_gtk::draw_palette`
/// (GTK, when A.4b ships).
pub fn picker_panel_to_palette(picker: &PickerPanel) -> quadraui::Palette {
    use quadraui::{Palette, PaletteItem, PalettePreview, StyledText, WidgetId};

    let items: Vec<PaletteItem> = picker
        .items
        .iter()
        .map(|it| PaletteItem {
            text: StyledText::plain(&it.display),
            detail: it.detail.as_deref().map(StyledText::plain),
            icon: None,
            match_positions: it.match_positions.clone(),
            depth: it.depth,
            expandable: it.expandable,
            expanded: it.expanded,
        })
        .collect();

    let preview = picker.preview.as_ref().map(|lines| {
        let highlight_line = lines.iter().position(|&(_, _, hl)| hl);
        PalettePreview {
            lines: lines
                .iter()
                .map(|(line_num, text, _)| StyledText::plain(format!("{line_num:4}: {text}")))
                .collect(),
            title: None,
            scroll_offset: picker.preview_scroll,
            highlight_line,
        }
    });

    Palette {
        id: WidgetId::new("picker"),
        title: picker.title.clone(),
        query: picker.query.clone(),
        query_cursor: picker.query.len(),
        items,
        selected_idx: picker.selected_idx,
        scroll_offset: picker.scroll_top,
        total_count: picker.total_count,
        has_focus: true,
        show_query: true,
        create_label: None,
        preview,
        mode: quadraui::PaletteMode::List,
    }
}

/// Convert `Engine`'s settings state into a generic `quadraui::Form`
/// for rendering through either `quadraui_tui::draw_form` (A.3b) or
/// `quadraui_gtk::draw_form` (A.3c). Backend-agnostic; reads only
/// engine fields.
///
/// Scope: covers the scrollable field list. Callers still handle the
/// panel header / search input / scrollbar themselves.
///
/// Field type mapping:
/// - `CoreCategory` / `ExtCategory` → `FieldKind::Label` (collapsible header)
/// - `CoreSetting` with `Bool` → `FieldKind::Toggle`
/// - `CoreSetting` currently being inline-edited (`engine.settings_editing
///   == Some(idx)`) → `FieldKind::TextInput` sourced from
///   `engine.settings_edit_buf`, with `cursor` set to the buffer's byte
///   length (edits are append/backspace-only — the cursor always sits at
///   the end, see `Engine::handle_settings_key`).
/// - `CoreSetting` with any other type, not being edited → `FieldKind::ReadOnly`
///   (enum cycling / numeric / string values still work — keys are
///   handled by `engine.handle_settings_key()`; the adapter just shows
///   the current value)
/// - `ExtSetting` mapped analogously via the manifest's declared type,
///   using `engine.ext_settings_editing` for the inline-edit check.
pub fn settings_to_form(engine: &Engine) -> quadraui::Form {
    use crate::core::engine::SettingsRow;
    use crate::core::settings::{setting_categories, SettingType, SETTING_DEFS};
    use quadraui::{FieldKind, Form, FormField, StyledText, WidgetId};

    let flat = engine.settings_flat_list();
    let cats = setting_categories();

    let mut fields: Vec<FormField> = Vec::with_capacity(flat.len());
    for row in &flat {
        let field = match row {
            SettingsRow::CoreCategory(cat_idx) => {
                let collapsed = *cat_idx < engine.settings_collapsed.len()
                    && engine.settings_collapsed[*cat_idx];
                let arrow = if collapsed { "▶ " } else { "▼ " };
                let cat_name = cats.get(*cat_idx).copied().unwrap_or("?");
                FormField {
                    id: WidgetId::new(format!("cat-{}", cat_idx)),
                    label: StyledText::plain(format!("{}{}", arrow, cat_name)),
                    kind: FieldKind::Label,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
            SettingsRow::ExtCategory(name) => {
                let collapsed = engine
                    .ext_settings_collapsed
                    .get(name)
                    .copied()
                    .unwrap_or(false);
                let arrow = if collapsed { "▶ " } else { "▼ " };
                let display = engine
                    .ext_available_manifests()
                    .into_iter()
                    .find(|m| &m.name == name)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_else(|| name.clone());
                FormField {
                    id: WidgetId::new(format!("ext-cat-{}", name)),
                    label: StyledText::plain(format!("{}{}", arrow, display)),
                    kind: FieldKind::Label,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
            SettingsRow::CoreSetting(idx) => {
                let def = &SETTING_DEFS[*idx];
                let kind = if engine.settings_editing == Some(*idx) {
                    FieldKind::TextInput {
                        value: engine.settings_edit_buf.clone(),
                        placeholder: String::new(),
                        cursor: Some(engine.settings_edit_buf.len()),
                        selection_anchor: None,
                    }
                } else {
                    let value_str = engine.settings.get_value_str(def.key);
                    match def.setting_type {
                        SettingType::Bool => FieldKind::Toggle {
                            value: value_str == "true",
                        },
                        _ => FieldKind::ReadOnly {
                            value: StyledText::plain(value_str),
                        },
                    }
                };
                FormField {
                    id: WidgetId::new(format!("setting-{}", idx)),
                    label: StyledText::plain(def.label),
                    kind,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
            SettingsRow::ExtSetting(ext_name, key) => {
                let editing_this = engine
                    .ext_settings_editing
                    .as_ref()
                    .is_some_and(|(en, ek)| en == ext_name && ek == key);
                let def_opt = engine.find_ext_setting_def(ext_name, key);
                let label_str = def_opt
                    .as_ref()
                    .map(|d| {
                        if d.label.is_empty() {
                            key.clone()
                        } else {
                            d.label.clone()
                        }
                    })
                    .unwrap_or_else(|| key.clone());
                let kind = if editing_this {
                    FieldKind::TextInput {
                        value: engine.settings_edit_buf.clone(),
                        placeholder: String::new(),
                        cursor: Some(engine.settings_edit_buf.len()),
                        selection_anchor: None,
                    }
                } else {
                    let value_str = engine.get_ext_setting(ext_name, key);
                    let is_bool = def_opt.as_ref().is_some_and(|d| d.r#type == "bool");
                    if is_bool {
                        FieldKind::Toggle {
                            value: value_str == "true",
                        }
                    } else {
                        FieldKind::ReadOnly {
                            value: StyledText::plain(value_str),
                        }
                    }
                };
                FormField {
                    id: WidgetId::new(format!("ext-setting-{}-{}", ext_name, key)),
                    label: StyledText::plain(label_str),
                    kind,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
        };
        fields.push(field);
    }

    let focused_field = fields.get(engine.settings_selected).map(|f| f.id.clone());

    Form {
        id: WidgetId::new("settings"),
        fields,
        focused_field,
        scroll_offset: engine.settings_scroll_top,
        has_focus: engine.settings_has_focus,
    }
}

/// Populate the engine's `settings_form_controller` with current form
/// data and scroll state. Call before `FormController::render()` or
/// `FormController::handle()`.
pub fn populate_settings_form_controller(engine: &Engine) {
    let form = settings_to_form(engine);
    let mut fc = engine.settings_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.settings_scroll_top);
    fc.set_has_focus(engine.settings_has_focus);
}

/// Route a pointer event over the Settings panel through the shared
/// `quadraui::FormController` and apply the result to engine state.
///
/// `rect` is the panel's content area in the caller's own coordinate space —
/// the *same* rect the last frame passed to
/// `FormController::render_and_cache`, since `handle_cached` re-derives its
/// row layout from it. Returns `true` when the event was consumed.
///
/// This is the click twin of [`populate_settings_form_controller`], and it
/// exists so neither backend has to re-derive the panel's row geometry by
/// hand: before #544 GTK computed `row_h = line_height * 1.4`, a header/search
/// band and a scrollbar gutter from a `DrawingArea`'s own width/height, none of
/// which survive the ShellApp migration (there is no per-panel DrawingArea any
/// more, so every one of those numbers read back `0`). `FormController` already
/// owns all of it and is the only thing that painted the rows.
///
/// Activation policy matches the keyboard path and the pre-migration GTK/TUI
/// mouse paths: a `Toggle` field flips on a single click, a category header
/// expands/collapses on a single click, and a value row selects on a single
/// click but only *activates* (opens the inline editor / cycles an enum) on a
/// double click.
pub fn handle_settings_form_ui_event(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
) -> bool {
    use crate::core::engine::SettingsRow;

    // `FormController` has no `DoubleClick` arm — probe with the equivalent
    // press and remember that the caller asked for activation.
    let (probe, activate_row) = match event {
        quadraui::UiEvent::DoubleClick { widget, position } => (
            quadraui::UiEvent::MouseDown {
                widget: widget.clone(),
                button: quadraui::MouseButton::Left,
                position: *position,
                modifiers: quadraui::Modifiers::default(),
            },
            true,
        ),
        other => (other.clone(), false),
    };

    populate_settings_form_controller(engine);
    let result = engine
        .settings_form_controller
        .borrow_mut()
        .handle_cached(&probe, rect);

    let sync_scroll = |engine: &mut Engine| {
        let offset = engine.settings_form_controller.borrow().scroll_offset();
        engine.settings_scroll_top = offset;
    };

    match result {
        quadraui::FormControllerEvent::Ignored => false,
        quadraui::FormControllerEvent::ScrollChanged | quadraui::FormControllerEvent::Consumed => {
            sync_scroll(engine);
            true
        }
        quadraui::FormControllerEvent::FormAction(action) => {
            let (id, activates) = match action {
                quadraui::FormEvent::ToggleChanged { id, .. } => (id, true),
                quadraui::FormEvent::ButtonClicked { id } => (id, true),
                quadraui::FormEvent::FocusChanged { id } => (id, activate_row),
                _ => return true,
            };
            // The form's fields are built 1:1 from `settings_flat_list()`
            // (see `settings_to_form`), so the field's position *is* the flat
            // index `settings_selected` indexes — read it off the controller
            // rather than re-parsing the id string, so the two can't drift.
            let idx = engine
                .settings_form_controller
                .borrow()
                .form()
                .and_then(|f| f.fields.iter().position(|field| field.id == id));
            let Some(idx) = idx else {
                return true;
            };
            engine.settings_has_focus = true;
            engine.settings_selected = idx;
            sync_scroll(engine);
            let is_category = matches!(
                engine.settings_flat_list().get(idx),
                Some(SettingsRow::CoreCategory(_)) | Some(SettingsRow::ExtCategory(_))
            );
            if activates || is_category {
                engine.handle_settings_key("Return", false, None);
            }
            true
        }
    }
}

/// Adapt the quickfix panel data into a generic `quadraui::ListView`.
///
/// The quickfix panel is a simple flat list of pre-formatted strings
/// with a header. `ListView` maps one-to-one. No decoration per row
/// because the input strings don't carry severity info; future
/// enhancement: parse severity from the text or extend
/// `QuickfixPanel` to carry `Decoration`.
pub fn quickfix_to_list_view(qf: &QuickfixPanel) -> quadraui::ListView {
    use quadraui::{ListItem, ListView, StyledText, WidgetId};

    let focus_mark = if qf.has_focus { " [FOCUS]" } else { "" };
    let title_text = format!(" QUICKFIX ({} items){}", qf.total_items, focus_mark);

    let items: Vec<ListItem> = qf
        .items
        .iter()
        .map(|s| ListItem {
            text: StyledText::plain(s),
            icon: None,
            detail: None,
            decoration: quadraui::Decoration::Normal,
        })
        .collect();

    ListView {
        id: WidgetId::new("quickfix"),
        title: Some(StyledText::plain(title_text)),
        items,
        selected_idx: qf.selected_idx,
        scroll_offset: 0, // set by caller from local scroll_top
        has_focus: qf.has_focus,
        bordered: false,
        h_scroll: 0,
        max_content_width: None,
        show_v_scrollbar: false,
    }
}

fn build_ext_sidebar_data(engine: &Engine) -> Option<ExtSidebarData> {
    // Always build so backends can check ext_sidebar_has_focus.
    let manifest_to_item = |m: &crate::core::extensions::ExtensionManifest,
                            installed: bool,
                            has_update: bool|
     -> ExtSidebarItem {
        ExtSidebarItem {
            name: m.name.clone(),
            display_name: if m.display_name.is_empty() {
                m.name.clone()
            } else {
                m.display_name.clone()
            },
            description: m.description.clone(),
            lsp_binary: m.lsp.binary.clone(),
            dap_adapter: m.dap.adapter.clone(),
            script_count: m.scripts.len(),
            installed,
            update_available: has_update,
        }
    };

    let items_installed: Vec<ExtSidebarItem> = engine
        .ext_available_manifests()
        .iter()
        .filter(|m| engine.extension_state.is_installed(&m.name))
        .filter(|m| {
            let q = engine.ext_sidebar_query.to_lowercase();
            q.is_empty()
                || m.name.to_lowercase().contains(&q)
                || m.display_name.to_lowercase().contains(&q)
        })
        .map(|m| manifest_to_item(m, true, engine.ext_has_update(&m.name)))
        .collect();

    let items_available: Vec<ExtSidebarItem> = engine
        .ext_available_manifests()
        .iter()
        .filter(|m| !engine.extension_state.is_installed(&m.name))
        .filter(|m| {
            let q = engine.ext_sidebar_query.to_lowercase();
            q.is_empty()
                || m.name.to_lowercase().contains(&q)
                || m.display_name.to_lowercase().contains(&q)
        })
        .map(|m| manifest_to_item(m, false, false))
        .collect();

    Some(ExtSidebarData {
        items_installed,
        items_available,
        sections_expanded: engine.ext_sidebar_sections_expanded,
        selected: engine.ext_sidebar_selected,
        has_focus: engine.ext_sidebar_has_focus,
        query: engine.ext_sidebar_query.clone(),
        input_active: engine.ext_sidebar_input_active,
        fetching: engine.ext_registry_fetching,
        panel_scroll: engine.ext_sidebar_panel_scroll,
    })
}

fn build_ext_panel_data(engine: &Engine) -> Option<ExtPanelData> {
    let panel_name = engine.ext_panel_active.as_ref()?;
    let reg = engine.ext_panels.get(panel_name)?;
    let expanded_vec = engine.ext_panel_sections_expanded.get(panel_name);
    let sections: Vec<ExtPanelSectionData> = reg
        .sections
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let expanded = expanded_vec.and_then(|v| v.get(i)).copied().unwrap_or(true);
            let key = (panel_name.clone(), name.clone());
            let all_items = engine
                .ext_panel_items
                .get(&key)
                .cloned()
                .unwrap_or_default();
            // Filter items for tree visibility (hide children of collapsed tree nodes)
            let visible_indices = engine.ext_panel_visible_indices(panel_name, &all_items);
            let items: Vec<_> = visible_indices
                .into_iter()
                .filter_map(|idx| {
                    all_items.get(idx).cloned().map(|mut item| {
                        // Resolve user-toggled tree expansion state into the
                        // item so `ext_panel_to_tree_view` doesn't need engine
                        // access. The engine map is the source of truth once
                        // the user has toggled; `item.expanded` is the plugin
                        // default otherwise.
                        if item.expandable {
                            let key = (panel_name.clone(), item.id.clone());
                            if let Some(&v) = engine.ext_panel_tree_expanded.get(&key) {
                                item.expanded = v;
                            }
                        }
                        item
                    })
                })
                .collect();
            ExtPanelSectionData {
                name: name.clone(),
                items,
                expanded,
            }
        })
        .collect();
    Some(ExtPanelData {
        name: panel_name.clone(),
        title: reg.title.clone(),
        sections,
        selected: engine.ext_panel_selected,
        has_focus: engine.ext_panel_has_focus,
        scroll_top: engine.ext_panel_scroll_top,
        input_text: engine
            .ext_panel_input_text
            .get(panel_name)
            .cloned()
            .unwrap_or_default(),
        input_active: engine.ext_panel_input_active,
        help_open: engine.ext_panel_help_open,
        help_bindings: engine
            .ext_panel_help_bindings
            .get(panel_name)
            .cloned()
            .unwrap_or_default(),
    })
}

fn build_ai_panel_data(engine: &Engine) -> Option<AiPanelData> {
    // Always build so backends can check ai_has_focus.
    let messages = engine
        .ai_messages
        .iter()
        .map(|m| AiPanelMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();
    Some(AiPanelData {
        messages,
        input: engine.ai_input.clone(),
        has_focus: engine.ai_has_focus,
        input_active: engine.ai_input_active,
        streaming: engine.ai_streaming,
        scroll_top: engine.ai_scroll_top,
        input_cursor: engine.ai_input_cursor,
    })
}

/// Build the cell grid for a single terminal session.
///
/// Uses `TerminalSession::to_terminal()` to get the base snapshot from the
/// quadraui primitive. Post-processes in place: clears `is_cursor` when
/// `cursor_active` is `false`, and stamps find-match highlights.
///
/// `find` is `(matches, qlen, active_match_idx)`.
#[allow(clippy::type_complexity)]
fn build_pane_rows(
    sess: &quadraui::terminal_engine::TerminalSession,
    cursor_active: bool,
    find: Option<(&[(usize, u16, u16)], usize, usize)>,
) -> Vec<Vec<quadraui::TerminalCell>> {
    // Build snapshot — quadraui handles history blending, scroll offset, selection, cursor.
    // The WidgetId is a placeholder; only `snapshot.cells` is used here — the Terminal
    // struct is immediately destructured and the id is discarded.  The scrollbar is
    // built separately in `build_terminal_draw_data` from `TerminalPanel` fields.
    let snapshot = sess.to_terminal(quadraui::WidgetId::new("_pane"), None);
    let scroll_offset = sess.scroll_offset();
    let rows_count = snapshot.cells.len();

    let mut rows = snapshot.cells;

    // Clear cursor marker when this pane is not the focused one.
    if !cursor_active {
        for row in &mut rows {
            for cell in row {
                cell.is_cursor = false;
            }
        }
    }

    // Apply find-match highlights.
    if let Some((matches, qlen, active_idx)) = find {
        let current_offset = scroll_offset as isize;
        let term_rows = rows_count as isize;
        for (mi, &(moffset, mr, mc)) in matches.iter().enumerate() {
            let visible_row = mr as isize + current_offset - moffset as isize;
            if visible_row < 0 || visible_row >= term_rows {
                continue;
            }
            let row_idx = visible_row as usize;
            if row_idx < rows.len() {
                for char_off in 0..qlen {
                    let col_idx = mc as usize + char_off;
                    if col_idx < rows[row_idx].len() {
                        if mi == active_idx {
                            rows[row_idx][col_idx].is_find_active = true;
                        } else {
                            rows[row_idx][col_idx].is_find_match = true;
                        }
                    }
                }
            }
        }
    }

    rows
}

/// Build the TerminalPanel from engine state (when terminal is open).
fn build_terminal_panel(engine: &Engine) -> Option<TerminalPanel> {
    if !engine.terminal_open {
        return None;
    }

    // Prepare find-highlight data (applies only to the focused/active pane).
    let match_count = engine.terminal_find_matches.len();
    let find_selected_idx = if match_count > 0 {
        engine.terminal_find_selected % match_count
    } else {
        0
    };
    #[allow(clippy::type_complexity)]
    let find_data: Option<(&[(usize, u16, u16)], usize, usize)> =
        if engine.terminal_find_active && match_count > 0 {
            Some((
                &engine.terminal_find_matches,
                engine.terminal_find_query.chars().count(),
                find_selected_idx,
            ))
        } else {
            None
        };

    // ── Split view: two panes side-by-side ────────────────────────────────────
    if engine.terminal_split && engine.terminal_panes.len() >= 2 {
        let left_pane = &engine.terminal_panes[0].session;
        let right_pane = &engine.terminal_panes[1].session;
        let left_cursor_active = engine.terminal_has_focus && engine.terminal_active == 0;
        let right_cursor_active = engine.terminal_has_focus && engine.terminal_active == 1;

        // Find highlights only shown in the focused pane.
        let left_find = if engine.terminal_active == 0 {
            find_data
        } else {
            None
        };
        let right_find = if engine.terminal_active == 1 {
            find_data
        } else {
            None
        };

        let split_left_rows = build_pane_rows(left_pane, left_cursor_active, left_find);
        let rows = build_pane_rows(right_pane, right_cursor_active, right_find);

        // Active pane supplies scroll / scrollback for the scrollbar.
        let active_pane = if engine.terminal_active == 1 {
            right_pane
        } else {
            left_pane
        };

        return Some(TerminalPanel {
            rows,
            content_rows: right_pane.rows(),
            content_cols: right_pane.cols(),
            has_focus: engine.terminal_has_focus,
            scroll_offset: active_pane.scroll_offset(),
            scrollback_rows: active_pane.history_len(),
            tab_count: engine.terminal_panes.len(),
            active_tab: engine.terminal_active,
            find_active: engine.terminal_find_active,
            find_query: engine.terminal_find_query.clone(),
            find_match_count: match_count,
            find_selected_idx,
            split_left_rows: Some(split_left_rows),
            split_left_cols: if engine.terminal_split_left_cols > 0 {
                engine.terminal_split_left_cols
            } else {
                left_pane.cols()
            },
            split_focus: engine.terminal_active as u8,
            maximized: engine.terminal_maximized,
        });
    }

    // ── Single-pane (normal) view ──────────────────────────────────────────────
    let term = engine.active_terminal()?;
    let hist_len = term.history_len();
    let scroll_offset = term.scroll_offset();
    let cursor_active = engine.terminal_has_focus;
    let rows = build_pane_rows(term, cursor_active, find_data);

    Some(TerminalPanel {
        rows,
        content_rows: term.rows(),
        content_cols: term.cols(),
        has_focus: engine.terminal_has_focus,
        scroll_offset,
        scrollback_rows: hist_len,
        tab_count: engine.terminal_panes.len(),
        active_tab: engine.terminal_active,
        find_active: engine.terminal_find_active,
        find_query: engine.terminal_find_query.clone(),
        find_match_count: match_count,
        find_selected_idx,
        split_left_rows: None,
        split_left_cols: 0,
        split_focus: 0,
        maximized: engine.terminal_maximized,
    })
}

/// Build breadcrumb segments for the active editor group (public API for click handlers).
pub fn build_breadcrumbs_for_active_group(engine: &Engine) -> Vec<BreadcrumbSegment> {
    build_breadcrumbs_for_group(engine, engine.active_group)
}

/// Build breadcrumb segments for a single editor group.
fn build_breadcrumbs_for_group(engine: &Engine, group_id: GroupId) -> Vec<BreadcrumbSegment> {
    let group = match engine.editor_groups.get(&group_id) {
        Some(g) => g,
        None => return vec![],
    };
    let window_id = group.tabs[group.active_tab].active_window;
    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return vec![],
    };
    let buf_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return vec![],
    };

    let mut segments = Vec::new();
    let mut idx = 0usize;

    // Path segments (relative to cwd)
    if let Some(ref file_path) = buf_state.file_path {
        let clean_path = crate::core::paths::strip_unc_prefix(file_path);
        let clean_cwd = crate::core::paths::strip_unc_prefix(&engine.cwd);
        let display = if let Ok(rel) = clean_path.strip_prefix(clean_cwd.as_ref()) {
            rel.to_string_lossy().to_string()
        } else {
            clean_path.to_string_lossy().to_string()
        };
        let parts: Vec<&str> = display.split(std::path::MAIN_SEPARATOR).collect();
        let mut accumulated = engine.cwd.clone();
        for part in &parts {
            accumulated = accumulated.join(part);
            segments.push(BreadcrumbSegment {
                label: part.to_string(),
                is_last: false,
                is_symbol: false,
                index: idx,
                path_prefix: Some(accumulated.clone()),
                symbol_line: None,
            });
            idx += 1;
        }
    }

    // Symbol segments from tree-sitter
    {
        let cursor = &window.view.cursor;
        let text = buf_state.buffer.to_string();
        let scopes = if let Some(ref syn) = buf_state.syntax {
            syn.enclosing_scopes(&text, cursor.line, cursor.col)
        } else {
            Vec::new()
        };
        for scope in scopes {
            segments.push(BreadcrumbSegment {
                label: scope.name,
                is_last: false,
                is_symbol: true,
                index: idx,
                path_prefix: None,
                symbol_line: Some(scope.line),
            });
            idx += 1;
        }
    }

    // Mark the last segment
    if let Some(last) = segments.last_mut() {
        last.is_last = true;
    }

    segments
}

// ─── Private builder helpers ──────────────────────────────────────────────────

fn build_tab_bar_for_group_by_id(engine: &Engine, group_id: GroupId) -> Vec<TabInfo> {
    let group = match engine.editor_groups.get(&group_id) {
        Some(g) => g,
        None => return vec![],
    };
    group
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let active = i == group.active_tab;
            let window_id = tab.active_window;
            let (name, dirty, preview) = if let Some(window) = engine.windows.get(&window_id) {
                if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                    (
                        format!(" {}: {} ", i + 1, state.display_name()),
                        state.dirty,
                        state.preview,
                    )
                } else {
                    (format!(" {}: [No Name] ", i + 1), false, false)
                }
            } else {
                (format!(" {}: [No Name] ", i + 1), false, false)
            };
            TabInfo {
                name,
                active,
                dirty,
                preview,
            }
        })
        .collect()
}

fn build_tab_bar(engine: &Engine) -> Vec<TabInfo> {
    // ScreenLayout.tab_bar always holds the first group's tabs.
    let first_id = engine.group_layout.group_ids().first().copied();
    match first_id {
        Some(gid) => build_tab_bar_for_group_by_id(engine, gid),
        None => vec![],
    }
}

/// Return the number of visual rows a buffer line of `line_char_len` characters
/// occupies when the viewport is `viewport_cols` columns wide.
/// Always returns at least 1 (even for empty lines).
pub fn visual_rows_for_line(line_char_len: usize, viewport_cols: usize) -> usize {
    if viewport_cols == 0 {
        return 1;
    }
    line_char_len.div_ceil(viewport_cols).max(1)
}

/// Compute word-aware wrap segment boundaries for a line.
/// Returns a list of `(start_char, end_char)` pairs. Breaks prefer word boundaries
/// (spaces, hyphens, punctuation) so words are not split mid-way.
pub fn compute_word_wrap_segments(line: &str, viewport_cols: usize) -> Vec<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let total = chars.len();
    if viewport_cols == 0 || total <= viewport_cols {
        return vec![(0, total)];
    }
    let mut segments = Vec::new();
    let mut pos = 0;
    while pos < total {
        let remaining = total - pos;
        if remaining <= viewport_cols {
            segments.push((pos, total));
            break;
        }
        let end = pos + viewport_cols;
        // Scan backwards from the break point to find a word boundary (space or after punctuation).
        let mut break_at = end;
        for i in (pos + 1..=end).rev() {
            if chars[i - 1] == ' ' || chars[i - 1] == '-' || chars[i - 1] == '/' {
                break_at = i;
                break;
            }
        }
        // If no boundary found within the segment, hard-break at viewport width.
        if break_at == end && !chars[end - 1].is_whitespace() {
            // Check if we found a boundary at all (break_at didn't change means
            // the for loop completed without breaking).
            let found = (pos + 1..=end)
                .rev()
                .any(|i| chars[i - 1] == ' ' || chars[i - 1] == '-' || chars[i - 1] == '/');
            if !found {
                break_at = end;
            }
        }
        segments.push((pos, break_at));
        // Safety: guarantee forward progress to prevent infinite loops.
        pos = break_at.max(pos + 1);
    }
    segments
}

/// Map a visible row index (0-based from scroll_top) to the corresponding
/// buffer line index, skipping lines hidden inside closed folds.
/// Shared across all GUI backends for click hit-testing.
pub fn view_row_to_buf_line(
    view: &crate::core::view::View,
    scroll_top: usize,
    view_row: usize,
    total_lines: usize,
) -> usize {
    let mut buf_line = scroll_top;
    let mut visible = 0usize;
    while buf_line < total_lines {
        if view.is_line_hidden(buf_line) {
            buf_line += 1;
            continue;
        }
        if visible == view_row {
            return buf_line;
        }
        visible += 1;
        if let Some(fold) = view.fold_at(buf_line) {
            buf_line = fold.end + 1;
        } else {
            buf_line += 1;
        }
    }
    // Clamp to last valid line
    total_lines.saturating_sub(1)
}

/// Like `view_row_to_buf_line`, but accounts for word-wrapped lines.
/// Returns `(buffer_line, segment_col_offset)` — the segment offset is the
/// character index within the buffer line where the clicked visual segment starts.
/// Shared across all GUI backends for click hit-testing with `:set wrap`.
pub fn view_row_to_buf_pos_wrap(
    view: &crate::core::view::View,
    buffer: &crate::core::buffer::Buffer,
    scroll_top: usize,
    view_row: usize,
    total_lines: usize,
    viewport_cols: usize,
) -> (usize, usize) {
    let mut buf_line = scroll_top;
    let mut visible = 0usize;
    while buf_line < total_lines {
        if view.is_line_hidden(buf_line) {
            buf_line += 1;
            continue;
        }
        // Compute how many visual rows this buffer line occupies when wrapped.
        let line_str = buffer.content.line(buf_line).to_string();
        let line_str = line_str.trim_end_matches('\n');
        let segments = compute_word_wrap_segments(line_str, viewport_cols);
        let visual_rows = segments.len();
        if view_row < visible + visual_rows {
            // The clicked row falls within this buffer line.
            let seg_idx = view_row - visible;
            let seg_col_offset = segments.get(seg_idx).map(|&(start, _)| start).unwrap_or(0);
            return (buf_line, seg_col_offset);
        }
        visible += visual_rows;
        if let Some(fold) = view.fold_at(buf_line) {
            buf_line = fold.end + 1;
        } else {
            buf_line += 1;
        }
    }
    (total_lines.saturating_sub(1), 0)
}

/// Slice `spans` to cover only the byte range `[seg_start_byte, seg_end_byte)`,
/// adjusting `start_byte`/`end_byte` to be relative to `seg_start_byte`.
/// Used when splitting a wrapped line into per-segment `RenderedLine` entries.
fn slice_spans_for_segment(
    spans: &[StyledSpan],
    seg_start_byte: usize,
    seg_end_byte: usize,
) -> Vec<StyledSpan> {
    let mut result = Vec::new();
    for span in spans {
        let overlap_start = span.start_byte.max(seg_start_byte);
        let overlap_end = span.end_byte.min(seg_end_byte);
        if overlap_start < overlap_end {
            result.push(StyledSpan {
                start_byte: overlap_start - seg_start_byte,
                end_byte: overlap_end - seg_start_byte,
                style: span.style,
            });
        }
    }
    result
}

/// Convert a character index within a UTF-8 string to its byte offset.
/// Returns `s.len()` if `char_idx` is beyond the string length.
fn char_to_byte_offset(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

#[allow(clippy::too_many_arguments)]
fn build_rendered_window(
    engine: &Engine,
    theme: &Theme,
    window_id: WindowId,
    rect: &WindowRect,
    visible_lines: usize,
    char_width: f64,
    is_active: bool,
    multi_window: bool,
    color_headings: bool,
) -> RenderedWindow {
    let empty = |id: WindowId| RenderedWindow {
        window_id: id,
        rect: *rect,
        lines: vec![],
        cursor: None,
        extra_cursors: vec![],
        selection: None,
        extra_selections: vec![],
        yank_highlight: None,
        scroll_top: 0,
        scroll_left: 0,
        total_lines: 0,
        gutter_char_width: 0,
        text_viewport_cols: 0,
        is_active,
        show_active_bg: false,
        has_git_diff: false,
        has_breakpoints: false,
        max_col: 0,
        diagnostic_gutter: std::collections::HashMap::new(),
        code_action_lines: std::collections::HashSet::new(),
        bracket_match_positions: Vec::new(),
        active_indent_col: None,
        tabstop: engine.settings.tabstop.max(1) as usize,
        cursorline: engine.settings.cursorline,
        status_line: None,
    };

    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return empty(window_id),
    };
    let buffer_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return empty(window_id),
    };

    let buffer = &buffer_state.buffer;
    let view = &window.view;
    let total_lines = buffer.len_lines();
    // Clamp scroll_top so that line_to_byte never panics when the cursor was
    // set to a line beyond the buffer (e.g. DAP exception in a stdlib file
    // that failed to open, leaving a small buffer with a large scroll offset).
    let scroll_top = view.scroll_top.min(total_lines);
    let cursor_line = view.cursor.line;

    // Whether this buffer has git diff data.
    let has_git = !buffer_state.git_diff.is_empty();

    // Look up LSP diagnostics for this buffer.
    // Diagnostics are keyed by absolute path (from LSP URIs), but buffer file_path
    // may be relative, so use the pre-computed canonical_path cached at file-open
    // time rather than calling canonicalize() (a filesystem syscall) every frame.
    let canonical_path = buffer_state.canonical_path.as_ref();
    let file_diagnostics = canonical_path.and_then(|p| engine.lsp_diagnostics.get(p));

    // Pre-index diagnostics by start line in a single pass.
    // This gives O(1) per-line lookup during visible-line rendering AND builds the gutter
    // severity map simultaneously, replacing two separate O(N_diags) scans with one.
    let mut diag_by_line: std::collections::HashMap<usize, Vec<&crate::core::lsp::Diagnostic>> =
        std::collections::HashMap::new();
    let mut diagnostic_gutter: std::collections::HashMap<
        usize,
        crate::core::lsp::DiagnosticSeverity,
    > = std::collections::HashMap::new();
    if let Some(diags) = file_diagnostics {
        for d in diags {
            let line = d.range.start.line as usize;
            diag_by_line.entry(line).or_default().push(d);
            let entry = diagnostic_gutter.entry(line).or_insert(d.severity);
            if (d.severity as u8) < (*entry as u8) {
                *entry = d.severity;
            }
        }
    }

    // DAP breakpoints for this buffer.
    // Use the raw buffer path as key (matches how dap_toggle_breakpoint stores them).
    let bp_file_key = buffer_state
        .file_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bp_infos: &[crate::core::dap::BreakpointInfo] = engine
        .dap_breakpoints
        .get(&bp_file_key)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let bp_lines: Vec<u64> = bp_infos.iter().map(|bp| bp.line).collect();
    // Show the breakpoint column when any BP is set for this file, or a DAP
    // session is active (so the column width stays stable during a session).
    let has_bp = !bp_lines.is_empty() || engine.dap_session_active;

    // Stopped-line path for per-line comparison (try canonical, then raw).
    let dap_stop_path = engine.dap_current_line.as_ref().map(|(p, _)| p.as_str());

    // Markdown preview buffers never show line numbers.
    let line_number_mode = if buffer_state.md_rendered.is_some() {
        LineNumberMode::None
    } else {
        engine.settings.line_numbers
    };

    // Gutter width in character columns (always includes fold indicator column).
    let gutter_char_width =
        calculate_gutter_cols(line_number_mode, total_lines, char_width, has_git, has_bp);

    // Compute the accurate content width (in character columns) directly from the
    // precise pixel rect and measured char_width.  This avoids the approximate
    // viewport_cols that was stored during the resize callback (which used a
    // hardcoded char_width_approx of 9.0 px and a fixed gutter offset of 5).
    // For the TUI backend, rect.width is already in cell columns and char_width=1.0,
    // so the formula reduces to rect.width - gutter_char_width, which is exact.
    // In the GTK backend (char_width > 1.0) reserve pixels for the vertical
    // scrollbar overlay so text never renders behind it.  CSS requests 4px
    // but GTK may allocate slightly more; 8px is a safe reserve.
    let scrollbar_px: f64 = if char_width > 1.0 { 8.0 } else { 0.0 };
    let render_viewport_cols = if char_width > 0.0 {
        let total_chars = ((rect.width - scrollbar_px) / char_width).floor() as usize;
        total_chars.saturating_sub(gutter_char_width).max(1)
    } else {
        view.viewport_cols.max(1)
    };

    // Narrow the highlights slice to only the visible window using binary search.
    // Tree-sitter emits highlights sorted by start_byte, so partition_point is valid.
    // This reduces build_spans from O(N_total_highlights) per line to O(N_window_highlights).
    let window_start_byte = buffer.content.line_to_byte(scroll_top);
    let approx_end_line = (scroll_top + visible_lines + 1).min(total_lines);
    let window_end_byte = if approx_end_line < total_lines {
        buffer.content.line_to_byte(approx_end_line)
    } else {
        buffer.content.len_bytes()
    };
    let hl_lo = buffer_state
        .highlights
        .partition_point(|h| h.1 <= window_start_byte);
    let hl_hi = buffer_state
        .highlights
        .partition_point(|h| h.0 < window_end_byte);
    let visible_hl = &buffer_state.highlights[hl_lo..hl_hi];

    // Compute search matches for this buffer.  The engine's `search_matches`
    // only indexes the *active* buffer, so for other visible buffers we must
    // compute matches from `search_query` against this buffer's text.
    let active_buf_id = engine
        .windows
        .get(&engine.active_window_id())
        .map(|w| w.buffer_id);
    let buf_search_matches: Vec<(usize, usize)> =
        if !engine.settings.hlsearch || engine.search_query.is_empty() {
            Vec::new()
        } else if Some(window.buffer_id) == active_buf_id {
            engine.search_matches.clone()
        } else {
            compute_search_matches_for_buffer(buffer, &engine.search_query, &engine.settings)
        };

    // Ghost text (AI inline completion): only in the active window, Insert mode.
    // Multi-line completions are stored in full (Tab-accept inserts everything).
    // The first line is shown after the cursor (ghost_suffix on the cursor line).
    // Subsequent lines are inserted as virtual ghost continuation rows so the
    // user can see the full suggestion before accepting with Tab.
    let (ghost_for_cursor_line, ghost_continuation_lines): (Option<String>, Vec<String>) =
        if is_active && engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
            match &engine.ai_ghost_text {
                None => (None, Vec::new()),
                Some(g) => {
                    let mut it = g.lines();
                    let first = it.next().unwrap_or("").to_string();
                    let cont: Vec<String> = it.map(|l| l.to_string()).collect();
                    (Some(first), cont)
                }
            }
        } else {
            (None, Vec::new())
        };

    // Look up aligned diff data for this window (for visual padding).
    let diff_aligned: Option<&[AlignedDiffEntry]> =
        engine.diff_aligned.get(&window_id).map(|v| v.as_slice());

    // Build rendered lines (fold-aware: skip hidden lines, jump over fold bodies)
    let mut lines = Vec::with_capacity(visible_lines);

    // When aligned diff data exists, iterate through the aligned sequence
    // so padding lines appear at the correct visual positions.
    //
    // `view.aligned_top` (set by `sync_scroll_binds`) wins when present:
    // it pins the starting aligned index so both panes of a scroll-bound
    // pair land on exactly the same row. Without it we fall back to a
    // seek-from-`scroll_top` heuristic, then back up over any leading
    // padding so a hunk's filler rows render at the top of the viewport
    // when scroll_top lands just past them (#166).
    let mut aligned_idx: usize = if let Some(aligned) = diff_aligned {
        if let Some(top) = view.aligned_top {
            top.min(aligned.len())
        } else {
            let seek_idx = aligned
                .iter()
                .position(|e| e.source_line.is_some_and(|sl| sl >= scroll_top))
                .unwrap_or(0);
            let mut k = seek_idx;
            while k > 0 && aligned[k - 1].source_line.is_none() {
                k -= 1;
            }
            k
        }
    } else {
        0
    };
    // When `aligned_top` pins the start at a padding entry, advance
    // `line_idx` to the next real source line so the buffer-line-driven
    // outer loop emits padding for the leading None entries before
    // emitting that real line. Falls back to `scroll_top` when there is
    // no aligned data (the non-diff path) or no Some entry remains
    // (trailing-padding edge case).
    let mut line_idx = if let Some(aligned) = diff_aligned {
        aligned[aligned_idx..]
            .iter()
            .find_map(|e| e.source_line)
            .unwrap_or(scroll_top)
    } else {
        scroll_top
    };
    while lines.len() < visible_lines && line_idx < total_lines {
        // Skip hidden lines (fold bodies).
        if view.is_line_hidden(line_idx) {
            // Also advance aligned_idx past this hidden line's entry
            // (and any adjacent padding) so padding for folded regions
            // doesn't get emitted as blank lines.
            if let Some(aligned) = diff_aligned {
                while aligned_idx < aligned.len() {
                    match aligned[aligned_idx].source_line {
                        Some(sl) if sl == line_idx => {
                            aligned_idx += 1;
                            break;
                        }
                        Some(sl) if sl > line_idx => break,
                        _ => aligned_idx += 1, // skip padding or earlier source lines
                    }
                }
            }
            line_idx += 1;
            continue;
        }

        // Emit padding lines from the aligned diff sequence before this buffer line.
        if let Some(aligned) = diff_aligned {
            while aligned_idx < aligned.len() && lines.len() < visible_lines {
                let entry = &aligned[aligned_idx];
                if let Some(sl) = entry.source_line {
                    if sl >= line_idx {
                        break; // reached the current buffer line
                    }
                    // This source line is before scroll_top — skip it.
                    aligned_idx += 1;
                    continue;
                }
                // When unchanged lines are hidden (fold-filtered diff view),
                // suppress padding lines — alignment is meaningless when
                // the unchanged context between hunks is collapsed.
                if engine.diff_unchanged_hidden {
                    aligned_idx += 1;
                    continue;
                }
                // Padding entry — emit an empty rendered line.
                let padding_gutter = format!(
                    "{:>width$} ",
                    "",
                    width = gutter_char_width.saturating_sub(1)
                );
                lines.push(RenderedLine {
                    gutter_text: padding_gutter,
                    raw_text: String::new(),
                    spans: vec![],
                    line_idx,
                    git_diff: None,
                    diagnostics: vec![],
                    spell_errors: vec![],
                    diff_status: Some(DiffLine::Padding),
                    is_breakpoint: false,
                    is_conditional_bp: false,
                    is_dap_current: false,
                    is_wrap_continuation: false,
                    segment_col_offset: 0,
                    annotation: None,
                    ghost_suffix: None,
                    is_current_line: false,
                    is_fold_header: false,
                    folded_line_count: 0,
                    is_ghost_continuation: false,
                    indent_guides: vec![],
                    colorcolumns: vec![],
                });
                aligned_idx += 1;
            }
            if lines.len() >= visible_lines {
                break;
            }
            // Advance aligned_idx past this buffer line's entry.
            if aligned_idx < aligned.len() {
                if let Some(sl) = aligned[aligned_idx].source_line {
                    if sl == line_idx {
                        aligned_idx += 1;
                    }
                }
            }
        }

        let is_fold_header = view.fold_at(line_idx).is_some();
        let folded_line_count = view.fold_at(line_idx).map(|f| f.end - f.start).unwrap_or(0);

        let line = buffer.content.line(line_idx);
        let line_str = line.to_string().replace('\0', "");
        let line_start_byte = buffer.content.line_to_byte(line_idx);
        let line_end_byte = line_start_byte + line.len_bytes();

        let spans = if let Some(ref md) = buffer_state.md_rendered {
            if line_idx < md.spans.len() {
                let code_hl = md.code_highlights.get(line_idx);
                md_spans_to_styled(&md.spans[line_idx], code_hl, theme, color_headings)
            } else {
                vec![]
            }
        } else {
            let is_markdown = buffer_state
                .file_path
                .as_ref()
                .and_then(|p| p.to_str())
                .and_then(crate::core::syntax::SyntaxLanguage::from_path)
                == Some(crate::core::syntax::SyntaxLanguage::Markdown);
            build_spans(
                engine,
                theme,
                visible_hl,
                &buffer_state.semantic_tokens,
                buffer,
                line_idx,
                &line_str,
                line_start_byte,
                line_end_byte,
                is_markdown,
                &buf_search_matches,
                Some(window.buffer_id) == active_buf_id,
            )
        };

        // Git diff status for this line.
        let git_status = if has_git {
            buffer_state.git_diff.get(line_idx).copied().flatten()
        } else {
            None
        };

        // DAP: is there a breakpoint on this line? Is the adapter stopped here?
        let line_1based = line_idx as u64 + 1;
        let is_breakpoint = has_bp && bp_lines.binary_search(&line_1based).is_ok();
        let is_conditional_bp = is_breakpoint
            && bp_infos.iter().any(|bp| {
                bp.line == line_1based && (bp.condition.is_some() || bp.hit_condition.is_some())
            });
        let is_dap_current = engine
            .dap_current_line
            .as_ref()
            .map(|(path, l)| {
                *l == line_1based
                    && (dap_stop_path == Some(path.as_str())
                        || canonical_path
                            .map(|cp| cp.to_string_lossy().as_ref() == path.as_str())
                            .unwrap_or(false))
            })
            .unwrap_or(false);

        let fold_char = fold_indicator_char(buffer, view, line_idx);
        // Number of leading marker columns (bp + git) subtracted from the
        // numeric portion so line numbers fill their allotted width correctly.
        let marker_cols = if has_bp { 1 } else { 0 } + if has_git { 1 } else { 0 };
        let base_gutter = format_gutter_with_fold(
            line_number_mode,
            line_idx,
            cursor_line,
            gutter_char_width.saturating_sub(marker_cols),
            fold_char,
        );
        // Build gutter_text: [bp_char][git_char][fold+nums]
        let gutter_text = {
            let bp_part = if has_bp {
                if is_dap_current && is_breakpoint {
                    "◉" // breakpoint + current line
                } else if is_dap_current {
                    "▶" // current execution line (no breakpoint)
                } else if is_conditional_bp {
                    "◆" // conditional breakpoint
                } else if is_breakpoint {
                    "●" // breakpoint
                } else {
                    " "
                }
            } else {
                ""
            };
            let git_part = if has_git {
                match git_status {
                    Some(GitLineStatus::Added) | Some(GitLineStatus::Modified) => "▌",
                    Some(GitLineStatus::Deleted) => "▾",
                    None => " ",
                }
            } else {
                ""
            };
            format!("{}{}{}", bp_part, git_part, base_gutter)
        };

        // LSP diagnostics for this line — O(1) lookup via pre-indexed map.
        let line_diagnostics: Vec<DiagnosticMark> = if let Some(diags) = diag_by_line.get(&line_idx)
        {
            diags
                .iter()
                .map(|d| {
                    // Reuse line_str already computed above — avoids redundant rope lookup.
                    let start_col =
                        crate::core::lsp::utf16_offset_to_char(&line_str, d.range.start.character);
                    let end_col = if d.range.end.line as usize == line_idx {
                        crate::core::lsp::utf16_offset_to_char(&line_str, d.range.end.character)
                    } else {
                        line_str.len()
                    };
                    DiagnosticMark {
                        start_col,
                        end_col: end_col.max(start_col + 1),
                        severity: d.severity,
                        message: d.message.clone(),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        // Spell-check errors for this line — computed on visible lines only.
        let line_spell_errors: Vec<SpellMark> = if engine.settings.spell {
            if let Some(ref checker) = engine.spell_checker {
                let syntax_lang = buffer_state
                    .file_path
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .and_then(crate::core::syntax::SyntaxLanguage::from_path);
                let line_start_byte = buffer.content.line_to_byte(line_idx);
                crate::core::spell::check_line(
                    checker,
                    &line_str,
                    &buffer_state.highlights,
                    line_start_byte,
                    syntax_lang,
                )
                .into_iter()
                .map(|e| SpellMark {
                    start_col: e.start_col,
                    end_col: e.end_col,
                })
                .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Two-way diff status for this line.
        let diff_status = engine
            .diff_results
            .get(&window_id)
            .and_then(|v| v.get(line_idx))
            .copied();

        let is_md_preview = engine.md_preview_links.contains_key(&window.buffer_id);
        let wrap_on =
            (engine.settings.wrap || is_md_preview) && render_viewport_cols > 0 && !is_fold_header;
        let line_char_len = line_str.chars().count();

        if wrap_on && line_char_len > render_viewport_cols {
            // Split long line into viewport-width segments with word-boundary wrapping.
            let vp = render_viewport_cols;
            // Build segment boundaries using word-aware splitting.
            let segment_boundaries = compute_word_wrap_segments(&line_str, vp);
            let num_segments = segment_boundaries.len();
            let cursor_seg = if line_idx == cursor_line {
                // Find which segment contains the cursor column.
                segment_boundaries
                    .iter()
                    .position(|&(start, end)| view.cursor.col >= start && view.cursor.col < end)
                    .unwrap_or(num_segments.saturating_sub(1))
            } else {
                usize::MAX // won't match any segment
            };
            // Blank gutter for continuation rows (same width as normal gutter).
            let blank_gutter = " ".repeat(gutter_char_width);
            for (seg, &(seg_start_char, seg_end_char)) in segment_boundaries.iter().enumerate() {
                if lines.len() >= visible_lines {
                    break;
                }
                let seg_start_byte = char_to_byte_offset(&line_str, seg_start_char);
                let seg_end_byte = char_to_byte_offset(&line_str, seg_end_char);
                let seg_text = line_str[seg_start_byte..seg_end_byte].to_string();
                let seg_spans = slice_spans_for_segment(&spans, seg_start_byte, seg_end_byte);
                let is_cont = seg > 0;
                lines.push(RenderedLine {
                    raw_text: seg_text,
                    gutter_text: if is_cont {
                        blank_gutter.clone()
                    } else {
                        gutter_text.clone()
                    },
                    is_current_line: line_idx == cursor_line && seg == cursor_seg,
                    spans: seg_spans,
                    is_fold_header: false,
                    folded_line_count: 0,
                    line_idx,
                    git_diff: if is_cont { None } else { git_status },
                    diagnostics: if is_cont {
                        Vec::new()
                    } else {
                        line_diagnostics.clone()
                    },
                    spell_errors: if is_cont {
                        Vec::new()
                    } else {
                        line_spell_errors.clone()
                    },
                    diff_status,
                    is_breakpoint: !is_cont && is_breakpoint,
                    is_conditional_bp: !is_cont && is_conditional_bp,
                    is_dap_current,
                    is_wrap_continuation: is_cont,
                    segment_col_offset: seg_start_char,
                    annotation: if is_cont
                        || (engine.mode == crate::core::Mode::Insert && !engine.is_vscode_mode())
                    {
                        None
                    } else {
                        engine.line_annotations.get(&line_idx).cloned()
                    },
                    ghost_suffix: if line_idx == cursor_line && seg == cursor_seg {
                        ghost_for_cursor_line.clone()
                    } else {
                        None
                    },
                    is_ghost_continuation: false,
                    indent_guides: Vec::new(), // filled below
                    colorcolumns: Vec::new(),  // filled below
                });

                // After the cursor segment, insert ghost continuation rows.
                if line_idx == cursor_line && seg == cursor_seg {
                    for cont in &ghost_continuation_lines {
                        if lines.len() >= visible_lines {
                            break;
                        }
                        lines.push(RenderedLine {
                            raw_text: String::new(),
                            gutter_text: blank_gutter.clone(),
                            is_current_line: false,
                            spans: Vec::new(),
                            is_fold_header: false,
                            folded_line_count: 0,
                            line_idx,
                            git_diff: None,
                            diagnostics: Vec::new(),
                            spell_errors: Vec::new(),
                            diff_status: None,
                            is_breakpoint: false,
                            is_conditional_bp: false,
                            is_dap_current: false,
                            is_wrap_continuation: true,
                            segment_col_offset: 0,
                            annotation: None,
                            ghost_suffix: Some(cont.clone()),
                            is_ghost_continuation: true,
                            indent_guides: Vec::new(),
                            colorcolumns: Vec::new(),
                        });
                    }
                }
            }
        } else {
            lines.push(RenderedLine {
                raw_text: line_str,
                gutter_text,
                is_current_line: line_idx == cursor_line,
                spans,
                is_fold_header,
                folded_line_count,
                line_idx,
                git_diff: git_status,
                diagnostics: line_diagnostics,
                spell_errors: line_spell_errors,
                diff_status,
                is_breakpoint,
                is_conditional_bp,
                is_dap_current,
                is_wrap_continuation: false,
                segment_col_offset: 0,
                annotation: if engine.mode == crate::core::Mode::Insert && !engine.is_vscode_mode()
                {
                    None
                } else {
                    engine.line_annotations.get(&line_idx).cloned()
                },
                ghost_suffix: if line_idx == cursor_line {
                    ghost_for_cursor_line.clone()
                } else {
                    None
                },
                is_ghost_continuation: false,
                indent_guides: Vec::new(), // filled below
                colorcolumns: Vec::new(),  // filled below
            });

            // After the cursor line, insert ghost continuation rows.
            if line_idx == cursor_line {
                let blank_gutter = " ".repeat(gutter_char_width);
                for cont in &ghost_continuation_lines {
                    if lines.len() >= visible_lines {
                        break;
                    }
                    lines.push(RenderedLine {
                        raw_text: String::new(),
                        gutter_text: blank_gutter.clone(),
                        is_current_line: false,
                        spans: Vec::new(),
                        is_fold_header: false,
                        folded_line_count: 0,
                        line_idx,
                        git_diff: None,
                        diagnostics: Vec::new(),
                        spell_errors: Vec::new(),
                        diff_status: None,
                        is_breakpoint: false,
                        is_conditional_bp: false,
                        is_dap_current: false,
                        is_wrap_continuation: true,
                        segment_col_offset: 0,
                        annotation: None,
                        ghost_suffix: Some(cont.clone()),
                        is_ghost_continuation: true,
                        indent_guides: Vec::new(),
                        colorcolumns: Vec::new(),
                    });
                }
            }
        }

        // Jump past the fold body for fold headers.
        if let Some(fold) = view.fold_at(line_idx) {
            line_idx = fold.end + 1;
        } else {
            line_idx += 1;
        }
    }

    // Cursor (only if visible) — find its index in the rendered lines array.
    let cursor = if is_active {
        lines
            .iter()
            .enumerate()
            .find(|(_, l)| l.is_current_line)
            .map(|(view_line, l)| {
                let shape = if engine.pending_key == Some('r') {
                    CursorShape::Underline
                } else if engine.is_vscode_mode() {
                    CursorShape::Bar
                } else {
                    match engine.mode {
                        Mode::Insert => CursorShape::Bar,
                        _ => CursorShape::Block,
                    }
                };
                // When wrapping, the cursor col is relative to the segment start.
                let col = view.cursor.col.saturating_sub(l.segment_col_offset);
                (CursorPos { view_line, col }, shape)
            })
    } else {
        None
    };

    // Secondary cursors — map each extra cursor to its view_line + col.
    let extra_cursors: Vec<CursorPos> = view
        .extra_cursors
        .iter()
        .filter_map(|ec| {
            lines
                .iter()
                .enumerate()
                .find(|(_, l)| l.line_idx == ec.line && !l.is_wrap_continuation)
                .map(|(view_line, l)| {
                    let col = ec.col.saturating_sub(l.segment_col_offset);
                    CursorPos { view_line, col }
                })
        })
        .collect();

    // Visual selection (only for active window)
    let selection = if is_active {
        build_selection(engine, scroll_top, visible_lines)
    } else {
        None
    };

    // Yank highlight (only for active window)
    let yank_highlight = if is_active {
        engine.yank_highlight.map(|(start, end, is_linewise)| {
            let (s, e) = if (start.line, start.col) <= (end.line, end.col) {
                (start, end)
            } else {
                (end, start)
            };
            SelectionRange {
                kind: if is_linewise {
                    SelectionKind::Line
                } else {
                    SelectionKind::Char
                },
                start_line: s.line,
                start_col: s.col,
                end_line: e.line,
                end_col: e.col,
            }
        })
    } else {
        None
    };

    // Maximum line length across the whole buffer. When wrap is on, there is no
    // horizontal scrolling, so we report 0 to suppress the horizontal scrollbar.
    let is_md_preview = engine.md_preview_links.contains_key(&window.buffer_id);
    let max_col = if engine.settings.wrap || is_md_preview {
        0
    } else {
        buffer_state.max_col
    };

    // diagnostic_gutter is already built in the single-pass pre-indexing above.

    // ── Indent guides ──────────────────────────────────────────────────────
    let tabstop = engine.settings.tabstop.max(1) as usize;
    let mut active_indent_col: Option<usize> = None;
    if engine.settings.indent_guides {
        // Compute the indent level for each visible line (in columns).
        let line_indents: Vec<Option<usize>> = lines
            .iter()
            .map(|l| {
                if l.is_ghost_continuation || l.is_wrap_continuation {
                    return None; // not a real line for indent purposes
                }
                let text = &l.raw_text;
                let mut cols = 0usize;
                for ch in text.chars() {
                    match ch {
                        ' ' => cols += 1,
                        '\t' => cols += tabstop - (cols % tabstop),
                        _ => break,
                    }
                }
                // Blank lines (only whitespace/newline) return None so guides bridge
                let trimmed = text.trim_start();
                let non_ws = !trimmed.is_empty() && trimmed != "\n" && trimmed != "\r\n";
                if non_ws {
                    Some(cols)
                } else {
                    None // blank line — will be bridged
                }
            })
            .collect();

        // Determine active guide column from cursor line indent
        if let Some(cursor_pos) = &cursor {
            let cursor_view_line = cursor_pos.0.view_line;
            if cursor_view_line < line_indents.len() {
                if let Some(indent) = line_indents[cursor_view_line] {
                    // Active guide is the highest tabstop ≤ cursor indent
                    if indent >= tabstop {
                        let guide_col = (indent / tabstop) * tabstop;
                        // Use the guide one level below if cursor indent is exact multiple
                        active_indent_col = Some(guide_col - tabstop);
                    }
                }
            }
        }

        // Assign indent guides per line, bridging blank lines
        for (i, line) in lines.iter_mut().enumerate() {
            if line.is_ghost_continuation {
                continue;
            }
            let indent = match line_indents[i] {
                Some(ind) => ind,
                None => {
                    // Blank line: bridge using min indent of surrounding non-blank lines
                    let above = line_indents[..i].iter().rev().find_map(|x| *x).unwrap_or(0);
                    let below = line_indents[i + 1..].iter().find_map(|x| *x).unwrap_or(0);
                    above.min(below)
                }
            };
            let mut guides = Vec::new();
            let mut col = tabstop;
            while col <= indent {
                guides.push(col - tabstop); // guide at the start of each tabstop level
                col += tabstop;
            }
            line.indent_guides = guides;
        }
    }

    // ── Color columns ──────────────────────────────────────────────────────
    let cc_positions = engine.settings.colorcolumn_positions();
    if !cc_positions.is_empty() {
        for line in lines.iter_mut() {
            line.colorcolumns = cc_positions.clone();
        }
    }

    // ── Bracket match positions ────────────────────────────────────────────
    let bracket_match_positions = if engine.settings.match_brackets && is_active {
        if let Some((match_line, match_col)) = engine.bracket_match {
            let mut positions = Vec::with_capacity(2);
            // Cursor bracket position
            let cursor_line_idx = view.cursor.line;
            let cursor_col_idx = view.cursor.col;
            for (vi, l) in lines.iter().enumerate() {
                if l.line_idx == cursor_line_idx
                    && !l.is_ghost_continuation
                    && !l.is_wrap_continuation
                {
                    positions.push((vi, cursor_col_idx.saturating_sub(l.segment_col_offset)));
                }
                if l.line_idx == match_line && !l.is_ghost_continuation && !l.is_wrap_continuation {
                    positions.push((vi, match_col.saturating_sub(l.segment_col_offset)));
                }
            }
            positions.dedup();
            positions
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Extra selections for Ctrl+D multi-cursor word selections.
    // Each extra cursor sits at the END of a word; derive selection start
    // from the primary selection length.
    let extra_selections = if is_active && !view.extra_cursors.is_empty() {
        if let Some(sel) = selection
            .as_ref()
            .filter(|s| s.kind == SelectionKind::Char && s.start_line == s.end_line)
        {
            let sel_len = sel.end_col + 1 - sel.start_col; // inclusive
            view.extra_cursors
                .iter()
                .map(|ec| SelectionRange {
                    kind: SelectionKind::Char,
                    start_line: ec.line,
                    start_col: ec.col + 1 - sel_len,
                    end_line: ec.line,
                    end_col: ec.col,
                })
                .collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    RenderedWindow {
        window_id,
        rect: *rect,
        lines,
        cursor,
        extra_cursors,
        selection,
        extra_selections,
        yank_highlight,
        scroll_top,
        scroll_left: view.scroll_left,
        total_lines,
        gutter_char_width,
        text_viewport_cols: render_viewport_cols,
        is_active,
        show_active_bg: is_active && multi_window,
        has_git_diff: has_git,
        has_breakpoints: has_bp,
        max_col,
        diagnostic_gutter,
        bracket_match_positions,
        active_indent_col,
        tabstop: engine.settings.tabstop.max(1) as usize,
        code_action_lines: {
            // Only show lightbulb on the cursor line (like VSCode) — not on every
            // line that has cached actions, which would be noisy in Rust files where
            // rust-analyzer offers refactors on nearly every line.
            let cl = view.cursor.line;
            let has = canonical_path
                .and_then(|p| engine.lsp_code_actions.get(p))
                .and_then(|m| m.get(&cl))
                .is_some_and(|v| !v.is_empty());
            if has {
                std::collections::HashSet::from([cl])
            } else {
                std::collections::HashSet::new()
            }
        },
        cursorline: engine.settings.cursorline,
        status_line: None,
    }
}

/// Convert markdown style spans into rendering `StyledSpan`s.
/// When `code_highlights` is non-empty, tree-sitter colors override CodeBlock spans.
fn md_spans_to_styled(
    md_spans: &[crate::core::markdown::MdSpan],
    code_highlights: Option<&Vec<crate::core::markdown::MdCodeHighlight>>,
    theme: &Theme,
    color_headings: bool,
) -> Vec<StyledSpan> {
    use crate::core::markdown::MdStyle;
    // If this line has tree-sitter code highlights, use those instead.
    if let Some(highlights) = code_highlights {
        if !highlights.is_empty() {
            return highlights
                .iter()
                .map(|h| StyledSpan {
                    start_byte: h.start_byte,
                    end_byte: h.end_byte,
                    style: Style {
                        fg: theme.scope_color(&h.scope),
                        bg: None,
                        bold: false,
                        italic: false,
                        font_scale: 1.0,
                    },
                })
                .collect();
        }
    }
    md_spans
        .iter()
        .map(|s| {
            let (fg, bold, italic, font_scale) = match s.style {
                MdStyle::Heading(1) => {
                    let c = if color_headings {
                        theme.md_heading1
                    } else {
                        theme.foreground
                    };
                    (c, true, false, 1.4)
                }
                MdStyle::Heading(2) => {
                    let c = if color_headings {
                        theme.md_heading2
                    } else {
                        theme.foreground
                    };
                    (c, true, false, 1.2)
                }
                MdStyle::Heading(_) => {
                    let c = if color_headings {
                        theme.md_heading3
                    } else {
                        theme.foreground
                    };
                    (c, true, false, 1.1)
                }
                MdStyle::Bold => (theme.foreground, true, false, 1.0),
                MdStyle::Italic => (theme.foreground, false, true, 1.0),
                MdStyle::BoldItalic => (theme.foreground, true, true, 1.0),
                MdStyle::Code | MdStyle::CodeBlock => (theme.md_code, false, false, 1.0),
                MdStyle::Link => (theme.md_link, false, false, 1.0),
                MdStyle::LinkUrl => (theme.md_link, false, true, 1.0),
                MdStyle::BlockQuote => (theme.md_heading3, false, true, 1.0),
                MdStyle::ListBullet => (theme.md_heading1, true, false, 1.0),
                MdStyle::HorizontalRule => (theme.annotation_fg, false, false, 1.0),
                MdStyle::Image => (theme.md_link, false, true, 1.0),
            };
            StyledSpan {
                start_byte: s.start_byte,
                end_byte: s.end_byte,
                style: Style {
                    fg,
                    bg: None,
                    bold,
                    italic,
                    font_scale,
                },
            }
        })
        .collect()
}

/// Build styled spans for one line: syntax highlights + search matches.
#[allow(clippy::too_many_arguments)]
/// Regex-based inline markdown highlighting for bold, italic, inline code, and links.
/// This compensates for not having tree-sitter inline injection support.
fn md_inline_spans(line: &str, theme: &Theme, spans: &mut Vec<StyledSpan>) {
    let bytes = line.as_bytes();

    // Inline code: `code` — requires non-empty content between backticks.
    // Skip runs of 3+ backticks (fenced code block delimiters).
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // Count consecutive backticks
            let tick_run_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let tick_count = i - tick_run_start;
            if tick_count >= 3 {
                // Fenced code delimiter — skip, tree-sitter handles this
                continue;
            }
            // Single or double backtick — find matching closing run
            let content_start = i;
            loop {
                // Find next backtick
                while i < bytes.len() && bytes[i] != b'`' {
                    i += 1;
                }
                if i >= bytes.len() {
                    break;
                }
                // Count closing backticks
                let close_start = i;
                while i < bytes.len() && bytes[i] == b'`' {
                    i += 1;
                }
                if i - close_start == tick_count && i - close_start - tick_count < i {
                    // Matching close — only highlight if there's content
                    if content_start < close_start {
                        spans.push(StyledSpan {
                            start_byte: tick_run_start,
                            end_byte: i,
                            style: Style {
                                fg: theme.scope_color("string"),
                                bg: None,
                                bold: false,
                                italic: false,
                                font_scale: 1.0,
                            },
                        });
                    }
                    break;
                }
                // Not matching — keep searching
            }
            continue;
        }
        i += 1;
    }

    // Bold: **text** or __text__
    for delim in &["**", "__"] {
        let d = delim.as_bytes();
        let mut pos = 0;
        while pos + d.len() < bytes.len() {
            if bytes[pos..].starts_with(d) {
                // For __, require word boundary (not inside a word)
                if d[0] == b'_' && pos > 0 && bytes[pos - 1] != b' ' && bytes[pos - 1] != b'\t' {
                    pos += 1;
                    continue;
                }
                let open = pos;
                pos += d.len();
                // Find closing delimiter
                while pos + d.len() <= bytes.len() && !bytes[pos..].starts_with(d) {
                    pos += 1;
                }
                if pos + d.len() <= bytes.len() && bytes[pos..].starts_with(d) {
                    let close = pos + d.len();
                    spans.push(StyledSpan {
                        start_byte: open,
                        end_byte: close,
                        style: Style {
                            fg: theme.scope_color("variable"),
                            bg: None,
                            bold: true,
                            italic: false,
                            font_scale: 1.0,
                        },
                    });
                    pos = close;
                    continue;
                }
            }
            pos += 1;
        }
    }

    // Italic: *text* or _text_
    // For underscore: require word boundary (space or start-of-line before open,
    // space or end-of-line after close) to avoid matching inside_words_like_this.
    for &delim_byte in b"*_" {
        let need_boundary = delim_byte == b'_';
        let mut pos = 0;
        while pos < bytes.len() {
            if bytes[pos] == delim_byte {
                // Skip if this is a bold delimiter (double)
                if pos + 1 < bytes.len() && bytes[pos + 1] == delim_byte {
                    pos += 2;
                    // Skip past bold content + closing **/__
                    while pos < bytes.len() {
                        if bytes[pos] == delim_byte
                            && pos + 1 < bytes.len()
                            && bytes[pos + 1] == delim_byte
                        {
                            pos += 2;
                            break;
                        }
                        pos += 1;
                    }
                    continue;
                }
                // Word boundary check for underscore
                if need_boundary && pos > 0 && bytes[pos - 1] != b' ' && bytes[pos - 1] != b'\t' {
                    pos += 1;
                    continue;
                }
                let open = pos;
                pos += 1;
                while pos < bytes.len() && bytes[pos] != delim_byte {
                    pos += 1;
                }
                if pos < bytes.len() {
                    let close = pos + 1;
                    // Check closing word boundary for underscore
                    let close_ok = !need_boundary
                        || close >= bytes.len()
                        || bytes[close] == b' '
                        || bytes[close] == b'\t'
                        || bytes[close] == b'.'
                        || bytes[close] == b','
                        || bytes[close] == b':'
                        || bytes[close] == b';'
                        || bytes[close] == b')'
                        || bytes[close] == b']';
                    // Only if there's content between delimiters
                    if close - open > 2 && close_ok {
                        spans.push(StyledSpan {
                            start_byte: open,
                            end_byte: close,
                            style: Style {
                                fg: theme.scope_color("variable"),
                                bg: None,
                                bold: false,
                                italic: true,
                                font_scale: 1.0,
                            },
                        });
                    }
                    pos = close;
                    continue;
                }
            }
            pos += 1;
        }
    }

    // Links: [text](url) — color the URL part
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] == b'[' {
            let bracket_start = pos;
            pos += 1;
            // Find ]
            while pos < bytes.len() && bytes[pos] != b']' {
                pos += 1;
            }
            if pos + 1 < bytes.len() && bytes[pos] == b']' && bytes[pos + 1] == b'(' {
                let bracket_end = pos;
                // Color [text] as link
                spans.push(StyledSpan {
                    start_byte: bracket_start,
                    end_byte: bracket_end + 1,
                    style: Style {
                        fg: theme.scope_color("type"),
                        bg: None,
                        bold: false,
                        italic: false,
                        font_scale: 1.0,
                    },
                });
                pos += 2; // skip ](
                let url_start = pos;
                while pos < bytes.len() && bytes[pos] != b')' {
                    pos += 1;
                }
                if pos < bytes.len() {
                    spans.push(StyledSpan {
                        start_byte: url_start - 1, // include (
                        end_byte: pos + 1,         // include )
                        style: Style {
                            fg: theme.scope_color("comment"),
                            bg: None,
                            bold: false,
                            italic: false,
                            font_scale: 1.0,
                        },
                    });
                    pos += 1;
                    continue;
                }
            }
        }
        pos += 1;
    }
}

/// Compute search match char-offset pairs for a buffer that is NOT the active one.
fn compute_search_matches_for_buffer(
    buffer: &crate::core::buffer::Buffer,
    query: &str,
    settings: &crate::core::settings::Settings,
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let text = buffer.to_string();

    let case_insensitive =
        settings.ignorecase && !(settings.smartcase && query.chars().any(|c| c.is_uppercase()));

    if case_insensitive {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        let mut byte_pos = 0;
        while let Some(found) = text_lower[byte_pos..].find(&query_lower) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query_lower.len();
            let start_char = buffer.content.byte_to_char(start_byte);
            let end_char = buffer.content.byte_to_char(end_byte);
            matches.push((start_char, end_char));
            byte_pos = start_byte + 1;
        }
    } else {
        let mut byte_pos = 0;
        while let Some(found) = text[byte_pos..].find(query) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query.len();
            let start_char = buffer.content.byte_to_char(start_byte);
            let end_char = buffer.content.byte_to_char(end_byte);
            matches.push((start_char, end_char));
            byte_pos = start_byte + 1;
        }
    }
    matches
}

#[allow(clippy::too_many_arguments)]
fn build_spans(
    engine: &Engine,
    theme: &Theme,
    highlights: &[(usize, usize, String)],
    semantic_tokens: &[crate::core::lsp::SemanticToken],
    buffer: &crate::core::buffer::Buffer,
    line_idx: usize,
    line_str: &str,
    line_start_byte: usize,
    line_end_byte: usize,
    is_markdown: bool,
    search_matches: &[(usize, usize)],
    is_active_buffer: bool,
) -> Vec<StyledSpan> {
    let mut spans = Vec::new();

    // Syntax highlighting — iterate only the pre-narrowed window slice.
    for (start, end, scope) in highlights {
        if *end <= line_start_byte || *start >= line_end_byte {
            continue;
        }
        let rel_start = (*start).saturating_sub(line_start_byte);
        let rel_end = if *end > line_end_byte {
            line_str.len()
        } else {
            *end - line_start_byte
        };
        let color = theme.scope_color(scope);
        spans.push(StyledSpan {
            start_byte: rel_start,
            end_byte: rel_end,
            style: Style {
                fg: color,
                bg: None,
                bold: false,
                italic: false,
                font_scale: 1.0,
            },
        });
    }

    // Markdown inline highlighting — regex-based since tree-sitter-md's inline parser
    // requires injection support we don't have. Runs after tree-sitter block highlights
    // so inline elements layer on top.
    if is_markdown {
        md_inline_spans(line_str, theme, &mut spans);
    }

    // LSP semantic tokens overlay — these override tree-sitter spans since they're later.
    // Tokens are sorted by line (from delta-encoding), so binary search finds the first
    // token on this line efficiently.
    if !semantic_tokens.is_empty() {
        let line32 = line_idx as u32;
        let start_idx = semantic_tokens.partition_point(|t| t.line < line32);
        for tok in &semantic_tokens[start_idx..] {
            if tok.line != line32 {
                break;
            }
            if let Some(style) = theme.semantic_token_style(&tok.token_type, &tok.modifiers) {
                // Convert UTF-16 positions to byte offsets within line_str.
                let char_start = crate::core::lsp::utf16_offset_to_char(line_str, tok.start_char);
                let char_end =
                    crate::core::lsp::utf16_offset_to_char(line_str, tok.start_char + tok.length);
                // Convert char positions to byte offsets.
                let byte_start = line_str
                    .char_indices()
                    .nth(char_start)
                    .map(|(i, _)| i)
                    .unwrap_or(line_str.len());
                let byte_end = line_str
                    .char_indices()
                    .nth(char_end)
                    .map(|(i, _)| i)
                    .unwrap_or(line_str.len());
                if byte_start < byte_end {
                    spans.push(StyledSpan {
                        start_byte: byte_start,
                        end_byte: byte_end,
                        style,
                    });
                }
            }
        }
    }

    // Search match highlighting (skipped when hlsearch is disabled)
    if engine.settings.hlsearch && !search_matches.is_empty() {
        let line_start_char = buffer.content.line_to_char(line_idx);
        let line_char_count = line_str.chars().count();
        let line_end_char = line_start_char + line_char_count;

        for (match_idx, (match_start, match_end)) in search_matches.iter().enumerate() {
            if *match_end <= line_start_char || *match_start >= line_end_char {
                continue;
            }
            let match_start_char = (*match_start).max(line_start_char);
            let match_end_char = (*match_end).min(line_end_char);

            let rel_start_byte = line_str
                .char_indices()
                .nth(match_start_char - line_start_char)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let rel_end_byte = line_str
                .char_indices()
                .nth(match_end_char - line_start_char)
                .map(|(i, _)| i)
                .unwrap_or(line_str.len());

            let is_current = is_active_buffer && engine.search_index == Some(match_idx);
            let bg = if is_current {
                theme.search_current_match_bg
            } else {
                theme.search_match_bg
            };
            spans.push(StyledSpan {
                start_byte: rel_start_byte,
                end_byte: rel_end_byte,
                style: Style {
                    fg: theme.search_match_fg,
                    bg: Some(bg),
                    bold: false,
                    italic: false,
                    font_scale: 1.0,
                },
            });
        }
    }

    spans
}

/// Build a normalised [`SelectionRange`] from the engine's visual-mode state.
fn build_selection(
    engine: &Engine,
    scroll_top: usize,
    visible_lines: usize,
) -> Option<SelectionRange> {
    let anchor = engine.visual_anchor?;
    // When find/replace is open from visual mode, use the frozen cursor position
    // so the selection doesn't change as search jumps the live cursor to matches.
    let frozen_end;
    let cursor = if engine.find_replace_open {
        if let Some(end) = engine.find_replace_visual_end {
            frozen_end = end;
            &frozen_end
        } else {
            engine.cursor()
        }
    } else {
        engine.cursor()
    };

    let visual_mode = match engine.mode {
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => Some(engine.mode),
        // Show selection while typing a command/search entered from visual mode,
        // or while find/replace overlay is open from visual mode
        Mode::Command | Mode::Search => engine.command_from_visual,
        Mode::Normal if engine.find_replace_open => engine.command_from_visual,
        _ => None,
    };
    let kind = match visual_mode? {
        Mode::Visual => SelectionKind::Char,
        Mode::VisualLine => SelectionKind::Line,
        Mode::VisualBlock => SelectionKind::Block,
        _ => return None,
    };

    // For visual block the start/end cols need min/max normalisation
    let (start, end) = normalise_selection(anchor, *cursor);

    let (start_col, end_col) = match kind {
        SelectionKind::Block => (anchor.col.min(cursor.col), anchor.col.max(cursor.col)),
        _ => (start.col, end.col),
    };

    // Only emit a selection if it overlaps the visible area
    if end.line < scroll_top || start.line >= scroll_top + visible_lines {
        return None;
    }

    Some(SelectionRange {
        kind,
        start_line: start.line,
        start_col,
        end_line: end.line,
        end_col,
    })
}

/// Return (earlier, later) cursors so that `earlier.line <= later.line`.
fn normalise_selection(a: Cursor, b: Cursor) -> (Cursor, Cursor) {
    if a.line < b.line || (a.line == b.line && a.col <= b.col) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Count leading whitespace of a buffer line (tabs = 4 spaces).
fn line_indent_of(buffer: &Buffer, line_idx: usize) -> usize {
    let line = buffer.content.line(line_idx);
    let mut indent = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => indent += 1,
            '\t' => indent += 4,
            _ => break,
        }
    }
    indent
}

/// Determine the fold indicator character for a rendered line.
/// `+` = closed fold header, `-` = open foldable region, ` ` = neither.
///
/// To avoid false positives (e.g. blank lines, function-call continuations),
/// `-` is only shown when the current line is a **block opener**: non-blank
/// and whose trimmed text ends with `{` or `:`.
fn fold_indicator_char(buffer: &Buffer, view: &View, line_idx: usize) -> char {
    // Closed fold header takes priority.
    if view.fold_at(line_idx).is_some() {
        return '+';
    }
    // Only show `-` for genuine block-opener lines.
    let cur_line = buffer.content.line(line_idx);
    let cur_text: String = cur_line.chars().collect();
    let trimmed = cur_text
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .trim();
    if trimmed.is_empty() {
        return ' ';
    }
    let is_block_opener = trimmed.ends_with('{') || trimmed.ends_with(':');
    if !is_block_opener {
        return ' ';
    }
    // Confirm the next non-blank line has greater indentation.
    let total = buffer.len_lines();
    if line_idx + 1 < total {
        let next_line = buffer.content.line(line_idx + 1);
        let next_text: String = next_line.chars().collect();
        if !next_text.trim().is_empty()
            && line_indent_of(buffer, line_idx + 1) > line_indent_of(buffer, line_idx)
        {
            return '-';
        }
    }
    ' '
}

/// Compute the line-number text for a given mode/indices.
fn gutter_num_text(mode: LineNumberMode, line_idx: usize, cursor_line: usize) -> Option<String> {
    match mode {
        LineNumberMode::None => None,
        LineNumberMode::Absolute => Some((line_idx + 1).to_string()),
        LineNumberMode::Relative => {
            let dist = line_idx.abs_diff(cursor_line);
            if dist == 0 {
                Some((line_idx + 1).to_string())
            } else {
                Some(dist.to_string())
            }
        }
        LineNumberMode::Hybrid => {
            if line_idx == cursor_line {
                Some((line_idx + 1).to_string())
            } else {
                Some(line_idx.abs_diff(cursor_line).to_string())
            }
        }
    }
}

/// Pre-format the gutter string for one line.
/// Returns an empty string when line numbers are disabled.
fn format_gutter(
    mode: LineNumberMode,
    line_idx: usize,
    cursor_line: usize,
    gutter_char_width: usize,
) -> String {
    if gutter_char_width == 0 {
        return String::new();
    }
    let num_text = match gutter_num_text(mode, line_idx, cursor_line) {
        Some(t) => t,
        None => return String::new(),
    };
    // Right-align within gutter_char_width - 1 (leave one char gap on the right)
    format!(
        "{:>width$}",
        num_text,
        width = gutter_char_width.saturating_sub(1)
    )
}

/// Pre-format the gutter string with a fold indicator prefix.
///
/// Layout: `[fold_char][number right-aligned in gutter_char_width-2 cols]`
/// where the trailing column is the gap before code starts.
/// `fold_char` is `+` (closed fold), `-` (open foldable region), or ` `.
/// When `gutter_char_width == 1` (fold indicator only, no line numbers),
/// returns just the single fold character.
fn format_gutter_with_fold(
    mode: LineNumberMode,
    line_idx: usize,
    cursor_line: usize,
    gutter_char_width: usize,
    fold_char: char,
) -> String {
    if gutter_char_width == 0 {
        return String::new();
    }
    // Fold indicator only (line numbers disabled).
    if gutter_char_width == 1 {
        return fold_char.to_string();
    }
    let num_text = match gutter_num_text(mode, line_idx, cursor_line) {
        Some(t) => t,
        // Line numbers disabled but fold col is still present.
        None => return fold_char.to_string(),
    };
    // Number is right-aligned in gutter_char_width - 2 (1 for fold indicator, 1 trailing gap)
    let num_part = format!(
        "{:>width$}",
        num_text,
        width = gutter_char_width.saturating_sub(2)
    );
    format!("{}{}", fold_char, num_part)
}

/// Calculate the gutter width in *character columns* (0 = no gutter).
///
/// When line numbers are enabled the gutter always includes one extra column
/// for the fold indicator (`+`, `-`, or space).
/// When `has_git_diff` is true, one additional column is prepended for the
/// git diff marker (`▌` or space).
/// The GTK backend multiplies this by `char_width` pixels to get the pixel
/// gutter width; a TUI backend uses it directly as cell count.
pub fn calculate_gutter_cols(
    mode: LineNumberMode,
    total_lines: usize,
    _char_width: f64,
    has_git_diff: bool,
    has_breakpoints: bool,
) -> usize {
    let git = if has_git_diff { 1 } else { 0 };
    let bp = if has_breakpoints { 1 } else { 0 };
    match mode {
        // No line numbers: show only the 1-column fold indicator.
        LineNumberMode::None => 1 + git + bp,
        LineNumberMode::Absolute => {
            let digits = total_lines.to_string().len().max(1);
            digits + 2 + 1 + git + bp // digits + padding + fold indicator + git + bp
        }
        LineNumberMode::Relative | LineNumberMode::Hybrid => {
            let max_relative = total_lines.saturating_sub(1);
            let digits = max_relative.to_string().len().max(3);
            digits + 2 + 1 + git + bp
        }
    }
}

fn build_status_line(engine: &Engine) -> (String, String, Option<(usize, usize)>) {
    let mode_str = engine.mode_str();

    let filename = match engine.file_path() {
        Some(p) => p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string()),
        None => "[No Name]".to_string(),
    };

    let dirty = if engine.dirty() { " [+]" } else { "" };

    let recording = if let Some(reg) = engine.macro_recording {
        format!(" [recording @{}]", reg)
    } else {
        String::new()
    };

    // Build branch segment with ahead/behind counts
    let branch = if let Some(b) = engine.git_branch.as_deref() {
        let mut branch_text = b.to_string();
        if engine.sc_ahead > 0 || engine.sc_behind > 0 {
            let mut parts = Vec::new();
            if engine.sc_ahead > 0 {
                parts.push(format!("↑{}", engine.sc_ahead));
            }
            if engine.sc_behind > 0 {
                parts.push(format!("↓{}", engine.sc_behind));
            }
            branch_text = format!("{} {}", branch_text, parts.join(" "));
        }
        format!(" [{}]", branch_text)
    } else {
        String::new()
    };

    let prefix = format!(" -- {}{} -- {}{}", mode_str, recording, filename, dirty);
    let branch_range = if branch.is_empty() {
        None
    } else {
        let start = prefix.len();
        let end = start + branch.len();
        Some((start, end))
    };

    let left = format!("{}{}", prefix, branch);

    let cursor = engine.cursor();
    let (errors, warnings) = engine.diagnostic_counts();
    let diag_str = if errors > 0 || warnings > 0 {
        format!("  E:{} W:{}", errors, warnings)
    } else {
        String::new()
    };
    let right = format!(
        "Ln {}, Col {}  ({} lines){} ",
        cursor.line + 1,
        cursor.col + 1,
        engine.buffer().len_lines(),
        diag_str
    );

    (left, right, branch_range)
}

/// Build a quadraui `StatusBar` for the global (bottom-of-screen) status bar.
pub fn build_global_status_bar(engine: &Engine, theme: &Theme) -> quadraui::StatusBar {
    let (left, right, _branch_range) = build_status_line(engine);
    let fg = quadraui::Color::rgb(theme.status_fg.r, theme.status_fg.g, theme.status_fg.b);
    let bg = quadraui::Color::rgb(theme.status_bg.r, theme.status_bg.g, theme.status_bg.b);
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("status:global"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: left,
            fg,
            bg,
            bold: false,
            action_id: None,
        }],
        right_segments: vec![quadraui::StatusBarSegment {
            text: right,
            fg,
            bg,
            bold: false,
            action_id: None,
        }],
    }
}

/// Build a `quadraui::ToastStack` from `engine.toasts` for the
/// bottom-right corner. Backends call `quadraui::*::draw_toast_stack`
/// with the result. Returns None when there are no toasts so callers
/// can skip the draw entirely.
pub fn build_toast_stack(engine: &Engine) -> Option<quadraui::ToastStack> {
    if engine.toasts.is_empty() {
        return None;
    }
    Some(quadraui::ToastStack {
        id: quadraui::WidgetId::new("toasts"),
        corner: quadraui::ToastCorner::BottomRight,
        toasts: engine
            .toasts
            .iter()
            .map(|t| quadraui::ToastItem {
                id: quadraui::WidgetId::new(format!("toast-{}", t.id)),
                title: t.title.clone(),
                body: t.body.clone(),
                severity: t.severity,
                action: None,
                accent: None,
            })
            .collect(),
    })
}

/// Format the LSP status segment text when the server is still
/// indexing (#221). Renders `name • Indexing: 319/320` when the
/// server is publishing `$/progress`; falls back to the dimmed
/// `name… ` placeholder when no progress data is available.
///
/// Width discipline: progress notifications fire many times per second
/// with varying message lengths. If the segment width fluctuates,
/// `StatusBar::layout`'s priority-drop kicks in and lower-priority
/// segments flash in/out — visually glitchy. The formatter keeps the
/// segment width stable and ≤ ~28 cells by preferring fixed-width
/// detail (percentage, then `X/Y` if the message starts with one) and
/// otherwise dropping the message in favour of `stage…`.
pub fn format_lsp_progress_segment(
    label: &str,
    progress: Option<&crate::core::lsp_manager::LspProgress>,
) -> String {
    let Some(progress) = progress else {
        return format!("{label}… ");
    };
    let stage = if progress.title.is_empty() {
        "working"
    } else {
        progress.title.as_str()
    };
    let detail = compact_progress_detail(progress.message.as_deref(), progress.percentage);
    if detail.is_empty() {
        format!("{label} • {stage}… ")
    } else {
        format!("{label} • {stage}: {detail} ")
    }
}

/// Pick a compact, fixed-width-ish detail string from the progress
/// fields. Preference order:
///   1. `percentage` — always at most 4 chars (`100%`).
///   2. Leading `X/Y` of the message (rust-analyzer's path-laden
///      messages like `"34/285: /home/john/…"` collapse to `34/285`).
///   3. Empty — caller renders `stage…` instead. Skipping verbose
///      free-text messages keeps the segment from flapping width on
///      every `$/progress` report.
fn compact_progress_detail(message: Option<&str>, percentage: Option<u32>) -> String {
    if let Some(pct) = percentage {
        return format!("{pct}%");
    }
    if let Some(msg) = message {
        if let Some(prefix) = extract_xy_prefix(msg) {
            return prefix.to_string();
        }
    }
    String::new()
}

/// Extract a leading `digit+/digit+` prefix from a message, e.g.
/// `"34/285"` from `"34/285: /home/john/…"` or `"34/285"`. Returns
/// None when the message doesn't start with that shape.
fn extract_xy_prefix(msg: &str) -> Option<&str> {
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i == bytes.len() || bytes[i] != b'/' {
        return None;
    }
    let mut j = i + 1;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == i + 1 {
        return None;
    }
    Some(&msg[..j])
}

/// Build a per-window status line for a given window.
/// Active windows get a rich, colorful bar; inactive windows get dimmed minimal info.
pub fn build_window_status_line(
    engine: &Engine,
    theme: &Theme,
    window_id: WindowId,
    is_active: bool,
) -> WindowStatusLine {
    let window = engine.windows.get(&window_id);
    let buffer_state = window.and_then(|w| engine.buffer_manager.get(w.buffer_id));
    let view = window.map(|w| &w.view);

    // Filename
    let filename = buffer_state
        .and_then(|s| s.file_path.as_ref())
        .and_then(|p| p.file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .or_else(|| buffer_state.and_then(|s| s.scratch_name.as_ref()).cloned())
        .unwrap_or_else(|| "[No Name]".to_string());

    let dirty = buffer_state.is_some_and(|s| s.dirty);
    let cursor = view.map(|v| &v.cursor);
    // Filetype from path
    let filetype = buffer_state
        .and_then(|s| s.file_path.as_ref())
        .and_then(|p| crate::core::lsp::language_id_from_path(p))
        .unwrap_or_default();

    // Derive per-window status bar colors from the editor background.
    // Active: bg shifted ~10% from editor bg (lighter on dark themes, darker on light).
    // Inactive: uses theme's status_inactive_bg/fg.
    let lum = 0.299 * theme.background.r as f64
        + 0.587 * theme.background.g as f64
        + 0.114 * theme.background.b as f64;
    let bar_bg = if lum < 128.0 {
        theme.background.lighten(0.10)
    } else {
        theme.background.darken(0.10)
    };
    let bar_fg = theme.foreground;

    // Mode text color — use the mode badge color as a subtle text tint
    let mode_color = match engine.mode {
        Mode::Insert => theme.status_mode_insert_bg,
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => theme.status_mode_visual_bg,
        Mode::Replace => theme.status_mode_replace_bg,
        _ => bar_fg, // normal mode: just use regular fg
    };

    // Indentation display text
    let indent_text = if engine.settings.expand_tab {
        format!("Spaces: {} ", engine.settings.tabstop)
    } else {
        format!("Tab Size: {} ", engine.settings.tabstop)
    };

    // Line ending display
    let line_ending_str = buffer_state.map(|s| s.line_ending.as_str()).unwrap_or("LF");

    if is_active {
        // ── Active: MODE filename [+] branch | filetype indent encoding eol Ln:Col ──
        let mode_str = engine.mode_str();

        let mut left = vec![
            StatusSegment {
                text: format!(" {} ", mode_str),
                fg: mode_color,
                bg: bar_bg,
                bold: true,
                action: None,
            },
            StatusSegment {
                text: format!(" {}", filename),
                fg: bar_fg,
                bg: bar_bg,
                bold: true,
                action: None,
            },
        ];

        if dirty {
            left.push(StatusSegment {
                text: " [+]".to_string(),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: None,
            });
        }

        // Recording indicator
        if let Some(reg) = engine.macro_recording {
            left.push(StatusSegment {
                text: format!(" [rec @{}]", reg),
                fg: theme.status_mode_replace_bg,
                bg: bar_bg,
                bold: true,
                action: None,
            });
        }

        // Git branch
        if let Some(b) = engine.git_branch.as_deref() {
            let mut branch_text = b.to_string();
            if engine.sc_ahead > 0 || engine.sc_behind > 0 {
                let mut parts = Vec::new();
                if engine.sc_ahead > 0 {
                    parts.push(format!("↑{}", engine.sc_ahead));
                }
                if engine.sc_behind > 0 {
                    parts.push(format!("↓{}", engine.sc_behind));
                }
                branch_text = format!("{} {}", branch_text, parts.join(" "));
            }
            left.push(StatusSegment {
                text: format!("  {}", branch_text),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::SwitchBranch),
            });
        }

        // LSP status segment — server_has_responded in LspManager already tracks
        // whether the server is fully ready (responded to hover/definition/etc.).
        let lsp_status = window
            .map(|w| engine.lsp_status_for_buffer(w.buffer_id))
            .unwrap_or(crate::core::lsp_manager::LspStatus::None);
        // #221: when indexing is in flight, format `name • Indexing: 319/320`
        // from the latest $/progress snapshot. Falls back to the plain
        // `name…` placeholder when the server isn't reporting progress.
        let lsp_progress = window.and_then(|w| engine.lsp_progress_for_buffer(w.buffer_id));

        // Right side — ordered least-important → most-important (left → right
        // when right-aligned). Narrow bars drop from the front of this list,
        // so cursor position (highest priority) stays at the right edge.
        // See issue #159 for priority rationale.
        //
        // Drop order (least → most important):
        //   notification · menu toggle · panel toggle · sidebar toggle ·
        //   utf-8 · line ending · indent · filetype · LSP · cursor pos
        let mut right = Vec::new();

        // Build each segment optionally; push at the end in priority order.
        // (Segments whose data is absent simply stay None and aren't pushed.)

        // Notification — spinner for in-progress, bell for done
        let notification_seg = if !engine.notifications.is_empty() {
            let nf = crate::icons::nerd_fonts_enabled();
            let has_active = engine.has_active_notifications();
            let has_done = engine.has_done_notifications();
            let (icon, fg_color) = if has_active {
                let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
                let elapsed = engine
                    .notifications
                    .iter()
                    .filter(|n| !n.done)
                    .map(|n| n.created_at)
                    .min()
                    .map(|t| t.elapsed().as_millis() as usize / 100)
                    .unwrap_or(0);
                let frame = frames[elapsed % frames.len()];
                (format!("{frame}"), theme.function)
            } else if has_done {
                let bell: &str = if nf { "󰂞" } else { "*" };
                (bell.to_string(), theme.string_lit)
            } else {
                (String::new(), bar_fg)
            };
            if !icon.is_empty() {
                let msg = engine
                    .notifications
                    .last()
                    .map(|n| {
                        if n.message.len() > 30 {
                            format!("{}…", &n.message[..29])
                        } else {
                            n.message.clone()
                        }
                    })
                    .unwrap_or_default();
                let action = if has_done {
                    Some(StatusAction::DismissNotifications)
                } else {
                    None
                };
                Some(StatusSegment {
                    text: format!(" {icon} {msg} "),
                    fg: fg_color,
                    bg: bar_bg,
                    bold: false,
                    action,
                })
            } else {
                None
            }
        } else {
            None
        };

        // Layout toggle buttons
        let toggle_fg = |active: bool| {
            if active {
                bar_fg
            } else {
                theme.status_inactive_fg
            }
        };
        let nf = crate::icons::nerd_fonts_enabled();

        let menu_toggle_seg = if engine.menu_bar_toggleable {
            Some(StatusSegment {
                text: if nf { " 󰍜 " } else { " [M] " }.to_string(),
                fg: toggle_fg(engine.menu_bar_visible),
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::ToggleMenuBar),
            })
        } else {
            None
        };

        let panel_toggle_seg = StatusSegment {
            text: if nf { " 󰆍 " } else { " [P] " }.to_string(),
            fg: toggle_fg(engine.terminal_open || engine.bottom_panel_open),
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::TogglePanel),
        };

        let sidebar_toggle_seg = StatusSegment {
            text: if nf { " 󰘖 " } else { " [S] " }.to_string(),
            fg: toggle_fg(engine.session.explorer_visible),
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ToggleSidebar),
        };

        let encoding_seg = StatusSegment {
            text: "utf-8 ".to_string(),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ChangeEncoding),
        };

        let line_ending_seg = StatusSegment {
            text: format!("{} ", line_ending_str),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ChangeLineEnding),
        };

        let indent_seg = StatusSegment {
            text: indent_text.clone(),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ChangeIndentation),
        };

        let filetype_seg = if !filetype.is_empty() {
            Some(StatusSegment {
                text: format!("{} ", filetype),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::ChangeLanguage),
            })
        } else {
            None
        };

        let lsp_seg = {
            use crate::core::lsp_manager::LspStatus;
            let (lsp_text, lsp_fg) = match &lsp_status {
                LspStatus::Running(name) => (Some(format!("{} ", name)), bar_fg),
                LspStatus::Initializing(name) => {
                    let label = if name.is_empty() { "LSP" } else { name };
                    let text = format_lsp_progress_segment(label, lsp_progress.as_ref());
                    (Some(text), theme.status_inactive_fg)
                }
                LspStatus::Installing => (Some("LSP↓ ".to_string()), theme.status_inactive_fg),
                LspStatus::Crashed => (Some("LSP✗ ".to_string()), theme.status_mode_replace_bg),
                LspStatus::None => (None, bar_fg),
            };
            lsp_text.map(|text| StatusSegment {
                text,
                fg: lsp_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::LspInfo),
            })
        };

        let cursor_seg = cursor.map(|c| StatusSegment {
            text: format!(" Ln {}, Col {} ", c.line + 1, c.col + 1),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::GoToLine),
        });

        // Push in priority order: least-important first.
        if let Some(s) = notification_seg {
            right.push(s);
        }
        if let Some(s) = menu_toggle_seg {
            right.push(s);
        }
        right.push(panel_toggle_seg);
        right.push(sidebar_toggle_seg);
        right.push(encoding_seg);
        right.push(line_ending_seg);
        right.push(indent_seg);
        if let Some(s) = filetype_seg {
            right.push(s);
        }
        if let Some(s) = lsp_seg {
            right.push(s);
        }
        if let Some(s) = cursor_seg {
            right.push(s);
        }

        WindowStatusLine {
            left_segments: left,
            right_segments: right,
        }
    } else {
        // ── Inactive window: filename [+]  |  Ln:Col ──
        let mut left = vec![StatusSegment {
            text: format!(" {}", filename),
            fg: theme.status_inactive_fg,
            bg: theme.status_inactive_bg,
            bold: false,
            action: None,
        }];

        if dirty {
            left.push(StatusSegment {
                text: " [+]".to_string(),
                fg: theme.status_inactive_fg,
                bg: theme.status_inactive_bg,
                bold: false,
                action: None,
            });
        }

        let right = if let Some(c) = cursor {
            vec![StatusSegment {
                text: format!("Ln {}, Col {} ", c.line + 1, c.col + 1),
                fg: theme.status_inactive_fg,
                bg: theme.status_inactive_bg,
                bold: false,
                action: None,
            }]
        } else {
            vec![]
        };

        WindowStatusLine {
            left_segments: left,
            right_segments: right,
        }
    }
}

/// Compute hit regions from status line segments.
/// `bar_width` is the total width in char cells.
/// Returns `(col, width, action)` tuples for all interactive segments.
pub fn compute_status_hit_regions(
    left: &[StatusSegment],
    right: &[StatusSegment],
    bar_width: usize,
) -> Vec<(u16, u16, StatusAction)> {
    let mut regions = Vec::new();
    // Left segments: accumulate from col 0
    let mut col: u16 = 0;
    for seg in left {
        let w = seg.text.chars().count() as u16;
        if let Some(ref action) = seg.action {
            regions.push((col, w, action.clone()));
        }
        col += w;
    }
    // Right segments: right-aligned
    let right_width: usize = right.iter().map(|s| s.text.chars().count()).sum();
    let mut col = bar_width.saturating_sub(right_width) as u16;
    for seg in right {
        let w = seg.text.chars().count() as u16;
        if let Some(ref action) = seg.action {
            regions.push((col, w, action.clone()));
        }
        col += w;
    }
    regions
}

/// Resolve a column position to a `StatusAction` using pre-computed hit regions.
pub fn resolve_status_bar_click(
    hit_regions: &[(u16, u16, StatusAction)],
    col: u16,
) -> Option<StatusAction> {
    for &(start, width, ref action) in hit_regions {
        if col >= start && col < start + width {
            return Some(action.clone());
        }
    }
    None
}

// ─── quadraui::TabBar adapter (A.6c / A.6d) ──────────────────────────────────

/// The `WidgetId` every editor tab bar paints under.
///
/// Backends cache a resolved `quadraui::TabBarLayout` per `WidgetId` at paint
/// time, and quadraui#594's `GtkDriver::tab_center` / `tab_close_center` look
/// the layout back up by that id — so a test aiming a click at a specific tab
/// needs this exact string. Named here rather than spelled out at each call
/// site so the harness and the primitive cannot drift apart (#659).
///
/// Note this is a *per-bar* id, not a per-group one: in a split every group's
/// tab bar paints under the same id, so the cached layout is whichever group
/// painted last. That is fine for the single-group tests that consume it and
/// is why [`crate::core::window::GroupId`]-keyed geometry still exists on the
/// `App` side for production click routing.
pub const EDITOR_TAB_BAR_WIDGET_ID: &str = "tabs:group";

/// Build a `quadraui::TabBar` primitive from the render-level tab args.
/// Shared by TUI and GTK backends — the primitive is layout-agnostic;
/// backends interpret it against their own measurement / drawing models.
///
/// Right-side segment order (mirrors the pre-migration layout):
/// `[diff label?] [diff prev] [diff next] [diff fold?] [split right] [split down] [action menu]`
///
/// `active_accent` carries the active-tab accent colour only when the group
/// is focused. TUI interprets as underline; GTK as 2px top bar.
/// `width_cells` on each segment is a TUI hint; GTK measures with Pango.
pub fn build_tab_bar_primitive(
    tabs: &[TabInfo],
    show_split_btns: bool,
    diff_toolbar: Option<&DiffToolbarData>,
    tab_scroll_offset: usize,
    active_accent: Option<quadraui::Color>,
) -> quadraui::TabBar {
    let tab_items: Vec<quadraui::TabItem> = tabs
        .iter()
        .map(|t| quadraui::TabItem {
            label: t.name.clone(),
            is_active: t.active,
            is_dirty: t.dirty,
            is_preview: t.preview,
            is_closable: true,
        })
        .collect();

    let mut right: Vec<quadraui::TabBarSegment> = Vec::new();

    // Build a 3-cell tab-bar button segment from an `Icon`. When nerd
    // fonts are enabled, the icon glyph is rendered as a 2-cell wide
    // glyph (` <wide>`); otherwise the fallback ASCII char takes 1
    // cell, padded with spaces (` <c> `). Either way `width_cells = 3`
    // matches the rasteriser's per-cell stride so layout positions
    // line up with what gets painted.
    fn tab_btn_segment(
        icon: &crate::icons::Icon,
        id: &str,
        is_active: bool,
    ) -> quadraui::TabBarSegment {
        let text = if crate::icons::nerd_fonts_enabled() {
            format!(" {}", icon.s())
        } else {
            format!(" {} ", icon.s())
        };
        quadraui::TabBarSegment {
            text,
            width_cells: 3,
            id: Some(quadraui::WidgetId::new(id)),
            is_active,
        }
    }

    if let Some(dt) = diff_toolbar {
        if let Some(label) = &dt.change_label {
            let text = format!(" {label}");
            let width = text.chars().count() as u16;
            right.push(quadraui::TabBarSegment {
                text,
                width_cells: width,
                id: None,
                is_active: false,
            });
        }
        right.push(tab_btn_segment(
            &crate::icons::DIFF_PREV,
            "tab:diff_prev",
            false,
        ));
        right.push(tab_btn_segment(
            &crate::icons::DIFF_NEXT,
            "tab:diff_next",
            false,
        ));
        right.push(tab_btn_segment(
            &crate::icons::DIFF_FOLD,
            "tab:diff_toggle",
            dt.unchanged_hidden,
        ));
    }

    if show_split_btns {
        right.push(tab_btn_segment(
            &crate::icons::SPLIT_RIGHT,
            "tab:split_right",
            false,
        ));
        right.push(tab_btn_segment(
            &crate::icons::SPLIT_DOWN,
            "tab:split_down",
            false,
        ));
    }

    right.push(quadraui::TabBarSegment {
        // Action menu uses U+22EF (HORIZONTAL ELLIPSIS) which is a
        // standard Unicode glyph, not a Nerd Font codepoint.
        text: " \u{22EF} ".to_string(),
        width_cells: 3,
        id: Some(quadraui::WidgetId::new("tab:action_menu")),
        is_active: false,
    });

    quadraui::TabBar {
        id: quadraui::WidgetId::new(EDITOR_TAB_BAR_WIDGET_ID),
        tabs: tab_items,
        scroll_offset: tab_scroll_offset,
        right_segments: right,
        active_accent,
        show_tab_close: true,
        compact: false,
    }
}

/// Build a `quadraui::TabBar` for the bottom panel tab switcher
/// (Terminal / Debug Output). The close button (×) is a right segment.
/// Tabs with `close_width: 0.0` suppress per-tab close glyphs.
pub fn build_bottom_panel_tab_bar(
    active: &BottomPanelKind,
    has_terminal: bool,
    has_debug_output: bool,
) -> quadraui::TabBar {
    let mut tabs = Vec::new();
    if has_terminal {
        tabs.push(quadraui::TabItem {
            label: "Terminal".to_string(),
            is_active: *active == BottomPanelKind::Terminal,
            is_dirty: false,
            is_preview: false,
            is_closable: true,
        });
    }
    if has_debug_output {
        tabs.push(quadraui::TabItem {
            label: "Debug Output".to_string(),
            is_active: *active == BottomPanelKind::DebugOutput,
            is_dirty: false,
            is_preview: false,
            is_closable: true,
        });
    }

    let close_seg = quadraui::TabBarSegment {
        text: " \u{00d7} ".to_string(),
        width_cells: 3,
        id: Some(quadraui::WidgetId::new("bottom_tab:close")),
        is_active: false,
    };

    quadraui::TabBar {
        id: quadraui::WidgetId::new("tabs:bottom_panel"),
        tabs,
        scroll_offset: 0,
        right_segments: vec![close_seg],
        active_accent: None,
        show_tab_close: false,
        compact: true,
    }
}

// ─── Terminal toolbar adapter (#305) ─────────────────────────────────────────

/// Nerd-font icons for the terminal toolbar segments.
const NF_TERM_CLOSE: &str = "󰅖";
const NF_TERM_SPLIT: &str = "󰤼";
const NF_TERM_MAXIMIZE: &str = "󰊗";
const NF_TERM_UNMAXIMIZE: &str = "󰊓";

/// The terminal toolbar is either a find bar or a tab strip.
pub enum TerminalToolbar {
    FindBar(quadraui::StatusBar),
    TabStrip(quadraui::TabBar),
}

/// Build a `TerminalToolbar` from the current terminal panel state.
pub fn build_terminal_toolbar(panel: &TerminalPanel, theme: &Theme) -> TerminalToolbar {
    if panel.find_active {
        let fg = to_quadraui_color(theme.status_fg);
        let bg = to_quadraui_color(theme.status_bg);

        let match_info = if panel.find_match_count == 0 {
            if panel.find_query.is_empty() {
                String::new()
            } else {
                " (no matches)".to_string()
            }
        } else {
            format!(
                " ({}/{})",
                panel.find_selected_idx + 1,
                panel.find_match_count
            )
        };
        let find_text = format!(" FIND: {}█{}", panel.find_query, match_info);

        TerminalToolbar::FindBar(quadraui::StatusBar {
            id: quadraui::WidgetId::new("term_toolbar"),
            left_segments: vec![quadraui::StatusBarSegment {
                text: find_text,
                fg,
                bg,
                bold: false,
                action_id: None,
            }],
            right_segments: vec![quadraui::StatusBarSegment {
                text: format!(" {} ", NF_TERM_CLOSE),
                fg,
                bg,
                bold: false,
                action_id: Some(quadraui::WidgetId::new("term_toolbar:find_close")),
            }],
        })
    } else {
        let mut tabs: Vec<quadraui::TabItem> = (0..panel.tab_count)
            .map(|i| quadraui::TabItem {
                label: format!("[{}]", i + 1),
                is_active: i == panel.active_tab,
                is_dirty: false,
                is_preview: false,
                is_closable: true,
            })
            .collect();

        if tabs.is_empty() {
            tabs.push(quadraui::TabItem {
                label: "TERMINAL".to_string(),
                is_active: false,
                is_dirty: false,
                is_preview: false,
                is_closable: true,
            });
        }

        let maxicon = if panel.maximized {
            NF_TERM_UNMAXIMIZE
        } else {
            NF_TERM_MAXIMIZE
        };

        let right = vec![
            quadraui::TabBarSegment {
                text: "+ ".to_string(),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:add")),
                is_active: false,
            },
            quadraui::TabBarSegment {
                text: format!("{} ", NF_TERM_SPLIT),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:split")),
                is_active: false,
            },
            quadraui::TabBarSegment {
                text: format!("{} ", maxicon),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:maximize")),
                is_active: false,
            },
            quadraui::TabBarSegment {
                text: format!("{} ", NF_TERM_CLOSE),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:close")),
                is_active: false,
            },
        ];

        TerminalToolbar::TabStrip(quadraui::TabBar {
            id: quadraui::WidgetId::new("term_toolbar"),
            tabs,
            scroll_offset: 0,
            right_segments: right,
            active_accent: None,
            show_tab_close: false,
            compact: true,
        })
    }
}

/// Convert a vimcode `Color` into a `quadraui::Color`. Used by GTK to pass
/// the theme accent colour into `build_tab_bar_primitive`.
pub fn to_quadraui_color(c: Color) -> quadraui::Color {
    quadraui::Color::rgb(c.r, c.g, c.b)
}

/// Build the backend-agnostic `quadraui::Theme` from vimcode's rich
/// `render::Theme`. Shared by both TUI and GTK backends — every
/// `draw_*` delegate and `Backend::set_current_theme` call site uses
/// this single source of truth.
pub fn to_quadraui_theme(theme: &Theme) -> quadraui::Theme {
    let chrome = to_quadraui_theme_chrome(theme);
    to_quadraui_theme_editor(theme, chrome)
}

fn to_quadraui_theme_chrome(theme: &Theme) -> quadraui::Theme {
    let q = to_quadraui_color;
    quadraui::Theme {
        background: q(theme.background),
        foreground: q(theme.foreground),
        tab_bar_bg: q(theme.tab_bar_bg),
        tab_active_bg: q(theme.tab_active_bg),
        tab_active_fg: q(theme.tab_active_fg),
        tab_inactive_fg: q(theme.tab_inactive_fg),
        tab_preview_active_fg: q(theme.tab_preview_active_fg),
        tab_preview_inactive_fg: q(theme.tab_preview_inactive_fg),
        separator: q(theme.separator),
        surface_bg: q(theme.fuzzy_bg),
        surface_fg: q(theme.fuzzy_fg),
        selected_bg: q(theme.fuzzy_selected_bg),
        border_fg: q(theme.fuzzy_border),
        title_fg: q(theme.fuzzy_title_fg),
        header_bg: q(theme.status_bg),
        header_fg: q(theme.status_fg),
        muted_fg: q(theme.line_number_fg),
        error_fg: q(theme.diagnostic_error),
        warning_fg: q(theme.diagnostic_warning),
        query_fg: q(theme.fuzzy_query_fg),
        match_fg: q(theme.fuzzy_match_fg),
        accent_fg: q(theme.cursor),
        hover_bg: q(theme.hover_bg),
        hover_fg: q(theme.hover_fg),
        hover_border: q(theme.hover_border),
        input_bg: q(theme.completion_bg),
        inactive_fg: q(theme.status_inactive_fg),
        selection_bg: q(theme.selection),
        link_fg: q(theme.md_link),
        completion_bg: q(theme.completion_bg),
        completion_fg: q(theme.completion_fg),
        completion_border: q(theme.completion_border),
        completion_selected_bg: q(theme.completion_selected_bg),
        accent_bg: q(theme.tab_active_accent),
        scrollbar_track: q(theme.separator),
        scrollbar_thumb: q(theme.scrollbar_thumb),
        ..quadraui::Theme::default()
    }
}

fn to_quadraui_theme_editor(theme: &Theme, chrome: quadraui::Theme) -> quadraui::Theme {
    let q = to_quadraui_color;
    quadraui::Theme {
        editor_active_background: q(theme.active_background),
        cursorline_bg: q(theme.cursorline_bg),
        dap_stopped_bg: q(theme.dap_stopped_bg),
        colorcolumn_bg: q(theme.colorcolumn_bg),
        diff_added_bg: q(theme.diff_added_bg),
        diff_removed_bg: q(theme.diff_removed_bg),
        diff_padding_bg: q(theme.diff_padding_bg),
        line_number_fg: q(theme.line_number_fg),
        line_number_active_fg: q(theme.line_number_active_fg),
        diagnostic_error: q(theme.diagnostic_error),
        diagnostic_warning: q(theme.diagnostic_warning),
        diagnostic_info: q(theme.diagnostic_info),
        diagnostic_hint: q(theme.diagnostic_hint),
        git_added: q(theme.git_added),
        git_modified: q(theme.git_modified),
        git_deleted: q(theme.git_deleted),
        lightbulb: q(theme.lightbulb),
        spell_error: q(theme.spell_error),
        cursor: q(theme.cursor),
        cursor_normal_alpha: theme.cursor_normal_alpha as f32,
        selection: q(theme.selection),
        selection_alpha: theme.selection_alpha as f32,
        yank_highlight_bg: q(theme.yank_highlight_bg),
        yank_highlight_alpha: theme.yank_highlight_alpha as f32,
        bracket_match_bg: q(theme.bracket_match_bg),
        indent_guide_fg: q(theme.indent_guide_fg),
        indent_guide_active_fg: q(theme.indent_guide_active_fg),
        annotation_fg: q(theme.annotation_fg),
        ghost_text_fg: q(theme.ghost_text_fg),
        ..chrome
    }
}

// ─── quadraui::Editor adapter (#276 Stage 1C) ────────────────────────────────
//
// Convert a vimcode `RenderedWindow` (engine-side IR) into a
// `quadraui::Editor` for the lifted TUI / GTK rasterisers. Field-for-
// field mapping; the engine builder remains unchanged (`RenderedWindow`
// is still consumed by mouse hit-testing in `tui_main/mouse.rs` and
// `gtk/click.rs`, which is why we adapt at the boundary rather than
// retargeting the builder).

/// Build the [`quadraui::Editor`] + [`quadraui::EditorLayout`] pair for a
/// window using **exactly** the same construction paint uses
/// (`to_q_editor` + `Editor::layout(editor.rect, ...)`), so click-column
/// resolution and paint derive from one shared geometry computation
/// instead of two independently reconstructed ones (#560). Callers pass
/// the resulting `&Editor`/`&EditorLayout` straight into
/// `quadraui::Backend::editor_col_at_x` (GTK: exact Pango `xy_to_index`
/// against the same per-span-attributed layout `draw_editor` painted
/// with; TUI: `EditorLayout::col_at_x`'s uniform monospace division) —
/// neither backend hand-rolls its own text-column inverse anymore.
pub fn editor_text_layout(
    rw: &RenderedWindow,
    char_width: f64,
    line_height: f64,
) -> (quadraui::Editor, quadraui::EditorLayout) {
    let editor = to_q_editor(rw);
    let layout = editor.layout(editor.rect, char_width as f32, line_height as f32);
    (editor, layout)
}

/// Build a [`quadraui::Editor`] from a [`RenderedWindow`]. The
/// per-window status line is **not** included — the caller paints
/// it after calling `draw_editor` (status-line lift was Session 241).
pub fn to_q_editor(rw: &RenderedWindow) -> quadraui::Editor {
    quadraui::Editor {
        id: quadraui::WidgetId::new(format!("editor:{}", rw.window_id.0)),
        rect: quadraui::Rect::new(
            rw.rect.x as f32,
            rw.rect.y as f32,
            rw.rect.width as f32,
            rw.rect.height as f32,
        ),
        lines: rw.lines.iter().map(to_q_editor_line).collect(),
        cursor: rw.cursor.map(|(pos, shape)| quadraui::EditorCursor {
            pos: to_q_cursor_pos(pos),
            shape: to_q_cursor_shape(shape),
        }),
        extra_cursors: rw
            .extra_cursors
            .iter()
            .copied()
            .map(to_q_cursor_pos)
            .collect(),
        selection: rw.selection.as_ref().map(to_q_selection),
        extra_selections: rw.extra_selections.iter().map(to_q_selection).collect(),
        yank_highlight: rw.yank_highlight.as_ref().map(to_q_selection),
        scroll_top: rw.scroll_top,
        scroll_left: rw.scroll_left,
        total_lines: rw.total_lines,
        max_col: rw.max_col,
        gutter_char_width: rw.gutter_char_width,
        is_active: rw.is_active,
        show_active_bg: rw.show_active_bg,
        has_git_diff: rw.has_git_diff,
        has_breakpoints: rw.has_breakpoints,
        diagnostic_gutter: rw
            .diagnostic_gutter
            .iter()
            .map(|(&l, &s)| (l, to_q_severity(s)))
            .collect(),
        code_action_lines: rw.code_action_lines.iter().copied().collect(),
        bracket_match_positions: rw.bracket_match_positions.clone(),
        active_indent_col: rw.active_indent_col,
        tabstop: rw.tabstop,
        cursorline: rw.cursorline,
        lightbulb_glyph: crate::icons::LIGHTBULB.c(),
    }
}

fn to_q_editor_line(rl: &RenderedLine) -> quadraui::EditorLine {
    quadraui::EditorLine {
        raw_text: rl.raw_text.clone(),
        gutter_text: rl.gutter_text.clone(),
        spans: rl.spans.iter().map(to_q_styled_span).collect(),
        line_idx: rl.line_idx,
        is_current_line: rl.is_current_line,
        is_fold_header: rl.is_fold_header,
        folded_line_count: rl.folded_line_count,
        git_diff: rl.git_diff.map(to_q_git_status),
        diff_status: rl.diff_status.map(to_q_diff_line),
        diagnostics: rl.diagnostics.iter().map(to_q_diagnostic_mark).collect(),
        spell_errors: rl.spell_errors.iter().map(to_q_spell_mark).collect(),
        is_breakpoint: rl.is_breakpoint,
        is_conditional_bp: rl.is_conditional_bp,
        is_dap_current: rl.is_dap_current,
        is_wrap_continuation: rl.is_wrap_continuation,
        segment_col_offset: rl.segment_col_offset,
        annotation: rl.annotation.clone(),
        ghost_suffix: rl.ghost_suffix.clone(),
        is_ghost_continuation: rl.is_ghost_continuation,
        indent_guides: rl.indent_guides.clone(),
        colorcolumns: rl.colorcolumns.clone(),
    }
}

fn to_q_styled_span(span: &StyledSpan) -> quadraui::EditorStyledSpan {
    quadraui::EditorStyledSpan {
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        style: quadraui::EditorStyle {
            fg: to_quadraui_color(span.style.fg),
            bg: span.style.bg.map(to_quadraui_color),
            bold: span.style.bold,
            italic: span.style.italic,
            font_scale: span.style.font_scale as f32,
        },
    }
}

fn to_q_cursor_pos(pos: CursorPos) -> quadraui::EditorCursorPos {
    quadraui::EditorCursorPos {
        view_line: pos.view_line,
        col: pos.col,
    }
}

fn to_q_cursor_shape(shape: CursorShape) -> quadraui::EditorCursorShape {
    match shape {
        CursorShape::Block => quadraui::EditorCursorShape::Block,
        CursorShape::Bar => quadraui::EditorCursorShape::Bar,
        CursorShape::Underline => quadraui::EditorCursorShape::Underline,
    }
}

fn to_q_selection(sel: &SelectionRange) -> quadraui::EditorSelection {
    quadraui::EditorSelection {
        kind: match sel.kind {
            SelectionKind::Char => quadraui::EditorSelectionKind::Char,
            SelectionKind::Line => quadraui::EditorSelectionKind::Line,
            SelectionKind::Block => quadraui::EditorSelectionKind::Block,
        },
        start_line: sel.start_line,
        start_col: sel.start_col,
        end_line: sel.end_line,
        end_col: sel.end_col,
    }
}

fn to_q_severity(s: crate::core::lsp::DiagnosticSeverity) -> quadraui::DiagnosticSeverity {
    use crate::core::lsp::DiagnosticSeverity as V;
    match s {
        V::Error => quadraui::DiagnosticSeverity::Error,
        V::Warning => quadraui::DiagnosticSeverity::Warning,
        V::Information => quadraui::DiagnosticSeverity::Information,
        V::Hint => quadraui::DiagnosticSeverity::Hint,
    }
}

fn to_q_git_status(s: GitLineStatus) -> quadraui::GitLineStatus {
    match s {
        GitLineStatus::Added => quadraui::GitLineStatus::Added,
        GitLineStatus::Modified => quadraui::GitLineStatus::Modified,
        GitLineStatus::Deleted => quadraui::GitLineStatus::Deleted,
    }
}

fn to_q_diff_line(d: DiffLine) -> quadraui::DiffLine {
    match d {
        DiffLine::Same => quadraui::DiffLine::Same,
        DiffLine::Added => quadraui::DiffLine::Added,
        DiffLine::Removed => quadraui::DiffLine::Removed,
        DiffLine::Padding => quadraui::DiffLine::Padding,
    }
}

fn to_q_diagnostic_mark(dm: &DiagnosticMark) -> quadraui::DiagnosticMark {
    quadraui::DiagnosticMark {
        start_col: dm.start_col,
        end_col: dm.end_col,
        severity: to_q_severity(dm.severity),
        message: dm.message.clone(),
    }
}

fn to_q_spell_mark(sm: &SpellMark) -> quadraui::SpellMark {
    quadraui::SpellMark {
        start_col: sm.start_col,
        end_col: sm.end_col,
    }
}

// ─── quadraui::StatusBar adapter (A.6a) ──────────────────────────────────────

/// String id encoding a `StatusAction`. Paired with [`status_action_from_id`].
/// Used to adapt vimcode's engine-side `StatusAction` enum to quadraui's
/// type-erased `WidgetId`-keyed segment actions.
pub fn status_action_id(action: &StatusAction) -> &'static str {
    match action {
        StatusAction::GoToLine => "status:goto_line",
        StatusAction::ChangeLanguage => "status:change_language",
        StatusAction::ChangeIndentation => "status:change_indentation",
        StatusAction::ChangeLineEnding => "status:change_line_ending",
        StatusAction::ChangeEncoding => "status:change_encoding",
        StatusAction::SwitchBranch => "status:switch_branch",
        StatusAction::LspInfo => "status:lsp_info",
        StatusAction::ToggleSidebar => "status:toggle_sidebar",
        StatusAction::TogglePanel => "status:toggle_panel",
        StatusAction::ToggleMenuBar => "status:toggle_menu_bar",
        StatusAction::DismissNotifications => "status:dismiss_notifications",
    }
}

/// Inverse of [`status_action_id`]: decode a `WidgetId` string back into a
/// `StatusAction`. Returns `None` for unknown ids (plugin-emitted, future, etc.).
pub fn status_action_from_id(id: &str) -> Option<StatusAction> {
    match id {
        "status:goto_line" => Some(StatusAction::GoToLine),
        "status:change_language" => Some(StatusAction::ChangeLanguage),
        "status:change_indentation" => Some(StatusAction::ChangeIndentation),
        "status:change_line_ending" => Some(StatusAction::ChangeLineEnding),
        "status:change_encoding" => Some(StatusAction::ChangeEncoding),
        "status:switch_branch" => Some(StatusAction::SwitchBranch),
        "status:lsp_info" => Some(StatusAction::LspInfo),
        "status:toggle_sidebar" => Some(StatusAction::ToggleSidebar),
        "status:toggle_panel" => Some(StatusAction::TogglePanel),
        "status:toggle_menu_bar" => Some(StatusAction::ToggleMenuBar),
        "status:dismiss_notifications" => Some(StatusAction::DismissNotifications),
        _ => None,
    }
}

/// Convert a `WindowStatusLine` (built by `build_window_status_line`) into a
/// `quadraui::StatusBar` primitive. Engine-owned `StatusAction` enums are
/// flattened to opaque `WidgetId` strings so the primitive is
/// engine-agnostic (plugin invariants §10).
///
/// `id` identifies the bar (useful if multiple status bars are rendered, e.g.
/// per-window). Callers can use e.g. `WidgetId::new("status:w0")`.
pub fn window_status_line_to_status_bar(
    status: &WindowStatusLine,
    id: quadraui::WidgetId,
) -> quadraui::StatusBar {
    fn to_seg(s: &StatusSegment) -> quadraui::StatusBarSegment {
        quadraui::StatusBarSegment {
            text: s.text.clone(),
            fg: quadraui::Color::rgb(s.fg.r, s.fg.g, s.fg.b),
            bg: quadraui::Color::rgb(s.bg.r, s.bg.g, s.bg.b),
            bold: s.bold,
            action_id: s
                .action
                .as_ref()
                .map(|a| quadraui::WidgetId::new(status_action_id(a))),
        }
    }
    quadraui::StatusBar {
        id,
        left_segments: status.left_segments.iter().map(to_seg).collect(),
        right_segments: status.right_segments.iter().map(to_seg).collect(),
    }
}

fn build_command_line(engine: &Engine) -> CommandLineData {
    let (text, right_align, show_cursor, cursor_anchor_text) = match engine.mode {
        Mode::Command if engine.history_search_active => {
            let display = format!(
                "(reverse-i-search)'{}': {}",
                engine.history_search_query, engine.command_buffer
            );
            // Cursor sits after the full `:command_buffer` text (in the command line)
            let anchor = format!(":{}", engine.command_buffer);
            (display, false, true, anchor)
        }
        Mode::Command => {
            let prefix_chars: String = engine
                .command_buffer
                .chars()
                .take(engine.command_cursor)
                .collect();
            let anchor = format!(":{}", prefix_chars);
            let full = format!(":{}", engine.command_buffer);
            (full, false, true, anchor)
        }
        Mode::Search => {
            let ch = match engine.search_direction {
                SearchDirection::Forward => '/',
                SearchDirection::Backward => '?',
            };
            let prefix_chars: String = engine
                .command_buffer
                .chars()
                .take(engine.command_cursor)
                .collect();
            let anchor = format!("{}{}", ch, prefix_chars);
            let full = format!("{}{}", ch, engine.command_buffer);
            (full, false, true, anchor)
        }
        Mode::Normal | Mode::Visual | Mode::VisualLine => {
            if let Some(count) = engine.peek_count() {
                (count.to_string(), true, false, String::new())
            } else {
                (engine.message.clone(), false, false, String::new())
            }
        }
        _ => (engine.message.clone(), false, false, String::new()),
    };

    // Safety: strip newlines so the command line never exceeds one row
    let text = if let Some(first) = text.lines().next() {
        first.to_string()
    } else {
        text
    };

    CommandLineData {
        text,
        right_align,
        show_cursor,
        cursor_anchor_text,
    }
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
    /// Click was on a tab-bar split button: (group_id, direction).
    SplitButton(GroupId, SplitDirection),
    /// Click was on a tab's close button: (group_id, tab_idx).
    CloseTab(GroupId, usize),
    /// Click was on a diff toolbar prev-change button.
    DiffToolbarPrev,
    /// Click was on a diff toolbar next-change button.
    DiffToolbarNext,
    /// Click was on a diff toolbar toggle-fold button.
    DiffToolbarToggleFold,
    /// Click was on a per-window status bar segment with an action.
    StatusBarAction(StatusAction),
    /// Click was on the editor action menu button.
    ActionMenuButton(GroupId),
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
    fn contains(&self, x: f64, y: f64) -> bool {
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
pub fn divider_ratio_from_pos(div: &impl DividerGeometry, x: f64, y: f64) -> f64 {
    let mouse_pos = match div.direction() {
        SplitDirection::Vertical => x,
        SplitDirection::Horizontal => y,
    };
    (mouse_pos - div.axis_start()) / div.axis_size()
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
    let (direction, rect) = match div.direction() {
        // vimcode's `Vertical` = side-by-side panes = quadraui's `Horizontal`
        // (their `Split::direction` names the divider's own orientation
        // relative to "panes side by side" vs "panes stacked", the inverse
        // of vimcode's "divider direction" naming — see primitives/split.rs).
        SplitDirection::Vertical => (
            quadraui::SplitDirection::Horizontal,
            quadraui::Rect::new(
                div.axis_start() as f32,
                div.cross_start() as f32,
                div.axis_size() as f32,
                div.cross_size() as f32,
            ),
        ),
        SplitDirection::Horizontal => (
            quadraui::SplitDirection::Vertical,
            quadraui::Rect::new(
                div.cross_start() as f32,
                div.axis_start() as f32,
                div.cross_size() as f32,
                div.axis_size() as f32,
            ),
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

/// Resolve a view-row + relative column to `(buf_line, text_col)` using
/// the pre-computed `RenderedLine` data.
pub fn resolve_text_position(
    rw: &RenderedWindow,
    view_row: usize,
    text_rel_col: usize,
) -> (usize, usize) {
    let rl = rw.lines.get(view_row);
    let buf_line = rl.map(|l| l.line_idx).unwrap_or(rw.scroll_top + view_row);
    let seg_offset = rl.map(|l| l.segment_col_offset).unwrap_or(0);
    let text_col = text_rel_col + rw.scroll_left + seg_offset;
    (buf_line, text_col)
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
    let viewport_cols = if char_width > 0.0 {
        ((rw.rect.width - sb_w) / char_width).floor() as usize
    } else {
        1
    }
    .saturating_sub(rw.gutter_char_width)
    .max(1);
    let has_h_sb = rw.max_col > viewport_cols && viewport_lines > 1;

    // 2. Vertical scrollbar (rightmost column).
    if has_v_sb && rel_x >= rw.rect.width - sb_w {
        return WindowZone::VerticalScrollbar { view_row };
    }

    // 3. Horizontal scrollbar (bottom content row, above status bar).
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

    // 4. Gutter.
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

    // 5. Text area.
    let text_rel_x = rel_x - gutter_w;
    WindowZone::TextArea {
        view_row,
        buf_line: line_idx,
        seg_col_offset,
        text_rel_x,
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
    let per_window = engine.settings.window_status_line;
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
    let quickfix_h = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        6.0 * lh
    } else {
        0.0
    };
    let has_separated = per_window && !engine.settings.status_line_above_terminal && bp_open;
    let separated_status_h = separated_status_height_px(lh, has_separated);
    let wildmenu_h = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        lh
    };
    let status_bar_h = status_bar_height_px(lh, per_window, !engine.wildmenu_items.is_empty());
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
            quickfix_rows: if engine.quickfix_open && !engine.quickfix_items.is_empty() {
                6
            } else {
                0
            },
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

/// Compute the tab bar row height in pixels (the row containing tab labels).
/// Used by GTK and Win-GUI backends.
pub fn tab_row_height_px(line_height: f64) -> f64 {
    (line_height * 1.6).ceil()
}

/// Compute the full tab bar height including optional breadcrumb row.
/// Used by GTK and Win-GUI backends.
pub fn tab_bar_height_px(line_height: f64, breadcrumbs: bool) -> f64 {
    let row_h = tab_row_height_px(line_height);
    if breadcrumbs {
        row_h + line_height
    } else {
        row_h
    }
}

/// Compute the height of the bottom chrome (status bar + wildmenu) in pixels.
pub fn status_bar_height_px(
    line_height: f64,
    per_window_status_line: bool,
    has_wildmenu: bool,
) -> f64 {
    let wildmenu_px = if has_wildmenu { line_height } else { 0.0 };
    let global_rows = if per_window_status_line { 1.0 } else { 2.0 };
    line_height * global_rows + wildmenu_px
}

/// Compute the quickfix panel height in pixels (0 if closed).
pub fn quickfix_height_px(line_height: f64, quickfix_open: bool, item_count: usize) -> f64 {
    if quickfix_open {
        let n = item_count.clamp(1, 10) as f64;
        (n + 1.0) * line_height
    } else {
        0.0
    }
}

/// Compute the terminal/bottom panel height in pixels (0 if closed).
pub fn terminal_panel_height_px(line_height: f64, panel_open: bool, panel_rows: usize) -> f64 {
    if panel_open {
        (panel_rows + 2) as f64 * line_height
    } else {
        0.0
    }
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

// ─── Tab drop-zone (shared) ─────────────────────────────────────────────────

pub struct TabDropGroup {
    pub group_id: GroupId,
    pub rect: quadraui::DropGroupRect,
    pub tab_scroll_offset: usize,
}

pub struct TabDropOverlay {
    pub highlight: Option<quadraui::Rect>,
    pub insertion_bar: Option<quadraui::Rect>,
    pub ghost_position: (f32, f32),
}

/// Lightweight group-bounds descriptor for [`build_tab_drop_groups`].
pub struct DropGroupBounds {
    pub group_id: GroupId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub content_height: f32,
    pub tab_scroll_offset: usize,
}

/// Build `TabDropGroup`s from a set of group bounds.
///
/// `tab_bar_height` is in the same units as the bounds (cells for TUI,
/// pixels for GTK/Win-GUI). Each group's bounds describe the
/// **content area** — the function prepends `tab_bar_height` above.
///
/// `tab_slots_map` maps `GroupId.0` → visible tab slot positions in
/// the same coordinate system as the bounds.
pub fn build_tab_drop_groups(
    group_bounds: &[DropGroupBounds],
    engine: &crate::core::engine::Engine,
    tab_bar_height: f32,
    tab_slots_map: &std::collections::HashMap<usize, Vec<(f32, f32)>>,
) -> (Vec<TabDropGroup>, f32) {
    let mut groups = Vec::new();
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
        groups.push(TabDropGroup {
            group_id: gb.group_id,
            rect: quadraui::DropGroupRect {
                bounds: quadraui::Rect::new(
                    gb.x,
                    gb.y - eff_tbh,
                    gb.width,
                    eff_tbh + gb.content_height,
                ),
                tab_slots,
            },
            tab_scroll_offset: gb.tab_scroll_offset,
        });
    }

    let effective_tbh = if groups.iter().any(|g| engine.is_tab_bar_hidden(g.group_id)) {
        0.0
    } else {
        tab_bar_height
    };
    (groups, effective_tbh)
}

/// Build [`DropGroupBounds`] from a `ScreenLayout`. Both TUI and GTK call
/// this when the `ScreenLayout` is available (draw path, or TUI's cached
/// layout).
///
/// [`DropGroupBounds`] (and `build_tab_drop_groups`, which reconstructs the
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

pub fn compute_tab_drop_zone(
    cursor_x: f32,
    cursor_y: f32,
    groups: &[TabDropGroup],
    tab_bar_height: f32,
) -> crate::core::window::DropZone {
    use crate::core::window::DropZone;

    let rects: Vec<quadraui::DropGroupRect> = groups.iter().map(|g| g.rect.clone()).collect();
    match quadraui::compute_drop_zone(cursor_x, cursor_y, &rects, tab_bar_height) {
        Some(qz) => {
            let g = &groups[qz.group_idx];
            match qz.kind {
                quadraui::DropZoneKind::Center => DropZone::Center(g.group_id),
                quadraui::DropZoneKind::Split(edge) => {
                    let (dir, new_first) = match edge {
                        quadraui::DropEdge::Left => (SplitDirection::Vertical, true),
                        quadraui::DropEdge::Right => (SplitDirection::Vertical, false),
                        quadraui::DropEdge::Top => (SplitDirection::Horizontal, true),
                        quadraui::DropEdge::Bottom => (SplitDirection::Horizontal, false),
                    };
                    DropZone::Split(g.group_id, dir, new_first)
                }
                quadraui::DropZoneKind::TabReorder(idx) => {
                    DropZone::TabReorder(g.group_id, g.tab_scroll_offset + idx)
                }
            }
        }
        None => DropZone::None,
    }
}

pub fn compute_tab_drop_overlay(
    drop_zone: &crate::core::window::DropZone,
    groups: &[TabDropGroup],
    cursor: (f32, f32),
    tab_bar_height: f32,
    bar_thickness: f32,
    ghost_offset: f32,
) -> Option<TabDropOverlay> {
    use crate::core::window::DropZone;

    let ghost_position = (cursor.0 + ghost_offset, cursor.1);

    match drop_zone {
        DropZone::None => None,
        DropZone::Center(gid) => {
            let g = groups.iter().find(|g| g.group_id == *gid)?;
            let b = &g.rect.bounds;
            Some(TabDropOverlay {
                highlight: Some(quadraui::Rect::new(b.x, b.y, b.width, b.height)),
                insertion_bar: None,
                ghost_position,
            })
        }
        DropZone::Split(gid, dir, new_first) => {
            let g = groups.iter().find(|g| g.group_id == *gid)?;
            let b = &g.rect.bounds;
            let h = match (dir, new_first) {
                (SplitDirection::Vertical, true) => {
                    quadraui::Rect::new(b.x, b.y, b.width / 2.0, b.height)
                }
                (SplitDirection::Vertical, false) => {
                    quadraui::Rect::new(b.x + b.width / 2.0, b.y, b.width / 2.0, b.height)
                }
                (SplitDirection::Horizontal, true) => {
                    quadraui::Rect::new(b.x, b.y, b.width, b.height / 2.0)
                }
                (SplitDirection::Horizontal, false) => {
                    quadraui::Rect::new(b.x, b.y + b.height / 2.0, b.width, b.height / 2.0)
                }
            };
            Some(TabDropOverlay {
                highlight: Some(h),
                insertion_bar: None,
                ghost_position,
            })
        }
        DropZone::TabReorder(gid, abs_idx) => {
            let g = groups.iter().find(|g| g.group_id == *gid)?;
            let b = &g.rect.bounds;
            let vis_idx = abs_idx.saturating_sub(g.tab_scroll_offset);
            let bar_x = if vis_idx < g.rect.tab_slots.len() {
                g.rect.tab_slots[vis_idx].0
            } else if let Some(last) = g.rect.tab_slots.last() {
                last.1
            } else {
                b.x
            };
            Some(TabDropOverlay {
                highlight: Some(quadraui::Rect::new(b.x, b.y, b.width, tab_bar_height)),
                insertion_bar: Some(quadraui::Rect::new(
                    bar_x - bar_thickness / 2.0,
                    b.y,
                    bar_thickness,
                    tab_bar_height,
                )),
                ghost_position,
            })
        }
    }
}

/// Compute the scrollbar-to-scroll-top mapping from a click position.
/// Returns the new `scroll_top` value.
///
/// - `click_pos`: relative position of click within the scrollbar track (0.0 .. track_len).
/// - `track_len`: total length of the scrollbar track in pixels (or cells).
/// - `total_lines`: total number of lines in the buffer.
/// - `viewport_lines`: number of visible lines in the viewport.
pub fn scrollbar_click_to_scroll_top(
    click_pos: f64,
    track_len: f64,
    total_lines: usize,
    viewport_lines: usize,
) -> usize {
    if track_len <= 0.0 || total_lines <= viewport_lines {
        return 0;
    }
    let ratio = (click_pos / track_len).clamp(0.0, 1.0);
    let max_scroll = total_lines.saturating_sub(viewport_lines);
    ((ratio * max_scroll as f64).round() as usize).min(max_scroll)
}

/// Compute the display column from a pixel/cell X offset within the text area.
/// Handles tab expansion (tabs = `tabstop` display columns).
///
/// - `line_text`: the text of the buffer line.
/// - `x_offset`: click position relative to the text area start, in character-width units
///   (i.e. `(pixel_x - gutter_px) / char_width` for pixel backends, or `col - gutter` for TUI).
/// - `tabstop`: tab stop width (default 4).
/// - `scroll_left`: horizontal scroll offset in display columns.
///
/// Returns the buffer column index.
pub fn display_col_to_buffer_col(
    line_text: &str,
    x_offset: usize,
    tabstop: usize,
    scroll_left: usize,
) -> usize {
    let target_display_col = x_offset + scroll_left;
    let mut display_col = 0usize;
    for (i, ch) in line_text.chars().enumerate() {
        if display_col >= target_display_col {
            return i;
        }
        if ch == '\t' {
            display_col += tabstop - (display_col % tabstop);
        } else {
            display_col += 1;
        }
    }
    line_text.chars().count()
}

/// Check if a click at `col` within a tab of total width `tab_width` is on the close button.
/// Close button occupies the rightmost `close_cols` columns of the tab.
pub fn is_tab_close_click(col_in_tab: usize, tab_width: usize, close_cols: usize) -> bool {
    tab_width > close_cols && col_in_tab >= tab_width - close_cols
}

/// Matches a key binding string (e.g. `<C-S-e>`) against abstract modifier flags
/// and a key name/char. This is the backend-agnostic core of key matching.
///
/// - `binding`: Vim-style binding string like `<C-b>`, `<C-S-e>`, `<A-x>`.
/// - `ctrl`, `shift`, `alt`: whether these modifiers are pressed.
/// - `key_char`: the lowercase character of the pressed key (if printable).
/// - `is_tab`: true if the pressed key is Tab.
/// - `is_space`: true if the pressed key is Space.
/// - `is_escape`: true if the pressed key is Escape.
#[allow(clippy::too_many_arguments)]
pub fn matches_key_binding(
    binding: &str,
    ctrl: bool,
    shift: bool,
    alt: bool,
    key_char: Option<char>,
    is_tab: bool,
    is_space: bool,
    is_escape: bool,
) -> bool {
    let Some((want_ctrl, want_shift, want_alt, key_name)) =
        crate::core::settings::parse_key_binding_named(binding)
    else {
        return false;
    };
    if want_ctrl != ctrl || want_shift != shift || want_alt != alt {
        return false;
    }
    match key_name.as_str() {
        "Tab" | "tab" => is_tab,
        "Space" | "space" => is_space,
        "Escape" | "Esc" => is_escape,
        s if s.chars().count() == 1 => {
            let want = s.chars().next().unwrap().to_ascii_lowercase();
            key_char
                .map(|c| c.to_ascii_lowercase() == want)
                .unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_sc_data() -> SourceControlData {
        SourceControlData {
            branch: "main".into(),
            ahead: 0,
            behind: 0,
            staged: vec![],
            unstaged: vec![],
            worktrees: vec![],
            log: vec![],
            sections_expanded: [true; 4],
            selected: 0,
            has_focus: false,
            commit_message: String::new(),
            commit_cursor: 0,
            commit_input_active: false,
            button_focused: None,
            button_hovered: None,
            branch_picker: None,
            help_open: false,
            sc_sections_start_y: None,
        }
    }

    #[test]
    fn test_sc_button_toolbar_ids_and_shape() {
        use crate::core::engine::SC_BUTTON_IDS;
        use quadraui::ToolbarButton;

        let sc = empty_sc_data();
        let bar = sc_button_toolbar(&sc);
        assert_eq!(bar.buttons.len(), 4);

        // Ids appear in button-index order and match the shared constant.
        for (i, btn) in bar.buttons.iter().enumerate() {
            match btn {
                ToolbarButton::Action { id, label, .. } => {
                    assert_eq!(id.as_str(), SC_BUTTON_IDS[i]);
                    // Commit carries a label; Push/Pull/Sync are icon-only.
                    if i == 0 {
                        assert_eq!(label, "Commit");
                    } else {
                        assert!(label.is_empty());
                    }
                }
                other => panic!("expected Action, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_sc_button_toolbar_commit_enabled_tracks_message() {
        use quadraui::ToolbarButton;

        let commit_enabled = |sc: &SourceControlData| match &sc_button_toolbar(sc).buttons[0] {
            ToolbarButton::Action { enabled, .. } => *enabled,
            _ => panic!("commit button missing"),
        };

        let mut sc = empty_sc_data();
        assert!(!commit_enabled(&sc), "empty message → Commit disabled");

        sc.commit_message = "   ".into();
        assert!(!commit_enabled(&sc), "whitespace-only message → disabled");

        sc.commit_message = "feat: x".into();
        assert!(commit_enabled(&sc), "non-empty message → Commit enabled");
    }

    #[test]
    fn test_sc_button_id_index_round_trip() {
        use crate::core::engine::Engine;
        for idx in 0..4 {
            let id = Engine::sc_button_id(idx).expect("id for valid index");
            assert_eq!(Engine::sc_button_index(&id), Some(idx));
        }
        assert!(Engine::sc_button_id(4).is_none());
    }

    #[test]
    fn test_sc_button_toolbar_hit_test_resolves_index() {
        use crate::core::engine::Engine;
        use quadraui::ToolbarHit;

        // Lay the toolbar out the way the TUI backend does, then prove a
        // click inside each button's bounds maps back to its index.
        let sc = empty_sc_data();
        let bar = sc_button_toolbar(&sc);
        let area = ratatui::layout::Rect::new(0, 5, 60, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);

        // Push is button index 1 and is enabled (icon-only).
        let push = &layout.visible_items[1];
        let hit = layout.hit_test(push.bounds.x + 0.5, push.bounds.y);
        match hit {
            ToolbarHit::Button(id) => assert_eq!(Engine::sc_button_index(&id), Some(1)),
            other => panic!("expected Button hit, got {other:?}"),
        }
    }

    #[test]
    fn test_sc_button_toolbar_disabled_commit_not_clickable() {
        use quadraui::ToolbarHit;

        // Empty message → Commit disabled → its slot hit-tests as Empty.
        let sc = empty_sc_data();
        let bar = sc_button_toolbar(&sc);
        let area = ratatui::layout::Rect::new(0, 0, 60, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);
        let commit = &layout.visible_items[0];
        assert!(!commit.clickable, "disabled Commit must not be clickable");
        assert_eq!(
            layout.hit_test(commit.bounds.x + 0.5, commit.bounds.y),
            ToolbarHit::Empty
        );
    }

    // ─── SC SidebarPanel tests (#509) ────────────────────────────────────────

    #[test]
    fn test_sc_sidebar_panel_toolbar_slot_reserved_at_top() {
        use quadraui::{primitives::toolbar::ToolbarItemMeasure, SidebarPanelMeasure};

        let sc = empty_sc_data();
        let panel = sc_sidebar_panel(&sc);
        // TUI default: toolbar_height = 1 cell, item_width = 8 cells.
        let area = quadraui::Rect::new(0.0, 5.0, 60.0, 20.0);
        let measure = SidebarPanelMeasure::new(1.0, 8.0);
        let layout = panel.layout(area, measure, |_| ToolbarItemMeasure::new(8.0));

        // Toolbar slot is reserved at the top.
        let tb = layout.toolbar_bounds.expect("toolbar slot reserved");
        assert_eq!(tb.y, 5.0, "toolbar slot starts at panel top");
        assert_eq!(tb.height, 1.0, "TUI default toolbar height = 1 cell");
        // Content starts immediately below toolbar slot (no padding — option a).
        assert_eq!(
            layout.content_bounds.y, 6.0,
            "content starts at toolbar_y + 1"
        );
        assert_eq!(
            layout.content_bounds.height, 19.0,
            "content height = panel_height - 1"
        );
    }

    #[test]
    fn test_sc_sidebar_panel_hit_test_resolves_toolbar_button_and_content() {
        use crate::core::engine::Engine;
        use quadraui::{
            primitives::toolbar::ToolbarItemMeasure, SidebarPanelHit, SidebarPanelMeasure,
        };

        let mut sc = empty_sc_data();
        sc.commit_message = "feat: fix".into(); // non-empty → Commit enabled
        let panel = sc_sidebar_panel(&sc);
        let area = quadraui::Rect::new(0.0, 0.0, 60.0, 10.0);
        let measure = SidebarPanelMeasure::new(1.0, 8.0);
        let layout = panel.layout(area, measure, |_| ToolbarItemMeasure::new(8.0));

        // A hit in the toolbar slot (y=0, which is the toolbar row) on a button.
        let hit = layout.hit_test(0.5, 0.0);
        match hit {
            SidebarPanelHit::ToolbarButton(id) => {
                assert_eq!(
                    Engine::sc_button_index(&id),
                    Some(0),
                    "first button = Commit"
                );
            }
            other => panic!("expected ToolbarButton hit, got {other:?}"),
        }

        // A hit in the content area (y=2, content starts at y=1) returns content-local coords.
        let hit = layout.hit_test(5.0, 2.0);
        match hit {
            SidebarPanelHit::Content { x, y } => {
                assert_eq!(x, 5.0);
                assert_eq!(y, 1.0, "content-local y = abs_y - content_bounds.y = 2-1");
            }
            other => panic!("expected Content hit, got {other:?}"),
        }
    }

    #[test]
    fn test_sc_sidebar_panel_content_bounds_height() {
        use quadraui::{primitives::toolbar::ToolbarItemMeasure, SidebarPanelMeasure};

        let sc = empty_sc_data();
        let panel = sc_sidebar_panel(&sc);
        // Simulate a 15-row panel starting at y=3 (e.g. after 3 header/commit rows).
        let area = quadraui::Rect::new(0.0, 3.0, 40.0, 15.0);
        let measure = SidebarPanelMeasure::new(1.0, 8.0);
        let layout = panel.layout(area, measure, |_| ToolbarItemMeasure::new(8.0));

        assert_eq!(
            layout.content_bounds.height, 14.0,
            "content = panel_height(15) - toolbar_slot(1)"
        );
        assert_eq!(layout.content_bounds.y, 4.0, "content starts at y=3+1=4");
    }

    // ─── Debug toolbar tests (#510) ──────────────────────────────────────────

    /// Helper: return an engine with DAP session state as specified.
    fn engine_with_dap(session_active: bool, stopped_thread: Option<u64>) -> Engine {
        let mut e = Engine::new();
        e.debug_toolbar_visible = true;
        e.dap_session_active = session_active;
        e.dap_stopped_thread = stopped_thread;
        e
    }

    #[test]
    fn debug_toolbar_button_ids_round_trip() {
        use crate::core::engine::{Engine, DEBUG_BUTTON_IDS};
        use quadraui::ToolbarButton;

        let engine = engine_with_dap(true, Some(1u64));
        let bar = debug_toolbar(&engine);

        // 8 entries: 7 action buttons + 1 separator after index 3.
        assert_eq!(bar.buttons.len(), 8);

        let mut action_idx = 0usize;
        for btn in &bar.buttons {
            match btn {
                ToolbarButton::Action { id, .. } => {
                    // id matches DEBUG_BUTTON_IDS[action_idx]
                    assert_eq!(
                        id.as_str(),
                        DEBUG_BUTTON_IDS[action_idx],
                        "button {action_idx} id mismatch"
                    );
                    // round-trip: id → index → same action_idx
                    let idx = Engine::debug_button_index(id).expect("index for valid id");
                    assert_eq!(idx, action_idx);
                    action_idx += 1;
                }
                ToolbarButton::Separator => {
                    // separator sits between button 3 (Restart) and button 4 (Step Over)
                    assert_eq!(action_idx, 4, "separator must come after index 3");
                }
                ToolbarButton::Label { .. } => {
                    panic!("unexpected Label variant in debug toolbar");
                }
            }
        }
        assert_eq!(action_idx, 7, "expected 7 action buttons");
    }

    #[test]
    fn debug_toolbar_disabled_when_no_session() {
        use quadraui::ToolbarButton;

        let engine = engine_with_dap(false, None);
        let bar = debug_toolbar(&engine);
        for btn in &bar.buttons {
            if let ToolbarButton::Action { enabled, label, .. } = btn {
                assert!(
                    !enabled,
                    "button '{label}' should be disabled with no session"
                );
            }
        }
    }

    #[test]
    fn debug_toolbar_steps_disabled_while_running() {
        use quadraui::ToolbarButton;

        // Session active, not stopped (running).
        let engine = engine_with_dap(true, None);
        let bar = debug_toolbar(&engine);

        let get_enabled = |label: &str| {
            bar.buttons.iter().find_map(|b| {
                if let ToolbarButton::Action {
                    enabled, label: l, ..
                } = b
                {
                    if l == label {
                        return Some(*enabled);
                    }
                }
                None
            })
        };

        // Running → Continue/Step* disabled, Pause enabled, Stop/Restart enabled.
        assert_eq!(get_enabled("Continue"), Some(false));
        assert_eq!(get_enabled("Step Over"), Some(false));
        assert_eq!(get_enabled("Step Into"), Some(false));
        assert_eq!(get_enabled("Step Out"), Some(false));
        assert_eq!(get_enabled("Pause"), Some(true));
        assert_eq!(get_enabled("Stop"), Some(true));
        assert_eq!(get_enabled("Restart"), Some(true));
    }

    #[test]
    fn debug_toolbar_steps_enabled_when_stopped() {
        use quadraui::ToolbarButton;

        // Session active, stopped at thread 1.
        let engine = engine_with_dap(true, Some(1u64));
        let bar = debug_toolbar(&engine);

        let get_enabled = |label: &str| {
            bar.buttons.iter().find_map(|b| {
                if let ToolbarButton::Action {
                    enabled, label: l, ..
                } = b
                {
                    if l == label {
                        return Some(*enabled);
                    }
                }
                None
            })
        };

        // Stopped → Continue/Step* enabled, Pause disabled, Stop/Restart enabled.
        assert_eq!(get_enabled("Continue"), Some(true));
        assert_eq!(get_enabled("Step Over"), Some(true));
        assert_eq!(get_enabled("Step Into"), Some(true));
        assert_eq!(get_enabled("Step Out"), Some(true));
        assert_eq!(get_enabled("Pause"), Some(false));
        assert_eq!(get_enabled("Stop"), Some(true));
        assert_eq!(get_enabled("Restart"), Some(true));
    }

    #[test]
    fn debug_toolbar_hit_test_resolves_each_button() {
        use crate::core::engine::Engine;
        use quadraui::ToolbarHit;

        let engine = engine_with_dap(true, Some(1u64));
        let bar = debug_toolbar(&engine);
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);

        // Hit-test each visible_item that is clickable and assert that it
        // resolves back to its expected DEBUG_BUTTON_IDS entry.
        for item in &layout.visible_items {
            if !item.clickable {
                continue;
            }
            let hit = layout.hit_test(item.bounds.x + 0.5, item.bounds.y);
            match hit {
                ToolbarHit::Button(ref id) => {
                    let idx = Engine::debug_button_index(id)
                        .unwrap_or_else(|| panic!("unknown id {:?}", id.as_str()));
                    assert!(idx < 7, "index {idx} out of range");
                }
                ToolbarHit::Empty => {
                    panic!(
                        "clickable item hit_test returned Empty at {:?}",
                        item.bounds
                    );
                }
            }
        }
    }

    #[test]
    fn debug_toolbar_disabled_button_not_clickable() {
        use quadraui::ToolbarHit;

        // No session → all buttons disabled.
        let engine = engine_with_dap(false, None);
        let bar = debug_toolbar(&engine);
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);

        // Every visible_item must be not clickable and hit_test must return Empty.
        for item in &layout.visible_items {
            assert!(
                !item.clickable,
                "disabled button at {:?} should not be clickable",
                item.bounds
            );
            assert_eq!(
                layout.hit_test(item.bounds.x + 0.5, item.bounds.y),
                ToolbarHit::Empty,
                "disabled button hit_test should return Empty"
            );
        }
    }

    #[test]
    fn test_ext_panel_to_tree_view_shape() {
        use crate::core::plugin::{ExtPanelAction, ExtPanelBadge, ExtPanelItem, ExtPanelStyle};
        use quadraui::Decoration;

        let mut item_a = ExtPanelItem {
            text: "Item A".into(),
            id: "a".into(),
            indent: 0,
            style: ExtPanelStyle::Normal,
            expandable: true,
            expanded: true,
            badges: vec![ExtPanelBadge {
                text: "main".into(),
                color: "green".into(),
            }],
            actions: vec![ExtPanelAction {
                label: "Stage".into(),
                key: "s".into(),
            }],
            hint: "h".into(),
            ..Default::default()
        };
        item_a.icon = "\u{f04b}".into();

        let item_b_child = ExtPanelItem {
            text: "Child".into(),
            id: "a_child".into(),
            indent: 1,
            parent_id: "a".into(),
            style: ExtPanelStyle::Accent,
            ..Default::default()
        };

        let item_c_dim = ExtPanelItem {
            text: "Dim".into(),
            id: "c".into(),
            style: ExtPanelStyle::Dim,
            ..Default::default()
        };

        let item_sep = ExtPanelItem {
            is_separator: true,
            ..Default::default()
        };

        let panel = ExtPanelData {
            name: "my_ext".into(),
            title: "MY EXT".into(),
            sections: vec![
                ExtPanelSectionData {
                    name: "Open".into(),
                    items: vec![item_a, item_b_child, item_sep, item_c_dim],
                    expanded: true,
                },
                ExtPanelSectionData {
                    name: "Closed".into(),
                    items: vec![ExtPanelItem {
                        text: "Hidden".into(),
                        ..Default::default()
                    }],
                    expanded: false,
                },
            ],
            // Select the second visible item (`Child`, flat idx 2: header=0, item_a=1, child=2).
            selected: 2,
            has_focus: true,
            scroll_top: 0,
            input_text: String::new(),
            input_active: false,
            help_open: false,
            help_bindings: vec![],
        };

        let theme = Theme::onedark();
        let tv = ext_panel_to_tree_view(&panel, &theme);

        // Expect rows: [0]=Open header, [0,0]=Item A, [0,1]=Child, [0,2]=separator,
        // [0,3]=Dim, [1]=Closed header (collapsed → no children).
        assert_eq!(tv.rows.len(), 6, "rows: {:?}", tv.rows.len());
        assert_eq!(tv.rows[0].path, vec![0]);
        assert_eq!(tv.rows[0].decoration, Decoration::Header);
        assert_eq!(tv.rows[0].is_expanded, Some(true));
        assert_eq!(tv.rows[1].path, vec![0, 0]);
        assert_eq!(tv.rows[1].indent, 1);
        assert_eq!(tv.rows[1].is_expanded, Some(true)); // expandable item
        assert!(
            tv.rows[1].badge.is_some(),
            "badges + action + hint combined"
        );
        assert!(tv.rows[1].icon.is_some(), "icon converted");
        assert_eq!(tv.rows[2].path, vec![0, 1]);
        assert_eq!(tv.rows[2].indent, 2); // indent 1 + 1
        assert_eq!(tv.rows[2].is_expanded, None); // not expandable
                                                  // Separator is muted line glyph.
        assert_eq!(tv.rows[3].decoration, Decoration::Muted);
        assert_eq!(tv.rows[3].text.spans[0].text, "\u{2500}");
        // Dim item maps to Muted.
        assert_eq!(tv.rows[4].decoration, Decoration::Muted);
        // Collapsed section: header only, no children.
        assert_eq!(tv.rows[5].path, vec![1]);
        assert_eq!(tv.rows[5].is_expanded, Some(false));

        // Selection: flat idx 2 = Child → path [0, 1].
        assert_eq!(tv.selected_path, Some(vec![0, 1]));
        assert_eq!(tv.scroll_offset, 0);
        assert!(tv.has_focus);
    }

    #[test]
    fn test_ext_panel_to_tree_view_no_focus_no_selection() {
        let panel = ExtPanelData {
            name: "x".into(),
            title: "X".into(),
            sections: vec![ExtPanelSectionData {
                name: "S".into(),
                items: vec![],
                expanded: true,
            }],
            selected: 0,
            has_focus: false,
            scroll_top: 5,
            input_text: String::new(),
            input_active: false,
            help_open: false,
            help_bindings: vec![],
        };
        let tv = ext_panel_to_tree_view(&panel, &Theme::onedark());
        assert_eq!(tv.selected_path, None);
        assert_eq!(tv.scroll_offset, 5);
        assert!(!tv.has_focus);
    }

    #[test]
    fn test_try_from_hex() {
        assert_eq!(
            Color::try_from_hex("#ff0000"),
            Some(Color::from_rgb(255, 0, 0))
        );
        assert_eq!(
            Color::try_from_hex("00ff00"),
            Some(Color::from_rgb(0, 255, 0))
        );
        assert_eq!(
            Color::try_from_hex("#abc"),
            Some(Color::from_rgb(0xaa, 0xbb, 0xcc))
        );
        // 8-digit hex (alpha discarded)
        assert_eq!(
            Color::try_from_hex("#ff000080"),
            Some(Color::from_rgb(255, 0, 0))
        );
        assert_eq!(Color::try_from_hex("xyz"), None);
        assert_eq!(Color::try_from_hex(""), None);
    }

    #[test]
    fn test_strip_json_comments() {
        let input = r#"{
  // line comment
  "key": "value", /* block */
  "str": "has // no comment"
}"#;
        let stripped = strip_json_comments(input);
        let val: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(val["key"], "value");
        assert_eq!(val["str"], "has // no comment");
    }

    #[test]
    fn test_lighten_darken() {
        let c = Color::from_rgb(100, 100, 100);
        let lighter = c.lighten(0.5);
        assert!(lighter.r > 100 && lighter.r < 255);
        let darker = c.darken(0.5);
        assert!(darker.r < 100 && darker.r > 0);
        // Extremes
        assert_eq!(c.lighten(1.0), Color::from_rgb(255, 255, 255));
        assert_eq!(c.darken(1.0), Color::from_rgb(0, 0, 0));
    }

    #[test]
    fn test_from_vscode_json() {
        let dir = std::env::temp_dir().join("vimcode_test_theme");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-theme.json");
        std::fs::write(
            &path,
            r##"{
            // Test VSCode theme
            "name": "Test Theme",
            "colors": {
                "editor.background": "#1e1e2e",
                "editor.foreground": "#cdd6f4",
                "editorCursor.foreground": "#f5e0dc",
                "editor.selectionBackground": "#585b7066",
                "editorLineNumber.foreground": "#6c7086",
                "statusBar.background": "#181825",
                "statusBar.foreground": "#cdd6f4"
            },
            "tokenColors": [
                {
                    "scope": ["keyword", "keyword.control"],
                    "settings": { "foreground": "#cba6f7" }
                },
                {
                    "scope": "string",
                    "settings": { "foreground": "#a6e3a1" }
                },
                {
                    "scope": "comment",
                    "settings": { "foreground": "#6c7086" }
                }
            ]
        }"##,
        )
        .unwrap();

        let theme = Theme::from_vscode_json(&path).unwrap();
        assert_eq!(theme.background, Color::try_from_hex("#1e1e2e").unwrap());
        assert_eq!(theme.foreground, Color::try_from_hex("#cdd6f4").unwrap());
        assert_eq!(theme.cursor, Color::try_from_hex("#f5e0dc").unwrap());
        assert_eq!(theme.keyword, Color::try_from_hex("#cba6f7").unwrap());
        assert_eq!(theme.string_lit, Color::try_from_hex("#a6e3a1").unwrap());
        assert_eq!(theme.comment, Color::try_from_hex("#6c7086").unwrap());
        assert_eq!(theme.status_bg, Color::try_from_hex("#181825").unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_button_label() {
        assert_eq!(super::format_button_label("Recover", 'r'), "[R]ecover");
        assert_eq!(
            super::format_button_label("Delete swap", 'd'),
            "[D]elete swap"
        );
        assert_eq!(super::format_button_label("Abort", 'a'), "[A]bort");
        assert_eq!(super::format_button_label("OK", 'o'), "[O]K");
        // Hotkey not in label → prepended.
        assert_eq!(super::format_button_label("Yes", 'z'), "[Z] Yes");
    }

    #[test]
    fn test_diff_toolbar_on_both_group_tab_bars() {
        use crate::core::engine::{Engine, OpenMode};
        use crate::core::window::SplitDirection;

        let dir = std::env::temp_dir().join("vimcode_render_diff_groups");
        std::fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "same\nold\nsame\n").unwrap();
        std::fs::write(&f2, "same\nnew\nsame\n").unwrap();

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&f1, OpenMode::Permanent)
            .unwrap();
        engine.execute_command("diffthis");

        // Create a second editor group and open the second file.
        engine.open_editor_group(SplitDirection::Vertical);
        engine
            .open_file_with_mode(&f2, OpenMode::Permanent)
            .unwrap();
        engine.execute_command("diffthis");
        assert!(engine.is_in_diff_view());

        // Build window rects for both groups.
        let content_bounds = WindowRect::new(0.0, 1.0, 80.0, 24.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, 1.0);
        let theme = Theme::onedark();
        let layout = build_screen_layout(&engine, &theme, &rects, 1.0, 1.0, false);

        // Both group tab bars should have diff_toolbar populated.
        assert!(
            layout.editor_group_split.is_some(),
            "should have editor group split"
        );
        assert!(
            layout.group_tab_bars.len() >= 2,
            "should have 2+ group tab bars"
        );
        for gtb in &layout.group_tab_bars {
            assert!(
                gtb.diff_toolbar.is_some(),
                "group {:?} should have diff toolbar, but it's None",
                gtb.group_id
            );
        }
    }

    #[test]
    fn test_spell_errors_in_rendered_lines() {
        use crate::core::Engine;

        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "the quik brown fox\n");
        engine.settings.spell = true;
        engine.ensure_spell_checker();

        let rects = vec![(
            engine.active_window_id(),
            WindowRect::new(0.0, 0.0, 80.0, 24.0),
        )];
        let theme = Theme::onedark();
        let layout = build_screen_layout(&engine, &theme, &rects, 1.0, 1.0, false);

        // The first window's first line should have a spell error on "quik".
        let window = &layout.windows[0];
        let first_line = &window.lines[0];
        assert!(
            !first_line.spell_errors.is_empty(),
            "expected spell errors on 'the quik brown fox', got none"
        );
        assert_eq!(first_line.spell_errors[0].start_col, 4);
        assert_eq!(first_line.spell_errors[0].end_col, 8);
    }

    // ── Per-window status line tests ─────────────────────────────────────────

    #[test]
    fn test_window_status_line_active() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello world\nsecond line\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // Active window should have a mode badge as the first left segment
        assert!(!status.left_segments.is_empty());
        assert!(
            status.left_segments[0].text.contains("NORMAL"),
            "expected NORMAL mode badge, got '{}'",
            status.left_segments[0].text
        );
        assert!(status.left_segments[0].bold);

        // Should have right segments with cursor position
        assert!(!status.right_segments.is_empty());
        let right_text: String = status
            .right_segments
            .iter()
            .map(|s| s.text.clone())
            .collect();
        assert!(
            right_text.contains("Ln 1"),
            "expected cursor position, got '{}'",
            right_text
        );
    }

    #[test]
    fn test_window_status_line_inactive() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, false);

        // Inactive should NOT have mode badge
        assert!(!status.left_segments.is_empty());
        assert!(
            !status.left_segments[0].text.contains("NORMAL"),
            "inactive status should not contain mode badge"
        );
        // All segments should use inactive colors
        for seg in &status.left_segments {
            assert_eq!(seg.fg, theme.status_inactive_fg);
        }
    }

    #[test]
    fn test_window_status_line_dirty_indicator() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "text\n");
        engine
            .buffer_manager
            .get_mut(engine.active_buffer_id())
            .unwrap()
            .dirty = true;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        let left_text: String = status
            .left_segments
            .iter()
            .map(|s| s.text.clone())
            .collect();
        assert!(
            left_text.contains("[+]"),
            "expected dirty indicator, got '{}'",
            left_text
        );
    }

    #[test]
    fn test_window_status_line_insert_mode() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.mode = crate::core::Mode::Insert;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        assert!(status.left_segments[0].text.contains("INSERT"));
        // Mode color used as text tint, not background
        assert_eq!(status.left_segments[0].fg, theme.status_mode_insert_bg);
        // Background is derived from theme.background.lighten(0.10)
        assert_eq!(status.left_segments[0].bg, theme.background.lighten(0.10));
    }

    #[test]
    fn test_build_screen_layout_per_window_status() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine
            .buffer_mut()
            .insert(0, "line 1\nline 2\nline 3\nline 4\nline 5\n");

        let wid = engine.active_window_id();
        let rects = vec![(wid, WindowRect::new(0.0, 0.0, 80.0, 24.0))];
        let theme = Theme::onedark();
        let layout = build_screen_layout(&engine, &theme, &rects, 1.0, 1.0, false);

        // Each window should have a status_line
        assert!(layout.windows[0].status_line.is_some());

        // visible_lines should be rect height - 1 (status bar takes 1 row)
        assert_eq!(
            layout.windows[0].lines.len(),
            5, // only 5 lines of content, less than 23 visible lines
            "lines should contain the buffer's actual lines"
        );

        // Global status bar should be None when per-window is on
        assert!(layout.global_status_bar.is_none());
    }

    #[test]
    fn test_build_screen_layout_no_per_window_status() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let mut engine = Engine::new();
        engine.settings.window_status_line = false;
        engine.buffer_mut().insert(0, "hello\n");

        let wid = engine.active_window_id();
        let rects = vec![(wid, WindowRect::new(0.0, 0.0, 80.0, 24.0))];
        let theme = Theme::onedark();
        let layout = build_screen_layout(&engine, &theme, &rects, 1.0, 1.0, false);

        // No per-window status line
        assert!(layout.windows[0].status_line.is_none());

        // Global status bar should be populated
        assert!(layout.global_status_bar.is_some());
    }

    #[test]
    fn test_status_segments_have_actions() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // Right segments should include GoToLine on cursor position
        let goto = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::GoToLine));
        assert!(goto.is_some(), "expected GoToLine action on Ln/Col segment");

        // Right segments should include ChangeIndentation
        let indent = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeIndentation));
        assert!(
            indent.is_some(),
            "expected ChangeIndentation action on indent segment"
        );

        // Right segments should include ChangeEncoding
        let enc = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeEncoding));
        assert!(enc.is_some(), "expected ChangeEncoding action");

        // Right segments should include ChangeLineEnding
        let eol = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeLineEnding));
        assert!(eol.is_some(), "expected ChangeLineEnding action");

        // Inactive window segments should have no actions
        let inactive = build_window_status_line(&engine, &theme, wid, false);
        for seg in inactive
            .left_segments
            .iter()
            .chain(inactive.right_segments.iter())
        {
            assert_eq!(seg.action, None, "inactive segments should have no actions");
        }
    }

    #[test]
    fn test_status_line_ending_segment() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        // Default is LF
        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);
        let eol_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeLineEnding))
            .expect("expected line ending segment");
        assert!(
            eol_seg.text.contains("LF"),
            "expected LF, got '{}'",
            eol_seg.text
        );
    }

    #[test]
    fn test_status_indentation_segment() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.settings.expand_tab = true;
        engine.settings.tabstop = 4;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);
        let indent_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeIndentation))
            .expect("expected indent segment");
        assert!(
            indent_seg.text.contains("Spaces: 4"),
            "expected 'Spaces: 4', got '{}'",
            indent_seg.text
        );
    }

    #[test]
    fn test_line_ending_detection() {
        use crate::core::buffer_manager::LineEnding;
        assert_eq!(LineEnding::detect("hello\nworld\n"), LineEnding::LF);
        assert_eq!(LineEnding::detect("hello\r\nworld\r\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("no newline"), LineEnding::LF);
        assert_eq!(LineEnding::detect(""), LineEnding::LF);
    }

    // ─── #221: LSP progress segment formatter ───────────────────────
    #[test]
    fn test_lsp_progress_segment_no_progress() {
        // Pre-#221 behaviour: no progress data → dimmed `name… `.
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", None),
            "rust-analyzer… "
        );
    }

    #[test]
    fn test_lsp_progress_segment_prefers_percentage() {
        // VSCode-style with percentage available: detail is the
        // fixed-width `42%`, not the verbose message string.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Indexing".to_string(),
            message: Some("319/320".to_string()),
            percentage: Some(99),
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Indexing: 99% "
        );
    }

    #[test]
    fn test_lsp_progress_segment_falls_back_to_percentage() {
        // No message string → use percentage as detail.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Indexing".to_string(),
            message: None,
            percentage: Some(42),
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Indexing: 42% "
        );
    }

    #[test]
    fn test_lsp_progress_segment_title_only() {
        // begin with just a title and nothing else: show stage with `…`.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Fetching".to_string(),
            message: None,
            percentage: None,
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Fetching… "
        );
    }

    #[test]
    fn test_lsp_progress_segment_extracts_xy_count_from_path_message() {
        // rust-analyzer's "Roots Scanned" messages embed the full path
        // (e.g. "34/285: /home/john/.cargo/registry/…"). When no
        // percentage is provided, surface the leading `34/285` so the
        // user still sees concrete progress without the path noise.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Roots Scanned".to_string(),
            message: Some(
                "34/285: /home/john/.cargo/registry/src/index.crates.io-1949cf8c/gio-0.18.4"
                    .to_string(),
            ),
            percentage: None,
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Roots Scanned: 34/285 "
        );
    }

    #[test]
    fn test_lsp_progress_segment_drops_unbounded_message() {
        // Free-text messages without a percentage or X/Y prefix
        // (e.g. "cargo metadata: Blocking …") would balloon the segment
        // width and trigger fit-or-drop flicker — we drop the message
        // text and fall back to `stage…`.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Fetching".to_string(),
            message: Some(
                "cargo metadata: Blocking waiting for file lock on package cache".to_string(),
            ),
            percentage: None,
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Fetching… "
        );
    }

    #[test]
    fn test_lsp_progress_segment_empty_title_uses_working() {
        // Defensive: some servers begin without a title — show "working"
        // rather than a blank stage label.
        let progress = crate::core::lsp_manager::LspProgress {
            title: String::new(),
            message: None,
            percentage: Some(50),
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • working: 50% "
        );
    }

    #[test]
    fn test_lsp_progress_segment_width_bound() {
        // Width discipline: the formatted segment must stay ≤ 32 cells
        // for the longest plausible title + percentage combo, to prevent
        // the status bar's priority-drop from flapping during streaming
        // $/progress reports.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Building compile-time-deps".to_string(),
            message: None,
            percentage: Some(100),
        };
        let s = format_lsp_progress_segment("rust-analyzer", Some(&progress));
        // Width covers `rust-analyzer • Building compile-time-deps: 100% ` ≈ 51 chars.
        // Longest realistic title in rust-analyzer's vocabulary —
        // shorter labels (e.g. "Indexing", "Fetching") stay well under.
        assert!(s.chars().count() < 60, "segment too long: {s:?}");
    }

    #[test]
    fn test_lsp_status_no_manager() {
        use crate::core::engine::Engine;
        // Engine::new() has no lsp_manager — LSP segment should not appear
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // No LSP segment when no manager is running
        let lsp_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::LspInfo));
        assert!(
            lsp_seg.is_none(),
            "should not show LSP segment without lsp_manager"
        );
    }

    // ─── Shared layout helper tests ─────────────────────────────────────────

    #[test]
    fn test_tab_bar_height_px() {
        let lh = 20.0;
        let no_bc = tab_bar_height_px(lh, false);
        let with_bc = tab_bar_height_px(lh, true);
        assert_eq!(no_bc, (lh * 1.6).ceil());
        assert_eq!(with_bc, (lh * 1.6).ceil() + lh);
    }

    #[test]
    fn test_status_bar_height_px() {
        let lh = 16.0;
        // per-window status → 1 global row
        assert_eq!(status_bar_height_px(lh, true, false), lh);
        // no per-window → 2 global rows
        assert_eq!(status_bar_height_px(lh, false, false), 2.0 * lh);
        // with wildmenu adds one line_height
        assert_eq!(status_bar_height_px(lh, true, true), 2.0 * lh);
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

    // ─── ScreenLayout rendering tests ────────────────────────────────────

    /// Build a ScreenLayout for an engine with the given content at the given
    /// terminal dimensions (in character cells).
    fn render_engine(engine: &Engine, width: f64, height: f64) -> ScreenLayout {
        let bounds = WindowRect::new(0.0, 0.0, width, height);
        let (rects, _) = engine.calculate_group_window_rects(bounds, 1.0);
        let theme = Theme::onedark();
        build_screen_layout(engine, &theme, &rects, 1.0, 1.0, true)
    }

    fn test_engine(text: &str) -> Engine {
        crate::core::session::suppress_disk_saves();
        // `Engine::new_for_test()` builds settings/session/history/git_branch
        // from in-memory defaults instead of loading ambient disk/git state
        // (#615, #439, #617) — see its doc comment for why call-then-overwrite
        // on `Engine::new()` doesn't reliably undo `app_shell.hide_sidebar()`,
        // and why leaving `git_branch` unset here matters: it feeds the
        // right-hand status segments rendered by this file's tests.
        let mut e = Engine::new_for_test();
        e.mode = Mode::Normal;
        if !text.is_empty() {
            e.buffer_mut().insert(0, text);
        }
        e
    }

    #[test]
    fn test_settings_to_form_read_only_by_default() {
        let e = test_engine("");
        let idx = crate::core::settings::SETTING_DEFS
            .iter()
            .position(|d| d.key == "font_family")
            .expect("font_family setting exists");
        let form = settings_to_form(&e);
        let field = form
            .fields
            .iter()
            .find(|f| f.id == quadraui::WidgetId::new(format!("setting-{idx}")))
            .expect("font_family field present");
        assert!(
            matches!(field.kind, quadraui::FieldKind::ReadOnly { .. }),
            "non-edited StringVal setting should render ReadOnly, got {:?}",
            field.kind
        );
    }

    #[test]
    fn test_settings_to_form_inline_edit_emits_text_input_with_cursor() {
        let mut e = test_engine("");
        let idx = crate::core::settings::SETTING_DEFS
            .iter()
            .position(|d| d.key == "font_family")
            .expect("font_family setting exists");
        e.settings_editing = Some(idx);
        e.settings_edit_buf = "Fira Code".to_string();

        let form = settings_to_form(&e);
        let field = form
            .fields
            .iter()
            .find(|f| f.id == quadraui::WidgetId::new(format!("setting-{idx}")))
            .expect("font_family field present");
        match &field.kind {
            quadraui::FieldKind::TextInput { value, cursor, .. } => {
                assert_eq!(value, "Fira Code");
                assert_eq!(*cursor, Some("Fira Code".len()));
            }
            other => panic!("expected TextInput while editing, got {other:?}"),
        }
    }

    #[test]
    fn test_settings_to_form_ext_setting_inline_edit_emits_text_input() {
        use crate::core::extensions::{ExtSettingDef, ExtensionManifest};
        use crate::core::session::InstalledExtension;

        let mut e = test_engine("");
        e.ext_registry = Some(vec![ExtensionManifest {
            name: "myext".to_string(),
            display_name: "My Ext".to_string(),
            settings: vec![ExtSettingDef {
                key: "greeting".to_string(),
                label: "Greeting".to_string(),
                r#type: "string".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }]);
        e.extension_state.installed.push(InstalledExtension {
            name: "myext".to_string(),
            version: String::new(),
        });
        e.ext_settings_editing = Some(("myext".to_string(), "greeting".to_string()));
        e.settings_edit_buf = "hi".to_string();

        let form = settings_to_form(&e);
        let field = form
            .fields
            .iter()
            .find(|f| f.id == quadraui::WidgetId::new("ext-setting-myext-greeting"))
            .expect("ext setting field present");
        match &field.kind {
            quadraui::FieldKind::TextInput { value, cursor, .. } => {
                assert_eq!(value, "hi");
                assert_eq!(*cursor, Some("hi".len()));
            }
            other => panic!("expected TextInput while editing ext setting, got {other:?}"),
        }
    }

    #[test]
    fn test_screen_layout_basic_structure() {
        let e = test_engine("Hello, world!\nSecond line\nThird line\n");
        let layout = render_engine(&e, 80.0, 24.0);

        // Should have exactly one window
        assert_eq!(layout.windows.len(), 1, "single buffer = single window");

        // Window should contain rendered lines
        let win = &layout.windows[0];
        assert!(
            win.lines.len() >= 3,
            "should render at least 3 content lines"
        );
        assert!(win.is_active);
        assert!(win.cursor.is_some(), "cursor should be visible");

        // First line content
        assert_eq!(win.lines[0].raw_text.trim_end(), "Hello, world!");

        // Tab bar should have one tab
        assert!(!layout.tab_bar.is_empty());
        assert!(layout.tab_bar[0].active);
    }

    #[test]
    fn test_screen_layout_cursor_position() {
        let mut e = test_engine("abcdef\nghijkl\n");
        // Move cursor to line 1, col 3
        e.handle_key("j", Some('j'), false);
        e.handle_key("l", Some('l'), false);
        e.handle_key("l", Some('l'), false);
        e.handle_key("l", Some('l'), false);
        let layout = render_engine(&e, 80.0, 24.0);

        let win = &layout.windows[0];
        let (cursor_pos, _shape) = win.cursor.unwrap();
        assert_eq!(cursor_pos.view_line, 1, "cursor on second line");
        assert_eq!(cursor_pos.col, 3, "cursor at col 3");
    }

    #[test]
    fn test_screen_layout_split_windows() {
        let mut e = test_engine("file one\n");
        // Open a vertical split
        e.open_editor_group(SplitDirection::Vertical);

        let layout = render_engine(&e, 80.0, 24.0);
        assert_eq!(layout.windows.len(), 2, "vsplit should produce two windows");

        // Windows should divide the horizontal space
        let w0 = &layout.windows[0];
        let w1 = &layout.windows[1];
        assert!(w0.rect.width > 0.0);
        assert!(w1.rect.width > 0.0);
        assert!(
            (w0.rect.width + w1.rect.width - 80.0).abs() < 2.0,
            "widths should approximately sum to terminal width"
        );
    }

    #[test]
    fn test_screen_layout_terminal_open() {
        let mut e = test_engine("content\n");
        e.terminal_open = true;
        e.session.terminal_panel_rows = 10;

        let layout = render_engine(&e, 80.0, 24.0);

        // Bottom panel active tab should reflect terminal
        assert_eq!(
            layout.bottom_tabs.active,
            BottomPanelKind::Terminal,
            "bottom panel should show terminal tab"
        );

        // Editor window height should be reduced (less than full 24 rows)
        let win = &layout.windows[0];
        assert!(
            win.rect.height < 24.0,
            "editor should be shorter when terminal is open"
        );
    }

    #[test]
    fn test_screen_layout_visual_selection() {
        let mut e = test_engine("select this text\n");
        // Enter visual mode and select 5 chars
        e.handle_key("v", Some('v'), false);
        for _ in 0..4 {
            e.handle_key("l", Some('l'), false);
        }

        let layout = render_engine(&e, 80.0, 24.0);
        let win = &layout.windows[0];
        assert!(
            win.selection.is_some(),
            "visual mode should produce a selection range"
        );
    }

    #[test]
    fn test_screen_layout_command_line() {
        let mut e = test_engine("hello\n");
        // Enter command mode
        e.handle_key(":", Some(':'), false);
        e.handle_key("w", Some('w'), false);

        let layout = render_engine(&e, 80.0, 24.0);
        assert!(
            layout.command.text.contains(":w"),
            "command line should show ':w', got: {:?}",
            layout.command.text
        );
        assert!(layout.command.show_cursor);
    }

    #[test]
    fn test_screen_layout_dirty_tab() {
        let mut e = test_engine("hello\n");
        // Make a change to dirty the buffer
        e.handle_key("i", Some('i'), false);
        e.handle_key("x", Some('x'), false);
        e.handle_key("Escape", None, false);

        let layout = render_engine(&e, 80.0, 24.0);
        assert!(
            layout.tab_bar[0].dirty,
            "modified buffer should show dirty tab"
        );
    }

    #[test]
    fn test_screen_layout_line_numbers() {
        let mut e = test_engine("line1\nline2\nline3\nline4\nline5\n");
        e.settings.line_numbers = LineNumberMode::Absolute;
        let layout = render_engine(&e, 80.0, 24.0);

        let win = &layout.windows[0];
        assert!(
            win.gutter_char_width > 0,
            "line numbers should produce a gutter"
        );
        // Gutter text should have line numbers
        assert!(win.lines[0].gutter_text.contains('1'));
        assert!(win.lines[1].gutter_text.contains('2'));
    }

    #[test]
    fn test_screen_layout_status_segments() {
        let e = test_engine("hello\n");
        let layout = render_engine(&e, 80.0, 24.0);

        // Per-window status lines should have segments
        let win = &layout.windows[0];
        if let Some(ref status) = win.status_line {
            assert!(
                !status.left_segments.is_empty(),
                "status should have left segments"
            );
            assert!(
                !status.right_segments.is_empty(),
                "status should have right segments"
            );

            // Mode should be shown
            let mode_text: String = status
                .left_segments
                .iter()
                .map(|s| s.text.as_str())
                .collect();
            assert!(
                mode_text.contains("NORMAL") || mode_text.contains("NOR"),
                "status should show normal mode, got: {mode_text}"
            );
        }
    }

    // ─── Backend Parity Tests ────────────────────────────────────────────────

    /// Helper: compute the set difference (elements in `expected` but not in `actual`).
    fn missing_elements(expected: &[UiElement], actual: &[UiElement]) -> Vec<UiElement> {
        let actual_set: std::collections::HashSet<_> = actual.iter().collect();
        expected
            .iter()
            .filter(|e| !actual_set.contains(e))
            .cloned()
            .collect()
    }

    #[test]
    fn test_parity_basic_layout_tui() {
        let e = test_engine("Hello\nWorld\n");
        let layout = render_engine(&e, 80.0, 24.0);

        let expected = collect_expected_ui_elements(&layout);
        let tui = collect_ui_elements_tui(&layout);
        let missing = missing_elements(&expected, &tui);
        assert!(
            missing.is_empty(),
            "TUI missing elements: {missing:?}\n  expected: {expected:?}\n  got: {tui:?}"
        );
    }

    #[test]
    fn test_parity_basic_layout_wingui() {
        let e = test_engine("Hello\nWorld\n");
        let layout = render_engine(&e, 80.0, 24.0);

        let expected = collect_expected_ui_elements(&layout);
        let wingui = collect_ui_elements_wingui(&layout);
        let missing = missing_elements(&expected, &wingui);
        assert!(
            missing.is_empty(),
            "Win-GUI missing elements: {missing:?}\n  expected: {expected:?}\n  got: {wingui:?}"
        );
    }

    #[test]
    fn test_parity_with_completion_popup() {
        let mut e = test_engine("fn main() {\n    let x = 1;\n}\n");
        // Simulate an active completion menu
        e.completion_candidates = vec!["println".to_string(), "print".to_string()];
        e.completion_idx = Some(0);
        e.completion_start_col = 0;
        let layout = render_engine(&e, 80.0, 24.0);
        // The completion popup should be present
        assert!(layout.completion.is_some(), "completion should be active");

        let expected = collect_expected_ui_elements(&layout);
        for (name, collector) in [
            (
                "TUI",
                collect_ui_elements_tui as fn(&ScreenLayout) -> Vec<UiElement>,
            ),
            ("Win-GUI", collect_ui_elements_wingui),
        ] {
            let actual = collector(&layout);
            let missing = missing_elements(&expected, &actual);
            assert!(
                missing.is_empty(),
                "{name} missing elements with completion: {missing:?}"
            );
        }
    }

    #[test]
    fn test_parity_with_dialog() {
        use crate::core::engine::DialogButton;
        let mut e = test_engine("test content\n");
        e.show_dialog(
            "test_dialog",
            "Confirm",
            vec!["Are you sure?".to_string()],
            vec![
                DialogButton {
                    label: "Yes".into(),
                    hotkey: 'y',
                    action: "yes".into(),
                },
                DialogButton {
                    label: "No".into(),
                    hotkey: 'n',
                    action: "no".into(),
                },
            ],
        );
        let layout = render_engine(&e, 80.0, 24.0);
        assert!(layout.dialog.is_some(), "dialog should be active");

        let expected = collect_expected_ui_elements(&layout);
        for (name, collector) in [
            (
                "TUI",
                collect_ui_elements_tui as fn(&ScreenLayout) -> Vec<UiElement>,
            ),
            ("Win-GUI", collect_ui_elements_wingui),
        ] {
            let actual = collector(&layout);
            let missing = missing_elements(&expected, &actual);
            assert!(
                missing.is_empty(),
                "{name} missing elements with dialog: {missing:?}"
            );
        }
    }

    #[test]
    fn test_parity_with_menu_bar() {
        let mut e = test_engine("hello\n");
        e.menu_bar_visible = true;
        let layout = render_engine(&e, 80.0, 24.0);
        assert!(layout.menu_bar_visible, "menu bar should be visible");

        let expected = collect_expected_ui_elements(&layout);
        for (name, collector) in [
            (
                "TUI",
                collect_ui_elements_tui as fn(&ScreenLayout) -> Vec<UiElement>,
            ),
            ("Win-GUI", collect_ui_elements_wingui),
        ] {
            let actual = collector(&layout);
            let missing = missing_elements(&expected, &actual);
            assert!(
                missing.is_empty(),
                "{name} missing elements with menu bar: {missing:?}"
            );
        }
    }

    #[test]
    fn test_parity_wingui_no_known_gaps() {
        // All previously-known Win-GUI gaps have been fixed.  This test
        // verifies that no regressions have been introduced.
        let mut e = test_engine("hello world\n");
        e.menu_bar_visible = true;
        e.debug_toolbar_visible = true;
        e.dap_session_active = true;
        let layout = render_engine(&e, 80.0, 24.0);

        let expected = collect_expected_ui_elements(&layout);
        let wingui = collect_ui_elements_wingui(&layout);
        let missing = missing_elements(&expected, &wingui);
        assert!(
            missing.is_empty(),
            "Win-GUI missing elements (regressions): {missing:?}"
        );
    }

    #[test]
    fn test_parity_all_elements_covered_by_expected() {
        // Verify that collect_expected_ui_elements produces at least the
        // baseline set of elements for a simple layout.
        let e = test_engine("line1\nline2\n");
        let layout = render_engine(&e, 80.0, 24.0);
        let expected = collect_expected_ui_elements(&layout);

        // Must always have: tab bar, at least one window, command line, activity bar
        assert!(expected.contains(&UiElement::TabBar));
        assert!(expected.contains(&UiElement::EditorWindow { window_idx: 0 }));
        assert!(expected.contains(&UiElement::CommandLine));
        assert!(expected.contains(&UiElement::ActivityBar));
    }

    // ── Phase 2c: Action / click-handler parity tests ───────────────────

    #[test]
    fn test_action_parity_tui_covers_all_required() {
        let required = all_required_ui_actions();
        let tui = collect_ui_actions_tui();
        let missing: Vec<_> = required.iter().filter(|a| !tui.contains(a)).collect();
        assert!(
            missing.is_empty(),
            "TUI missing required actions: {missing:?}"
        );
    }

    #[test]
    fn test_action_parity_wingui_covers_all_required() {
        let required = all_required_ui_actions();
        let wingui = collect_ui_actions_wingui();
        let missing: Vec<_> = required.iter().filter(|a| !wingui.contains(a)).collect();
        assert!(
            missing.is_empty(),
            "Win-GUI missing required actions: {missing:?}"
        );
    }

    #[test]
    fn test_action_parity_wingui_matches_tui() {
        let tui = collect_ui_actions_tui();
        let wingui = collect_ui_actions_wingui();
        let tui_only: Vec<_> = tui.iter().filter(|a| !wingui.contains(a)).collect();
        let wingui_only: Vec<_> = wingui.iter().filter(|a| !tui.contains(a)).collect();
        assert!(
            tui_only.is_empty() && wingui_only.is_empty(),
            "Action parity mismatch:\n  TUI-only: {tui_only:?}\n  Win-GUI-only: {wingui_only:?}"
        );
    }

    /// Test that `open_file_preview` reuses/creates a preview tab, NOT replacing
    /// a permanent buffer. This is the contract for explorer single-click.
    #[test]
    fn test_open_file_preview_does_not_replace_permanent() {
        let mut e = test_engine("first file\n");
        let dir = std::env::temp_dir().join("vimcode_test_preview");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "file A\n").unwrap();
        std::fs::write(&f2, "file B\n").unwrap();

        // Open f1 permanently (simulates existing tab)
        e.open_file_in_tab(&f1);
        let buf_a = e.active_buffer_id();
        assert_eq!(e.active_group().tabs.len(), 2); // scratch + f1

        // Preview f2 (simulates explorer single-click)
        e.open_file_preview(&f2);
        let buf_b = e.active_buffer_id();
        assert_ne!(buf_a, buf_b, "Preview should show different buffer");
        assert_eq!(
            e.active_group().tabs.len(),
            3,
            "Preview should create a new tab, not replace"
        );

        // Preview another file — should reuse the preview tab
        let f3 = dir.join("c.txt");
        std::fs::write(&f3, "file C\n").unwrap();
        e.open_file_preview(&f3);
        assert_eq!(
            e.active_group().tabs.len(),
            3,
            "Second preview should reuse the preview tab"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Test that `open_file_in_tab` always creates a new tab (or switches to
    /// existing). This is the contract for explorer double-click / Enter.
    #[test]
    fn test_open_file_in_tab_creates_new_tab() {
        let mut e = test_engine("scratch\n");
        let dir = std::env::temp_dir().join("vimcode_test_tab");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "file A\n").unwrap();
        std::fs::write(&f2, "file B\n").unwrap();

        let initial_tabs = e.active_group().tabs.len();
        e.open_file_in_tab(&f1);
        assert_eq!(e.active_group().tabs.len(), initial_tabs + 1);
        e.open_file_in_tab(&f2);
        assert_eq!(e.active_group().tabs.len(), initial_tabs + 2);

        // Opening f1 again should switch to existing tab, not create another
        e.open_file_in_tab(&f1);
        assert_eq!(e.active_group().tabs.len(), initial_tabs + 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Test that the default shell is platform-appropriate.
    #[test]
    fn test_default_shell_platform() {
        let shell = crate::core::terminal::default_shell();
        #[cfg(target_os = "windows")]
        {
            // Must NOT be /bin/bash on Windows
            assert!(
                !shell.contains("/bin/bash"),
                "Windows default shell should not be /bin/bash, got: {shell}"
            );
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Should be $SHELL or /bin/bash on Unix
            assert!(
                shell.contains("sh") || shell.contains("zsh") || shell.contains("fish"),
                "Unix default shell should be a known shell, got: {shell}"
            );
        }
    }

    // =====================================================================
    // Phase 2d: Behavioral backend parity tests
    //
    // These tests simulate user interaction sequences (the same engine method
    // calls that every backend must make) and assert that the engine state
    // transitions are correct.  A bug here means every backend is broken;
    // a missing engine call in a specific backend would pass these tests but
    // fail the Phase 2c static parity check.
    // =====================================================================

    /// Tab click switches to the correct tab and promotes preview tabs.
    #[test]
    fn test_behavior_tab_click_switches_tab() {
        let mut e = test_engine("first\n");
        let dir = std::env::temp_dir().join("vimcode_test_tab_click");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "file A\n").unwrap();
        std::fs::write(&f2, "file B\n").unwrap();

        e.open_file_in_tab(&f1);
        e.open_file_in_tab(&f2);
        // Now on tab 2 (f2).  Switch back to tab 0 (scratch).
        e.goto_tab(0);
        assert_eq!(
            e.active_group().active_tab,
            0,
            "goto_tab(0) should switch to first tab"
        );

        // Switch to tab 1 (f1)
        e.goto_tab(1);
        assert_eq!(e.active_group().active_tab, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tab close removes the tab and falls back to an adjacent tab.
    #[test]
    fn test_behavior_tab_close_removes_tab() {
        let mut e = test_engine("scratch\n");
        let dir = std::env::temp_dir().join("vimcode_test_tab_close");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        std::fs::write(&f1, "file A\n").unwrap();

        e.open_file_in_tab(&f1);
        assert_eq!(e.active_group().tabs.len(), 2);

        // Close the active tab (f1) — should fall back to scratch
        e.close_tab();
        assert_eq!(
            e.active_group().tabs.len(),
            1,
            "close_tab should remove the tab"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Backends must check dirty() before calling close_tab().
    /// This test verifies that dirty() detects unsaved changes so backends
    /// can show a confirmation dialog.  close_tab() itself is a raw operation.
    #[test]
    fn test_behavior_dirty_check_before_tab_close() {
        let mut e = test_engine("");
        let dir = std::env::temp_dir().join("vimcode_test_dirty_close");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        std::fs::write(&f1, "original\n").unwrap();

        e.open_file_in_tab(&f1);
        assert!(!e.dirty(), "freshly opened file should not be dirty");

        // Make the buffer dirty by inserting text
        e.handle_key("i", Some('i'), false);
        e.handle_key("x", Some('x'), false);
        e.handle_key("Escape", None, false);
        assert!(
            e.dirty(),
            "buffer should be dirty after insert — backends must check this before close_tab()"
        );

        // Verify the backend contract: if dirty() is true, do NOT call
        // close_tab() directly — show a dialog first.  We verify the raw
        // close_tab still works (backends call it after user confirms).
        let tabs_before = e.active_group().tabs.len();
        e.close_tab();
        assert_eq!(
            e.active_group().tabs.len(),
            tabs_before - 1,
            "close_tab() is a raw operation — backends gate it with dirty() check"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Context menu open/select/dismiss lifecycle.
    #[test]
    fn test_behavior_context_menu_lifecycle() {
        let mut e = test_engine("hello world\n");

        // Open editor context menu
        e.open_editor_context_menu(10, 5);
        assert!(
            e.context_menu.is_some(),
            "open_editor_context_menu should populate context_menu state"
        );
        let items_count = e.context_menu.as_ref().unwrap().items.len();
        assert!(items_count > 0, "context menu should have items");

        // Dismiss by clicking outside
        e.close_context_menu();
        assert!(
            e.context_menu.is_none(),
            "close_context_menu should clear the state"
        );

        // Open again and confirm an item
        e.open_editor_context_menu(10, 5);
        assert!(e.context_menu.is_some());
        let _action = e.context_menu_confirm();
        // After confirm, the menu should be closed
        assert!(
            e.context_menu.is_none(),
            "context_menu_confirm should close the menu"
        );
    }

    /// Explorer context menu opens with correct target type.
    #[test]
    fn test_behavior_explorer_context_menu() {
        let mut e = test_engine("");
        let dir = std::env::temp_dir().join("vimcode_test_ctx_explorer");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("test.txt");
        std::fs::write(&f1, "content\n").unwrap();

        e.open_explorer_context_menu(f1.clone(), false, 5, 10);
        assert!(e.context_menu.is_some());
        let ctx = e.context_menu.as_ref().unwrap();
        assert!(
            matches!(
                ctx.target,
                crate::core::engine::ContextMenuTarget::ExplorerFile { .. }
            ),
            "file click should produce ExplorerFile target"
        );

        e.close_context_menu();

        // Directory context menu
        e.open_explorer_context_menu(dir.clone(), true, 5, 10);
        assert!(e.context_menu.is_some());
        let ctx = e.context_menu.as_ref().unwrap();
        assert!(
            matches!(
                ctx.target,
                crate::core::engine::ContextMenuTarget::ExplorerDir { .. }
            ),
            "dir click should produce ExplorerDir target"
        );

        e.close_context_menu();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tab context menu opens with correct target.
    #[test]
    fn test_behavior_tab_context_menu() {
        let mut e = test_engine("hello\n");
        let gid = e.active_group;
        e.open_tab_context_menu(gid, 0, 20, 5);
        assert!(e.context_menu.is_some());
        let ctx = e.context_menu.as_ref().unwrap();
        assert!(
            matches!(
                ctx.target,
                crate::core::engine::ContextMenuTarget::Tab { .. }
            ),
            "tab right-click should produce Tab target"
        );
        e.close_context_menu();
    }

    /// Double-click in editor selects a word (enters visual mode).
    #[test]
    fn test_behavior_editor_double_click_selects_word() {
        let mut e = test_engine("hello world\n");
        let wid = e.active_window_id();
        e.mouse_double_click(wid, 0, 2); // double-click on "hello"
        assert_eq!(
            e.mode,
            crate::core::mode::Mode::Visual,
            "double-click should enter visual mode"
        );
    }

    /// Editor hover popup lifecycle: show → focus → scroll → dismiss.
    #[test]
    fn test_behavior_editor_hover_lifecycle() {
        let mut e = test_engine("fn main() {}\n");

        // Show a hover popup
        e.show_editor_hover(
            0,
            3,
            "**main** — entry point",
            crate::core::engine::EditorHoverSource::Lsp,
            false,
            false,
        );
        assert!(
            e.editor_hover.is_some(),
            "show_editor_hover should set editor_hover"
        );
        assert!(
            !e.editor_hover_has_focus,
            "hover should not auto-focus without take_focus"
        );

        // Focus the popup (simulates click on hover)
        e.editor_hover_focus();
        assert!(
            e.editor_hover_has_focus,
            "editor_hover_focus should set focus flag"
        );

        // Scroll the popup
        let scrolled = e.editor_hover_scroll(1);
        // Scroll may or may not change offset depending on content length,
        // but the method should not panic
        let _ = scrolled;

        // Dismiss
        e.dismiss_editor_hover();
        assert!(
            e.editor_hover.is_none(),
            "dismiss_editor_hover should clear popup"
        );
        assert!(!e.editor_hover_has_focus, "dismiss should clear focus flag");
    }

    /// Activity bar click toggles sidebar focus flags.
    #[test]
    fn test_behavior_sidebar_focus_toggle() {
        let mut e = test_engine("hello\n");

        // Simulate activity bar click → explorer
        e.explorer_has_focus = true;
        assert!(e.explorer_has_focus);

        // Simulate clicking editor → clear sidebar focus
        e.clear_sidebar_focus();
        assert!(
            !e.explorer_has_focus,
            "clear_sidebar_focus should clear explorer"
        );
        assert!(!e.search_has_focus);
        assert!(!e.sc_has_focus);
        assert!(!e.settings_has_focus);
        assert!(!e.ai_has_focus);
    }

    /// Terminal new tab / close tab lifecycle.
    #[test]
    fn test_behavior_terminal_new_and_close() {
        let mut e = test_engine("hello\n");

        // Create a terminal tab
        e.terminal_new_tab(80, 24);
        assert!(
            !e.terminal_panes.is_empty(),
            "terminal_new_tab should create a tab"
        );
        let count_after_new = e.terminal_panes.len();

        // Create another
        e.terminal_new_tab(80, 24);
        assert_eq!(e.terminal_panes.len(), count_after_new + 1);

        // Close active tab
        e.terminal_close_active_tab();
        assert_eq!(e.terminal_panes.len(), count_after_new);
    }

    /// Terminal split toggle.
    #[test]
    fn test_behavior_terminal_split_toggle() {
        let mut e = test_engine("hello\n");
        e.terminal_new_tab(80, 24);
        assert!(!e.terminal_split, "split should be off initially");

        e.terminal_toggle_split(80, 24);
        assert!(e.terminal_split, "toggle should enable split");

        e.terminal_toggle_split(80, 24);
        assert!(!e.terminal_split, "second toggle should disable split");
    }

    /// Tab drag and drop between groups creates a new split.
    #[test]
    fn test_behavior_tab_drag_drop_creates_split() {
        let mut e = test_engine("scratch\n");
        let dir = std::env::temp_dir().join("vimcode_test_drag_drop");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        std::fs::write(&f1, "file A\n").unwrap();

        e.open_file_in_tab(&f1);
        assert_eq!(e.editor_groups.len(), 1, "start with one group");

        let gid = e.active_group;
        // Move tab 1 (f1) to create a vertical split
        e.move_tab_to_new_split(
            gid,
            1,
            gid,
            crate::core::window::SplitDirection::Vertical,
            false,
        );
        assert_eq!(
            e.editor_groups.len(),
            2,
            "dropping into a split should create a second editor group"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Preview tab is promoted to permanent when goto_tab selects it.
    #[test]
    fn test_behavior_goto_tab_promotes_preview() {
        let mut e = test_engine("scratch\n");
        let dir = std::env::temp_dir().join("vimcode_test_promote");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        std::fs::write(&f1, "file A\n").unwrap();

        e.open_file_preview(&f1);
        let preview_buf = e.active_buffer_id();
        assert!(
            e.preview_buffer_id == Some(preview_buf),
            "open_file_preview should set preview_buffer_id"
        );

        // Switch away and back via goto_tab (simulates clicking the tab)
        let tab_idx = e.active_group().active_tab;
        e.goto_tab(0);
        e.goto_tab(tab_idx);
        assert!(
            e.preview_buffer_id.is_none() || e.preview_buffer_id != Some(preview_buf),
            "goto_tab should promote preview to permanent"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Editor click clears sidebar focus (backends must call clear_sidebar_focus).
    #[test]
    fn test_behavior_editor_click_clears_sidebar() {
        let mut e = test_engine("hello world\n");

        // Simulate various sidebar focus states
        e.explorer_has_focus = true;
        e.search_has_focus = true;
        e.sc_has_focus = true;
        e.ai_has_focus = true;
        e.settings_has_focus = true;

        // Simulate what backends do on editor click: clear sidebar, then click
        e.clear_sidebar_focus();
        let wid = e.active_window_id();
        e.mouse_click(wid, 0, 3);

        assert!(!e.explorer_has_focus);
        assert!(!e.search_has_focus);
        assert!(!e.sc_has_focus);
        assert!(!e.ai_has_focus);
        assert!(!e.settings_has_focus);
    }

    /// mouse_click moves cursor to the clicked position.
    #[test]
    fn test_behavior_mouse_click_moves_cursor() {
        let mut e = test_engine("hello world\nsecond line\n");
        let wid = e.active_window_id();
        e.mouse_click(wid, 1, 3);
        assert_eq!(e.cursor().line, 1, "click should move to line 1");
        assert_eq!(e.cursor().col, 3, "click should move to col 3");
    }

    /// Preview reuse: opening multiple previews reuses the same tab slot.
    #[test]
    fn test_behavior_preview_reuse_then_permanent() {
        let mut e = test_engine("scratch\n");
        let dir = std::env::temp_dir().join("vimcode_test_preview_reuse");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        let f3 = dir.join("c.txt");
        std::fs::write(&f1, "A\n").unwrap();
        std::fs::write(&f2, "B\n").unwrap();
        std::fs::write(&f3, "C\n").unwrap();

        // Preview f1
        e.open_file_preview(&f1);
        assert_eq!(e.active_group().tabs.len(), 2);

        // Preview f2 — should reuse the preview slot
        e.open_file_preview(&f2);
        assert_eq!(e.active_group().tabs.len(), 2, "preview should reuse slot");

        // Open f3 permanently — should create a new tab
        e.open_file_in_tab(&f3);
        assert_eq!(
            e.active_group().tabs.len(),
            3,
            "permanent open after preview should add a tab"
        );

        // Opening yet another preview should still reuse the preview slot
        e.open_file_preview(&f1);
        // The key invariant: at most one preview buffer exists at a time
        // preview_buffer_id tracks it (None means no preview, Some means exactly one)
        assert!(
            e.preview_buffer_id.is_some(),
            "should have a preview buffer after open_file_preview"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Tooltip adapter tests (hover popup + signature help) ───────────────

    #[test]
    fn test_hover_popup_to_tooltip_plain_multiline() {
        let hover = HoverPopup {
            text: "fn foo() -> i32\nReturns the answer.".to_string(),
            anchor_line: 5,
            anchor_col: 10,
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, layout) =
            hover_popup_to_quadraui_tooltip(&hover, 30.0, 20.0, viewport, 1.0, 1.0);

        // Plain multi-line path: styled_lines is None, text carries newlines.
        assert!(tooltip.styled_lines.is_none());
        assert!(tooltip.text.contains('\n'));
        // Placement preferred Top — layout resolves to Top because there's
        // room (anchor_y=20, height=2 → fits above).
        assert_eq!(layout.resolved_placement, quadraui::ResolvedPlacement::Top);
        // Popup is positioned above the cursor line.
        assert!(layout.bounds.y < 20.0);
    }

    #[test]
    fn test_signature_help_to_tooltip_highlights_active_param() {
        let theme = Theme::onedark();
        // Label: "fn from(s: &str) -> String"
        //         0    5   9       18
        // Params: param 0 is "s: &str" starting at byte 8 (after "fn from(").
        let sig = SignatureHelp {
            label: "fn from(s: &str) -> String".to_string(),
            params: vec![(8, 15)], // byte offsets of "s: &str"
            active_param: Some(0),
            anchor_line: 3,
            anchor_col: 20,
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, layout) =
            signature_help_to_quadraui_tooltip(&sig, 40.0, 15.0, viewport, &theme, 1.0, 1.0);

        // Styled path is active.
        let lines = tooltip.styled_lines.as_ref().expect("styled spans");
        assert_eq!(lines.len(), 1);
        let styled = &lines[0];
        // 5 spans: leading " ", pre, active, post, trailing " ".
        assert_eq!(styled.spans.len(), 5);
        assert_eq!(styled.spans[0].text, " ");
        assert_eq!(styled.spans[1].text, "fn from(");
        assert_eq!(styled.spans[2].text, "s: &str");
        assert_eq!(styled.spans[3].text, ") -> String");
        assert_eq!(styled.spans[4].text, " ");

        // Active span uses theme keyword colour; surrounding spans use hover_fg.
        let kw = to_q_color(theme.keyword);
        let fg = to_q_color(theme.hover_fg);
        assert_eq!(styled.spans[2].fg, Some(kw));
        assert_eq!(styled.spans[1].fg, Some(fg));
        assert_eq!(styled.spans[3].fg, Some(fg));

        // Single-line height; width sized to label + padding + borders.
        assert_eq!(layout.bounds.height, 1.0);
        assert!(layout.bounds.width >= 26.0);
    }

    #[test]
    fn test_signature_help_to_tooltip_no_active_param() {
        let theme = Theme::onedark();
        let sig = SignatureHelp {
            label: "fn noop()".to_string(),
            params: Vec::new(),
            active_param: None,
            anchor_line: 0,
            anchor_col: 0,
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, _layout) =
            signature_help_to_quadraui_tooltip(&sig, 10.0, 5.0, viewport, &theme, 1.0, 1.0);

        let lines = tooltip.styled_lines.as_ref().expect("styled spans");
        assert_eq!(lines.len(), 1);
        let styled = &lines[0];
        // Without active param: leading " ", full label as one span, trailing " ".
        assert_eq!(styled.spans.len(), 3);
        assert_eq!(styled.spans[1].text, "fn noop()");
        // Everything uses hover_fg (no keyword highlight).
        let fg = to_q_color(theme.hover_fg);
        assert_eq!(styled.spans[1].fg, Some(fg));
    }

    // ── editor_hover_to_quadraui_rich_text adapter tests (#488) ──────────────

    /// Build a minimal `EditorHoverPopupData` with a known link on line 0 so
    /// we can verify that `editor_hover_to_quadraui_rich_text` maps it into
    /// `RichTextPopup.links` with the correct byte offsets.  These offsets are
    /// exactly what the GTK `link_widths` closure indexes into via
    /// `pango_layout.index_to_pos(start_byte)` — wrong offsets would shift the
    /// hit-rect even if the Pango measurement itself is accurate (#488).
    #[test]
    fn test_editor_hover_to_quadraui_rich_text_link_offsets() {
        use crate::core::markdown::{MdRendered, MdSpan, MdStyle};

        let line = "See https://example.com for details";
        // byte offsets of "https://example.com": starts at 4, ends at 23
        let link_start = 4usize;
        let link_end = 23usize;
        assert_eq!(&line[link_start..link_end], "https://example.com");

        let rendered = MdRendered {
            lines: vec![line.to_string()],
            spans: vec![vec![MdSpan {
                start_byte: link_start,
                end_byte: link_end,
                style: MdStyle::LinkUrl,
            }]],
            code_highlights: vec![vec![]],
        };
        let eh = EditorHoverPopupData {
            rendered,
            links: vec![(0, link_start, link_end, "https://example.com".to_string())],
            anchor_line: 0,
            anchor_col: 0,
            scroll_top: 0,
            focused_link: None,
            has_focus: false,
            popup_width: 40,
            frozen_scroll_top: 0,
            frozen_scroll_left: 0,
            selection: None,
        };
        let theme = Theme::onedark();
        let popup = editor_hover_to_quadraui_rich_text(&eh, &theme);

        // Exactly one link.
        assert_eq!(popup.links.len(), 1, "one link expected");
        let link = &popup.links[0];
        // Byte offsets must survive the conversion unchanged.
        assert_eq!(link.line, 0);
        assert_eq!(
            link.start_byte, link_start,
            "start_byte mismatch — GTK index_to_pos would compute wrong x0"
        );
        assert_eq!(
            link.end_byte, link_end,
            "end_byte mismatch — GTK index_to_pos would compute wrong x1"
        );
        assert_eq!(link.url, "https://example.com");
        // line_text[0] must equal the raw line so index_to_pos byte indices are valid.
        assert_eq!(
            popup.line_text.get(0).map(String::as_str),
            Some(line),
            "line_text must carry the raw text unchanged"
        );
        // Sanity: the byte range must index valid UTF-8 within line_text.
        let raw = &popup.line_text[0];
        assert_eq!(&raw[link.start_byte..link.end_byte], "https://example.com");
    }

    /// Multi-link hover: two URLs on different lines.  Verifies that lines and
    /// link indices stay in sync after the adapter — an off-by-one in `links`
    /// would cause the GTK closure to measure the wrong line or wrong span.
    #[test]
    fn test_editor_hover_to_quadraui_rich_text_multi_link() {
        use crate::core::markdown::{MdRendered, MdSpan, MdStyle};

        let line0 = "Docs: https://docs.rs/foo";
        let line1 = "Also see https://crates.io/crates/foo";
        // "https://docs.rs/foo" starts at 6, ends at 25
        // "https://crates.io/crates/foo" starts at 9, ends at 37
        let (s0, e0) = (6, 25);
        let (s1, e1) = (9, 37);
        assert_eq!(&line0[s0..e0], "https://docs.rs/foo");
        assert_eq!(&line1[s1..e1], "https://crates.io/crates/foo");

        let rendered = MdRendered {
            lines: vec![line0.to_string(), line1.to_string()],
            spans: vec![
                vec![MdSpan {
                    start_byte: s0,
                    end_byte: e0,
                    style: MdStyle::LinkUrl,
                }],
                vec![MdSpan {
                    start_byte: s1,
                    end_byte: e1,
                    style: MdStyle::LinkUrl,
                }],
            ],
            code_highlights: vec![vec![], vec![]],
        };
        let eh = EditorHoverPopupData {
            rendered,
            links: vec![
                (0, s0, e0, "https://docs.rs/foo".to_string()),
                (1, s1, e1, "https://crates.io/crates/foo".to_string()),
            ],
            anchor_line: 0,
            anchor_col: 0,
            scroll_top: 0,
            focused_link: None,
            has_focus: false,
            popup_width: 40,
            frozen_scroll_top: 0,
            frozen_scroll_left: 0,
            selection: None,
        };
        let theme = Theme::onedark();
        let popup = editor_hover_to_quadraui_rich_text(&eh, &theme);

        assert_eq!(popup.links.len(), 2);
        assert_eq!(popup.links[0].line, 0);
        assert_eq!(popup.links[0].start_byte, s0);
        assert_eq!(popup.links[0].end_byte, e0);
        assert_eq!(popup.links[1].line, 1);
        assert_eq!(popup.links[1].start_byte, s1);
        assert_eq!(popup.links[1].end_byte, e1);
        // line_text must be in sync with link offsets.
        assert_eq!(&popup.line_text[0][s0..e0], "https://docs.rs/foo");
        assert_eq!(&popup.line_text[1][s1..e1], "https://crates.io/crates/foo");
    }

    #[test]
    fn test_tab_switcher_to_list_view_dirty_and_scroll() {
        let ts = TabSwitcherPanel {
            items: vec![
                ("main.rs".to_string(), "/src/main.rs".to_string(), false),
                ("lib.rs".to_string(), "/src/lib.rs".to_string(), true),
                (
                    "keys.rs".to_string(),
                    "/src/core/keys.rs".to_string(),
                    false,
                ),
                ("tests.rs".to_string(), "/src/tests.rs".to_string(), false),
                ("todo.md".to_string(), "".to_string(), false),
            ],
            selected_idx: 4,
        };
        let list = tab_switcher_to_quadraui_list_view(&ts, 3);

        // Bordered modal with title overlay.
        assert!(list.bordered);
        assert!(list.title.is_some());
        // 5 items, all present.
        assert_eq!(list.items.len(), 5);
        // Dirty marker appended to filename label (rendered as text,
        // not detail — matches legacy behavior).
        let lib_text: String = list.items[1]
            .text
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(lib_text.contains("●"));
        let main_text: String = list.items[0]
            .text
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(!main_text.contains("●"));
        // Paths appear as detail (right-aligned dimmed in the rasteriser).
        assert!(list.items[0].detail.is_some());
        // Empty path → no detail (avoids rendering a lone trailing space).
        assert!(list.items[4].detail.is_none());
        // Scroll so selected (idx=4) is visible inside max_visible=3:
        // offset = 4 + 1 - 3 = 2.
        assert_eq!(list.scroll_offset, 2);
        assert_eq!(list.selected_idx, 4);
    }

    #[test]
    fn test_diff_peek_to_tooltip_per_line_colors_and_action_bar() {
        let theme = Theme::onedark();
        let peek = DiffPeekPopup {
            anchor_line: 5,
            hunk_lines: vec![
                " let x = 1;".to_string(),
                "-old line".to_string(),
                "+new line".to_string(),
            ],
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, layout) =
            diff_peek_to_quadraui_tooltip(&peek, 30.0, 10.0, viewport, &theme, 1.0, 1.0);

        // Multi-line styled path active.
        let lines = tooltip.styled_lines.as_ref().expect("styled_lines");
        // 3 hunk lines + 1 action bar = 4 rows.
        assert_eq!(lines.len(), 4);

        let added = to_q_color(theme.git_added);
        let deleted = to_q_color(theme.git_deleted);
        let fg = to_q_color(theme.hover_fg);

        // Context line: hover_fg.
        assert_eq!(lines[0].spans[0].fg, Some(fg));
        // Deleted line: git_deleted.
        assert_eq!(lines[1].spans[0].fg, Some(deleted));
        // Added line: git_added.
        assert_eq!(lines[2].spans[0].fg, Some(added));
        // Action bar: default fg, contains hotkey labels.
        let action: String = lines[3].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(action.contains("[s] Stage"));
        assert!(action.contains("[r] Revert"));
        assert!(action.contains("[q] Close"));
        assert_eq!(lines[3].spans[0].fg, Some(fg));

        // Placement: prefers Bottom (legacy diff peek always rendered below).
        // Anchor at y=10 with viewport height 50 → fits below → Bottom resolved.
        assert_eq!(
            layout.resolved_placement,
            quadraui::ResolvedPlacement::Bottom
        );
        assert!(layout.bounds.y > 10.0);
        // Multi-row height (4 rows).
        assert_eq!(layout.bounds.height, 4.0);
    }

    #[test]
    fn test_signature_help_active_param_out_of_range_falls_back() {
        let theme = Theme::onedark();
        // active_param index points past end of params list — adapter falls
        // back to no-highlight path.
        let sig = SignatureHelp {
            label: "fn foo(x: i32)".to_string(),
            params: vec![(7, 13)],
            active_param: Some(5), // out of range
            anchor_line: 0,
            anchor_col: 0,
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, _layout) =
            signature_help_to_quadraui_tooltip(&sig, 10.0, 5.0, viewport, &theme, 1.0, 1.0);
        let lines = tooltip.styled_lines.as_ref().expect("styled spans");
        assert_eq!(lines.len(), 1);
        let styled = &lines[0];
        // Fallback: 3 spans (leading-pad, whole-label, trailing-pad).
        assert_eq!(styled.spans.len(), 3);
        assert_eq!(styled.spans[1].text, "fn foo(x: i32)");
    }

    #[test]
    fn test_breadcrumb_bounds_do_not_overlap_first_line() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let mut engine = Engine::new();
        engine.settings.breadcrumbs = true;
        engine.buffer_mut().insert(0, "line 1\nline 2\nline 3\n");

        let line_height = 20.0;
        let char_width = 8.0;
        let tbh = tab_bar_height_px(line_height, true);
        let wid = engine.active_window_id();
        let rects = vec![(wid, WindowRect::new(0.0, tbh, 800.0, 600.0 - tbh))];
        let theme = Theme::onedark();
        let layout = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);

        assert!(!layout.breadcrumbs.is_empty());
        let bc = &layout.breadcrumbs[0];
        // Breadcrumb bounds must sit ABOVE the window content, not overlap it.
        let window_top = layout.windows[0].rect.y;
        assert!(
            bc.bounds.y + bc.bounds.height <= window_top,
            "breadcrumb bottom ({}) must not exceed window top ({})",
            bc.bounds.y + bc.bounds.height,
            window_top,
        );

        // Clicking at the window top (line 1) must return Window, not Breadcrumb.
        let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);
        let zone = screen_zone_hit_test(
            &layout,
            100.0,
            window_top,
            tbh,
            single_tab_hidden,
            engine.active_group,
        );
        assert!(
            matches!(zone, ScreenZone::Window { .. }),
            "click at window_top should hit Window zone, got {:?}",
            zone,
        );
    }

    /// #546 FAILED-3 regression: GTK's `main_content_bounds` gained a
    /// persistent nonzero `(x, y)` offset once the always-visible
    /// menu/title-bar chrome band landed (#552) — every window rect (and
    /// every click coordinate) GTK builds lives in that same absolute
    /// space, not one that starts at `(0, 0)`. The single-group branch of
    /// `screen_zone_hit_test` used to hardcode `y >= 0.0` as the tab row's
    /// top, so a click on the actually-rendered (offset) tab bar — e.g. a
    /// tab's close button — was silently misclassified as a `Window` hit
    /// instead of `TabBar`, and the click just moved the cursor instead of
    /// closing the tab. This pins the fix: derive the bar's bounds from the
    /// real (possibly-offset) window rects, exactly like the multi-group
    /// branch above already does via `GroupTabBar::bounds`.
    #[test]
    fn test_single_group_tab_bar_hit_test_with_editor_offset() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let engine = Engine::new();
        let line_height = 20.0;
        let char_width = 8.0;
        let tbh = tab_bar_height_px(line_height, false);

        // Simulate a chrome-shifted `main_content_bounds`: editor content
        // starts at (50, 100), not (0, 0).
        let content_x = 50.0;
        let content_y = 100.0;
        let wid = engine.active_window_id();
        let rects = vec![(
            wid,
            WindowRect::new(content_x, content_y + tbh, 800.0, 600.0),
        )];
        let theme = Theme::onedark();
        let layout = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);

        // A click at the tab row's actual (offset) position must resolve to
        // TabBar, not fall through to a Window hit underneath.
        let zone = screen_zone_hit_test(
            &layout,
            content_x + 5.0,
            content_y + 2.0,
            tbh,
            single_tab_hidden,
            engine.active_group,
        );
        match zone {
            ScreenZone::TabBar {
                group_id, local_x, ..
            } => {
                assert_eq!(group_id, engine.active_group);
                assert!(
                    (local_x - 5.0).abs() < f64::EPSILON,
                    "local_x should be relative to the bar's left edge, got {local_x}"
                );
            }
            other => panic!("expected TabBar zone, got {other:?}"),
        }

        // A click below the tab bar, inside the window, must still resolve
        // to Window — the offset derivation shouldn't just widen the band
        // indefinitely.
        let zone = screen_zone_hit_test(
            &layout,
            content_x + 5.0,
            content_y + tbh + 5.0,
            tbh,
            single_tab_hidden,
            engine.active_group,
        );
        assert!(
            matches!(zone, ScreenZone::Window { .. }),
            "click below the tab bar should hit Window zone, got {zone:?}"
        );
    }

    /// #553 (click-side counterpart of #549's draw-loop unification): the
    /// single-group and split-group tab-bar hit bands come out of ONE
    /// derivation, and in both shapes the band's top edge is
    /// `window_content_top - tab_bar_height` — never a hardcoded origin.
    ///
    /// The regression this guards is asymmetric by construction: with a
    /// chrome-shifted content origin the split arm kept working (it derived the
    /// top from `GroupTabBar::bounds`) while the single arm went dead (it
    /// assumed `y >= 0.0`), which is exactly the "works with 2+ groups, dead
    /// with 1" symptom #553 reports. So both shapes are asserted here against
    /// the same offset layout.
    #[test]
    fn test_tab_bar_hit_bands_single_and_split_share_one_derivation() {
        use crate::core::engine::Engine;
        use crate::core::window::{SplitDirection, WindowRect};

        let line_height = 20.0;
        let char_width = 8.0;
        let tbh = tab_bar_height_px(line_height, false);
        // Chrome-shifted content origin — the #552 menu/title-bar band.
        let content = WindowRect::new(50.0, 100.0, 800.0, 600.0);
        let theme = Theme::onedark();

        // ── Single group ──────────────────────────────────────────────────
        let mut engine = Engine::new();
        engine.new_tab(None); // 2 tabs, so `hide_single_tab` can't suppress the bar
        let (rects, _) = engine.calculate_group_window_rects(content, tbh);
        let layout = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        let bands = tab_bar_hit_bands(
            &layout,
            tbh,
            engine.is_tab_bar_hidden(engine.active_group),
            engine.active_group,
        );
        assert_eq!(bands.len(), 1, "one group draws one tab bar: {bands:?}");
        assert_eq!(bands[0].group_id, engine.active_group);
        assert_eq!(
            bands[0].y, content.y,
            "the single-group band must start at the *content* origin minus the bar height, \
             not at 0.0 (#546 FAILED-3 / #553): {bands:?}"
        );
        assert!(bands[0].contains(content.x + 5.0, content.y + 2.0));
        assert!(
            !bands[0].contains(content.x + 5.0, content.y - 1.0),
            "the band must not extend above the reserved chrome"
        );

        // ── Split groups ──────────────────────────────────────────────────
        engine.open_editor_group(SplitDirection::Vertical);
        let (rects, _) = engine.calculate_group_window_rects(content, tbh);
        let layout = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        let split_bands = tab_bar_hit_bands(
            &layout,
            tbh,
            engine.is_tab_bar_hidden(engine.active_group),
            engine.active_group,
        );
        assert_eq!(
            split_bands.len(),
            2,
            "two groups draw two tab bars: {split_bands:?}"
        );
        for band in &split_bands {
            assert_eq!(
                band.y, content.y,
                "every split band uses the same content-derived top edge as the \
                 single-group one: {split_bands:?}"
            );
            assert!(band.width > 0.0);
            assert!(band.contains(band.x + 1.0, band.y + 1.0));
        }
        // The two bands tile the content width without overlapping.
        let mut xs: Vec<f64> = split_bands.iter().map(|b| b.x).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(xs[0] < xs[1], "split bands must sit side by side: {xs:?}");
    }

    /// Pins that `bc.bounds.y` (row units, matching TUI's convention) shifts
    /// up by one row when a single-tab group's tab bar is hidden
    /// (`hide_single_tab`), and down by one when it's shown. TUI's
    /// single-group breadcrumb draw used to special-case
    /// `is_tab_bar_hidden` itself instead of trusting `bc.bounds.y` (#547);
    /// this pins the equivalence that made unifying it onto
    /// `breadcrumb_draw_targets` safe — `calculate_group_window_rects` →
    /// `adjust_group_rects_for_hidden_tabs` is what actually shifts the
    /// window (and thus breadcrumb) bounds.
    #[test]
    fn test_single_group_breadcrumb_bounds_reflect_hidden_tab_bar() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 1.0; // TUI row units.
        let char_width = 1.0;
        let theme = Theme::onedark();

        let bounds_y_for = |hide_single_tab: bool| -> f64 {
            let mut engine = Engine::new();
            engine.settings.breadcrumbs = true;
            engine.settings.hide_single_tab = hide_single_tab;
            // TUI's own row-unit convention (`tui_tab_bar_height` in
            // `render_impl.rs`), NOT `tab_bar_height_px` — that helper rounds
            // to a pixel-oriented `line_height * 1.6` tab row for GTK/Win-GUI,
            // which doesn't map to a clean row count in TUI's 1-row-per-line
            // units.
            let tbh = 2.0;
            let content_bounds = WindowRect::new(0.0, 0.0, 80.0, 24.0);
            let (rects, _) = engine.calculate_group_window_rects(content_bounds, tbh);
            let layout =
                build_screen_layout(&engine, &theme, &rects, line_height, char_width, true);
            assert!(!layout.breadcrumbs.is_empty());
            layout.breadcrumbs[0].bounds.y
        };

        // Tab bar shown (default): breadcrumb sits one row below it.
        assert_eq!(bounds_y_for(false), 1.0);
        // Tab bar hidden (single tab, hide_single_tab=true): breadcrumb
        // claims the row the tab bar would have used.
        assert_eq!(bounds_y_for(true), 0.0);
    }

    /// Direct unit test for `breadcrumb_draw_targets` itself (#547 review
    /// finding: the test above only pins the pre-existing `build_screen_layout`
    /// bounds computation, never the new shared helper). Covers the
    /// `terminal_maximized` early return, the pass-through of already-absolute
    /// bounds (#550 — the `origin_offset` translation this used to carry was
    /// dropped once both backends feed absolute window rects), the
    /// `segments.is_empty()` filter, and the zero-width fallback filter.
    #[test]
    fn test_breadcrumb_draw_targets_offset_terminal_maximized_and_filters() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 20.0;
        let char_width = 8.0;
        let theme = Theme::onedark();

        let build_screen = || {
            let mut engine = Engine::new();
            engine.settings.breadcrumbs = true;
            // A default `Engine::new()` buffer has no `file_path`, which
            // produces zero breadcrumb segments (see
            // `build_breadcrumbs_for_group`) — give it a path so the
            // non-maximized case below actually has a segment to draw.
            let buf_id = engine.active_buffer_id();
            engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
                Some(std::path::PathBuf::from("src/main.rs"));
            let tbh = 24.0;
            // Non-zero origin to prove `breadcrumb_draw_targets` passes
            // through absolute bounds untouched rather than assuming (0,0).
            let content_bounds = WindowRect::new(10.0, 20.0, 800.0, 600.0);
            let (rects, _) = engine.calculate_group_window_rects(content_bounds, tbh);
            build_screen_layout(&engine, &theme, &rects, line_height, char_width, true)
        };

        let screen = build_screen();
        assert_eq!(screen.breadcrumbs.len(), 1);
        assert!(!screen.breadcrumbs[0].segments.is_empty());
        assert!(screen.breadcrumbs[0].bounds.width > 0.0);

        // `terminal_maximized` short-circuits to empty.
        let targets = breadcrumb_draw_targets(&screen, true, line_height);
        assert!(
            targets.is_empty(),
            "terminal_maximized must suppress all breadcrumb targets"
        );

        // Not maximized: one target, matching the already-absolute bounds.
        let targets = breadcrumb_draw_targets(&screen, false, line_height);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].rect.x, screen.breadcrumbs[0].bounds.x as f32);
        assert_eq!(targets[0].rect.y, screen.breadcrumbs[0].bounds.y as f32);
        assert_eq!(
            targets[0].rect.width,
            screen.breadcrumbs[0].bounds.width as f32
        );
        assert_eq!(targets[0].rect.height, line_height as f32);

        // Empty segments are filtered out even when not maximized.
        let mut screen_no_segments = build_screen();
        screen_no_segments.breadcrumbs[0].segments.clear();
        let targets = breadcrumb_draw_targets(&screen_no_segments, false, line_height);
        assert!(
            targets.is_empty(),
            "a breadcrumb bar with no segments must not be drawn"
        );

        // Zero-width bounds (the `min_x == f64::MAX` fallback for a group with
        // no matching window rects) are filtered out too, so GTK doesn't need
        // its own `rect.width > 0.0` guard (unlike TUI's pre-existing one).
        let mut screen_zero_width = build_screen();
        screen_zero_width.breadcrumbs[0].bounds.width = 0.0;
        let targets = breadcrumb_draw_targets(&screen_zero_width, false, line_height);
        assert!(
            targets.is_empty(),
            "a zero-width breadcrumb bar must not be drawn"
        );
    }

    /// Direct unit test for `tab_bar_draw_targets` (#549, follow-up to
    /// #547's `breadcrumb_draw_targets`; rewritten for #551).
    ///
    /// The single-group case used to be served by a hand-written `else` arm
    /// that painted a caller-supplied `(x, y, width)` rect. #551 deleted it:
    /// `ScreenLayout::group_tab_bars` now holds one entry for one group, and
    /// the generic `bounds.y - reserved_h` math must reproduce *exactly* the
    /// full-width editor-top rect the deleted arm hard-coded. That equivalence
    /// is the whole point of the refactor, so it is asserted explicitly below
    /// against the `content_bounds` the caller laid the frame out with.
    #[test]
    fn test_tab_bar_draw_targets_single_and_split() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 20.0;
        let char_width = 8.0;
        let theme = Theme::onedark();
        let tab_row_h = 32.0; // lh * 1.6
        let reserved_h = 32.0; // no breadcrumbs: reserved == tab row height

        // ── Single-group mode ───────────────────────────────────────────
        // Non-zero origin so "the generic path reproduces the old hard-coded
        // editor-origin rect" is a real claim, not a zero-origin coincidence.
        let mut engine = Engine::new();
        let content_bounds = WindowRect::new(10.0, 20.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, reserved_h);
        let screen = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        assert!(screen.editor_group_split.is_none());
        // #551: the per-group chrome is populated even with a single group.
        assert_eq!(
            screen.group_tab_bars.len(),
            1,
            "one group must still produce one GroupTabBar (split-of-1)"
        );
        assert!(
            screen.group_dividers.is_empty(),
            "one group has no inter-group dividers"
        );

        let targets = tab_bar_draw_targets(&engine, &screen, tab_row_h, reserved_h);
        assert_eq!(targets.len(), 1);
        // Exactly the rect the deleted single-group arm used to hard-code from
        // the caller's editor origin/width.
        assert_eq!(targets[0].rect.x, content_bounds.x as f32);
        assert_eq!(targets[0].rect.y, content_bounds.y as f32);
        assert_eq!(targets[0].rect.width, content_bounds.width as f32);
        assert_eq!(targets[0].rect.height, tab_row_h as f32);
        assert_eq!(targets[0].group_id, engine.active_group);

        // Hiding the single group's tab bar suppresses the target entirely.
        engine.settings.hide_single_tab = true;
        let screen_hidden =
            build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        let targets = tab_bar_draw_targets(&engine, &screen_hidden, tab_row_h, reserved_h);
        assert!(
            targets.is_empty(),
            "a hidden single-group tab bar must not be drawn"
        );

        // ── Split-group mode ────────────────────────────────────────────
        // Non-zero content_bounds origin to prove `tab_bar_draw_targets`
        // passes through absolute bounds untouched rather than assuming (0,0).
        let mut engine = Engine::new();
        engine.execute_command("EditorGroupSplit");
        assert_eq!(engine.group_layout.leaf_count(), 2);
        let content_bounds = WindowRect::new(5.0, 7.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, reserved_h);
        let screen = build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        assert!(
            screen.editor_group_split.is_some(),
            "2 groups must produce Some(editor_group_split)"
        );
        assert_eq!(screen.group_tab_bars.len(), 2);
        assert_eq!(
            screen.group_dividers.len(),
            1,
            "a 2-group split has exactly one inter-group divider"
        );

        // Rect derived from the already-absolute `bounds.y - reserved_h`.
        let targets = tab_bar_draw_targets(&engine, &screen, tab_row_h, reserved_h);
        assert_eq!(targets.len(), 2);
        for (target, gtb) in targets.iter().zip(screen.group_tab_bars.iter()) {
            assert_eq!(target.group_id, gtb.group_id);
            assert_eq!(target.rect.x, gtb.bounds.x as f32);
            assert_eq!(target.rect.y, (gtb.bounds.y - reserved_h) as f32);
            assert_eq!(target.rect.width, gtb.bounds.width as f32);
            assert_eq!(target.rect.height, tab_row_h as f32);
        }

        // Note: `is_tab_bar_hidden` only ever returns true in single-group
        // mode (`hide_single_tab` + `leaf_count() <= 1`, see
        // `Engine::is_tab_bar_hidden`), so there's no reachable per-group
        // "hidden" state to exercise here in split mode — the
        // `is_tab_bar_hidden` filter is defensive, matching what the
        // pre-existing per-backend loops did.

        // Zero-width bounds (the `min_x == f64::MAX` fallback) are filtered
        // out too, mirroring `breadcrumb_draw_targets`.
        let mut screen_zero_width =
            build_screen_layout(&engine, &theme, &rects, line_height, char_width, false);
        screen_zero_width.group_tab_bars[0].bounds.width = 0.0;
        let targets = tab_bar_draw_targets(&engine, &screen_zero_width, tab_row_h, reserved_h);
        assert_eq!(
            targets.len(),
            1,
            "a zero-width group tab bar must not be drawn"
        );
    }

    /// #551: the single `GroupTabBar` synthesised for an unsplit editor must
    /// carry byte-for-byte the same tab content the old single-group-only
    /// fields did. If these ever diverge, the unified draw path would silently
    /// paint a *different* tab bar than the pre-#551 code did — the exact
    /// class of regression #547 hit when a single-group calculation drifted
    /// from the generic one.
    #[test]
    fn test_single_group_tab_bar_matches_legacy_single_group_fields() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let theme = Theme::onedark();
        let mut engine = Engine::new();
        // More than one tab so tab labels/active flags are non-trivial.
        engine.execute_command("tabnew");
        engine.execute_command("tabnew");
        assert_eq!(engine.group_layout.leaf_count(), 1);

        let content_bounds = WindowRect::new(3.0, 4.0, 120.0, 40.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, 1.0);
        let screen = build_screen_layout(&engine, &theme, &rects, 1.0, 1.0, false);

        assert_eq!(screen.group_tab_bars.len(), 1);
        let gtb = &screen.group_tab_bars[0];
        assert_eq!(gtb.group_id, engine.active_group);
        assert_eq!(
            gtb.tabs.len(),
            screen.tab_bar.len(),
            "group tab list must match the legacy single-group tab list"
        );
        for (a, b) in gtb.tabs.iter().zip(screen.tab_bar.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.active, b.active);
            assert_eq!(a.dirty, b.dirty);
        }
        assert_eq!(gtb.tab_scroll_offset, screen.tab_scroll_offset);
        // `TabBarHitRegion`/`TabBarClickTarget` are `Debug` but not `PartialEq`,
        // so compare their debug renderings pairwise.
        assert_eq!(
            gtb.hit_regions.len(),
            screen.tab_bar_hit_regions.len(),
            "group hit regions must match the legacy single-group hit regions"
        );
        for ((ra, ta), (rb, tb)) in gtb
            .hit_regions
            .iter()
            .zip(screen.tab_bar_hit_regions.iter())
        {
            assert_eq!(format!("{ra:?}"), format!("{rb:?}"));
            assert_eq!(format!("{ta:?}"), format!("{tb:?}"));
        }
        // The group's bounds must be the editor content area, i.e. the tab row
        // recovered from it lands exactly on the editor's top edge.
        assert_eq!(gtb.bounds.x, content_bounds.x);
        assert_eq!(gtb.bounds.y - 1.0, content_bounds.y);
        assert_eq!(gtb.bounds.width, content_bounds.width);
    }

    /// #551: `screen_to_drop_group_bounds` lost its
    /// origin/size/tab-bar-height parameters along with the single-group arm
    /// that needed them. The bounds it returns for an unsplit editor must
    /// still be the *content* area (past the tab bar), which is what
    /// `build_tab_drop_groups` reconstructs the tab-bar band from — the #477
    /// regression this pins.
    #[test]
    fn test_drop_group_bounds_single_group_is_content_area() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let theme = Theme::onedark();
        let engine = Engine::new();
        let content_bounds = WindowRect::new(6.0, 2.0, 90.0, 30.0);
        let tab_bar_height = 1.0;
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, tab_bar_height);
        let screen = build_screen_layout(&engine, &theme, &rects, 1.0, 1.0, false);

        let bounds = screen_to_drop_group_bounds(&screen);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].group_id, engine.active_group);
        assert_eq!(bounds[0].x, content_bounds.x as f32);
        assert_eq!(
            bounds[0].y,
            (content_bounds.y + tab_bar_height) as f32,
            "drop bounds start below the tab bar"
        );
        assert_eq!(bounds[0].width, content_bounds.width as f32);
        assert_eq!(
            bounds[0].content_height,
            (content_bounds.height - tab_bar_height) as f32
        );
    }

    /// Explorer tree row icons must always carry both a Nerd Font glyph and
    /// a distinct fallback (#547) — selection between them is entirely the
    /// backend's job (`Backend::set_nerd_fonts`), not `build_explorer_tree_rows`'s.
    /// This is the platform-neutral half of the #547 icon regression: the
    /// shared row-building logic was never the problem, but pinning it
    /// guards against the fix drifting back to a GTK-only icon shim.
    #[test]
    fn test_explorer_tree_rows_carry_glyph_and_fallback_icons() {
        use crate::core::engine::{Engine, ExplorerRow};
        use std::path::PathBuf;

        let engine = Engine::new();
        let theme = Theme::onedark();
        let rows = vec![
            ExplorerRow {
                depth: 0,
                name: "src".to_string(),
                path: PathBuf::from("src"),
                is_dir: true,
                is_expanded: true,
            },
            ExplorerRow {
                depth: 1,
                name: "main.rs".to_string(),
                path: PathBuf::from("src/main.rs"),
                is_dir: false,
                is_expanded: false,
            },
        ];

        let tree_rows = build_explorer_tree_rows(&rows, &engine, &theme);
        assert_eq!(tree_rows.len(), 2);
        for row in &tree_rows {
            let icon = row.icon.as_ref().expect("every explorer row has an icon");
            assert!(!icon.glyph.is_empty(), "glyph must not be empty");
            assert!(!icon.fallback.is_empty(), "fallback must not be empty");
            assert_ne!(
                icon.glyph, icon.fallback,
                "glyph and fallback must differ so backend selection is observable"
            );
        }
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
        // x=450 is 25% across the group, not 75% (which is what 450/600 would
        // give if the origin were dropped).
        let r = divider_ratio_from_pos(&dividers[0], 450.0, 300.0);
        assert!((r - 0.25).abs() < 0.0001, "ratio = {r}");
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
}
