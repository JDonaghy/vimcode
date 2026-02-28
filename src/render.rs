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
use crate::core::buffer_manager::BufferState;
use crate::core::dap::DapVariable;
pub use crate::core::engine::{BottomPanelKind, DebugSidebarSection};
use crate::core::engine::{DiffLine, Engine, SearchDirection};
use crate::core::lsp::SignatureHelpData;
use crate::core::settings::LineNumberMode;
use crate::core::terminal::TermSelection as CoreTermSelection;
use crate::core::view::View;
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

    /// Normalise to the (0.0..=1.0, 0.0..=1.0, 0.0..=1.0) triple expected by
    /// Cairo's `set_source_rgb` / `set_source_rgba`.
    pub fn to_cairo(self) -> (f64, f64, f64) {
        (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
        )
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

// ─── Style / StyledSpan ──────────────────────────────────────────────────────

/// Text style for a span of characters.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub fg: Color,
    /// Background override; `None` means the window background shows through.
    pub bg: Option<Color>,
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
    /// Active visual selection, or `None`.
    pub selection: Option<SelectionRange>,
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
}

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
                label: "Open Workspace…",
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
                label: "Close Tab",
                shortcut: "",
                vscode_shortcut: "Ctrl+W",
                action: "bd",
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
                vscode_shortcut: "",
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
                action: "copy",
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

    // DAP stopped-line highlight
    pub dap_stopped_bg: Color,
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

            // Two-way diff backgrounds (dark green / dark red)
            diff_added_bg: Color::from_hex("#1e3a1e"), // dark green
            diff_removed_bg: Color::from_hex("#3a1e1e"), // dark red

            // DAP stopped-line (dark amber)
            dap_stopped_bg: Color::from_hex("#3a3000"),
        }
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
            _ => self.default_fg,
        }
    }
}

// ─── build_screen_layout ──────────────────────────────────────────────────────

/// Build a complete `ScreenLayout` from current engine state.
///
/// # Parameters
/// - `engine` — current editor state (no GTK types)
/// - `theme` — colour scheme
/// - `window_rects` — pixel-space rects for each window in the current tab,
///   as returned by `engine.calculate_window_rects()`
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
            )
        })
        .collect();

    let (status_left, status_right) = build_status_line(engine);
    let command = build_command_line(engine);

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

    ScreenLayout {
        tab_bar,
        windows,
        status_left,
        status_right,
        command,
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

// ─── Private builder helpers ──────────────────────────────────────────────────

fn build_tab_bar(engine: &Engine) -> Vec<TabInfo> {
    engine
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let active = i == engine.active_tab;
            let window_id = tab.active_window;
            let (name, dirty, preview) = if let Some(window) = engine.windows.get(&window_id) {
                if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                    let dirty_marker = if state.dirty { "*" } else { "" };
                    (
                        format!(" {}: {}{} ", i + 1, state.display_name(), dirty_marker),
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

/// Return the number of visual rows a buffer line of `line_char_len` characters
/// occupies when the viewport is `viewport_cols` columns wide.
/// Always returns at least 1 (even for empty lines).
pub fn visual_rows_for_line(line_char_len: usize, viewport_cols: usize) -> usize {
    if viewport_cols == 0 {
        return 1;
    }
    line_char_len.div_ceil(viewport_cols).max(1)
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
) -> RenderedWindow {
    let empty = |id: WindowId| RenderedWindow {
        window_id: id,
        rect: *rect,
        lines: vec![],
        cursor: None,
        selection: None,
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
    let scroll_top = view.scroll_top;
    let total_lines = buffer.content.len_lines();
    let cursor_line = view.cursor.line;

    // Whether this buffer has git diff data.
    let has_git = !buffer_state.git_diff.is_empty();

    // Look up LSP diagnostics for this buffer.
    // Diagnostics are keyed by absolute path (from LSP URIs), but buffer file_path
    // may be relative, so canonicalize for the lookup.
    let canonical_path = buffer_state
        .file_path
        .as_ref()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
    let file_diagnostics = canonical_path
        .as_ref()
        .and_then(|p| engine.lsp_diagnostics.get(p));

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

    // Gutter width in character columns (always includes fold indicator column).
    let gutter_char_width = calculate_gutter_cols(
        engine.settings.line_numbers,
        total_lines,
        char_width,
        has_git,
        has_bp,
    );

    // Build rendered lines (fold-aware: skip hidden lines, jump over fold bodies)
    let mut lines = Vec::with_capacity(visible_lines);
    let mut line_idx = scroll_top;
    while lines.len() < visible_lines && line_idx < total_lines {
        // Skip hidden lines (fold bodies).
        if view.is_line_hidden(line_idx) {
            line_idx += 1;
            continue;
        }

        let is_fold_header = view.fold_at(line_idx).is_some();
        let folded_line_count = view.fold_at(line_idx).map(|f| f.end - f.start).unwrap_or(0);

        let line = buffer.content.line(line_idx);
        let line_str = line.to_string();
        let line_start_byte = buffer.content.line_to_byte(line_idx);
        let line_end_byte = line_start_byte + line.len_bytes();

        let spans = build_spans(
            engine,
            theme,
            buffer_state,
            buffer,
            line_idx,
            &line_str,
            line_start_byte,
            line_end_byte,
        );

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
                            .as_ref()
                            .map(|cp| cp.to_string_lossy().as_ref() == path.as_str())
                            .unwrap_or(false))
            })
            .unwrap_or(false);

        let fold_char = fold_indicator_char(buffer, view, line_idx);
        // Number of leading marker columns (bp + git) subtracted from the
        // numeric portion so line numbers fill their allotted width correctly.
        let marker_cols = if has_bp { 1 } else { 0 } + if has_git { 1 } else { 0 };
        let base_gutter = format_gutter_with_fold(
            engine.settings.line_numbers,
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
                    None => " ",
                }
            } else {
                ""
            };
            format!("{}{}{}", bp_part, git_part, base_gutter)
        };

        // LSP diagnostics for this line.
        let line_diagnostics: Vec<DiagnosticMark> = file_diagnostics
            .map(|diags| {
                diags
                    .iter()
                    .filter(|d| d.range.start.line as usize == line_idx)
                    .map(|d| {
                        let line_text: String = buffer.content.line(line_idx).chars().collect();
                        let start_col = crate::core::lsp::utf16_offset_to_char(
                            &line_text,
                            d.range.start.character,
                        );
                        let end_col = if d.range.end.line as usize == line_idx {
                            crate::core::lsp::utf16_offset_to_char(
                                &line_text,
                                d.range.end.character,
                            )
                        } else {
                            line_text.len()
                        };
                        DiagnosticMark {
                            start_col,
                            end_col: end_col.max(start_col + 1),
                            severity: d.severity,
                            message: d.message.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Two-way diff status for this line.
        let diff_status = engine
            .diff_results
            .get(&window_id)
            .and_then(|v| v.get(line_idx))
            .copied();

        let wrap_on = engine.settings.wrap && view.viewport_cols > 0 && !is_fold_header;
        let line_char_len = line_str.chars().count();

        if wrap_on && line_char_len > view.viewport_cols {
            // Split long line into viewport-width segments.
            let vp = view.viewport_cols;
            let num_segments = visual_rows_for_line(line_char_len, vp);
            let cursor_seg = if line_idx == cursor_line {
                view.cursor.col / vp
            } else {
                usize::MAX // won't match any segment
            };
            // Blank gutter for continuation rows (same width as normal gutter).
            let blank_gutter = " ".repeat(gutter_char_width);
            for seg in 0..num_segments {
                if lines.len() >= visible_lines {
                    break;
                }
                let seg_start_char = seg * vp;
                let seg_end_char = ((seg + 1) * vp).min(line_char_len);
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
                    diff_status: if is_cont { None } else { diff_status },
                    is_breakpoint: !is_cont && is_breakpoint,
                    is_conditional_bp: !is_cont && is_conditional_bp,
                    is_dap_current,
                    is_wrap_continuation: is_cont,
                    segment_col_offset: seg_start_char,
                });
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
                diff_status,
                is_breakpoint,
                is_conditional_bp,
                is_dap_current,
                is_wrap_continuation: false,
                segment_col_offset: 0,
            });
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

    // Visual selection (only for active window)
    let selection = if is_active {
        build_selection(engine, scroll_top, visible_lines)
    } else {
        None
    };

    // Maximum line length across the whole buffer. When wrap is on, there is no
    // horizontal scrolling, so we report 0 to suppress the horizontal scrollbar.
    let max_col = if engine.settings.wrap {
        0
    } else {
        buffer_state.max_col
    };

    // Build diagnostic gutter map (line → worst severity).
    let mut diagnostic_gutter = std::collections::HashMap::new();
    if let Some(diags) = file_diagnostics {
        for d in diags {
            let line = d.range.start.line as usize;
            let entry = diagnostic_gutter.entry(line).or_insert(d.severity);
            // Lower numeric value = worse severity (Error=1 < Warning=2 etc.)
            if (d.severity as u8) < (*entry as u8) {
                *entry = d.severity;
            }
        }
    }

    RenderedWindow {
        window_id,
        rect: *rect,
        lines,
        cursor,
        selection,
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
    }
}

/// Build styled spans for one line: syntax highlights + search matches.
#[allow(clippy::too_many_arguments)]
fn build_spans(
    engine: &Engine,
    theme: &Theme,
    buffer_state: &BufferState,
    buffer: &crate::core::buffer::Buffer,
    line_idx: usize,
    line_str: &str,
    line_start_byte: usize,
    line_end_byte: usize,
) -> Vec<StyledSpan> {
    let mut spans = Vec::new();

    // Syntax highlighting
    for (start, end, scope) in &buffer_state.highlights {
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
            },
        });
    }

    // Search match highlighting
    if !engine.search_matches.is_empty() {
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
    let total = buffer.content.len_lines();
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
        Some(p) => p.display().to_string(),
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
            let t = format!(":{}", engine.command_buffer);
            (t.clone(), false, true, t)
        }
        Mode::Search => {
            let ch = match engine.search_direction {
                SearchDirection::Forward => '/',
                SearchDirection::Backward => '?',
            };
            let t = format!("{}{}", ch, engine.command_buffer);
            (t.clone(), false, true, t)
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
