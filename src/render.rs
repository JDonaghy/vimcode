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
pub use crate::core::settings::{SettingDef, SettingType, SETTING_DEFS};
use crate::core::terminal::TermSelection as CoreTermSelection;
use crate::core::view::View;
use crate::core::window::{GroupDivider, GroupId};
use crate::core::{Cursor, GitLineStatus, Mode, WindowId, WindowRect};

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

    /// Normalise to the (0.0..=1.0, 0.0..=1.0, 0.0..=1.0) triple expected by
    /// Cairo's `set_source_rgb` / `set_source_rgba`.
    pub fn to_cairo(self) -> (f64, f64, f64) {
        (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
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
    /// True when this is a soft-wrap continuation row (the 2nd+ visual row of a
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
}

/// One segment in the breadcrumb bar (either a path component or a symbol).
#[derive(Debug, Clone)]
pub struct BreadcrumbSegment {
    pub label: String,
    pub is_last: bool,
    pub is_symbol: bool,
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
    /// Transient yank-highlight region (flashes briefly after a yank). `None` if no active highlight.
    pub yank_highlight: Option<SelectionRange>,
    /// Bracket pair positions to highlight (cursor bracket + matching bracket).
    /// Each entry is (view_line, col). Up to 2 entries.
    pub bracket_match_positions: Vec<(usize, usize)>,
    /// The indent guide column that should be highlighted as "active" (cursor's scope).
    pub active_indent_col: Option<usize>,
    /// Tab stop width for expanding `\t` to spaces in TUI rendering.
    pub tabstop: usize,
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

// ─── FuzzyPanel ──────────────────────────────────────────────────────────────

/// Data needed to render the fuzzy file-picker modal.
#[derive(Debug, Clone)]
pub struct FuzzyPanel {
    /// Current query string typed by the user.
    pub query: String,
    /// Display paths for the filtered results (up to 50).
    pub results: Vec<String>,
    /// Index of the currently highlighted result.
    pub selected_idx: usize,
    /// Total number of files in the project (for the status line).
    pub total_files: usize,
}

// ─── LiveGrepPanel ────────────────────────────────────────────────────────────

/// Data needed to render the live grep modal.
#[derive(Debug, Clone)]
pub struct LiveGrepPanel {
    /// Current query typed by the user.
    pub query: String,
    /// Result display strings: "basename.rs:N: snippet text"
    pub results: Vec<String>,
    /// Index of the currently highlighted result.
    pub selected_idx: usize,
    /// Total number of matched lines.
    pub total_matches: usize,
    /// Preview lines: (1-based line number, text, is_match_line)
    pub preview_lines: Vec<(usize, String, bool)>,
}

// ─── CommandPalettePanel ──────────────────────────────────────────────────────

/// Data needed to render the command palette modal.
#[derive(Debug, Clone)]
pub struct CommandPalettePanel {
    /// Current query typed by the user.
    pub query: String,
    /// Filtered command list: (label, shortcut) display pairs.
    pub items: Vec<(String, String)>,
    /// Index of the currently highlighted result.
    pub selected_idx: usize,
    /// Scroll offset into the filtered list.
    pub scroll_top: usize,
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
    /// True when the commit input row is in edit mode.
    pub commit_input_active: bool,
    /// Which action button is keyboard-focused (0=Commit 1=Push 2=Pull 3=Sync), or None.
    pub button_focused: Option<usize>,
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
}

/// A single section within an extension panel.
#[derive(Debug, Clone)]
pub struct ExtPanelSectionData {
    pub name: String,
    pub items: Vec<crate::core::plugin::ExtPanelItem>,
    pub expanded: bool,
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
                vscode_shortcut: "",
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
                action: "term",
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
                action: "term",
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
                action: "keys",
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
pub static DEBUG_BUTTONS: &[DebugButton] = &[
    DebugButton {
        icon: "\u{f040a}",
        label: "Continue",
        key_hint: "F5",
        action: "continue",
        enabled: true,
    },
    DebugButton {
        icon: "\u{f03e4}",
        label: "Pause",
        key_hint: "F6",
        action: "pause",
        enabled: true,
    },
    DebugButton {
        icon: "\u{f04db}",
        label: "Stop",
        key_hint: "Shift+F5",
        action: "stop",
        enabled: true,
    },
    DebugButton {
        icon: "\u{f0459}",
        label: "Restart",
        key_hint: "Ctrl+Shift+F5",
        action: "restart",
        enabled: true,
    },
    // separator goes here (rendered between index 3 and 4)
    DebugButton {
        icon: "\u{f0457}",
        label: "Step Over",
        key_hint: "F10",
        action: "stepover",
        enabled: true,
    },
    DebugButton {
        icon: "\u{f0459}",
        label: "Step Into",
        key_hint: "F11",
        action: "stepin",
        enabled: true,
    },
    DebugButton {
        icon: "\u{f0458}",
        label: "Step Out",
        key_hint: "Shift+F11",
        action: "stepout",
        enabled: true,
    },
];

// ─── ScreenLayout ─────────────────────────────────────────────────────────────

/// The complete, platform-agnostic description of one editor frame.
/// Build it with [`build_screen_layout`], then hand it to the backend renderer.
#[derive(Debug)]
pub struct ScreenLayout {
    pub tab_bar: Vec<TabInfo>,
    pub windows: Vec<RenderedWindow>,
    pub status_left: String,
    pub status_right: String,
    pub command: CommandLineData,
    /// Wildmenu bar (Tab completion in command mode), or `None` when inactive.
    pub wildmenu: Option<WildmenuData>,
    pub active_window_id: WindowId,
    /// Completion popup to show, or `None` when inactive.
    pub completion: Option<CompletionMenu>,
    /// Hover information popup, or `None` when inactive.
    pub hover: Option<HoverPopup>,
    /// Fuzzy file-picker modal, or `None` when inactive.
    pub fuzzy: Option<FuzzyPanel>,
    /// Live grep modal, or `None` when inactive.
    pub live_grep: Option<LiveGrepPanel>,
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
    /// Command palette modal — `Some` when open.
    pub command_palette: Option<CommandPalettePanel>,
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
    /// Git diff peek popup — `Some` when the user is previewing a diff hunk.
    pub diff_peek: Option<DiffPeekPopup>,
    /// Diff toolbar data for the single-group tab bar.
    pub diff_toolbar: Option<DiffToolbarData>,
    /// Modal dialog popup — `Some` when a dialog is open.
    pub dialog: Option<DialogPanel>,
    /// Context menu popup — `Some` when an engine context menu is open.
    pub context_menu: Option<ContextMenuPanel>,
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
}

/// Format a button label with the hotkey character bracketed.
/// e.g., `format_button_label("Recover", 'r')` → `"[R]ecover"`.
pub fn format_button_label(label: &str, hotkey: char) -> String {
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

    // Status line
    pub status_bg: Color,
    pub status_fg: Color,

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

    // Two-way diff background colours
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_padding_bg: Color,

    // DAP stopped-line highlight
    pub dap_stopped_bg: Color,

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

    // Bracket match highlight
    pub bracket_match_bg: Color,
}

impl Theme {
    /// The OneDark-inspired colour scheme currently used by VimCode.
    /// All values are derived directly from the Cairo RGB tuples in the
    /// original `draw_*` functions.
    pub fn onedark() -> Self {
        Self {
            // (0.1, 0.1, 0.1)
            background: Color::from_hex("#1a1a1a"),
            // (0.12, 0.12, 0.12)
            active_background: Color::from_hex("#1e1e1e"),
            // (0.9, 0.9, 0.9)
            foreground: Color::from_hex("#e5e5e5"),

            keyword: Color::from_hex("#c678dd"),
            string_lit: Color::from_hex("#98c379"),
            comment: Color::from_hex("#5c6370"),
            function: Color::from_hex("#61afef"),
            type_name: Color::from_hex("#e5c07b"),
            variable: Color::from_hex("#e06c75"),
            number: Color::from_hex("#d19a66"),
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

            // (0.2, 0.2, 0.3)
            status_bg: Color::from_hex("#33334c"),
            // (0.9, 0.9, 0.9)
            status_fg: Color::from_hex("#e5e5e5"),

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

            // Two-way diff backgrounds — must be clearly green/red in terminals
            diff_added_bg: Color::from_hex("#14541a"),
            diff_removed_bg: Color::from_hex("#541a1a"),
            diff_padding_bg: Color::from_hex("#2d2d2d"),

            // DAP stopped-line (dark amber)
            dap_stopped_bg: Color::from_hex("#3a3000"),

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

            sidebar_sel_bg: Color::from_hex("#2c313a"), // focused: subtle highlight
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
            bracket_match_bg: Color::from_hex("#3a3d41"),
        }
    }

    /// Gruvbox Dark colour scheme.
    pub fn gruvbox_dark() -> Self {
        Self {
            background: Color::from_hex("#282828"),
            active_background: Color::from_hex("#32302f"),
            foreground: Color::from_hex("#ebdbb2"),

            keyword: Color::from_hex("#fb4934"),
            string_lit: Color::from_hex("#b8bb26"),
            comment: Color::from_hex("#928374"),
            function: Color::from_hex("#8ec07c"),
            type_name: Color::from_hex("#fabd2f"),
            variable: Color::from_hex("#83a598"),
            number: Color::from_hex("#d3869b"),
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

            status_bg: Color::from_hex("#504945"),
            status_fg: Color::from_hex("#ebdbb2"),

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

            hover_bg: Color::from_hex("#32302f"),
            hover_fg: Color::from_hex("#ebdbb2"),
            hover_border: Color::from_hex("#458588"),

            fuzzy_bg: Color::from_hex("#32302f"),
            fuzzy_selected_bg: Color::from_hex("#504945"),
            fuzzy_fg: Color::from_hex("#ebdbb2"),
            fuzzy_query_fg: Color::from_hex("#8ec07c"),
            fuzzy_border: Color::from_hex("#458588"),
            fuzzy_title_fg: Color::from_hex("#fabd2f"),

            // (bg #282828)
            diff_added_bg: Color::from_hex("#1e5e24"),
            diff_removed_bg: Color::from_hex("#5e2424"),
            diff_padding_bg: Color::from_hex("#333333"),

            dap_stopped_bg: Color::from_hex("#3a3000"),

            yank_highlight_bg: Color::from_hex("#b8bb26"),
            yank_highlight_alpha: 0.35,

            annotation_fg: Color::from_hex("#928374"),
            ghost_text_fg: Color::from_hex("#7c6f64"),

            md_heading1: Color::from_hex("#fabd2f"),
            md_heading2: Color::from_hex("#83a598"),
            md_heading3: Color::from_hex("#d3869b"),
            md_code: Color::from_hex("#b8bb26"),
            md_link: Color::from_hex("#83a598"),

            sidebar_sel_bg: Color::from_hex("#3c3836"), // focused
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
            bracket_match_bg: Color::from_hex("#504945"),
        }
    }

    /// Tokyo Night colour scheme.
    pub fn tokyo_night() -> Self {
        Self {
            background: Color::from_hex("#1a1b26"),
            active_background: Color::from_hex("#1f2335"),
            foreground: Color::from_hex("#c0caf5"),

            keyword: Color::from_hex("#bb9af7"),
            string_lit: Color::from_hex("#9ece6a"),
            comment: Color::from_hex("#565f89"),
            function: Color::from_hex("#7aa2f7"),
            type_name: Color::from_hex("#e0af68"),
            variable: Color::from_hex("#f7768e"),
            number: Color::from_hex("#ff9e64"),
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

            status_bg: Color::from_hex("#292e42"),
            status_fg: Color::from_hex("#c0caf5"),

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

            hover_bg: Color::from_hex("#1f2335"),
            hover_fg: Color::from_hex("#c0caf5"),
            hover_border: Color::from_hex("#7aa2f7"),

            fuzzy_bg: Color::from_hex("#1f2335"),
            fuzzy_selected_bg: Color::from_hex("#364a82"),
            fuzzy_fg: Color::from_hex("#c0caf5"),
            fuzzy_query_fg: Color::from_hex("#7aa2f7"),
            fuzzy_border: Color::from_hex("#7aa2f7"),
            fuzzy_title_fg: Color::from_hex("#e0af68"),

            // (bg #1a1b26)
            diff_added_bg: Color::from_hex("#14541a"),
            diff_removed_bg: Color::from_hex("#541a28"),
            diff_padding_bg: Color::from_hex("#252530"),

            dap_stopped_bg: Color::from_hex("#2a2500"),

            yank_highlight_bg: Color::from_hex("#9ece6a"),
            yank_highlight_alpha: 0.35,

            annotation_fg: Color::from_hex("#565f89"),
            ghost_text_fg: Color::from_hex("#414868"),

            md_heading1: Color::from_hex("#e0af68"),
            md_heading2: Color::from_hex("#7aa2f7"),
            md_heading3: Color::from_hex("#bb9af7"),
            md_code: Color::from_hex("#9ece6a"),
            md_link: Color::from_hex("#7aa2f7"),

            sidebar_sel_bg: Color::from_hex("#292e42"), // focused
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
            bracket_match_bg: Color::from_hex("#364a82"),
        }
    }

    /// Solarized Dark colour scheme.
    pub fn solarized_dark() -> Self {
        Self {
            background: Color::from_hex("#002b36"),
            active_background: Color::from_hex("#073642"),
            foreground: Color::from_hex("#839496"),

            keyword: Color::from_hex("#859900"),
            string_lit: Color::from_hex("#2aa198"),
            comment: Color::from_hex("#586e75"),
            function: Color::from_hex("#268bd2"),
            type_name: Color::from_hex("#b58900"),
            variable: Color::from_hex("#dc322f"),
            number: Color::from_hex("#2aa198"),
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

            status_bg: Color::from_hex("#073642"),
            status_fg: Color::from_hex("#93a1a1"),

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

            hover_bg: Color::from_hex("#073642"),
            hover_fg: Color::from_hex("#93a1a1"),
            hover_border: Color::from_hex("#268bd2"),

            fuzzy_bg: Color::from_hex("#073642"),
            fuzzy_selected_bg: Color::from_hex("#0d4a5a"),
            fuzzy_fg: Color::from_hex("#839496"),
            fuzzy_query_fg: Color::from_hex("#268bd2"),
            fuzzy_border: Color::from_hex("#268bd2"),
            fuzzy_title_fg: Color::from_hex("#b58900"),

            // (bg #002b36)
            diff_added_bg: Color::from_hex("#005e30"),
            diff_removed_bg: Color::from_hex("#5e1a28"),
            diff_padding_bg: Color::from_hex("#0a3545"),

            dap_stopped_bg: Color::from_hex("#2b2000"),

            yank_highlight_bg: Color::from_hex("#859900"),
            yank_highlight_alpha: 0.35,

            annotation_fg: Color::from_hex("#586e75"),
            ghost_text_fg: Color::from_hex("#4a5e68"),

            md_heading1: Color::from_hex("#b58900"),
            md_heading2: Color::from_hex("#268bd2"),
            md_heading3: Color::from_hex("#6c71c4"),
            md_code: Color::from_hex("#859900"),
            md_link: Color::from_hex("#268bd2"),

            sidebar_sel_bg: Color::from_hex("#073642"), // focused
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
            bracket_match_bg: Color::from_hex("#0d4a5a"),
        }
    }

    /// VSCode Dark+ colour scheme.
    pub fn vscode_dark() -> Self {
        Self {
            background: Color::from_hex("#1e1e1e"),
            active_background: Color::from_hex("#252526"),
            foreground: Color::from_hex("#d4d4d4"),

            keyword: Color::from_hex("#569cd6"),    // blue
            string_lit: Color::from_hex("#ce9178"), // salmon
            comment: Color::from_hex("#6a9955"),    // green
            function: Color::from_hex("#dcdcaa"),   // yellow
            type_name: Color::from_hex("#4ec9b0"),  // teal
            variable: Color::from_hex("#9cdcfe"),   // light blue
            number: Color::from_hex("#b5cea8"),     // light green
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

            status_bg: Color::from_hex("#007acc"),
            status_fg: Color::from_hex("#ffffff"),

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

            hover_bg: Color::from_hex("#252526"),
            hover_fg: Color::from_hex("#d4d4d4"),
            hover_border: Color::from_hex("#454545"),

            fuzzy_bg: Color::from_hex("#252526"),
            fuzzy_selected_bg: Color::from_hex("#04395e"),
            fuzzy_fg: Color::from_hex("#d4d4d4"),
            fuzzy_query_fg: Color::from_hex("#0097fb"),
            fuzzy_border: Color::from_hex("#007acc"),
            fuzzy_title_fg: Color::from_hex("#dcdcaa"),

            // (bg #1e1e1e)
            diff_added_bg: Color::from_hex("#14541a"),
            diff_removed_bg: Color::from_hex("#541a1a"),
            diff_padding_bg: Color::from_hex("#2d2d2d"),

            dap_stopped_bg: Color::from_hex("#3a3000"),

            yank_highlight_bg: Color::from_hex("#dcdcaa"),
            yank_highlight_alpha: 0.25,

            annotation_fg: Color::from_hex("#858585"),
            ghost_text_fg: Color::from_hex("#5a5a5a"),

            md_heading1: Color::from_hex("#dcdcaa"),
            md_heading2: Color::from_hex("#569cd6"),
            md_heading3: Color::from_hex("#c586c0"),
            md_code: Color::from_hex("#ce9178"),
            md_link: Color::from_hex("#3794ff"),

            sidebar_sel_bg: Color::from_hex("#37373d"),
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
            bracket_match_bg: Color::from_hex("#3a3d41"),
        }
    }

    /// Return a theme by name. Falls back to `onedark` for unknown names.
    pub fn from_name(name: &str) -> Self {
        match name {
            "gruvbox" | "gruvbox-dark" => Self::gruvbox_dark(),
            "tokyo-night" | "tokyonight" => Self::tokyo_night(),
            "solarized" | "solarized-dark" => Self::solarized_dark(),
            "vscode-dark" | "vscode" | "dark+" => Self::vscode_dark(),
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

    /// Return the list of all built-in theme names.
    pub fn available_names() -> Vec<String> {
        let mut names: Vec<String> = vec![
            "onedark".into(),
            "gruvbox-dark".into(),
            "tokyo-night".into(),
            "solarized-dark".into(),
            "vscode-dark".into(),
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
                        "keyword" | "keyword.control" | "keyword.operator" | "storage"
                        | "storage.type" | "storage.modifier" => {
                            theme.keyword = fg;
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
            "keyword" | "operator" => self.keyword,
            "string" => self.string_lit,
            "comment" => self.comment,
            "function" | "method" => self.function,
            "type" | "class" | "struct" => self.type_name,
            "variable" => self.variable,
            "number" => self.number,
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
            "keyword" | "modifier" => self.keyword,
            "function" | "method" => self.function,
            "type" | "class" | "struct" | "enum" => self.type_name,
            "variable" => self.variable,
            "string" | "regexp" => self.string_lit,
            "comment" => self.comment,
            "number" => self.number,
            "operator" => self.keyword,
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

    let windows = window_rects
        .iter()
        .map(|(window_id, rect)| {
            let visible_lines = (rect.height / line_height).floor() as usize;
            let is_active = *window_id == active_window_id;
            build_rendered_window(
                engine,
                theme,
                *window_id,
                rect,
                visible_lines,
                char_width,
                is_active,
                multi_window,
                color_headings,
            )
        })
        .collect();

    let (status_left, status_right) = build_status_line(engine);
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

    let fuzzy = engine.fuzzy_open.then(|| FuzzyPanel {
        query: engine.fuzzy_query.clone(),
        results: engine
            .fuzzy_results
            .iter()
            .map(|(_, d)| d.clone())
            .collect(),
        selected_idx: engine.fuzzy_selected,
        total_files: engine.fuzzy_all_files.len(),
    });

    let live_grep = engine.grep_open.then(|| {
        let results = engine
            .grep_results
            .iter()
            .map(|m| {
                let basename = m.file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                let snippet = m.line_text.trim();
                let snippet: String = snippet.chars().take(60).collect();
                format!("{}:{}: {}", basename, m.line + 1, snippet)
            })
            .collect();
        LiveGrepPanel {
            query: engine.grep_query.clone(),
            results,
            selected_idx: engine.grep_selected,
            total_matches: engine.grep_results.len(),
            preview_lines: engine.grep_preview_lines.clone(),
        }
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
        let title = engine
            .active_buffer_name()
            .map(|n| format!("VimCode \u{2014} {}", n))
            .unwrap_or_else(|| "VimCode".to_string());
        MenuBarData {
            open_menu_idx: engine.menu_open_idx,
            open_items,
            open_menu_col,
            highlighted_item_idx: engine.menu_highlighted_item,
            title,
            show_window_controls: false, // GTK backend overrides this
            is_vscode_mode: engine.is_vscode_mode(),
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
                        "\u{f0d7} " // ▼
                    } else {
                        "\u{f0da} " // ▶
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
                "\u{f0d7} " // ▼
            } else {
                "\u{f0da} " // ▶
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
                "\u{f0d7} " // ▼
            } else {
                "\u{f0da} " // ▶
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
                    "\u{f0da} " // ▶
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
                    "\u{f111}" // ●
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

    // Build command palette panel data.
    let command_palette = engine.palette_open.then(|| {
        use crate::core::engine::PALETTE_COMMANDS;
        let use_vscode = engine.is_vscode_mode();
        CommandPalettePanel {
            query: engine.palette_query.clone(),
            items: engine
                .palette_results
                .iter()
                .map(|&i| {
                    let cmd = &PALETTE_COMMANDS[i];
                    let sc = if use_vscode && !cmd.vscode_shortcut.is_empty() {
                        cmd.vscode_shortcut.to_string()
                    } else {
                        cmd.shortcut.to_string()
                    };
                    (cmd.label.to_string(), sc)
                })
                .collect(),
            selected_idx: engine.palette_selected,
            scroll_top: engine.palette_scroll_top,
        }
    });

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
                GroupTabBar {
                    group_id: gid,
                    tabs,
                    bounds,
                    diff_toolbar,
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
        command,
        wildmenu,
        active_window_id,
        completion,
        hover,
        fuzzy,
        live_grep,
        quickfix,
        bottom_tabs,
        signature_help,
        menu_bar,
        debug_toolbar,
        debug_sidebar,
        source_control,
        command_palette,
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
        dialog: engine.dialog.as_ref().map(|d| DialogPanel {
            title: d.title.clone(),
            body: d.body.clone(),
            buttons: d
                .buttons
                .iter()
                .enumerate()
                .map(|(i, btn)| (format_button_label(&btn.label, btn.hotkey), i == d.selected))
                .collect(),
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
        commit_input_active: engine.sc_commit_input_active,
        button_focused: engine.sc_button_focused,
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
            let items = engine
                .ext_panel_items
                .get(&key)
                .cloned()
                .unwrap_or_default();
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
    })
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

    // Path segments (relative to cwd)
    if let Some(ref file_path) = buf_state.file_path {
        let display = if let Ok(rel) = file_path.strip_prefix(&engine.cwd) {
            rel.to_string_lossy().to_string()
        } else {
            file_path.to_string_lossy().to_string()
        };
        let parts: Vec<&str> = display.split(std::path::MAIN_SEPARATOR).collect();
        for part in &parts {
            segments.push(BreadcrumbSegment {
                label: part.to_string(),
                is_last: false,
                is_symbol: false,
            });
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
            });
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
        bracket_match_positions: Vec::new(),
        active_indent_col: None,
        tabstop: engine.settings.tabstop.max(1) as usize,
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
        let line_str = line.to_string();
        let line_start_byte = buffer.content.line_to_byte(line_idx);
        let line_end_byte = line_start_byte + line.len_bytes();

        let spans = if let Some(ref md) = buffer_state.md_rendered {
            if line_idx < md.spans.len() {
                md_spans_to_styled(&md.spans[line_idx], theme, color_headings)
            } else {
                vec![]
            }
        } else {
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
                    annotation: if is_cont || engine.mode == crate::core::Mode::Insert {
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
                annotation: if engine.mode == crate::core::Mode::Insert {
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
    }
}

/// Convert markdown style spans into rendering `StyledSpan`s.
fn md_spans_to_styled(
    md_spans: &[crate::core::markdown::MdSpan],
    theme: &Theme,
    color_headings: bool,
) -> Vec<StyledSpan> {
    use crate::core::markdown::MdStyle;
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
    if engine.settings.hlsearch && !engine.search_matches.is_empty() {
        let line_start_char = buffer.content.line_to_char(line_idx);
        let line_char_count = line_str.chars().count();
        let line_end_char = line_start_char + line_char_count;

        for (match_idx, (match_start, match_end)) in engine.search_matches.iter().enumerate() {
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

            let is_current = engine.search_index == Some(match_idx);
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
    let cursor = engine.cursor();

    let kind = match engine.mode {
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

fn build_status_line(engine: &Engine) -> (String, String) {
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

    let branch = engine
        .git_branch
        .as_deref()
        .map(|b| format!(" [{}]", b))
        .unwrap_or_default();

    let left = format!(
        " -- {}{} -- {}{}{}",
        mode_str, recording, filename, dirty, branch
    );

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

    (left, right)
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

    CommandLineData {
        text,
        right_align,
        show_cursor,
        cursor_anchor_text,
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
}
