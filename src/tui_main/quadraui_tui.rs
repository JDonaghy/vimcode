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
    if area.width == 0 || area.height == 0 {
        return;
    }

    let row_bg = rc(theme.tab_bar_bg);
    let hdr_bg = rc(theme.status_bg);
    let hdr_fg = rc(theme.status_fg);
    let item_fg = rc(theme.foreground);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let dim_fg = rc(theme.line_number_fg);

    let indent_cells = tree.style.indent as usize;

    // Per D6: ask the primitive for a layout. TreeView is row-uniform
    // in TUI (1 cell per row), so the measurer returns constant 1.0.
    let layout = tree.layout(area.width as f32, area.height as f32, |_| {
        quadraui::TreeRowMeasure::new(1.0)
    });

    for visible_row in &layout.visible_rows {
        let row = &tree.rows[visible_row.row_idx];
        let y = area.y + visible_row.bounds.y.round() as u16;
        if y >= area.y + area.height {
            break;
        }

        let is_header = matches!(row.decoration, quadraui::Decoration::Header);
        let is_selected =
            tree.has_focus && tree.selected_path.as_ref().is_some_and(|p| p == &row.path);

        // Header rows (e.g. SC section titles) get a distinct background;
        // ordinary branches (e.g. folders in the file explorer) render
        // like leaves so they don't stand out from sibling files. Selection
        // takes priority over both.
        let (default_fg, bg) = match (is_header, is_selected) {
            (_, true) => (hdr_fg, sel_bg),
            (true, false) => (hdr_fg, hdr_bg),
            (false, false) => (item_fg, row_bg),
        };

        // Fill the row background.
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', default_fg, bg);
        }

        // Build the prefix: indent + chevron (for branches) + space.
        let mut col: usize = 0;
        let indent_spaces = (row.indent as usize) * indent_cells;
        col += indent_spaces;

        if let Some(expanded) = row.is_expanded {
            if tree.style.show_chevrons {
                let chevron = if expanded {
                    &tree.style.chevron_expanded
                } else {
                    &tree.style.chevron_collapsed
                };
                for ch in chevron.chars() {
                    if col >= area.width as usize {
                        break;
                    }
                    set_cell(buf, area.x + col as u16, y, ch, default_fg, bg);
                    col += 1;
                }
                // Trailing space after chevron.
                if col < area.width as usize {
                    set_cell(buf, area.x + col as u16, y, ' ', default_fg, bg);
                    col += 1;
                }
            }
        } else {
            // Leaves: small leading gap for readability.
            col += 2.min(area.width as usize - col.min(area.width as usize));
        }

        // Icon (optional).
        if let Some(ref icon) = row.icon {
            let glyph = if crate::icons::nerd_fonts_enabled() {
                &icon.glyph
            } else {
                &icon.fallback
            };
            for ch in glyph.chars() {
                if col >= area.width as usize {
                    break;
                }
                set_cell(buf, area.x + col as u16, y, ch, default_fg, bg);
                col += 1;
            }
            if col < area.width as usize {
                set_cell(buf, area.x + col as u16, y, ' ', default_fg, bg);
                col += 1;
            }
        }

        // Text spans.
        let text_start = col;
        let text_end = draw_styled_text(
            buf,
            area,
            y,
            col,
            &row.text,
            default_fg,
            bg,
            row.decoration,
            dim_fg,
        );
        col = text_end;

        // Badge (right-aligned within area).
        if let Some(ref badge) = row.badge {
            let badge_width: usize = badge.text.chars().count();
            let badge_start_col = (area.width as usize).saturating_sub(badge_width);
            // Only draw if there's room between text and badge.
            if badge_start_col > text_start {
                let badge_fg = badge.fg.map(qc).unwrap_or(dim_fg);
                let badge_bg = badge.bg.map(qc).unwrap_or(bg);
                let mut bx = badge_start_col;
                for ch in badge.text.chars() {
                    if bx >= area.width as usize {
                        break;
                    }
                    set_cell(buf, area.x + bx as u16, y, ch, badge_fg, badge_bg);
                    bx += 1;
                }
            }
        }

        // Silence unused warning if text draw filled the line.
        let _ = col;
    }
}

/// Draw a `StyledText` starting at `col` on row `y`. Returns the column
/// after the last drawn character. Honors `decoration` as a final colour
/// override for the whole line (e.g. `Muted` dims everything that wasn't
/// already coloured).
#[allow(clippy::too_many_arguments)]
fn draw_styled_text(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    start_col: usize,
    text: &quadraui::StyledText,
    default_fg: RatatuiColor,
    bg: RatatuiColor,
    decoration: quadraui::Decoration,
    dim_fg: RatatuiColor,
) -> usize {
    let mut col = start_col;
    for span in &text.spans {
        let span_fg = if let Some(c) = span.fg {
            qc(c)
        } else if matches!(decoration, quadraui::Decoration::Muted) {
            dim_fg
        } else {
            default_fg
        };
        let span_bg = span.bg.map(qc).unwrap_or(bg);
        for ch in span.text.chars() {
            if col >= area.width as usize {
                return col;
            }
            set_cell(buf, area.x + col as u16, y, ch, span_fg, span_bg);
            col += 1;
        }
    }
    col
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
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bg = rc(theme.tab_bar_bg);
    let fg = rc(theme.foreground);
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let dim_fg = rc(theme.line_number_fg);
    let accent_fg = rc(theme.cursor);

    // Per D6: ask the primitive for a layout. TUI keeps rows compact
    // at 1 cell each; GTK uses taller rows.
    let layout = form.layout(area.width as f32, area.height as f32, |_| {
        quadraui::FormFieldMeasure::new(1.0)
    });

    for visible_field in &layout.visible_fields {
        let field = &form.fields[visible_field.field_idx];
        let y = area.y + visible_field.bounds.y.round() as u16;

        let is_focused = form.has_focus
            && form
                .focused_field
                .as_ref()
                .is_some_and(|id| id == &field.id);
        let is_header = matches!(field.kind, quadraui::FieldKind::Label);

        let (default_fg, row_bg) = match (is_header, is_focused) {
            (_, true) => (fg, sel_bg),
            (true, false) => (hdr_fg, hdr_bg),
            (false, false) => (fg, bg),
        };

        // Fill the row.
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', default_fg, row_bg);
        }

        let field_fg = if field.disabled { dim_fg } else { default_fg };

        // Label on the left (column 1 for a small indent).
        let label_col = 1usize;
        let label_end = draw_styled_text(
            buf,
            area,
            y,
            label_col,
            &field.label,
            field_fg,
            row_bg,
            quadraui::Decoration::Normal,
            dim_fg,
        );

        // Input rendering — right-side, aligned to column area.width.saturating_sub(input_width + 1)
        // except headers and read-only display which use whatever space is left on the same row.
        match &field.kind {
            quadraui::FieldKind::Label => {
                // No separate input — label spans the row.
            }
            quadraui::FieldKind::Toggle { value } => {
                let glyph = if *value { "[x]" } else { "[ ]" };
                let w = glyph.chars().count();
                let start_col = (area.width as usize).saturating_sub(w + 2);
                if start_col > label_end + 1 {
                    let input_fg = if *value { accent_fg } else { field_fg };
                    let mut col = start_col;
                    for ch in glyph.chars() {
                        if col >= area.width as usize {
                            break;
                        }
                        set_cell(buf, area.x + col as u16, y, ch, input_fg, row_bg);
                        col += 1;
                    }
                }
            }
            quadraui::FieldKind::TextInput {
                value,
                placeholder,
                cursor,
                selection_anchor,
            } => {
                let shown = if value.is_empty() {
                    placeholder.as_str()
                } else {
                    value.as_str()
                };
                let input_fg = if value.is_empty() { dim_fg } else { field_fg };
                let max_input = (area.width as usize * 2 / 3).max(10);
                let desired = shown.chars().count().min(max_input);
                let start_col = (area.width as usize).saturating_sub(desired + 2);

                // Selection range (byte offsets) — only when value non-empty.
                let (sel_lo, sel_hi) = if value.is_empty() {
                    (0, 0)
                } else {
                    match (cursor, selection_anchor) {
                        (Some(c), Some(a)) if c != a => (*c.min(a), *c.max(a)),
                        _ => (0, 0),
                    }
                };

                if start_col > label_end + 1 {
                    // Left bracket.
                    if start_col > 0 && start_col - 1 < area.width as usize {
                        set_cell(buf, area.x + (start_col - 1) as u16, y, '[', dim_fg, row_bg);
                    }
                    let mut col = start_col;
                    let mut byte = 0usize;
                    for ch in shown.chars().take(desired) {
                        if col >= area.width as usize {
                            break;
                        }
                        let in_selection = sel_hi > sel_lo && byte >= sel_lo && byte < sel_hi;
                        let cell_bg = if in_selection { sel_bg } else { row_bg };
                        set_cell(buf, area.x + col as u16, y, ch, input_fg, cell_bg);
                        col += 1;
                        byte += ch.len_utf8();
                    }
                    // Right bracket.
                    if col < area.width as usize {
                        set_cell(buf, area.x + col as u16, y, ']', dim_fg, row_bg);
                    }

                    // Cursor rendering: invert fg/bg at the cursor position.
                    // Only shown when the value is non-empty; cursor inside
                    // the placeholder text is a later refinement.
                    if let Some(cur) = cursor {
                        if !value.is_empty() {
                            let mut byte = 0usize;
                            let mut char_idx = 0usize;
                            for ch in shown.chars().take(desired) {
                                if byte >= *cur {
                                    break;
                                }
                                byte += ch.len_utf8();
                                char_idx += 1;
                            }
                            let cursor_col = start_col + char_idx;
                            if cursor_col < area.width as usize {
                                let ch = shown.chars().nth(char_idx).unwrap_or(' ');
                                set_cell(buf, area.x + cursor_col as u16, y, ch, row_bg, field_fg);
                            }
                        }
                    }
                }
            }
            quadraui::FieldKind::Button => {
                // The field's label IS the button caption. Redraw it
                // wrapped in angle-brackets on the right side, overwriting
                // the normal label rendering.
                // First blank out the left-side label we already drew.
                for x in area.x..area.x + (label_end as u16).min(area.width) {
                    set_cell(buf, x, y, ' ', default_fg, row_bg);
                }
                let width = field.label.visible_width() + 4; // "< ... >"
                let start_col = (area.width as usize).saturating_sub(width + 1);
                if start_col < area.width as usize {
                    let brk_fg = if is_focused { accent_fg } else { dim_fg };
                    let text_fg = if field.disabled { dim_fg } else { field_fg };
                    set_cell(buf, area.x + start_col as u16, y, '<', brk_fg, row_bg);
                    let after_lt = draw_styled_text(
                        buf,
                        area,
                        y,
                        start_col + 2,
                        &field.label,
                        text_fg,
                        row_bg,
                        quadraui::Decoration::Normal,
                        dim_fg,
                    );
                    if after_lt < area.width as usize {
                        set_cell(buf, area.x + after_lt as u16, y, ' ', brk_fg, row_bg);
                    }
                    if after_lt + 1 < area.width as usize {
                        set_cell(buf, area.x + (after_lt + 1) as u16, y, '>', brk_fg, row_bg);
                    }
                }
            }
            quadraui::FieldKind::ReadOnly { value } => {
                // Draw the read-only value right-aligned, dimmed.
                let w = value.visible_width();
                let start_col = (area.width as usize).saturating_sub(w + 2);
                if start_col > label_end + 1 {
                    draw_styled_text(
                        buf,
                        area,
                        y,
                        start_col,
                        value,
                        dim_fg,
                        row_bg,
                        quadraui::Decoration::Muted,
                        dim_fg,
                    );
                }
            }
            quadraui::FieldKind::Slider {
                value,
                min,
                max,
                step: _,
            } => {
                // Simple TUI slider: " [====----] value " right-aligned.
                let range = (*max - *min).max(f32::EPSILON);
                let frac = ((*value - *min) / range).clamp(0.0, 1.0);
                let track_cells: usize = 12;
                let filled = (frac * track_cells as f32).round() as usize;
                let value_str = format!("{value:.2}");
                let total = track_cells + 2 + value_str.chars().count() + 2; // "[" + track + "]" + space + value
                let start_col = (area.width as usize).saturating_sub(total + 2);
                if start_col > label_end + 1 {
                    let mut col = start_col;
                    set_cell(buf, area.x + col as u16, y, '[', dim_fg, row_bg);
                    col += 1;
                    for i in 0..track_cells {
                        let ch = if i < filled { '=' } else { '-' };
                        let fg = if i < filled { accent_fg } else { dim_fg };
                        set_cell(buf, area.x + col as u16, y, ch, fg, row_bg);
                        col += 1;
                    }
                    set_cell(buf, area.x + col as u16, y, ']', dim_fg, row_bg);
                    col += 2;
                    for ch in value_str.chars() {
                        if col >= area.width as usize {
                            break;
                        }
                        set_cell(buf, area.x + col as u16, y, ch, field_fg, row_bg);
                        col += 1;
                    }
                }
            }
            quadraui::FieldKind::ColorPicker { value } => {
                // Render "■ #rrggbb" with the swatch tinted. TUI can't
                // do real colour picker; the click opens an app-supplied
                // palette.
                let hex = format!("#{:02x}{:02x}{:02x}", value.r, value.g, value.b);
                let total = 2 + hex.chars().count(); // "■ " + hex
                let start_col = (area.width as usize).saturating_sub(total + 2);
                if start_col > label_end + 1 {
                    let swatch_fg = ratatui::style::Color::Rgb(value.r, value.g, value.b);
                    set_cell(
                        buf,
                        area.x + start_col as u16,
                        y,
                        '\u{25A0}',
                        swatch_fg,
                        row_bg,
                    );
                    let mut col = start_col + 2;
                    for ch in hex.chars() {
                        if col >= area.width as usize {
                            break;
                        }
                        set_cell(buf, area.x + col as u16, y, ch, field_fg, row_bg);
                        col += 1;
                    }
                }
            }
            quadraui::FieldKind::Dropdown {
                options,
                selected_idx,
            } => {
                // Render the selected option + a "▾" chevron indicating
                // the dropdown can expand. Apps draw the full list on
                // activation separately.
                let chosen = options.get(*selected_idx).cloned().unwrap_or_default();
                let label_w = chosen.visible_width();
                let total = label_w + 4; // " text ▾ "
                let start_col = (area.width as usize).saturating_sub(total + 1);
                if start_col > label_end + 1 {
                    draw_styled_text(
                        buf,
                        area,
                        y,
                        start_col + 1,
                        &chosen,
                        field_fg,
                        row_bg,
                        quadraui::Decoration::Normal,
                        dim_fg,
                    );
                    let chev_col = start_col + 1 + label_w + 1;
                    if chev_col < area.width as usize {
                        set_cell(buf, area.x + chev_col as u16, y, '\u{25BE}', dim_fg, row_bg);
                    }
                }
            }
        }
    }
}

/// Draw a `quadraui::Palette` modal overlay into `area` on `buf`.
///
/// Layout (matches the pre-migration picker popup minus the preview
/// pane and tree-indent rendering — those cases fall through to the
/// legacy renderer until the primitive carries them):
///
/// ```text
/// ╭─ Title  N/M ──╮
/// │ > query▌      │
/// ├───────────────┤
/// │  Item 1       │
/// │  Item 2       │
/// │  Item 3 detail│
/// ╰───────────────╯
/// ```
///
/// `match_positions` on each item highlight matched characters with
/// the accent `fuzzy_match_fg` colour.
pub(super) fn draw_palette(
    buf: &mut Buffer,
    area: Rect,
    palette: &quadraui::Palette,
    theme: &Theme,
) {
    if area.width < 4 || area.height < 4 {
        return;
    }

    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let query_fg = rc(theme.fuzzy_query_fg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);
    let match_fg = rc(theme.fuzzy_match_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let dim_fg = rc(theme.line_number_fg);

    let x0 = area.x;
    let y0 = area.y;
    let w = area.width;
    let h = area.height;
    let y_end = y0 + h;

    // Clear the popup area so cycling between pickers with different
    // content lengths doesn't leave stale characters behind.
    for y in y0..y_end {
        for x in x0..x0 + w {
            set_cell(buf, x, y, ' ', fg, bg);
        }
    }

    // Top border with title.
    for col in 0..w {
        let ch = if col == 0 {
            '╭'
        } else if col == w - 1 {
            '╮'
        } else {
            '─'
        };
        set_cell(buf, x0 + col, y0, ch, border_fg, bg);
    }
    let title_text = if palette.total_count > 0 {
        format!(
            " {}  {}/{} ",
            palette.title,
            palette.items.len(),
            palette.total_count
        )
    } else {
        format!(" {} ", palette.title)
    };
    for (i, ch) in title_text.chars().enumerate() {
        let col = 2 + i as u16;
        if col + 1 >= w {
            break;
        }
        set_cell(buf, x0 + col, y0, ch, title_fg, bg);
    }

    // Query line.
    if h >= 3 {
        let row = y0 + 1;
        set_cell(buf, x0, row, '│', border_fg, bg);
        if w >= 2 {
            set_cell(buf, x0 + w - 1, row, '│', border_fg, bg);
        }
        let prompt = "> ";
        let mut col = 1u16;
        for ch in prompt.chars() {
            if col + 1 >= w {
                break;
            }
            set_cell(buf, x0 + col, row, ch, query_fg, bg);
            col += 1;
        }
        let query_start = col;
        for ch in palette.query.chars() {
            if col + 1 >= w {
                break;
            }
            set_cell(buf, x0 + col, row, ch, query_fg, bg);
            col += 1;
        }
        // Cursor block: map byte offset → visible column.
        let mut byte = 0usize;
        let mut char_idx = 0usize;
        for ch in palette.query.chars() {
            if byte >= palette.query_cursor {
                break;
            }
            byte += ch.len_utf8();
            char_idx += 1;
        }
        let cursor_col = query_start + char_idx as u16;
        if cursor_col + 1 < w {
            let ch = palette.query.chars().nth(char_idx).unwrap_or(' ');
            set_cell(buf, x0 + cursor_col, row, ch, bg, query_fg);
        }
    }

    // Separator row.
    if h >= 4 {
        let row = y0 + 2;
        for col in 0..w {
            let ch = if col == 0 {
                '├'
            } else if col == w - 1 {
                '┤'
            } else {
                '─'
            };
            set_cell(buf, x0 + col, row, ch, border_fg, bg);
        }
    }

    // Result rows.
    let items_row0 = y0 + 3;
    let items_row_end = y_end - 1;
    let visible_rows = items_row_end.saturating_sub(items_row0) as usize;
    let total = palette.items.len();
    let has_scrollbar = total > visible_rows;
    let item_end_col = if has_scrollbar { w - 2 } else { w - 1 };

    // Clamp scroll_offset so the selected item is always visible. The engine
    // updates scroll_top with a conservative heuristic that doesn't know the
    // actual renderer row count, so the renderer is authoritative here.
    let effective_offset = if visible_rows == 0 {
        0
    } else if palette.selected_idx < palette.scroll_offset {
        palette.selected_idx
    } else if palette.selected_idx >= palette.scroll_offset + visible_rows {
        palette.selected_idx + 1 - visible_rows
    } else {
        palette.scroll_offset
    };

    for (vis_i, item) in palette
        .items
        .iter()
        .enumerate()
        .skip(effective_offset)
        .take(visible_rows)
    {
        let row = items_row0 + (vis_i - effective_offset) as u16;
        if row >= items_row_end {
            break;
        }
        let is_selected = vis_i == palette.selected_idx && palette.has_focus;
        let row_bg = if is_selected { sel_bg } else { bg };

        set_cell(buf, x0, row, '│', border_fg, bg);
        if w >= 2 {
            set_cell(buf, x0 + w - 1, row, '│', border_fg, bg);
        }
        for col in 1..item_end_col {
            set_cell(buf, x0 + col, row, ' ', fg, row_bg);
        }

        let mut col = 2u16;

        // Icon.
        if let Some(ref icon) = item.icon {
            let glyph = if crate::icons::nerd_fonts_enabled() {
                icon.glyph.as_str()
            } else {
                icon.fallback.as_str()
            };
            for ch in glyph.chars() {
                if col >= item_end_col {
                    break;
                }
                set_cell(buf, x0 + col, row, ch, fg, row_bg);
                col += 1;
            }
            if col < item_end_col {
                set_cell(buf, x0 + col, row, ' ', fg, row_bg);
                col += 1;
            }
        }

        // Text — per-character match-highlighting based on byte offsets.
        let full_text: String = item.text.spans.iter().map(|s| s.text.as_str()).collect();
        let mut byte = 0usize;
        for ch in full_text.chars() {
            if col >= item_end_col {
                break;
            }
            let is_match = item.match_positions.contains(&byte);
            let ch_fg = if is_match { match_fg } else { fg };
            set_cell(buf, x0 + col, row, ch, ch_fg, row_bg);
            col += 1;
            byte += ch.len_utf8();
        }
        let text_end_col = col;

        // Detail (right-aligned, dimmed) — only drawn when there's room.
        if let Some(ref detail) = item.detail {
            let detail_text: String = detail.spans.iter().map(|s| s.text.as_str()).collect();
            let detail_w = detail_text.chars().count() as u16;
            if item_end_col > text_end_col + detail_w + 1 {
                let start = item_end_col.saturating_sub(detail_w + 1);
                let mut dcol = start;
                for ch in detail_text.chars() {
                    if dcol >= item_end_col {
                        break;
                    }
                    set_cell(buf, x0 + dcol, row, ch, dim_fg, row_bg);
                    dcol += 1;
                }
            }
        }

        // Scrollbar.
        if has_scrollbar {
            let sb_col = w - 2;
            let track_len = visible_rows;
            let thumb_len = (visible_rows * visible_rows / total.max(1)).max(1);
            let thumb_start = effective_offset * track_len / total.max(1);
            let vi_off = vis_i - effective_offset;
            let in_thumb = vi_off >= thumb_start && vi_off < thumb_start + thumb_len;
            let ch = if in_thumb { '█' } else { '░' };
            set_cell(buf, x0 + sb_col, row, ch, border_fg, bg);
        }
    }

    // Empty rows between last item and bottom border: just borders.
    let drawn = total.saturating_sub(effective_offset).min(visible_rows) as u16;
    for row in items_row0 + drawn..items_row_end {
        set_cell(buf, x0, row, '│', border_fg, bg);
        if w >= 2 {
            set_cell(buf, x0 + w - 1, row, '│', border_fg, bg);
        }
    }

    // Bottom border.
    let row = y_end - 1;
    for col in 0..w {
        let ch = if col == 0 {
            '╰'
        } else if col == w - 1 {
            '╯'
        } else {
            '─'
        };
        set_cell(buf, x0 + col, row, ch, border_fg, bg);
    }
}

/// Draw a `quadraui::ListView` into `area` on `buf`.
///
/// Layout: optional title header (if `list.title` is `Some`), then one
/// row per item. Selected row gets a `▶ ` prefix and `sel_bg`
/// background. Optional icons sit left of the text; optional detail
/// text is right-aligned and dimmed.
pub(super) fn draw_list(buf: &mut Buffer, area: Rect, list: &quadraui::ListView, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.background);
    let dim_fg = rc(theme.line_number_fg);
    let error_fg = rc(theme.diagnostic_error);
    let warn_fg = rc(theme.diagnostic_warning);

    // Per D6: ask the primitive for a layout. Title row is 1 cell if
    // present; items are 1 cell each. Layout resolves which items are
    // visible and their y positions.
    let title_h = if list.title.is_some() { 1.0 } else { 0.0 };
    let layout = list.layout(area.width as f32, area.height as f32, title_h, |_| {
        quadraui::ListItemMeasure::new(1.0)
    });

    // Draw title header (if present).
    if let Some(title_bounds) = layout.title_bounds {
        if let Some(ref title) = list.title {
            let y = area.y + title_bounds.y.round() as u16;
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', hdr_fg, hdr_bg);
            }
            draw_styled_text(
                buf,
                area,
                y,
                1,
                title,
                hdr_fg,
                hdr_bg,
                quadraui::Decoration::Normal,
                dim_fg,
            );
        }
    }

    for visible_item in &layout.visible_items {
        let item = &list.items[visible_item.item_idx];
        let y = area.y + visible_item.bounds.y.round() as u16;
        let is_selected = visible_item.item_idx == list.selected_idx && list.has_focus;
        let bg = if is_selected { sel_bg } else { row_bg };
        let decoration_fg = match item.decoration {
            quadraui::Decoration::Error => error_fg,
            quadraui::Decoration::Warning => warn_fg,
            quadraui::Decoration::Muted => dim_fg,
            _ => fg,
        };

        // Fill row background.
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', decoration_fg, bg);
        }

        let mut col = 0u16;

        // Selection indicator.
        let prefix = if is_selected { "▶ " } else { "  " };
        for ch in prefix.chars() {
            if col >= area.width {
                break;
            }
            set_cell(buf, area.x + col, y, ch, decoration_fg, bg);
            col += 1;
        }

        // Icon (optional).
        if let Some(ref icon) = item.icon {
            let glyph = if crate::icons::nerd_fonts_enabled() {
                icon.glyph.as_str()
            } else {
                icon.fallback.as_str()
            };
            for ch in glyph.chars() {
                if col >= area.width {
                    break;
                }
                set_cell(buf, area.x + col, y, ch, decoration_fg, bg);
                col += 1;
            }
            if col < area.width {
                set_cell(buf, area.x + col, y, ' ', decoration_fg, bg);
                col += 1;
            }
        }

        // Text.
        let text_end_col = draw_styled_text(
            buf,
            area,
            y,
            col as usize,
            &item.text,
            decoration_fg,
            bg,
            item.decoration,
            dim_fg,
        );

        // Detail (right-aligned, dimmed).
        if let Some(ref detail) = item.detail {
            let detail_w: usize = detail.spans.iter().map(|s| s.text.chars().count()).sum();
            let start = (area.width as usize).saturating_sub(detail_w + 1);
            if start > text_end_col + 1 {
                draw_styled_text(
                    buf,
                    area,
                    y,
                    start,
                    detail,
                    dim_fg,
                    bg,
                    quadraui::Decoration::Muted,
                    dim_fg,
                );
            }
        }
    }
}

/// Draw a `quadraui::StatusBar` as a single row.
///
/// `area` is the target rect (height is ignored beyond the first row).
/// Left segments accumulate from the left edge; right segments are
/// right-aligned inside `area.width`. If the two halves would overlap
/// (bar too narrow), the right half wins — left segments are truncated.
///
/// Background fill uses the first segment's `bg` so the bar looks
/// continuous even when gaps exist between left and right halves.
pub(super) fn draw_status_bar(
    buf: &mut Buffer,
    area: Rect,
    bar: &quadraui::StatusBar,
    layout: &quadraui::StatusBarLayout,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let y = area.y;

    let fallback_bg = theme.background;
    let fill_bg = bar
        .left_segments
        .first()
        .or(bar.right_segments.first())
        .map(|s| qc(s.bg))
        .unwrap_or(RatatuiColor::Rgb(
            fallback_bg.r,
            fallback_bg.g,
            fallback_bg.b,
        ));
    for col in 0..area.width {
        set_cell(buf, area.x + col, y, ' ', fill_bg, fill_bg);
    }

    // Iterate layout.visible_segments — positions are already resolved by
    // quadraui::StatusBar::layout() per D6. The rasteriser just paints.
    for vs in &layout.visible_segments {
        let seg = match vs.side {
            quadraui::StatusSegmentSide::Left => &bar.left_segments[vs.segment_idx],
            quadraui::StatusSegmentSide::Right => &bar.right_segments[vs.segment_idx],
        };
        let fg = qc(seg.fg);
        let bg = qc(seg.bg);
        let start_x = area.x + vs.bounds.x.round() as u16;
        let bar_end = area.x + area.width;
        let mut cx = start_x;
        for ch in seg.text.chars() {
            if cx >= bar_end {
                break;
            }
            set_cell(buf, cx, y, ch, fg, bg);
            if seg.bold {
                if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(cx, y)) {
                    cell.set_style(
                        ratatui::style::Style::default()
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    );
                }
            }
            cx += 1;
        }
    }
}

/// Narrow hardcoded set of PUA glyphs that render as 2 cells in terminals
/// and therefore need `set_cell_wide` (which marks the continuation cell).
/// Matches the specific icons the old `render_tab_bar` used with
/// `set_cell_wide` — other PUA chars (e.g. `SPLIT_DOWN` at `\u{f0d7}`)
/// render as 1 cell in practice and use plain `set_cell`. Extend this list
/// as new wide glyphs are added to tab bars / status bars.
fn is_nerd_wide(c: char) -> bool {
    matches!(
        c,
        '\u{F0932}' // SPLIT_RIGHT
        | '\u{F0143}' // DIFF_PREV
        | '\u{F0140}' // DIFF_NEXT
        | '\u{F0233}' // DIFF_FOLD
    )
}

/// Draw a `quadraui::TabBar` into `area`, consuming a pre-computed
/// `TabBarLayout` per D6. Returns the width (in char cells) available
/// for tab content (`bar_width - reserved_by_right_segments`). The
/// engine uses this return value to decide how many tabs fit and what
/// scroll offset to use on the next frame.
///
/// # D6 contract
///
/// Positions come from `layout.visible_tabs` / `layout.visible_segments`
/// — this function does not decide placement. It only rasterises what
/// the layout says. If you see a layout problem here, fix it in
/// [`quadraui::TabBar::layout`], not in this function.
///
/// # Visual details preserved from the pre-layout version
///
/// * Active tab: `tab_active_fg` + `tab_active_bg`, optional underline
///   accent on the filename portion (chars after the last `": "`).
/// * Dirty tab: close-position shows `●` (theme.foreground) instead of `×`.
/// * Preview tab: italic text; double-italic-underlined when active+accent.
/// * Right segments: each segment drawn in its native cell width, with
///   Nerd Font wide glyphs using `set_cell_wide`. Highlighted segments
///   (`is_active = true`) use `tab_active_fg` instead of `tab_inactive_fg`.
pub(super) fn draw_tab_bar(
    buf: &mut Buffer,
    area: Rect,
    bar: &quadraui::TabBar,
    layout: &quadraui::TabBarLayout,
    theme: &Theme,
) -> usize {
    if area.width == 0 || area.height == 0 {
        return 0;
    }

    let bar_bg = rc(theme.tab_bar_bg);
    let btn_fg = rc(theme.tab_inactive_fg);
    let btn_fg_active = rc(theme.tab_active_fg);
    let foreground = rc(theme.foreground);

    // Fill bar background.
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
    }

    // Tab-content width (engine feedback): bar minus reserved right area.
    let reserved: u16 = bar.right_segments.iter().map(|s| s.width_cells).sum();
    let tab_content_width = if area.width >= reserved {
        (area.width - reserved) as usize
    } else {
        area.width as usize
    };

    // ── Right-aligned segments (from layout) ───────────────────────────
    for vs in &layout.visible_segments {
        let seg = &bar.right_segments[vs.segment_idx];
        let fg = if seg.is_active { btn_fg_active } else { btn_fg };
        let bx = area.x + vs.bounds.x.round() as u16;
        let seg_end = bx + seg.width_cells;
        let mut cx = bx;
        for ch in seg.text.chars() {
            if cx >= seg_end {
                break;
            }
            if ch == ' ' {
                set_cell(buf, cx, area.y, ' ', fg, bar_bg);
                cx += 1;
            } else if is_nerd_wide(ch) {
                if cx + 1 < seg_end + 1 {
                    super::set_cell_wide(buf, cx, area.y, ch, fg, bar_bg);
                    cx += 2;
                } else {
                    // Not enough room for a 2-cell glyph — skip.
                    cx += 1;
                }
            } else {
                set_cell(buf, cx, area.y, ch, fg, bar_bg);
                cx += 1;
            }
        }
    }

    // ── Tabs (from layout) ─────────────────────────────────────────────
    let accent = bar.active_accent.map(qc);
    let active_fg = rc(theme.tab_active_fg);
    let active_bg = rc(theme.tab_active_bg);
    let preview_active_fg = rc(theme.tab_preview_active_fg);
    let inactive_fg = rc(theme.tab_inactive_fg);
    let preview_inactive_fg = rc(theme.tab_preview_inactive_fg);
    let separator = rc(theme.separator);

    for vt in &layout.visible_tabs {
        let tab = &bar.tabs[vt.tab_idx];
        let tab_x = area.x + vt.bounds.x.round() as u16;

        let (fg, bg) = match (tab.is_active, tab.is_preview) {
            (true, true) => (preview_active_fg, active_bg),
            (true, false) => (active_fg, active_bg),
            (false, true) => (preview_inactive_fg, bar_bg),
            (false, false) => (inactive_fg, bar_bg),
        };

        let mut modifier = ratatui::style::Modifier::empty();
        if tab.is_preview {
            modifier |= ratatui::style::Modifier::ITALIC;
        }
        if tab.is_active && accent.is_some() {
            modifier |= ratatui::style::Modifier::UNDERLINED;
        }
        let prefix_mod = if tab.is_preview {
            ratatui::style::Modifier::ITALIC
        } else {
            ratatui::style::Modifier::empty()
        };

        // The layout carries total width; within that, close_bounds
        // covers the trailing close-glyph + separator cells (if the tab
        // has a close button). Label occupies the leading cells up to
        // close_bounds.x.
        let tab_width = vt.bounds.width.round() as u16;
        let label_width = match vt.close_bounds {
            Some(cb) => (cb.x - vt.bounds.x).round() as u16,
            None => tab_width,
        };
        let tab_end = tab_x + tab_width;
        let label_end = tab_x + label_width;

        // Filename portion (after the last ": ") carries the underline accent.
        let prefix_len = tab.label.rfind(": ").map(|p| p + 2).unwrap_or(0);

        let mut x = tab_x;
        for (ci, ch) in tab.label.chars().enumerate() {
            if x >= label_end {
                break;
            }
            let in_filename = ci >= prefix_len;
            let cell_mod = if in_filename { modifier } else { prefix_mod };
            let ul = if in_filename && tab.is_active {
                accent
            } else {
                None
            };
            super::set_cell_styled(buf, x, area.y, ch, fg, bg, cell_mod, ul);
            x += 1;
        }

        // Close indicator: ● for dirty, × otherwise. Only if the tab has
        // a close button (close_bounds is Some).
        if vt.close_bounds.is_some() && x < tab_end {
            let (close_ch, close_fg) = if tab.is_dirty {
                ('●', foreground)
            } else if tab.is_active {
                (super::TAB_CLOSE_CHAR, active_fg)
            } else {
                (super::TAB_CLOSE_CHAR, separator)
            };
            set_cell(buf, x, area.y, close_ch, close_fg, bg);
            x += 1;
        }
        // Trailing separator space (within the tab's bounds, uses bar bg).
        if x < tab_end {
            set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
        }
    }

    tab_content_width
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
    let bg = rc(theme.tab_bar_bg);
    let fg = rc(theme.foreground);
    let sep_fg = rc(theme.line_number_fg);
    let dim_fg = rc(theme.line_number_fg);

    // layout.bounds is the INNER items region; the border is chrome
    // drawn one cell outside on every side.
    let inner_x = layout.bounds.x.round() as u16;
    let inner_y = layout.bounds.y.round() as u16;
    let inner_w = layout.bounds.width.round() as u16;
    let inner_h = layout.bounds.height.round() as u16;
    if inner_w == 0 || inner_h == 0 {
        return;
    }
    let bx = inner_x.saturating_sub(1);
    let by = inner_y.saturating_sub(1);
    let bw = inner_w + 2;
    let bh = inner_h + 2;

    // Draw the border box.
    for dy in 0..bh {
        for dx in 0..bw {
            let cx = bx + dx;
            let cy = by + dy;
            let ch = if dy == 0 {
                if dx == 0 {
                    '┌'
                } else if dx == bw - 1 {
                    '┐'
                } else {
                    '─'
                }
            } else if dy == bh - 1 {
                if dx == 0 {
                    '└'
                } else if dx == bw - 1 {
                    '┘'
                } else {
                    '─'
                }
            } else if dx == 0 || dx == bw - 1 {
                '│'
            } else {
                ' '
            };
            set_cell(buf, cx, cy, ch, fg, bg);
        }
    }

    // Draw items. layout.visible_items includes both actions and separators.
    for vis in &layout.visible_items {
        let item = &menu.items[vis.item_idx];
        let row_y = vis.bounds.y.round() as u16;
        if vis.is_separator {
            // Horizontal dash across the inner width.
            for dx in 0..inner_w {
                set_cell(buf, inner_x + dx, row_y, '─', sep_fg, bg);
            }
            continue;
        }
        let is_selected = vis.item_idx == menu.selected_idx;
        let (item_fg, item_bg) = if is_selected && vis.clickable {
            (bg, fg) // inverted
        } else if !vis.clickable {
            (dim_fg, bg)
        } else {
            (fg, bg)
        };
        // Fill row bg across inner_w.
        for dx in 0..inner_w {
            set_cell(buf, inner_x + dx, row_y, ' ', item_fg, item_bg);
        }
        // Label at col +1 (small inset from border).
        let label = item
            .label
            .spans
            .first()
            .map(|s| s.text.as_str())
            .unwrap_or("");
        for (i, ch) in label.chars().enumerate() {
            let col = inner_x + 1 + i as u16;
            if col >= inner_x + inner_w {
                break;
            }
            set_cell(buf, col, row_y, ch, item_fg, item_bg);
        }
        // Shortcut right-aligned (from item.detail).
        if let Some(ref det) = item.detail {
            let shortcut = det.spans.first().map(|s| s.text.as_str()).unwrap_or("");
            let sc_w = shortcut.chars().count() as u16;
            let sc_start = inner_x + inner_w.saturating_sub(sc_w + 1);
            let sc_fg = if is_selected && vis.clickable {
                item_fg
            } else {
                dim_fg
            };
            for (i, ch) in shortcut.chars().enumerate() {
                let col = sc_start + i as u16;
                if col >= inner_x + inner_w {
                    break;
                }
                set_cell(buf, col, row_y, ch, sc_fg, item_bg);
            }
        }
    }
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
    let bg = rc(theme.fuzzy_bg);
    let fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let border_fg = rc(theme.fuzzy_border);
    let title_fg = rc(theme.fuzzy_title_fg);

    let x = layout.bounds.x.round() as u16;
    let y = layout.bounds.y.round() as u16;
    let w = layout.bounds.width.round() as u16;
    let h = layout.bounds.height.round() as u16;
    if w == 0 || h == 0 {
        return;
    }

    // Clear the box area.
    for row in y..y + h {
        for col in x..x + w {
            set_cell(buf, col, row, ' ', fg, bg);
        }
    }

    // Top border with title overlay.
    for col in 0..w {
        let ch = if col == 0 {
            '╭'
        } else if col == w - 1 {
            '╮'
        } else {
            '─'
        };
        set_cell(buf, x + col, y, ch, border_fg, bg);
    }
    let title_text = format!(" {} ", title_as_plain(&dialog.title));
    for (i, ch) in title_text.chars().enumerate() {
        let col = 2 + i as u16;
        if col + 1 >= w {
            break;
        }
        set_cell(buf, x + col, y, ch, title_fg, bg);
    }

    // Left/right borders for content rows.
    for row in (y + 1)..(y + h - 1) {
        set_cell(buf, x, row, '│', border_fg, bg);
        set_cell(buf, x + w - 1, row, '│', border_fg, bg);
    }
    // Bottom border.
    for col in 0..w {
        let ch = if col == 0 {
            '╰'
        } else if col == w - 1 {
            '╯'
        } else {
            '─'
        };
        set_cell(buf, x + col, y + h - 1, ch, border_fg, bg);
    }

    // Body — split on \n so each line renders on its own row.
    let body_x = layout.body_bounds.x.round() as u16;
    let body_y = layout.body_bounds.y.round() as u16;
    let body_w = layout.body_bounds.width.round() as u16;
    let body_text = title_as_plain(&dialog.body);
    for (i, line) in body_text.split('\n').enumerate() {
        let row = body_y + i as u16;
        if row >= body_y + layout.body_bounds.height.round() as u16 {
            break;
        }
        for (j, ch) in line.chars().enumerate() {
            let col = body_x + j as u16;
            if col >= body_x + body_w {
                break;
            }
            set_cell(buf, col, row, ch, fg, bg);
        }
    }

    // Input field (if present).
    if let (Some(input_bounds), Some(input)) = (layout.input_bounds, &dialog.input) {
        let ix = input_bounds.x.round() as u16;
        let iy = input_bounds.y.round() as u16;
        let iw = input_bounds.width.round() as u16;
        let inp_bg = rc(theme.completion_bg);
        // Fill input background.
        for col in ix..ix + iw {
            set_cell(buf, col, iy, ' ', fg, inp_bg);
        }
        // Draw the input value with a leading space.
        let display = format!(" {}", input.value);
        for (i, ch) in display.chars().enumerate() {
            let col = ix + i as u16;
            if col >= ix + iw {
                break;
            }
            set_cell(buf, col, iy, ch, fg, inp_bg);
        }
    }

    // Buttons — iterate visible_buttons; highlight default one.
    for vis in &layout.visible_buttons {
        let btn = &dialog.buttons[vis.button_idx];
        let bx = vis.bounds.x.round() as u16;
        let by = vis.bounds.y.round() as u16;
        let bw = vis.bounds.width.round() as u16;
        let btn_bg = if btn.is_default { sel_bg } else { bg };
        for col in bx..bx + bw {
            set_cell(buf, col, by, ' ', fg, btn_bg);
        }
        // Center the label inside the button area.
        let label_w = btn.label.chars().count() as u16;
        let start = bx + (bw.saturating_sub(label_w)) / 2;
        for (i, ch) in btn.label.chars().enumerate() {
            let col = start + i as u16;
            if col >= bx + bw {
                break;
            }
            set_cell(buf, col, by, ch, fg, btn_bg);
        }
    }
}

/// Flatten a `StyledText` to a plain `String` for simple single-span
/// rendering paths that don't use colour overrides. For now dialog
/// title + body never carry style overrides — `StyledText::plain`
/// constructs them.
fn title_as_plain(text: &quadraui::StyledText) -> String {
    text.spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
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
    let x = layout.bounds.x.round() as u16;
    let y = layout.bounds.y.round() as u16;
    let w = layout.bounds.width.round() as u16;
    let h = layout.bounds.height.round() as u16;
    if w == 0 || h == 0 {
        return;
    }

    let fg = tooltip.fg.map(qc).unwrap_or_else(|| rc(theme.hover_fg));
    let bg = tooltip.bg.map(qc).unwrap_or_else(|| rc(theme.hover_bg));
    let border = rc(theme.hover_border);

    // Helper: paint a row's side-bar borders + background fill.
    let paint_row_background = |buf: &mut Buffer, row: u16| {
        for col in 0..w {
            let ch = if col == 0 || col == w - 1 { '│' } else { ' ' };
            let cell_fg = if col == 0 || col == w - 1 { border } else { fg };
            set_cell(buf, x + col, row, ch, cell_fg, bg);
        }
    };

    if let Some(ref styled) = tooltip.styled {
        // Single-line styled path. Fill row 0 and render spans left-to-right.
        paint_row_background(buf, y);
        let mut col_off: u16 = 2; // skip border + 1 pad
        for span in &styled.spans {
            let span_fg = span.fg.map(qc).unwrap_or(fg);
            let span_bg = span.bg.map(qc).unwrap_or(bg);
            for ch in span.text.chars() {
                let col = x + col_off;
                if col + 1 >= x + w {
                    return;
                }
                set_cell(buf, col, y, ch, span_fg, span_bg);
                col_off += 1;
            }
        }
        return;
    }

    let lines: Vec<&str> = tooltip.text.lines().collect();
    for (i, text_line) in lines.iter().enumerate().take(h as usize) {
        let row = y + i as u16;
        paint_row_background(buf, row);
        // Render text starting at col 2 (skip border + 1 pad).
        for (j, ch) in text_line.chars().enumerate() {
            let col = x + 2 + j as u16;
            if col + 1 >= x + w {
                break;
            }
            set_cell(buf, col, row, ch, fg, bg);
        }
    }
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
