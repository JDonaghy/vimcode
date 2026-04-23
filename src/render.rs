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
use crate::core::engine::{AlignedDiffEntry, DiffLine, Engine, SearchDirection};
pub use crate::core::engine::{BottomPanelKind, DebugSidebarSection};
use crate::core::lsp::SignatureHelpData;
use crate::core::settings::LineNumberMode;
use crate::core::terminal::TermSelection as CoreTermSelection;
use crate::core::view::View;
use crate::core::window::{GroupDivider, GroupId, SplitDirection};
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

    // Per-tab width: name chars + TAB_CLOSE_COLS for the close-and-sep glyph.
    // Close hit region is the trailing 2 cells (matches legacy behaviour:
    // clicks on × or the trailing separator count as close).
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|t| t.name.chars().count() + TAB_CLOSE_COLS as usize)
        .collect();

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
#[derive(Debug, Clone)]
pub struct BreadcrumbBar {
    pub group_id: GroupId,
    pub segments: Vec<BreadcrumbSegment>,
    pub bounds: WindowRect,
}

/// Present when the editor area is split into two or more independent groups.
/// `ScreenLayout.tab_bar` always contains the first group's tab bar for
/// backward compat in single-group mode.
#[derive(Debug, Clone)]
pub struct EditorGroupSplitData {
    /// Tab bars for ALL groups (in tree traversal order).
    pub group_tab_bars: Vec<GroupTabBar>,
    /// ID of the currently focused group.
    pub active_group: GroupId,
    /// Dividers between groups (for drawing divider lines and drag handling).
    pub dividers: Vec<GroupDivider>,
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

// ─── TabSwitcherPanel ─────────────────────────────────────────────────────

/// Data needed to render the tab switcher popup (Ctrl+Tab MRU list).
#[derive(Debug, Clone)]
pub struct TabSwitcherPanel {
    /// MRU-ordered items: (filename, full_path, is_dirty).
    pub items: Vec<(String, String, bool)>,
    /// Index of the currently highlighted item.
    pub selected_idx: usize,
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

/// A single item rendered in the debug sidebar.
#[derive(Debug, Clone)]
pub struct DebugSidebarItem {
    /// Pre-formatted display text.
    pub text: String,
    /// Indentation level (0 = top-level, 1 = one indent, …).
    pub indent: u8,
    /// Whether this item is currently selected (cursor highlight).
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
    /// True when a DAP session is active.
    pub session_active: bool,
    /// True when the debuggee is paused (breakpoint hit, step completed, etc.).
    pub stopped: bool,
    /// Variables section items (flat tree with ▶/▼ prefixes).
    pub variables: Vec<DebugSidebarItem>,
    /// Watch section items (expression = value).
    pub watch: Vec<DebugSidebarItem>,
    /// Call Stack section items.
    pub frames: Vec<DebugSidebarItem>,
    /// Breakpoints section items (always populated from dap_breakpoints).
    pub breakpoints: Vec<DebugSidebarItem>,
    /// Which section is currently focused.
    pub active_section: DebugSidebarSection,
    /// Selected item index within the active section.
    pub sidebar_selected: usize,
    /// Whether the debug sidebar panel has keyboard focus.
    pub has_focus: bool,
    /// Name of the selected launch configuration, or `None` if no configs loaded.
    pub launch_config_name: Option<String>,
    /// Debug output lines for the Debug Output bottom tab.
    pub debug_output_lines: Vec<String>,
    /// Most-recent expression evaluation result, or `None`.
    pub eval_result: Option<String>,
    /// Per-section scroll offset (items to skip from top) for [Variables, Watch, CallStack, Breakpoints].
    pub scroll_offsets: [usize; 4],
    /// Per-section allocated content heights in rows (excluding section header).
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

/// A single rendered cell in the terminal grid.
#[derive(Debug, Clone)]
pub struct TerminalCell {
    pub ch: char,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Whether this cell is within the mouse selection.
    pub selected: bool,
    /// Whether this cell is the VT100 cursor position.
    pub is_cursor: bool,
    /// Whether this cell is part of a non-active find match (dim highlight).
    pub is_find_match: bool,
    /// Whether this cell is part of the currently selected find match (bright highlight).
    pub is_find_active: bool,
}

/// A text selection range within the terminal content area.
#[derive(Debug, Clone)]
pub struct TermSelection {
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col: u16,
}

/// Data needed to render the integrated terminal bottom panel.
#[derive(Debug)]
pub struct TerminalPanel {
    /// Rendered cell grid: `rows[content_row][col]`
    pub rows: Vec<Vec<TerminalCell>>,
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
    pub split_left_rows: Option<Vec<Vec<TerminalCell>>>,
    /// Column count of the left pane in split view.
    pub split_left_cols: u16,
    /// Which pane has keyboard focus in split view: 0 = left, 1 = right.
    pub split_focus: u8,
    /// Whether the panel is currently maximized (fills editor area).
    /// Backends can render a different icon glyph based on this.
    pub maximized: bool,
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

/// Data for the visible menu bar strip and optional open dropdown.
#[derive(Debug)]
pub struct MenuBarData {
    /// Index (into `MENU_STRUCTURE`) of the currently open dropdown, or `None`.
    pub open_menu_idx: Option<usize>,
    /// Items in the currently open submenu (empty when no dropdown open).
    pub open_items: Vec<MenuItemData>,
    /// Approximate terminal column where the open menu header starts (for TUI anchor).
    pub open_menu_col: u16,
    /// Index into `open_items` of the keyboard-highlighted row, or `None`.
    pub highlighted_item_idx: Option<usize>,
    /// Title string shown to the right of menu labels (e.g. "VimCode — engine.rs").
    pub title: String,
    /// When true the backend should render its own window control buttons (─ ☐ ✕).
    /// Set to true by the GTK backend which uses `set_decorated(false)`.
    pub show_window_controls: bool,
    /// When true, use `vscode_shortcut` instead of `shortcut` for menu items.
    pub is_vscode_mode: bool,
    /// Whether the back navigation arrow is enabled (history available).
    pub nav_back_enabled: bool,
    /// Whether the forward navigation arrow is enabled (history available).
    pub nav_forward_enabled: bool,
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
    /// Activity bar (sidebar icon strip) — rendered by backends, not in ScreenLayout directly.
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
    /// Must call: `engine.tab_drag_begin()`, `engine.tab_drag_drop(zone)`
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
    if layout.menu_bar.is_some() {
        elems.push(UiElement::MenuBar);
        if layout
            .menu_bar
            .as_ref()
            .is_some_and(|m| m.open_menu_idx.is_some())
        {
            elems.push(UiElement::MenuDropdown);
        }
    }

    // Tab bar(s)
    if let Some(ref split) = layout.editor_group_split {
        for (i, _gtb) in split.group_tab_bars.iter().enumerate() {
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
    if let Some(ref split) = layout.editor_group_split {
        for gtb in &split.group_tab_bars {
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
    if layout.menu_bar.is_some() {
        elems.push(UiElement::MenuBar);
    }

    // draw_frame(): tab bar(s)
    if let Some(ref split) = layout.editor_group_split {
        for (i, _gtb) in split.group_tab_bars.iter().enumerate() {
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
    if layout
        .menu_bar
        .as_ref()
        .is_some_and(|m| m.open_menu_idx.is_some())
    {
        elems.push(UiElement::MenuDropdown);
    }

    // draw_tab_bar() / draw_group_tab_bar(): diff toolbar
    if layout.diff_toolbar.is_some() {
        elems.push(UiElement::DiffToolbar);
    }
    if let Some(ref split) = layout.editor_group_split {
        for gtb in &split.group_tab_bars {
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
    if layout.menu_bar.is_some() {
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
    if let Some(ref split) = layout.editor_group_split {
        for (i, _gtb) in split.group_tab_bars.iter().enumerate() {
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
    if let Some(ref split) = layout.editor_group_split {
        for gtb in &split.group_tab_bars {
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
    if layout
        .menu_bar
        .as_ref()
        .is_some_and(|m| m.open_menu_idx.is_some())
    {
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
    pub status_left: String,
    pub status_right: String,
    /// Byte range within `status_left` where the git branch name appears (for click detection).
    pub status_branch_range: Option<(usize, usize)>,
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
    pub menu_bar: Option<MenuBarData>,
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
    /// When the editor is split into two groups, this carries group 1's tab bar
    /// and split geometry. `None` in the default single-group mode.
    pub editor_group_split: Option<EditorGroupSplitData>,
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
    /// When `status_line_above_terminal` is active AND the terminal panel is open,
    /// this carries the active window's status line to render as a dedicated row
    /// above the terminal panel. When `Some`, per-window `status_line` fields on
    /// individual `RenderedWindow`s are `None`.
    pub separated_status_line: Option<WindowStatusLine>,
}

/// Context menu data for TUI rendering.
#[derive(Debug, Clone)]
pub struct ContextMenuPanel {
    pub items: Vec<ContextMenuRenderItem>,
    pub selected_idx: usize,
    pub screen_col: u16,
    pub screen_row: u16,
}

/// A single rendered context menu item.
#[derive(Debug, Clone)]
pub struct ContextMenuRenderItem {
    pub label: String,
    pub shortcut: String,
    pub separator_after: bool,
    pub enabled: bool,
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
        body: quadraui::StyledText::plain(panel.body.join("\n")),
        buttons,
        severity: None,
        vertical_buttons: panel.vertical_buttons,
        input: panel.input.as_ref().map(|inp| quadraui::DialogInput {
            value: inp.display.clone(),
            placeholder: String::new(),
            cursor: None,
        }),
    }
}

/// Render data for a dialog text input field.
#[derive(Debug, Clone)]
pub struct DialogInputPanel {
    /// Display text (masked for passwords).
    pub display: String,
}

// Re-export hit-test types and functions from engine so backends can use `render::*`.
pub use crate::core::engine::{
    compute_find_replace_hit_regions, FindReplaceClickTarget, FrHitRegion, FR_PANEL_WIDTH,
};

/// The inline find/replace overlay displayed at the top-right of the active editor group.
#[derive(Debug, Clone)]
pub struct FindReplacePanel {
    /// Current query text in the find field.
    pub query: String,
    /// Current replacement text (only shown when `show_replace` is true).
    pub replacement: String,
    /// Whether the replace row is visible.
    pub show_replace: bool,
    /// Which field has focus: 0 = find, 1 = replace.
    pub focus: u8,
    /// Cursor position within the focused field (char offset).
    pub cursor: usize,
    /// Selection anchor in the focused field. When Some, text between anchor and cursor is selected.
    pub sel_anchor: Option<usize>,
    /// "N of M" match count display, or "No results" / empty.
    pub match_info: String,
    /// Toggle button states (find row).
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    /// Toggle button states (replace row).
    pub preserve_case: bool,
    /// Find in selection mode.
    pub in_selection: bool,
    /// Bounding rect of the active editor group (pixel coords for GTK/Win-GUI,
    /// approximate for TUI). The overlay positions itself at the top-right of this rect.
    pub group_bounds: WindowRect,
    /// Panel width in char cells (used by backends for positioning).
    pub panel_width: u16,
    /// Hit regions for click handling, in char-cell units relative to the panel
    /// content corner (inside borders). Computed once in `build_screen_layout()`.
    pub hit_regions: Vec<(FrHitRegion, FindReplaceClickTarget)>,
}

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
    // active window's status into a separated bar rendered below the terminal.
    // When the setting is ON (default), per-window status bars stay inside each
    // window — they're naturally above the terminal by being part of the editor area.
    let separate_status =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open;

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

    let (status_left, status_right, status_branch_range) = if per_window_status {
        (String::new(), String::new(), None)
    } else {
        build_status_line(engine)
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

    let menu_bar = engine.menu_bar_visible.then(|| {
        let open_items = if let Some(midx) = engine.menu_open_idx {
            if let Some((_, _, items)) = MENU_STRUCTURE.get(midx) {
                items.to_vec()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        // Compute approximate column position of the active menu header for dropdown anchor.
        let open_menu_col: u16 = if let Some(midx) = engine.menu_open_idx {
            // Hamburger (3) + spaces between labels: each label ~5-8 chars
            let mut col: u16 = 3; // hamburger icon width
            for i in 0..midx {
                if let Some((name, _, _)) = MENU_STRUCTURE.get(i) {
                    col += name.len() as u16 + 2; // label + 2 spaces
                }
            }
            col
        } else {
            0
        };
        // Use workspace directory name (not active file) so the centered
        // search box stays fixed when switching tabs (like VSCode Command Center).
        let title = engine
            .cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "VimCode".to_string());
        MenuBarData {
            open_menu_idx: engine.menu_open_idx,
            open_items,
            open_menu_col,
            highlighted_item_idx: engine.menu_highlighted_item,
            title,
            show_window_controls: false, // GTK backend overrides this
            is_vscode_mode: engine.is_vscode_mode(),
            nav_back_enabled: engine.tab_nav_can_go_back(),
            nav_forward_enabled: engine.tab_nav_can_go_forward(),
        }
    });

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
    let editor_group_split = if n >= 2 {
        // Build group rects using a dummy content_bounds — backends will compute
        // their own actual rects, but we need the bounds here for GroupTabBar.
        // The caller supplies window_rects which already reflect actual bounds.
        let group_ids = engine.group_layout.group_ids();
        // Compute group bounds from the window_rects: each group's bounds is
        // the bounding box of its windows, expanded upward by line_height for tab bar.
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
                let bar_width = bounds.width as u16;
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
                GroupTabBar {
                    group_id: gid,
                    tabs,
                    bounds,
                    diff_toolbar,
                    tab_scroll_offset,
                    hit_regions,
                }
            })
            .collect();
        // Collect dividers — use the total content bounds from window_rects
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
        let dividers = engine.group_layout.dividers(content_bounds, &mut 0);
        Some(EditorGroupSplitData {
            group_tab_bars,
            active_group: engine.active_group,
            dividers,
            num_groups: n,
        })
    } else {
        None
    };

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
                let bounds = WindowRect::new(min_x, min_y, max_x - min_x, line_height);
                BreadcrumbBar {
                    group_id: gid,
                    segments,
                    bounds,
                }
            })
            .collect()
    } else {
        vec![]
    };

    // Compute diff toolbar for single-group mode (multi-group has it on GroupTabBar).
    let diff_toolbar = if editor_group_split.is_none() && engine.is_in_diff_view() {
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

    ScreenLayout {
        tab_bar,
        windows,
        status_left,
        status_right,
        status_branch_range,
        command,
        wildmenu,
        active_window_id,
        completion,
        hover,
        quickfix,
        bottom_tabs,
        signature_help,
        menu_bar,
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
                group_bounds: active_group_bounds,
                panel_width: panel_w,
                hit_regions,
            })
        } else {
            None
        },
        tab_tooltip: engine.tab_hover_tooltip.clone(),
        tab_scroll_offset: engine
            .editor_groups
            .get(&engine.active_group)
            .map(|g| g.tab_scroll_offset)
            .unwrap_or(0),
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
    })
}

/// Convert vimcode's internal `Color` to quadraui's `Color`. Alpha is fully opaque.
fn to_q_color(c: Color) -> quadraui::Color {
    quadraui::Color::rgb(c.r, c.g, c.b)
}

/// Adapt a `SourceControlData` (vimcode's internal representation) into a
/// generic `quadraui::TreeView` that backends can render through the shared
/// tree-primitive drawing path.
///
/// Scope: covers the four expandable sections only — Staged, Changes,
/// Worktrees, and Recent Commits. The header row, commit input, and action
/// button row remain the responsibility of the existing SC panel code;
/// they will migrate in later A.x stages when their primitives land.
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

/// One visible row in the flat explorer file-tree list. Shared across all
/// backends; each backend maintains its own `Vec<ExplorerRow>` alongside
/// scroll / selection state and calls `explorer_to_tree_view` once per
/// frame to build the `quadraui::TreeView` the shared `draw_tree`
/// primitive consumes.
#[derive(Debug, Clone)]
pub struct ExplorerRow {
    pub depth: usize,
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_dir: bool,
    pub is_expanded: bool,
}

/// Adapt a flat explorer row list into a `quadraui::TreeView` for the
/// shared `draw_tree` primitive. Each backend drives its own flat-row
/// model (GTK via `ExplorerState`, Win-GUI via `WinSidebar`) and calls
/// this adapter on every draw.
///
/// Overlays per-row git status letters and LSP diagnostic counts via
/// `engine.explorer_indicators()` — the cached indicator map keyed by
/// canonical path. Directories get a folder glyph; files get the
/// extension-based icon from `icons::file_icon`.
pub fn explorer_to_tree_view(
    rows: &[ExplorerRow],
    scroll_top: usize,
    selected: usize,
    has_focus: bool,
    engine: &Engine,
) -> quadraui::TreeView {
    use quadraui::{
        Badge, Decoration, Icon as QIcon, SelectionMode, StyledText, TreeRow, TreeStyle, TreeView,
        WidgetId,
    };

    let (git_statuses, diag_counts) = engine.explorer_indicators();

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
                Some(Badge::plain(if errors > 9 {
                    "9+".to_string()
                } else {
                    errors.to_string()
                }))
            } else if warnings > 0 {
                Some(Badge::plain(if warnings > 9 {
                    "9+".to_string()
                } else {
                    warnings.to_string()
                }))
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
        });
    }

    let selected_path = if selected < out.len() {
        Some(vec![selected as u16])
    } else {
        None
    };

    TreeView {
        id: WidgetId::new("explorer-tree"),
        rows: out,
        selection_mode: SelectionMode::Single,
        selected_path,
        scroll_offset: scroll_top,
        style: TreeStyle::default(),
        has_focus,
    }
}

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
pub fn picker_panel_to_palette(picker: &PickerPanel) -> Option<quadraui::Palette> {
    use quadraui::{Palette, PaletteItem, StyledText, WidgetId};

    if picker.preview.is_some() {
        return None;
    }
    if picker.items.iter().any(|it| it.depth > 0 || it.expandable) {
        return None;
    }

    let items: Vec<PaletteItem> = picker
        .items
        .iter()
        .map(|it| PaletteItem {
            text: StyledText::plain(&it.display),
            detail: it.detail.as_deref().map(StyledText::plain),
            icon: None,
            match_positions: it.match_positions.clone(),
        })
        .collect();

    Some(Palette {
        id: WidgetId::new("picker"),
        title: picker.title.clone(),
        query: picker.query.clone(),
        query_cursor: picker.query.len(),
        items,
        selected_idx: picker.selected_idx,
        scroll_offset: picker.scroll_top,
        total_count: picker.total_count,
        has_focus: true,
    })
}

/// Convert `Engine`'s settings state into a generic `quadraui::Form`
/// for rendering through either `quadraui_tui::draw_form` (A.3b) or
/// `quadraui_gtk::draw_form` (A.3c). Backend-agnostic; reads only
/// engine fields.
///
/// Scope: covers the scrollable field list. Callers still handle the
/// panel header / search input / scrollbar themselves, and fall back
/// to a legacy inline renderer when `settings_editing.is_some()` or
/// `ext_settings_editing.is_some()` because the `Form` primitive does
/// not yet render an editable `TextInput` for inline-edit mode (the
/// cursor-aware primitive support landed in A.3d but the adapter has
/// not been upgraded to emit `TextInput` for active-edit fields —
/// tracked as a future refinement).
///
/// Field type mapping:
/// - `CoreCategory` / `ExtCategory` → `FieldKind::Label` (collapsible header)
/// - `CoreSetting` with `Bool` → `FieldKind::Toggle`
/// - `CoreSetting` with any other type → `FieldKind::ReadOnly`
///   (enum cycling / numeric / string values still work — keys are
///   handled by `engine.handle_settings_key()`; the adapter just shows
///   the current value)
/// - `ExtSetting` mapped analogously via the manifest's declared type.
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
                }
            }
            SettingsRow::CoreSetting(idx) => {
                let def = &SETTING_DEFS[*idx];
                let value_str = engine.settings.get_value_str(def.key);
                let kind = match def.setting_type {
                    SettingType::Bool => FieldKind::Toggle {
                        value: value_str == "true",
                    },
                    _ => FieldKind::ReadOnly {
                        value: StyledText::plain(value_str),
                    },
                };
                FormField {
                    id: WidgetId::new(format!("setting-{}", idx)),
                    label: StyledText::plain(def.label),
                    kind,
                    hint: StyledText::default(),
                    disabled: false,
                }
            }
            SettingsRow::ExtSetting(ext_name, key) => {
                let def_opt = engine.find_ext_setting_def(ext_name, key);
                let value_str = engine.get_ext_setting(ext_name, key);
                let (label_str, kind) = if let Some(d) = def_opt {
                    let label = if d.label.is_empty() {
                        key.clone()
                    } else {
                        d.label.clone()
                    };
                    let kind = if d.r#type == "bool" {
                        FieldKind::Toggle {
                            value: value_str == "true",
                        }
                    } else {
                        FieldKind::ReadOnly {
                            value: StyledText::plain(value_str),
                        }
                    };
                    (label, kind)
                } else {
                    (
                        key.clone(),
                        FieldKind::ReadOnly {
                            value: StyledText::plain(value_str),
                        },
                    )
                };
                FormField {
                    id: WidgetId::new(format!("ext-setting-{}-{}", ext_name, key)),
                    label: StyledText::plain(label_str),
                    kind,
                    hint: StyledText::default(),
                    disabled: false,
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
                .filter_map(|idx| all_items.get(idx).cloned())
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

/// Map a vt100 color to an RGB triple.
/// Falls back to reasonable defaults for the OneDark theme.
fn map_vt100_color(color: vt100::Color, is_bg: bool) -> (u8, u8, u8) {
    match color {
        vt100::Color::Default => {
            if is_bg {
                (30, 30, 30) // terminal background (~#1e1e1e)
            } else {
                (229, 229, 229) // terminal foreground (~#e5e5e5)
            }
        }
        vt100::Color::Rgb(r, g, b) => (r, g, b),
        vt100::Color::Idx(n) => xterm_256_color(n),
    }
}

/// Standard xterm 256-color palette lookup.
fn xterm_256_color(n: u8) -> (u8, u8, u8) {
    // Colors 0-15: system colors (approximate)
    const SYSTEM: [(u8, u8, u8); 16] = [
        (0, 0, 0),       // 0: Black
        (128, 0, 0),     // 1: Maroon
        (0, 128, 0),     // 2: Green
        (128, 128, 0),   // 3: Olive
        (0, 0, 128),     // 4: Navy
        (128, 0, 128),   // 5: Purple
        (0, 128, 128),   // 6: Teal
        (192, 192, 192), // 7: Silver
        (128, 128, 128), // 8: Grey
        (255, 0, 0),     // 9: Red
        (0, 255, 0),     // 10: Lime
        (255, 255, 0),   // 11: Yellow
        (0, 0, 255),     // 12: Blue
        (255, 0, 255),   // 13: Fuchsia
        (0, 255, 255),   // 14: Aqua
        (255, 255, 255), // 15: White
    ];
    if n < 16 {
        return SYSTEM[n as usize];
    }
    // Colors 16-231: 6×6×6 color cube
    if n < 232 {
        let idx = n - 16;
        let b = idx % 6;
        let g = (idx / 6) % 6;
        let r = idx / 36;
        let to_byte = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        return (to_byte(r), to_byte(g), to_byte(b));
    }
    // Colors 232-255: grayscale
    let gray = 8 + (n - 232) * 10;
    (gray, gray, gray)
}

/// Normalize a terminal selection so start ≤ end in reading order.
fn normalize_term_selection(sel: &CoreTermSelection) -> (u16, u16, u16, u16) {
    if (sel.start_row, sel.start_col) <= (sel.end_row, sel.end_col) {
        (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
    } else {
        (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
    }
}

/// Build the cell grid for a single terminal pane.
///
/// `cursor_active` controls whether the VT100 cursor position is highlighted.
/// `find` carries per-match highlighting data; pass `None` for the inactive pane.
#[allow(clippy::type_complexity)]
fn build_pane_rows(
    term: &crate::core::terminal::TerminalPane,
    cursor_active: bool,
    find: Option<(&[(usize, u16, u16)], usize, usize)>, // (matches, qlen, active_idx)
) -> Vec<Vec<TerminalCell>> {
    let screen = term.parser.screen();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let rows_count = term.rows as usize;
    let cols_count = term.cols as usize;
    let scroll_offset = term.scroll_offset;
    let hist_len = term.history.len();

    let sel_bounds = if scroll_offset == 0 {
        term.selection.as_ref().map(normalize_term_selection)
    } else {
        None
    };

    let mut rows: Vec<Vec<TerminalCell>> = (0..rows_count)
        .map(|display_r| {
            (0..cols_count)
                .map(|c| {
                    let cu = c as u16;
                    let (ch, fg, bg, bold, italic, underline, is_cursor, selected) = if display_r
                        < scroll_offset
                    {
                        let hist_idx_signed =
                            hist_len as isize - scroll_offset as isize + display_r as isize;
                        if hist_idx_signed >= 0 {
                            let hist_idx = hist_idx_signed as usize;
                            if let Some(hist_row) = term.history.get(hist_idx) {
                                let hc = hist_row.get(c).copied().unwrap_or_default();
                                (
                                    hc.ch,
                                    map_vt100_color(hc.fg, false),
                                    map_vt100_color(hc.bg, true),
                                    hc.bold,
                                    hc.italic,
                                    hc.underline,
                                    false,
                                    false,
                                )
                            } else {
                                (
                                    ' ',
                                    (229, 229, 229),
                                    (30, 30, 30),
                                    false,
                                    false,
                                    false,
                                    false,
                                    false,
                                )
                            }
                        } else {
                            (
                                ' ',
                                (229, 229, 229),
                                (30, 30, 30),
                                false,
                                false,
                                false,
                                false,
                                false,
                            )
                        }
                    } else {
                        let live_r = (display_r - scroll_offset) as u16;
                        let cell_opt = screen.cell(live_r, cu);
                        let (ch, fg, bg, bold, italic, underline) = if let Some(cell) = cell_opt {
                            let contents = cell.contents();
                            let ch = contents.chars().next().unwrap_or(' ');
                            (
                                ch,
                                map_vt100_color(cell.fgcolor(), false),
                                map_vt100_color(cell.bgcolor(), true),
                                cell.bold(),
                                cell.italic(),
                                cell.underline(),
                            )
                        } else {
                            (' ', (229, 229, 229), (30, 30, 30), false, false, false)
                        };
                        let is_cursor = scroll_offset == 0
                            && cursor_active
                            && live_r == cursor_row
                            && cu == cursor_col;
                        let selected = sel_bounds.is_some_and(|(r0, c0, r1, c1)| {
                            if r0 == r1 {
                                live_r == r0 && cu >= c0 && cu <= c1
                            } else if live_r == r0 {
                                cu >= c0
                            } else if live_r == r1 {
                                cu <= c1
                            } else {
                                live_r > r0 && live_r < r1
                            }
                        });
                        (ch, fg, bg, bold, italic, underline, is_cursor, selected)
                    };

                    TerminalCell {
                        ch,
                        fg,
                        bg,
                        bold,
                        italic,
                        underline,
                        selected,
                        is_cursor,
                        is_find_match: false,
                        is_find_active: false,
                    }
                })
                .collect()
        })
        .collect();

    // Apply find match highlights when provided.
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
        let left_pane = &engine.terminal_panes[0];
        let right_pane = &engine.terminal_panes[1];
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
            content_rows: right_pane.rows,
            content_cols: right_pane.cols,
            has_focus: engine.terminal_has_focus,
            scroll_offset: active_pane.scroll_offset,
            scrollback_rows: active_pane.history.len(),
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
                left_pane.cols
            },
            split_focus: engine.terminal_active as u8,
            maximized: engine.terminal_maximized,
        });
    }

    // ── Single-pane (normal) view ──────────────────────────────────────────────
    let term = engine.active_terminal()?;
    let hist_len = term.history.len();
    let scroll_offset = term.scroll_offset;
    let cursor_active = engine.terminal_has_focus;
    let rows = build_pane_rows(term, cursor_active, find_data);

    Some(TerminalPanel {
        rows,
        content_rows: term.rows,
        content_cols: term.cols,
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
    let mut aligned_idx: usize = if let Some(aligned) = diff_aligned {
        // Find the aligned entry corresponding to scroll_top.
        aligned
            .iter()
            .position(|e| e.source_line.is_some_and(|sl| sl >= scroll_top))
            .unwrap_or(0)
    } else {
        0
    };
    let mut line_idx = scroll_top;
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
                    (Some(format!("{}… ", label)), theme.status_inactive_fg)
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

// ─── quadraui::Terminal adapter (A.7) ────────────────────────────────────────

/// Convert a vimcode `TerminalCell` row grid into a `quadraui::Terminal`
/// snapshot. Used by both TUI (`render_terminal_pane_cells`) and GTK
/// (`draw_terminal_cells`) so the per-cell rendering path is shared.
///
/// The conversion is a 1:1 mapping — render-side cells already carry the
/// overlay flags (selected, is_cursor, is_find_match, is_find_active) the
/// primitive expects.
pub fn terminal_cells_to_quadraui(
    rows: &[Vec<TerminalCell>],
    id: quadraui::WidgetId,
) -> quadraui::Terminal {
    let cells = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| quadraui::TerminalCell {
                    ch: c.ch,
                    fg: quadraui::Color::rgb(c.fg.0, c.fg.1, c.fg.2),
                    bg: quadraui::Color::rgb(c.bg.0, c.bg.1, c.bg.2),
                    bold: c.bold,
                    italic: c.italic,
                    underline: c.underline,
                    selected: c.selected,
                    is_cursor: c.is_cursor,
                    is_find_match: c.is_find_match,
                    is_find_active: c.is_find_active,
                })
                .collect()
        })
        .collect();
    quadraui::Terminal { id, cells }
}

// ─── quadraui::TabBar adapter (A.6c / A.6d) ──────────────────────────────────

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
        })
        .collect();

    let mut right: Vec<quadraui::TabBarSegment> = Vec::new();

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
        right.push(quadraui::TabBarSegment {
            text: " \u{F0143}".to_string(),
            width_cells: 3,
            id: Some(quadraui::WidgetId::new("tab:diff_prev")),
            is_active: false,
        });
        right.push(quadraui::TabBarSegment {
            text: " \u{F0140}".to_string(),
            width_cells: 3,
            id: Some(quadraui::WidgetId::new("tab:diff_next")),
            is_active: false,
        });
        right.push(quadraui::TabBarSegment {
            text: " \u{F0233}".to_string(),
            width_cells: 3,
            id: Some(quadraui::WidgetId::new("tab:diff_toggle")),
            is_active: dt.unchanged_hidden,
        });
    }

    if show_split_btns {
        right.push(quadraui::TabBarSegment {
            text: " \u{F0932}".to_string(),
            width_cells: 3,
            id: Some(quadraui::WidgetId::new("tab:split_right")),
            is_active: false,
        });
        let split_down = crate::icons::SPLIT_DOWN.c();
        right.push(quadraui::TabBarSegment {
            text: format!(" {split_down} "),
            width_cells: 3,
            id: Some(quadraui::WidgetId::new("tab:split_down")),
            is_active: false,
        });
    }

    right.push(quadraui::TabBarSegment {
        text: " \u{22EF} ".to_string(),
        width_cells: 3,
        id: Some(quadraui::WidgetId::new("tab:action_menu")),
        is_active: false,
    });

    quadraui::TabBar {
        id: quadraui::WidgetId::new("tabs:group"),
        tabs: tab_items,
        scroll_offset: tab_scroll_offset,
        right_segments: right,
        active_accent,
    }
}

/// Convert a vimcode `Color` into a `quadraui::Color`. Used by GTK to pass
/// the theme accent colour into `build_tab_bar_primitive`.
pub fn to_quadraui_color(c: Color) -> quadraui::Color {
    quadraui::Color::rgb(c.r, c.g, c.b)
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

/// Compute the Y coordinate of the editor bottom edge (below which status/terminal/etc live).
#[allow(clippy::too_many_arguments)]
pub fn editor_bottom_px(
    total_height: f64,
    line_height: f64,
    per_window_status_line: bool,
    has_wildmenu: bool,
    quickfix_open: bool,
    quickfix_item_count: usize,
    panel_open: bool,
    panel_rows: usize,
    debug_toolbar_visible: bool,
    has_separated_status: bool,
) -> f64 {
    total_height
        - status_bar_height_px(line_height, per_window_status_line, has_wildmenu)
        - quickfix_height_px(line_height, quickfix_open, quickfix_item_count)
        - terminal_panel_height_px(line_height, panel_open, panel_rows)
        - debug_toolbar_height_px(line_height, debug_toolbar_visible)
        - separated_status_height_px(line_height, has_separated_status)
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
        let split = layout
            .editor_group_split
            .expect("should have editor group split");
        assert!(
            split.group_tab_bars.len() >= 2,
            "should have 2+ group tab bars"
        );
        for gtb in &split.group_tab_bars {
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

        // Global status bar should be empty
        assert!(layout.status_left.is_empty());
        assert!(layout.status_right.is_empty());
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
        assert!(!layout.status_left.is_empty());
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
    fn test_editor_bottom_px() {
        let lh = 20.0;
        let total = 800.0;
        let eb = editor_bottom_px(total, lh, true, false, false, 0, false, 0, false, false);
        assert_eq!(eb, total - lh); // only status bar (1 row)
    }

    #[test]
    fn test_editor_bottom_px_with_separated_status() {
        let lh = 20.0;
        let total = 800.0;
        // With separated status, editor bottom is 1 extra row lower
        let without = editor_bottom_px(total, lh, true, false, false, 0, true, 10, false, false);
        let with = editor_bottom_px(total, lh, true, false, false, 0, true, 10, false, true);
        assert_eq!(without - with, lh);
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
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.mode = Mode::Normal;
        if !text.is_empty() {
            e.buffer_mut().insert(0, text);
        }
        e
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
        assert!(layout.menu_bar.is_some(), "menu bar should be visible");

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

    /// Phase 2c source-code verification: grep the Win-GUI source for the
    /// engine method calls required by each [`UiAction`]. This catches cases
    /// where a hand-curated list claims an action is handled but the actual
    /// engine call is missing from the source code.
    #[test]
    fn test_wingui_source_contains_required_calls() {
        // Read both Win-GUI source files (use CARGO_MANIFEST_DIR for stable path)
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let mod_path = std::path::Path::new(manifest_dir).join("src/win_gui/mod.rs");
        let draw_path = std::path::Path::new(manifest_dir).join("src/win_gui/draw.rs");
        let mod_src = std::fs::read_to_string(&mod_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", mod_path.display()));
        let draw_src = std::fs::read_to_string(&draw_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", draw_path.display()));
        let src = format!("{mod_src}\n{draw_src}");

        // Map each UiAction to the engine method call(s) that MUST appear in
        // the source.  Draw-order actions check draw function call order
        // instead of engine methods.
        let checks: Vec<(UiAction, &[&str])> = vec![
            (UiAction::ExplorerSingleClickFile, &["open_file_preview"]),
            (UiAction::ExplorerDoubleClickFile, &["open_file_in_tab"]),
            (UiAction::ExplorerEnterOnFile, &["open_file_in_tab"]),
            (
                UiAction::ExplorerRightClick,
                &["open_explorer_context_menu"],
            ),
            (UiAction::ContextMenuClickInside, &["context_menu_confirm"]),
            (UiAction::ContextMenuClickOutside, &["close_context_menu"]),
            (UiAction::TabClick, &["goto_tab"]),
            (UiAction::TabCloseClick, &["close_tab"]),
            (UiAction::TabRightClick, &["open_tab_context_menu"]),
            (UiAction::TabDragDrop, &["tab_drag_begin", "tab_drag_drop"]),
            (UiAction::EditorRightClick, &["open_editor_context_menu"]),
            (UiAction::EditorDoubleClick, &["mouse_double_click"]),
            // EditorScroll: scroll_down_visible/scroll_up_visible or
            // set_scroll_top_for_window — any is fine
            (UiAction::EditorScroll, &["scroll_down_visible"]),
            (UiAction::EditorHoverClick, &["editor_hover_focus"]),
            (UiAction::EditorHoverDismiss, &["dismiss_editor_hover"]),
            (UiAction::EditorHoverScroll, &["editor_hover_scroll"]),
            (UiAction::DebugToolbarButtonClick, &["execute_command"]),
            (UiAction::TerminalSplitButton, &["terminal_toggle_split"]),
            (UiAction::TerminalAddButton, &["terminal_new_tab"]),
            (
                UiAction::TerminalCloseButton,
                &["terminal_close_active_tab"],
            ),
            (
                UiAction::TerminalMaximizeButton,
                &["toggle_terminal_maximize"],
            ),
            (UiAction::TerminalSplitPaneClick, &["terminal_active"]),
            // Activity bar: check for panel toggle dispatch
            (UiAction::ActivityBarClick, &["active_panel"]),
            (UiAction::ActivityBarSettingsClick, &["Settings"]),
            // Draw order: verify draw sequence in on_paint / draw_frame
            (
                UiAction::DrawOrderContextMenuAboveSidebar,
                &["draw_context_menu", "draw_sidebar"],
            ),
            (UiAction::DrawOrderDialogOnTop, &["draw_dialog"]),
            (
                UiAction::DrawOrderMenuDropdownAboveSidebar,
                &["draw_menu_dropdown"],
            ),
        ];

        let mut missing = Vec::new();
        for (action, required_calls) in &checks {
            for call in *required_calls {
                if !src.contains(call) {
                    missing.push(format!(
                        "{action:?} requires `{call}` — not found in Win-GUI source"
                    ));
                }
            }
        }

        assert!(
            missing.is_empty(),
            "Win-GUI source missing required engine calls:\n  {}",
            missing.join("\n  ")
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
        e.tab_drag_begin(gid, 1); // drag f1's tab

        // Drop to create a vertical split
        e.tab_drag_drop(crate::core::window::DropZone::Split(
            gid,
            crate::core::window::SplitDirection::Vertical,
            false,
        ));
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
}
