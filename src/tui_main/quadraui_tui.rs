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
/// Respects `tree.scroll_offset` and clips rows that fall outside `area`.
/// Does not draw a scrollbar yet — scrollbars are a later primitive stage.
pub(super) fn draw_tree(buf: &mut Buffer, area: Rect, tree: &quadraui::TreeView, theme: &Theme) {
    quadraui::tui::draw_tree(
        buf,
        area,
        tree,
        &q_theme(theme),
        crate::icons::nerd_fonts_enabled(),
    );
}

/// Draw a `quadraui::Form` into `area` on `buf`.
///
/// Layout: one row per field. Label on the left, input on the right.
/// Headers (`FieldKind::Label`) span the full width in bold.
/// Disabled fields render dimmed and their inputs are bracketed the
/// same as enabled ones (the app skips them during focus navigation).
///
/// Focused field gets a subtle background tint. Text input cursor
/// and selection (A.3d) are rendered when the `TextInput` field's
/// `cursor` / `selection_anchor` are `Some(_)`.
pub(super) fn draw_form(buf: &mut Buffer, area: Rect, form: &quadraui::Form, theme: &Theme) {
    quadraui::tui::draw_form(buf, area, form, &q_theme(theme));
}

/// Build the backend-agnostic `quadraui::Theme` from vimcode's rich
/// `render::Theme`. Shared by every public-rasteriser delegate so each
/// migration adds the field it needs in one place. Lift sequence is
/// driven by #223.
pub(super) fn q_theme(theme: &Theme) -> quadraui::Theme {
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
    }
}

pub(super) fn draw_tab_bar(
    buf: &mut Buffer,
    area: Rect,
    bar: &quadraui::TabBar,
    layout: &quadraui::TabBarLayout,
    theme: &Theme,
) -> usize {
    quadraui::tui::draw_tab_bar(buf, area, bar, layout, &q_theme(theme))
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

/// Draw a `quadraui::Completions` popup via its D6 `CompletionsLayout`.
/// Thin vertical list with side borders, matching the pre-migration
/// `render_completion_popup` chrome.
pub(super) fn draw_completions(
    buf: &mut Buffer,
    completions: &quadraui::Completions,
    layout: &quadraui::CompletionsLayout,
    theme: &Theme,
) {
    let bg = rc(theme.completion_bg);
    let sel_bg = rc(theme.completion_selected_bg);
    let fg = rc(theme.completion_fg);
    let border = rc(theme.completion_border);

    let x = layout.bounds.x.round() as u16;
    let y = layout.bounds.y.round() as u16;
    let w = layout.bounds.width.round() as u16;
    if w < 3 {
        return;
    }

    for vis in &layout.visible_items {
        let item = &completions.items[vis.item_idx];
        let row_y = y + (vis.bounds.y - layout.bounds.y).round() as u16;
        let is_selected = vis.item_idx == completions.selected_idx;
        let row_bg = if is_selected { sel_bg } else { bg };

        // Fill the row background.
        for col in 0..w {
            set_cell(buf, x + col, row_y, ' ', fg, row_bg);
        }
        // Left + right borders.
        set_cell(buf, x, row_y, '│', border, bg);
        set_cell(buf, x + w - 1, row_y, '│', border, bg);

        // Render the candidate text starting at col 2 (after border + space).
        let label = item
            .label
            .spans
            .first()
            .map(|s| s.text.as_str())
            .unwrap_or("");
        let display = format!(" {label}");
        for (j, ch) in display.chars().enumerate() {
            let col = x + 1 + j as u16;
            if col + 1 >= x + w {
                break;
            }
            set_cell(buf, col, row_y, ch, fg, row_bg);
        }
    }
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
    use crate::core::engine::FindReplaceClickTarget as T;

    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let border_fg = rc(theme.fuzzy_border);
    let accent_bg = rc(theme.tab_active_accent);
    let sel_bg = rc(theme.selection);
    let btn_sel_bg = rc(theme.fuzzy_selected_bg);

    let panel_w: u16 = 50.min(area.width.saturating_sub(2));
    let row_count: u16 = if panel.show_replace { 2 } else { 1 };
    let panel_h: u16 = row_count + 2; // +2 for top/bottom borders

    // Position: top-right of active editor group.
    let gb = &panel.group_bounds;
    let gb_right = editor_left + gb.x as u16 + gb.width as u16;
    let x = gb_right.saturating_sub(panel_w + 1).max(editor_left);
    let y = (gb.y as u16).max(1);
    if panel_w == 0 || panel_h == 0 {
        return;
    }

    // Clear background.
    for row in y..y + panel_h {
        for col in x..x + panel_w {
            if col < area.width && row < area.height {
                set_cell(buf, col, row, ' ', fg, bg);
            }
        }
    }

    // Borders.
    for col in x..x + panel_w {
        set_cell(buf, col, y, '─', border_fg, bg);
    }
    set_cell(buf, x, y, '┌', border_fg, bg);
    if x + panel_w > 0 {
        set_cell(buf, x + panel_w - 1, y, '┐', border_fg, bg);
    }
    let bot = y + panel_h - 1;
    for col in x..x + panel_w {
        set_cell(buf, col, bot, '─', border_fg, bg);
    }
    set_cell(buf, x, bot, '└', border_fg, bg);
    if x + panel_w > 0 {
        set_cell(buf, x + panel_w - 1, bot, '┘', border_fg, bg);
    }
    for row in y + 1..bot {
        set_cell(buf, x, row, '│', border_fg, bg);
        if x + panel_w > 0 {
            set_cell(buf, x + panel_w - 1, row, '│', border_fg, bg);
        }
    }

    // Content origin: 1 cell inside the borders.
    let content_x = x + 1;
    let content_y = y + 1;
    let right_edge = x + panel_w - 1;

    // Helper: paint a string at a region (col is content-relative).
    let paint_at = |buf: &mut Buffer,
                    region_col: u16,
                    region_row: u16,
                    text: &str,
                    cell_fg: ratatui::style::Color,
                    cell_bg: ratatui::style::Color,
                    width: u16| {
        let row_y = content_y + region_row;
        for (i, ch) in text.chars().enumerate() {
            let cx = content_x + region_col + i as u16;
            if cx >= content_x + width.saturating_add(region_col) {
                break;
            }
            if cx < right_edge {
                set_cell(buf, cx, row_y, ch, cell_fg, cell_bg);
            }
        }
    };

    // Helper: paint a single character at a region.
    let paint_char = |buf: &mut Buffer,
                      region_col: u16,
                      region_row: u16,
                      ch: char,
                      cell_fg: ratatui::style::Color,
                      cell_bg: ratatui::style::Color| {
        let row_y = content_y + region_row;
        let cx = content_x + region_col;
        if cx < right_edge {
            set_cell(buf, cx, row_y, ch, cell_fg, cell_bg);
        }
    };

    // Helper: paint a focused input field's text + cursor + selection.
    // `text` is the field contents; the field starts at `region_col`
    // (content-relative) on `region_row` and is `region_w` wide.
    let paint_input = |buf: &mut Buffer,
                       text: &str,
                       region_col: u16,
                       region_row: u16,
                       region_w: u16,
                       focused: bool,
                       cursor: usize,
                       sel_anchor: Option<usize>| {
        let row_y = content_y + region_row;
        // Body text.
        for (i, ch) in text.chars().enumerate() {
            let cx = content_x + region_col + i as u16;
            if cx >= content_x + region_col + region_w {
                break;
            }
            if cx < right_edge {
                set_cell(buf, cx, row_y, ch, fg, bg);
            }
        }
        if !focused {
            return;
        }
        // Selection (drawn before cursor so cursor wins on overlap).
        if let Some(anchor) = sel_anchor {
            let s = anchor.min(cursor) as u16;
            let e = anchor.max(cursor) as u16;
            for i in s..e {
                let cx = content_x + region_col + i;
                if cx >= content_x + region_col + region_w {
                    break;
                }
                if cx < right_edge {
                    let ch = text.chars().nth(i as usize).unwrap_or(' ');
                    set_cell(buf, cx, row_y, ch, fg, sel_bg);
                }
            }
        }
        // Cursor block.
        let cursor_col = content_x + region_col + cursor as u16;
        if cursor_col < content_x + region_col + region_w && cursor_col < right_edge {
            let ch = text.chars().nth(cursor).unwrap_or(' ');
            set_cell(buf, cursor_col, row_y, ch, bg, fg);
        }
    };

    // Walk hit regions, painting per target. The match count is NOT
    // a hit region (it's status text, non-clickable), so we paint it
    // separately by reading the gap between the last toggle (.*) and
    // PrevMatch (the first nav button).
    let mut regex_end_col: Option<u16> = None;
    let mut prev_match_col: Option<u16> = None;

    for (region, target) in &panel.hit_regions {
        match target {
            T::Chevron => {
                let chevron = if panel.show_replace { '▼' } else { '▶' };
                paint_char(buf, region.col, region.row, chevron, fg, bg);
            }
            T::FindInput(_) => {
                paint_input(
                    buf,
                    &panel.query,
                    region.col,
                    region.row,
                    region.width,
                    panel.focus == 0,
                    panel.cursor,
                    panel.sel_anchor,
                );
            }
            T::ReplaceInput(_) => {
                paint_input(
                    buf,
                    &panel.replacement,
                    region.col,
                    region.row,
                    region.width,
                    panel.focus == 1,
                    panel.cursor,
                    panel.sel_anchor,
                );
            }
            T::ToggleCase => {
                let (t_fg, t_bg) = if panel.case_sensitive {
                    (bg, accent_bg)
                } else {
                    (fg, bg)
                };
                paint_at(buf, region.col, region.row, "Aa", t_fg, t_bg, region.width);
            }
            T::ToggleWholeWord => {
                let (t_fg, t_bg) = if panel.whole_word {
                    (bg, accent_bg)
                } else {
                    (fg, bg)
                };
                paint_at(buf, region.col, region.row, "ab", t_fg, t_bg, region.width);
            }
            T::ToggleRegex => {
                let (t_fg, t_bg) = if panel.use_regex {
                    (bg, accent_bg)
                } else {
                    (fg, bg)
                };
                paint_at(buf, region.col, region.row, ".*", t_fg, t_bg, region.width);
                regex_end_col = Some(region.col + region.width);
            }
            T::PrevMatch => {
                paint_char(buf, region.col, region.row, '↑', fg, bg);
                prev_match_col.get_or_insert(region.col);
            }
            T::NextMatch => {
                paint_char(buf, region.col, region.row, '↓', fg, bg);
            }
            T::ToggleInSelection => {
                let (n_fg, n_bg) = if panel.in_selection {
                    (bg, accent_bg)
                } else {
                    (fg, bg)
                };
                paint_char(buf, region.col, region.row, '≡', n_fg, n_bg);
            }
            T::Close => {
                paint_char(buf, region.col, region.row, '×', fg, bg);
            }
            T::TogglePreserveCase => {
                let (ab_fg, ab_bg) = if panel.preserve_case {
                    (bg, accent_bg)
                } else {
                    (fg, bg)
                };
                paint_at(
                    buf,
                    region.col,
                    region.row,
                    "AB",
                    ab_fg,
                    ab_bg,
                    region.width,
                );
            }
            T::ReplaceCurrent => {
                paint_at(
                    buf,
                    region.col,
                    region.row,
                    crate::icons::FIND_REPLACE.s(),
                    fg,
                    btn_sel_bg,
                    region.width,
                );
            }
            T::ReplaceAll => {
                paint_at(
                    buf,
                    region.col,
                    region.row,
                    crate::icons::FIND_REPLACE_ALL.s(),
                    fg,
                    btn_sel_bg,
                    region.width,
                );
            }
        }
    }

    // Match count text — sits between the regex toggle and the
    // PrevMatch arrow, in find row (row 0). Non-clickable, so it has
    // no FrHitRegion entry; positions are derived from neighbours.
    if let (Some(start_col), Some(end_col)) = (regex_end_col, prev_match_col) {
        // 1-cell gap on the left after the regex toggle.
        let info_col = start_col + 1;
        let info_w = end_col.saturating_sub(info_col + 1); // 1-cell gap before PrevMatch
        if info_w > 0 {
            paint_at(buf, info_col, 0, &panel.match_info, fg, bg, info_w);
        }
    }
}

/// Draw a `quadraui::RichTextPopup` into the buffer.
///
/// Per D6: the caller invokes `popup.layout(...)` to get
/// `RichTextPopupLayout` (bounds + visible_lines + scrollbar +
/// link_hit_regions), and this rasteriser paints them verbatim.
///
/// Renders: bordered box with focus-tinted border colour, per-cell
/// styled spans (fg + bold + italic + underline), text-selection
/// inversion, focused-link underline, and a thumb scrollbar on the
/// right border when content overflows.
pub(super) fn draw_rich_text_popup(
    buf: &mut Buffer,
    popup: &quadraui::RichTextPopup,
    layout: &quadraui::RichTextPopupLayout,
    theme: &Theme,
) {
    let bx = layout.bounds.x.round() as u16;
    let by = layout.bounds.y.round() as u16;
    let bw = layout.bounds.width.round() as u16;
    let bh = layout.bounds.height.round() as u16;
    if bw == 0 || bh == 0 {
        return;
    }

    let bg = popup.bg.map(qc).unwrap_or_else(|| rc(theme.hover_bg));
    let fg = popup.fg.map(qc).unwrap_or_else(|| rc(theme.hover_fg));
    let border = if popup.has_focus {
        rc(theme.md_link)
    } else {
        rc(theme.hover_border)
    };

    // Top border with corners.
    for c in 0..bw {
        let cx = bx + c;
        let ch = if c == 0 {
            '┌'
        } else if c == bw - 1 {
            '┐'
        } else {
            '─'
        };
        set_cell(buf, cx, by, ch, border, bg);
    }
    // Bottom border.
    for c in 0..bw {
        let cx = bx + c;
        let ch = if c == 0 {
            '└'
        } else if c == bw - 1 {
            '┘'
        } else {
            '─'
        };
        set_cell(buf, cx, by + bh - 1, ch, border, bg);
    }
    // Side borders + content fill for inner rows.
    for row in 1..bh - 1 {
        set_cell(buf, bx, by + row, '│', border, bg);
        set_cell(buf, bx + bw - 1, by + row, '│', border, bg);
        for col in 1..bw - 1 {
            set_cell(buf, bx + col, by + row, ' ', fg, bg);
        }
    }

    // Visible lines: walk styled spans char-by-char.
    for vis in &layout.visible_lines {
        let row_y = vis.bounds.y.round() as u16;
        let line_x = vis.bounds.x.round() as u16;
        let line_w = vis.bounds.width.round() as u16;
        if row_y >= by + bh - 1 {
            break;
        }

        let line_idx = vis.line_idx;
        let styled = popup.lines.get(line_idx);
        let raw_text = popup
            .line_text
            .get(line_idx)
            .map(String::as_str)
            .unwrap_or("");

        let focused_link_range = popup.focused_link.and_then(|fi| {
            popup
                .links
                .get(fi)
                .filter(|l| l.line == line_idx)
                .map(|l| (l.start_byte, l.end_byte))
        });

        let mut col_off: u16 = 0;
        let mut byte_pos: usize = 0;
        if let Some(styled) = styled {
            for span in &styled.spans {
                let span_fg = span.fg.map(qc).unwrap_or(fg);
                let span_bg = span.bg.map(qc).unwrap_or(bg);
                for ch in span.text.chars() {
                    if col_off >= line_w {
                        break;
                    }
                    let cx = line_x + col_off;
                    let char_col = col_off as usize;

                    // Selection inversion.
                    let in_selection = popup
                        .selection
                        .map(|s| s.contains(line_idx, char_col))
                        .unwrap_or(false);
                    let (cell_fg, cell_bg) = if in_selection {
                        (bg, span_fg)
                    } else {
                        (span_fg, span_bg)
                    };
                    set_cell(buf, cx, row_y, ch, cell_fg, cell_bg);

                    // Focused-link underline.
                    if popup.has_focus
                        && !in_selection
                        && focused_link_range
                            .map(|(s, e)| byte_pos >= s && byte_pos < e)
                            .unwrap_or(false)
                    {
                        if let Some(cell) =
                            buf.cell_mut(ratatui::prelude::Position { x: cx, y: row_y })
                        {
                            cell.set_style(
                                cell.style()
                                    .add_modifier(ratatui::style::Modifier::UNDERLINED),
                            );
                        }
                    }

                    col_off += 1;
                    byte_pos += ch.len_utf8();
                }
            }
        }
        // Pad the rest of the line with bg.
        while col_off < line_w {
            let cx = line_x + col_off;
            let in_selection = popup
                .selection
                .map(|s| s.contains(line_idx, col_off as usize))
                .unwrap_or(false);
            let cell_bg = if in_selection { fg } else { bg };
            set_cell(buf, cx, row_y, ' ', fg, cell_bg);
            col_off += 1;
        }
        let _ = raw_text;
    }

    // Scrollbar thumb on the right border, when present.
    if let Some(sb) = layout.scrollbar {
        let track_x = sb.track.x.round() as u16;
        let track_y = sb.track.y.round() as u16;
        let track_h = sb.track.height.round() as u16;
        let thumb_y = sb.thumb.y.round() as u16;
        let thumb_h = sb.thumb.height.round() as u16;
        for r in 0..track_h {
            let cy = track_y + r;
            let in_thumb = cy >= thumb_y && cy < thumb_y + thumb_h;
            let ch = if in_thumb { '█' } else { '░' };
            let cell_fg = if in_thumb { border } else { fg };
            set_cell(buf, track_x, cy, ch, cell_fg, bg);
        }
    }
}
