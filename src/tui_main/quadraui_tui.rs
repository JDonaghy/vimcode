//! TUI backend for `quadraui` primitives.
//!
//! This module provides `draw_*` free functions that render `quadraui`
//! primitives into a ratatui `Buffer`. Over time this file will grow to
//! cover every primitive; currently supports `TreeView` (A.1a),
//! `Form` (A.3a), `Palette` (A.4), and `ListView` (A.5).

use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color as RatatuiColor;

/// Convert a `quadraui::Color` to the ratatui palette colour used by
/// `set_cell`.
fn qc(c: quadraui::Color) -> RatatuiColor {
    RatatuiColor::Rgb(c.r, c.g, c.b)
}

pub(super) fn q_theme(theme: &Theme) -> quadraui::Theme {
    render::to_quadraui_theme(theme)
}

/// Draw a `quadraui::ActivityBar` as a vertical icon strip.
///
/// Top items render from the top edge downward, one row per item.
/// Bottom items render from the bottom edge upward. If the two groups
/// would overlap (area too small), bottom items win and top items are
/// clipped. Each item occupies a single row (no row height beyond 1),
/// and the icon is painted at `area.x + 1` to leave the left column
/// free for the active-item accent bar `▎`.
///
/// Keyboard-selected items get a full-row selection-bg fill; active
/// items get a left-edge accent bar (unless keyboard-selected, where
/// the selection bg takes precedence).
/// Draw a `quadraui::ContextMenu` popup via its D6 `ContextMenuLayout`.
/// Matches the pre-migration chrome: thin box border, selected item
/// rendered inverted, separators as a horizontal dash line, disabled
/// items dimmed. Shortcut (from item.detail) is right-aligned.
pub(super) fn draw_context_menu(
    buf: &mut Buffer,
    menu: &quadraui::ContextMenu,
    layout: &quadraui::ContextMenuLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_context_menu(buf, menu, layout, &q_theme(theme));
}

/// Draw a `quadraui::Completions` popup via the lifted
/// `quadraui::tui::draw_completions` rasteriser (#266). Vimcode's shim
/// role is to map the rich `render::Theme` to the smaller
/// `quadraui::Theme` via `q_theme()` — the body of the rasteriser
/// lives in the quadraui crate.
pub(super) fn draw_completions(
    buf: &mut Buffer,
    completions: &quadraui::Completions,
    layout: &quadraui::CompletionsLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_completions(buf, completions, layout, &q_theme(theme));
}

/// Draw a `quadraui::Dialog` via its D6 `DialogLayout`. Handles the
/// rounded-border chrome the TUI has always drawn and respects
/// horizontal vs. vertical button layout.
///
/// The body text may contain embedded `\n` for multi-line messages —
/// each line is drawn on its own row inside `layout.body_bounds`.
pub(super) fn draw_dialog(
    buf: &mut Buffer,
    dialog: &quadraui::Dialog,
    layout: &quadraui::DialogLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_dialog(buf, dialog, layout, &q_theme(theme));
}

/// Draw a `quadraui::Tooltip` into `layout.bounds` on `buf`. Renders a
/// text box with side-bar borders only (`│` on the first and last
/// columns, no top/bottom border) — matches the visual style used by
/// the LSP hover popup and signature help.
///
/// If `tooltip.styled` is `Some`, a single line of styled spans is
/// rendered (signature help path). Otherwise `tooltip.text` is split
/// on `\n` and each line is rendered plain (hover popup path). Lines
/// that exceed the box width are truncated.
pub(super) fn draw_tooltip(
    buf: &mut Buffer,
    tooltip: &quadraui::Tooltip,
    layout: &quadraui::TooltipLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_tooltip(buf, tooltip, layout, &q_theme(theme));
}

pub(super) fn draw_activity_bar(
    buf: &mut Buffer,
    area: Rect,
    bar: &quadraui::ActivityBar,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bar_bg = rc(theme.tab_bar_bg);
    let icon_fg = rc(theme.activity_bar_fg);
    let accent_fg = bar.active_accent.map(qc).unwrap_or(rc(theme.cursor));
    let sel_bg = bar.selection_bg.map(qc).unwrap_or(rc(theme.cursor));

    // Fill the entire strip with the bar background.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', icon_fg, bar_bg);
        }
    }

    // Per D6: ask the primitive for a layout. ActivityBar uses
    // uniform 1-cell rows in TUI; the layout handles top/bottom
    // pinning and collision (bottom wins).
    let layout = bar.layout(area.width as f32, area.height as f32, 1.0);

    for visible in &layout.visible_items {
        let y = area.y + visible.bounds.y.round() as u16;
        let item = match visible.side {
            quadraui::ActivitySide::Top => &bar.top_items[visible.item_idx],
            quadraui::ActivitySide::Bottom => &bar.bottom_items[visible.item_idx],
        };
        let row_bg = if item.is_keyboard_selected {
            sel_bg
        } else {
            bar_bg
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', icon_fg, row_bg);
        }
        if area.width >= 3 {
            let icon_ch = item.icon.chars().next().unwrap_or('?');
            set_cell(buf, area.x + 1, y, icon_ch, icon_fg, row_bg);
        }
        if item.is_active && !item.is_keyboard_selected {
            set_cell(buf, area.x, y, '\u{258E}', accent_fg, bar_bg); // ▎
        }
    }
}

/// Draw one row of a `quadraui::Terminal` cell grid into a ratatui buffer.
///
/// `start_x` / `screen_row` are the destination cell coordinates;
/// `max_cols` clips the row to the visible width. `theme` supplies
/// fallback colours for find-match overlays — the cell's own `fg` / `bg`
/// win for normal cells and cursor/selection (which use inverted colours).
/// Draw the find/replace overlay by walking `panel.hit_regions` (the
/// shared cross-backend layout source-of-truth from
/// `core::engine::compute_find_replace_hit_regions`). Painting and
/// hit-test then derive from the same `FrHitRegion` list, so column
/// drift bugs (the same class fixed for debug toolbar + breadcrumb)
/// can't recur on this overlay.
///
/// `panel.group_bounds.x/y` is already absolute terminal-screen space
/// (#550 — it's derived from `window_rects`, which TUI now feeds in
/// absolute coordinates like GTK, rather than content-area-relative). The
/// underlying `quadraui::tui::draw_find_replace` rasteriser still takes an
/// `editor_left` translation param (it's TUI-only — GTK never calls this
/// path — and quadraui's signature can't be changed from here); this
/// wrapper always passes `0` so that internal translation is a no-op
/// instead of double-counting the origin already baked into
/// `group_bounds`. The click-hit-test mirroring this paint math is in
/// `mouse.rs`'s find/replace handler — keep the two in sync.
///
/// Painting that the hit-region list doesn't directly cover —
/// borders, the match-count text (a non-clickable status string), and
/// the focused field's cursor + selection — is layered in around the
/// region-driven dispatch.
pub(super) fn draw_find_replace(
    buf: &mut Buffer,
    area: Rect,
    panel: &crate::render::FindReplacePanel,
    theme: &Theme,
) {
    quadraui::tui::draw_find_replace(buf, area, panel, &q_theme(theme), 0);
}

/// Draw a `quadraui::RichTextPopup` into the buffer via the lifted
/// `quadraui::tui::draw_rich_text_popup` rasteriser (#266). Vimcode's
/// shim role is to map the rich `render::Theme` to the smaller
/// `quadraui::Theme` via `q_theme()` — the body of the rasteriser
/// lives in the quadraui crate.
pub(super) fn draw_rich_text_popup(
    buf: &mut Buffer,
    popup: &quadraui::RichTextPopup,
    layout: &quadraui::RichTextPopupLayout,
    theme: &Theme,
) {
    quadraui::tui::draw_rich_text_popup(buf, popup, layout, &q_theme(theme));
}
