//! GTK backend for `quadraui` primitives — vimcode-side helpers.
//!
//! Most primitive paint paths now go through `Backend::draw_X` via
//! `quadraui::ScreenLayout::draw()` (#446, chunks B+D, #469). What
//! remains here is `q_theme()` — the theme adapter every GTK draw call
//! uses — plus a couple of rich-text-popup constants re-exported for
//! the click-side scrollbar geometry helper.

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
