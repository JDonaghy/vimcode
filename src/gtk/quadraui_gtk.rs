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
    quadraui::gtk::draw_status_bar(cr, layout, x, y, width, line_height, bar, &q_theme(theme))
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

/// Draw a `quadraui::TabBar` and reshape the public `TabBarHits` into
/// vimcode's `TabBarHitInfo` (which carries app-specific groupings for
/// the diff toolbar / split / action-menu buttons keyed by their
/// `WidgetId` in `bar.right_segments`).
///
/// The actual painting + generic hit-region collection lives in
/// `quadraui::gtk::draw_tab_bar`; this wrapper only does the
/// vimcode-side WidgetId lookup. `hovered_close_tab` is forwarded
/// unchanged.
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

    // The public rasteriser uses whatever font is on the layout when
    // it's called. Vimcode renders tabs in a sans-serif UI font (not
    // the editor's monospace font), so set that before delegating and
    // restore the caller's font afterwards.
    let saved_font = layout.font_description().unwrap_or_default();
    let ui_font_desc = FontDescription::from_string(&super::UI_FONT());
    layout.set_font_description(Some(&ui_font_desc));

    let hits = quadraui::gtk::draw_tab_bar(
        cr,
        layout,
        width,
        line_height,
        y_offset,
        bar,
        &q_theme(theme),
        hovered_close_tab,
    );

    layout.set_font_description(Some(&saved_font));

    // Reshape `hits.right_segment_bounds` into vimcode's app-specific
    // (diff_btns, split_btns, action_btn) groupings using the segments'
    // `WidgetId` strings emitted by `build_tab_bar_primitive`.
    let mut prev: Option<(f64, f64)> = None;
    let mut next: Option<(f64, f64)> = None;
    let mut fold: Option<(f64, f64)> = None;
    let mut split_right: Option<(f64, f64)> = None;
    let mut split_down: Option<(f64, f64)> = None;
    let mut action: Option<(f64, f64)> = None;
    for (i, seg) in bar.right_segments.iter().enumerate() {
        let bounds = hits.right_segment_bounds.get(i).copied();
        let Some(b) = bounds else { continue };
        if let Some(ref id) = seg.id {
            match id.as_str() {
                "tab:diff_prev" => prev = Some(b),
                "tab:diff_next" => next = Some(b),
                "tab:diff_toggle" => fold = Some(b),
                "tab:split_right" => split_right = Some(b),
                "tab:split_down" => split_down = Some(b),
                "tab:action_menu" => action = Some(b),
                _ => {}
            }
        }
    }

    let diff_btns = match (prev, next, fold) {
        (Some(p), Some(n), Some(f)) => Some((p.0, p.1, n.0, n.1, f.0, f.1)),
        _ => None,
    };
    // Preserve the legacy `(both_btns_px, btn_right_px)` contract.
    let split_btns = match (split_right, split_down) {
        (Some(sr), Some(sd)) => {
            let sr_w = sr.1 - sr.0;
            let sd_w = sd.1 - sd.0;
            Some((sr_w + sd_w, sr_w))
        }
        _ => None,
    };

    TabBarHitInfo {
        slot_positions: hits.slot_positions,
        diff_btns,
        split_btns,
        action_btn: action,
        available_cols: hits.available_cols,
        correct_scroll_offset: hits.correct_scroll_offset,
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
    quadraui::gtk::draw_tooltip(
        cr,
        layout,
        tooltip,
        tooltip_layout,
        line_height,
        padding_x,
        &q_theme(theme),
    );
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
    quadraui::gtk::draw_dialog(
        cr,
        layout,
        ui_layout,
        dialog,
        dialog_layout,
        line_height,
        &q_theme(theme),
    )
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
    quadraui::gtk::draw_context_menu(cr, layout, menu, menu_layout, line_height, &q_theme(theme))
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

    // Save the layout's current font_description before our per-line
    // `set_font_description(ui_font_desc)` calls inside the loop
    // below. Without this, the UI font leaks into subsequent draw
    // calls in the same frame — most visibly: the palette / dialog
    // / context-menu rendering immediately after the hover popup
    // would render in the UI font instead of the editor font (#247
    // second symptom).
    let saved_font = pango_layout.font_description();

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

    // Restore the layout's font_description so subsequent popup /
    // overlay paints in the same frame use the editor font, not the
    // UI font we set above. (#247 second symptom.)
    pango_layout.set_font_description(saved_font.as_ref());

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
