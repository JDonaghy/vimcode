//! Win-GUI (Direct2D / DirectWrite) backend for `quadraui` primitives.
//!
//! Direct2D counterpart to `src/tui_main/quadraui_tui.rs` and
//! `src/gtk/quadraui_gtk.rs`. Currently supports `TreeView` (A.1c) and
//! `StatusBar` (A.6b-win).

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::draw::DrawContext;
use crate::render::Color;

/// Convert a `quadraui::Color` (0-255 RGBA) into vimcode's `render::Color`.
/// Alpha is dropped.
fn qc_to_color(c: quadraui::Color) -> Color {
    Color::from_rgb(c.r, c.g, c.b)
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

/// Draw a `quadraui::TreeView` into `(x, y, w, h)` on `ctx.rt`, using
/// `ctx.format` for monospace text and `ctx.theme` for default colours.
///
/// Row height is `ctx.line_height` (uniform for headers and leaves) to
/// match the pre-migration Win-GUI SC panel cadence. GTK uses `line_height`
/// for headers and `line_height * 1.4` for leaves; Win-GUI keeps rows
/// uniform so monospace columns stay aligned without per-row drift.
///
/// Does not draw a scrollbar. Scrollbars are a later primitive stage.
pub(super) fn draw_tree(
    ctx: &DrawContext,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tree: &quadraui::TreeView,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let lh = ctx.line_height;
    let cw = ctx.char_width;

    let bg = ctx.theme.tab_bar_bg;
    let hdr_bg = ctx.theme.status_bg;
    let hdr_fg = ctx.theme.status_fg;
    let fg = ctx.theme.foreground;
    let dim = ctx.theme.line_number_fg;
    let sel_bg = ctx.theme.fuzzy_selected_bg;

    // Fill tree background.
    unsafe {
        let brush = ctx.solid_brush(bg);
        ctx.rt.FillRectangle(&rect(x, y, w, h), &brush);
    }

    // One visual indent level = 2 char widths (monospace-friendly).
    let indent_px = cw * 2.0;
    let mut y_off = y;
    let y_end = y + h;

    for row in tree.rows.iter().skip(tree.scroll_offset) {
        if y_off >= y_end {
            break;
        }

        let is_header = matches!(row.decoration, quadraui::Decoration::Header);
        let is_selected =
            tree.has_focus && tree.selected_path.as_ref().is_some_and(|p| p == &row.path);

        // Header rows get a distinct background (SC section styling).
        // Ordinary branches render like leaves so folders don't visually
        // separate from sibling files in a recursive tree.
        let (def_fg, row_bg) = if is_selected {
            (hdr_fg, sel_bg)
        } else if is_header {
            (hdr_fg, hdr_bg)
        } else if matches!(row.decoration, quadraui::Decoration::Muted) {
            (dim, bg)
        } else {
            (fg, bg)
        };

        let row_h = lh;

        // Fill row background when it differs from the tree background.
        if row_bg != bg {
            unsafe {
                let brush = ctx.solid_brush(row_bg);
                ctx.rt.FillRectangle(&rect(x, y_off, w, row_h), &brush);
            }
        }

        // Leading offset: small left margin + indent level.
        let mut cursor_x = x + cw * 0.5 + (row.indent as f32) * indent_px;

        // Chevron for branches.
        if let Some(expanded) = row.is_expanded {
            if tree.style.show_chevrons {
                let chevron = if expanded {
                    &tree.style.chevron_expanded
                } else {
                    &tree.style.chevron_collapsed
                };
                ctx.draw_text(chevron, cursor_x, y_off, def_fg);
                cursor_x += ctx.mono_text_width(chevron) + cw * 0.5;
            }
        } else {
            // Leaves: align past the chevron column.
            cursor_x += cw * 1.5;
        }

        // Per-row icons (folder / file-type nerd glyphs) are intentionally
        // skipped on Win-GUI until the backend gains a tree-sized icon
        // IDWriteTextFormat (editor-font-size Nerd Font / Segoe MDL2 with
        // left alignment). Drawing them through `ctx.draw_text` would use
        // the mono editor font (typically Consolas), which has no Nerd
        // Font glyphs — so nerd codepoints render as tofu and the ASCII
        // fallbacks ("+", ".") look like accidental punctuation. Matches
        // the pre-A.2c Win-GUI explorer (chevron + name only).
        let _ = row.icon.as_ref();

        // Reserve space for the right-aligned badge so text truncation
        // doesn't overwrite it.
        let badge_info = row.badge.as_ref().map(|b| {
            let bw = ctx.mono_text_width(&b.text);
            let bfg = b.fg.map(qc_to_color).unwrap_or(dim);
            let bbg = b.bg.map(qc_to_color).unwrap_or(row_bg);
            (b.text.clone(), bw, bfg, bbg)
        });
        let badge_reserve = badge_info
            .as_ref()
            .map(|(_, bw, ..)| *bw + cw)
            .unwrap_or(0.0);
        let text_right_limit = x + w - badge_reserve - cw * 0.5;

        // Text spans — each with its own foreground.
        for span in &row.text.spans {
            if cursor_x >= text_right_limit {
                break;
            }
            let span_fg = if let Some(c) = span.fg {
                qc_to_color(c)
            } else if matches!(row.decoration, quadraui::Decoration::Muted) {
                dim
            } else {
                def_fg
            };
            if let Some(sbg) = span.bg {
                let sbg_c = qc_to_color(sbg);
                let sw = ctx.mono_text_width(&span.text);
                unsafe {
                    let brush = ctx.solid_brush(sbg_c);
                    ctx.rt.FillRectangle(
                        &rect(cursor_x, y_off, sw.min(text_right_limit - cursor_x), row_h),
                        &brush,
                    );
                }
            }
            // Truncate the span if it would overrun the right limit.
            let available = (text_right_limit - cursor_x).max(0.0);
            let max_chars = (available / cw) as usize;
            let drawn: String = span.text.chars().take(max_chars).collect();
            if !drawn.is_empty() {
                ctx.draw_text(&drawn, cursor_x, y_off, span_fg);
                cursor_x += cw * drawn.chars().count() as f32;
            }
        }

        // Badge (right-aligned).
        if let Some((btext, bw, bfg, bbg)) = badge_info {
            let bx = x + w - bw - cw * 0.5;
            if bx > cursor_x - cw * 0.5 {
                if bbg != row_bg {
                    unsafe {
                        let brush = ctx.solid_brush(bbg);
                        ctx.rt.FillRectangle(
                            &rect(bx - cw * 0.25, y_off, bw + cw * 0.5, row_h),
                            &brush,
                        );
                    }
                }
                ctx.draw_text(&btext, bx, y_off, bfg);
            }
        }

        y_off += row_h;
    }
}

// ─── StatusBar (A.6b-win) ────────────────────────────────────────────────────

/// Minimum gap (in character columns) between the rightmost left segment
/// and the leftmost visible right segment. Mirrors the GTK backend's
/// `MIN_GAP_PX = 16` once converted to char widths (~2 mono cells).
const STATUS_BAR_MIN_GAP_CHARS: usize = 2;

/// Draw a `quadraui::StatusBar` into `(x, y, width, ctx.line_height)` on
/// `ctx.rt`, using the editor's monospace font.
///
/// Matches the GTK reference (`src/gtk/quadraui_gtk.rs::draw_status_bar`)
/// in shape: left segments accumulate from `x`; right segments are
/// right-aligned, with low-priority segments dropped from the front per
/// `quadraui::StatusBar::fit_right_start_chars` (#159) so the cursor
/// position is always visible. Backgrounds clip when the two halves
/// would collide.
///
/// Width and hit positions are computed in **character columns** (Win-GUI
/// uses a fixed-width mono font: `pixel_width = chars().count() * char_width`).
/// `win_status_segment_hit_test` consults the same `fit_right_start_chars`
/// policy so clicks on a hidden right segment cannot fire.
///
/// **Bold attribute is ignored for v1.** Win-GUI's text path uses a
/// single non-bold `IDWriteTextFormat`; supporting bold would require a
/// second format and would make hit-test math diverge from the rasterizer
/// (proportional vs char-count widths). The pre-A.6b Win-GUI per-window
/// status bar didn't honour `bold` either.
pub(super) fn draw_status_bar(
    ctx: &DrawContext,
    x: f32,
    y: f32,
    width: f32,
    bar: &quadraui::StatusBar,
) {
    if width <= 0.0 {
        return;
    }
    let cw = ctx.char_width;
    let lh = ctx.line_height;
    let bar_chars = (width / cw).floor() as usize;

    // Background fill: first segment's bg, else theme.background.
    let fill = bar
        .left_segments
        .first()
        .or(bar.right_segments.first())
        .map(|s| qc_to_color(s.bg))
        .unwrap_or(ctx.theme.background);
    unsafe {
        let brush = ctx.solid_brush(fill);
        ctx.rt.FillRectangle(&rect(x, y, width, lh), &brush);
    }

    // ── Left segments — accumulate from x ────────────────────────────────
    let mut cx = x;
    let x_end = x + width;
    for seg in &bar.left_segments {
        if cx >= x_end {
            break;
        }
        let seg_w = seg.text.chars().count() as f32 * cw;
        let visible_w = (x_end - cx).min(seg_w);
        let bg = qc_to_color(seg.bg);
        unsafe {
            let brush = ctx.solid_brush(bg);
            ctx.rt.FillRectangle(&rect(cx, y, visible_w, lh), &brush);
        }
        ctx.draw_text(&seg.text, cx, y, qc_to_color(seg.fg));
        cx += seg_w;
    }

    // ── Right segments — drop low-priority ones until they fit ───────────
    let start_idx = bar.fit_right_start_chars(bar_chars, STATUS_BAR_MIN_GAP_CHARS);
    let visible_widths: Vec<f32> = bar.right_segments[start_idx..]
        .iter()
        .map(|seg| seg.text.chars().count() as f32 * cw)
        .collect();
    let visible_total: f32 = visible_widths.iter().sum();
    let mut rx = (x + width - visible_total).max(cx);
    for (offset, seg) in bar.right_segments[start_idx..].iter().enumerate() {
        let seg_w = visible_widths[offset];
        let bg = qc_to_color(seg.bg);
        unsafe {
            let brush = ctx.solid_brush(bg);
            ctx.rt.FillRectangle(&rect(rx, y, seg_w, lh), &brush);
        }
        ctx.draw_text(&seg.text, rx, y, qc_to_color(seg.fg));
        rx += seg_w;
    }
}
