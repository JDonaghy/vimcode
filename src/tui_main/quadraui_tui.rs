//! TUI backend for `quadraui` primitives.
//!
//! This module provides `draw_*` free functions that render `quadraui`
//! primitives into a ratatui `Buffer`. Over time this file will grow to
//! cover every primitive; currently supports `TreeView` (A.1a) and
//! `Form` (A.3a).

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

    let visible = tree
        .rows
        .iter()
        .skip(tree.scroll_offset)
        .take(area.height as usize);

    for (vis_i, row) in visible.enumerate() {
        let y = area.y + vis_i as u16;
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

    // Field row height in cells. TUI keeps it compact at 1; GTK will use
    // taller rows (item_height-style) when A.3b/c land.
    let row_h: u16 = 1;

    let visible = form
        .fields
        .iter()
        .skip(form.scroll_offset)
        .take(area.height as usize);

    for (vis_i, field) in visible.enumerate() {
        let y = area.y + (vis_i as u16) * row_h;
        if y >= area.y + area.height {
            break;
        }

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
        }
    }
}
