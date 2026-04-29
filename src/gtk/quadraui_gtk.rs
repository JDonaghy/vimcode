//! GTK backend for `quadraui` primitives.
//!
//! Cairo + Pango equivalent of `src/tui_main/quadraui_tui.rs`. Each
//! `draw_*` function consumes a `quadraui` primitive description and
//! rasterises it onto the provided `cairo::Context`. Currently supports
//! `TreeView` (A.1b), `Form` (A.3c), `ListView` (A.5b), and `Palette`
//! (A.4b).

use super::*;

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
        inactive_fg: q(theme.status_inactive_fg),
        selection_bg: q(theme.selection),
        link_fg: q(theme.md_link),
        completion_bg: q(theme.completion_bg),
        completion_fg: q(theme.completion_fg),
        completion_border: q(theme.completion_border),
        completion_selected_bg: q(theme.completion_selected_bg),
    }
}

// ─── Activity bar / Terminal lift (B5c.5) ───────────────────────────────────
//
// `draw_activity_bar` and `draw_terminal_cells` lifted to
// `quadraui::gtk::*` (taking `quadraui::Theme`). Vimcode call sites
// invoke them directly via `quadraui::gtk::draw_activity_bar` /
// `quadraui::gtk::draw_terminal_cells`, building the
// `quadraui::Theme` via `q_theme()`. The previous in-tree
// `ActivityBarHit` struct has been replaced by
// `quadraui::ActivityBarRowHit` (same field shape).

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
    dialog: &quadraui::Dialog,
    dialog_layout: &quadraui::DialogLayout,
    line_height: f64,
    theme: &Theme,
) -> Vec<(f64, f64, f64, f64)> {
    let ui_font_desc = pango::FontDescription::from_string(&super::draw::UI_FONT());
    quadraui::gtk::draw_dialog(
        cr,
        layout,
        &ui_font_desc,
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
/// geometry stay in sync (#215). Re-exported from `quadraui::gtk` so
/// the rasteriser and the hit-test agree by construction.
pub(super) const RICH_TEXT_POPUP_SB_WIDTH: f64 = quadraui::gtk::RICH_TEXT_POPUP_SB_WIDTH;
/// Pixels of inset between the scrollbar's right edge and the popup's
/// right border. Same role as `RICH_TEXT_POPUP_SB_WIDTH`.
pub(super) const RICH_TEXT_POPUP_SB_INSET: f64 = quadraui::gtk::RICH_TEXT_POPUP_SB_INSET;

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
    let _ = (line_height, char_width);
    let ui_font_desc = pango::FontDescription::from_string(&super::draw::UI_FONT());
    quadraui::gtk::draw_rich_text_popup(
        cr,
        pango_layout,
        &ui_font_desc,
        popup,
        layout,
        &q_theme(theme),
    )
}
