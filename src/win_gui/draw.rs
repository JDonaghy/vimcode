//! Direct2D rendering of `ScreenLayout`.
//!
//! Consumes the platform-agnostic `ScreenLayout` and paints it onto a
//! Direct2D render target using DirectWrite for text.

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;

use crate::render::{
    Color, CursorShape, RenderedLine, RenderedWindow, ScreenLayout, SelectionKind, Theme,
};

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
    pub theme: &'a Theme,
    pub char_width: f32,
    pub line_height: f32,
}

impl<'a> DrawContext<'a> {
    /// Draw the full editor frame from a `ScreenLayout`.
    pub fn draw_frame(&self, layout: &ScreenLayout) {
        unsafe {
            // Clear background
            self.rt.Clear(Some(&color_f(self.theme.background)));

            // Draw tab bar
            self.draw_tab_bar(layout);

            // Draw editor windows
            for rw in &layout.windows {
                self.draw_editor_window(rw);
            }

            // Draw status bar
            self.draw_status_bar(layout);

            // Draw command line
            self.draw_command_line(layout);

            // Draw cursor on active window
            for rw in &layout.windows {
                if rw.is_active {
                    self.draw_cursor(rw);
                }
            }

            // Draw completion menu
            if let Some(ref comp) = layout.completion {
                self.draw_completion(comp);
            }
        }
    }

    // ─── Tab bar ─────────────────────────────────────────────────────────────

    fn draw_tab_bar(&self, layout: &ScreenLayout) {
        let tab_bg = self.solid_brush(self.theme.tab_bar_bg);
        let y = 0.0f32;
        let h = self.line_height;

        unsafe {
            // Tab bar background
            self.rt
                .FillRectangle(&rect_f(0.0, y, 10000.0, h), &tab_bg);

            let mut x = 0.0f32;
            for tab in &layout.tab_bar {
                let bg = if tab.active {
                    self.solid_brush(self.theme.active_background)
                } else {
                    self.solid_brush(self.theme.tab_bar_bg)
                };
                let fg_color = if tab.active {
                    self.theme.foreground
                } else {
                    self.theme.line_number_fg
                };

                let tab_w = (tab.name.chars().count() as f32 + 3.0) * self.char_width;

                self.rt.FillRectangle(&rect_f(x, y, tab_w, h), &bg);
                self.draw_text(&tab.name, x + self.char_width, y, fg_color);

                // Active tab accent
                if tab.active {
                    let accent = self.solid_brush(self.theme.tab_active_accent);
                    self.rt
                        .FillRectangle(&rect_f(x, y, tab_w, 2.0), &accent);
                }

                x += tab_w;
            }
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
        let y = height - 2.0 * self.line_height;
        let bg = self.solid_brush(self.theme.status_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, y, width, self.line_height), &bg);
        }
        self.draw_text(
            &layout.status_left,
            self.char_width * 0.5,
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

    // ─── Command line ────────────────────────────────────────────────────────

    fn draw_command_line(&self, layout: &ScreenLayout) {
        let (width, height) = self.rt_size();
        let y = height - self.line_height;
        let bg = self.solid_brush(self.theme.background);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, y, width, self.line_height), &bg);
        }
        let cmd = &layout.command;
        self.draw_text(&cmd.text, self.char_width * 0.5, y, self.theme.foreground);
    }

    // ─── Completion popup ────────────────────────────────────────────────────

    fn draw_completion(&self, comp: &crate::render::CompletionMenu) {
        let bg = self.solid_brush(self.theme.completion_bg);
        let sel_bg = self.solid_brush(self.theme.selection);
        let popup_w = (comp.max_width as f32 + 4.0) * self.char_width;
        // Position at left edge, below cursor area (approximate)
        let x = 4.0 * self.char_width;
        let y = 2.0 * self.line_height;

        for (i, candidate) in comp.candidates.iter().enumerate() {
            let iy = y + i as f32 * self.line_height;
            let is_selected = i == comp.selected_idx;
            let row_bg = if is_selected { &sel_bg } else { &bg };
            unsafe {
                self.rt
                    .FillRectangle(&rect_f(x, iy, popup_w, self.line_height), row_bg);
            }
            self.draw_text(candidate, x + self.char_width, iy, self.theme.foreground);
        }
    }

    // ─── Primitive helpers ───────────────────────────────────────────────────

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
