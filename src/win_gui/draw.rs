//! Direct2D rendering of `ScreenLayout`.
//!
//! Consumes the platform-agnostic `ScreenLayout` and paints it onto a
//! Direct2D render target using DirectWrite for text.

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;

use crate::core::engine::Notification;
use crate::render::{
    BreadcrumbBar, Color, CursorShape, MenuBarData, RenderedLine, RenderedWindow, ScreenLayout,
    SelectionKind, Theme, MENU_STRUCTURE,
};

use super::{SidebarPanel, WinSidebar};
use crate::icons;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn color_f(c: Color) -> D2D1_COLOR_F {
    let (r, g, b, a) = c.to_f32_rgba();
    D2D1_COLOR_F { r, g, b, a }
}

fn color_f_alpha(c: Color, a: f32) -> D2D1_COLOR_F {
    let (r, g, b, _) = c.to_f32_rgba();
    D2D1_COLOR_F { r, g, b, a }
}

fn rect_f(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

/// Parse a badge color string (hex like "#4ec9b0" or named colors) to Color.
fn parse_badge_color_d2d(color: &str) -> Option<Color> {
    if let Some(hex) = color.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::from_rgb(r, g, b));
        }
    }
    match color {
        "red" => Some(Color::from_rgb(255, 80, 80)),
        "green" => Some(Color::from_rgb(80, 200, 120)),
        "blue" => Some(Color::from_rgb(80, 150, 255)),
        "yellow" => Some(Color::from_rgb(220, 200, 60)),
        "orange" => Some(Color::from_rgb(230, 150, 50)),
        "purple" => Some(Color::from_rgb(180, 100, 220)),
        "cyan" => Some(Color::from_rgb(80, 200, 200)),
        _ => None,
    }
}

// ─── Text measurement ───────────────────────────────────────────────────────

/// Measure the width of a single character in the monospace font.
pub fn measure_char_width(dwrite: &IDWriteFactory, format: &IDWriteTextFormat) -> f32 {
    unsafe {
        let layout: IDWriteTextLayout = dwrite
            .CreateTextLayout(&[b'M' as u16], format, 1000.0, 1000.0)
            .expect("CreateTextLayout for char width");
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics).expect("GetMetrics");
        metrics.width
    }
}

/// Measure the line height of the font.
pub fn measure_line_height(dwrite: &IDWriteFactory, format: &IDWriteTextFormat) -> f32 {
    unsafe {
        let layout: IDWriteTextLayout = dwrite
            .CreateTextLayout(&[b'M' as u16], format, 1000.0, 1000.0)
            .expect("CreateTextLayout for line height");
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics).expect("GetMetrics");
        metrics.height
    }
}

// ─── Main drawing entry point ────────────────────────────────────────────────

pub struct DrawContext<'a> {
    pub rt: &'a ID2D1HwndRenderTarget,
    pub dwrite: &'a IDWriteFactory,
    pub format: &'a IDWriteTextFormat,
    /// Proportional UI font (Segoe UI) for menus and tab labels.
    pub ui_format: &'a IDWriteTextFormat,
    /// Icon font for activity bar icons (larger, Nerd Font glyphs).
    pub icon_format: &'a IDWriteTextFormat,
    pub theme: &'a Theme,
    pub char_width: f32,
    pub line_height: f32,
    /// Left edge of the editor area (sidebar width offset).
    pub editor_left: f32,
    /// X position of the hovered tab (for tooltip placement).
    pub tab_tooltip_x: f32,
    /// Which caption button (0=min, 1=max, 2=close) is hovered, or None.
    pub caption_hover: Option<usize>,
    /// Whether the window is currently maximized (affects the max/restore icon).
    pub is_maximized: bool,
    /// True when icon_format uses "Symbols Nerd Font" (bundled) instead of Segoe MDL2.
    pub nerd_icon_font: bool,
}

impl<'a> DrawContext<'a> {
    /// Draw the full editor frame from a `ScreenLayout`.
    pub fn draw_frame(&self, layout: &ScreenLayout) {
        unsafe {
            // Clear background
            self.rt.Clear(Some(&color_f(self.theme.background)));

            // Draw menu bar
            if let Some(ref menu) = layout.menu_bar {
                self.draw_menu_bar(menu);
            }

            // Draw caption buttons (min/max/close) over the menu bar row
            self.draw_caption_buttons();

            // Draw tab bar(s)
            if let Some(ref split) = layout.editor_group_split {
                for gtb in &split.group_tab_bars {
                    let is_active = gtb.group_id == split.active_group;
                    let has_breadcrumb = layout
                        .breadcrumbs
                        .iter()
                        .any(|bc| bc.group_id == gtb.group_id && !bc.segments.is_empty());
                    self.draw_group_tab_bar(gtb, is_active, has_breadcrumb);
                }
                // Draw group dividers
                self.draw_group_dividers(split);
            } else {
                self.draw_tab_bar(layout);
            }

            // Draw breadcrumbs
            for bc in &layout.breadcrumbs {
                self.draw_breadcrumb_bar(bc);
            }

            // Draw editor windows
            for rw in &layout.windows {
                self.draw_editor_window(rw);
            }

            // Draw status bar (only when not separated above terminal)
            if layout.separated_status_line.is_none() {
                self.draw_status_bar(layout);
            }

            // Draw command line (position depends on whether status is above terminal)
            self.draw_command_line(layout);

            // Draw cursors on all windows (active gets block/bar, inactive gets thin line)
            for rw in &layout.windows {
                self.draw_cursor(rw);
            }

            // Draw tab tooltip (below tab bar, on hover)
            if let Some(ref tooltip) = layout.tab_tooltip {
                self.draw_tab_tooltip(tooltip, layout);
            }

            // Draw completion menu anchored at cursor
            if let Some(ref comp) = layout.completion {
                let active = layout.windows.iter().find(|w| w.is_active);
                self.draw_completion(comp, active);
            }

            // Draw hover popup
            if let Some(ref hover) = layout.hover {
                let active = layout.windows.iter().find(|w| w.is_active);
                self.draw_hover(hover, active);
            }

            // Draw editor hover popup (rich markdown, gh / mouse dwell)
            if let Some(ref eh) = layout.editor_hover {
                let active = layout.windows.iter().find(|w| w.is_active);
                self.draw_editor_hover(eh, active);
            }

            // Draw diff peek popup (inline git hunk preview)
            if let Some(ref peek) = layout.diff_peek {
                let active = layout.windows.iter().find(|w| w.is_active);
                self.draw_diff_peek(peek, active);
            }

            // Draw signature help popup
            if let Some(ref sig) = layout.signature_help {
                let active = layout.windows.iter().find(|w| w.is_active);
                self.draw_signature_help(sig, active);
            }

            // Draw wildmenu (Tab completion bar)
            if let Some(ref wm) = layout.wildmenu {
                self.draw_wildmenu(wm, layout);
            }

            // Draw quickfix panel
            if let Some(ref qf) = layout.quickfix {
                self.draw_quickfix(qf, layout);
            }

            // Draw separated status line (above terminal)
            if let Some(ref status) = layout.separated_status_line {
                self.draw_separated_status_line(status, layout);
            }

            // Draw debug toolbar strip (DAP controls)
            if let Some(ref toolbar) = layout.debug_toolbar {
                self.draw_debug_toolbar(toolbar);
            }

            // Draw terminal panel
            if let Some(ref term) = layout.bottom_tabs.terminal {
                self.draw_terminal(term, layout);
            }

            // Draw panel hover popup (sidebar item hover)
            if let Some(ref ph) = layout.panel_hover {
                self.draw_panel_hover(ph);
            }

            // Draw picker (command palette / fuzzy finder)
            if let Some(ref picker) = layout.picker {
                self.draw_picker(picker);
            }

            // Draw tab switcher
            if let Some(ref ts) = layout.tab_switcher {
                self.draw_tab_switcher(ts);
            }

            // NOTE: context menu, menu dropdown, tab drag overlay, and dialog
            // are drawn separately after sidebar in on_paint for correct z-order.
        }
    }

    // ─── Tab bar ─────────────────────────────────────────────────────────────

    fn draw_tab_bar(&self, layout: &ScreenLayout) {
        // Tab bar starts below the title/menu bar when visible
        let y = if layout.menu_bar.is_some() {
            super::TITLE_BAR_TOP_INSET + self.line_height * super::TITLE_BAR_HEIGHT_MULT
        } else {
            0.0f32
        };
        let h = self.line_height * super::TAB_BAR_HEIGHT_MULT;
        let (width, _) = self.rt_size();
        let x = self.editor_left; // start after sidebar
        let tab_bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, width - x, h), &tab_bg);
        }
        let text_y = y + (h - self.line_height) / 2.0; // vertically center text
                                                       // Reserve space for diff toolbar so tabs don't extend under it.
                                                       // Mirrors the legacy width estimation (3 symbols × 3 cells + 4
                                                       // separator cells + optional change-label) — A.6d-win v2 will
                                                       // unify this into the quadraui::TabBar primitive's right_segments.
        let dt_reserve = layout.diff_toolbar.as_ref().map_or(0.0, |dt| {
            let cw = self.char_width;
            let mut parts_len: usize = 0;
            if let Some(ref label) = dt.change_label {
                parts_len += label.len() + 2;
            }
            parts_len += 3 * 3 + 4;
            parts_len as f32 * cw + cw * 2.0
        });
        // A.6d-win v1: build the quadraui::TabBar primitive (no diff/split/
        // scroll yet — those land in v2) and rasterise the tabs through
        // quadraui_win::draw_tab_bar. The diff toolbar continues to render
        // separately via the legacy path below.
        let bar = crate::render::build_tab_bar_primitive(&layout.tab_bar, false, None, 0, None);
        super::quadraui_win::draw_tab_bar(self, &bar, x, text_y, width - x - dt_reserve, true);
        // Diff toolbar (change nav buttons) at right edge
        if let Some(ref dt) = layout.diff_toolbar {
            self.draw_diff_toolbar_in_tab_bar(dt, x, y, width - x, h);
        }
    }

    fn draw_group_tab_bar(
        &self,
        gtb: &crate::render::GroupTabBar,
        is_active_group: bool,
        has_breadcrumb: bool,
    ) {
        let h = self.line_height * super::TAB_BAR_HEIGHT_MULT;
        let x = gtb.bounds.x as f32;
        // bounds.y is the content area top; the reserved space above includes
        // the tab bar height + optional breadcrumb row. Subtract the full amount.
        let breadcrumb_offset = if has_breadcrumb {
            self.line_height
        } else {
            0.0
        };
        let y = gtb.bounds.y as f32 - h - breadcrumb_offset;
        let w = gtb.bounds.width as f32;

        let bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, w, h), &bg);
        }
        let text_y = y + (h - self.line_height) / 2.0;
        let dt_reserve = gtb.diff_toolbar.as_ref().map_or(0.0, |dt| {
            let cw = self.char_width;
            let mut parts_len: usize = 0;
            if let Some(ref label) = dt.change_label {
                parts_len += label.len() + 2;
            }
            parts_len += 3 * 3 + 4;
            parts_len as f32 * cw + cw * 2.0
        });
        // A.6d-win v1: same migration as the single-group bar above.
        let bar = crate::render::build_tab_bar_primitive(&gtb.tabs, false, None, 0, None);
        super::quadraui_win::draw_tab_bar(self, &bar, x, text_y, w - dt_reserve, is_active_group);
        // Diff toolbar (change nav buttons) at right edge
        if let Some(ref dt) = &gtb.diff_toolbar {
            self.draw_diff_toolbar_in_tab_bar(dt, x, y, w, h);
        }
    }

    fn draw_group_dividers(&self, split: &crate::render::EditorGroupSplitData) {
        let divider_brush = self.solid_brush(self.theme.separator);
        for div in &split.dividers {
            let (x, y, w, h) = match div.direction {
                crate::core::window::SplitDirection::Vertical => (
                    div.position as f32,
                    div.cross_start as f32,
                    2.0,
                    div.cross_size as f32,
                ),
                crate::core::window::SplitDirection::Horizontal => (
                    div.cross_start as f32,
                    div.position as f32,
                    div.cross_size as f32,
                    2.0,
                ),
            };
            unsafe {
                self.rt.FillRectangle(&rect_f(x, y, w, h), &divider_brush);
            }
        }
    }

    // ─── Breadcrumbs ─────────────────────────────────────────────────────────

    fn draw_breadcrumb_bar(&self, bc: &BreadcrumbBar) {
        // Skip drawing when there are no segments — an empty breadcrumb background
        // would cover the tab bar for groups whose active tab has no file path
        // (e.g. scratch buffers, diff views).
        if bc.segments.is_empty() {
            return;
        }
        let lh = self.line_height;
        let cw = self.char_width;
        let bx = bc.bounds.x as f32;
        let by = bc.bounds.y as f32 - lh; // breadcrumb row sits just above the editor window
        let bw = bc.bounds.width as f32;

        // Background
        let bg_brush = self.solid_brush(self.theme.breadcrumb_bg);
        unsafe {
            self.rt.FillRectangle(&rect_f(bx, by, bw, lh), &bg_brush);
        }

        let separator = " \u{203A} "; // " › "
        let mut x = bx + cw; // left padding

        for (i, seg) in bc.segments.iter().enumerate() {
            // Separator before all but the first
            if i > 0 {
                self.draw_text(separator, x, by, self.theme.breadcrumb_fg);
                x += separator.chars().count() as f32 * cw;
            }

            let fg = if seg.is_last {
                self.theme.breadcrumb_active_fg
            } else {
                self.theme.breadcrumb_fg
            };
            self.draw_text(&seg.label, x, by, fg);
            x += seg.label.chars().count() as f32 * cw;
        }
    }

    // ─── Editor window ────────────────────────────────────────────────────────

    fn draw_editor_window(&self, rw: &RenderedWindow) {
        let rx = rw.rect.x as f32;
        let ry = rw.rect.y as f32;
        let rw_w = rw.rect.width as f32;
        let rw_h = rw.rect.height as f32;

        // Clip to window bounds so text doesn't bleed into adjacent areas
        unsafe {
            self.rt.PushAxisAlignedClip(
                &rect_f(rx, ry, rw_w, rw_h),
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }

        // Background fill for the window
        let bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt.FillRectangle(&rect_f(rx, ry, rw_w, rw_h), &bg);
        }

        let gutter_chars = rw.gutter_char_width;
        let gutter_px = gutter_chars as f32 * self.char_width;
        let h_scroll_px = rw.scroll_left as f32 * self.char_width;
        let text_x = rx + gutter_px - h_scroll_px;

        for (row_idx, line) in rw.lines.iter().enumerate() {
            let line_y = ry + (row_idx as f32) * self.line_height;

            // Current line highlight (full width, not affected by scroll)
            if line.is_current_line && rw.cursorline && rw.is_active {
                let cl_bg = self.solid_brush(self.theme.cursorline_bg);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(
                            rx + gutter_px,
                            line_y,
                            rw.rect.width as f32 - gutter_px,
                            self.line_height,
                        ),
                        &cl_bg,
                    );
                }
            }

            // Gutter (line numbers — not affected by horizontal scroll)
            self.draw_text(
                &line.gutter_text,
                rx,
                line_y,
                if line.is_current_line && rw.is_active {
                    self.theme.foreground
                } else {
                    self.theme.line_number_fg
                },
            );

            // Git gutter indicator (not affected by horizontal scroll)
            if let Some(ref diff) = line.git_diff {
                let gutter_color = match diff {
                    crate::core::GitLineStatus::Added => self.theme.git_added,
                    crate::core::GitLineStatus::Modified => self.theme.git_modified,
                    crate::core::GitLineStatus::Deleted => self.theme.git_deleted,
                };
                let indicator_brush = self.solid_brush(gutter_color);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(
                            rx + gutter_px - self.char_width * 0.3,
                            line_y,
                            3.0,
                            self.line_height,
                        ),
                        &indicator_brush,
                    );
                }
            }
        }

        // Clip text area (excludes gutter) so scrolled text doesn't bleed over line numbers
        let text_clip_x = rx + gutter_px;
        let text_clip_w = rw_w - gutter_px;
        let text_clip_h = rw_h;
        unsafe {
            self.rt.PushAxisAlignedClip(
                &rect_f(text_clip_x, ry, text_clip_w, text_clip_h),
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }

        for (row_idx, line) in rw.lines.iter().enumerate() {
            let line_y = ry + (row_idx as f32) * self.line_height;

            // Selection highlight (scrolled with text)
            if let Some(ref sel) = rw.selection {
                self.draw_selection_for_line(rw, line, row_idx, sel, text_x, line_y);
            }

            // Syntax-highlighted text spans (scrolled with text)
            self.draw_styled_line(line, text_x, line_y);

            // Diagnostic underlines (scrolled with text)
            for diag in &line.diagnostics {
                self.draw_diagnostic_underline(diag, text_x, line_y);
            }

            // Indent guides (scrolled with text)
            for &guide_col in &line.indent_guides {
                let gx = text_x + guide_col as f32 * self.char_width;
                let guide_brush = self.solid_brush_alpha(self.theme.line_number_fg, 0.3);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(gx, line_y, 1.0, self.line_height), &guide_brush);
                }
            }

            // Color columns (scrolled with text)
            if !line.colorcolumns.is_empty() {
                let cc_brush = self.solid_brush(self.theme.colorcolumn_bg);
                for &cc_col in &line.colorcolumns {
                    let cx = text_x + cc_col as f32 * self.char_width;
                    unsafe {
                        self.rt.FillRectangle(
                            &rect_f(cx, line_y, self.char_width, self.line_height),
                            &cc_brush,
                        );
                    }
                }
            }

            // Ghost text (scrolled with text)
            if let Some(ref ghost) = line.ghost_suffix {
                let text_len = line.raw_text.trim_end_matches('\n').chars().count();
                let gx = text_x + text_len as f32 * self.char_width;
                self.draw_text(ghost, gx, line_y, self.theme.line_number_fg);
            }

            // Inline annotation (scrolled with text)
            if let Some(ref ann) = line.annotation {
                let text_len = line.raw_text.trim_end_matches('\n').chars().count();
                let ax = text_x + (text_len as f32 + 2.0) * self.char_width;
                self.draw_text(ann, ax, line_y, self.theme.line_number_fg);
            }
        }

        // Pop text area clip
        unsafe {
            self.rt.PopAxisAlignedClip();
        }

        // Scrollbar (thin track on right edge)
        if rw.total_lines > 0 {
            let sb_width = super::SCROLLBAR_WIDTH;
            let sb_x = rx + rw.rect.width as f32 - sb_width;
            let editor_h = rw.rect.height as f32
                - if rw.status_line.is_some() {
                    self.line_height
                } else {
                    0.0
                };
            let viewport_lines = rw.lines.len();
            if rw.total_lines > viewport_lines {
                // Track background
                let track_brush = self.solid_brush(self.theme.scrollbar_track);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(sb_x, ry, sb_width, editor_h), &track_brush);
                }
                // Thumb
                let ratio = viewport_lines as f32 / rw.total_lines as f32;
                let thumb_h = (editor_h * ratio).max(20.0);
                let scroll_ratio =
                    rw.scroll_top as f32 / (rw.total_lines.saturating_sub(viewport_lines)) as f32;
                let thumb_y = ry + scroll_ratio * (editor_h - thumb_h);
                let thumb_brush = self.solid_brush(self.theme.scrollbar_thumb);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(sb_x, thumb_y, sb_width, thumb_h), &thumb_brush);
                }
            }
        }

        // Horizontal scrollbar (thin track at bottom of editor area)
        {
            let sb_height = super::SCROLLBAR_WIDTH;
            let gutter_px = rw.gutter_char_width as f32 * self.char_width;
            let has_v_scrollbar = rw.total_lines > rw.lines.len();
            let v_sb_w = if has_v_scrollbar {
                super::SCROLLBAR_WIDTH
            } else {
                0.0
            };
            let track_x = rx + gutter_px;
            let track_w = rw.rect.width as f32 - gutter_px - v_sb_w;
            let viewport_cols = (track_w / self.char_width).floor() as usize;
            if rw.max_col > viewport_cols && track_w > 0.0 {
                let editor_h = rw.rect.height as f32
                    - if rw.status_line.is_some() {
                        self.line_height
                    } else {
                        0.0
                    };
                let sb_y = ry + editor_h - sb_height;
                // Track background
                let track_brush = self.solid_brush(self.theme.scrollbar_track);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(track_x, sb_y, track_w, sb_height), &track_brush);
                }
                // Thumb
                let ratio = viewport_cols as f32 / rw.max_col as f32;
                let thumb_w = (track_w * ratio).max(20.0);
                let max_scroll = rw.max_col.saturating_sub(viewport_cols);
                let scroll_ratio = if max_scroll > 0 {
                    rw.scroll_left as f32 / max_scroll as f32
                } else {
                    0.0
                };
                let thumb_x = track_x + scroll_ratio * (track_w - thumb_w);
                let thumb_brush = self.solid_brush(self.theme.scrollbar_thumb);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(thumb_x, sb_y, thumb_w, sb_height), &thumb_brush);
                }
            }
        }

        // Per-window status line — A.6b-win: route through quadraui::StatusBar.
        // Right-segment fit policy (#159) drops low-priority segments when the
        // bar is narrow; `win_status_segment_hit_test` applies the same policy
        // so clicks stay aligned with what's actually drawn.
        if let Some(ref status) = rw.status_line {
            let status_y = ry + rw.rect.height as f32 - self.line_height;
            let bar_w = rw.rect.width as f32;
            let bar = crate::render::window_status_line_to_status_bar(
                status,
                quadraui::WidgetId::new("status:window"),
            );
            super::quadraui_win::draw_status_bar(self, rx, status_y, bar_w, &bar);
        }

        // Pop editor window clip
        unsafe {
            self.rt.PopAxisAlignedClip();
        }
    }

    fn draw_styled_line(&self, line: &RenderedLine, x: f32, y: f32) {
        if line.spans.is_empty() {
            // No syntax: draw raw text in default color
            self.draw_text(&line.raw_text, x, y, self.theme.foreground);
            return;
        }

        let raw = &line.raw_text;
        let raw_len = raw.len();
        let mut cursor_byte = 0usize;

        for span in &line.spans {
            let span_start = span.start_byte.min(raw_len);
            let span_end = span.end_byte.min(raw_len);

            // Draw gap text (between previous span end and this span start) in default color
            if cursor_byte < span_start {
                let gap_text = safe_slice(raw, cursor_byte, span_start);
                if !gap_text.is_empty() {
                    let char_offset = raw[..cursor_byte.min(raw_len)].chars().count();
                    let gx = x + char_offset as f32 * self.char_width;
                    self.draw_text(gap_text, gx, y, self.theme.foreground);
                }
            }

            // Draw span text in its styled color
            let span_text = safe_slice(raw, span_start, span_end);
            if !span_text.is_empty() {
                let char_offset = raw[..span_start].chars().count();
                let sx = x + char_offset as f32 * self.char_width;
                self.draw_text(span_text, sx, y, span.style.fg);
            }

            if span_end > cursor_byte {
                cursor_byte = span_end;
            }
        }

        // Draw any trailing text after the last span
        if cursor_byte < raw_len {
            let tail_text = safe_slice(raw, cursor_byte, raw_len);
            if !tail_text.is_empty() {
                let char_offset = raw[..cursor_byte].chars().count();
                let tx = x + char_offset as f32 * self.char_width;
                self.draw_text(tail_text, tx, y, self.theme.foreground);
            }
        }
    }

    fn draw_selection_for_line(
        &self,
        _rw: &RenderedWindow,
        line: &RenderedLine,
        _row_idx: usize,
        sel: &crate::render::SelectionRange,
        text_x: f32,
        line_y: f32,
    ) {
        let buf_line = line.line_idx;

        if buf_line < sel.start_line || buf_line > sel.end_line {
            return;
        }

        let line_len = line.raw_text.chars().count();
        let (sel_start, sel_end) = match sel.kind {
            SelectionKind::Line => (0, line_len),
            SelectionKind::Block => {
                let sc = sel.start_col.min(sel.end_col);
                let ec = sel.start_col.max(sel.end_col) + 1;
                (sc.min(line_len), ec.min(line_len))
            }
            SelectionKind::Char => {
                let sc = if buf_line == sel.start_line {
                    sel.start_col
                } else {
                    0
                };
                let ec = if buf_line == sel.end_line {
                    sel.end_col + 1
                } else {
                    line_len
                };
                (sc.min(line_len), ec.min(line_len))
            }
        };

        if sel_start >= sel_end {
            return;
        }

        let sel_brush =
            self.solid_brush_alpha(self.theme.selection, self.theme.selection_alpha as f32);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(
                    text_x + sel_start as f32 * self.char_width,
                    line_y,
                    (sel_end - sel_start) as f32 * self.char_width,
                    self.line_height,
                ),
                &sel_brush,
            );
        }
    }

    // ─── Cursor ──────────────────────────────────────────────────────────────

    fn draw_cursor(&self, rw: &RenderedWindow) {
        let Some((pos, shape)) = &rw.cursor else {
            return;
        };
        let rx = rw.rect.x as f32;
        let ry = rw.rect.y as f32;
        let gutter_px = rw.gutter_char_width as f32 * self.char_width;
        let h_scroll_px = rw.scroll_left as f32 * self.char_width;
        let cx = rx + gutter_px + pos.col as f32 * self.char_width - h_scroll_px;
        let cy = ry + pos.view_line as f32 * self.line_height;
        let cursor_brush = self.solid_brush(self.theme.cursor);

        unsafe {
            match shape {
                CursorShape::Block => {
                    let block_brush = self.solid_brush_alpha(self.theme.cursor, 0.7);
                    self.rt.FillRectangle(
                        &rect_f(cx, cy, self.char_width, self.line_height),
                        &block_brush,
                    );
                }
                CursorShape::Bar => {
                    self.rt
                        .FillRectangle(&rect_f(cx, cy, 2.0, self.line_height), &cursor_brush);
                }
                CursorShape::Underline => {
                    self.rt.FillRectangle(
                        &rect_f(cx, cy + self.line_height - 2.0, self.char_width, 2.0),
                        &cursor_brush,
                    );
                }
            }
        }
    }

    // ─── Status bar ──────────────────────────────────────────────────────────

    fn draw_status_bar(&self, layout: &ScreenLayout) {
        // Skip global status bar when per-window status lines are active (status_left is empty)
        if layout.status_left.is_empty() && layout.status_right.is_empty() {
            return;
        }
        let (width, height) = self.rt_size();
        let x0 = self.editor_left;
        let y = height - 2.0 * self.line_height;
        let bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(x0, y, width - x0, self.line_height), &bg);
        }
        self.draw_text(
            &layout.status_left,
            x0 + self.char_width * 0.5,
            y,
            self.theme.status_fg,
        );

        let right_w = layout.status_right.chars().count() as f32 * self.char_width;
        self.draw_text(
            &layout.status_right,
            width - right_w - self.char_width * 0.5,
            y,
            self.theme.status_fg,
        );
    }

    // ─── Separated status line (above terminal) ────────────────────────────

    fn draw_separated_status_line(
        &self,
        status: &crate::render::WindowStatusLine,
        _layout: &ScreenLayout,
    ) {
        let (width, height) = self.rt_size();
        let x0 = self.editor_left;
        // The separated status sits just above the terminal panel.
        // Layout: editor | sep_status | terminal | status_bar | cmd_line
        let terminal_px = _layout
            .bottom_tabs
            .terminal
            .as_ref()
            .map(|t| (t.content_rows as f32 + 2.0) * self.line_height)
            .unwrap_or(0.0);
        let sep_y = height - terminal_px - 2.0 * self.line_height;
        let bar_width = width - x0;
        // A.6b-win: route through quadraui::StatusBar so #159 fit policy +
        // per-segment background fills + future bold/click-region work all
        // share the same code path with the per-window status bar.
        let bar = crate::render::window_status_line_to_status_bar(
            status,
            quadraui::WidgetId::new("status:separated"),
        );
        super::quadraui_win::draw_status_bar(self, x0, sep_y, bar_width, &bar);
    }

    // ─── Command line ────────────────────────────────────────────────────────

    fn draw_command_line(&self, layout: &ScreenLayout) {
        let (width, height) = self.rt_size();
        let lh = self.line_height;
        // Leave a small margin at the bottom so descenders (g, y, p, :) aren't clipped
        let margin = (lh * 0.15).max(2.0);
        let y = if layout.separated_status_line.is_some() {
            let terminal_px = layout
                .bottom_tabs
                .terminal
                .as_ref()
                .map(|t| (t.content_rows as f32 + 2.0) * lh)
                .unwrap_or(0.0);
            height - terminal_px - lh - margin
        } else {
            height - lh - margin
        };
        // Fill from editor_left to right edge; sidebar area is covered by its own background
        let bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(self.editor_left, y, width - self.editor_left, lh + margin),
                &bg,
            );
        }
        let cmd = &layout.command;
        self.draw_text(
            &cmd.text,
            self.editor_left + self.char_width * 0.5,
            y,
            self.theme.foreground,
        );
    }

    // ─── Completion popup ────────────────────────────────────────────────────

    fn draw_completion(
        &self,
        comp: &crate::render::CompletionMenu,
        active_window: Option<&RenderedWindow>,
    ) {
        let bg = self.solid_brush(self.theme.completion_bg);
        let sel_bg = self.solid_brush(self.theme.selection);
        let border_brush = self.solid_brush(self.theme.separator);
        let popup_w = (comp.max_width as f32 + 4.0) * self.char_width;
        let max_visible = comp.candidates.len().min(10);
        let popup_h = max_visible as f32 * self.line_height;

        // Position below the cursor in the active window
        let (x, y) = if let Some(rw) = active_window {
            if let Some((pos, _)) = &rw.cursor {
                let gutter_px = rw.gutter_char_width as f32 * self.char_width;
                let cx = rw.rect.x as f32 + gutter_px + pos.col as f32 * self.char_width;
                let cy = rw.rect.y as f32 + (pos.view_line as f32 + 1.0) * self.line_height;
                // Clamp to window bounds
                let (rt_w, rt_h) = self.rt_size();
                let fx = cx.min(rt_w - popup_w - 2.0).max(0.0);
                let fy = if cy + popup_h > rt_h - 2.0 * self.line_height {
                    // Show above cursor instead
                    rw.rect.y as f32 + pos.view_line as f32 * self.line_height - popup_h
                } else {
                    cy
                };
                (fx, fy.max(0.0))
            } else {
                (4.0 * self.char_width, 2.0 * self.line_height)
            }
        } else {
            (4.0 * self.char_width, 2.0 * self.line_height)
        };

        unsafe {
            // Background + border
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border_brush, 1.0, None);
        }

        for (i, candidate) in comp.candidates.iter().take(max_visible).enumerate() {
            let iy = y + i as f32 * self.line_height;
            let is_selected = i == comp.selected_idx;
            if is_selected {
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(x, iy, popup_w, self.line_height), &sel_bg);
                }
            }
            self.draw_text(candidate, x + self.char_width, iy, self.theme.foreground);
        }
    }

    fn draw_hover(
        &self,
        hover: &crate::render::HoverPopup,
        active_window: Option<&RenderedWindow>,
    ) {
        let bg = self.solid_brush(self.theme.completion_bg);
        let border_brush = self.solid_brush(self.theme.separator);

        // Compute popup size from text content
        let lines: Vec<&str> = hover.text.lines().collect();
        let max_line_chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(20);
        let popup_w = (max_line_chars as f32 + 4.0) * self.char_width;
        let popup_h = lines.len() as f32 * self.line_height + 4.0;
        let max_popup_w = {
            let (rt_w, _) = self.rt_size();
            rt_w * 0.6
        };
        let popup_w = popup_w.min(max_popup_w);

        // Position above the anchor in the active window
        let (x, y) = if let Some(rw) = active_window {
            let gutter_px = rw.gutter_char_width as f32 * self.char_width;
            let scroll_top = rw.lines.first().map_or(0, |l| l.line_idx);
            let view_line = hover.anchor_line.saturating_sub(scroll_top);
            let cx = rw.rect.x as f32 + gutter_px + hover.anchor_col as f32 * self.char_width;
            let cy = rw.rect.y as f32 + view_line as f32 * self.line_height;
            // Prefer above cursor
            let fy = if cy >= popup_h {
                cy - popup_h
            } else {
                cy + self.line_height
            };
            let (rt_w, _) = self.rt_size();
            (cx.min(rt_w - popup_w - 2.0).max(0.0), fy.max(0.0))
        } else {
            (0.0, 0.0)
        };

        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border_brush, 1.0, None);
        }

        for (i, line_text) in lines.iter().enumerate() {
            self.draw_text(
                line_text,
                x + self.char_width,
                y + 2.0 + i as f32 * self.line_height,
                self.theme.foreground,
            );
        }
    }

    pub fn draw_tab_drag_overlay(
        &self,
        layout: &ScreenLayout,
        engine: &crate::core::engine::Engine,
    ) {
        use crate::core::window::{DropZone, SplitDirection};

        let drag = match &engine.tab_drag {
            Some(td) => td,
            None => return,
        };

        let zone = &engine.tab_drop_zone;
        let lh = self.line_height;
        let tab_h = lh * super::TAB_BAR_HEIGHT_MULT;

        // Blue overlay for drop zone
        let overlay_color = Color::from_rgb(30, 60, 120);
        let overlay_brush = self.solid_brush(overlay_color);

        // Determine the highlight rectangle based on drop zone
        if let Some(ref split) = layout.editor_group_split {
            for gtb in &split.group_tab_bars {
                let bx = gtb.bounds.x as f32;
                let by = gtb.bounds.y as f32;
                let bw = gtb.bounds.width as f32;
                let bh = gtb.bounds.height as f32;
                // Account for breadcrumb row when positioning tab bar overlay
                let has_bc = layout
                    .breadcrumbs
                    .iter()
                    .any(|bc| bc.group_id == gtb.group_id && !bc.segments.is_empty());
                let bc_offset = if has_bc { lh } else { 0.0 };
                let tab_bar_y = by - tab_h - bc_offset;

                match zone {
                    DropZone::Center(gid) if *gid == gtb.group_id => unsafe {
                        self.rt.FillRectangle(
                            &rect_f(bx, tab_bar_y, bw, bh + tab_h + bc_offset),
                            &overlay_brush,
                        );
                    },
                    DropZone::Split(gid, dir, new_first) if *gid == gtb.group_id => {
                        let (rx, ry, rw, rh) = match (dir, new_first) {
                            (SplitDirection::Vertical, true) => (bx, by, bw / 2.0, bh),
                            (SplitDirection::Vertical, false) => (bx + bw / 2.0, by, bw / 2.0, bh),
                            (SplitDirection::Horizontal, true) => (bx, by, bw, bh / 2.0),
                            (SplitDirection::Horizontal, false) => {
                                (bx, by + bh / 2.0, bw, bh / 2.0)
                            }
                        };
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(rx, ry, rw, rh), &overlay_brush);
                        }
                    }
                    DropZone::TabReorder(gid, idx) if *gid == gtb.group_id => {
                        // Highlight the tab bar
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(bx, tab_bar_y, bw, tab_h), &overlay_brush);
                        }
                        // Draw insertion bar
                        let cw = self.char_width;
                        let mut x = bx;
                        for (i, tab) in gtb.tabs.iter().enumerate() {
                            if i == *idx {
                                break;
                            }
                            x += (tab.name.chars().count() as f32 + 3.0) * cw;
                        }
                        let bar_brush = self.solid_brush(self.theme.cursor);
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(x - 1.0, tab_bar_y, 2.0, tab_h), &bar_brush);
                        }
                    }
                    _ => {}
                }
            }
        } else {
            // Single group — handle TabReorder insertion bar + Split/Center overlay
            match zone {
                DropZone::TabReorder(_, idx) => {
                    let tab_y = if layout.menu_bar.is_some() {
                        super::TITLE_BAR_TOP_INSET + lh * super::TITLE_BAR_HEIGHT_MULT
                    } else {
                        0.0
                    };
                    let cw = self.char_width;
                    let mut x = self.editor_left;
                    for (i, tab) in layout.tab_bar.iter().enumerate() {
                        if i == *idx {
                            break;
                        }
                        x += (tab.name.chars().count() as f32 + 3.0) * cw;
                    }
                    let bar_brush = self.solid_brush(self.theme.cursor);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(x - 1.0, tab_y, 2.0, tab_h), &bar_brush);
                    }
                }
                DropZone::Split(_, dir, new_first) => {
                    // Use the first window rect as the editor area
                    if let Some(rw) = layout.windows.first() {
                        let bx = rw.rect.x as f32;
                        let by = rw.rect.y as f32;
                        let bw = rw.rect.width as f32;
                        let bh = rw.rect.height as f32;
                        let (rx, ry, rw_f, rh_f) = match (dir, new_first) {
                            (SplitDirection::Vertical, true) => (bx, by, bw / 2.0, bh),
                            (SplitDirection::Vertical, false) => (bx + bw / 2.0, by, bw / 2.0, bh),
                            (SplitDirection::Horizontal, true) => (bx, by, bw, bh / 2.0),
                            (SplitDirection::Horizontal, false) => {
                                (bx, by + bh / 2.0, bw, bh / 2.0)
                            }
                        };
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(rx, ry, rw_f, rh_f), &overlay_brush);
                        }
                    }
                }
                DropZone::Center(_) => {
                    // Highlight entire editor area
                    if let Some(rw) = layout.windows.first() {
                        let tab_y = if layout.menu_bar.is_some() {
                            super::TITLE_BAR_TOP_INSET + lh * super::TITLE_BAR_HEIGHT_MULT
                        } else {
                            0.0
                        };
                        unsafe {
                            self.rt.FillRectangle(
                                &rect_f(
                                    rw.rect.x as f32,
                                    tab_y,
                                    rw.rect.width as f32,
                                    rw.rect.height as f32 + rw.rect.y as f32 - tab_y,
                                ),
                                &overlay_brush,
                            );
                        }
                    }
                }
                DropZone::None => {}
            }
        }

        // Ghost label near mouse cursor
        if let Some((mx, my)) = engine.tab_drag_mouse {
            let mx = mx as f32;
            let my = my as f32;
            let label = &drag.tab_name;
            let ghost_bg = self.solid_brush(Color::from_rgb(60, 60, 60));
            let ghost_w = (label.chars().count() as f32 + 2.0) * self.char_width;
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(mx + 10.0, my - lh / 2.0, ghost_w, lh), &ghost_bg);
            }
            self.draw_ui_text(
                label,
                mx + 10.0 + self.char_width,
                my - lh / 2.0,
                lh,
                self.theme.foreground,
            );
        }
    }

    pub(super) fn draw_dialog(&self, dialog: &crate::render::DialogPanel) {
        let (rt_w, rt_h) = self.rt_size();

        // Semi-transparent overlay
        let overlay = self.solid_brush_alpha(self.theme.background, 0.6);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, 0.0, rt_w, rt_h), &overlay);
        }

        // Dialog box — auto-size width to fit buttons and body text
        let cw = self.char_width;
        let btn_total_w: f32 = dialog
            .buttons
            .iter()
            .map(|(label, _)| (label.chars().count() as f32 + 2.0) * cw + cw)
            .sum::<f32>()
            + cw * 2.0; // padding
        let body_max_w = dialog
            .body
            .iter()
            .map(|line| line.chars().count() as f32 * cw + cw * 4.0)
            .fold(0.0f32, f32::max);
        let title_w = dialog.title.chars().count() as f32 * cw + cw * 4.0;
        let content_w = btn_total_w.max(body_max_w).max(title_w);
        let dialog_w = content_w.max(300.0).min(rt_w - 40.0);
        let dialog_h = (dialog.body.len() as f32 + 3.0) * self.line_height + 20.0;
        let dx = (rt_w - dialog_w) / 2.0;
        let dy = (rt_h - dialog_h) / 2.0;

        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(dx, dy, dialog_w, dialog_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(dx, dy, dialog_w, dialog_h), &border, 1.0, None);
        }

        // Title
        self.draw_text(
            &dialog.title,
            dx + self.char_width,
            dy + 4.0,
            self.theme.foreground,
        );

        // Body lines
        for (i, line_text) in dialog.body.iter().enumerate() {
            self.draw_text(
                line_text,
                dx + self.char_width,
                dy + (i as f32 + 1.5) * self.line_height,
                self.theme.foreground,
            );
        }

        // Buttons
        let btn_y = dy + dialog_h - self.line_height - 8.0;
        let mut bx = dx + dialog_w - self.char_width;
        for (label, is_selected) in dialog.buttons.iter().rev() {
            let btn_w = (label.chars().count() as f32 + 2.0) * self.char_width;
            bx -= btn_w;
            let btn_bg = if *is_selected {
                self.solid_brush(self.theme.selection)
            } else {
                self.solid_brush(self.theme.status_bg)
            };
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(bx, btn_y, btn_w, self.line_height), &btn_bg);
            }
            self.draw_text(label, bx + self.char_width, btn_y, self.theme.foreground);
            bx -= self.char_width;
        }
    }

    /// Draw the find/replace overlay (VSCode layout).
    /// Returns cached geometry for click handling.
    pub(super) fn draw_find_replace(
        &self,
        panel: &crate::render::FindReplacePanel,
    ) -> super::FindReplaceRect {
        let cw = self.char_width;
        let lh = self.line_height;
        let pad = 4.0;
        let btn_s = lh; // square button size

        // Layout: [chevron] [input 25ch] [Aa][ab][.*] [count] [↑][↓][≡][×]
        let input_w = 25.0 * cw;
        let toggles_w = 3.0 * (btn_s + 2.0); // Aa, ab, .*
        let info_w = 9.0 * cw; // "99 of 999"
        let nav_w = 4.0 * (btn_s + 2.0); // ↑ ↓ ≡ ×
        let chevron_w = cw * 1.5;
        let panel_w = chevron_w + input_w + pad + toggles_w + info_w + nav_w + pad;

        let row_count = if panel.show_replace { 2.0 } else { 1.0 };
        let panel_h = lh * row_count + pad * (row_count + 1.0);

        // Position: top-right of active editor group
        let gb = &panel.group_bounds;
        let group_right = gb.x as f32 + gb.width as f32;
        let x = (group_right - panel_w - 4.0).max(gb.x as f32);
        let y = gb.y as f32 + pad;

        // Background & border
        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, panel_w, panel_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, panel_w, panel_h), &border, 1.0, None);
        }

        let accent = self.solid_brush(self.theme.tab_active_accent);
        let input_bg = self.solid_brush(self.theme.background);
        let dim_fg = self.theme.foreground;
        let btn_bg = self.solid_brush(self.theme.status_bg);

        // --- Find row ---
        let row_y = y + pad;
        let chevron = if panel.show_replace { "▼" } else { "▶" };
        self.draw_text(chevron, x + 2.0, row_y, dim_fg);
        let input_x = x + chevron_w;

        // Find input
        unsafe {
            self.rt
                .FillRectangle(&rect_f(input_x, row_y, input_w, lh), &input_bg);
            self.rt
                .DrawRectangle(&rect_f(input_x, row_y, input_w, lh), &border, 0.5, None);
        }
        self.draw_text(&panel.query, input_x + 2.0, row_y, dim_fg);
        if panel.focus == 0 {
            // Draw selection highlight if any
            if let Some(anchor) = panel.sel_anchor {
                let sel_start = anchor.min(panel.cursor);
                let sel_end = anchor.max(panel.cursor);
                if sel_start != sel_end {
                    let sx = input_x + 2.0 + sel_start as f32 * cw;
                    let sw = (sel_end - sel_start) as f32 * cw;
                    let sel_brush = self.solid_brush_alpha(self.theme.selection, 0.5);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(sx, row_y, sw, lh), &sel_brush);
                    }
                }
            }
            // Cursor
            let cursor_x = input_x + 2.0 + panel.cursor as f32 * cw;
            let cursor_brush = self.solid_brush(dim_fg);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(cursor_x, row_y + 1.0, 2.0, lh - 2.0), &cursor_brush);
            }
        }

        // Toggle buttons: [Aa] [ab] [.*]
        let mut bx = input_x + input_w + pad;
        for (label, active) in [
            ("Aa", panel.case_sensitive),
            ("ab", panel.whole_word),
            (".*", panel.use_regex),
        ] {
            let tw = label.chars().count() as f32 * cw + 4.0;
            if active {
                unsafe {
                    self.rt.FillRectangle(&rect_f(bx, row_y, tw, lh), &accent);
                }
                self.draw_text(label, bx + 2.0, row_y, self.theme.background);
            } else {
                unsafe {
                    self.rt
                        .DrawRectangle(&rect_f(bx, row_y, tw, lh), &border, 0.5, None);
                }
                self.draw_text(label, bx + 2.0, row_y, dim_fg);
            }
            bx += tw + 2.0;
        }

        // Match count (fixed-width slot)
        bx += 2.0;
        let info_text = &panel.match_info;
        self.draw_text(info_text, bx, row_y, dim_fg);
        bx += info_w;

        // Nav buttons: [↑] [↓] [≡] [×]
        let mut nav_x = [0.0f32; 4];
        let nav_labels = [
            "\u{2191}",
            "\u{2193}",
            crate::icons::FIND_IN_SEL.s(),
            crate::icons::FIND_CLOSE.s(),
        ];
        let nav_active = [false, false, panel.in_selection, false];
        for (i, label) in nav_labels.iter().enumerate() {
            nav_x[i] = bx;
            if nav_active[i] {
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(bx, row_y, btn_s, lh), &accent);
                }
                self.draw_text(label, bx + (btn_s - cw) * 0.5, row_y, self.theme.background);
            } else {
                unsafe {
                    self.rt
                        .DrawRectangle(&rect_f(bx, row_y, btn_s, lh), &border, 0.5, None);
                }
                self.draw_text(label, bx + (btn_s - cw) * 0.5, row_y, dim_fg);
            }
            bx += btn_s + 2.0;
        }

        // --- Replace row ---
        let mut rep_btns = [0.0f32; 4];
        if panel.show_replace {
            let rep_y = row_y + lh + pad;

            // Replace input (same position as find input)
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(input_x, rep_y, input_w, lh), &input_bg);
                self.rt
                    .DrawRectangle(&rect_f(input_x, rep_y, input_w, lh), &border, 0.5, None);
            }
            self.draw_text(&panel.replacement, input_x + 2.0, rep_y, dim_fg);
            if panel.focus == 1 {
                // Selection highlight
                if let Some(anchor) = panel.sel_anchor {
                    let sel_start = anchor.min(panel.cursor);
                    let sel_end = anchor.max(panel.cursor);
                    if sel_start != sel_end {
                        let sx = input_x + 2.0 + sel_start as f32 * cw;
                        let sw = (sel_end - sel_start) as f32 * cw;
                        let sel_brush = self.solid_brush_alpha(self.theme.selection, 0.5);
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(sx, rep_y, sw, lh), &sel_brush);
                        }
                    }
                }
                // Cursor
                let cursor_x = input_x + 2.0 + panel.cursor as f32 * cw;
                let cursor_brush = self.solid_brush(dim_fg);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(cursor_x, rep_y + 1.0, 2.0, lh - 2.0),
                        &cursor_brush,
                    );
                }
            }

            // Replace row buttons: [AB] [replace] [replace_all]
            let mut rbx = input_x + input_w + pad;
            let ab_w = 2.0 * cw + 4.0;
            let ab_x = rbx;
            if panel.preserve_case {
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(rbx, rep_y, ab_w, lh), &accent);
                }
                self.draw_text("AB", rbx + 2.0, rep_y, self.theme.background);
            } else {
                unsafe {
                    self.rt
                        .DrawRectangle(&rect_f(rbx, rep_y, ab_w, lh), &border, 0.5, None);
                }
                self.draw_text("AB", rbx + 2.0, rep_y, dim_fg);
            }
            rbx += ab_w + 2.0;

            let r1_x = rbx;
            unsafe {
                self.rt
                    .DrawRectangle(&rect_f(rbx, rep_y, btn_s, lh), &border, 0.5, None);
            }
            self.draw_text(crate::icons::FIND_REPLACE.s(), rbx + 2.0, rep_y, dim_fg);
            rbx += btn_s + 2.0;

            let r_all_x = rbx;
            unsafe {
                self.rt
                    .DrawRectangle(&rect_f(rbx, rep_y, btn_s, lh), &border, 0.5, None);
            }
            self.draw_text(crate::icons::FIND_REPLACE_ALL.s(), rbx + 2.0, rep_y, dim_fg);
            rep_btns = [ab_x, ab_w, r1_x, r_all_x];
        }

        super::FindReplaceRect {
            x,
            y,
            w: panel_w,
            h: panel_h,
            input_x,
            input_w,
            row_y,
            nav_x,
            btn_s,
            rep_y: if panel.show_replace {
                row_y + lh + pad
            } else {
                0.0
            },
            rep_btns,
        }
    }

    // ─── Diagnostic underlines ──────────────────────────────────────────────

    fn draw_diagnostic_underline(
        &self,
        diag: &crate::render::DiagnosticMark,
        text_x: f32,
        line_y: f32,
    ) {
        use crate::core::lsp::DiagnosticSeverity;
        let color = match diag.severity {
            DiagnosticSeverity::Error => self.theme.diagnostic_error,
            DiagnosticSeverity::Warning => self.theme.diagnostic_warning,
            DiagnosticSeverity::Information => self.theme.diagnostic_info,
            DiagnosticSeverity::Hint => self.theme.diagnostic_hint,
        };
        let brush = self.solid_brush(color);
        let x1 = text_x + diag.start_col as f32 * self.char_width;
        let x2 = text_x + diag.end_col as f32 * self.char_width;
        let y = line_y + self.line_height - 2.0;
        // Draw a zigzag underline (3 segments per character width)
        let step = (self.char_width / 3.0).max(2.0);
        let mut x = x1;
        let mut up = false;
        unsafe {
            while x < x2 {
                let nx = (x + step).min(x2);
                let y_off: f32 = if up { -2.0 } else { 0.0 };
                let ny_off: f32 = if up { 0.0 } else { -2.0 };
                self.rt
                    .FillRectangle(&rect_f(x, y + y_off.min(ny_off), nx - x, 2.0), &brush);
                up = !up;
                x = nx;
            }
        }
    }

    // ─── Signature help ──────────────────────────────────────────────────────

    fn draw_signature_help(
        &self,
        sig: &crate::render::SignatureHelp,
        active_window: Option<&RenderedWindow>,
    ) {
        let bg = self.solid_brush(self.theme.completion_bg);
        let border_brush = self.solid_brush(self.theme.separator);
        let popup_w = (sig.label.chars().count() as f32 + 4.0) * self.char_width;
        let popup_h = self.line_height + 4.0;

        let (x, y) = if let Some(rw) = active_window {
            let gutter_px = rw.gutter_char_width as f32 * self.char_width;
            let scroll_top = rw.lines.first().map_or(0, |l| l.line_idx);
            let view_line = sig.anchor_line.saturating_sub(scroll_top);
            let cx = rw.rect.x as f32 + gutter_px + sig.anchor_col as f32 * self.char_width;
            let cy = rw.rect.y as f32 + view_line as f32 * self.line_height;
            let fy = if cy >= popup_h {
                cy - popup_h
            } else {
                cy + self.line_height
            };
            let (rt_w, _) = self.rt_size();
            (cx.min(rt_w - popup_w - 2.0).max(0.0), fy.max(0.0))
        } else {
            (0.0, 0.0)
        };

        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border_brush, 1.0, None);
        }

        // Draw the label, highlighting the active parameter
        let tx = x + self.char_width;
        let ty = y + 2.0;
        if let Some(active_idx) = sig.active_param {
            if let Some(&(start, end)) = sig.params.get(active_idx) {
                // Before active param
                let before = safe_slice(&sig.label, 0, start);
                self.draw_text(before, tx, ty, self.theme.foreground);
                // Active param (highlighted)
                let param_text = safe_slice(&sig.label, start, end);
                let param_x = tx + before.chars().count() as f32 * self.char_width;
                self.draw_text(param_text, param_x, ty, self.theme.tab_active_accent);
                // After active param
                let after = safe_slice(&sig.label, end, sig.label.len());
                let after_x = param_x + param_text.chars().count() as f32 * self.char_width;
                self.draw_text(after, after_x, ty, self.theme.foreground);
            } else {
                self.draw_text(&sig.label, tx, ty, self.theme.foreground);
            }
        } else {
            self.draw_text(&sig.label, tx, ty, self.theme.foreground);
        }
    }

    // ─── Editor hover popup (rich markdown, gh / mouse dwell) ──────────────

    fn draw_editor_hover(
        &self,
        eh: &crate::render::EditorHoverPopupData,
        active_window: Option<&RenderedWindow>,
    ) {
        use crate::core::markdown::MdStyle;

        let lines = &eh.rendered.lines;
        if lines.is_empty() {
            return;
        }
        let cw = self.char_width;
        let lh = self.line_height;
        let (rt_w, rt_h) = self.rt_size();

        let max_height = 20;
        let scroll = eh.scroll_top;
        let visible_count = lines.len().saturating_sub(scroll).min(max_height);
        if visible_count == 0 {
            return;
        }
        let num_lines = lines.len().min(max_height);
        let content_w = (eh.popup_width as f32 + 2.0) * cw;
        let popup_w = content_w.clamp(12.0 * cw, rt_w * 0.7);
        let popup_h = num_lines as f32 * lh + 8.0; // padding top+bottom

        // Position relative to active window anchor
        let (x, y) = if let Some(rw) = active_window {
            let gutter_px = rw.gutter_char_width as f32 * cw;
            let view_line = eh.anchor_line.saturating_sub(eh.frozen_scroll_top);
            let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left);
            let cx = rw.rect.x as f32 + gutter_px + vis_col as f32 * cw;
            let cy = rw.rect.y as f32 + view_line as f32 * lh;
            let fy = if cy >= popup_h + 4.0 {
                cy - popup_h
            } else {
                cy + lh
            };
            (cx.min(rt_w - popup_w - 4.0).max(0.0), fy.max(0.0))
        } else {
            (0.0, 0.0)
        };

        let bg = self.solid_brush(self.theme.hover_bg);
        let border_color = if eh.has_focus {
            self.theme.md_link
        } else {
            self.theme.hover_border
        };
        let border_brush = self.solid_brush(border_color);

        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border_brush, 1.0, None);
        }

        // Render content lines with markdown styling
        for (li, text_line) in lines.iter().skip(scroll).enumerate().take(num_lines) {
            let line_y = y + 4.0 + li as f32 * lh;
            if line_y + lh > rt_h {
                break;
            }
            let actual_line = scroll + li;
            let line_spans = eh.rendered.spans.get(actual_line);
            let code_hl = eh.rendered.code_highlights.get(actual_line);
            let has_code_hl = code_hl.is_some_and(|h| !h.is_empty());

            let mut col_x = x + cw; // left padding
            let mut byte_pos: usize = 0;
            for ch in text_line.chars() {
                let ch_len = ch.len_utf8();
                let fg_color = if has_code_hl {
                    code_hl
                        .unwrap()
                        .iter()
                        .find(|h| byte_pos >= h.start_byte && byte_pos < h.end_byte)
                        .map(|h| self.theme.scope_color(&h.scope))
                        .unwrap_or(self.theme.md_code)
                } else if let Some(spans) = line_spans {
                    spans
                        .iter()
                        .find(|sp| byte_pos >= sp.start_byte && byte_pos < sp.end_byte)
                        .map(|sp| match sp.style {
                            MdStyle::Heading(1) => self.theme.md_heading1,
                            MdStyle::Heading(2) => self.theme.md_heading2,
                            MdStyle::Heading(_) => self.theme.md_heading3,
                            MdStyle::Bold | MdStyle::BoldItalic => self.theme.hover_fg,
                            MdStyle::Code | MdStyle::CodeBlock => self.theme.md_code,
                            MdStyle::Link | MdStyle::LinkUrl => self.theme.md_link,
                            MdStyle::BlockQuote => self.theme.md_heading3,
                            MdStyle::ListBullet => self.theme.md_heading1,
                            _ => self.theme.hover_fg,
                        })
                        .unwrap_or(self.theme.hover_fg)
                } else {
                    self.theme.hover_fg
                };

                if col_x + cw <= x + popup_w - cw {
                    // Selection highlight
                    let char_col = ((col_x - x - cw) / cw) as usize;
                    let in_selection = if let Some((sl, sc, el, ec)) = eh.selection {
                        if sl == el {
                            actual_line == sl && char_col >= sc && char_col < ec
                        } else if actual_line == sl {
                            char_col >= sc
                        } else if actual_line == el {
                            char_col < ec
                        } else {
                            actual_line > sl && actual_line < el
                        }
                    } else {
                        false
                    };
                    if in_selection {
                        let sel_brush = self.solid_brush(self.theme.selection);
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(col_x, line_y, cw, lh), &sel_brush);
                        }
                    }
                    self.draw_text(&ch.to_string(), col_x, line_y, fg_color);
                }
                byte_pos += ch_len;
                col_x += cw;
            }
        }

        // Scrollbar when content overflows
        if lines.len() > max_height && num_lines > 0 {
            let track_h = num_lines as f32 * lh;
            let thumb_ratio = num_lines as f32 / lines.len() as f32;
            let thumb_h = (track_h * thumb_ratio).max(lh);
            let max_scroll = lines.len().saturating_sub(max_height);
            let thumb_top = if max_scroll > 0 {
                (scroll as f32 / max_scroll as f32) * (track_h - thumb_h)
            } else {
                0.0
            };
            let sb_x = x + popup_w - 6.0;
            let sb_y = y + 4.0;
            let track_brush = self.solid_brush(self.theme.scrollbar_track);
            let thumb_brush = self.solid_brush(self.theme.scrollbar_thumb);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(sb_x, sb_y, 4.0, track_h), &track_brush);
                self.rt
                    .FillRectangle(&rect_f(sb_x, sb_y + thumb_top, 4.0, thumb_h), &thumb_brush);
            }
        }
    }

    // ─── Diff peek popup (inline git hunk preview) ──────────────────────────

    fn draw_diff_peek(
        &self,
        peek: &crate::render::DiffPeekPopup,
        active_window: Option<&RenderedWindow>,
    ) {
        if peek.hunk_lines.is_empty() {
            return;
        }
        let cw = self.char_width;
        let lh = self.line_height;
        let (rt_w, _) = self.rt_size();

        let max_lines = 29;
        let action_bar_lines = 1;
        let num_lines = (peek.hunk_lines.len().min(max_lines) + action_bar_lines) as f32;
        let max_len = peek
            .hunk_lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(10);
        let popup_w = ((max_len + 4) as f32 * cw).max(20.0 * cw);
        let popup_h = num_lines * lh + 8.0;

        // Position below anchor in active window
        let (x, y) = if let Some(rw) = active_window {
            let gutter_px = rw.gutter_char_width as f32 * cw;
            let scroll_top = rw.lines.first().map_or(0, |l| l.line_idx);
            let view_line = peek.anchor_line.saturating_sub(scroll_top);
            let cx = rw.rect.x as f32 + gutter_px;
            let cy = rw.rect.y as f32 + (view_line as f32 + 1.0) * lh;
            (cx.min(rt_w - popup_w - 4.0).max(0.0), cy.max(0.0))
        } else {
            (0.0, 0.0)
        };

        let bg = self.solid_brush(self.theme.hover_bg);
        let border_brush = self.solid_brush(self.theme.hover_border);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border_brush, 1.0, None);
        }

        // Draw diff lines with color coding
        for (i, hline) in peek.hunk_lines.iter().enumerate().take(max_lines) {
            let fg_color = if hline.starts_with('+') {
                self.theme.git_added
            } else if hline.starts_with('-') {
                self.theme.git_deleted
            } else {
                self.theme.hover_fg
            };
            self.draw_text(hline, x + cw, y + 4.0 + i as f32 * lh, fg_color);
        }

        // Action bar at bottom
        let action_y = y + 4.0 + peek.hunk_lines.len().min(max_lines) as f32 * lh;
        let actions = "[s] Stage  [r] Revert  [q] Close";
        self.draw_text(actions, x + cw, action_y, self.theme.line_number_fg);
    }

    // ─── Debug toolbar (DAP control strip) ──────────────────────────────────

    fn draw_debug_toolbar(&self, toolbar: &crate::render::DebugToolbarData) {
        let cw = self.char_width;
        let lh = self.line_height;
        let (width, height) = self.rt_size();
        let x0 = self.editor_left;

        // Position: above the terminal/status area, similar to TUI layout
        // Place it just above the status bar row (height - 2*lh for status, -lh for toolbar)
        let y = height - 3.0 * lh;
        let bar_w = width - x0;

        let bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt.FillRectangle(&rect_f(x0, y, bar_w, lh), &bg);
        }

        let fg = if toolbar.session_active {
            self.theme.status_fg
        } else {
            self.theme.line_number_fg
        };
        let dim_fg = self.theme.line_number_fg;

        let mut col = x0 + cw;
        for (idx, btn) in toolbar.buttons.iter().enumerate() {
            // Separator between button groups (index 3→4)
            if idx == 4 {
                self.draw_text("│", col, y, dim_fg);
                col += cw * 2.0;
            }
            // Icon + key hint
            let label = format!("{}({})", btn.icon, btn.key_hint);
            self.draw_text(&label, col, y, fg);
            col += (label.chars().count() as f32 + 1.0) * cw;
        }
    }

    // ─── Diff toolbar (change nav buttons in tab bar) ───────────────────────

    fn draw_diff_toolbar_in_tab_bar(
        &self,
        dt: &crate::render::DiffToolbarData,
        bar_x: f32,
        bar_y: f32,
        bar_w: f32,
        bar_h: f32,
    ) {
        let cw = self.char_width;
        let text_y = bar_y + (bar_h - self.line_height) / 2.0;
        let dim_fg = self.theme.line_number_fg;

        // Build label: "2 of 5  ↑ ↓ ≡"
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref label) = dt.change_label {
            parts.push(label.clone());
        }
        parts.push("\u{2191}".to_string()); // ↑ prev
        parts.push("\u{2193}".to_string()); // ↓ next
        parts.push("\u{2261}".to_string()); // fold toggle

        let label = parts.join("  ");
        let label_w = label.chars().count() as f32 * cw;
        let rx = bar_x + bar_w - label_w - cw * 2.0;

        let active_fg = if dt.unchanged_hidden {
            self.theme.tab_active_accent
        } else {
            dim_fg
        };
        // Draw change label part in foreground, buttons in dim
        if let Some(ref change_label) = dt.change_label {
            self.draw_text(change_label, rx, text_y, self.theme.foreground);
            let offset = change_label.chars().count() as f32 * cw;
            let rest = "  \u{2191}  \u{2193}  \u{2261}".to_string();
            self.draw_text(&rest, rx + offset, text_y, active_fg);
        } else {
            self.draw_text(&label, rx, text_y, active_fg);
        }
    }

    // ─── Tab tooltip (file path on hover) ───────────────────────────────────

    fn draw_tab_tooltip(&self, tooltip: &str, layout: &ScreenLayout) {
        let cw = self.char_width;
        let lh = self.line_height;

        // Position just below the tab bar
        let tab_bar_bottom = if layout.menu_bar.is_some() {
            super::TITLE_BAR_TOP_INSET
                + lh * super::TITLE_BAR_HEIGHT_MULT
                + lh * super::TAB_BAR_HEIGHT_MULT
        } else {
            lh * super::TAB_BAR_HEIGHT_MULT
        };

        let tooltip_w = tooltip.chars().count() as f32 * cw + cw * 2.0;
        let tooltip_h = lh + 4.0;
        let (rt_w, _) = self.rt_size();
        // Position under the hovered tab, clamped to window bounds
        let x = self
            .tab_tooltip_x
            .max(self.editor_left)
            .min(rt_w - tooltip_w);

        let bg = self.solid_brush(self.theme.hover_bg);
        let border_brush = self.solid_brush(self.theme.hover_border);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(x, tab_bar_bottom, tooltip_w, tooltip_h), &bg);
            self.rt.DrawRectangle(
                &rect_f(x, tab_bar_bottom, tooltip_w, tooltip_h),
                &border_brush,
                1.0,
                None,
            );
        }
        self.draw_text(tooltip, x + cw, tab_bar_bottom + 2.0, self.theme.hover_fg);
    }

    // ─── Panel hover popup (sidebar item hover) ─────────────────────────────

    fn draw_panel_hover(&self, ph: &crate::render::PanelHoverPopupData) {
        use crate::core::markdown::MdStyle;

        let lines = &ph.rendered.lines;
        if lines.is_empty() {
            return;
        }
        let cw = self.char_width;
        let lh = self.line_height;
        let (rt_w, rt_h) = self.rt_size();

        let max_height = 20;
        let num_lines = lines.len().min(max_height);
        let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(10);
        let popup_w = ((max_len + 4) as f32 * cw).clamp(12.0 * cw, rt_w * 0.5);
        let popup_h = num_lines as f32 * lh + 8.0;

        // Position to the right of the sidebar
        let x = self.editor_left + 2.0;
        // Vertically align with the hovered item
        let y = (ph.item_index as f32 * lh + lh * 2.0)
            .min(rt_h - popup_h)
            .max(0.0);

        let bg = self.solid_brush(self.theme.hover_bg);
        let border_brush = self.solid_brush(self.theme.hover_border);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border_brush, 1.0, None);
        }

        // Render content lines with markdown styling
        for (li, text_line) in lines.iter().enumerate().take(num_lines) {
            let line_y = y + 4.0 + li as f32 * lh;
            if line_y + lh > rt_h {
                break;
            }
            let line_spans = ph.rendered.spans.get(li);
            let code_hl = ph.rendered.code_highlights.get(li);
            let has_code_hl = code_hl.is_some_and(|h| !h.is_empty());

            let mut col_x = x + cw;
            let mut byte_pos: usize = 0;
            for ch in text_line.chars() {
                let ch_len = ch.len_utf8();
                let fg_color = if has_code_hl {
                    code_hl
                        .unwrap()
                        .iter()
                        .find(|h| byte_pos >= h.start_byte && byte_pos < h.end_byte)
                        .map(|h| self.theme.scope_color(&h.scope))
                        .unwrap_or(self.theme.md_code)
                } else if let Some(spans) = line_spans {
                    spans
                        .iter()
                        .find(|sp| byte_pos >= sp.start_byte && byte_pos < sp.end_byte)
                        .map(|sp| match sp.style {
                            MdStyle::Heading(1) => self.theme.md_heading1,
                            MdStyle::Heading(2) => self.theme.md_heading2,
                            MdStyle::Heading(_) => self.theme.md_heading3,
                            MdStyle::Bold | MdStyle::BoldItalic => self.theme.hover_fg,
                            MdStyle::Code | MdStyle::CodeBlock => self.theme.md_code,
                            MdStyle::Link | MdStyle::LinkUrl => self.theme.md_link,
                            MdStyle::BlockQuote => self.theme.md_heading3,
                            MdStyle::ListBullet => self.theme.md_heading1,
                            _ => self.theme.hover_fg,
                        })
                        .unwrap_or(self.theme.hover_fg)
                } else {
                    self.theme.hover_fg
                };

                if col_x + cw <= x + popup_w - cw {
                    self.draw_text(&ch.to_string(), col_x, line_y, fg_color);
                }
                byte_pos += ch_len;
                col_x += cw;
            }
        }
    }

    // ─── Wildmenu (Tab completion bar) ───────────────────────────────────────

    fn draw_wildmenu(&self, wm: &crate::render::WildmenuData, layout: &ScreenLayout) {
        let (width, height) = self.rt_size();
        // Draw just above the command line
        let y = height - 2.0 * self.line_height;
        // Shift status bar up by one row when wildmenu is shown
        let _ = layout;

        let bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, y, width, self.line_height), &bg);
        }

        let mut x = self.char_width;
        for (i, item) in wm.items.iter().enumerate() {
            let is_selected = wm.selected == Some(i);
            let item_w = (item.chars().count() as f32 + 2.0) * self.char_width;
            if is_selected {
                let sel_bg = self.solid_brush(self.theme.selection);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(x, y, item_w, self.line_height), &sel_bg);
                }
            }
            self.draw_text(item, x + self.char_width, y, self.theme.foreground);
            x += item_w;
        }
    }

    // ─── Quickfix panel ──────────────────────────────────────────────────────

    fn draw_quickfix(&self, qf: &crate::render::QuickfixPanel, _layout: &ScreenLayout) {
        let (width, height) = self.rt_size();
        let max_visible = 6usize.min(qf.items.len());
        let panel_h = max_visible as f32 * self.line_height + self.line_height; // +1 for header
                                                                                // Position above the status bar
        let y = height - 2.0 * self.line_height - panel_h;

        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt.FillRectangle(&rect_f(0.0, y, width, panel_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(0.0, y, width, panel_h), &border, 1.0, None);
        }

        // Header
        let header = format!("Quickfix ({}/{})", qf.selected_idx + 1, qf.total_items);
        self.draw_text(&header, self.char_width, y, self.theme.foreground);

        // Items
        for (i, item) in qf.items.iter().take(max_visible).enumerate() {
            let iy = y + (i as f32 + 1.0) * self.line_height;
            if i == qf.selected_idx {
                let sel = self.solid_brush(self.theme.selection);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(0.0, iy, width, self.line_height), &sel);
                }
            }
            self.draw_text(item, self.char_width * 2.0, iy, self.theme.foreground);
        }
    }

    // ─── Picker (command palette / fuzzy finder) ─────────────────────────────

    fn draw_picker(&self, picker: &crate::render::PickerPanel) {
        let (rt_w, rt_h) = self.rt_size();

        // Semi-transparent overlay
        let overlay = self.solid_brush_alpha(self.theme.background, 0.4);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, 0.0, rt_w, rt_h), &overlay);
        }

        let max_visible = 12usize;
        let has_preview = picker.preview.is_some();
        let list_w = if has_preview { rt_w * 0.4 } else { rt_w * 0.6 };
        let total_w = if has_preview { rt_w * 0.8 } else { list_w };
        let header_h = self.line_height * 2.0; // title + input
        let body_h = max_visible as f32 * self.line_height;
        let total_h = header_h + body_h;

        let dx = (rt_w - total_w) / 2.0;
        let dy = rt_h * 0.15;

        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(dx, dy, total_w, total_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(dx, dy, total_w, total_h), &border, 1.0, None);
        }

        // Title bar
        let title_bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(dx, dy, total_w, self.line_height), &title_bg);
        }
        let title_text = format!(
            "{} ({}/{})",
            picker.title,
            picker.items.len(),
            picker.total_count
        );
        self.draw_text(&title_text, dx + self.char_width, dy, self.theme.foreground);

        // Query input
        let input_y = dy + self.line_height;
        let input_bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(
                    dx + 4.0,
                    input_y + 2.0,
                    list_w - 8.0,
                    self.line_height - 4.0,
                ),
                &input_bg,
            );
        }
        let query_display = format!("> {}", picker.query);
        self.draw_text(
            &query_display,
            dx + self.char_width,
            input_y,
            self.theme.foreground,
        );

        // Item list
        let list_y = dy + header_h;
        for (i, item) in picker
            .items
            .iter()
            .skip(picker.scroll_top)
            .take(max_visible)
            .enumerate()
        {
            let iy = list_y + i as f32 * self.line_height;
            let actual_idx = picker.scroll_top + i;
            if actual_idx == picker.selected_idx {
                let sel = self.solid_brush(self.theme.selection);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(dx, iy, list_w, self.line_height), &sel);
                }
            }

            // Indent for tree items
            let indent = item.depth as f32 * 2.0 * self.char_width;
            let ix = dx + self.char_width + indent;

            // Expand arrow for tree items
            if item.expandable {
                let arrow = if item.expanded {
                    "\u{25BC}"
                } else {
                    "\u{25B6}"
                };
                self.draw_text(
                    arrow,
                    ix - self.char_width * 1.5,
                    iy,
                    self.theme.line_number_fg,
                );
            }

            // Draw display text with match highlights
            if item.match_positions.is_empty() {
                self.draw_text(&item.display, ix, iy, self.theme.foreground);
            } else {
                // Render char by char, highlighting matches
                let mut cx = ix;
                for (ci, ch) in item.display.chars().enumerate() {
                    let color = if item.match_positions.contains(&ci) {
                        self.theme.tab_active_accent
                    } else {
                        self.theme.foreground
                    };
                    let s = String::from(ch);
                    self.draw_text(&s, cx, iy, color);
                    cx += self.char_width;
                }
            }

            // Detail hint (right-aligned)
            if let Some(ref detail) = item.detail {
                let dw = detail.chars().count() as f32 * self.char_width;
                self.draw_text(
                    detail,
                    dx + list_w - dw - self.char_width,
                    iy,
                    self.theme.line_number_fg,
                );
            }
        }

        // Preview pane
        if let Some(ref preview_lines) = picker.preview {
            let preview_x = dx + list_w;
            let preview_w = total_w - list_w;
            let sep = self.solid_brush(self.theme.separator);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(preview_x, dy + header_h, 1.0, body_h), &sep);
            }

            for (i, (line_no, text, is_hl)) in preview_lines.iter().take(max_visible).enumerate() {
                let py = list_y + i as f32 * self.line_height;
                if *is_hl {
                    let hl = self.solid_brush_alpha(self.theme.selection, 0.4);
                    unsafe {
                        self.rt.FillRectangle(
                            &rect_f(preview_x + 1.0, py, preview_w - 1.0, self.line_height),
                            &hl,
                        );
                    }
                }
                // Line number
                let ln_text = format!("{:>4} ", line_no);
                self.draw_text(
                    &ln_text,
                    preview_x + self.char_width,
                    py,
                    self.theme.line_number_fg,
                );
                // Content
                self.draw_text(
                    text,
                    preview_x + 6.0 * self.char_width,
                    py,
                    self.theme.foreground,
                );
            }
        }
    }

    // ─── Tab switcher (Ctrl+Tab) ─────────────────────────────────────────────

    fn draw_tab_switcher(&self, ts: &crate::render::TabSwitcherPanel) {
        let (rt_w, rt_h) = self.rt_size();

        let max_visible = ts.items.len().min(12);
        let max_name_w = ts
            .items
            .iter()
            .map(|(n, _, _)| n.chars().count())
            .max()
            .unwrap_or(20);
        let popup_w = (max_name_w as f32 + 6.0) * self.char_width;
        let popup_h = max_visible as f32 * self.line_height;
        let dx = (rt_w - popup_w) / 2.0;
        let dy = (rt_h - popup_h) / 2.0;

        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(dx, dy, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(dx, dy, popup_w, popup_h), &border, 1.0, None);
        }

        for (i, (name, _path, is_dirty)) in ts.items.iter().take(max_visible).enumerate() {
            let iy = dy + i as f32 * self.line_height;
            if i == ts.selected_idx {
                let sel = self.solid_brush(self.theme.selection);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(dx, iy, popup_w, self.line_height), &sel);
                }
            }
            let label = if *is_dirty {
                format!("\u{25CF} {}", name)
            } else {
                format!("  {}", name)
            };
            self.draw_text(&label, dx + self.char_width, iy, self.theme.foreground);
        }
    }

    // ─── Context menu ────────────────────────────────────────────────────────

    pub(super) fn draw_context_menu(&self, ctx: &crate::render::ContextMenuPanel) {
        let max_label = ctx
            .items
            .iter()
            .map(|i| i.label.chars().count() + i.shortcut.chars().count() + 4)
            .max()
            .unwrap_or(20);
        let popup_w = max_label as f32 * self.char_width;
        let popup_h = ctx.items.len() as f32 * self.line_height;
        let x = ctx.screen_col as f32 * self.char_width;
        let y = ctx.screen_row as f32 * self.line_height;

        // Clamp to screen
        let (rt_w, rt_h) = self.rt_size();
        let x = x.min(rt_w - popup_w).max(0.0);
        let y = y.min(rt_h - popup_h).max(0.0);

        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt
                .DrawRectangle(&rect_f(x, y, popup_w, popup_h), &border, 1.0, None);
        }

        for (i, item) in ctx.items.iter().enumerate() {
            let iy = y + i as f32 * self.line_height;
            if i == ctx.selected_idx {
                let sel = self.solid_brush(self.theme.selection);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(x, iy, popup_w, self.line_height), &sel);
                }
            }
            self.draw_text(&item.label, x + self.char_width, iy, self.theme.foreground);
            if !item.shortcut.is_empty() {
                let sw = item.shortcut.chars().count() as f32 * self.char_width;
                self.draw_text(
                    &item.shortcut,
                    x + popup_w - sw - self.char_width,
                    iy,
                    self.theme.line_number_fg,
                );
            }
            if item.separator_after {
                let sep = self.solid_brush(self.theme.separator);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(x + 4.0, iy + self.line_height - 1.0, popup_w - 8.0, 1.0),
                        &sep,
                    );
                }
            }
        }
    }

    // ─── Sidebar ─────────────────────────────────────────────────────────────

    pub(super) fn draw_sidebar(
        &self,
        sidebar: &WinSidebar,
        screen: &ScreenLayout,
        menu_bar_y: f32,
        bottom_chrome_px: f32,
        engine: &crate::core::engine::Engine,
    ) {
        let (_, rt_h) = self.rt_size();
        let sidebar_bottom = rt_h - bottom_chrome_px;
        let ab_w = sidebar.activity_bar_px;
        let top = menu_bar_y; // sidebar starts below the menu bar

        // Activity bar background (always drawn, full height)
        let ab_bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, top, ab_w, rt_h - top), &ab_bg);
        }

        // Activity bar icons — use Nerd Font codepoints when available (bundled
        // vimcode-icons.ttf), fall back to Segoe MDL2 Assets / Segoe Fluent Icons.
        let icon_row_h = ab_w; // square cells matching the 48px activity bar width
        let panels: &[(SidebarPanel, &str)] = if self.nerd_icon_font {
            &[
                (SidebarPanel::Explorer, icons::EXPLORER.nerd),
                (SidebarPanel::Search, icons::SEARCH_COD.nerd),
                (SidebarPanel::Debug, icons::DEBUG.nerd),
                (SidebarPanel::Git, icons::GIT_BRANCH.nerd),
                (SidebarPanel::Extensions, icons::EXTENSIONS.nerd),
                (SidebarPanel::Ai, icons::AI_CHAT.nerd),
            ]
        } else {
            &[
                (SidebarPanel::Explorer, "\u{ED25}"),   // Segoe FileExplorer
                (SidebarPanel::Search, "\u{E721}"),     // Segoe Search
                (SidebarPanel::Debug, "\u{EBE8}"),      // Segoe Bug
                (SidebarPanel::Git, "\u{E8D4}"),        // Segoe BranchFork2
                (SidebarPanel::Extensions, "\u{EA86}"), // Segoe Puzzle
                (SidebarPanel::Ai, "\u{E8BD}"),         // Segoe Chat
            ]
        };

        for (i, &(panel, icon)) in panels.iter().enumerate() {
            let y = top + i as f32 * icon_row_h;
            let is_active = sidebar.visible
                && sidebar.ext_panel_name.is_none()
                && sidebar.active_panel == panel;

            if is_active {
                // Left accent bar
                let accent = self.solid_brush(self.theme.tab_active_accent);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(0.0, y, 2.0, icon_row_h), &accent);
                }
                // Active background
                let sel = self.solid_brush(self.theme.active_background);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(2.0, y, ab_w - 2.0, icon_row_h), &sel);
                }
            }

            // Icon text (centered in activity bar cell)
            self.draw_icon_text(icon, 0.0, y, ab_w, icon_row_h, self.theme.activity_bar_fg);
        }

        // Extension panel icons (after fixed panels, before settings gear)
        {
            let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
            ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
            for (i, panel) in ext_panels.iter().enumerate() {
                let y = top + (panels.len() + i) as f32 * icon_row_h;
                if y + icon_row_h >= sidebar_bottom - icon_row_h {
                    break; // leave room for settings gear
                }
                let is_active =
                    sidebar.visible && sidebar.ext_panel_name.as_deref() == Some(&panel.name);
                if is_active {
                    let accent = self.solid_brush(self.theme.tab_active_accent);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(0.0, y, 2.0, icon_row_h), &accent);
                    }
                    let sel = self.solid_brush(self.theme.active_background);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(2.0, y, ab_w - 2.0, icon_row_h), &sel);
                    }
                }
                let icon_char = panel.resolved_icon();
                let icon_str: String;
                let icon_text = if self.nerd_icon_font {
                    // Nerd Font available — render the glyph directly
                    icon_str = String::from(icon_char);
                    icon_str.as_str()
                } else {
                    // Map Nerd Font glyphs to Segoe MDL2 equivalents
                    match icon_char {
                        '\u{f1d3}' => "\u{E81C}",              // git branch → History
                        '\u{e702}' | '\u{e725}' => "\u{E8D4}", // git → BranchFork2
                        '\u{f120}' | '\u{e795}' => "\u{E756}", // terminal → CommandPrompt
                        '\u{f002}' | '\u{f422}' => "\u{E721}", // search → Search
                        '\u{f07b}' | '\u{f07c}' => "\u{ED25}", // folder → FileExplorer
                        '\u{f188}' => "\u{EBE8}",              // bug → Bug
                        '\u{f085}' | '\u{e615}' => "\u{E713}", // cog → Settings
                        '\u{f075}' | '\u{f27a}' => "\u{E8BD}", // comment → Chat
                        _ => "\u{E74C}",                       // fallback → Page
                    }
                };
                self.draw_icon_text(
                    icon_text,
                    0.0,
                    y,
                    ab_w,
                    icon_row_h,
                    self.theme.activity_bar_fg,
                );
            }
        }

        // Settings gear pinned to bottom of activity bar (like TUI/VSCode)
        {
            let y = sidebar_bottom - icon_row_h;
            let is_active = sidebar.visible && sidebar.active_panel == SidebarPanel::Settings;
            if is_active {
                let accent = self.solid_brush(self.theme.tab_active_accent);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(0.0, y, 2.0, icon_row_h), &accent);
                }
                let sel = self.solid_brush(self.theme.active_background);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(2.0, y, ab_w - 2.0, icon_row_h), &sel);
                }
            }
            let gear_icon = if self.nerd_icon_font {
                icons::SETTINGS.nerd
            } else {
                "\u{E713}" // Segoe Settings gear
            };
            self.draw_icon_text(
                gear_icon,
                0.0,
                y,
                ab_w,
                icon_row_h,
                self.theme.activity_bar_fg,
            );
        }

        // If sidebar panel is not visible, we're done
        if !sidebar.visible {
            return;
        }

        // Panel background (full height including bottom chrome, so no gaps)
        let panel_x = ab_w;
        let panel_w = sidebar.panel_width;
        let panel_bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x, top, panel_w, rt_h - top), &panel_bg);
        }

        // Separator line between panel and editor
        let sep = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x + panel_w - 1.0, top, 1.0, rt_h - top), &sep);
        }

        // Clip panel content to prevent bleeding into status/command line area
        let clip_rect = D2D_RECT_F {
            left: panel_x,
            top,
            right: panel_x + panel_w,
            bottom: sidebar_bottom,
        };
        unsafe {
            self.rt
                .PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }

        let panel_h = sidebar_bottom - top;
        if sidebar.ext_panel_name.is_some() {
            // Extension panel overrides the normal panel
            self.draw_ext_panel(screen, engine, panel_x, panel_w, panel_h, top);
        } else {
            match sidebar.active_panel {
                SidebarPanel::Explorer => {
                    self.draw_explorer_panel(sidebar, panel_x, panel_w, panel_h, top, engine);
                }
                SidebarPanel::Git => self.draw_git_panel(screen, panel_x, panel_w, panel_h, top),
                SidebarPanel::Debug => {
                    self.draw_debug_panel(screen, panel_x, panel_w, panel_h, top)
                }
                SidebarPanel::Extensions => {
                    self.draw_extensions_panel(screen, panel_x, panel_w, panel_h, top);
                }
                SidebarPanel::Search => {
                    self.draw_search_panel(engine, sidebar, panel_x, panel_w, panel_h, top);
                }
                SidebarPanel::Ai => self.draw_ai_panel(screen, panel_x, panel_w, panel_h, top),
                SidebarPanel::Settings => {
                    self.draw_settings_panel(engine, panel_x, panel_w, panel_h, top);
                }
            }
        }

        // Pop clip
        unsafe {
            self.rt.PopAxisAlignedClip();
        }
    }

    fn draw_explorer_panel(
        &self,
        sidebar: &WinSidebar,
        panel_x: f32,
        panel_w: f32,
        panel_h: f32,
        top: f32,
        engine: &crate::core::engine::Engine,
    ) {
        // Row 0: "EXPLORER" panel header (kept as a bespoke row to match
        // the pre-migration Win-GUI layout; SC/Debug/etc. panels also
        // render their own headers outside the primitive). The tree
        // begins at `top + line_height`.
        self.draw_text(
            "EXPLORER",
            panel_x + self.char_width,
            top,
            self.theme.foreground,
        );

        // Phase A.2c: file-tree rows now render through the shared
        // `quadraui::TreeView` primitive. The adapter
        // (`render::explorer_to_tree_view`) overlays git-status letters
        // and LSP diagnostic badges onto each row; `quadraui_win::
        // draw_tree` rasterises the tree with uniform `line_height`
        // rows (matching A.1c's SC panel cadence) so the flat-row
        // click-hit math in `src/win_gui/mod.rs` — `(row - 1)` to skip
        // the EXPLORER header — remains valid.
        let tree_start_y = top + self.line_height;
        let tree_h = (panel_h - self.line_height).max(0.0);
        let tree = crate::render::explorer_to_tree_view(
            &sidebar.rows,
            sidebar.scroll_top,
            sidebar.selected,
            sidebar.has_focus,
            engine,
        );
        super::quadraui_win::draw_tree(self, panel_x, tree_start_y, panel_w, tree_h, &tree);
    }

    // ─── Git (Source Control) panel ─────────────────────────────────────────

    fn draw_git_panel(
        &self,
        screen: &ScreenLayout,
        panel_x: f32,
        panel_w: f32,
        panel_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;
        let bg = self.theme.tab_bar_bg;
        let fg = self.theme.foreground;
        let dim = self.theme.line_number_fg;
        let sel_bg = self.theme.fuzzy_selected_bg;
        let hdr_bg = self.theme.status_bg;
        let hdr_fg = self.theme.status_fg;

        // Background fill
        let bg_brush = self.solid_brush(bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x, top, panel_w, panel_h), &bg_brush);
        }

        let Some(ref sc) = screen.source_control else {
            // Header
            let hdr_brush = self.solid_brush(hdr_bg);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(panel_x, top, panel_w, lh), &hdr_brush);
            }
            self.draw_text("  SOURCE CONTROL", panel_x, top, hdr_fg);
            self.draw_text("No git repository", panel_x + cw, top + lh, dim);
            return;
        };

        let mut ry: f32 = 0.0;

        // ── Row 0: Header "SOURCE CONTROL" with branch + ahead/behind ───
        {
            let hdr_brush = self.solid_brush(hdr_bg);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(panel_x, top + ry, panel_w, lh), &hdr_brush);
            }
            let branch_info = if sc.ahead > 0 || sc.behind > 0 {
                format!(
                    "  SOURCE CONTROL  {}  \u{2191}{} \u{2193}{}",
                    sc.branch, sc.ahead, sc.behind
                )
            } else {
                format!("  SOURCE CONTROL  {}", sc.branch)
            };
            self.draw_text(&branch_info, panel_x, top + ry, hdr_fg);
            ry += lh;
        }
        if ry >= panel_h {
            return;
        }

        // ── Hint bar at bottom (when focused) ──────────────────────────
        let hint_h = if sc.has_focus { lh } else { 0.0 };
        if sc.has_focus {
            let hint_y = top + panel_h - lh;
            let hdr_brush = self.solid_brush(hdr_bg);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(panel_x, hint_y, panel_w, lh), &hdr_brush);
            }
            self.draw_text(" Press '?' for help", panel_x, hint_y, dim);
        }
        let content_bottom = panel_h - hint_h;

        // ── Commit input row(s) ─────────────────────────────────────────
        let commit_lines: Vec<&str> = sc.commit_message.split('\n').collect();
        let commit_rows = commit_lines.len().max(1);
        {
            let inp_bg = if sc.commit_input_active { sel_bg } else { bg };
            let prompt_fg = if sc.commit_input_active { fg } else { dim };
            let inp_brush = self.solid_brush(inp_bg);
            let commit_h = commit_rows as f32 * lh;
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(panel_x, top + ry, panel_w, commit_h), &inp_brush);
            }

            // Cursor position for active input
            let (cursor_line, cursor_col) = if sc.commit_input_active {
                let before = &sc.commit_message[..sc.commit_cursor.min(sc.commit_message.len())];
                let cl = before.matches('\n').count();
                let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                (cl, before[line_start..].chars().count())
            } else {
                (0, 0)
            };

            let prefix = " \u{270E}  "; // ✎ pencil
            let pad = "    ";

            if sc.commit_message.is_empty() && !sc.commit_input_active {
                self.draw_text(
                    &format!("{}Message (press c)", prefix),
                    panel_x,
                    top + ry,
                    prompt_fg,
                );
            } else {
                for (line_idx, line) in commit_lines.iter().enumerate() {
                    let line_y = top + ry + line_idx as f32 * lh;
                    if line_y >= top + content_bottom {
                        break;
                    }
                    let pfx = if line_idx == 0 { prefix } else { pad };
                    let text = format!("{}{}", pfx, line);
                    self.draw_text(&text, panel_x, line_y, prompt_fg);

                    // Draw cursor
                    if sc.commit_input_active && line_idx == cursor_line {
                        let pfx_len = pfx.chars().count();
                        let cursor_x = panel_x + (pfx_len + cursor_col) as f32 * cw;
                        let cursor_brush = self.solid_brush(fg);
                        unsafe {
                            self.rt
                                .FillRectangle(&rect_f(cursor_x, line_y, 1.5, lh), &cursor_brush);
                        }
                    }
                }
            }
            ry += commit_h;
        }
        if ry >= content_bottom {
            return;
        }

        // ── Button row (Commit / Push / Pull / Sync) ────────────────────
        {
            // Padding above
            ry += lh * 0.3;
            let btn_y = top + ry;
            let btn_bg_color = hdr_bg;
            let hover_bg = Color {
                r: (hdr_bg.r as u16 + 20).min(255) as u8,
                g: (hdr_bg.g as u16 + 20).min(255) as u8,
                b: (hdr_bg.b as u16 + 20).min(255) as u8,
            };

            let commit_w = (panel_w * 0.5).max(cw);
            let remain = panel_w - commit_w;
            let icon_w = (remain / 3.0).max(cw);

            let buttons: [(f32, f32, &str, usize); 4] = [
                (0.0, commit_w, " \u{2713} Commit", 0),
                (commit_w, icon_w, " \u{2191}", 1), // Push ↑
                (commit_w + icon_w, icon_w, " \u{2193}", 2), // Pull ↓
                (
                    commit_w + icon_w * 2.0,
                    panel_w - commit_w - icon_w * 2.0,
                    " \u{21BB}", // Sync ↻
                    3,
                ),
            ];
            for (x_off, seg_w, text, btn_idx) in &buttons {
                let bx = panel_x + x_off;
                let is_focused = sc.button_focused == Some(*btn_idx);
                let is_hovered = sc.button_hovered == Some(*btn_idx);
                let (b_fg, b_bg) = if is_focused {
                    (btn_bg_color, hdr_fg) // inverted
                } else if is_hovered {
                    (hdr_fg, hover_bg)
                } else {
                    (hdr_fg, btn_bg_color)
                };
                let btn_brush = self.solid_brush(b_bg);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(bx, btn_y, *seg_w, lh), &btn_brush);
                }
                self.draw_text(text, bx, btn_y, b_fg);
            }
            ry += lh;
            // Padding below
            ry += lh * 0.3;
        }
        if ry >= content_bottom {
            return;
        }

        // ── Sections — migrated to the `quadraui::TreeView` primitive ──
        // (Phase A.1c). Adapter builds a TreeView covering the four
        // sections (Staged / Changes / Worktrees / Log); `quadraui_win::
        // draw_tree` renders it with Direct2D + DirectWrite. Row height
        // is `lh` uniformly, matching the previous layout so the
        // click-hit math in `src/win_gui/mod.rs` continues to work.
        let sc_tree = crate::render::source_control_to_tree_view(sc, self.theme);
        let tree_row_count = sc_tree.rows.len();
        let sections_h = (content_bottom - ry).max(0.0);
        super::quadraui_win::draw_tree(self, panel_x, top + ry, panel_w, sections_h, &sc_tree);
        ry += tree_row_count as f32 * lh;

        // ── Scrollbar ───────────────────────────────────────────────────
        // Total content rows = tree rows + commit rows + button row + gaps.
        let total_content_h = ry;
        let visible_h = content_bottom - lh; // minus header
        if total_content_h > visible_h && visible_h > 0.0 {
            let track_h = content_bottom - lh;
            let thumb_h = (track_h * visible_h / total_content_h).max(8.0);
            // No scroll offset for now (sections are rendered top-to-bottom).
            let sb_x = panel_x + panel_w - 5.0;
            let dim_brush = self.solid_brush(dim);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(sb_x, top + lh, 4.0, thumb_h), &dim_brush);
            }
        }

        // ── Branch picker popup ─────────────────────────────────────────
        if let Some(ref bp) = sc.branch_picker {
            let popup_w = panel_w.min(300.0);
            let popup_h = if bp.create_mode {
                lh * 3.0
            } else {
                (panel_h * 0.6).min(lh * 15.0)
            };
            let popup_x = panel_x + (panel_w - popup_w) / 2.0;
            let popup_y = top + lh * 2.0;

            let popup_bg_color = self.theme.completion_bg;
            let popup_fg_color = self.theme.completion_fg;
            let popup_border_color = self.theme.completion_border;
            let popup_sel_color = self.theme.completion_selected_bg;

            // Background + border
            let pbg = self.solid_brush(popup_bg_color);
            let pborder = self.solid_brush(popup_border_color);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(popup_x, popup_y, popup_w, popup_h), &pbg);
                self.rt.DrawRectangle(
                    &rect_f(popup_x, popup_y, popup_w, popup_h),
                    &pborder,
                    1.0,
                    None,
                );
            }

            // Title
            let title = if bp.create_mode {
                "New Branch"
            } else {
                "Switch Branch"
            };
            self.draw_text(title, popup_x + 8.0, popup_y, popup_fg_color);

            if bp.create_mode {
                // Name input
                let iy = popup_y + lh;
                self.draw_text("Name: ", popup_x + 8.0, iy, dim);
                let input_x = popup_x + 8.0 + self.mono_text_width("Name: ");
                self.draw_text(&bp.create_input, input_x, iy, popup_fg_color);
                let cursor_x = input_x + self.mono_text_width(&bp.create_input);
                let cursor_brush = self.solid_brush(popup_fg_color);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(cursor_x, iy, 1.5, lh), &cursor_brush);
                }
            } else {
                // Search input
                let iy = popup_y + lh;
                self.draw_text(" \u{1F50D} ", popup_x, iy, dim);
                let qx = popup_x + self.mono_text_width(" \u{1F50D} ");
                self.draw_text(&bp.query, qx, iy, popup_fg_color);

                // Branch list
                let list_y = popup_y + lh * 2.0;
                let list_h = ((popup_h - lh * 3.0) / lh) as usize;
                let scroll_off = if bp.selected >= list_h {
                    bp.selected - list_h + 1
                } else {
                    0
                };
                for (vi, (name, is_current)) in
                    bp.results.iter().skip(scroll_off).take(list_h).enumerate()
                {
                    let ey = list_y + vi as f32 * lh;
                    let is_sel = vi + scroll_off == bp.selected;
                    if is_sel {
                        let sel_brush = self.solid_brush(popup_sel_color);
                        unsafe {
                            self.rt.FillRectangle(
                                &rect_f(popup_x + 1.0, ey, popup_w - 2.0, lh),
                                &sel_brush,
                            );
                        }
                    }
                    let marker = if *is_current { "\u{25CF} " } else { "  " };
                    let display = format!("{}{}", marker, name);
                    self.draw_text(&display, popup_x + 8.0, ey, popup_fg_color);
                }
            }
        }

        // ── Help dialog ─────────────────────────────────────────────────
        if sc.help_open {
            let bindings: &[(&str, &str)] = &[
                ("j/k", "Navigate"),
                ("s", "Stage / unstage"),
                ("S", "Stage all"),
                ("d", "Discard file"),
                ("D", "Discard all unstaged"),
                ("c", "Commit message"),
                ("b", "Switch branch"),
                ("B", "Create branch"),
                ("p", "Push"),
                ("P", "Pull"),
                ("f", "Fetch"),
                ("r", "Refresh"),
                ("Tab", "Expand / collapse"),
                ("Enter", "Open file"),
                ("q/Esc", "Close panel"),
            ];

            let popup_w = panel_w.min(280.0);
            let popup_h = lh * (bindings.len() as f32 + 2.0);
            let popup_x = panel_x + (panel_w - popup_w) / 2.0;
            let popup_y = top + (panel_h - popup_h) / 2.0;

            let popup_bg_color = self.theme.completion_bg;
            let popup_fg_color = self.theme.completion_fg;
            let popup_border_color = self.theme.completion_border;

            let pbg = self.solid_brush(popup_bg_color);
            let pborder = self.solid_brush(popup_border_color);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(popup_x, popup_y, popup_w, popup_h), &pbg);
                self.rt.DrawRectangle(
                    &rect_f(popup_x, popup_y, popup_w, popup_h),
                    &pborder,
                    1.0,
                    None,
                );
            }

            // Title + close hint
            self.draw_text("Keybindings", popup_x + 8.0, popup_y, popup_fg_color);
            self.draw_text("x", popup_x + popup_w - 16.0, popup_y, popup_fg_color);

            // Bindings
            let key_color = self.theme.function;
            for (i, (key, desc)) in bindings.iter().enumerate() {
                let bind_y = popup_y + lh * (i as f32 + 1.0);
                if bind_y >= popup_y + popup_h - lh {
                    break;
                }
                self.draw_text(key, popup_x + 12.0, bind_y, key_color);
                self.draw_text(desc, popup_x + 100.0, bind_y, popup_fg_color);
            }
        }
    }

    // ─── Debug panel ─────────────────────────────────────────────────────────

    fn draw_debug_panel(
        &self,
        screen: &ScreenLayout,
        panel_x: f32,
        panel_w: f32,
        _rt_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;
        let _ = panel_w;

        self.draw_text("DEBUG", panel_x + cw, top, self.theme.foreground);

        let sidebar = &screen.debug_sidebar;
        if !sidebar.session_active {
            let cfg = sidebar.launch_config_name.as_deref().unwrap_or("no config");
            self.draw_text(
                &format!("Config: {}", cfg),
                panel_x + cw,
                top + lh,
                self.theme.line_number_fg,
            );
            self.draw_text(
                "Press F5 to start",
                panel_x + cw,
                top + lh * 2.0,
                self.theme.line_number_fg,
            );
            return;
        }

        let status = if sidebar.stopped { "PAUSED" } else { "RUNNING" };
        let status_color = if sidebar.stopped {
            self.theme.diagnostic_warning
        } else {
            self.theme.git_added
        };
        self.draw_text(status, panel_x + cw, top + lh, status_color);

        let mut y = top + lh * 2.5;
        let sections: &[(&str, &[crate::render::DebugSidebarItem])] = &[
            ("Variables", &sidebar.variables),
            ("Watch", &sidebar.watch),
            ("Call Stack", &sidebar.frames),
            ("Breakpoints", &sidebar.breakpoints),
        ];
        for (name, items) in sections {
            self.draw_text(
                &format!("\u{25BC} {}", name),
                panel_x + cw,
                y,
                self.theme.foreground,
            );
            y += lh;
            for item in items.iter().take(15) {
                self.draw_text(&item.text, panel_x + cw * 2.5, y, self.theme.foreground);
                y += lh;
            }
            y += lh * 0.3;
        }
    }

    // ─── Extensions panel ────────────────────────────────────────────────────

    fn draw_extensions_panel(
        &self,
        screen: &ScreenLayout,
        panel_x: f32,
        panel_w: f32,
        _rt_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;
        let _ = panel_w;

        self.draw_text("EXTENSIONS", panel_x + cw, top, self.theme.foreground);

        let Some(ref ext) = screen.ext_sidebar else {
            self.draw_text(
                "No extension data",
                panel_x + cw,
                top + lh,
                self.theme.line_number_fg,
            );
            return;
        };

        let mut y = top + lh * 1.5;

        // Installed
        let arrow = if ext.sections_expanded[0] {
            "\u{25BC}"
        } else {
            "\u{25B6}"
        };
        self.draw_text(
            &format!("{} Installed ({})", arrow, ext.items_installed.len()),
            panel_x + cw,
            y,
            self.theme.foreground,
        );
        y += lh;
        if ext.sections_expanded[0] {
            for (i, item) in ext.items_installed.iter().enumerate() {
                let is_sel = ext.has_focus && ext.selected == i;
                if is_sel {
                    let sel_bg = self.solid_brush(self.theme.explorer_active_bg);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(panel_x, y, panel_w, lh), &sel_bg);
                    }
                }
                self.draw_text(&item.name, panel_x + cw * 2.5, y, self.theme.foreground);
                y += lh;
            }
        }

        y += lh * 0.3;

        // Available
        let arrow = if ext.sections_expanded[1] {
            "\u{25BC}"
        } else {
            "\u{25B6}"
        };
        self.draw_text(
            &format!("{} Available ({})", arrow, ext.items_available.len()),
            panel_x + cw,
            y,
            self.theme.foreground,
        );
        y += lh;
        if ext.sections_expanded[1] {
            let inst_len = ext.items_installed.len();
            for (i, item) in ext.items_available.iter().enumerate() {
                let is_sel = ext.has_focus && ext.selected == inst_len + i;
                if is_sel {
                    let sel_bg = self.solid_brush(self.theme.explorer_active_bg);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(panel_x, y, panel_w, lh), &sel_bg);
                    }
                }
                self.draw_text(&item.name, panel_x + cw * 2.5, y, self.theme.foreground);
                y += lh;
            }
        }
    }

    // ─── Search panel ─────────────────────────────────────────────────────────

    fn draw_search_panel(
        &self,
        engine: &crate::core::engine::Engine,
        sidebar: &WinSidebar,
        panel_x: f32,
        panel_w: f32,
        panel_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;
        let fg = self.theme.foreground;
        let dim = self.theme.line_number_fg;

        // Header
        self.draw_text("SEARCH", panel_x + cw, top, fg);

        // Search input box
        let input_y = top + lh * 1.5;
        let input_active = sidebar.search_input_mode && !sidebar.replace_input_focused;
        let input_bg_color = if input_active {
            self.theme.active_background
        } else {
            self.theme.background
        };
        let input_bg = self.solid_brush(input_bg_color);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &input_bg,
            );
        }
        let border_color = if input_active {
            self.theme.cursor
        } else {
            self.theme.separator
        };
        let border = self.solid_brush(border_color);
        unsafe {
            self.rt.DrawRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &border,
                1.0,
                None,
            );
        }
        if engine.project_search_query.is_empty() {
            self.draw_text("Search…", panel_x + cw, input_y, dim);
        } else {
            self.draw_text(&engine.project_search_query, panel_x + cw, input_y, fg);
        }

        // Replace input box
        let replace_y = input_y + lh * 1.2;
        let rep_active = sidebar.search_input_mode && sidebar.replace_input_focused;
        let rep_bg_color = if rep_active {
            self.theme.active_background
        } else {
            self.theme.background
        };
        let rep_bg = self.solid_brush(rep_bg_color);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(panel_x + cw * 0.5, replace_y, panel_w - cw, lh),
                &rep_bg,
            );
            let rep_border_color = if rep_active {
                self.theme.cursor
            } else {
                self.theme.separator
            };
            let rep_border = self.solid_brush(rep_border_color);
            self.rt.DrawRectangle(
                &rect_f(panel_x + cw * 0.5, replace_y, panel_w - cw, lh),
                &rep_border,
                1.0,
                None,
            );
        }
        if engine.project_replace_text.is_empty() {
            self.draw_text("Replace…", panel_x + cw, replace_y, dim);
        } else {
            self.draw_text(&engine.project_replace_text, panel_x + cw, replace_y, fg);
        }

        // Toggle indicators
        let toggle_y = replace_y + lh * 1.2;
        let opts = &engine.project_search_options;
        let active_color = self.theme.keyword;
        let mut tx = panel_x + cw * 0.5;
        let draw_toggle = |ctx: &DrawContext, label: &str, active: bool, x: &mut f32| {
            let color = if active { active_color } else { dim };
            ctx.draw_text(label, *x, toggle_y, color);
            *x += (label.len() as f32 + 1.0) * cw;
        };
        draw_toggle(self, "Aa", opts.case_sensitive, &mut tx);
        draw_toggle(self, "Ab|", opts.whole_word, &mut tx);
        draw_toggle(self, ".*", opts.use_regex, &mut tx);
        self.draw_text("Alt+C/W/R", tx + cw, toggle_y, dim);

        // Status / hint
        let status_y = toggle_y + lh;
        let status = if engine.project_search_results.is_empty() {
            if engine.project_search_query.is_empty() {
                "Type to search, Enter to run".to_string()
            } else {
                format!("{} results", engine.project_search_results.len())
            }
        } else {
            format!("{} results", engine.project_search_results.len())
        };
        self.draw_text(&status, panel_x + cw * 0.5, status_y, dim);

        // Results list
        let results_y = status_y + lh;
        let results = &engine.project_search_results;
        if results.is_empty() {
            return;
        }
        let max_rows = ((top + panel_h - results_y) / lh).floor() as usize;
        let root = engine
            .workspace_root
            .as_deref()
            .unwrap_or(std::path::Path::new(""));
        let mut last_file: Option<&std::path::Path> = None;
        let mut row = 0;
        let mut skip = sidebar.search_scroll_top;
        let selected = engine.project_search_selected;

        for (idx, m) in results.iter().enumerate() {
            if row >= max_rows {
                break;
            }
            // File header
            if last_file != Some(m.file.as_path()) {
                last_file = Some(m.file.as_path());
                if skip > 0 {
                    skip -= 1;
                } else {
                    let rel = m.file.strip_prefix(root).unwrap_or(&m.file);
                    let ry = results_y + row as f32 * lh;
                    self.draw_text(
                        &rel.display().to_string(),
                        panel_x + cw * 0.5,
                        ry,
                        self.theme.keyword,
                    );
                    row += 1;
                    if row >= max_rows {
                        break;
                    }
                }
            }
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let ry = results_y + row as f32 * lh;
            let snippet = format!("  {}: {}", m.line + 1, m.line_text.trim());
            // Highlight selected row
            if idx == selected {
                let sel_bg = self.solid_brush(self.theme.selection);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(panel_x + cw * 0.5, ry, panel_w - cw, lh), &sel_bg);
                }
                self.draw_text(&snippet, panel_x + cw, ry, fg);
            } else {
                self.draw_text(&snippet, panel_x + cw, ry, dim);
            }
            row += 1;
        }
    }

    // ─── AI panel ────────────────────────────────────────────────────────────

    fn draw_ai_panel(
        &self,
        screen: &ScreenLayout,
        panel_x: f32,
        panel_w: f32,
        rt_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;

        self.draw_text("AI ASSISTANT", panel_x + cw, top, self.theme.foreground);

        let Some(ref ai) = screen.ai_panel else {
            self.draw_text(
                "Set ai_api_key in settings",
                panel_x + cw,
                top + lh * 1.5,
                self.theme.line_number_fg,
            );
            return;
        };

        // Messages
        let mut y = top + lh * 1.5;
        let max_y = rt_h - lh * 3.0;
        let wrap_cols = ((panel_w - cw * 2.0) / cw).floor() as usize;

        for msg in ai.messages.iter().skip(ai.scroll_top) {
            if y > max_y {
                break;
            }

            let role_color = if msg.role == "user" {
                self.theme.tab_active_accent
            } else {
                self.theme.git_added
            };
            let label = if msg.role == "user" { "You:" } else { "AI:" };
            self.draw_text(label, panel_x + cw, y, role_color);
            y += lh;

            // Word-wrap content
            for line in msg.content.lines() {
                if y > max_y {
                    break;
                }
                if wrap_cols > 0 && line.chars().count() > wrap_cols {
                    let mut remaining = line;
                    while !remaining.is_empty() && y <= max_y {
                        let take: String = remaining.chars().take(wrap_cols).collect();
                        self.draw_text(&take, panel_x + cw * 1.5, y, self.theme.foreground);
                        remaining = &remaining[take.len()..];
                        y += lh;
                    }
                } else {
                    self.draw_text(line, panel_x + cw * 1.5, y, self.theme.foreground);
                    y += lh;
                }
            }
            y += lh * 0.3;
        }

        // Input box at bottom
        let input_y = rt_h - lh * 2.0;
        let input_bg_color = if ai.input_active {
            self.theme.active_background
        } else {
            self.theme.background
        };
        let input_bg = self.solid_brush(input_bg_color);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &input_bg,
            );
        }
        let border_color = if ai.input_active {
            self.theme.cursor
        } else {
            self.theme.separator
        };
        let border = self.solid_brush(border_color);
        unsafe {
            self.rt.DrawRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &border,
                1.0,
                None,
            );
        }
        let input_text = if ai.input.is_empty() && !ai.input_active {
            "Click here to ask a question…"
        } else if ai.input.is_empty() {
            "Type your message…"
        } else {
            &ai.input
        };
        let input_color = if ai.input.is_empty() {
            self.theme.line_number_fg
        } else {
            self.theme.foreground
        };
        self.draw_text(input_text, panel_x + cw, input_y, input_color);
        // Draw cursor when input is active
        if ai.input_active {
            let cursor_x = panel_x + cw + ai.input_cursor as f32 * self.char_width;
            let cursor_brush = self.solid_brush(self.theme.cursor);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(cursor_x, input_y, 2.0, lh), &cursor_brush);
            }
        }
    }

    // ─── Notification toasts ────────────────────────────────────────────────

    pub(super) fn draw_notifications(&self, notifications: &[Notification]) {
        if notifications.is_empty() {
            return;
        }
        let (rt_w, rt_h) = self.rt_size();
        let lh = self.line_height;
        let cw = self.char_width;
        let toast_w = 300.0f32.min(rt_w * 0.4);
        let margin = 8.0f32;
        let x = rt_w - toast_w - margin;
        let mut y = rt_h - 3.0 * lh - margin; // above status+command line

        let bg = self.solid_brush(self.theme.completion_bg);
        let border = self.solid_brush(self.theme.separator);

        for notif in notifications.iter().rev().take(3) {
            let spinner = if notif.done { "\u{2714}" } else { "\u{25CB}" };
            let text = format!("{} {}", spinner, notif.message);
            let toast_h = lh + 4.0;
            y -= toast_h + 4.0;

            unsafe {
                self.rt.FillRectangle(&rect_f(x, y, toast_w, toast_h), &bg);
                self.rt
                    .DrawRectangle(&rect_f(x, y, toast_w, toast_h), &border, 1.0, None);
            }

            let color = if notif.done {
                self.theme.git_added
            } else {
                self.theme.foreground
            };
            self.draw_text(&text, x + cw * 0.5, y + 2.0, color);
        }
    }

    fn draw_settings_panel(
        &self,
        engine: &crate::core::engine::Engine,
        panel_x: f32,
        panel_w: f32,
        _panel_h: f32,
        top: f32,
    ) {
        use crate::core::engine::SettingsRow;
        use crate::core::settings::{setting_categories, SettingType, SETTING_DEFS};

        let lh = self.line_height;
        let cw = self.char_width;
        let fg = self.theme.foreground;
        let dim_fg = self.theme.line_number_fg;
        let key_fg = self.theme.keyword;
        let cat_fg = self.theme.keyword;
        let sel_bg = if engine.settings_has_focus {
            self.theme.sidebar_sel_bg
        } else {
            self.theme.sidebar_sel_bg_inactive
        };

        // Row 0: Header
        let header_bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x, top, panel_w, lh), &header_bg);
        }
        self.draw_text(" SETTINGS", panel_x + cw * 0.5, top, self.theme.status_fg);

        // Row 1: Search input
        let search_y = top + lh;
        let search_bg = if engine.settings_input_active {
            sel_bg
        } else {
            self.theme.tab_bar_bg
        };
        let sb = self.solid_brush(search_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x, search_y, panel_w, lh), &sb);
        }
        let mut search_text = format!(" / {}", engine.settings_query);
        if engine.settings_input_active {
            search_text.push('█');
        }
        self.draw_text(&search_text, panel_x + cw * 0.5, search_y, dim_fg);

        // Rows 2+: scrollable form
        let content_y = top + lh * 2.0;
        let flat = engine.settings_flat_list();
        let cats = setting_categories();
        let scroll = engine.settings_scroll_top;
        let max_rows = (((_panel_h - lh * 2.0) / lh).floor() as usize).max(1);
        let right_edge = panel_x + panel_w - cw; // leave room for scrollbar

        for vi in 0..max_rows {
            let fi = scroll + vi;
            if fi >= flat.len() {
                break;
            }
            let y = content_y + vi as f32 * lh;
            let row = &flat[fi];
            let is_selected = fi == engine.settings_selected && engine.settings_has_focus;

            if is_selected {
                let sb = self.solid_brush(sel_bg);
                unsafe {
                    self.rt.FillRectangle(&rect_f(panel_x, y, panel_w, lh), &sb);
                }
            }

            match row {
                SettingsRow::CoreCategory(cat_idx) => {
                    let collapsed = *cat_idx < engine.settings_collapsed.len()
                        && engine.settings_collapsed[*cat_idx];
                    let arrow = if collapsed { "\u{25B6}" } else { "\u{25BC}" };
                    let cat_name = cats.get(*cat_idx).copied().unwrap_or("?");
                    let text = format!(" {} {}", arrow, cat_name);
                    self.draw_text(&text, panel_x + cw, y, cat_fg);
                }
                SettingsRow::ExtCategory(name) => {
                    let collapsed = engine
                        .ext_settings_collapsed
                        .get(name)
                        .copied()
                        .unwrap_or(false);
                    let arrow = if collapsed { "\u{25B6}" } else { "\u{25BC}" };
                    let display = engine
                        .ext_available_manifests()
                        .into_iter()
                        .find(|m| &m.name == name)
                        .map(|m| m.display_name.clone())
                        .unwrap_or_else(|| name.clone());
                    let text = format!(" {} {}", arrow, display);
                    self.draw_text(&text, panel_x + cw, y, cat_fg);
                }
                SettingsRow::CoreSetting(idx) => {
                    let def = &SETTING_DEFS[*idx];
                    // Label on the left
                    self.draw_text(def.label, panel_x + cw * 3.0, y, fg);

                    let editing_this = engine.settings_editing == Some(*idx);

                    // Value on the right
                    let val_text = match &def.setting_type {
                        SettingType::Bool => {
                            let val = engine.settings.get_value_str(def.key);
                            if val == "true" {
                                "[\u{2713}]".to_string()
                            } else {
                                "[ ]".to_string()
                            }
                        }
                        SettingType::Integer { .. } => {
                            if editing_this {
                                format!("{}\u{2588}", engine.settings_edit_buf)
                            } else {
                                engine.settings.get_value_str(def.key)
                            }
                        }
                        SettingType::Enum(_) | SettingType::DynamicEnum(_) => {
                            format!("{} \u{25B8}", engine.settings.get_value_str(def.key))
                        }
                        SettingType::StringVal => {
                            if editing_this {
                                format!("{}\u{2588}", engine.settings_edit_buf)
                            } else {
                                let val = engine.settings.get_value_str(def.key);
                                if val.is_empty() {
                                    "(empty)".to_string()
                                } else {
                                    val
                                }
                            }
                        }
                        SettingType::BufferEditor => match def.key {
                            "keymaps" => {
                                format!("{} defined \u{25B8}", engine.settings.keymaps.len())
                            }
                            "extension_registries" => {
                                format!(
                                    "{} configured \u{25B8}",
                                    engine.settings.extension_registries.len()
                                )
                            }
                            _ => "\u{25B8}".to_string(),
                        },
                    };
                    let val_w = val_text.chars().count() as f32 * cw;
                    let vx = (right_edge - val_w).max(panel_x + cw * 3.0);
                    let val_color = if editing_this { fg } else { key_fg };
                    self.draw_text(&val_text, vx, y, val_color);
                }
                SettingsRow::ExtSetting(ext_name, ext_key) => {
                    let def = engine.find_ext_setting_def(ext_name, ext_key);
                    let label = def
                        .as_ref()
                        .map(|d| d.label.as_str())
                        .unwrap_or(ext_key.as_str());
                    self.draw_text(label, panel_x + cw * 3.0, y, fg);

                    let val = engine.get_ext_setting(ext_name, ext_key);
                    let typ = def.as_ref().map(|d| d.r#type.as_str()).unwrap_or("string");
                    let val_text = match typ {
                        "bool" => {
                            if val == "true" {
                                "[\u{2713}]".to_string()
                            } else {
                                "[ ]".to_string()
                            }
                        }
                        _ => {
                            if val.is_empty() {
                                "(empty)".to_string()
                            } else {
                                val
                            }
                        }
                    };
                    let val_w = val_text.chars().count() as f32 * cw;
                    let vx = (right_edge - val_w).max(panel_x + cw * 3.0);
                    self.draw_text(&val_text, vx, y, key_fg);
                }
            }
        }
    }

    // ─── Extension panel (plugin-provided dynamic panels) ──────────────────

    fn draw_ext_panel(
        &self,
        screen: &ScreenLayout,
        engine: &crate::core::engine::Engine,
        panel_x: f32,
        panel_w: f32,
        panel_h: f32,
        top: f32,
    ) {
        use crate::core::plugin::ExtPanelStyle;

        let Some(ref panel) = screen.ext_panel else {
            return;
        };

        let lh = self.line_height;
        let bg = self.theme.tab_bar_bg;
        let fg = self.theme.foreground;
        let dim = self.theme.line_number_fg;
        let accent = self.theme.keyword;
        let sel_bg = self.theme.fuzzy_selected_bg;
        let hdr_bg = self.theme.status_bg;
        let hdr_fg = self.theme.status_fg;

        // Background
        let bg_brush = self.solid_brush(bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x, top, panel_w, panel_h), &bg_brush);
        }

        let mut ry: f32 = 0.0;

        // ── Row 0: panel header ─────────────────────────────────────────
        let hdr_brush = self.solid_brush(hdr_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(panel_x, top + ry, panel_w, lh), &hdr_brush);
        }
        let hdr_text = format!("  {}", panel.title);
        self.draw_text(&hdr_text, panel_x + 2.0, top + ry, hdr_fg);
        ry += lh;
        if ry >= panel_h {
            return;
        }

        // ── Search input row (when active or has text) ──────────────────
        if panel.input_active || !panel.input_text.is_empty() {
            self.draw_text(" / ", panel_x, top + ry, dim);
            let prefix_w = self.mono_text_width(" / ");
            let input_color = if panel.input_active { fg } else { dim };
            self.draw_text(&panel.input_text, panel_x + prefix_w, top + ry, input_color);
            if panel.input_active {
                let tw = self.mono_text_width(&panel.input_text);
                let cursor_brush = self.solid_brush(fg);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(panel_x + prefix_w + tw, top + ry, 1.5, lh),
                        &cursor_brush,
                    );
                }
            }
            ry += lh;
            if ry >= panel_h {
                return;
            }
        }

        // ── Build flat list of rows ─────────────────────────────────────
        struct FlatRow {
            text: String,
            hint: String,
            is_header: bool,
            style: ExtPanelStyle,
            is_separator: bool,
            badges: Vec<crate::core::plugin::ExtPanelBadge>,
            actions: Vec<crate::core::plugin::ExtPanelAction>,
        }
        let mut flat_rows: Vec<FlatRow> = Vec::new();
        for section in &panel.sections {
            let arrow = if section.expanded {
                "\u{25BC}"
            } else {
                "\u{25B6}"
            };
            flat_rows.push(FlatRow {
                text: format!(" {} {}", arrow, section.name),
                hint: String::new(),
                is_header: true,
                style: ExtPanelStyle::Header,
                is_separator: false,
                badges: Vec::new(),
                actions: Vec::new(),
            });
            if section.expanded {
                for item in &section.items {
                    if item.is_separator {
                        flat_rows.push(FlatRow {
                            text: String::new(),
                            hint: String::new(),
                            is_header: false,
                            style: ExtPanelStyle::Dim,
                            is_separator: true,
                            badges: Vec::new(),
                            actions: Vec::new(),
                        });
                        continue;
                    }
                    let indent = "  ".repeat(item.indent as usize + 1);
                    let chevron = if item.expandable {
                        if item.expanded {
                            "\u{25BC} "
                        } else {
                            "\u{25B6} "
                        }
                    } else {
                        ""
                    };
                    let icon_part = if item.icon.is_empty() {
                        String::new()
                    } else {
                        format!("{} ", item.icon)
                    };
                    flat_rows.push(FlatRow {
                        text: format!("{}{}{}{}", indent, chevron, icon_part, item.text),
                        hint: item.hint.clone(),
                        style: item.style,
                        is_header: false,
                        is_separator: false,
                        badges: item.badges.clone(),
                        actions: item.actions.clone(),
                    });
                }
            }
        }

        // ── Render visible rows ─────────────────────────────────────────
        let content_h = panel_h - ry;
        let max_rows = (content_h / lh) as usize;
        let scroll = panel.scroll_top;
        let visible_start = scroll.min(flat_rows.len());

        for (ri, row) in flat_rows[visible_start..].iter().enumerate().take(max_rows) {
            let row_y = top + ry + ri as f32 * lh;
            let is_sel = (scroll + ri) == panel.selected;

            // Separator (thin horizontal line)
            if row.is_separator {
                let sep_y = row_y + lh / 2.0;
                let dim_brush = self.solid_brush(dim);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(panel_x + 8.0, sep_y, panel_w - 16.0, 1.0),
                        &dim_brush,
                    );
                }
                continue;
            }

            // Selection highlight
            if is_sel && panel.has_focus {
                let sel_brush = self.solid_brush(sel_bg);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(panel_x, row_y, panel_w, lh), &sel_brush);
                }
            }

            // Choose foreground color based on style
            let text_color = if row.is_header {
                fg
            } else {
                match row.style {
                    ExtPanelStyle::Header => fg,
                    ExtPanelStyle::Dim => dim,
                    ExtPanelStyle::Accent => accent,
                    ExtPanelStyle::Normal => fg,
                }
            };

            // Measure right-side decorations width first
            let mut right_w: f32 = 0.0;
            if !row.hint.is_empty() {
                right_w += self.mono_text_width(&row.hint) + 4.0;
            }
            if is_sel && panel.has_focus {
                for action in &row.actions {
                    right_w += self.mono_text_width(&format!(" {} ", action.label)) + 4.0;
                }
            }
            for badge in &row.badges {
                right_w += self.mono_text_width(&format!(" {} ", badge.text)) + 4.0;
            }

            // Draw row text — truncate to fit before right decorations
            let max_text_w = (panel_w - 6.0 - right_w).max(20.0);
            let text_chars = row.text.chars().count() as f32 * self.char_width;
            if text_chars <= max_text_w {
                self.draw_text(&row.text, panel_x + 2.0, row_y, text_color);
            } else {
                // Truncate with ellipsis
                let max_chars = ((max_text_w / self.char_width) as usize).saturating_sub(1);
                let truncated: String = row
                    .text
                    .chars()
                    .take(max_chars)
                    .chain(std::iter::once('\u{2026}'))
                    .collect();
                self.draw_text(&truncated, panel_x + 2.0, row_y, text_color);
            }

            // Draw right-side decorations from right to left
            let mut rx = panel_x + panel_w - 4.0;

            // Hint (rightmost)
            if !row.hint.is_empty() {
                let hw = self.mono_text_width(&row.hint);
                rx -= hw;
                self.draw_text(&row.hint, rx, row_y, dim);
                rx -= 4.0;
            }

            // Actions (only on selected row)
            if is_sel && panel.has_focus {
                for action in row.actions.iter().rev() {
                    let action_text = format!(" {} ", action.label);
                    let aw = self.mono_text_width(&action_text);
                    rx -= aw;
                    let accent_brush = self.solid_brush(accent);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(rx, row_y + 2.0, aw, lh - 4.0), &accent_brush);
                    }
                    self.draw_text(&action_text, rx, row_y, bg);
                    rx -= 4.0;
                }
            }

            // Badges
            for badge in row.badges.iter().rev() {
                let badge_text = format!(" {} ", badge.text);
                let bw = self.mono_text_width(&badge_text);
                rx -= bw;
                let badge_color = parse_badge_color_d2d(&badge.color).unwrap_or(dim);
                // Semi-transparent background
                let badge_bg = self.solid_brush_alpha(badge_color, 0.25);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(rx, row_y + 2.0, bw, lh - 4.0), &badge_bg);
                }
                self.draw_text(&badge_text, rx, row_y, badge_color);
                rx -= 4.0;
            }
        }

        // ── Scrollbar ───────────────────────────────────────────────────
        let total = flat_rows.len();
        if total > max_rows && max_rows > 0 {
            let track_h = content_h;
            let thumb_h = (track_h * max_rows as f32 / total as f32).max(4.0);
            let thumb_top = scroll as f32 * track_h / total as f32;
            let sb_x = panel_x + panel_w - 5.0;
            let dim_brush = self.solid_brush(dim);
            unsafe {
                self.rt.FillRectangle(
                    &rect_f(sb_x, top + ry + thumb_top, 4.0, thumb_h),
                    &dim_brush,
                );
            }
        }

        // ── Help popup overlay ──────────────────────────────────────────
        if panel.help_open && !panel.help_bindings.is_empty() {
            let bindings = &panel.help_bindings;
            let popup_w = panel_w.min(280.0);
            let popup_h = lh * (bindings.len() as f32 + 2.0);
            let popup_x = panel_x + (panel_w - popup_w) / 2.0;
            let popup_y = top + (panel_h - popup_h) / 2.0;

            let popup_bg = self.solid_brush(self.theme.completion_bg);
            let popup_border = self.solid_brush(self.theme.completion_border);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(popup_x, popup_y, popup_w, popup_h), &popup_bg);
                self.rt.DrawRectangle(
                    &rect_f(popup_x, popup_y, popup_w, popup_h),
                    &popup_border,
                    1.0,
                    None,
                );
            }

            // Title + close hint
            self.draw_text(
                "Keybindings",
                popup_x + 8.0,
                popup_y,
                self.theme.completion_fg,
            );
            self.draw_text(
                "x",
                popup_x + popup_w - 16.0,
                popup_y,
                self.theme.completion_fg,
            );

            // Bindings
            for (i, (key, desc)) in bindings.iter().enumerate() {
                let bind_y = popup_y + lh * (i as f32 + 1.0);
                self.draw_text(key, popup_x + 12.0, bind_y, self.theme.function);
                self.draw_text(desc, popup_x + 100.0, bind_y, self.theme.completion_fg);
            }
        }
    }

    // ─── Primitive helpers ───────────────────────────────────────────────────

    // ─── Terminal panel ────────────────────────────────────────────────────

    /// Draw a single terminal cell (background + character + cursor).
    fn draw_terminal_cell(
        &self,
        cell: &crate::render::TerminalCell,
        cx: f32,
        cy: f32,
        show_cursor: bool,
    ) {
        let cw = self.char_width;
        let lh = self.line_height;

        // Cell background
        let has_custom_bg =
            cell.bg != (0, 0, 0) || cell.selected || cell.is_find_match || cell.is_find_active;
        if has_custom_bg {
            let bg_color = if cell.is_find_active {
                self.theme.search_match_fg
            } else if cell.is_find_match {
                self.theme.search_match_bg
            } else if cell.selected {
                self.theme.selection
            } else {
                Color::from_rgb(cell.bg.0, cell.bg.1, cell.bg.2)
            };
            let bg_brush = self.solid_brush(bg_color);
            unsafe {
                self.rt.FillRectangle(&rect_f(cx, cy, cw, lh), &bg_brush);
            }
        }

        // Cell character
        if cell.ch != ' ' && cell.ch != '\0' {
            let fg_color = Color::from_rgb(cell.fg.0, cell.fg.1, cell.fg.2);
            let mut buf = [0u8; 4];
            let s = cell.ch.encode_utf8(&mut buf);
            self.draw_text(s, cx, cy, fg_color);
        }

        // Cursor
        if cell.is_cursor && show_cursor {
            let cursor_brush = self.solid_brush_alpha(self.theme.cursor, 0.7);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(cx, cy, cw, lh), &cursor_brush);
            }
        }
    }

    fn draw_terminal(&self, term: &crate::render::TerminalPanel, layout: &ScreenLayout) {
        let lh = self.line_height;
        let cw = self.char_width;
        let (width, height) = self.rt_size();

        // Below-terminal rows: when status_line_above_terminal is active,
        // the separated status + cmd are above the terminal, so only cmd is below.
        // When per-window status lines are active (no global status bar), only cmd is below.
        let below_rows = if layout.separated_status_line.is_some() {
            0.0 // status+cmd are above the terminal
        } else if layout.status_left.is_empty() && layout.status_right.is_empty() {
            1.0 // per-window status: only cmd line below
        } else {
            2.0 // global status bar + cmd line below
        };
        let above_rows = if layout.separated_status_line.is_some() {
            2.0 // status + cmd above terminal
        } else {
            0.0
        };
        let total_rows = term.content_rows as f32 + 1.0; // +1 for toolbar
        let panel_y = height - (total_rows + above_rows + below_rows) * lh;

        // Toolbar background
        let toolbar_bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(self.editor_left, panel_y, width - self.editor_left, lh),
                &toolbar_bg,
            );
        }

        // Toolbar: tab labels + buttons
        let mut tx = self.editor_left + cw;
        let nf = crate::icons::nerd_fonts_enabled();

        // Terminal tabs
        for i in 0..term.tab_count {
            let label = if i == term.active_tab {
                format!(" {} Terminal {} ", if nf { "\u{f120}" } else { "$" }, i + 1)
            } else {
                format!(" {} ", i + 1)
            };
            let is_active = i == term.active_tab;
            let fg = if is_active {
                self.theme.foreground
            } else {
                self.theme.line_number_fg
            };
            self.draw_text(&label, tx, panel_y, fg);
            tx += label.chars().count() as f32 * cw;
        }

        // Toolbar buttons (right-aligned): + split max × ...
        {
            let nf = crate::icons::nerd_fonts_enabled();
            let btn_close = if nf { "×" } else { "x" };
            let btn_split = if nf { "󰤼" } else { "⊞" };
            let btn_max = if term.maximized {
                if nf {
                    "󰊓"
                } else {
                    "["
                }
            } else if nf {
                "󰊗"
            } else {
                "]"
            };
            let btn_add = "+";
            // Draw right-to-left: × max split +
            let mut bx = width - cw * 2.0;
            self.draw_text(btn_close, bx, panel_y, self.theme.line_number_fg);
            bx -= cw * 2.0;
            self.draw_text(btn_max, bx, panel_y, self.theme.line_number_fg);
            bx -= cw * 2.0;
            self.draw_text(btn_split, bx, panel_y, self.theme.line_number_fg);
            bx -= cw * 2.0;
            self.draw_text(btn_add, bx, panel_y, self.theme.line_number_fg);
        }

        // Content background
        let content_y = panel_y + lh;
        let content_h = term.content_rows as f32 * lh;
        let term_bg_brush = self.solid_brush(self.theme.terminal_bg);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(
                    self.editor_left,
                    content_y,
                    width - self.editor_left,
                    content_h,
                ),
                &term_bg_brush,
            );
        }

        // Draw terminal cells — either split (two panes + divider) or single
        if let Some(ref left_rows) = term.split_left_rows {
            let split_cols = term.split_left_cols as f32;
            let div_x = self.editor_left + split_cols * cw;

            // Left pane cells
            for (row_idx, row) in left_rows.iter().enumerate() {
                let cy = content_y + row_idx as f32 * lh;
                if cy + lh > height - below_rows * lh {
                    break;
                }
                for (col_idx, cell) in row.iter().enumerate() {
                    let cx = self.editor_left + col_idx as f32 * cw;
                    if cx + cw > div_x {
                        break;
                    }
                    self.draw_terminal_cell(cell, cx, cy, term.has_focus && term.split_focus == 0);
                }
            }

            // Divider
            let div_brush = self.solid_brush(self.theme.separator);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(div_x, content_y, 1.0, content_h), &div_brush);
            }

            // Right pane cells
            let right_x = div_x + cw; // skip divider column
            for (row_idx, row) in term.rows.iter().enumerate() {
                let cy = content_y + row_idx as f32 * lh;
                if cy + lh > height - below_rows * lh {
                    break;
                }
                for (col_idx, cell) in row.iter().enumerate() {
                    let cx = right_x + col_idx as f32 * cw;
                    if cx + cw > width {
                        break;
                    }
                    self.draw_terminal_cell(cell, cx, cy, term.has_focus && term.split_focus == 1);
                }
            }
        } else {
            // Single pane
            for (row_idx, row) in term.rows.iter().enumerate() {
                let cy = content_y + row_idx as f32 * lh;
                if cy + lh > height - below_rows * lh {
                    break;
                }
                for (col_idx, cell) in row.iter().enumerate() {
                    let cx = self.editor_left + col_idx as f32 * cw;
                    if cx + cw > width {
                        break;
                    }
                    self.draw_terminal_cell(cell, cx, cy, term.has_focus);
                }
            }
        }

        // Find bar (if active)
        if term.find_active {
            let find_y = content_y;
            let find_w = 250.0f32.min(width - self.editor_left - cw);
            let find_x = width - find_w - cw;
            let find_bg = self.solid_brush(self.theme.tab_bar_bg);
            let find_border = self.solid_brush(self.theme.separator);
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(find_x, find_y, find_w, lh), &find_bg);
                self.rt
                    .DrawRectangle(&rect_f(find_x, find_y, find_w, lh), &find_border, 1.0, None);
            }
            let query_display = if term.find_query.is_empty() {
                "Find..."
            } else {
                &term.find_query
            };
            let query_fg = if term.find_query.is_empty() {
                self.theme.line_number_fg
            } else {
                self.theme.foreground
            };
            self.draw_text(query_display, find_x + cw * 0.5, find_y, query_fg);

            // Match count
            if !term.find_query.is_empty() {
                let count_text =
                    format!("{}/{}", term.find_selected_idx + 1, term.find_match_count);
                let count_x = find_x + find_w - (count_text.len() as f32 + 1.0) * cw;
                self.draw_text(&count_text, count_x, find_y, self.theme.line_number_fg);
            }
        }
    }

    // ─── Menu bar ─────────────────────────────────────────────────────────

    fn draw_menu_bar(&self, data: &MenuBarData) {
        let (width, _) = self.rt_size();
        let lh = self.line_height;
        let top = super::TITLE_BAR_TOP_INSET;
        let bar_h = lh * super::TITLE_BAR_HEIGHT_MULT;
        let cw = self.char_width;
        // Title/menu bar uses a dark background (like VSCode's title bar),
        // not the status bar color which can be bright blue.
        let title_bg = self.theme.tab_bar_bg;
        let bar_bg = self.solid_brush(title_bg);
        let bar_fg = self.theme.foreground;
        let dim_fg = self.theme.line_number_fg;
        // Vertical offset to center text within the taller title bar (below top inset)
        let text_y = top + (bar_h - lh) / 2.0;

        // Background (fill from top inset)
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, top, width, bar_h), &bar_bg);
        }

        // Menu labels (proportional UI font)
        let pad = 8.0; // horizontal padding around each label
        let mut x = pad;
        for (idx, (name, _, _)) in MENU_STRUCTURE.iter().enumerate() {
            let is_open = data.open_menu_idx == Some(idx);
            let label_w = self.measure_ui_text(name) + pad * 2.0;

            if is_open {
                // Highlight background for open menu (subtle lighter bg)
                let hl_brush = self.solid_brush(title_bg.lighten(0.15));
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(x, top, label_w, bar_h), &hl_brush);
                }
                self.draw_ui_text(name, x + pad, text_y, bar_h, bar_fg);
            } else {
                self.draw_ui_text(name, x + pad, text_y, bar_h, bar_fg);
            }
            x += label_w;
        }

        // Center: nav arrows + command center search box
        let menu_end = x;
        let arrows_w = 4.0 * cw; // "◀ ▶ "
        let title_display = if data.title.is_empty() {
            String::new()
        } else {
            format!("\u{1f50d} {}", data.title)
        };
        let text_len = title_display.chars().count() as f32;
        let box_w = if !title_display.is_empty() {
            (text_len + 4.0) * cw
        } else {
            0.0
        };
        let gap = if box_w > 0.0 { cw } else { 0.0 };
        let total_unit = arrows_w + gap + box_w;
        // Reserve space for caption buttons (min/max/close) at the right edge
        let caption_btns_w = super::CAPTION_BTN_COUNT * super::CAPTION_BTN_WIDTH;
        let available = width - menu_end - caption_btns_w;

        if available >= total_unit + 2.0 * cw {
            let unit_start = menu_end + (available - total_unit) / 2.0;

            // Back arrow
            let back_color = if data.nav_back_enabled {
                bar_fg
            } else {
                dim_fg
            };
            self.draw_text("◀", unit_start, text_y, back_color);
            let fwd_color = if data.nav_forward_enabled {
                bar_fg
            } else {
                dim_fg
            };
            self.draw_text("▶", unit_start + 2.0 * cw, text_y, fwd_color);

            // Search box
            if !title_display.is_empty() {
                let bx = unit_start + arrows_w + gap;
                self.draw_text("[", bx, text_y, dim_fg);
                self.draw_text(&title_display, bx + cw, text_y, bar_fg);
                let end_x = bx + (text_len + 3.0) * cw;
                self.draw_text("]", end_x, text_y, dim_fg);
            }
        }
    }

    /// Draw minimize / maximize / close buttons at the right edge of the title bar row.
    fn draw_caption_buttons(&self) {
        let (width, _) = self.rt_size();
        let lh = self.line_height;
        let top = super::TITLE_BAR_TOP_INSET;
        let bar_h = lh * super::TITLE_BAR_HEIGHT_MULT;
        let btn_w = super::CAPTION_BTN_WIDTH;
        let btn_count = super::CAPTION_BTN_COUNT as usize;
        let btn_start = width - btn_w * btn_count as f32;

        let fg = self.theme.foreground;

        for i in 0..btn_count {
            let x = btn_start + i as f32 * btn_w;

            // Hover highlight
            if self.caption_hover == Some(i) {
                let hover_color = if i == 2 {
                    // Close button: red hover
                    Color::from_rgb(232, 17, 35)
                } else {
                    self.theme.tab_bar_bg.lighten(0.15)
                };
                let brush = self.solid_brush(hover_color);
                unsafe {
                    self.rt.FillRectangle(&rect_f(x, top, btn_w, bar_h), &brush);
                }
            }

            // Button icon (centered in the button rect)
            let icon_color = if i == 2 && self.caption_hover == Some(2) {
                Color::from_rgb(255, 255, 255) // white on red for close hover
            } else {
                fg
            };
            let icon = match i {
                0 => "\u{2500}", // ─ minimize
                1 => {
                    if self.is_maximized {
                        "\u{25A3}" // ▣ restore
                    } else {
                        "\u{25A1}" // □ maximize
                    }
                }
                _ => "\u{2715}", // ✕ close
            };
            // Center the icon in the button
            let icon_w = self.char_width;
            let ix = x + (btn_w - icon_w) / 2.0;
            let iy = top + (bar_h - lh) / 2.0;
            self.draw_text(icon, ix, iy, icon_color);
        }
    }

    pub(super) fn draw_menu_dropdown(&self, data: &MenuBarData) {
        let Some(midx) = data.open_menu_idx else {
            return;
        };
        if data.open_items.is_empty() {
            return;
        }

        let cw = self.char_width;
        let lh = self.line_height;
        let popup_bg = self.theme.background.lighten(0.10);
        let popup_fg = self.theme.foreground;
        let sep_fg = self.theme.line_number_fg;

        // Compute dropdown position
        let max_label = data
            .open_items
            .iter()
            .map(|i| i.label.len())
            .max()
            .unwrap_or(4);
        let max_shortcut = data
            .open_items
            .iter()
            .map(|i| {
                if data.is_vscode_mode && !i.vscode_shortcut.is_empty() {
                    i.vscode_shortcut.len()
                } else {
                    i.shortcut.len()
                }
            })
            .max()
            .unwrap_or(0);
        let popup_chars = (max_label + max_shortcut + 6).clamp(20, 50);
        let popup_w = popup_chars as f32 * cw;
        let popup_h = (data.open_items.len() as f32 + 1.0) * lh; // +1 for padding

        // X position: under the menu label
        let mut label_x = cw; // matches draw_menu_bar left padding
        for i in 0..midx {
            if let Some((name, _, _)) = MENU_STRUCTURE.get(i) {
                label_x += (name.len() as f32 + 2.0) * cw; // " Name "
            }
        }
        let (width, _) = self.rt_size();
        let popup_x = label_x.min(width - popup_w);
        let popup_y = super::TITLE_BAR_TOP_INSET + lh * super::TITLE_BAR_HEIGHT_MULT; // just below title/menu bar

        // Background + border
        let bg_brush = self.solid_brush(popup_bg);
        let border_brush = self.solid_brush_alpha(self.theme.foreground, 0.3);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(popup_x, popup_y, popup_w, popup_h), &bg_brush);
            self.rt.DrawRectangle(
                &rect_f(popup_x, popup_y, popup_w, popup_h),
                &border_brush,
                1.0,
                None,
            );
        }

        // Draw items
        let mut iy = popup_y + lh * 0.25; // small top padding
        for (item_idx, item) in data.open_items.iter().enumerate() {
            let is_highlighted = data.highlighted_item_idx == Some(item_idx);

            if item.separator {
                let sep_brush = self.solid_brush_alpha(sep_fg, 0.5);
                unsafe {
                    self.rt.FillRectangle(
                        &rect_f(popup_x + cw, iy + lh * 0.45, popup_w - 2.0 * cw, 1.0),
                        &sep_brush,
                    );
                }
            } else {
                if is_highlighted {
                    let hl_brush = self.solid_brush_alpha(self.theme.cursor, 0.3);
                    unsafe {
                        self.rt.FillRectangle(
                            &rect_f(popup_x + 1.0, iy, popup_w - 2.0, lh),
                            &hl_brush,
                        );
                    }
                }

                let fg = if item.enabled { popup_fg } else { sep_fg };
                self.draw_text(item.label, popup_x + 2.0 * cw, iy, fg);

                // Right-aligned shortcut
                let shortcut = if data.is_vscode_mode && !item.vscode_shortcut.is_empty() {
                    item.vscode_shortcut
                } else {
                    item.shortcut
                };
                if !shortcut.is_empty() {
                    let sc_x = popup_x + popup_w - (shortcut.len() as f32 + 2.0) * cw;
                    self.draw_text(shortcut, sc_x, iy, sep_fg);
                }
            }
            iy += lh;
        }
    }

    // ─── Text helpers ───────────────────────────────────────────────────────

    pub(super) fn draw_text(&self, text: &str, x: f32, y: f32, color: Color) {
        if text.is_empty() {
            return;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let brush = self.solid_brush(color);
        unsafe {
            self.rt.DrawText(
                &wide,
                self.format,
                &rect_f(x, y, 10000.0, self.line_height),
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Draw text using the proportional UI font (Segoe UI) for menus and tabs.
    pub(super) fn draw_ui_text(&self, text: &str, x: f32, y: f32, height: f32, color: Color) {
        if text.is_empty() {
            return;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let brush = self.solid_brush(color);
        unsafe {
            self.rt.DrawText(
                &wide,
                self.ui_format,
                &rect_f(x, y, 10000.0, height),
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Draw text using the icon font (Nerd Font), centered in a box.
    fn draw_icon_text(&self, text: &str, x: f32, y: f32, width: f32, height: f32, color: Color) {
        if text.is_empty() {
            return;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let brush = self.solid_brush(color);
        unsafe {
            self.rt.DrawText(
                &wide,
                self.icon_format,
                &rect_f(x, y, width, height),
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Measure text width using the UI font.
    pub(super) fn measure_ui_text(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout: IDWriteTextLayout = self
                .dwrite
                .CreateTextLayout(&wide, self.ui_format, 10000.0, 1000.0)
                .expect("CreateTextLayout");
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).expect("GetMetrics");
            metrics.width
        }
    }

    /// Approximate monospace text width using char_width × char count.
    pub(super) fn mono_text_width(&self, text: &str) -> f32 {
        self.char_width * text.chars().count() as f32
    }

    pub(super) fn solid_brush(&self, c: Color) -> ID2D1SolidColorBrush {
        unsafe {
            self.rt
                .CreateSolidColorBrush(&color_f(c), None)
                .expect("CreateSolidColorBrush")
        }
    }

    fn solid_brush_alpha(&self, c: Color, alpha: f32) -> ID2D1SolidColorBrush {
        unsafe {
            self.rt
                .CreateSolidColorBrush(&color_f_alpha(c, alpha), None)
                .expect("CreateSolidColorBrush")
        }
    }

    fn rt_size(&self) -> (f32, f32) {
        unsafe {
            let size = self.rt.GetSize();
            (size.width, size.height)
        }
    }
}

/// Safe byte-slice of a string, clamped to char boundaries.
fn safe_slice(s: &str, start: usize, end: usize) -> &str {
    let start = start.min(s.len());
    let end = end.min(s.len());
    // Back up to char boundaries
    let start = (0..=start)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    let end = (end..=s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len());
    &s[start..end]
}
