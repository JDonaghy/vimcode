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
    pub theme: &'a Theme,
    pub char_width: f32,
    pub line_height: f32,
    /// Left edge of the editor area (sidebar width offset).
    pub editor_left: f32,
    /// Which caption button (0=min, 1=max, 2=close) is hovered, or None.
    pub caption_hover: Option<usize>,
    /// Whether the window is currently maximized (affects the max/restore icon).
    pub is_maximized: bool,
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
                    self.draw_group_tab_bar(gtb, is_active);
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

            // Draw terminal panel
            if let Some(ref term) = layout.bottom_tabs.terminal {
                self.draw_terminal(term);
            }

            // Draw picker (command palette / fuzzy finder)
            if let Some(ref picker) = layout.picker {
                self.draw_picker(picker);
            }

            // Draw tab switcher
            if let Some(ref ts) = layout.tab_switcher {
                self.draw_tab_switcher(ts);
            }

            // Draw context menu
            if let Some(ref ctx) = layout.context_menu {
                self.draw_context_menu(ctx);
            }

            // NOTE: menu dropdown drawn separately after sidebar (see on_paint)

            // Draw dialog (on top of everything)
            if let Some(ref dialog) = layout.dialog {
                self.draw_dialog(dialog);
            }
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
        self.draw_tabs(&layout.tab_bar, x, text_y, width - x);
    }

    fn draw_group_tab_bar(&self, gtb: &crate::render::GroupTabBar, is_active_group: bool) {
        let h = self.line_height * super::TAB_BAR_HEIGHT_MULT;
        let x = gtb.bounds.x as f32;
        let y = gtb.bounds.y as f32 - h; // tab bar sits above the group content
        let w = gtb.bounds.width as f32;

        let _ = is_active_group; // reserved for future per-group styling
        let bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, w, h), &bg);
        }
        let text_y = y + (h - self.line_height) / 2.0;
        self.draw_tabs(&gtb.tabs, x, text_y, w);
    }

    fn draw_tabs(&self, tabs: &[crate::render::TabInfo], x_origin: f32, y: f32, _max_width: f32) {
        let tab_h = self.line_height * super::TAB_BAR_HEIGHT_MULT;
        let mut x = x_origin;
        let pad = 12.0; // horizontal padding inside each tab

        for tab in tabs {
            let bg = if tab.active {
                self.solid_brush(self.theme.active_background)
            } else {
                self.solid_brush(self.theme.tab_bar_bg)
            };
            let fg_color = if tab.active {
                if tab.preview {
                    self.theme.line_number_fg // dimmer for preview tabs
                } else {
                    self.theme.foreground
                }
            } else {
                self.theme.line_number_fg
            };

            let name_w = self.measure_ui_text(&tab.name);
            let close_w = self.char_width; // close button width
            let tab_w = pad + name_w + pad + close_w + pad * 0.5;

            unsafe {
                self.rt.FillRectangle(&rect_f(x, y, tab_w, tab_h), &bg);
            }

            // Tab name (UI font, vertically centered)
            self.draw_ui_text(&tab.name, x + pad, y, tab_h, fg_color);

            // Dirty indicator (dot) or close button (x)
            let close_x = x + tab_w - close_w - pad * 0.5;
            if tab.dirty {
                self.draw_text(
                    "\u{25CF}",
                    close_x,
                    y + (tab_h - self.line_height) / 2.0,
                    self.theme.git_modified,
                );
            } else {
                self.draw_text(
                    "\u{00D7}",
                    close_x,
                    y + (tab_h - self.line_height) / 2.0,
                    self.theme.line_number_fg,
                );
            }

            // Active tab accent bar (2px at top)
            if tab.active {
                let accent = self.solid_brush(self.theme.tab_active_accent);
                unsafe {
                    self.rt.FillRectangle(&rect_f(x, y, tab_w, 2.0), &accent);
                }
            }

            // Tab separator
            unsafe {
                let sep = self.solid_brush(self.theme.separator);
                self.rt
                    .FillRectangle(&rect_f(x + tab_w - 1.0, y + 4.0, 1.0, tab_h - 8.0), &sep);
            }

            x += tab_w;
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

        // Background fill for the window
        let bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(rx, ry, rw.rect.width as f32, rw.rect.height as f32),
                &bg,
            );
        }

        let gutter_chars = rw.gutter_char_width;
        let gutter_px = gutter_chars as f32 * self.char_width;

        for (row_idx, line) in rw.lines.iter().enumerate() {
            let line_y = ry + (row_idx as f32) * self.line_height;

            // Current line highlight
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

            // Gutter (line numbers)
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

            // Git gutter indicator
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

            // Selection highlight
            if let Some(ref sel) = rw.selection {
                self.draw_selection_for_line(rw, line, row_idx, sel, rx + gutter_px, line_y);
            }

            // Syntax-highlighted text spans
            self.draw_styled_line(line, rx + gutter_px, line_y);

            // Diagnostic underlines (squiggles)
            for diag in &line.diagnostics {
                self.draw_diagnostic_underline(diag, rx + gutter_px, line_y);
            }

            // Indent guides
            for &guide_col in &line.indent_guides {
                let gx = rx + gutter_px + guide_col as f32 * self.char_width;
                let guide_brush = self.solid_brush_alpha(self.theme.line_number_fg, 0.3);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(gx, line_y, 1.0, self.line_height), &guide_brush);
                }
            }

            // Ghost text (AI completions)
            if let Some(ref ghost) = line.ghost_suffix {
                let text_len = line.raw_text.trim_end_matches('\n').chars().count();
                let gx = rx + gutter_px + text_len as f32 * self.char_width;
                self.draw_text(ghost, gx, line_y, self.theme.line_number_fg);
            }

            // Inline annotation
            if let Some(ref ann) = line.annotation {
                let text_len = line.raw_text.trim_end_matches('\n').chars().count();
                let ax = rx + gutter_px + (text_len as f32 + 2.0) * self.char_width;
                self.draw_text(ann, ax, line_y, self.theme.line_number_fg);
            }
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
                let track_brush = self.solid_brush_alpha(self.theme.line_number_fg, 0.15);
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
                let thumb_brush = self.solid_brush_alpha(self.theme.foreground, 0.35);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(sb_x, thumb_y, sb_width, thumb_h), &thumb_brush);
                }
            }
        }

        // Per-window status line
        if let Some(ref status) = rw.status_line {
            let status_y = ry + rw.rect.height as f32 - self.line_height;
            let status_bg_brush = self.solid_brush(self.theme.status_bg);
            unsafe {
                self.rt.FillRectangle(
                    &rect_f(rx, status_y, rw.rect.width as f32, self.line_height),
                    &status_bg_brush,
                );
            }
            // Left segments
            let mut sx = rx + self.char_width * 0.5;
            for seg in &status.left_segments {
                self.draw_text(&seg.text, sx, status_y, seg.fg);
                sx += seg.text.chars().count() as f32 * self.char_width;
            }
            // Right segments
            let right_text: String = status
                .right_segments
                .iter()
                .map(|s| s.text.as_str())
                .collect();
            let right_w = right_text.chars().count() as f32 * self.char_width;
            let mut sx = rx + rw.rect.width as f32 - right_w - self.char_width * 0.5;
            for seg in &status.right_segments {
                self.draw_text(&seg.text, sx, status_y, seg.fg);
                sx += seg.text.chars().count() as f32 * self.char_width;
            }
        }
    }

    fn draw_styled_line(&self, line: &RenderedLine, x: f32, y: f32) {
        if line.spans.is_empty() {
            // No syntax: draw raw text in default color
            self.draw_text(&line.raw_text, x, y, self.theme.foreground);
            return;
        }

        for span in &line.spans {
            let span_text = safe_slice(&line.raw_text, span.start_byte, span.end_byte);
            if span_text.is_empty() {
                continue;
            }
            // Compute character offset from byte offset for positioning
            let char_offset = line.raw_text[..span.start_byte.min(line.raw_text.len())]
                .chars()
                .count();
            let sx = x + char_offset as f32 * self.char_width;
            self.draw_text(span_text, sx, y, span.style.fg);
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
        let cx = rx + gutter_px + pos.col as f32 * self.char_width;
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
        // The terminal panel content row count tells us how much space it uses.
        let terminal_px = _layout
            .bottom_tabs
            .terminal
            .as_ref()
            .map(|t| (t.content_rows as f32 + 2.0) * self.line_height)
            .unwrap_or(0.0);
        // Layout when above: [editor][sep_status][cmd][terminal]
        // sep_y = height - terminal - cmd - sep_status
        let sep_y = height - terminal_px - 2.0 * self.line_height;
        let bar_width = width - x0;
        let bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(x0, sep_y, bar_width, self.line_height), &bg);
        }
        // Left segments
        let mut sx = x0 + self.char_width * 0.5;
        for seg in &status.left_segments {
            self.draw_text(&seg.text, sx, sep_y, seg.fg);
            sx += seg.text.chars().count() as f32 * self.char_width;
        }
        // Right segments
        let right_text: String = status
            .right_segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let right_w = right_text.chars().count() as f32 * self.char_width;
        let mut sx2 = x0 + bar_width - right_w - self.char_width * 0.5;
        for seg in &status.right_segments {
            self.draw_text(&seg.text, sx2, sep_y, seg.fg);
            sx2 += seg.text.chars().count() as f32 * self.char_width;
        }
    }

    // ─── Command line ────────────────────────────────────────────────────────

    fn draw_command_line(&self, layout: &ScreenLayout) {
        let (width, height) = self.rt_size();
        let x0 = self.editor_left;
        let y = if layout.separated_status_line.is_some() {
            // Above terminal: cmd line is right below separated status, above terminal
            let terminal_px = layout
                .bottom_tabs
                .terminal
                .as_ref()
                .map(|t| (t.content_rows as f32 + 2.0) * self.line_height)
                .unwrap_or(0.0);
            height - terminal_px - self.line_height
        } else {
            height - self.line_height
        };
        let bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(x0, y, width - x0, self.line_height), &bg);
        }
        let cmd = &layout.command;
        self.draw_text(
            &cmd.text,
            x0 + self.char_width * 0.5,
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

    fn draw_dialog(&self, dialog: &crate::render::DialogPanel) {
        let (rt_w, rt_h) = self.rt_size();

        // Semi-transparent overlay
        let overlay = self.solid_brush_alpha(self.theme.background, 0.6);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, 0.0, rt_w, rt_h), &overlay);
        }

        // Dialog box
        let dialog_w = 400.0f32.min(rt_w - 40.0);
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

    fn draw_context_menu(&self, ctx: &crate::render::ContextMenuPanel) {
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
    ) {
        let (_, rt_h) = self.rt_size();
        let ab_w = sidebar.activity_bar_px;
        let top = menu_bar_y; // sidebar starts below the menu bar

        // Activity bar background (always drawn)
        let ab_bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, top, ab_w, rt_h - top), &ab_bg);
        }

        // Activity bar icons
        let panels = [
            (SidebarPanel::Explorer, "\u{1F4C1}"),  // folder
            (SidebarPanel::Search, "\u{1F50D}"),    // magnifying glass
            (SidebarPanel::Debug, "\u{1F41B}"),     // bug
            (SidebarPanel::Git, "\u{2442}"),        // branch-like
            (SidebarPanel::Extensions, "\u{2B9E}"), // extension-like
            (SidebarPanel::Ai, "\u{1F4AC}"),        // chat bubble
        ];

        for (i, &(panel, icon)) in panels.iter().enumerate() {
            let y = top + i as f32 * self.line_height;
            let is_active = sidebar.visible && sidebar.active_panel == panel;

            if is_active {
                // Left accent bar
                let accent = self.solid_brush(self.theme.tab_active_accent);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(0.0, y, 2.0, self.line_height), &accent);
                }
                // Active background
                let sel = self.solid_brush(self.theme.active_background);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(2.0, y, ab_w - 2.0, self.line_height), &sel);
                }
            }

            // Icon text (centered in activity bar)
            let icon_x = (ab_w - self.char_width) / 2.0;
            self.draw_text(icon, icon_x, y, self.theme.activity_bar_fg);
        }

        // Settings gear pinned to bottom of activity bar (like TUI/VSCode)
        {
            let y = rt_h - self.line_height;
            let is_active = sidebar.visible && sidebar.active_panel == SidebarPanel::Settings;
            if is_active {
                let accent = self.solid_brush(self.theme.tab_active_accent);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(0.0, y, 2.0, self.line_height), &accent);
                }
                let sel = self.solid_brush(self.theme.active_background);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(2.0, y, ab_w - 2.0, self.line_height), &sel);
                }
            }
            let icon_x = (ab_w - self.char_width) / 2.0;
            self.draw_text("\u{2699}", icon_x, y, self.theme.activity_bar_fg); // gear icon
        }

        // If sidebar panel is not visible, we're done
        if !sidebar.visible {
            return;
        }

        // Panel background
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

        // Clip panel content to prevent bleeding into editor area
        let clip_rect = D2D_RECT_F {
            left: panel_x,
            top,
            right: panel_x + panel_w,
            bottom: rt_h,
        };
        unsafe {
            self.rt
                .PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }

        let panel_h = rt_h - top;
        match sidebar.active_panel {
            SidebarPanel::Explorer => {
                self.draw_explorer_panel(sidebar, panel_x, panel_w, panel_h, top);
            }
            SidebarPanel::Git => self.draw_git_panel(screen, panel_x, panel_w, panel_h, top),
            SidebarPanel::Debug => self.draw_debug_panel(screen, panel_x, panel_w, panel_h, top),
            SidebarPanel::Extensions => {
                self.draw_extensions_panel(screen, panel_x, panel_w, panel_h, top);
            }
            SidebarPanel::Search => self.draw_search_panel(screen, panel_x, panel_w, panel_h, top),
            SidebarPanel::Ai => self.draw_ai_panel(screen, panel_x, panel_w, panel_h, top),
            SidebarPanel::Settings => {
                self.draw_text(
                    "SETTINGS",
                    panel_x + self.char_width,
                    top,
                    self.theme.foreground,
                );
                self.draw_text(
                    "Edit settings.json",
                    panel_x + self.char_width,
                    top + self.line_height * 1.5,
                    self.theme.line_number_fg,
                );
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
        _rt_h: f32,
        top: f32,
    ) {
        let sel_color = if sidebar.has_focus {
            self.theme.sidebar_sel_bg
        } else {
            self.theme.sidebar_sel_bg_inactive
        };
        // Header
        let header_y = top;
        self.draw_text(
            "EXPLORER",
            panel_x + self.char_width,
            header_y,
            self.theme.foreground,
        );

        // File tree rows
        let tree_start_y = top + self.line_height;
        let max_rows = ((_rt_h - tree_start_y) / self.line_height).floor() as usize;

        for (vis_idx, row) in sidebar
            .rows
            .iter()
            .skip(sidebar.scroll_top)
            .take(max_rows)
            .enumerate()
        {
            let actual_idx = sidebar.scroll_top + vis_idx;
            let y = tree_start_y + vis_idx as f32 * self.line_height;

            // Selection highlight
            if actual_idx == sidebar.selected {
                let sel_bg = self.solid_brush(sel_color);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(panel_x, y, panel_w, self.line_height), &sel_bg);
                }
            }

            // Indent + expand arrow
            let indent_px = row.depth as f32 * self.char_width * 1.5;
            let text_x = panel_x + self.char_width + indent_px;

            if row.is_dir {
                let arrow = if row.is_expanded {
                    "\u{25BC}"
                } else {
                    "\u{25B6}"
                };
                self.draw_text(arrow, text_x, y, self.theme.line_number_fg);
                // Dir name after arrow
                self.draw_text(
                    &row.name,
                    text_x + self.char_width * 1.5,
                    y,
                    self.theme.foreground,
                );
            } else {
                // File name (no arrow, offset by arrow width for alignment)
                self.draw_text(
                    &row.name,
                    text_x + self.char_width * 1.5,
                    y,
                    self.theme.explorer_file_fg,
                );
            }
        }
    }

    // ─── Git (Source Control) panel ─────────────────────────────────────────

    fn draw_git_panel(
        &self,
        screen: &ScreenLayout,
        panel_x: f32,
        panel_w: f32,
        _rt_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;

        // Header
        self.draw_text("SOURCE CONTROL", panel_x + cw, top, self.theme.foreground);

        let Some(ref sc) = screen.source_control else {
            self.draw_text(
                "No git repository",
                panel_x + cw,
                top + lh,
                self.theme.line_number_fg,
            );
            return;
        };

        // Branch info
        let branch_text = format!("\u{2442} {} ", sc.branch);
        let ahead_behind = if sc.ahead > 0 || sc.behind > 0 {
            format!("\u{2191}{} \u{2193}{}", sc.ahead, sc.behind)
        } else {
            String::new()
        };
        self.draw_text(&branch_text, panel_x + cw, top + lh, self.theme.foreground);
        self.draw_text(
            &ahead_behind,
            panel_x + cw + branch_text.chars().count() as f32 * cw,
            top + lh,
            self.theme.line_number_fg,
        );

        let mut y = top + lh * 2.5;

        // Staged section
        if sc.sections_expanded[0] {
            let header = format!("\u{25BC} Staged ({})", sc.staged.len());
            self.draw_text(&header, panel_x + cw, y, self.theme.foreground);
            y += lh;
            for item in &sc.staged {
                let color = match item.status_char {
                    'A' => self.theme.git_added,
                    'M' => self.theme.git_modified,
                    'D' => self.theme.git_deleted,
                    _ => self.theme.foreground,
                };
                let line = format!("{} {}", item.status_char, item.path);
                self.draw_text(&line, panel_x + cw * 2.5, y, color);
                y += lh;
            }
        } else {
            let header = format!("\u{25B6} Staged ({})", sc.staged.len());
            self.draw_text(&header, panel_x + cw, y, self.theme.foreground);
            y += lh;
        }

        y += lh * 0.3;

        // Unstaged section
        if sc.sections_expanded[1] {
            let header = format!("\u{25BC} Changes ({})", sc.unstaged.len());
            self.draw_text(&header, panel_x + cw, y, self.theme.foreground);
            y += lh;
            for item in &sc.unstaged {
                let color = match item.status_char {
                    'M' => self.theme.git_modified,
                    'D' => self.theme.git_deleted,
                    '?' => self.theme.line_number_fg,
                    _ => self.theme.foreground,
                };
                let line = format!("{} {}", item.status_char, item.path);
                self.draw_text(&line, panel_x + cw * 2.5, y, color);
                y += lh;
            }
        } else {
            let header = format!("\u{25B6} Changes ({})", sc.unstaged.len());
            self.draw_text(&header, panel_x + cw, y, self.theme.foreground);
            y += lh;
        }

        // Log section (if expanded)
        if sc.sections_expanded.len() > 3 && sc.sections_expanded[3] && !sc.log.is_empty() {
            y += lh * 0.3;
            let header = format!("\u{25BC} Log ({})", sc.log.len());
            self.draw_text(&header, panel_x + cw, y, self.theme.foreground);
            y += lh;
            for entry in sc.log.iter().take(20) {
                let line = format!(
                    "{} {}",
                    &entry.hash[..7.min(entry.hash.len())],
                    entry.message
                );
                self.draw_text(&line, panel_x + cw * 2.5, y, self.theme.line_number_fg);
                y += lh;
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
            for item in &ext.items_installed {
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
            for item in &ext.items_available {
                self.draw_text(&item.name, panel_x + cw * 2.5, y, self.theme.foreground);
                y += lh;
            }
        }
    }

    // ─── Search panel ─────────────────────────────────────────────────────────

    fn draw_search_panel(
        &self,
        _screen: &ScreenLayout,
        panel_x: f32,
        panel_w: f32,
        _rt_h: f32,
        top: f32,
    ) {
        let lh = self.line_height;
        let cw = self.char_width;
        let _ = panel_w;

        self.draw_text("SEARCH", panel_x + cw, top, self.theme.foreground);

        // Search input box placeholder
        let input_y = top + lh * 1.5;
        let input_bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &input_bg,
            );
        }
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt.DrawRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &border,
                1.0,
                None,
            );
        }
        self.draw_text(
            "Search (use :grep)",
            panel_x + cw,
            input_y,
            self.theme.line_number_fg,
        );
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
        let input_bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt.FillRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &input_bg,
            );
        }
        let border = self.solid_brush(self.theme.separator);
        unsafe {
            self.rt.DrawRectangle(
                &rect_f(panel_x + cw * 0.5, input_y, panel_w - cw, lh),
                &border,
                1.0,
                None,
            );
        }
        let input_text = if ai.input.is_empty() {
            "Ask a question..."
        } else {
            &ai.input
        };
        let input_color = if ai.input.is_empty() {
            self.theme.line_number_fg
        } else {
            self.theme.foreground
        };
        self.draw_text(input_text, panel_x + cw, input_y, input_color);
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

    // ─── Primitive helpers ───────────────────────────────────────────────────

    // ─── Terminal panel ────────────────────────────────────────────────────

    fn draw_terminal(&self, term: &crate::render::TerminalPanel) {
        let lh = self.line_height;
        let cw = self.char_width;
        let (width, height) = self.rt_size();

        // Terminal panel sits above status bar + command line (2 rows)
        let total_rows = term.content_rows as f32 + 1.0; // +1 for toolbar
        let panel_y = height - (total_rows + 2.0) * lh; // 2 rows for status+cmd below

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

        // Draw cell grid
        for (row_idx, row) in term.rows.iter().enumerate() {
            let cy = content_y + row_idx as f32 * lh;
            if cy + lh > height - 2.0 * lh {
                break; // don't overdraw into status/cmd
            }
            for (col_idx, cell) in row.iter().enumerate() {
                let cx = self.editor_left + col_idx as f32 * cw;
                if cx + cw > width {
                    break;
                }

                // Cell background (only if non-default or selected/match)
                let has_custom_bg = cell.bg != (0, 0, 0)
                    || cell.selected
                    || cell.is_find_match
                    || cell.is_find_active;
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
                if cell.is_cursor && term.has_focus {
                    let cursor_brush = self.solid_brush_alpha(self.theme.cursor, 0.7);
                    unsafe {
                        self.rt
                            .FillRectangle(&rect_f(cx, cy, cw, lh), &cursor_brush);
                    }
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

    fn draw_text(&self, text: &str, x: f32, y: f32, color: Color) {
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
    fn draw_ui_text(&self, text: &str, x: f32, y: f32, height: f32, color: Color) {
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

    /// Measure text width using the UI font.
    fn measure_ui_text(&self, text: &str) -> f32 {
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

    fn solid_brush(&self, c: Color) -> ID2D1SolidColorBrush {
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
