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

            // Draw editor windows
            for rw in &layout.windows {
                self.draw_editor_window(rw);
            }

            // Draw status bar
            self.draw_status_bar(layout);

            // Draw command line
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

            // Draw dialog
            if let Some(ref dialog) = layout.dialog {
                self.draw_dialog(dialog);
            }
        }
    }

    // ─── Tab bar ─────────────────────────────────────────────────────────────

    fn draw_tab_bar(&self, layout: &ScreenLayout) {
        let y = 0.0f32;
        let h = self.line_height;
        let (width, _) = self.rt_size();
        let tab_bg = self.solid_brush(self.theme.tab_bar_bg);
        unsafe {
            self.rt
                .FillRectangle(&rect_f(0.0, y, width, h), &tab_bg);
        }
        self.draw_tabs(&layout.tab_bar, 0.0, y, width);
    }

    fn draw_group_tab_bar(
        &self,
        gtb: &crate::render::GroupTabBar,
        is_active_group: bool,
    ) {
        let h = self.line_height;
        let x = gtb.bounds.x as f32;
        let y = gtb.bounds.y as f32 - h; // tab bar sits above the group content
        let w = gtb.bounds.width as f32;

        let bg = if is_active_group {
            self.solid_brush(self.theme.tab_bar_bg)
        } else {
            self.solid_brush(self.theme.tab_bar_bg)
        };
        unsafe {
            self.rt.FillRectangle(&rect_f(x, y, w, h), &bg);
        }
        self.draw_tabs(&gtb.tabs, x, y, w);
    }

    fn draw_tabs(&self, tabs: &[crate::render::TabInfo], x_origin: f32, y: f32, _max_width: f32) {
        let h = self.line_height;
        let mut x = x_origin;

        for tab in tabs {
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

            unsafe {
                self.rt.FillRectangle(&rect_f(x, y, tab_w, h), &bg);
            }

            // Tab name
            self.draw_text(&tab.name, x + self.char_width, y, fg_color);

            // Dirty indicator (dot) or close button (x)
            let close_x = x + tab_w - 2.0 * self.char_width;
            if tab.dirty {
                // Show a dot for unsaved changes
                self.draw_text("\u{25CF}", close_x, y, self.theme.git_modified);
            } else {
                self.draw_text("\u{00D7}", close_x, y, self.theme.line_number_fg);
            }

            // Active tab accent bar (2px at top)
            if tab.active {
                let accent = self.solid_brush(self.theme.tab_active_accent);
                unsafe {
                    self.rt
                        .FillRectangle(&rect_f(x, y, tab_w, 2.0), &accent);
                }
            }

            // Tab separator
            unsafe {
                let sep = self.solid_brush(self.theme.separator);
                self.rt.FillRectangle(
                    &rect_f(x + tab_w - 1.0, y + 4.0, 1.0, h - 8.0),
                    &sep,
                );
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
                self.rt
                    .FillRectangle(&rect_f(x, y, w, h), &divider_brush);
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
            self.rt
                .FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt.DrawRectangle(
                &rect_f(x, y, popup_w, popup_h),
                &border_brush,
                1.0,
                None,
            );
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
            self.draw_text(
                candidate,
                x + self.char_width,
                iy,
                self.theme.foreground,
            );
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
            self.rt
                .FillRectangle(&rect_f(x, y, popup_w, popup_h), &bg);
            self.rt.DrawRectangle(
                &rect_f(x, y, popup_w, popup_h),
                &border_brush,
                1.0,
                None,
            );
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
            self.rt.DrawRectangle(
                &rect_f(dx, dy, dialog_w, dialog_h),
                &border,
                1.0,
                None,
            );
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
