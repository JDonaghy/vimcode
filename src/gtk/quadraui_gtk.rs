//! GTK backend for `quadraui` primitives.
//!
//! Cairo + Pango equivalent of `src/tui_main/quadraui_tui.rs`. Each
//! `draw_*` function consumes a `quadraui` primitive description and
//! rasterises it onto the provided `cairo::Context`. Currently supports
//! `TreeView` (A.1b), `Form` (A.3c), and `ListView` (A.5b).

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

/// Draw a `quadraui::Form` into `(x, y, w, h)` on `cr`, using `layout`
/// for text measurement and `theme` for default colours.
///
/// Layout: one row per field. Row height is `(line_height * 1.4).round()`
/// for consistency with the GTK SC / explorer treeviews. Label on the
/// left, input on the right. Headers (`FieldKind::Label`) span the row
/// in status-bar styling.
// Dead-code allow: the GTK settings panel still uses native widgets
// (Switch / SpinButton / Entry / DropDown). Phase A.3c ships the GTK
// primitive renderer; a follow-up stage will replace the native-widget
// settings panel with a `DrawingArea` that calls this function.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_form(
    cr: &Context,
    layout: &pango::Layout,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    form: &quadraui::Form,
    theme: &Theme,
    line_height: f64,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = theme.status_fg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.foreground.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();
    let (accent_r, accent_g, accent_b) = theme.cursor.to_cairo();

    // Fill form background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();
    layout.set_attributes(None);

    let row_h = (line_height * 1.4).round();
    let mut y_off = y;
    let y_end = y + h;

    for field in form.fields.iter().skip(form.scroll_offset) {
        if y_off + row_h > y_end {
            break;
        }

        let is_focused = form.has_focus
            && form
                .focused_field
                .as_ref()
                .is_some_and(|id| id == &field.id);
        let is_header = matches!(field.kind, quadraui::FieldKind::Label);

        let (default_fg, row_bg) = if is_focused {
            ((fg_r, fg_g, fg_b), (sel_r, sel_g, sel_b))
        } else if is_header {
            ((hdr_fg_r, hdr_fg_g, hdr_fg_b), (hdr_r, hdr_g, hdr_b))
        } else {
            ((fg_r, fg_g, fg_b), (bg_r, bg_g, bg_b))
        };

        cr.set_source_rgb(row_bg.0, row_bg.1, row_bg.2);
        cr.rectangle(x, y_off, w, row_h);
        cr.fill().ok();

        let field_fg = if field.disabled {
            (dim_r, dim_g, dim_b)
        } else {
            default_fg
        };

        // Draw the label on the left.
        let label_text: String = field.label.spans.iter().map(|s| s.text.as_str()).collect();
        cr.set_source_rgb(field_fg.0, field_fg.1, field_fg.2);
        layout.set_text(&label_text);
        let (label_w, label_h) = layout.pixel_size();
        let label_x = x + 6.0;
        cr.move_to(label_x, y_off + (row_h - label_h as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        let label_right = label_x + label_w as f64;

        // Input on the right.
        let input_right = x + w - 8.0;
        match &field.kind {
            quadraui::FieldKind::Label => {
                // No separate input; label spans the row.
            }
            quadraui::FieldKind::Toggle { value } => {
                let glyph = if *value { "[x]" } else { "[ ]" };
                let fg_color = if *value && !field.disabled {
                    (accent_r, accent_g, accent_b)
                } else {
                    field_fg
                };
                cr.set_source_rgb(fg_color.0, fg_color.1, fg_color.2);
                layout.set_text(glyph);
                let (iw, ih) = layout.pixel_size();
                let ix = input_right - iw as f64;
                if ix > label_right + 8.0 {
                    cr.move_to(ix, y_off + (row_h - ih as f64) / 2.0);
                    pangocairo::show_layout(cr, layout);
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
                let input_fg = if value.is_empty() {
                    (dim_r, dim_g, dim_b)
                } else {
                    field_fg
                };

                // Measure `shown` and bracket it on the right.
                layout.set_text(shown);
                let (shown_w, shown_h) = layout.pixel_size();

                // "[value]" — capped at 60% of w.
                let max_width = (w * 0.6).max(80.0);
                let draw_w = (shown_w as f64).min(max_width);
                let ix = input_right - draw_w - 14.0; // 14 = 2 brackets + padding
                if ix > label_right + 8.0 {
                    // Left bracket.
                    cr.set_source_rgb(dim_r, dim_g, dim_b);
                    layout.set_text("[");
                    cr.move_to(ix, y_off + (row_h - shown_h as f64) / 2.0);
                    pangocairo::show_layout(cr, layout);

                    // Selection background.
                    if let (Some(cur), Some(anchor)) = (cursor, selection_anchor) {
                        if *cur != *anchor && !value.is_empty() {
                            let (lo, hi) = (*cur.min(anchor), *cur.max(anchor));
                            let before: String = shown.chars().take_while(|_| false).collect();
                            let _ = before; // silence unused-warning guard
                                            // Compute pixel offsets by re-measuring prefixes.
                            let prefix = &shown[..lo.min(shown.len())];
                            let sel_text = &shown[lo.min(shown.len())..hi.min(shown.len())];
                            layout.set_text(prefix);
                            let (prefix_w, _) = layout.pixel_size();
                            layout.set_text(sel_text);
                            let (sel_w, _) = layout.pixel_size();
                            cr.set_source_rgb(sel_r, sel_g, sel_b);
                            cr.rectangle(
                                ix + 8.0 + prefix_w as f64,
                                y_off + 2.0,
                                sel_w as f64,
                                row_h - 4.0,
                            );
                            cr.fill().ok();
                        }
                    }

                    // Text.
                    cr.set_source_rgb(input_fg.0, input_fg.1, input_fg.2);
                    layout.set_text(shown);
                    cr.move_to(ix + 8.0, y_off + (row_h - shown_h as f64) / 2.0);
                    pangocairo::show_layout(cr, layout);

                    // Right bracket.
                    cr.set_source_rgb(dim_r, dim_g, dim_b);
                    layout.set_text("]");
                    cr.move_to(
                        ix + 8.0 + draw_w + 2.0,
                        y_off + (row_h - shown_h as f64) / 2.0,
                    );
                    pangocairo::show_layout(cr, layout);

                    // Cursor (thin vertical bar) — only when value non-empty.
                    if let Some(cur) = cursor {
                        if !value.is_empty() {
                            let prefix = &shown[..(*cur).min(shown.len())];
                            layout.set_text(prefix);
                            let (prefix_w, _) = layout.pixel_size();
                            let cx = ix + 8.0 + prefix_w as f64;
                            cr.set_source_rgb(accent_r, accent_g, accent_b);
                            cr.rectangle(cx, y_off + 3.0, 1.5, row_h - 6.0);
                            cr.fill().ok();
                        }
                    }
                }
            }
            quadraui::FieldKind::Button => {
                // The field's label IS the caption. Redraw wrapped in
                // angle brackets on the right side; blank out the left-
                // side label we already drew.
                cr.set_source_rgb(row_bg.0, row_bg.1, row_bg.2);
                cr.rectangle(x, y_off, label_right - x + 1.0, row_h);
                cr.fill().ok();

                let cap_text: String = field.label.spans.iter().map(|s| s.text.as_str()).collect();
                layout.set_text(&cap_text);
                let (cap_w, cap_h) = layout.pixel_size();
                let total_w = cap_w as f64 + 24.0; // "< caption >"
                let ix = input_right - total_w;
                if ix > x + 8.0 {
                    let brk_color = if is_focused {
                        (accent_r, accent_g, accent_b)
                    } else {
                        (dim_r, dim_g, dim_b)
                    };
                    cr.set_source_rgb(brk_color.0, brk_color.1, brk_color.2);
                    layout.set_text("<");
                    cr.move_to(ix, y_off + (row_h - cap_h as f64) / 2.0);
                    pangocairo::show_layout(cr, layout);

                    cr.set_source_rgb(field_fg.0, field_fg.1, field_fg.2);
                    layout.set_text(&cap_text);
                    cr.move_to(ix + 12.0, y_off + (row_h - cap_h as f64) / 2.0);
                    pangocairo::show_layout(cr, layout);

                    cr.set_source_rgb(brk_color.0, brk_color.1, brk_color.2);
                    layout.set_text(">");
                    cr.move_to(
                        ix + 12.0 + cap_w as f64 + 4.0,
                        y_off + (row_h - cap_h as f64) / 2.0,
                    );
                    pangocairo::show_layout(cr, layout);
                }
            }
            quadraui::FieldKind::ReadOnly { value } => {
                let value_text: String = value.spans.iter().map(|s| s.text.as_str()).collect();
                layout.set_text(&value_text);
                let (vw, vh) = layout.pixel_size();
                let ix = input_right - vw as f64;
                if ix > label_right + 8.0 {
                    cr.set_source_rgb(dim_r, dim_g, dim_b);
                    cr.move_to(ix, y_off + (row_h - vh as f64) / 2.0);
                    pangocairo::show_layout(cr, layout);
                }
            }
        }

        y_off += row_h;
    }

    layout.set_attributes(None);
}

/// Draw a `quadraui::ListView` into `(x, y, w, h)` on `cr`, using `layout`
/// for text measurement and `theme` for default colours.
///
/// Layout: optional title header (status-bar styling) at the top, then
/// one `line_height`-tall row per item. Selected row gets a `▶ ` prefix
/// and `fuzzy_selected_bg` background. Optional icon sits left of the
/// text; optional detail is right-aligned and dimmed.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_list(
    cr: &Context,
    layout: &pango::Layout,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    list: &quadraui::ListView,
    theme: &Theme,
    line_height: f64,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let (bg_r, bg_g, bg_b) = vc_to_cairo(theme.background);
    let (hdr_r, hdr_g, hdr_b) = vc_to_cairo(theme.status_bg);
    let (hdr_fg_r, hdr_fg_g, hdr_fg_b) = vc_to_cairo(theme.status_fg);
    let (fg_r, fg_g, fg_b) = vc_to_cairo(theme.fuzzy_fg);
    let (dim_r, dim_g, dim_b) = vc_to_cairo(theme.line_number_fg);
    let (sel_r, sel_g, sel_b) = vc_to_cairo(theme.fuzzy_selected_bg);
    let (err_r, err_g, err_b) = vc_to_cairo(theme.diagnostic_error);
    let (warn_r, warn_g, warn_b) = vc_to_cairo(theme.diagnostic_warning);

    // Fill list background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);

    let mut y_off = y;
    let y_end = y + h;
    let use_nerd = icons::nerd_fonts_enabled();

    // Title header (optional). Rendered as a single full-width status-bar row.
    if let Some(ref title) = list.title {
        if y_off + line_height > y_end {
            return;
        }
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, y_off, w, line_height);
        cr.fill().ok();

        cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
        let title_text: String = title.spans.iter().map(|s| s.text.as_str()).collect();
        layout.set_text(&title_text);
        let (_, th) = layout.pixel_size();
        cr.move_to(x + 2.0, y_off + (line_height - th as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        y_off += line_height;
    }

    for (vis_i, item) in list.items.iter().enumerate().skip(list.scroll_offset) {
        if y_off + line_height > y_end {
            break;
        }

        let is_selected = vis_i == list.selected_idx && list.has_focus;

        // Decoration → foreground colour.
        let decoration_fg = match item.decoration {
            quadraui::Decoration::Error => (err_r, err_g, err_b),
            quadraui::Decoration::Warning => (warn_r, warn_g, warn_b),
            quadraui::Decoration::Muted => (dim_r, dim_g, dim_b),
            quadraui::Decoration::Header => (hdr_fg_r, hdr_fg_g, hdr_fg_b),
            _ => (fg_r, fg_g, fg_b),
        };
        let row_bg = if is_selected {
            (sel_r, sel_g, sel_b)
        } else if matches!(item.decoration, quadraui::Decoration::Header) {
            (hdr_r, hdr_g, hdr_b)
        } else {
            (bg_r, bg_g, bg_b)
        };

        // Fill row background.
        cr.set_source_rgb(row_bg.0, row_bg.1, row_bg.2);
        cr.rectangle(x, y_off, w, line_height);
        cr.fill().ok();

        let mut cursor_x = x + 2.0;

        // Selection indicator (▶ on selection, two spaces otherwise — keeps
        // non-selected row text aligned with selected row text).
        let prefix = if is_selected { "▶ " } else { "  " };
        cr.set_source_rgb(decoration_fg.0, decoration_fg.1, decoration_fg.2);
        layout.set_text(prefix);
        let (pw, ph) = layout.pixel_size();
        cr.move_to(cursor_x, y_off + (line_height - ph as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
        cursor_x += pw as f64;

        // Icon (optional).
        if let Some(ref icon) = item.icon {
            let glyph = if use_nerd {
                icon.glyph.as_str()
            } else {
                icon.fallback.as_str()
            };
            cr.set_source_rgb(decoration_fg.0, decoration_fg.1, decoration_fg.2);
            layout.set_text(glyph);
            let (iw, ih) = layout.pixel_size();
            cr.move_to(cursor_x, y_off + (line_height - ih as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            cursor_x += iw as f64 + 6.0;
        }

        // Reserve space for the detail (right-aligned, dimmed).
        let detail_info = item.detail.as_ref().map(|detail| {
            let detail_text: String = detail.spans.iter().map(|s| s.text.as_str()).collect();
            layout.set_text(&detail_text);
            let (dw, _) = layout.pixel_size();
            (detail_text, dw as f64)
        });
        let detail_reserve = detail_info.as_ref().map(|(_, dw)| *dw + 8.0).unwrap_or(0.0);
        let text_right_limit = x + w - detail_reserve - 4.0;

        // Text spans.
        for span in &item.text.spans {
            if cursor_x >= text_right_limit {
                break;
            }
            let span_fg = if let Some(c) = span.fg {
                qc_to_cairo(c)
            } else {
                decoration_fg
            };
            if let Some(sbg) = span.bg {
                let (sbr, sbg_, sbb) = qc_to_cairo(sbg);
                layout.set_text(&span.text);
                let (sw, _) = layout.pixel_size();
                cr.set_source_rgb(sbr, sbg_, sbb);
                cr.rectangle(
                    cursor_x,
                    y_off,
                    (sw as f64).min(text_right_limit - cursor_x),
                    line_height,
                );
                cr.fill().ok();
            }
            cr.set_source_rgb(span_fg.0, span_fg.1, span_fg.2);
            layout.set_text(&span.text);
            let (sw, sh) = layout.pixel_size();
            cr.move_to(cursor_x, y_off + (line_height - sh as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            cursor_x += sw as f64;
        }

        // Detail (right-aligned, dimmed).
        if let Some((detail_text, dw)) = detail_info {
            let dx = x + w - dw - 4.0;
            if dx > cursor_x {
                cr.set_source_rgb(dim_r, dim_g, dim_b);
                layout.set_text(&detail_text);
                let (_, dh) = layout.pixel_size();
                cr.move_to(dx, y_off + (line_height - dh as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
            }
        }

        y_off += line_height;
    }

    layout.set_attributes(None);
}
