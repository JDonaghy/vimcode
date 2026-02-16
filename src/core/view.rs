use super::Cursor;

/// View holds the per-window state for displaying a buffer.
/// Each window has its own View, allowing the same buffer to be
/// displayed with different cursor positions and scroll offsets.
#[derive(Debug, Clone)]
pub struct View {
    /// Cursor position within the buffer (line, col).
    pub cursor: Cursor,
    /// First visible line (for viewport scrolling).
    pub scroll_top: usize,
    /// Number of lines that fit in this window's text viewport.
    pub viewport_lines: usize,
    /// First visible column (for horizontal scrolling).
    pub scroll_left: usize,
    /// Number of columns that fit in this window's text viewport.
    pub viewport_cols: usize,
}

impl View {
    pub fn new() -> Self {
        Self {
            cursor: Cursor::new(),
            scroll_top: 0,
            viewport_lines: 40, // sensible default, overridden by UI
            scroll_left: 0,
            viewport_cols: 80, // sensible default, overridden by UI
        }
    }

    /// Ensure the cursor is visible within the viewport, adjusting scroll_top.
    pub fn ensure_cursor_visible(&mut self) {
        if self.cursor.line < self.scroll_top {
            self.scroll_top = self.cursor.line;
        }
        if self.viewport_lines > 0 && self.cursor.line >= self.scroll_top + self.viewport_lines {
            self.scroll_top = self.cursor.line - self.viewport_lines + 1;
        }
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_ensure_cursor_visible_scroll_down() {
        let mut view = View::new();
        view.viewport_lines = 10;
        view.scroll_top = 0;
        view.cursor.line = 15;

        view.ensure_cursor_visible();
        assert_eq!(view.scroll_top, 6); // 15 - 10 + 1 = 6
    }

    #[test]
    fn test_view_ensure_cursor_visible_scroll_up() {
        let mut view = View::new();
        view.viewport_lines = 10;
        view.scroll_top = 20;
        view.cursor.line = 5;

        view.ensure_cursor_visible();
        assert_eq!(view.scroll_top, 5);
    }
}
