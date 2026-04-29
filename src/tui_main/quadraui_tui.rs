//! TUI backend for `quadraui` primitives.
//!
//! This module provides `draw_*` free functions that render `quadraui`
//! primitives into a ratatui `Buffer`. Over time this file will grow to
//! cover every primitive; currently supports `TreeView` (A.1a),
//! `Form` (A.3a), `Palette` (A.4), and `ListView` (A.5).

use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color as RatatuiColor;

/// Convert a `quadraui::Color` to the ratatui palette colour used by
/// `set_cell`.
fn qc(c: quadraui::Color) -> RatatuiColor {
    RatatuiColor::Rgb(c.r, c.g, c.b)
}

/// Draw a `quadraui::TreeView` into `area` on `buf`, using the app-supplied
/// theme for default colours (row background, selection background, dim
/// foreground). Per-row colours carried inside the `TreeRow` (from
/// `StyledSpan` / `Badge`) override the theme defaults.
///
/// Build the backend-agnostic `quadraui::Theme` from vimcode's rich
/// `render::Theme`. Shared by every public-rasteriser delegate so each
/// migration adds the field it needs in one place. Lift sequence is
/// driven by #223 (chrome) and #276 (editor).
///
/// Composes [`q_theme_chrome`] (~36 fields covering chrome / popup /
/// list / dialog primitives) with [`q_theme_editor`] (~29 fields
/// covering the editor viewport — gutter, syntax, diagnostics,
/// cursor, selection, diff, indent guides, annotations). The split
/// keeps each section comprehensible as the field count grows.
pub(super) fn q_theme(theme: &Theme) -> quadraui::Theme {
    let chrome = q_theme_chrome(theme);
    q_theme_editor(theme, chrome)
}

/// Map vimcode chrome colours into the chrome-shaped subset of
/// `quadraui::Theme`. Returns a `Theme` whose editor-lift fields are
/// at their `Default` values; the caller (typically [`q_theme`])
/// overlays them via [`q_theme_editor`].
fn q_theme_chrome(theme: &Theme) -> quadraui::Theme {
    let q = render::to_quadraui_color;
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
        // The TUI vertical scrollbar historically used `theme.separator`
        // for the track shade — `theme.scrollbar_track` is set to the
        // editor bg in many themes (onedark #1a1a1a == bg) which makes
        // the track invisible. Mapping separator preserves the visible
        // pre-Stage-2 paint.
        scrollbar_track: q(theme.separator),
        scrollbar_thumb: q(theme.scrollbar_thumb),
        ..quadraui::Theme::default()
    }
}

/// Overlay the editor-lift colours (#276) onto a chrome-shaped
/// `quadraui::Theme`. The `chrome` argument carries the ~36 chrome
/// fields populated by [`q_theme_chrome`]; this function sets the
/// ~29 editor fields and returns the merged theme.
fn q_theme_editor(theme: &Theme, chrome: quadraui::Theme) -> quadraui::Theme {
    let q = render::to_quadraui_color;
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

/// Draw a `quadraui::ActivityBar` as a vertical icon strip.
///
/// Top items render from the top edge downward, one row per item.
/// Bottom items render from the bottom edge upward. If the two groups
/// would overlap (area too small), bottom items win and top items are
/// clipped. Each item occupies a single row (no row height beyond 1),
/// and the icon is painted at `area.x + 1` to leave the left column
/// free for the active-item accent bar `▎`.
///
/// Keyboard-selected items get a full-row selection-bg fill; active
/// items get a left-edge accent bar (unless keyboard-selected, where
/// the selection bg takes precedence).
/// Draw a `quadraui::ContextMenu` popup via its D6 `ContextMenuLayout`.
/// Matches the pre-migration chrome: thin box border, selected item
/// rendered inverted, separators as a horizontal dash line, disabled
/// items dimmed. Shortcut (from item.detail) is right-aligned.
pub(super) fn draw_context_menu(
    buf: &mut Buffer,
    menu: &quadraui::ContextMenu,
    layout: &quadraui::ContextMenuLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_context_menu(buf, menu, layout, &q_theme(theme));
}

/// Draw a `quadraui::Completions` popup via the lifted
/// `quadraui::tui::draw_completions` rasteriser (#266). Vimcode's shim
/// role is to map the rich `render::Theme` to the smaller
/// `quadraui::Theme` via `q_theme()` — the body of the rasteriser
/// lives in the quadraui crate.
pub(super) fn draw_completions(
    buf: &mut Buffer,
    completions: &quadraui::Completions,
    layout: &quadraui::CompletionsLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_completions(buf, completions, layout, &q_theme(theme));
}

/// Draw a `quadraui::Dialog` via its D6 `DialogLayout`. Handles the
/// rounded-border chrome the TUI has always drawn and respects
/// horizontal vs. vertical button layout.
///
/// The body text may contain embedded `\n` for multi-line messages —
/// each line is drawn on its own row inside `layout.body_bounds`.
pub(super) fn draw_dialog(
    buf: &mut Buffer,
    dialog: &quadraui::Dialog,
    layout: &quadraui::DialogLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_dialog(buf, dialog, layout, &q_theme(theme));
}

/// Draw a `quadraui::Tooltip` into `layout.bounds` on `buf`. Renders a
/// text box with side-bar borders only (`│` on the first and last
/// columns, no top/bottom border) — matches the visual style used by
/// the LSP hover popup and signature help.
///
/// If `tooltip.styled` is `Some`, a single line of styled spans is
/// rendered (signature help path). Otherwise `tooltip.text` is split
/// on `\n` and each line is rendered plain (hover popup path). Lines
/// that exceed the box width are truncated.
pub(super) fn draw_tooltip(
    buf: &mut Buffer,
    tooltip: &quadraui::Tooltip,
    layout: &quadraui::TooltipLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_tooltip(buf, tooltip, layout, &q_theme(theme));
}

pub(super) fn draw_activity_bar(
    buf: &mut Buffer,
    area: Rect,
    bar: &quadraui::ActivityBar,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bar_bg = rc(theme.tab_bar_bg);
    let icon_fg = rc(theme.activity_bar_fg);
    let accent_fg = bar.active_accent.map(qc).unwrap_or(rc(theme.cursor));
    let sel_bg = bar.selection_bg.map(qc).unwrap_or(rc(theme.cursor));

    // Fill the entire strip with the bar background.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', icon_fg, bar_bg);
        }
    }

    // Per D6: ask the primitive for a layout. ActivityBar uses
    // uniform 1-cell rows in TUI; the layout handles top/bottom
    // pinning and collision (bottom wins).
    let layout = bar.layout(area.width as f32, area.height as f32, 1.0);

    for visible in &layout.visible_items {
        let y = area.y + visible.bounds.y.round() as u16;
        let item = match visible.side {
            quadraui::ActivitySide::Top => &bar.top_items[visible.item_idx],
            quadraui::ActivitySide::Bottom => &bar.bottom_items[visible.item_idx],
        };
        let row_bg = if item.is_keyboard_selected {
            sel_bg
        } else {
            bar_bg
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', icon_fg, row_bg);
        }
        if area.width >= 3 {
            let icon_ch = item.icon.chars().next().unwrap_or('?');
            set_cell(buf, area.x + 1, y, icon_ch, icon_fg, row_bg);
        }
        if item.is_active && !item.is_keyboard_selected {
            set_cell(buf, area.x, y, '\u{258E}', accent_fg, bar_bg); // ▎
        }
    }
}

/// Draw one row of a `quadraui::Terminal` cell grid into a ratatui buffer.
///
/// `start_x` / `screen_row` are the destination cell coordinates;
/// `max_cols` clips the row to the visible width. `theme` supplies
/// fallback colours for find-match overlays — the cell's own `fg` / `bg`
/// win for normal cells and cursor/selection (which use inverted colours).
///
/// Caller iterates rows externally so the per-row terminal panel
/// decoration (gutter / focus bar / scroll padding) layers naturally.
/// Building the full `quadraui::Terminal` primitive once per frame and
/// dispatching here per row keeps allocations bounded — every cell is
/// drawn from already-owned data.
pub(super) fn draw_terminal_row(
    buf: &mut Buffer,
    row: &[quadraui::TerminalCell],
    start_x: u16,
    screen_row: u16,
    max_cols: u16,
    theme: &Theme,
) {
    for (col_idx, cell) in row.iter().enumerate() {
        let x = start_x + col_idx as u16;
        if x >= start_x + max_cols {
            break;
        }
        let fg = qc(cell.fg);
        let bg = qc(cell.bg);
        let (draw_fg, draw_bg) = if cell.is_cursor || cell.selected {
            (bg, fg)
        } else if cell.is_find_active {
            (rc(theme.search_match_fg), rc(theme.search_current_match_bg))
        } else if cell.is_find_match {
            (rc(theme.search_match_fg), rc(theme.search_match_bg))
        } else {
            (fg, bg)
        };
        let ch = if cell.ch == '\0' { ' ' } else { cell.ch };
        let mut modifier = ratatui::style::Modifier::empty();
        if cell.bold {
            modifier |= ratatui::style::Modifier::BOLD;
        }
        if cell.italic {
            modifier |= ratatui::style::Modifier::ITALIC;
        }
        if cell.underline {
            modifier |= ratatui::style::Modifier::UNDERLINED;
        }
        if modifier.is_empty() {
            set_cell(buf, x, screen_row, ch, draw_fg, draw_bg);
        } else {
            super::set_cell_styled(buf, x, screen_row, ch, draw_fg, draw_bg, modifier, None);
        }
    }
}

/// Draw the find/replace overlay by walking `panel.hit_regions` (the
/// shared cross-backend layout source-of-truth from
/// `core::engine::compute_find_replace_hit_regions`). Painting and
/// hit-test then derive from the same `FrHitRegion` list, so column
/// drift bugs (the same class fixed for debug toolbar + breadcrumb)
/// can't recur on this overlay.
///
/// `editor_left` is the absolute screen column of the editor area's
/// left edge (after activity bar + sidebar). `panel.group_bounds.x/y`
/// are content-relative; the overlay anchors at the top-right of the
/// active editor group.
///
/// Painting that the hit-region list doesn't directly cover —
/// borders, the match-count text (a non-clickable status string), and
/// the focused field's cursor + selection — is layered in around the
/// region-driven dispatch.
pub(super) fn draw_find_replace(
    buf: &mut Buffer,
    area: Rect,
    panel: &crate::render::FindReplacePanel,
    theme: &Theme,
    editor_left: u16,
) {
    quadraui::tui::draw_find_replace(buf, area, panel, &q_theme(theme), editor_left);
}

/// Draw a `quadraui::RichTextPopup` into the buffer via the lifted
/// `quadraui::tui::draw_rich_text_popup` rasteriser (#266). Vimcode's
/// shim role is to map the rich `render::Theme` to the smaller
/// `quadraui::Theme` via `q_theme()` — the body of the rasteriser
/// lives in the quadraui crate.
pub(super) fn draw_rich_text_popup(
    buf: &mut Buffer,
    popup: &quadraui::RichTextPopup,
    layout: &quadraui::RichTextPopupLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_rich_text_popup(buf, popup, layout, &q_theme(theme));
}
