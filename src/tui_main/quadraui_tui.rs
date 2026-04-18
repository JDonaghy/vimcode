//! TUI backend for `quadraui` primitives.
//!
//! This module provides `draw_*` free functions that render `quadraui`
//! primitives into a ratatui `Buffer`. Over time this file will grow to
//! cover every primitive; for Phase A.1a it supports only `TreeView`.

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
