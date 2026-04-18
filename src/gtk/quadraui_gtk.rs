//! GTK backend for `quadraui` primitives.
//!
//! Cairo + Pango equivalent of `src/tui_main/quadraui_tui.rs`. Each
//! `draw_*` function consumes a `quadraui` primitive description and
//! rasterises it onto the provided `cairo::Context`. Over time this file
//! grows to cover every primitive; for Phase A.1b it supports only
//! `TreeView`.

use super::*;

/// Convert vimcode's `Color` (0-255 RGB) into Cairo's (f64, f64, f64)
/// normalised RGB.
fn vc_to_cairo(c: render::Color) -> (f64, f64, f64) {
    c.to_cairo()
}

/// Convert a `quadraui::Color` (0-255 RGBA) into Cairo's normalised RGB.
/// Alpha is dropped — Cairo supports `set_source_rgba` if we ever need it.
fn qc_to_cairo(c: quadraui::Color) -> (f64, f64, f64) {
    (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0)
}

/// Draw a `quadraui::TreeView` into `(x, y, w, h)` on `cr`, using `layout`
/// for text measurement and `theme` for default colours.
///
/// Row heights match the existing GTK SC panel: branches use `line_height`,
/// leaves use `(line_height * 1.4).round()` (kept in sync with the click
/// handler in `src/gtk/mod.rs` that maps mouse positions to flat indices).
///
/// Does not draw a scrollbar. Scrollbars are a later primitive stage.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tree(
    cr: &Context,
    layout: &pango::Layout,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    tree: &quadraui::TreeView,
    theme: &Theme,
    line_height: f64,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let (bg_r, bg_g, bg_b) = vc_to_cairo(theme.tab_bar_bg);
    let (hdr_r, hdr_g, hdr_b) = vc_to_cairo(theme.status_bg);
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = vc_to_cairo(theme.status_fg);
    let (fg_r, fg_g, fg_b) = vc_to_cairo(theme.foreground);
    let (dim_r, dim_g, dim_b) = vc_to_cairo(theme.line_number_fg);
    let (sel_r, sel_g, sel_b) = vc_to_cairo(theme.fuzzy_selected_bg);

    // Fill tree background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);

    let item_height = (line_height * 1.4).round();
    let indent_px = (line_height * 0.9).round();
    let mut y_off = y;
    let y_end = y + h;

    let use_nerd = icons::nerd_fonts_enabled();

    for row in tree.rows.iter().skip(tree.scroll_offset) {
        if y_off >= y_end {
            break;
        }

        let is_branch = row.is_expanded.is_some();
        let is_header = matches!(row.decoration, quadraui::Decoration::Header);
        // Header rows get the tall row-height used by SC section titles;
        // regular branches (like explorer folders) and leaves use `item_height`
        // so dirs don't jump vertically relative to siblings.
        let row_h = if is_header { line_height } else { item_height };
        let _ = is_branch;

        let is_selected =
            tree.has_focus && tree.selected_path.as_ref().is_some_and(|p| p == &row.path);

        // Header rows get a distinct background (SC section styling).
        // Ordinary branches render like leaves so folders don't visually
        // separate from sibling files in a recursive tree.
        let (def_fg, row_bg) = if is_selected {
            ((hdr_fg_r, hdr_fg_g, hdr_fg_b), (sel_r, sel_g, sel_b))
        } else if is_header {
            ((hdr_fg_r, hdr_fg_g, hdr_fg_b), (hdr_r, hdr_g, hdr_b))
        } else if matches!(row.decoration, quadraui::Decoration::Muted) {
            ((dim_r, dim_g, dim_b), (bg_r, bg_g, bg_b))
        } else {
            ((fg_r, fg_g, fg_b), (bg_r, bg_g, bg_b))
        };

        // Fill row background.
        cr.set_source_rgb(row_bg.0, row_bg.1, row_bg.2);
        cr.rectangle(x, y_off, w, row_h);
        cr.fill().ok();

        // Leading horizontal offset: indent + chevron + icon.
        let mut cursor_x = x + 2.0 + (row.indent as f64) * indent_px;

        // Chevron for branches.
        if let Some(expanded) = row.is_expanded {
            if tree.style.show_chevrons {
                let chevron = if expanded {
                    &tree.style.chevron_expanded
                } else {
                    &tree.style.chevron_collapsed
                };
                cr.set_source_rgb(def_fg.0, def_fg.1, def_fg.2);
                layout.set_text(chevron);
                let (cw, ch) = layout.pixel_size();
                cr.move_to(cursor_x, y_off + (row_h - ch as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
                cursor_x += cw as f64 + 4.0;
            }
        } else {
            // Leaves get a small indent past the chevron column for alignment.
            cursor_x += line_height * 0.8;
        }

        // Icon (optional).
        if let Some(ref icon) = row.icon {
            let glyph = if use_nerd {
                icon.glyph.as_str()
            } else {
                icon.fallback.as_str()
            };
            cr.set_source_rgb(def_fg.0, def_fg.1, def_fg.2);
            layout.set_text(glyph);
            let (iw, ih) = layout.pixel_size();
            cr.move_to(cursor_x, y_off + (row_h - ih as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            cursor_x += iw as f64 + 6.0;
        }

        // Reserve space for the badge (right-aligned). We measure the badge
        // first so we can truncate text if they collide.
        let badge_info = row.badge.as_ref().map(|badge| {
            layout.set_text(&badge.text);
            let (bw, _) = layout.pixel_size();
            let fg = badge.fg.map(qc_to_cairo).unwrap_or((dim_r, dim_g, dim_b));
            let bg = badge.bg.map(qc_to_cairo).unwrap_or(row_bg);
            (badge.text.clone(), bw as f64, fg, bg)
        });
        let badge_reserve = badge_info
            .as_ref()
            .map(|(_, bw, ..)| *bw + 8.0)
            .unwrap_or(0.0);
        let text_right_limit = x + w - badge_reserve - 4.0;

        // Text spans — draw each with its own foreground.
        for span in &row.text.spans {
            if cursor_x >= text_right_limit {
                break;
            }
            let span_fg = if let Some(c) = span.fg {
                qc_to_cairo(c)
            } else if matches!(row.decoration, quadraui::Decoration::Muted) {
                (dim_r, dim_g, dim_b)
            } else {
                def_fg
            };
            // Paint span background if explicit.
            if let Some(sbg) = span.bg {
                let (sbr, sbg_, sbb) = qc_to_cairo(sbg);
                layout.set_text(&span.text);
                let (sw, _) = layout.pixel_size();
                cr.set_source_rgb(sbr, sbg_, sbb);
                cr.rectangle(
                    cursor_x,
                    y_off,
                    (sw as f64).min(text_right_limit - cursor_x),
                    row_h,
                );
                cr.fill().ok();
            }
            cr.set_source_rgb(span_fg.0, span_fg.1, span_fg.2);
            layout.set_text(&span.text);
            let (sw, sh) = layout.pixel_size();
            cr.move_to(cursor_x, y_off + (row_h - sh as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            cursor_x += sw as f64;
        }

        // Badge (right-aligned within area).
        if let Some((btext, bw, bfg, bbg)) = badge_info {
            let bx = x + w - bw - 4.0;
            if bx > cursor_x {
                // Paint badge background if distinct from row background.
                if bbg != row_bg {
                    cr.set_source_rgb(bbg.0, bbg.1, bbg.2);
                    cr.rectangle(bx - 2.0, y_off, bw + 4.0, row_h);
                    cr.fill().ok();
                }
                cr.set_source_rgb(bfg.0, bfg.1, bfg.2);
                layout.set_text(&btext);
                let (_, bh) = layout.pixel_size();
                cr.move_to(bx, y_off + (row_h - bh as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
            }
        }

        y_off += row_h;
    }

    // Reset Pango attributes so subsequent draw calls don't inherit state.
    layout.set_attributes(None);
}
