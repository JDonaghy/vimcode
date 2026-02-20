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
use crate::core::engine::{Engine, SearchDirection};
use crate::core::settings::LineNumberMode;
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

    ScreenLayout {
        tab_bar,
        windows,
        status_left,
        status_right,
        command,
        active_window_id,
        completion,
        hover,
    }
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

    // Gutter width in character columns (always includes fold indicator column).
    let gutter_char_width = calculate_gutter_cols(
        engine.settings.line_numbers,
        total_lines,
        char_width,
        has_git,
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

        let fold_char = fold_indicator_char(buffer, view, line_idx);
        let base_gutter = format_gutter_with_fold(
            engine.settings.line_numbers,
            line_idx,
            cursor_line,
            // Pass gutter_char_width minus the git column so numbers fill correctly.
            gutter_char_width.saturating_sub(if has_git { 1 } else { 0 }),
            fold_char,
        );
        let gutter_text = if has_git {
            let git_char = match git_status {
                Some(GitLineStatus::Added) | Some(GitLineStatus::Modified) => '▌',
                None => ' ',
            };
            format!("{}{}", git_char, base_gutter)
        } else {
            base_gutter
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
        });

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
            .position(|l| l.is_current_line)
            .map(|view_line| {
                let shape = if engine.pending_key == Some('r') {
                    CursorShape::Underline
                } else {
                    match engine.mode {
                        Mode::Insert => CursorShape::Bar,
                        _ => CursorShape::Block,
                    }
                };
                (
                    CursorPos {
                        view_line,
                        col: view.cursor.col,
                    },
                    shape,
                )
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

    // Maximum line length across the whole buffer.  Pre-computed in update_syntax()
    // so we don't pay an O(N_lines) scan here on every render frame.
    let max_col = buffer_state.max_col;

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
) -> usize {
    let git = if has_git_diff { 1 } else { 0 };
    match mode {
        // No line numbers: show only the 1-column fold indicator.
        LineNumberMode::None => 1 + git,
        LineNumberMode::Absolute => {
            let digits = total_lines.to_string().len().max(1);
            digits + 2 + 1 + git // digits + padding + fold indicator + git
        }
        LineNumberMode::Relative | LineNumberMode::Hybrid => {
            let max_relative = total_lines.saturating_sub(1);
            let digits = max_relative.to_string().len().max(3);
            digits + 2 + 1 + git
        }
    }
}

fn build_status_line(engine: &Engine) -> (String, String) {
    let mode_str = match engine.mode {
        Mode::Normal | Mode::Command | Mode::Search => "NORMAL",
        Mode::Insert => "INSERT",
        Mode::Visual => "VISUAL",
        Mode::VisualLine => "VISUAL LINE",
        Mode::VisualBlock => "VISUAL BLOCK",
    };

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
