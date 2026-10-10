use super::*;

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
    /// Window rectangle. GTK reads it directly as sub-pixel float geometry
    /// (Cairo paints exactly what's here). TUI reads it too — for popup
    /// positioning (`tui_main::render_impl`'s completion/hover popup
    /// clamping, #420) and, truncated to whole cells first via
    /// [`tui_window_paint_rect`], for click/drag/hover column resolution
    /// (#1040) — but is *not* the geometry TUI's own paint path draws
    /// into; see [`tui_window_paint_rect`]'s doc for why TUI code must
    /// truncate before using it.
    pub rect: WindowRect,
    /// Visible lines, one per row.
    ///
    /// **Not** the window's row *capacity* — `build_rendered_window`'s
    /// fill loop stops the moment it runs out of buffer content
    /// (`line_idx < total_lines`), so on a buffer shorter than the
    /// viewport this is shorter than the number of rows the window
    /// actually has room to paint. Use [`Self::visible_line_capacity`]
    /// for "how many rows could this window show" — e.g. feeding
    /// `Engine::set_viewport_for_window` (#1779: using `lines.len()`
    /// there pinned a 1-line buffer's `view.viewport_lines` to `1`, so
    /// the very next `ensure_cursor_visible` — on the keystroke that grew
    /// the buffer to 2 lines — believed the viewport could show only one
    /// line and scrolled line 0 out of view to keep the cursor's new
    /// line "visible").
    pub lines: Vec<RenderedLine>,
    /// This window's row *capacity* at the geometry `rect`/`line_height`
    /// this frame painted with — `(rect.height / line_height).floor()`,
    /// minus one for the per-window status row when that's shown (exactly
    /// the `visible_lines` local `build_screen_layout`'s window loop
    /// computes and feeds into `build_rendered_window`). Unlike
    /// [`Self::lines`]`.len()`, this does **not** shrink just because the
    /// buffer itself is shorter than the viewport — see that field's doc
    /// for the #1779 bug this distinction exists to keep fixed.
    pub visible_line_capacity: usize,
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
    /// Width reserved to the right of the text for the minimap strip plus
    /// the scroll-affordance gutter it always leaves clear alongside it
    /// (#1094 review), in the caller's own unit — `0.0` when the strip is
    /// off/self-suppressed for this window. `rect` reaches the pane's true
    /// right edge (#1094's own fix), so `window_zone_hit_test` needs this
    /// to find the strip's boundary and exclude it from `TextArea`/gutter
    /// click routing without re-deriving `scroll_gutter_width` (a private,
    /// backend-`scrollbar_reserve`-dependent quantity) a second time from
    /// scratch. Mirrors exactly what `build_rendered_window` already
    /// subtracted to produce `text_viewport_cols` above, so the two can
    /// never drift from each other.
    pub minimap_reserved_w: f64,
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
    /// Set when this window's buffer hosts a `vimcode.ui.register_view` view
    /// as an editor-area tab (`BufferState::plugin_view`, #1627), naming the
    /// view. `lines`/`cursor`/every other buffer-content field is left at its
    /// `build_rendered_window`-`empty()` default when this is `Some` — the
    /// window's content is a `Form`, painted by
    /// `App::paint_editor_windows_rung` instead of `Surface::Editor`, not
    /// buffer text.
    pub plugin_view: Option<String>,
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
    /// Sorted list of candidates. `CompletionCandidate` (#1805) carries
    /// optional kind/detail/documentation metadata — buffer-word and LSP
    /// candidates leave those `Text`/`None`, a `vimcode.completion.
    /// register` plugin source can set them.
    pub candidates: Vec<crate::core::completion::CompletionCandidate>,
    /// Index of the currently highlighted candidate.
    pub selected_idx: usize,
    /// Length (in chars) of the longest candidate's label — used for popup
    /// width.
    pub max_width: usize,
}

/// Map vimcode's own [`crate::core::completion::CompletionItemKind`] to
/// quadraui's `CompletionKind` — the one conversion point between the two
/// vocabularies (mirrors `plugin_ui`'s "vimcode-owned vocabulary, converted
/// to quadraui primitives in one place" rule).
fn completion_item_kind_to_quadraui(
    kind: crate::core::completion::CompletionItemKind,
) -> quadraui::CompletionKind {
    use crate::core::completion::CompletionItemKind as K;
    match kind {
        K::Text => quadraui::CompletionKind::Text,
        K::Method => quadraui::CompletionKind::Method,
        K::Function => quadraui::CompletionKind::Function,
        K::Constructor => quadraui::CompletionKind::Constructor,
        K::Field => quadraui::CompletionKind::Field,
        K::Variable => quadraui::CompletionKind::Variable,
        K::Class => quadraui::CompletionKind::Class,
        K::Interface => quadraui::CompletionKind::Interface,
        K::Module => quadraui::CompletionKind::Module,
        K::Property => quadraui::CompletionKind::Property,
        K::Unit => quadraui::CompletionKind::Unit,
        K::Value => quadraui::CompletionKind::Value,
        K::Enum => quadraui::CompletionKind::Enum,
        K::Keyword => quadraui::CompletionKind::Keyword,
        K::Snippet => quadraui::CompletionKind::Snippet,
        K::Color => quadraui::CompletionKind::Color,
        K::File => quadraui::CompletionKind::File,
        K::Reference => quadraui::CompletionKind::Reference,
        K::Folder => quadraui::CompletionKind::Folder,
        K::EnumMember => quadraui::CompletionKind::EnumMember,
        K::Constant => quadraui::CompletionKind::Constant,
        K::Struct => quadraui::CompletionKind::Struct,
        K::Event => quadraui::CompletionKind::Event,
        K::Operator => quadraui::CompletionKind::Operator,
        K::TypeParameter => quadraui::CompletionKind::TypeParameter,
    }
}

/// Convert a render-side `CompletionMenu` into a `quadraui::Completions`
/// for backend rasterisation via the D6 layout pipeline.
pub fn completion_menu_to_quadraui_completions(menu: &CompletionMenu) -> quadraui::Completions {
    let items = menu
        .candidates
        .iter()
        .map(|c| quadraui::CompletionItem {
            label: quadraui::StyledText::plain(c.label.clone()),
            detail: c.detail.clone().map(quadraui::StyledText::plain),
            documentation: c.documentation.clone().map(quadraui::StyledText::plain),
            kind: completion_item_kind_to_quadraui(c.kind),
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
/// (the `rev` on the `quadraui` dependency in `Cargo.toml`, formerly
/// `quadraui-pin.txt` = `f6d27c2`; see #691). Calling it compiles only
/// once the pin moves, which is its own deliberate, `cargo test`-verified
/// commit — ~70 quadraui commits rode along in `f6d27c2..develop` at the
/// time this was written, including tooltip-box and tab-label paint
/// changes that restate this repo's snapshots.
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
    /// Raw markdown source. Styled at paint time with the active theme —
    /// see `markdown_hover_to_quadraui_lines` (#821: adopts
    /// `quadraui::compose::markdown::render_markdown_to_styled`).
    pub markdown: String,
    /// Plain per-line text (markdown syntax stripped).
    pub line_text: Vec<String>,
    /// Per-line tree-sitter highlights for fenced code-block lines.
    pub code_highlights: Vec<Vec<crate::core::markdown::MdCodeHighlight>>,
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

/// Render hover-popup markdown into per-line `quadraui::StyledText` + per-line
/// heading font scale, via quadraui's shared
/// `quadraui::compose::markdown::render_markdown_to_styled` (#821 — replaces
/// the previous hand-rolled `MdStyle`-to-color byte-position span walk).
/// Shared by every rich-text hover/popup builder (editor hover, panel-item
/// hover) so markdown → styled-span conversion lives in exactly one place.
///
/// `code_highlights` are vimcode's own tree-sitter highlights for fenced
/// code-block lines (precomputed once at hover-show time by
/// `core::markdown::hover_markdown_structure`, since quadraui's renderer is
/// deliberately language-agnostic — see its own doc: "Tree-sitter-capable
/// callers opt into per-language highlighting"). They're overlaid onto the
/// relevant lines after quadraui's render.
pub(crate) fn markdown_hover_to_quadraui_lines(
    markdown: &str,
    code_highlights: &[Vec<crate::core::markdown::MdCodeHighlight>],
    theme: &Theme,
) -> (Vec<quadraui::StyledText>, Vec<f32>) {
    let q_theme = to_quadraui_theme(theme);
    let mut rendered = quadraui::compose::markdown::render_markdown_to_styled(markdown, &q_theme);
    for (line_idx, highlights) in code_highlights.iter().enumerate() {
        if highlights.is_empty() {
            continue;
        }
        if let Some(line) = rendered.lines.get_mut(line_idx) {
            overlay_code_highlights(line, highlights, theme);
        }
    }
    (rendered.lines, rendered.line_scales)
}

/// Recolor a fenced code-block content line's raw-code span with vimcode's
/// tree-sitter scope colors. Per quadraui's `render_code_content`, a
/// code-block content line always has its raw (unprefixed) code text as the
/// *last* span — the two before it are the code-rail indent + bar, which are
/// left untouched. `highlights`' byte offsets are relative to that last
/// span's text (see `core::markdown::hover_markdown_structure`'s doc).
fn overlay_code_highlights(
    line: &mut quadraui::StyledText,
    highlights: &[crate::core::markdown::MdCodeHighlight],
    theme: &Theme,
) {
    let Some(code_span) = line.spans.pop() else {
        return;
    };
    let default_fg = code_span.fg;
    let bg = code_span.bg;
    let scope_at = |byte_pos: usize| -> Option<&str> {
        highlights
            .iter()
            .find(|h| byte_pos >= h.start_byte && byte_pos < h.end_byte)
            .map(|h| h.scope.as_str())
    };

    let mut byte_pos = 0usize;
    let mut current_text = String::new();
    let mut current_scope: Option<&str> = None;
    let flush = |text: &mut String, scope: Option<&str>, spans: &mut Vec<quadraui::StyledSpan>| {
        if text.is_empty() {
            return;
        }
        let fg = scope.map(|s| theme.scope_color(s)).or(default_fg);
        spans.push(quadraui::StyledSpan {
            text: std::mem::take(text),
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false,
        });
    };

    for ch in code_span.text.chars() {
        let scope = scope_at(byte_pos);
        if scope != current_scope {
            flush(&mut current_text, current_scope, &mut line.spans);
            current_scope = scope;
        }
        current_text.push(ch);
        byte_pos += ch.len_utf8();
    }
    flush(&mut current_text, current_scope, &mut line.spans);
}

/// Convert `(line, start_byte, end_byte, url)` link tuples (the shape
/// shared by `EditorHoverPopupData` and `PanelHoverPopupData`) into
/// `quadraui::RichTextLink`s.
pub(crate) fn md_links_to_quadraui_rich_text_links(
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
    let (q_lines, line_scales) =
        markdown_hover_to_quadraui_lines(&eh.markdown, &eh.code_highlights, theme);
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
        line_text: eh.line_text.clone(),
        line_scales,
        scroll_top: eh.scroll_top,
        max_visible_rows: EDITOR_HOVER_MAX_ROWS,
        has_focus: eh.has_focus,
        selection: q_selection,
        links: q_links,
        focused_link: eh.focused_link,
        placement: quadraui::PopupPlacement::Above,
        padding: 0.0,
        fg: Some(theme.hover_fg),
        bg: Some(theme.hover_bg),
    }
}

/// Build, rasterise and hit-region-extract the editor hover popup via the
/// `quadraui::RichTextPopup` primitive. Shared by both backends' paint
/// paths (#669) — GTK previously duplicated this in the now-dead
/// `src/gtk/draw.rs::draw_editor_hover_popup`, with an added Pango-exact
/// link-width measure; that precision isn't reachable from
/// `render_content`'s `&mut dyn Backend`-only signature (no raw
/// `pango::Layout`, same class of gap TUI's now-deleted
/// `render_editor_hover_popup` wrapper hit for the raw `Frame` — see
/// `PLAN.md`), so both backends now use the same char-count-based
/// `link_widths` closure, scaled by `unit_w`. This only affects link
/// *hit-region* precision, not paint — the rasteriser re-measures glyphs
/// itself when drawing.
///
/// `unit_w` / `unit_h` are `1.0, 1.0` for TUI (cell-native) or
/// `char_width, line_height` in pixels for GTK. `popup_x` / `popup_y` /
/// `viewport` must already be expressed in that same space.
///
/// Returns `(link_rects, popup_bounds, scrollbar_hit)` — all `quadraui::Rect`,
/// in the caller's units — for mouse hit-testing. Called by the shared
/// [`paint_editor_popups`] (#1167), which both `render_impl.rs`'s TUI
/// wrapper and GTK's `App::paint_editor_popups_rung` call through; there is
/// no per-backend copy of this geometry any more (#831).
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
    Vec<(quadraui::Rect, String)>,
    Option<quadraui::Rect>,
    Option<PopupScrollbarHit>,
) {
    if eh.line_text.is_empty() {
        return (vec![], None, None);
    }
    let popup = editor_hover_to_quadraui_rich_text(eh, theme);
    let content_w = ((eh.popup_width as f32) * unit_w)
        .max(10.0 * unit_w)
        .min((viewport.width - 4.0 * unit_w).max(10.0 * unit_w));
    let measure = quadraui::RichTextPopupMeasure::new(content_w, unit_h);
    // #504: this used to be a char-count estimate (`chars().count() as f32
    // * unit_w`) — fine on TUI's fixed-width grid, but on a proportional
    // GTK font it drifts further from the real painted glyph position with
    // every preceding span, so a later link on the same line could end up
    // with a hit region nowhere near where it was actually drawn.
    // `quadraui::Backend::measure_text` already exists for exactly this
    // (D-014: real shaped advance on GUI backends, the same cell-count
    // this closure used to compute by hand on TUI) — route through it
    // instead of re-deriving the estimate here, so the hit region tracks
    // the same font the rasteriser paints with.
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
                    let slice = &t[start_byte.min(t.len())..end_byte.min(t.len())];
                    backend.measure_text(slice, quadraui::FontRole::Chrome).0
                })
                .unwrap_or(0.0)
        },
    );

    backend.draw_rich_text_popup(&popup, &layout);

    let link_rects: Vec<(quadraui::Rect, String)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (*rect, url)
        })
        .collect();

    let popup_rect = Some(layout.bounds);
    let scrollbar_hit = layout.scrollbar.map(|sb| PopupScrollbarHit {
        track: sb.track,
        thumb: sb.thumb,
        visible_rows: EDITOR_HOVER_MAX_ROWS,
        total: popup.lines.len(),
    });
    (link_rects, popup_rect, scrollbar_hit)
}

/// A single editor-anchored popup's already-resolved paint position: the
/// on-screen anchor point plus the viewport it clamps into, both expressed
/// in the caller's native units (TUI cell columns/rows, GTK pixels).
#[derive(Debug, Clone, Copy)]
pub struct PopupAnchor {
    pub x: f32,
    pub y: f32,
    pub viewport: quadraui::Rect,
}

/// Resolved `(x, y)` anchor points for each of the five editor-anchored
/// popups, in the caller's unit scale (`cw`/`lh`: `1.0`/`1.0` for TUI's
/// cell-native space, the actual pixel char-width/line-height for GTK).
/// `None` when that popup isn't open or there's no active window.
///
/// Deliberately does *not* carry the viewport each anchor clamps into —
/// see `paint_editor_popups`'s doc for the one pre-existing, intentional
/// per-backend difference (`editor_hover`'s clip viewport: TUI clamps into
/// the whole frame, GTK into the active window's own rect) that callers
/// still own so this mechanical extraction doesn't silently fold it away.
pub struct EditorPopupPoints {
    pub completion: Option<(f32, f32)>,
    pub hover: Option<(f32, f32)>,
    pub editor_hover: Option<(f32, f32)>,
    pub diff_peek: Option<(f32, f32)>,
    pub signature_help: Option<(f32, f32)>,
}

/// Compute [`EditorPopupPoints`] from the active window in `screen`.
///
/// `win_origin` is the active window's own `(x, y)` in the caller's unit
/// scale — `None` when there's no active window. Taken as a parameter
/// rather than read from `RenderedWindow::rect` directly because TUI must
/// snap it to the whole-cell grid its paint path actually truncated to
/// first (via [`tui_window_paint_rect`]; `RenderedWindow` rects come from
/// continuous float split math and are not integer-valued in general,
/// #1040), while GTK uses its raw sub-pixel float rect as-is — exactly the
/// same origin each backend already independently derives for its own
/// `win_viewport`.
///
/// Shared by TUI (`tui_main::render_impl::paint_editor_popups`) and GTK
/// (`App::paint_editor_popups_rung`) as of #1237 — before that each backend
/// re-derived every one of these five anchor points itself (gutter width,
/// scroll offset, tab-aware column resolution), and only TUI's completion
/// anchor happened to call [`quadraui::text_util::char_col_to_visual`]
/// first; the other four
/// anchors on *both* backends, and GTK's completion anchor specifically,
/// used the raw character column as a display column outright.
pub fn editor_popup_anchors(
    screen: &ScreenLayout,
    win_origin: Option<(f32, f32)>,
    cw: f32,
    lh: f32,
) -> EditorPopupPoints {
    let active_win = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id);

    // `view_row` is already relative to the top of the visible window (as
    // `CursorPos::view_line` is), so every caller below that starts from an
    // absolute buffer line (`anchor_line`) subtracts its own scroll offset
    // first.
    let anchor_xy = |view_row: usize, char_col: usize, scroll_left: usize| -> Option<(f32, f32)> {
        let win = active_win?;
        let (win_x, win_y) = win_origin?;
        let raw = win
            .lines
            .get(view_row)
            .map(|l| l.raw_text.as_str())
            .unwrap_or("");
        let vis_col = quadraui::text_util::char_col_to_visual(raw, char_col, win.tabstop)
            .saturating_sub(scroll_left) as f32;
        let x = win_x + win.gutter_char_width as f32 * cw + vis_col * cw;
        let y = win_y + view_row as f32 * lh;
        Some((x, y))
    };

    let completion = active_win.and_then(|win| {
        let (cursor_pos, _) = win.cursor.as_ref()?;
        anchor_xy(cursor_pos.view_line, cursor_pos.col, win.scroll_left)
    });

    let hover = screen.hover.as_ref().and_then(|h| {
        let win = active_win?;
        anchor_xy(
            h.anchor_line.saturating_sub(win.scroll_top),
            h.anchor_col,
            win.scroll_left,
        )
    });

    let editor_hover = screen.editor_hover.as_ref().and_then(|eh| {
        anchor_xy(
            eh.anchor_line.saturating_sub(eh.frozen_scroll_top),
            eh.anchor_col,
            eh.frozen_scroll_left,
        )
    });

    let signature_help = screen.signature_help.as_ref().and_then(|sig| {
        let win = active_win?;
        anchor_xy(
            sig.anchor_line.saturating_sub(win.scroll_top),
            sig.anchor_col,
            win.scroll_left,
        )
    });

    // Diff-peek anchors at the cursor's own row, left edge — no column
    // offset, so no tab expansion needed.
    let diff_peek = screen.diff_peek.as_ref().and_then(|peek| {
        let win = active_win?;
        let (win_x, win_y) = win_origin?;
        let view_row = peek.anchor_line.saturating_sub(win.scroll_top);
        let x = win_x + win.gutter_char_width as f32 * cw;
        let y = win_y + view_row as f32 * lh;
        Some((x, y))
    });

    EditorPopupPoints {
        completion,
        hover,
        editor_hover,
        diff_peek,
        signature_help,
    }
}

/// Paint the editor-anchored popups: completion menu, LSP hover, the rich
/// "editor hover" markdown popup, diff-peek, and signature-help.
///
/// Shared by TUI (`tui_main::render_impl::paint_editor_popups`) and GTK
/// (`App::paint_editor_popups_rung`) as of #1167 — before that the two were
/// ~160/~164-line near-verbatim copies of the same five
/// build-adapter → `.layout()` → `backend.draw_*` → cache-output-for-hit-
/// testing blocks, differing only in coordinate units and in exactly how
/// each anchor point is derived from the active window (gutter width,
/// scroll offsets, tab-aware column resolution for the cursor).
///
/// This function owns everything from "given a resolved anchor + viewport"
/// onward. Resolving the anchor point itself now goes through the shared
/// [`editor_popup_anchors`] (#1237 — see its doc for why GTK's completion
/// anchor and *all four* non-completion anchors on both backends used to
/// drift left on tab-indented lines), so only viewport selection and the
/// completion popup's width/height clamp remain per-backend wiring. One
/// deliberate *pre-existing* per-backend difference survives that
/// convergence too — the editor-hover popup's clip viewport: TUI clamps
/// into the whole frame `area`, GTK clamps into the active window's own
/// rect. Folding that one together would be a behavior change needing its
/// own issue + black-box test, not something to smuggle into this
/// refactor.
///
/// The four output caches are cleared unconditionally at the top (matching
/// GTK's existing per-frame behavior) rather than only-on-`Some` (TUI's
/// prior behavior for `completion_layout`/`editor_hover_link_rects`) —
/// verified safe, not a behavior change: every consumer
/// (`render::route_modal_overlay_click`'s `completion_open` /
/// `editor_hover_*` routing) already gates on live engine state before ever
/// reading the cached layout, so a stale cache was unreachable dead data,
/// never a click-routing hazard.
#[allow(clippy::too_many_arguments)]
pub fn paint_editor_popups(
    backend: &mut dyn quadraui::Backend,
    screen: &ScreenLayout,
    theme: &Theme,
    unit_w: f32,
    unit_h: f32,
    completion: Option<(PopupAnchor, f32, f32)>,
    hover: Option<PopupAnchor>,
    editor_hover: Option<PopupAnchor>,
    diff_peek: Option<PopupAnchor>,
    signature_help: Option<PopupAnchor>,
    completion_layout_out: &mut Option<quadraui::CompletionsLayout>,
    editor_hover_link_rects_out: &mut Vec<(quadraui::Rect, String)>,
    editor_hover_popup_rect_out: &mut Option<quadraui::Rect>,
    editor_hover_scrollbar_out: &mut Option<PopupScrollbarHit>,
) {
    *completion_layout_out = None;
    editor_hover_link_rects_out.clear();
    *editor_hover_popup_rect_out = None;
    *editor_hover_scrollbar_out = None;

    // ── Completion popup (rendered on top of editor) ───────────────────────
    if let (Some(menu), Some((anchor, popup_width, max_popup_height))) =
        (&screen.completion, completion)
    {
        let completions = completion_menu_to_quadraui_completions(menu);
        let layout = completions.layout(
            anchor.x,
            anchor.y,
            unit_h,
            anchor.viewport,
            popup_width,
            max_popup_height,
            |_| quadraui::CompletionItemMeasure::new(unit_h),
        );
        backend.draw_completions(&completions, &layout);
        *completion_layout_out = Some(layout);
    }

    // ── Hover popup (rendered on top of editor) ──────────────────────────────
    if let (Some(hv), Some(anchor)) = (&screen.hover, hover) {
        let (tooltip, layout) = hover_popup_to_quadraui_tooltip(
            hv,
            anchor.x,
            anchor.y,
            anchor.viewport,
            unit_w,
            unit_h,
        );
        backend.draw_tooltip(&tooltip, &layout);
    }

    // ── Editor hover popup (rich markdown, triggered by gh or mouse dwell) ─
    if let (Some(eh), Some(anchor)) = (&screen.editor_hover, editor_hover) {
        let (links, rect, sb) = editor_hover_popup_paint(
            backend,
            eh,
            anchor.x,
            anchor.y,
            anchor.viewport,
            theme,
            unit_w,
            unit_h,
        );
        *editor_hover_link_rects_out = links;
        *editor_hover_popup_rect_out = rect;
        *editor_hover_scrollbar_out = sb;
    }

    // ── Diff peek popup (inline git hunk preview) ──────────────────────────
    if let (Some(peek), Some(anchor)) = (&screen.diff_peek, diff_peek) {
        let (tooltip, layout) = diff_peek_to_quadraui_tooltip(
            peek,
            anchor.x,
            anchor.y,
            anchor.viewport,
            theme,
            unit_w,
            unit_h,
        );
        backend.draw_tooltip(&tooltip, &layout);
    }

    // ── Signature-help popup (shown in insert mode when cursor is inside a call) ─
    if let (Some(sig), Some(anchor)) = (&screen.signature_help, signature_help) {
        let (tooltip, layout) = signature_help_to_quadraui_tooltip(
            sig,
            anchor.x,
            anchor.y,
            anchor.viewport,
            theme,
            unit_w,
            unit_h,
        );
        backend.draw_tooltip(&tooltip, &layout);
    }
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
    let fg = theme.hover_fg;
    let kw = theme.keyword;

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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Completion popup → quadraui primitive (#1805) ─────────────────────

    /// A plugin-sourced `CompletionCandidate`'s `kind`/`detail`/
    /// `documentation` must thread all the way into the quadraui
    /// `CompletionItem` the popup actually paints — not just sit on the
    /// engine-side struct unused. Buffer-word/LSP candidates (`kind =
    /// Text`, no detail/documentation) must keep painting exactly as
    /// before (`CompletionKind::Text`, both `None`).
    #[test]
    fn completion_menu_to_quadraui_completions_threads_kind_detail_and_documentation() {
        use crate::core::completion::{CompletionCandidate, CompletionItemKind};

        let menu = CompletionMenu {
            candidates: vec![
                CompletionCandidate::plain("plain_word".to_string()),
                CompletionCandidate {
                    label: "foobar_plugin(..)".to_string(),
                    insert_text: "foobar_plugin".to_string(),
                    kind: CompletionItemKind::Function,
                    detail: Some("fn() -> ()".to_string()),
                    documentation: Some("docs".to_string()),
                    priority: 10,
                },
            ],
            selected_idx: 1,
            max_width: 18,
        };

        let completions = completion_menu_to_quadraui_completions(&menu);
        assert_eq!(completions.items.len(), 2);
        assert_eq!(completions.selected_idx, 1);

        let plain = &completions.items[0];
        assert_eq!(plain.label, quadraui::StyledText::plain("plain_word"));
        assert_eq!(plain.kind, quadraui::CompletionKind::Text);
        assert_eq!(plain.detail, None);
        assert_eq!(plain.documentation, None);

        let plugin_item = &completions.items[1];
        assert_eq!(
            plugin_item.label,
            quadraui::StyledText::plain("foobar_plugin(..)")
        );
        assert_eq!(plugin_item.kind, quadraui::CompletionKind::Function);
        assert_eq!(
            plugin_item.detail,
            Some(quadraui::StyledText::plain("fn() -> ()"))
        );
        assert_eq!(
            plugin_item.documentation,
            Some(quadraui::StyledText::plain("docs"))
        );
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
        let kw = theme.keyword;
        let fg = theme.hover_fg;
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
        let fg = theme.hover_fg;
        assert_eq!(styled.spans[1].fg, Some(fg));
    }
}
