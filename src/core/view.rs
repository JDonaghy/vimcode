use super::Cursor;

/// A closed fold region. Lines `start+1 ..= end` are hidden; `start` is the
/// visible header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRegion {
    /// Fold header line (always visible).
    pub start: usize,
    /// Last hidden line (inclusive). Must satisfy `end > start`.
    pub end: usize,
}

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
    /// Closed fold regions for this window, sorted by `start`, non-overlapping.
    /// Folds are ephemeral (not persisted to session).
    pub folds: Vec<FoldRegion>,
}

impl View {
    pub fn new() -> Self {
        Self {
            cursor: Cursor::new(),
            scroll_top: 0,
            viewport_lines: 40, // sensible default, overridden by UI
            scroll_left: 0,
            viewport_cols: 80, // sensible default, overridden by UI
            folds: Vec::new(),
        }
    }

    /// Returns `true` if `line_idx` is hidden inside a fold body (not the header).
    pub fn is_line_hidden(&self, line_idx: usize) -> bool {
        self.folds
            .iter()
            .any(|f| line_idx > f.start && line_idx <= f.end)
    }

    /// Returns a reference to the `FoldRegion` whose header is `line_idx`, if any.
    pub fn fold_at(&self, line_idx: usize) -> Option<&FoldRegion> {
        self.folds.iter().find(|f| f.start == line_idx)
    }

    /// Close a fold spanning `start..=end`.
    /// Merges or discards any existing overlapping folds to keep `folds` sorted
    /// and non-overlapping.
    pub fn close_fold(&mut self, start: usize, end: usize) {
        if end <= start {
            return;
        }
        // Remove any folds that are fully contained within the new region.
        self.folds.retain(|f| !(f.start >= start && f.end <= end));
        // Insert the new fold in sorted order.
        let pos = self.folds.partition_point(|f| f.start < start);
        self.folds.insert(pos, FoldRegion { start, end });
    }

    /// Open (remove) the fold whose header is `start`.
    pub fn open_fold(&mut self, start: usize) {
        self.folds.retain(|f| f.start != start);
    }

    /// Remove all folds in this window.
    pub fn open_all_folds(&mut self) {
        self.folds.clear();
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

    #[test]
    fn test_fold_is_line_hidden() {
        let mut view = View::new();
        view.close_fold(2, 5);
        // Header is visible
        assert!(!view.is_line_hidden(2));
        // Body lines are hidden
        assert!(view.is_line_hidden(3));
        assert!(view.is_line_hidden(4));
        assert!(view.is_line_hidden(5));
        // Lines outside fold are visible
        assert!(!view.is_line_hidden(1));
        assert!(!view.is_line_hidden(6));
    }

    #[test]
    fn test_fold_at() {
        let mut view = View::new();
        view.close_fold(2, 5);
        assert!(view.fold_at(2).is_some());
        assert!(view.fold_at(3).is_none()); // body, not header
        assert!(view.fold_at(1).is_none());
    }

    #[test]
    fn test_open_fold() {
        let mut view = View::new();
        view.close_fold(2, 5);
        view.open_fold(2);
        assert!(view.fold_at(2).is_none());
        assert!(!view.is_line_hidden(3));
    }

    #[test]
    fn test_open_all_folds() {
        let mut view = View::new();
        view.close_fold(0, 3);
        view.close_fold(5, 8);
        view.open_all_folds();
        assert!(view.folds.is_empty());
    }

    #[test]
    fn test_close_fold_sorted() {
        let mut view = View::new();
        view.close_fold(5, 8);
        view.close_fold(1, 3);
        // Should remain sorted by start
        assert_eq!(view.folds[0].start, 1);
        assert_eq!(view.folds[1].start, 5);
    }
}
