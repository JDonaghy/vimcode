//! GTK backend for `quadraui` primitives — vimcode-side helpers.
//!
//! Most primitive paint paths now go through `Backend::draw_X` via
//! `quadraui::ScreenLayout::draw()` (#446, chunks B+D). What remains
//! here is `q_theme()` — the theme adapter every GTK draw call uses —
//! plus a couple of rich-text-popup constants re-exported for the
//! click-side scrollbar geometry helper.

use super::*;

pub(super) fn q_theme(theme: &Theme) -> quadraui::Theme {
    render::to_quadraui_theme(theme)
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
///
/// Kept as a shim while the Surface::RichTextPopup migration (#463)
/// is investigated — paint via the trait broke click hit-test.
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
