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
/// Per D6: `TreeView::layout()` owns the row-stacking math — this
/// rasteriser supplies a per-row height measurer (header rows use
/// `line_height`; everything else uses `(line_height * 1.4).round()`,
/// matching the item-height convention shared with the mouse click
/// handlers in `src/gtk/mod.rs`), calls `tree.layout()`, and paints
/// the resolved `visible_rows` verbatim.
///
/// Row-type styling:
/// - Header rows (SC section titles) get status-bar bg + fg, shorter height.
/// - Selected rows get `fuzzy_selected_bg` + header fg (tall highlight).
/// - Muted rows render dim on the default tree bg.
/// - Other branches render like leaves so folders don't visually separate
///   from sibling files in a recursive tree.
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
    let use_nerd = icons::nerd_fonts_enabled();

    // Resolve the layout once per frame. Header rows are shorter than
    // item rows; the measurer returns each row's height so the
    // primitive can stack them accurately without the rasteriser
    // tracking its own `y_off` cursor.
    let tree_layout = tree.layout(w as f32, h as f32, |i| {
        let is_header = matches!(tree.rows[i].decoration, quadraui::Decoration::Header);
        quadraui::TreeRowMeasure::new(if is_header {
            line_height as f32
        } else {
            item_height as f32
        })
    });

    for vis_row in &tree_layout.visible_rows {
        let row = &tree.rows[vis_row.row_idx];
        let row_y = y + vis_row.bounds.y as f64;
        let row_h = vis_row.bounds.height as f64;

        let is_header = matches!(row.decoration, quadraui::Decoration::Header);
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
        cr.rectangle(x, row_y, w, row_h);
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
                cr.move_to(cursor_x, row_y + (row_h - ch as f64) / 2.0);
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
            cr.move_to(cursor_x, row_y + (row_h - ih as f64) / 2.0);
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
                    row_y,
                    (sw as f64).min(text_right_limit - cursor_x),
                    row_h,
                );
                cr.fill().ok();
            }
            cr.set_source_rgb(span_fg.0, span_fg.1, span_fg.2);
            layout.set_text(&span.text);
            let (sw, sh) = layout.pixel_size();
            cr.move_to(cursor_x, row_y + (row_h - sh as f64) / 2.0);
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
                    cr.rectangle(bx - 2.0, row_y, bw + 4.0, row_h);
                    cr.fill().ok();
                }
                cr.set_source_rgb(bfg.0, bfg.1, bfg.2);
                layout.set_text(&btext);
                let (_, bh) = layout.pixel_size();
                cr.move_to(bx, row_y + (row_h - bh as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
            }
        }
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
            // New #143 field kinds (Slider / ColorPicker / Dropdown) are
            // not yet rendered in GTK — they land with issue #143's
            // migration PR. For now the row is blank on the right side,
            // so existing forms keep working.
            quadraui::FieldKind::Slider { .. }
            | quadraui::FieldKind::ColorPicker { .. }
            | quadraui::FieldKind::Dropdown { .. } => {}
        }

        y_off += row_h;
    }

    layout.set_attributes(None);
}

/// Draw a `quadraui::ListView` into `(x, y, w, h)` on `cr`, using `layout`
/// for text measurement and `theme` for default colours.
///
/// Per D6: `ListView::layout()` owns the row positioning math — this
/// rasteriser supplies a constant per-item height measurer and a
/// `title_height` (`line_height` when `list.title.is_some()`, else 0),
/// then paints the returned `visible_items` and `title_bounds` verbatim.
/// Scroll math lives in the primitive; callers set `list.scroll_offset`
/// and the layout clamps it to the item count.
///
/// Decoration-driven: per-row fg colour derives from `item.decoration`
/// (Error / Warning / Muted / Header / Normal). Header rows get a
/// status-bar-style background so SC section headers stand out.
/// Selected row gets a `▶ ` prefix and `fuzzy_selected_bg` background.
///
/// Note: `list.bordered` is not yet honoured by this backend — no
/// GTK consumer currently sets `bordered = true`. If that changes, add
/// border drawing here and respect the inset bounds that
/// `ListView::layout` already returns when `bordered` is set.
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
    let use_nerd = icons::nerd_fonts_enabled();

    // Resolve the layout once per frame. Per-item heights are uniform
    // (`line_height` each); the primitive handles scroll-clipping and
    // the optional title row at the top.
    let title_h = if list.title.is_some() {
        line_height as f32
    } else {
        0.0
    };
    let list_layout = list.layout(w as f32, h as f32, title_h, |_| {
        quadraui::ListItemMeasure::new(line_height as f32)
    });

    // Title header (optional). Rendered as a single full-width status-bar row.
    if let (Some(title_bounds), Some(title)) = (list_layout.title_bounds, list.title.as_ref()) {
        let ty = y + title_bounds.y as f64;
        let th_px = title_bounds.height as f64;
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, ty, w, th_px);
        cr.fill().ok();

        cr.set_source_rgb(hdr_fg_r, hdr_fg_g, hdr_fg_b);
        let title_text: String = title.spans.iter().map(|s| s.text.as_str()).collect();
        layout.set_text(&title_text);
        let (_, text_h) = layout.pixel_size();
        cr.move_to(x + 2.0, ty + (th_px - text_h as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
    }

    for vis_item in &list_layout.visible_items {
        let item = &list.items[vis_item.item_idx];
        let row_y = y + vis_item.bounds.y as f64;
        let row_w = vis_item.bounds.width as f64;
        let row_h = vis_item.bounds.height as f64;

        let is_selected = vis_item.item_idx == list.selected_idx && list.has_focus;

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
        cr.rectangle(x, row_y, row_w, row_h);
        cr.fill().ok();

        let mut cursor_x = x + 2.0;

        // Selection indicator (▶ on selection, two spaces otherwise — keeps
        // non-selected row text aligned with selected row text).
        let prefix = if is_selected { "▶ " } else { "  " };
        cr.set_source_rgb(decoration_fg.0, decoration_fg.1, decoration_fg.2);
        layout.set_text(prefix);
        let (pw, ph) = layout.pixel_size();
        cr.move_to(cursor_x, row_y + (row_h - ph as f64) / 2.0);
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
            cr.move_to(cursor_x, row_y + (row_h - ih as f64) / 2.0);
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
        let text_right_limit = x + row_w - detail_reserve - 4.0;

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
                    row_y,
                    (sw as f64).min(text_right_limit - cursor_x),
                    row_h,
                );
                cr.fill().ok();
            }
            cr.set_source_rgb(span_fg.0, span_fg.1, span_fg.2);
            layout.set_text(&span.text);
            let (sw, sh) = layout.pixel_size();
            cr.move_to(cursor_x, row_y + (row_h - sh as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
            cursor_x += sw as f64;
        }

        // Detail (right-aligned, dimmed).
        if let Some((detail_text, dw)) = detail_info {
            let dx = x + row_w - dw - 4.0;
            if dx > cursor_x {
                cr.set_source_rgb(dim_r, dim_g, dim_b);
                layout.set_text(&detail_text);
                let (_, dh) = layout.pixel_size();
                cr.move_to(dx, row_y + (row_h - dh as f64) / 2.0);
                pangocairo::show_layout(cr, layout);
            }
        }
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

    // Items region sizing: leave room for title + query + separator at the
    // top and a small inset above the bottom border. `Palette::layout` owns
    // the title/query row bounds; the separator and scrollbar stay as
    // rasteriser chrome. Snap items height to a whole multiple of
    // line_height so the last row occupies a full cell.
    const BOTTOM_INSET: f64 = 4.0;
    let sep_y = y + 2.0 * line_height; // rendered below
    let rows_y = sep_y + 1.0;
    let rows_h_raw = ((y + h) - rows_y - BOTTOM_INSET).max(0.0);
    let visible_rows = (rows_h_raw / line_height) as usize;
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

    // Per D6: let `Palette::layout` resolve the title + query bounds and
    // the visible item window. The rasteriser then paints at the returned
    // coordinates — no per-row y-stepping math here. We shallow-clone the
    // palette so we can give `scroll_offset` the visibility-clamped
    // effective value without mutating the caller's state.
    let mut palette_local = palette.clone();
    palette_local.scroll_offset = effective_offset;
    let palette_layout = palette_local.layout(
        w as f32,
        (rows_y + rows_h - y) as f32, // title + query + separator + items region
        line_height as f32,           // title_height
        line_height as f32,           // query_height
        |_| quadraui::PaletteItemMeasure::new(line_height as f32),
    );

    // ── Title row ─────────────────────────────────────────────────────
    if let Some(title_bounds) = palette_layout.title_bounds {
        let ty = y + title_bounds.y as f64;
        let th_px = title_bounds.height as f64;
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
        let (_, text_h) = layout.pixel_size();
        cr.move_to(x + 8.0, ty + (th_px - text_h as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
    }

    // ── Query row ─────────────────────────────────────────────────────
    if let Some(query_bounds) = palette_layout.query_bounds {
        let query_y = y + query_bounds.y as f64;
        let qh_px = query_bounds.height as f64;
        let prompt = "> ";
        cr.set_source_rgb(query_r, query_g, query_b);
        layout.set_text(prompt);
        let (prompt_w, qh) = layout.pixel_size();
        cr.move_to(x + 8.0, query_y + (qh_px - qh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);

        let query_text_x = x + 8.0 + prompt_w as f64;
        layout.set_text(&palette.query);
        cr.move_to(query_text_x, query_y + (qh_px - qh as f64) / 2.0);
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
        cr.set_source_rgb(query_r, query_g, query_b);
        cr.rectangle(cursor_x, query_y, cursor_w, qh_px);
        cr.fill().ok();
        if !cursor_char.trim().is_empty() {
            cr.set_source_rgb(bg_r, bg_g, bg_b);
            cr.move_to(cursor_x, query_y + (qh_px - qh as f64) / 2.0);
            layout.set_text(&cursor_char);
            pangocairo::show_layout(cr, layout);
        }
    }

    // ── Separator row ─────────────────────────────────────────────────
    cr.set_source_rgb(border_r, border_g, border_b);
    cr.set_line_width(1.0);
    cr.move_to(x, sep_y);
    cr.line_to(x + w, sep_y);
    cr.stroke().ok();

    // ── Result rows ───────────────────────────────────────────────────
    cr.save().ok();
    cr.rectangle(x, rows_y, content_w, rows_h);
    cr.clip();

    // The primitive's item bounds sit immediately below the query
    // region at `y + title_height + query_height`; our rasteriser
    // reserves an extra 1px for the separator line beneath the query,
    // so we stack items at `rows_y + i * line_height` instead of
    // reusing `vis_item.bounds.y` directly. The primitive still
    // decides which items are visible (via `resolved_scroll_offset`
    // + the clipping heuristics inside `layout()`), which is the
    // important part — row counts and indices stay authoritative.
    for (render_i, vis_item) in palette_layout.visible_items.iter().enumerate() {
        let item = &palette.items[vis_item.item_idx];
        let row_y = rows_y + render_i as f64 * line_height;
        let row_h = line_height;
        let is_selected = vis_item.item_idx == palette.selected_idx && palette.has_focus;

        if is_selected {
            cr.set_source_rgb(sel_r, sel_g, sel_b);
            cr.rectangle(x, row_y, content_w, row_h);
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
            cr.move_to(cursor, row_y + (row_h - ih as f64) / 2.0);
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
        cr.move_to(cursor, row_y + (row_h - lh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);

        // Detail (right-aligned, dimmed).
        if let Some((detail_text, dw)) = detail_info {
            let dx = x + content_w - dw - 8.0;
            cr.set_source_rgb(dim_r, dim_g, dim_b);
            layout.set_attributes(None);
            layout.set_text(&detail_text);
            let (_, dh) = layout.pixel_size();
            cr.move_to(dx, row_y + (row_h - dh as f64) / 2.0);
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
/// Per D6: the `StatusBar::layout()` primitive owns the layout math
/// (left-accumulate, right-align, fit-drop). This rasteriser supplies
/// a Pango pixel-width measurement closure, calls `bar.layout()`, and
/// paints the returned `visible_segments` verbatim. No positional
/// math lives here — any layout policy change (e.g. the #159 priority
/// drop) happens once in quadraui and applies to TUI + GTK together.
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
    // Public rasteriser in `quadraui::gtk` consumes a backend-agnostic
    // `quadraui::Theme`. Build one from the rich vimcode theme — the
    // status bar reads only `background` (fallback fill when bar has
    // no segments) but `foreground` is populated for symmetry.
    let q_theme = quadraui::Theme {
        background: render::to_quadraui_color(theme.background),
        foreground: render::to_quadraui_color(theme.foreground),
    };
    quadraui::gtk::draw_status_bar(cr, layout, x, y, width, line_height, bar, &q_theme)
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
    /// The `scroll_offset` that would make the active tab visible in this
    /// frame, computed from actual Pango pixel measurements via
    /// `quadraui::TabBar::fit_active_scroll_offset`. The caller compares
    /// this to the engine's current `tab_scroll_offset` and triggers a
    /// repaint if they differ — the engine's char-based algorithm
    /// (`tab_display_width`) under-estimates GTK tab widths by ~4 chars
    /// per tab (it doesn't account for `tab_pad` / `tab_inner_gap` /
    /// close button), so without this correction the active tab can land
    /// off-screen.
    pub correct_scroll_offset: usize,
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
    let ui_font_desc = FontDescription::from_string(&super::UI_FONT());
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

    // Pre-measure every tab's full slot width (label + padding + close
    // button + outer gap) in pixels. We use this for two things: the
    // per-tab paint loop below, AND for `fit_active_scroll_offset` which
    // tells the caller whether the engine's `tab_scroll_offset` is
    // off — see `TabBarHitInfo.correct_scroll_offset`.
    let tab_slot_widths: Vec<f64> = bar
        .tabs
        .iter()
        .map(|tab| {
            if tab.is_preview {
                layout.set_font_description(Some(&italic_font));
            } else {
                layout.set_font_description(Some(&normal_font));
            }
            layout.set_text(&tab.label);
            let (name_w, _) = layout.pixel_size();
            tab_pad + name_w as f64 + tab_inner_gap + close_w + tab_pad + tab_outer_gap
        })
        .collect();

    // Compute the scroll offset that would make the active tab visible
    // given THIS frame's actual pixel widths. The caller compares to
    // bar.scroll_offset and triggers a repaint if they differ.
    let active_idx = bar.tabs.iter().position(|t| t.is_active);
    let correct_scroll_offset = if let Some(active) = active_idx {
        quadraui::TabBar::fit_active_scroll_offset(
            active,
            bar.tabs.len(),
            effective_tab_area as usize,
            |i| tab_slot_widths[i] as usize,
        )
    } else {
        bar.scroll_offset
    };

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
        correct_scroll_offset,
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

/// Draw a `quadraui::Tooltip` at its resolved layout position.
///
/// Per D6, the caller computes anchor + viewport + content measurement
/// and asks `tooltip.layout()` for the resolved bounds; this rasteriser
/// paints the box (background + 1px border) plus either the plain
/// `text` or per-row `styled_lines`.
///
/// `padding_x` is the horizontal padding (in pixels) from the left
/// border to the start of text — consumers typically pass the same
/// `char_width` they used when computing the tooltip's measured width.
pub(super) fn draw_tooltip(
    cr: &Context,
    layout: &pango::Layout,
    tooltip: &quadraui::Tooltip,
    tooltip_layout: &quadraui::TooltipLayout,
    line_height: f64,
    padding_x: f64,
    theme: &Theme,
) {
    let bounds = tooltip_layout.bounds;
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }

    let (bg_r, bg_g, bg_b) = tooltip
        .bg
        .map(qc_to_cairo)
        .unwrap_or_else(|| vc_to_cairo(theme.hover_bg));
    let (fg_r, fg_g, fg_b) = tooltip
        .fg
        .map(qc_to_cairo)
        .unwrap_or_else(|| vc_to_cairo(theme.hover_fg));
    let (br, bg, bb) = vc_to_cairo(theme.hover_border);

    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(
        bounds.x as f64,
        bounds.y as f64,
        bounds.width as f64,
        bounds.height as f64,
    );
    cr.fill().ok();

    cr.set_source_rgb(br, bg, bb);
    cr.set_line_width(1.0);
    cr.rectangle(
        bounds.x as f64,
        bounds.y as f64,
        bounds.width as f64,
        bounds.height as f64,
    );
    cr.stroke().ok();

    let text_x = bounds.x as f64 + padding_x;
    let text_top = bounds.y as f64 + 2.0;

    if let Some(ref styled_lines) = tooltip.styled_lines {
        // Multi-line styled path. Each `StyledText` is one row.
        // Per-span fg overrides the tooltip default; bg defaults to
        // tooltip bg (no per-span bg highlighting on GTK yet).
        for (i, styled) in styled_lines.iter().enumerate() {
            let row_y = text_top + i as f64 * line_height;
            if row_y + line_height > bounds.y as f64 + bounds.height as f64 {
                break;
            }
            cr.move_to(text_x, row_y);
            let mut x_off = text_x;
            for span in &styled.spans {
                let (sr, sg, sb) = span.fg.map(qc_to_cairo).unwrap_or((fg_r, fg_g, fg_b));
                cr.set_source_rgb(sr, sg, sb);
                layout.set_text(&span.text);
                layout.set_attributes(None);
                cr.move_to(x_off, row_y);
                pangocairo::show_layout(cr, layout);
                let (text_w, _) = layout.pixel_size();
                x_off += text_w as f64;
            }
        }
        return;
    }

    cr.set_source_rgb(fg_r, fg_g, fg_b);
    for (i, text_line) in tooltip.text.lines().enumerate() {
        let row_y = text_top + i as f64 * line_height;
        if row_y + line_height > bounds.y as f64 + bounds.height as f64 {
            break;
        }
        layout.set_text(text_line);
        layout.set_attributes(None);
        cr.move_to(text_x, row_y);
        pangocairo::show_layout(cr, layout);
    }
}

/// Flatten a `quadraui::StyledText` to a plain `String`. Dialog title +
/// body don't currently carry per-span style overrides, so plain text
/// suffices on GTK; mirrors the TUI helper of the same name.
fn styled_text_plain(text: &quadraui::StyledText) -> String {
    text.spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
}

/// Draw a `quadraui::Dialog` at its resolved layout. Returns the button
/// hit-rectangles (in the same `(x, y, w, h)` shape the legacy renderer
/// returned) so the caller's click handler keeps working unchanged.
///
/// Per D6, the caller measures title/body/buttons in pixels and asks
/// `dialog.layout()` for the resolved sub-bounds; this rasteriser paints
/// the box (background + 1px border), title bar, body text, optional
/// input, and buttons (with the default-button highlight on the
/// primary).
pub(super) fn draw_dialog(
    cr: &Context,
    layout: &pango::Layout,
    ui_layout: &pango::Layout,
    dialog: &quadraui::Dialog,
    dialog_layout: &quadraui::DialogLayout,
    line_height: f64,
    theme: &Theme,
) -> Vec<(f64, f64, f64, f64)> {
    let bounds = dialog_layout.bounds;
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Vec::new();
    }

    let (bg_r, bg_g, bg_b) = vc_to_cairo(theme.fuzzy_bg);
    let (fg_r, fg_g, fg_b) = vc_to_cairo(theme.fuzzy_fg);
    let (br_r, br_g, br_b) = vc_to_cairo(theme.fuzzy_border);
    let (sel_r, sel_g, sel_b) = vc_to_cairo(theme.fuzzy_selected_bg);
    let (input_bg_r, input_bg_g, input_bg_b) = vc_to_cairo(theme.completion_bg);

    let bx = bounds.x as f64;
    let by = bounds.y as f64;
    let bw = bounds.width as f64;
    let bh = bounds.height as f64;

    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(bx, by, bw, bh);
    cr.fill().ok();

    cr.set_source_rgb(br_r, br_g, br_b);
    cr.set_line_width(1.0);
    cr.rectangle(bx, by, bw, bh);
    cr.stroke().ok();

    if let Some(title_rect) = dialog_layout.title_bounds {
        let (tr, tg, tb) = vc_to_cairo(theme.fuzzy_title_fg);
        cr.set_source_rgb(tr, tg, tb);
        ui_layout.set_text(&styled_text_plain(&dialog.title));
        ui_layout.set_attributes(None);
        cr.move_to(title_rect.x as f64, title_rect.y as f64);
        pangocairo::show_layout(cr, ui_layout);
    }

    let body_b = dialog_layout.body_bounds;
    cr.set_source_rgb(fg_r, fg_g, fg_b);
    for (i, line) in styled_text_plain(&dialog.body).split('\n').enumerate() {
        let row_y = body_b.y as f64 + i as f64 * line_height;
        if row_y + line_height > body_b.y as f64 + body_b.height as f64 {
            break;
        }
        layout.set_text(line);
        layout.set_attributes(None);
        cr.move_to(body_b.x as f64, row_y);
        pangocairo::show_layout(cr, layout);
    }

    if let (Some(input_b), Some(input)) = (dialog_layout.input_bounds, dialog.input.as_ref()) {
        let ix = input_b.x as f64;
        let iy = input_b.y as f64;
        let iw = input_b.width as f64;
        let ih = input_b.height as f64;
        cr.set_source_rgb(input_bg_r, input_bg_g, input_bg_b);
        cr.rectangle(ix, iy, iw, ih);
        cr.fill().ok();
        cr.set_source_rgb(br_r, br_g, br_b);
        cr.rectangle(ix, iy, iw, ih);
        cr.stroke().ok();
        cr.set_source_rgb(fg_r, fg_g, fg_b);
        let display = if input.value.is_empty() {
            format!(" {}", input.placeholder)
        } else {
            format!(" {}", input.value)
        };
        layout.set_text(&display);
        layout.set_attributes(None);
        let (_, ilh) = layout.pixel_size();
        cr.move_to(ix + 2.0, iy + (ih - ilh as f64) / 2.0);
        pangocairo::show_layout(cr, layout);
    }

    let mut rects = Vec::with_capacity(dialog_layout.visible_buttons.len());
    for vis in &dialog_layout.visible_buttons {
        let btn = &dialog.buttons[vis.button_idx];
        let bx = vis.bounds.x as f64;
        let by = vis.bounds.y as f64;
        let bw = vis.bounds.width as f64;
        let bh = vis.bounds.height as f64;
        rects.push((bx, by, bw, bh));

        if btn.is_default {
            cr.set_source_rgb(sel_r, sel_g, sel_b);
            cr.rectangle(bx, by, bw, bh);
            cr.fill().ok();
        }

        let label = if dialog.vertical_buttons {
            let prefix = if btn.is_default { "▸ " } else { "  " };
            format!("{}{}", prefix, btn.label)
        } else {
            format!("  {}  ", btn.label)
        };
        cr.set_source_rgb(fg_r, fg_g, fg_b);
        ui_layout.set_text(&label);
        ui_layout.set_attributes(None);
        let (lw, lh) = ui_layout.pixel_size();
        let lw = lw as f64;
        let lh = lh as f64;
        let label_x = if dialog.vertical_buttons {
            bx + 4.0
        } else {
            bx + (bw - lw) / 2.0
        };
        let label_y = by + (bh - lh) / 2.0;
        cr.move_to(label_x, label_y);
        pangocairo::show_layout(cr, ui_layout);
    }
    rects
}

/// Draw a `quadraui::ContextMenu` at its resolved layout. Returns the
/// per-clickable-item hit-rectangles `(x, y, w, h, item_idx)` so the
/// caller's click handler can map a click to the original
/// `ContextMenuItem` index without re-running layout. Hover state is
/// owned by the primitive (`menu.selected_idx`) — the highlight
/// follows whatever the app sets, so callers update `selected_idx`
/// from mouse motion before invoking this rasteriser.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_context_menu(
    cr: &Context,
    layout: &pango::Layout,
    menu: &quadraui::ContextMenu,
    menu_layout: &quadraui::ContextMenuLayout,
    line_height: f64,
    theme: &Theme,
) -> Vec<(f64, f64, f64, f64, quadraui::WidgetId)> {
    let bounds = menu_layout.bounds;
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Vec::new();
    }

    let bx = bounds.x as f64;
    let by = bounds.y as f64;
    let bw = bounds.width as f64;
    let bh = bounds.height as f64;

    let bg = menu
        .bg
        .map(qc_to_cairo)
        .unwrap_or_else(|| vc_to_cairo(theme.hover_bg));
    let (br, bg_g, bb) = vc_to_cairo(theme.hover_border);
    let (fg_r, fg_g, fg_b) = vc_to_cairo(theme.foreground);
    let (sel_r, sel_g, sel_b) = vc_to_cairo(theme.sidebar_sel_bg);
    let (sep_r, sep_g, sep_b) = vc_to_cairo(theme.line_number_fg);
    let (dim_r, dim_g, dim_b) = vc_to_cairo(theme.foreground.darken(0.5));

    cr.set_source_rgb(bg.0, bg.1, bg.2);
    cr.rectangle(bx, by, bw, bh);
    cr.fill().ok();

    cr.set_source_rgb(br, bg_g, bb);
    cr.set_line_width(1.0);
    cr.rectangle(bx, by, bw, bh);
    cr.stroke().ok();

    let mut rects: Vec<(f64, f64, f64, f64, quadraui::WidgetId)> = Vec::new();

    for vis in &menu_layout.visible_items {
        let item = &menu.items[vis.item_idx];
        let row_x = vis.bounds.x as f64;
        let row_y = vis.bounds.y as f64;
        let row_w = vis.bounds.width as f64;
        let row_h = vis.bounds.height as f64;

        if vis.is_separator {
            cr.set_source_rgb(sep_r, sep_g, sep_b);
            cr.set_line_width(0.5);
            let sep_y = row_y + row_h * 0.5;
            cr.move_to(row_x + 4.0, sep_y);
            cr.line_to(row_x + row_w - 4.0, sep_y);
            cr.stroke().ok();
            continue;
        }

        let is_selected = vis.item_idx == menu.selected_idx && vis.clickable;
        if is_selected {
            cr.set_source_rgb(sel_r, sel_g, sel_b);
            cr.rectangle(row_x + 1.0, row_y, row_w - 2.0, row_h);
            cr.fill().ok();
        }

        let label_text = item
            .label
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        let (lr, lg, lb) = if vis.clickable {
            (fg_r, fg_g, fg_b)
        } else {
            (dim_r, dim_g, dim_b)
        };
        cr.set_source_rgb(lr, lg, lb);
        layout.set_text(&label_text);
        layout.set_attributes(None);
        let (_, lh) = layout.pixel_size();
        let text_y = row_y + (row_h - lh as f64) * 0.5;
        cr.move_to(row_x + 8.0, text_y);
        pangocairo::show_layout(cr, layout);

        if let Some(ref det) = item.detail {
            let det_text = det
                .spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>();
            if !det_text.is_empty() {
                layout.set_text(&det_text);
                let (sw, _) = layout.pixel_size();
                cr.set_source_rgb(sep_r, sep_g, sep_b);
                cr.move_to(row_x + row_w - sw as f64 - 8.0, text_y);
                pangocairo::show_layout(cr, layout);
            }
        }

        if vis.clickable {
            if let Some(ref id) = item.id {
                rects.push((row_x, row_y, row_w, row_h, id.clone()));
            }
        }
    }
    let _ = line_height;
    rects
}
/// Visible width of the rich-text-popup scrollbar in pixels. Wider
/// than the layout's 1px border so the bar is paint+click-friendly.
/// Shared with `draw_editor_hover_popup` so paint and hit-test
/// geometry stay in sync (#215).
pub(super) const RICH_TEXT_POPUP_SB_WIDTH: f64 = 8.0;
/// Pixels of inset between the scrollbar's right edge and the popup's
/// right border. Same role as `RICH_TEXT_POPUP_SB_WIDTH`.
pub(super) const RICH_TEXT_POPUP_SB_INSET: f64 = 1.0;

/// Draw a `quadraui::RichTextPopup` at its resolved layout. Returns
/// per-link hit regions in `(x, y, w, h, url)` form. Each visible
/// line is rendered as a SINGLE Pango call with an `AttrList` —
/// per-span fg/bold/italic + per-character selection bg become
/// attribute ranges. This avoids the per-span manual-advance bug
/// where proportional Pango widths drift from monospace
/// `char_width * char_count` math (#214 first-cut regression).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_rich_text_popup(
    cr: &Context,
    pango_layout: &pango::Layout,
    popup: &quadraui::RichTextPopup,
    layout: &quadraui::RichTextPopupLayout,
    line_height: f64,
    char_width: f64,
    theme: &Theme,
) -> Vec<(f64, f64, f64, f64, String)> {
    let _ = char_width;
    let _ = line_height;
    let bx = layout.bounds.x as f64;
    let by = layout.bounds.y as f64;
    let bw = layout.bounds.width as f64;
    let bh = layout.bounds.height as f64;
    if bw <= 0.0 || bh <= 0.0 {
        return Vec::new();
    }

    let (bg_r, bg_g, bg_b) = popup
        .bg
        .map(qc_to_cairo)
        .unwrap_or_else(|| vc_to_cairo(theme.hover_bg));
    let (fg_r, fg_g, fg_b) = popup
        .fg
        .map(qc_to_cairo)
        .unwrap_or_else(|| vc_to_cairo(theme.hover_fg));
    let (border_r, border_g, border_b) = if popup.has_focus {
        vc_to_cairo(theme.md_link)
    } else {
        vc_to_cairo(theme.hover_border)
    };

    // Background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(bx, by, bw, bh);
    cr.fill().ok();
    // Border.
    cr.set_source_rgb(border_r, border_g, border_b);
    cr.set_line_width(1.0);
    cr.rectangle(bx, by, bw, bh);
    cr.stroke().ok();

    let ui_font_desc = pango::FontDescription::from_string(&UI_FONT());

    // Clip text rendering to the content area so long lines (Pango is
    // unbounded) and selection rectangles don't bleed past the popup
    // boundary into the editor area behind. Restored at end of draw.
    let content = layout.content_bounds;
    cr.save().ok();
    cr.rectangle(
        content.x as f64,
        content.y as f64,
        content.width as f64,
        content.height as f64,
    );
    cr.clip();

    for vis in &layout.visible_lines {
        let row_y = vis.bounds.y as f64;
        let line_x = vis.bounds.x as f64;
        let line_idx = vis.line_idx;
        let raw_text = popup
            .line_text
            .get(line_idx)
            .map(String::as_str)
            .unwrap_or("");

        // Single-Pango-call render with per-span AttrList.
        if let Some(styled) = popup.lines.get(line_idx) {
            pango_layout.set_text(raw_text);
            pango_layout.set_font_description(Some(&ui_font_desc));
            let attrs = pango::AttrList::new();
            // Per-line font scale (markdown headings render larger).
            let line_scale = popup.line_scales.get(line_idx).copied().unwrap_or(1.0);
            if (line_scale - 1.0).abs() > 0.01 {
                let mut a = pango::AttrFloat::new_scale(line_scale as f64);
                a.set_start_index(0);
                a.set_end_index(raw_text.len() as u32);
                attrs.insert(a);
            }
            // Compute selection byte range once for this line.
            let (sel_start_byte, sel_end_byte) = popup
                .selection
                .map(|sel| selection_byte_range(sel, line_idx, raw_text))
                .unwrap_or((0, 0));
            let in_selection = |byte_start: usize, byte_end: usize| -> bool {
                sel_end_byte > sel_start_byte
                    && byte_start >= sel_start_byte
                    && byte_end <= sel_end_byte
            };
            let to_u16 = |c: u8| ((c as u16) << 8) | c as u16;
            let bg_color = popup.bg.unwrap_or(quadraui::Color::rgb(0, 0, 0));

            // Selection bg used to be a single Pango background attr, but
            // adjacent text runs (one per fg colour change) produced
            // hairline antialiasing gaps where the per-run rects met
            // (#219). The fix paints the selection rect once in Cairo
            // BEFORE the Pango render so the bg is a single solid fill.
            // The Pango call below still inverts fg per-character within
            // the selected range so the text remains legible.

            // Per-span fg + bold + italic. Each span is split by the
            // selection boundary so we can swap the fg colour to the
            // inverted (popup bg) for the in-selection chunk without
            // an attr-override conflict.
            let push_fg_attr =
                |attrs: &pango::AttrList, start: usize, end: usize, fg: quadraui::Color| {
                    let mut a =
                        pango::AttrColor::new_foreground(to_u16(fg.r), to_u16(fg.g), to_u16(fg.b));
                    a.set_start_index(start as u32);
                    a.set_end_index(end as u32);
                    attrs.insert(a);
                };
            let push_bold = |attrs: &pango::AttrList, start: usize, end: usize| {
                let mut a = pango::AttrInt::new_weight(pango::Weight::Bold);
                a.set_start_index(start as u32);
                a.set_end_index(end as u32);
                attrs.insert(a);
            };
            let push_italic = |attrs: &pango::AttrList, start: usize, end: usize| {
                let mut a = pango::AttrInt::new_style(pango::Style::Italic);
                a.set_start_index(start as u32);
                a.set_end_index(end as u32);
                attrs.insert(a);
            };
            let mut byte_pos: usize = 0;
            for span in &styled.spans {
                let len = span.text.len();
                let start = byte_pos;
                let end = byte_pos + len;

                // Split the span into up-to-three chunks based on
                // selection boundary: pre-selection / in-selection /
                // post-selection. Each chunk gets its own fg attr
                // (with inverted colour for the in-selection chunk).
                let span_fg = span.fg.unwrap_or(bg_color);
                let inv_fg = bg_color;

                let chunk_start_pre = start;
                let chunk_end_pre = end.min(sel_start_byte).max(start);
                let chunk_start_in = start.max(sel_start_byte).min(end);
                let chunk_end_in = end.min(sel_end_byte).max(chunk_start_in);
                let chunk_start_post = end.min(sel_end_byte).max(start);
                let chunk_end_post = end.max(chunk_start_post);

                if span.fg.is_some() && chunk_end_pre > chunk_start_pre {
                    push_fg_attr(&attrs, chunk_start_pre, chunk_end_pre, span_fg);
                }
                if chunk_end_in > chunk_start_in && in_selection(chunk_start_in, chunk_end_in) {
                    push_fg_attr(&attrs, chunk_start_in, chunk_end_in, inv_fg);
                }
                if span.fg.is_some() && chunk_end_post > chunk_start_post {
                    push_fg_attr(&attrs, chunk_start_post, chunk_end_post, span_fg);
                }
                if span.bold {
                    push_bold(&attrs, start, end);
                }
                if span.italic {
                    push_italic(&attrs, start, end);
                }
                byte_pos += len;
            }
            // Focused-link underline.
            if popup.has_focus {
                if let Some(focused) = popup.focused_link {
                    if let Some(link) = popup.links.get(focused) {
                        if link.line == line_idx {
                            let mut ul = pango::AttrInt::new_underline(pango::Underline::Single);
                            ul.set_start_index(link.start_byte as u32);
                            ul.set_end_index(link.end_byte as u32);
                            attrs.insert(ul);
                        }
                    }
                }
            }
            pango_layout.set_attributes(Some(&attrs));

            // Selection bg fill (Cairo rect underneath the text). With
            // attrs applied so `index_to_pos` honours the font scale on
            // heading rows. Pango byte indices clamp to text length, so
            // a sel_end_byte at end-of-line maps to the line's right
            // edge correctly.
            if sel_end_byte > sel_start_byte {
                let fg_color = popup
                    .fg
                    .unwrap_or_else(|| quadraui::Color::rgb(255, 255, 255));
                let start_pos = pango_layout.index_to_pos(sel_start_byte as i32);
                let end_pos = pango_layout.index_to_pos(sel_end_byte as i32);
                let x0 = line_x + start_pos.x() as f64 / pango::SCALE as f64;
                let x1 = line_x + end_pos.x() as f64 / pango::SCALE as f64;
                let row_h = vis.bounds.height as f64;
                cr.set_source_rgb(
                    fg_color.r as f64 / 255.0,
                    fg_color.g as f64 / 255.0,
                    fg_color.b as f64 / 255.0,
                );
                cr.rectangle(x0.min(x1), row_y, (x1 - x0).abs(), row_h);
                cr.fill().ok();
            }

            cr.set_source_rgb(fg_r, fg_g, fg_b);
            cr.move_to(line_x, row_y);
            pangocairo::show_layout(cr, pango_layout);
            pango_layout.set_attributes(None);
        }
    }

    cr.restore().ok(); // pop the content clip

    // Scrollbar — wider than the 1px border so it's visually + clickably
    // present. Draw at the right inside edge of the popup. Constants
    // shared with `draw_editor_hover_popup` so click hit-test matches
    // what's painted (#215).
    if let Some(sb) = layout.scrollbar {
        let sb_w = RICH_TEXT_POPUP_SB_WIDTH;
        let sb_x = bx + bw - sb_w - RICH_TEXT_POPUP_SB_INSET;
        let track_y = sb.track.y as f64;
        let track_h = sb.track.height as f64;
        // Track background.
        let (sr, sg, sbb) = vc_to_cairo(theme.line_number_fg);
        cr.set_source_rgba(sr, sg, sbb, 0.3);
        cr.rectangle(sb_x, track_y, sb_w, track_h);
        cr.fill().ok();
        // Thumb.
        let thumb_top_off = (sb.thumb.y - sb.track.y) as f64;
        let thumb_h = sb.thumb.height as f64;
        cr.set_source_rgb(border_r, border_g, border_b);
        cr.rectangle(sb_x + 1.0, track_y + thumb_top_off, sb_w - 2.0, thumb_h);
        cr.fill().ok();
    }

    // Link hit regions in (x, y, w, h, url) form.
    layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (
                rect.x as f64,
                rect.y as f64,
                rect.width as f64,
                rect.height as f64,
                url,
            )
        })
        .collect()
}

/// Translate a `TextSelection` (in char columns) into the byte range
/// that this line contributes to the selection. Returns `(0, 0)` when
/// the line is outside the selection.
fn selection_byte_range(
    sel: quadraui::TextSelection,
    line_idx: usize,
    line_text: &str,
) -> (usize, usize) {
    if line_idx < sel.start_line || line_idx > sel.end_line {
        return (0, 0);
    }
    let char_to_byte = |col: usize| -> usize {
        line_text
            .char_indices()
            .nth(col)
            .map(|(b, _)| b)
            .unwrap_or(line_text.len())
    };
    let (start_col, end_col) = if sel.start_line == sel.end_line {
        (sel.start_col, sel.end_col)
    } else if line_idx == sel.start_line {
        (sel.start_col, line_text.chars().count())
    } else if line_idx == sel.end_line {
        (0, sel.end_col)
    } else {
        (0, line_text.chars().count())
    };
    if end_col <= start_col {
        return (0, 0);
    }
    (char_to_byte(start_col), char_to_byte(end_col))
}
