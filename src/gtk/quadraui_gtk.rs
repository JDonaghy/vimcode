//! GTK backend for `quadraui` primitives.
//!
//! Cairo + Pango equivalent of `src/tui_main/quadraui_tui.rs`. Each
//! `draw_*` function consumes a `quadraui` primitive description and
//! rasterises it onto the provided `cairo::Context`. Currently supports
//! `TreeView` (A.1b), `Form` (A.3c), `ListView` (A.5b), and `Palette`
//! (A.4b).

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

/// Draw a `quadraui::Palette` modal into `(x, y, w, h)` on `cr`.
///
/// Layout mirrors the TUI `draw_palette` reference:
///   row 0         — title bar with `Title  N/M` count (when `total_count > 0`)
///   row 1         — query row `> <text>` with a cursor block at `query_cursor`
///   horizontal    — separator line beneath the query
///   rows 2..N     — filtered items, selected row highlighted, fuzzy-match
///                   characters coloured, optional right-aligned detail text
///   scrollbar     — on the right when items overflow
///
/// Caller is responsible for sizing / centring the popup within the
/// editor area. Draws a solid rectangle border (Cairo stroke) instead
/// of TUI-style box-drawing glyphs.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_palette(
    cr: &Context,
    layout: &pango::Layout,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    palette: &quadraui::Palette,
    theme: &Theme,
    line_height: f64,
) {
    if w < 20.0 || h < line_height * 4.0 {
        return;
    }

    // Hard clip the whole palette render to the popup bounds so nothing
    // — not the selection background, not the scrollbar thumb, not the
    // match-highlight attributes — can escape the popup frame. Closed
    // with the matching `cr.restore()` at the end of this function.
    cr.save().ok();
    cr.rectangle(x, y, w, h);
    cr.clip();

    let (bg_r, bg_g, bg_b) = vc_to_cairo(theme.fuzzy_bg);
    let (fg_r, fg_g, fg_b) = vc_to_cairo(theme.fuzzy_fg);
    let (query_r, query_g, query_b) = vc_to_cairo(theme.fuzzy_query_fg);
    let (border_r, border_g, border_b) = vc_to_cairo(theme.fuzzy_border);
    let (title_r, title_g, title_b) = vc_to_cairo(theme.fuzzy_title_fg);
    let (match_r, match_g, match_b) = vc_to_cairo(theme.fuzzy_match_fg);
    let (sel_r, sel_g, sel_b) = vc_to_cairo(theme.fuzzy_selected_bg);
    let (dim_r, dim_g, dim_b) = vc_to_cairo(theme.line_number_fg);

    // Background fill + border stroke.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    cr.set_source_rgb(border_r, border_g, border_b);
    cr.set_line_width(1.0);
    cr.rectangle(x, y, w, h);
    cr.stroke().ok();

    layout.set_attributes(None);

    // ── Title row ─────────────────────────────────────────────────────
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
    cr.set_source_rgb(title_r, title_g, title_b);
    layout.set_text(&title_text);
    let (_, th) = layout.pixel_size();
    cr.move_to(x + 8.0, y + (line_height - th as f64) / 2.0);
    pangocairo::show_layout(cr, layout);

    // ── Query row ─────────────────────────────────────────────────────
    let query_y = y + line_height;
    let prompt = "> ";
    cr.set_source_rgb(query_r, query_g, query_b);
    layout.set_text(prompt);
    let (prompt_w, qh) = layout.pixel_size();
    cr.move_to(x + 8.0, query_y + (line_height - qh as f64) / 2.0);
    pangocairo::show_layout(cr, layout);

    let query_text_x = x + 8.0 + prompt_w as f64;
    layout.set_text(&palette.query);
    let (query_w, _) = layout.pixel_size();
    cr.move_to(query_text_x, query_y + (line_height - qh as f64) / 2.0);
    pangocairo::show_layout(cr, layout);

    // Cursor block at the byte offset `query_cursor`.
    let cursor_prefix: &str = if palette.query_cursor >= palette.query.len() {
        palette.query.as_str()
    } else {
        &palette.query[..palette.query_cursor]
    };
    layout.set_text(cursor_prefix);
    let (cursor_prefix_w, _) = layout.pixel_size();
    let cursor_x = query_text_x + cursor_prefix_w as f64;
    let cursor_char: String = palette
        .query
        .get(palette.query_cursor..)
        .and_then(|s| s.chars().next())
        .map(|c| c.to_string())
        .unwrap_or_else(|| " ".to_string());
    layout.set_text(&cursor_char);
    let (cursor_w, _) = layout.pixel_size();
    let cursor_w = (cursor_w as f64).max(line_height * 0.45);
    // Fill cursor block with query_fg and re-draw the covered char in bg.
    cr.set_source_rgb(query_r, query_g, query_b);
    cr.rectangle(cursor_x, query_y, cursor_w, line_height);
    cr.fill().ok();
    if !cursor_char.trim().is_empty() {
        cr.set_source_rgb(bg_r, bg_g, bg_b);
        cr.move_to(cursor_x, query_y + (line_height - qh as f64) / 2.0);
        layout.set_text(&cursor_char);
        pangocairo::show_layout(cr, layout);
    }
    let _ = query_w; // currently unused; keep measurement for future right-edge clipping

    // ── Separator row ─────────────────────────────────────────────────
    let sep_y = y + 2.0 * line_height;
    cr.set_source_rgb(border_r, border_g, border_b);
    cr.set_line_width(1.0);
    cr.move_to(x, sep_y);
    cr.line_to(x + w, sep_y);
    cr.stroke().ok();

    // ── Result rows ───────────────────────────────────────────────────
    // Leave a small inset above the popup's bottom border so neither the
    // item-row backgrounds nor the scrollbar thumb bleed into/through the
    // border stroke line.
    const BOTTOM_INSET: f64 = 4.0;
    let rows_y = sep_y + 1.0;
    let rows_h = ((y + h) - rows_y - BOTTOM_INSET).max(0.0);
    let visible_rows = (rows_h / line_height) as usize;
    // Snap the usable row area to a whole number of rows so the last item
    // always occupies a full line_height cell.
    let rows_h = visible_rows as f64 * line_height;
    let total = palette.items.len();
    let has_scrollbar = total > visible_rows;
    const SB_W: f64 = 6.0;
    let content_w = if has_scrollbar { w - SB_W } else { w };

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

    cr.save().ok();
    cr.rectangle(x, rows_y, content_w, rows_h);
    cr.clip();

    for (vis_i, item) in palette
        .items
        .iter()
        .enumerate()
        .skip(effective_offset)
        .take(visible_rows)
    {
        let row_i = vis_i - effective_offset;
        let row_y = rows_y + row_i as f64 * line_height;
        let is_selected = vis_i == palette.selected_idx && palette.has_focus;

        if is_selected {
            cr.set_source_rgb(sel_r, sel_g, sel_b);
            cr.rectangle(x, row_y, content_w, line_height);
            cr.fill().ok();
        }

        // Concatenate all text spans for rendering + fuzzy-match mapping.
        let full_text: String = item.text.spans.iter().map(|s| s.text.as_str()).collect();

        // Build a Pango AttrList: default fg over full range, then match_fg
        // spans at each `match_positions` byte offset (1 char each).
        let attr_list = pango::AttrList::new();
        let mut attr_fg = pango::AttrColor::new_foreground(
            (fg_r * 65535.0) as u16,
            (fg_g * 65535.0) as u16,
            (fg_b * 65535.0) as u16,
        );
        attr_fg.set_start_index(0);
        attr_fg.set_end_index(full_text.len() as u32);
        attr_list.insert(attr_fg);

        if !item.match_positions.is_empty() {
            for &pos in &item.match_positions {
                if pos >= full_text.len() {
                    continue;
                }
                let char_len = full_text[pos..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
                let mut attr_match = pango::AttrColor::new_foreground(
                    (match_r * 65535.0) as u16,
                    (match_g * 65535.0) as u16,
                    (match_b * 65535.0) as u16,
                );
                attr_match.set_start_index(pos as u32);
                attr_match.set_end_index((pos + char_len) as u32);
                attr_list.insert(attr_match);
            }
        }

        // Horizontal cursor position for text after icon.
        let mut cursor = x + 8.0;

        // Icon (optional).
        if let Some(ref icon) = item.icon {
            let glyph = if icons::nerd_fonts_enabled() {
                icon.glyph.as_str()
            } else {
                icon.fallback.as_str()
            };
            layout.set_attributes(None);
            cr.set_source_rgb(fg_r, fg_g, fg_b);
            layout.set_text(glyph);
            let (iw, ih) = layout.pixel_size();
            cr.move_to(cursor, row_y + (line_height - ih as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            cursor += iw as f64 + 6.0;
        }

        // Reserve space for right-aligned detail so text doesn't overlap.
        let detail_info = item.detail.as_ref().map(|detail| {
            let detail_text: String = detail.spans.iter().map(|s| s.text.as_str()).collect();
            layout.set_attributes(None);
            layout.set_text(&detail_text);
            let (dw, _) = layout.pixel_size();
            (detail_text, dw as f64)
        });
        let detail_reserve = detail_info
            .as_ref()
            .map(|(_, dw)| *dw + 12.0)
            .unwrap_or(0.0);
        let _ = detail_reserve; // text draws within content_w; detail paints last

        // Primary text.
        layout.set_text(&full_text);
        layout.set_attributes(Some(&attr_list));
        let (_, lh) = layout.pixel_size();
        cr.move_to(cursor, row_y + (line_height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);

        // Detail (right-aligned, dimmed).
        if let Some((detail_text, dw)) = detail_info {
            let dx = x + content_w - dw - 8.0;
            cr.set_source_rgb(dim_r, dim_g, dim_b);
            layout.set_attributes(None);
            layout.set_text(&detail_text);
            let (_, dh) = layout.pixel_size();
            cr.move_to(dx, row_y + (line_height - dh as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
        }
    }

    cr.restore().ok();
    layout.set_attributes(None);

    // ── Scrollbar ─────────────────────────────────────────────────────
    if has_scrollbar && visible_rows > 0 {
        let sb_x = x + w - SB_W;
        let sb_track_y = rows_y;
        let sb_track_h = rows_h;

        // Track (dim bg tinted toward black).
        cr.set_source_rgb(bg_r * 0.7, bg_g * 0.7, bg_b * 0.7);
        cr.rectangle(sb_x, sb_track_y, SB_W, sb_track_h);
        cr.fill().ok();

        let thumb_ratio = visible_rows as f64 / total as f64;
        let thumb_h = (sb_track_h * thumb_ratio).max(8.0);
        let max_scroll = total.saturating_sub(visible_rows) as f64;
        let scroll_frac = if max_scroll > 0.0 {
            effective_offset as f64 / max_scroll
        } else {
            0.0
        };
        let thumb_y = sb_track_y + scroll_frac * (sb_track_h - thumb_h);

        cr.set_source_rgb(border_r, border_g, border_b);
        cr.rectangle(sb_x + 1.0, thumb_y, SB_W - 2.0, thumb_h);
        cr.fill().ok();
    }

    // Close the outer popup-bounds clip opened at function start.
    cr.restore().ok();
}

/// Draw a `quadraui::StatusBar` as a single row `line_height` tall.
///
/// Layout matches the TUI backend: left segments accumulate from the left
/// edge, right segments are right-aligned inside `width`. When the two
/// halves would collide, the left half wins up to where it meets the
/// right (Cairo just clips at the segment boundary).
///
/// Returns hit regions in local coordinates (relative to `x`) — caller
/// pushes them into the per-window segment map for click resolution.
/// Bold segments use Pango's bold weight attribute.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_status_bar(
    cr: &Context,
    layout: &pango::Layout,
    x: f64,
    y: f64,
    width: f64,
    line_height: f64,
    bar: &quadraui::StatusBar,
    theme: &Theme,
) -> Vec<quadraui::StatusBarHitRegion> {
    // Reset layout state the same way the legacy renderer did.
    layout.set_attributes(None);
    layout.set_width(-1);
    layout.set_ellipsize(pango::EllipsizeMode::None);

    // #157: clip to the bar's rect so right-aligned segments that overflow
    // the available width are truncated at the right edge instead of
    // painting past it into the next window's tab bar / status row.
    cr.save().ok();
    cr.rectangle(x, y, width, line_height);
    cr.clip();

    // Background fill: first segment's bg, else theme bg.
    let fill = bar
        .left_segments
        .first()
        .or(bar.right_segments.first())
        .map(|s| qc_to_cairo(s.bg))
        .unwrap_or_else(|| vc_to_cairo(theme.background));
    cr.set_source_rgb(fill.0, fill.1, fill.2);
    cr.rectangle(x, y, width, line_height);
    cr.fill().ok();

    let mut regions: Vec<quadraui::StatusBarHitRegion> = Vec::new();

    // Helper: apply bold attribute to the shared layout if the segment wants it.
    // Returns true when a bold attribute was installed, so the caller can clear afterwards.
    let apply_bold = |layout: &pango::Layout, bold: bool| {
        if bold {
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            layout.set_attributes(Some(&attrs));
        } else {
            layout.set_attributes(None);
        }
    };

    // ── Left segments — accumulate from x ────────────────────────────────
    let mut cx = x;
    for seg in &bar.left_segments {
        layout.set_text(&seg.text);
        apply_bold(layout, seg.bold);
        let (seg_w_px, _) = layout.pixel_size();
        let seg_w = seg_w_px as f64;

        if let Some(ref id) = seg.action_id {
            // Hit region widths use px (StatusBarHitRegion stores u16),
            // but for Cairo we need f64. We keep the primitive's u16 shape
            // by saturating; the real-world bar is far under u16::MAX px.
            regions.push(quadraui::StatusBarHitRegion {
                col: ((cx - x).round() as i64).clamp(0, u16::MAX as i64) as u16,
                width: (seg_w.round() as i64).clamp(0, u16::MAX as i64) as u16,
                id: id.clone(),
            });
        }

        let (sr, sg, sb) = qc_to_cairo(seg.bg);
        cr.set_source_rgb(sr, sg, sb);
        cr.rectangle(cx, y, seg_w, line_height);
        cr.fill().ok();

        let (fr, fg, fb) = qc_to_cairo(seg.fg);
        cr.set_source_rgb(fr, fg, fb);
        cr.move_to(cx, y);
        pangocairo::show_layout(cr, layout);

        cx += seg_w;
        if cx >= x + width {
            break;
        }
    }

    // ── Right segments — right-aligned ──────────────────────────────────
    //
    // #159: drop low-priority right segments from the front until they fit
    // with a ~16 px gap after the rightmost left segment. `right_segments`
    // is ordered least-important first, most-important (cursor position)
    // last — see `render::build_window_status_line`. The highest-priority
    // segment is always kept, even if it alone overflows; #157's clip
    // truncates visually in that edge case.
    const MIN_GAP_PX: f64 = 16.0;
    let right_widths_px: Vec<f64> = bar
        .right_segments
        .iter()
        .map(|seg| {
            layout.set_text(&seg.text);
            apply_bold(layout, seg.bold);
            let (w_px, _) = layout.pixel_size();
            w_px as f64
        })
        .collect();
    let right_total_w: f64 = right_widths_px.iter().sum();
    let left_end_px = cx - x;
    let max_right_px = (width - left_end_px - MIN_GAP_PX).max(0.0);

    let mut start_idx = 0;
    if !bar.right_segments.is_empty() && right_total_w > max_right_px {
        let last = bar.right_segments.len() - 1;
        let mut remaining = right_total_w;
        for (i, w) in right_widths_px.iter().enumerate() {
            if remaining <= max_right_px {
                start_idx = i;
                break;
            }
            if i == last {
                start_idx = i;
                break;
            }
            remaining -= w;
        }
    }

    let visible_total_w: f64 = right_widths_px[start_idx..].iter().sum();
    let mut rx = (x + width - visible_total_w).max(cx);
    for (offset, seg) in bar.right_segments[start_idx..].iter().enumerate() {
        layout.set_text(&seg.text);
        apply_bold(layout, seg.bold);
        let seg_w = right_widths_px[start_idx + offset];

        if let Some(ref id) = seg.action_id {
            regions.push(quadraui::StatusBarHitRegion {
                col: ((rx - x).round() as i64).clamp(0, u16::MAX as i64) as u16,
                width: (seg_w.round() as i64).clamp(0, u16::MAX as i64) as u16,
                id: id.clone(),
            });
        }

        let (sr, sg, sb) = qc_to_cairo(seg.bg);
        cr.set_source_rgb(sr, sg, sb);
        cr.rectangle(rx, y, seg_w, line_height);
        cr.fill().ok();

        let (fr, fg, fb) = qc_to_cairo(seg.fg);
        cr.set_source_rgb(fr, fg, fb);
        cr.move_to(rx, y);
        pangocairo::show_layout(cr, layout);

        rx += seg_w;
    }

    layout.set_attributes(None);
    cr.restore().ok();

    regions
}

// ─── Tab bar (A.6d) ──────────────────────────────────────────────────────────

/// Per-frame hit-region output of `draw_tab_bar`.
///
/// All positions are absolute pixel coordinates inside the target surface;
/// the caller typically stores them keyed by `GroupId` and consults them
/// when resolving mouse events.
#[derive(Debug, Default, Clone)]
pub(super) struct TabBarHitInfo {
    /// `[(start_x, end_x)]` per tab index. Tabs before `scroll_offset` get
    /// zero-width `(0.0, 0.0)` sentinels so indices match the tab list.
    pub slot_positions: Vec<(f64, f64)>,
    /// `(prev_start, prev_end, next_start, next_end, fold_start, fold_end)`
    /// — the three diff toolbar buttons' x ranges, if rendered.
    pub diff_btns: Option<(f64, f64, f64, f64, f64, f64)>,
    /// `(total_split_width, split_right_width)` for click dispatch.
    pub split_btns: Option<(f64, f64)>,
    /// `(start_x, end_x)` of the action menu button.
    pub action_btn: Option<(f64, f64)>,
    /// Tab-bar content width in **character columns** (not pixels).
    /// Used by the engine to compute how many tabs fit at a given font.
    pub available_cols: usize,
}

/// Draw a `quadraui::TabBar` into `(0, y_offset, width, tab_row_height)`
/// on `cr`, using a sans-serif UI font for labels (not the editor's
/// monospace font).
///
/// `hovered_close_tab` is a per-frame interaction override: when `Some(i)`
/// the primitive's `i`-th visible tab gets a rounded hover background behind
/// its close button. Part of the GTK-specific draw contract — the primitive
/// itself has no mouse-state.
///
/// Returns per-frame hit regions so the caller can dispatch clicks
/// (`App::tab_slot_positions_by_group`, etc.).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tab_bar(
    cr: &Context,
    layout: &pango::Layout,
    width: f64,
    line_height: f64,
    y_offset: f64,
    bar: &quadraui::TabBar,
    theme: &Theme,
    hovered_close_tab: Option<usize>,
) -> TabBarHitInfo {
    use pango::FontDescription;

    // Tab row is taller than line_height for vertical padding.
    let tab_row_height = (line_height * 1.6).ceil();
    let text_y_offset = y_offset + (tab_row_height - line_height) / 2.0;

    // Tab bar background
    let (r, g, b) = vc_to_cairo(theme.tab_bar_bg);
    cr.set_source_rgb(r, g, b);
    cr.rectangle(0.0, y_offset, width, tab_row_height);
    cr.fill().ok();

    layout.set_attributes(None);
    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(super::UI_FONT);
    layout.set_font_description(Some(&ui_font_desc));

    let normal_font = ui_font_desc.clone();
    let mut italic_font = normal_font.clone();
    italic_font.set_style(pango::Style::Italic);

    // Measure each right-side segment's pixel width.
    let mut right_widths: Vec<f64> = Vec::with_capacity(bar.right_segments.len());
    for seg in &bar.right_segments {
        layout.set_font_description(Some(&normal_font));
        layout.set_text(&seg.text);
        let (w, _) = layout.pixel_size();
        right_widths.push(w as f64);
    }
    let reserved_px: f64 = right_widths.iter().sum();
    let effective_tab_area = (width - reserved_px).max(0.0);

    // Per-tab measurement + layout + paint.
    let mut slot_positions: Vec<(f64, f64)> = Vec::with_capacity(bar.tabs.len());
    for _ in 0..bar.scroll_offset.min(bar.tabs.len()) {
        slot_positions.push((0.0, 0.0));
    }

    let close_w = {
        layout.set_font_description(Some(&normal_font));
        layout.set_text("×");
        let (w, _) = layout.pixel_size();
        w as f64
    };
    let tab_pad = 14.0;
    let tab_inner_gap = 10.0;
    let tab_outer_gap = 1.0;

    let mut x = 0.0_f64;
    for (tab_idx, tab) in bar.tabs.iter().enumerate().skip(bar.scroll_offset) {
        if tab.is_preview {
            layout.set_font_description(Some(&italic_font));
        } else {
            layout.set_font_description(Some(&normal_font));
        }
        layout.set_text(&tab.label);
        let (tab_name_w, _) = layout.pixel_size();
        let tab_w = tab_name_w as f64;
        let tab_content_w = tab_pad + tab_w + tab_inner_gap + close_w + tab_pad;
        let slot_w = tab_content_w + tab_outer_gap;
        if x + slot_w > effective_tab_area {
            break;
        }
        slot_positions.push((x, x + slot_w));

        // Tab background.
        let bg_col = if tab.is_active {
            theme.tab_active_bg
        } else {
            theme.tab_bar_bg
        };
        let (br, bgc, bb) = vc_to_cairo(bg_col);
        cr.set_source_rgb(br, bgc, bb);
        cr.rectangle(x, y_offset, tab_content_w, tab_row_height);
        cr.fill().ok();

        // Top accent bar for active tab in focused group.
        if tab.is_active {
            if let Some(accent) = bar.active_accent {
                let (ar, ag, ab) = qc_to_cairo(accent);
                cr.set_source_rgb(ar, ag, ab);
                cr.rectangle(x, y_offset, tab_content_w, 2.0);
                cr.fill().ok();
            }
        }

        // Tab text.
        let fg_col = match (tab.is_active, tab.is_preview) {
            (true, true) => theme.tab_preview_active_fg,
            (true, false) => theme.tab_active_fg,
            (false, true) => theme.tab_preview_inactive_fg,
            (false, false) => theme.tab_inactive_fg,
        };
        let (fr, fgc, fb) = vc_to_cairo(fg_col);
        cr.set_source_rgb(fr, fgc, fb);
        layout.set_font_description(Some(if tab.is_preview {
            &italic_font
        } else {
            &normal_font
        }));
        cr.move_to(x + tab_pad, text_y_offset);
        pangocairo::show_layout(cr, layout);

        // Close (×) or dirty (●) button — with optional rounded hover bg.
        let close_x = x + tab_pad + tab_w + tab_inner_gap;
        let is_close_hovered = hovered_close_tab == Some(tab_idx);
        if is_close_hovered {
            let pad = 2.0;
            let rx = close_x - pad;
            let ry = text_y_offset + pad;
            let rw = close_w + pad * 2.0;
            let rh = line_height - pad * 2.0;
            let (hr, hg, hb) = vc_to_cairo(theme.foreground);
            cr.set_source_rgba(hr, hg, hb, 0.15);
            let radius = 3.0;
            cr.new_path();
            cr.arc(
                rx + rw - radius,
                ry + radius,
                radius,
                -std::f64::consts::FRAC_PI_2,
                0.0,
            );
            cr.arc(
                rx + rw - radius,
                ry + rh - radius,
                radius,
                0.0,
                std::f64::consts::FRAC_PI_2,
            );
            cr.arc(
                rx + radius,
                ry + rh - radius,
                radius,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            );
            cr.arc(
                rx + radius,
                ry + radius,
                radius,
                std::f64::consts::PI,
                3.0 * std::f64::consts::FRAC_PI_2,
            );
            cr.close_path();
            cr.fill().ok();
        }
        let close_glyph = if tab.is_dirty && !is_close_hovered {
            "●"
        } else {
            "×"
        };
        let close_fg = if tab.is_dirty || is_close_hovered {
            theme.foreground
        } else if tab.is_active {
            theme.tab_inactive_fg
        } else {
            theme.separator
        };
        let (cr_, cg_, cb_) = vc_to_cairo(close_fg);
        cr.set_source_rgb(cr_, cg_, cb_);
        layout.set_font_description(Some(&normal_font));
        layout.set_text(close_glyph);
        cr.move_to(close_x, text_y_offset);
        pangocairo::show_layout(cr, layout);

        x += slot_w;
    }

    // ── Right-side segments ─────────────────────────────────────────────
    //
    // Each segment has a pre-measured width and is rendered with normal
    // weight (no bold). Clickable segments' hit regions are derived from
    // `build_tab_bar_primitive`'s segment order: the adapter emits
    // [diff_label?] [diff prev] [diff next] [diff fold] [split right]
    // [split down] [action menu]. We walk the emitted segments and
    // classify clickable ones by their WidgetId.
    let mut diff_regions: Option<[f64; 6]> = None;
    let mut split_info: Option<(f64, f64)> = None;
    let mut action_info: Option<(f64, f64)> = None;

    // Accumulate segment positions left-to-right, starting at `right_base`.
    let right_base = width - reserved_px;
    let mut sx = right_base;
    let mut prev_x: Option<f64> = None;
    let mut next_x: Option<f64> = None;
    let mut fold_x: Option<f64> = None;
    let mut split_right_x: Option<f64> = None;
    let mut split_down_x: Option<f64> = None;

    for (i, seg) in bar.right_segments.iter().enumerate() {
        let seg_w = right_widths[i];
        let fg_col = if seg.is_active {
            theme.tab_active_fg
        } else {
            theme.tab_inactive_fg
        };
        let (fr, fg_, fb) = vc_to_cairo(fg_col);
        cr.set_source_rgb(fr, fg_, fb);
        layout.set_font_description(Some(&normal_font));
        layout.set_text(&seg.text);
        cr.move_to(sx, text_y_offset);
        pangocairo::show_layout(cr, layout);

        if let Some(ref id) = seg.id {
            match id.as_str() {
                "tab:diff_prev" => prev_x = Some(sx),
                "tab:diff_next" => next_x = Some(sx),
                "tab:diff_toggle" => fold_x = Some(sx),
                "tab:split_right" => split_right_x = Some(sx),
                "tab:split_down" => split_down_x = Some(sx),
                "tab:action_menu" => action_info = Some((sx, sx + seg_w)),
                _ => {}
            }
        }
        sx += seg_w;
    }

    // Derive the legacy 6-tuple and 2-tuple shapes the GTK click handler
    // expects. Missing buttons degrade gracefully — callers already check
    // `Option::is_some()`.
    if let (Some(p), Some(n), Some(f)) = (prev_x, next_x, fold_x) {
        // Need each button's end x; recompute from measured widths.
        let prev_idx = bar
            .right_segments
            .iter()
            .position(|s| {
                s.id.as_ref()
                    .is_some_and(|id| id.as_str() == "tab:diff_prev")
            })
            .unwrap();
        let next_idx = bar
            .right_segments
            .iter()
            .position(|s| {
                s.id.as_ref()
                    .is_some_and(|id| id.as_str() == "tab:diff_next")
            })
            .unwrap();
        let fold_idx = bar
            .right_segments
            .iter()
            .position(|s| {
                s.id.as_ref()
                    .is_some_and(|id| id.as_str() == "tab:diff_toggle")
            })
            .unwrap();
        diff_regions = Some([
            p,
            p + right_widths[prev_idx],
            n,
            n + right_widths[next_idx],
            f,
            f + right_widths[fold_idx],
        ]);
    }
    if let (Some(sr), Some(_sd)) = (split_right_x, split_down_x) {
        let sr_idx = bar
            .right_segments
            .iter()
            .position(|s| {
                s.id.as_ref()
                    .is_some_and(|id| id.as_str() == "tab:split_right")
            })
            .unwrap();
        let sd_idx = bar
            .right_segments
            .iter()
            .position(|s| {
                s.id.as_ref()
                    .is_some_and(|id| id.as_str() == "tab:split_down")
            })
            .unwrap();
        let sr_w = right_widths[sr_idx];
        let sd_w = right_widths[sd_idx];
        split_info = Some((sr_w + sd_w, sr_w));
        // Redundant but preserves the original (both_btns_px, btn_right_px) contract.
        let _ = sr;
    }

    // Sample measurement for char-col estimation (shared with old renderer).
    layout.set_font_description(Some(&normal_font));
    layout.set_text("ABCDabcd0123.:_");
    let (sample_px, _) = layout.pixel_size();
    let char_w = (sample_px as f64 / 15.0).max(1.0);
    let available_cols = (effective_tab_area / char_w).floor().max(0.0) as usize;

    layout.set_font_description(Some(&saved_font));

    TabBarHitInfo {
        slot_positions,
        diff_btns: diff_regions.map(|a| (a[0], a[1], a[2], a[3], a[4], a[5])),
        split_btns: split_info,
        action_btn: action_info,
        available_cols,
    }
}

// ─── Activity bar (A.6f) ─────────────────────────────────────────────────────

/// Fixed height (in pixels) of a single activity bar row — matches the
/// legacy `gtk4::Button { set_height_request: 48 }` used in the view!
/// macro. Shared with the click hit-test in `src/gtk/mod.rs`.
pub(super) const ACTIVITY_ROW_PX: f64 = 48.0;

/// Per-row hit region for the GTK activity bar, in DA-local coordinates.
/// Caller dispatches on `id.as_str()` (e.g. `"activity:explorer"` or
/// `"activity:ext:foo"`) to resolve to a `SidebarPanel` variant.
#[derive(Debug, Clone)]
pub(super) struct ActivityBarHit {
    pub y_start: f64,
    pub y_end: f64,
    pub id: quadraui::WidgetId,
    pub tooltip: String,
}

/// Draw a `quadraui::ActivityBar` as a vertical icon strip. Cairo + Pango
/// equivalent of the TUI `quadraui_tui::draw_activity_bar`.
///
/// Geometry: top items from `y=0` downward at `ACTIVITY_ROW_PX` per row;
/// bottom items pin to the bottom edge upward. Icons rendered centred
/// horizontally and vertically in each cell using a Nerd-Font-sized Pango
/// layout (24 px, matching the `.activity-button` CSS from the legacy
/// native-widget path). Active items get a 2 px left-edge accent bar;
/// hovered items get a subtle background tint.
///
/// Returns per-row hit regions so the caller can route clicks and render
/// hover tooltips.
pub(super) fn draw_activity_bar(
    cr: &Context,
    layout: &pango::Layout,
    width: f64,
    height: f64,
    bar: &quadraui::ActivityBar,
    theme: &Theme,
    hovered_idx: Option<usize>,
) -> Vec<ActivityBarHit> {
    use pango::FontDescription;

    // Background.
    let (br, bgc, bb) = vc_to_cairo(theme.tab_bar_bg);
    cr.set_source_rgb(br, bgc, bb);
    cr.rectangle(0.0, 0.0, width, height);
    cr.fill().ok();

    // Right-edge separator matches the `.activity-bar { border-right }` CSS.
    let (sr, sg, sb) = vc_to_cairo(theme.separator);
    cr.set_source_rgb(sr, sg, sb);
    cr.rectangle(width - 1.0, 0.0, 1.0, height);
    cr.fill().ok();

    let saved_font = layout.font_description().unwrap_or_default();
    let icon_font = FontDescription::from_string("Symbols Nerd Font, monospace 20");
    layout.set_font_description(Some(&icon_font));
    layout.set_attributes(None);

    let accent_col = bar.active_accent.map(qc_to_cairo).unwrap_or_else(|| {
        let c = theme.cursor;
        (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0)
    });
    let inactive_fg = vc_to_cairo(theme.status_inactive_fg);
    let active_fg = vc_to_cairo(theme.foreground);
    let hover_bg = {
        // Subtle tint ~10% lighter than the bar bg, falling back to foreground-at-alpha.
        let c = theme.tab_bar_bg.lighten(0.10);
        (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0)
    };

    let rows_total = ((height / ACTIVITY_ROW_PX).floor() as usize).max(1);
    let bottom_count = bar.bottom_items.len().min(rows_total);
    let top_capacity = rows_total.saturating_sub(bottom_count);
    let mut regions: Vec<ActivityBarHit> = Vec::new();

    let draw_row = |y: f64,
                    item: &quadraui::ActivityItem,
                    row_idx: usize,
                    regions: &mut Vec<ActivityBarHit>| {
        let is_hovered = hovered_idx == Some(row_idx);

        // Hover background tint.
        if is_hovered {
            cr.set_source_rgb(hover_bg.0, hover_bg.1, hover_bg.2);
            cr.rectangle(0.0, y, width, ACTIVITY_ROW_PX);
            cr.fill().ok();
        }

        // Left accent bar on active rows (2 px, full row height).
        if item.is_active {
            cr.set_source_rgb(accent_col.0, accent_col.1, accent_col.2);
            cr.rectangle(0.0, y, 2.0, ACTIVITY_ROW_PX);
            cr.fill().ok();
        }

        // Icon glyph, centred in the row.
        layout.set_text(&item.icon);
        let (iw, ih) = layout.pixel_size();
        let fg = if item.is_active || is_hovered {
            active_fg
        } else {
            inactive_fg
        };
        cr.set_source_rgb(fg.0, fg.1, fg.2);
        cr.move_to(
            (width - iw as f64) / 2.0,
            y + (ACTIVITY_ROW_PX - ih as f64) / 2.0,
        );
        pangocairo::show_layout(cr, layout);

        regions.push(ActivityBarHit {
            y_start: y,
            y_end: y + ACTIVITY_ROW_PX,
            id: item.id.clone(),
            tooltip: item.tooltip.clone(),
        });
    };

    // Top items — clipped to `top_capacity` rows.
    for (row_idx, item) in bar.top_items.iter().take(top_capacity).enumerate() {
        draw_row(
            row_idx as f64 * ACTIVITY_ROW_PX,
            item,
            row_idx,
            &mut regions,
        );
    }

    // Bottom items — anchored to the true bottom edge in pixels (not rounded
    // down to a row-index boundary), so the settings icon ends flush with
    // `height` even when `height` isn't an exact multiple of `ACTIVITY_ROW_PX`.
    // The pre-migration `Separator { vexpand: true }` had this flex property;
    // fixed-row layout would otherwise leave a leftover strip below settings.
    for (k, item) in bar.bottom_items.iter().rev().take(bottom_count).enumerate() {
        let y = height - (k + 1) as f64 * ACTIVITY_ROW_PX;
        if y < 0.0 {
            break;
        }
        draw_row(y, item, regions.len(), &mut regions);
    }

    layout.set_font_description(Some(&saved_font));

    regions
}

// ─── Terminal cell grid (A.7) ────────────────────────────────────────────────

/// Draw a `quadraui::Terminal` cell grid via Cairo + Pango.
///
/// Iterates rows, then columns within each row, painting per-cell
/// background then foreground glyph (skipped for spaces). Overlay flags
/// (`is_cursor`, `is_find_active`, `is_find_match`, `selected`) override
/// the per-cell `bg`/`fg` to match the previous bespoke renderer.
///
/// Bold / italic / underline applied via Pango `AttrList` per cell —
/// matches the legacy code's per-cell attribute reset.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_terminal_cells(
    cr: &Context,
    layout: &pango::Layout,
    term: &quadraui::Terminal,
    x: f64,
    content_y: f64,
    cell_area_w: f64,
    line_height: f64,
    char_width: f64,
    theme: &Theme,
) {
    use pango::AttrList;

    for (row_idx, row) in term.cells.iter().enumerate() {
        let row_y = content_y + row_idx as f64 * line_height;
        let mut cell_x = x;
        for cell in row {
            if cell_x + char_width > x + cell_area_w {
                break;
            }
            // Cell background, with overlays.
            let (br, bg, bb) = (cell.bg.r, cell.bg.g, cell.bg.b);
            let (fr, fg2, fb) = (cell.fg.r, cell.fg.g, cell.fg.b);
            let (draw_br, draw_bg, draw_bb) = if cell.is_cursor {
                (fr, fg2, fb)
            } else if cell.is_find_active {
                (255u8, 165u8, 0u8)
            } else if cell.is_find_match {
                (100u8, 80u8, 20u8)
            } else if cell.selected {
                let (sr, sg, sb) = vc_to_cairo(theme.selection);
                ((sr * 255.0) as u8, (sg * 255.0) as u8, (sb * 255.0) as u8)
            } else {
                (br, bg, bb)
            };
            cr.set_source_rgb(
                draw_br as f64 / 255.0,
                draw_bg as f64 / 255.0,
                draw_bb as f64 / 255.0,
            );
            cr.rectangle(cell_x, row_y, char_width, line_height);
            cr.fill().ok();

            // Cell foreground glyph (skip blanks).
            if cell.ch != ' ' && cell.ch != '\0' {
                let (draw_fr, draw_fg, draw_fb) = if cell.is_cursor {
                    (br, bg, bb)
                } else if cell.is_find_active {
                    (0u8, 0u8, 0u8)
                } else {
                    (fr, fg2, fb)
                };
                cr.set_source_rgb(
                    draw_fr as f64 / 255.0,
                    draw_fg as f64 / 255.0,
                    draw_fb as f64 / 255.0,
                );

                let attrs = AttrList::new();
                if cell.bold {
                    attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
                }
                if cell.italic {
                    attrs.insert(pango::AttrInt::new_style(pango::Style::Italic));
                }
                if cell.underline {
                    attrs.insert(pango::AttrInt::new_underline(pango::Underline::Single));
                }
                layout.set_attributes(Some(&attrs));
                let s = cell.ch.to_string();
                layout.set_text(&s);
                cr.move_to(cell_x, row_y);
                pangocairo::show_layout(cr, layout);
                layout.set_attributes(None);
            }

            cell_x += char_width;
        }
    }
}
