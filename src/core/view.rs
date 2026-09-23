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
    /// Additional cursors added via Alt-D (add cursor at next match).
    /// All extra cursors receive the same insert-mode keystrokes as the primary cursor.
    /// Cleared on Escape from insert mode.
    pub extra_cursors: Vec<Cursor>,
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
    /// Every fold region ever defined for this window, open or closed —
    /// Vim's actual fold hierarchy (`:h folds`). Closing a fold (`zc`/`zf`)
    /// both defines it here and adds it to `folds`; opening it (`zo`) only
    /// removes it from `folds`, keeping the definition so a later `zc`
    /// recloses the *same* region instead of losing it. Before this field
    /// existed, `open_fold` deleted the region outright, so `zf{motion}` on
    /// text with no indent structure (nothing for `detect_fold_range` to
    /// rediscover) round-tripped `zo` into a fold `zc` could never reclose
    /// (#1006).
    pub fold_defs: Vec<FoldRegion>,
    /// In aligned-diff view, the aligned-row index this window's render
    /// should start at. Set by `sync_scroll_binds`; cleared when the
    /// window leaves aligned-diff mode (via `clear_diff_alignment`).
    ///
    /// Why: in aligned-diff mode, `scroll_top` (a buffer line) cannot
    /// uniquely identify which padding row the user wants at the top of
    /// the viewport — multiple aligned-row indices can map to the same
    /// buffer line via the seek-then-backup-over-padding logic. Storing
    /// the aligned index lets both panes start at exactly the same row,
    /// eliminating the cumulative drift past hunks (#166).
    pub aligned_top: Option<usize>,
}

impl View {
    pub fn new() -> Self {
        Self {
            cursor: Cursor::new(),
            extra_cursors: Vec::new(),
            scroll_top: 0,
            viewport_lines: 40, // sensible default, overridden by UI
            scroll_left: 0,
            viewport_cols: 80, // sensible default, overridden by UI
            folds: Vec::new(),
            fold_defs: Vec::new(),
            aligned_top: None,
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

    /// Record `start..=end` in the fold hierarchy without changing whether
    /// anything is currently closed. A no-op if the exact region is already
    /// defined (nested/overlapping regions are otherwise allowed — Vim folds
    /// nest).
    pub fn define_fold(&mut self, start: usize, end: usize) {
        if end <= start {
            return;
        }
        if self
            .fold_defs
            .iter()
            .any(|f| f.start == start && f.end == end)
        {
            return;
        }
        let pos = self.fold_defs.partition_point(|f| f.start < start);
        self.fold_defs.insert(pos, FoldRegion { start, end });
    }

    /// The innermost defined fold (open or closed) containing `line_idx`,
    /// i.e. the region `zc`/`za` would close when the cursor sits anywhere
    /// inside it. `None` if no fold is defined there at all.
    pub fn enclosing_fold_def(&self, line_idx: usize) -> Option<&FoldRegion> {
        self.fold_defs
            .iter()
            .filter(|f| f.start <= line_idx && line_idx <= f.end)
            .min_by_key(|f| f.end - f.start)
    }

    /// The outermost *closed* fold containing `line_idx`, if any — mirrors
    /// Vim's `foldclosed()` (which reports the outermost, not a nested
    /// one). Used to extend a linewise command's range when it touches a
    /// closed fold, e.g. `dd`/`yy` on a closed fold's header apply to every
    /// line inside it (`:h fold-behavior`, #1006).
    pub fn enclosing_closed_fold(&self, line_idx: usize) -> Option<&FoldRegion> {
        self.folds
            .iter()
            .filter(|f| f.start <= line_idx && line_idx <= f.end)
            .max_by_key(|f| f.end - f.start)
    }

    /// Close a fold spanning `start..=end`, keeping `folds` sorted by
    /// `start`. Also records the region in `fold_defs` (#1006) so a later
    /// `zo` then `zc` recloses exactly this region.
    ///
    /// Deliberately does **not** drop an existing closed fold that's fully
    /// contained in the new one (an earlier version did, to keep `folds`
    /// tidy) — closing a *nested* fold's parent must not silently reopen
    /// the child. Verified against `nvim --headless`: opening a closed
    /// outer fold with a closed inner fold still inside leaves the inner
    /// one closed (#1006) — `foldclosed()` on an inner line still reports
    /// the inner range. Losing the inner entry when the outer closed broke
    /// exactly that. Multiple overlapping closed entries are fine:
    /// `is_line_hidden` only needs *any* of them to match, and
    /// `next_visible_line`/`prev_visible_line` below take the outermost
    /// (widest) match rather than the first one found.
    pub fn close_fold(&mut self, start: usize, end: usize) {
        if end <= start {
            return;
        }
        self.define_fold(start, end);
        if self.folds.iter().any(|f| f.start == start && f.end == end) {
            return;
        }
        let pos = self.folds.partition_point(|f| f.start < start);
        self.folds.insert(pos, FoldRegion { start, end });
    }

    /// Open (remove) the fold whose header is `start`. The definition is
    /// kept in `fold_defs` — only its closed/open display state changes
    /// (#1006).
    pub fn open_fold(&mut self, start: usize) {
        self.folds.retain(|f| f.start != start);
    }

    /// Remove all folds in this window. Definitions are kept (`zR` doesn't
    /// forget folds, it just opens them — #1006).
    pub fn open_all_folds(&mut self) {
        self.folds.clear();
    }

    /// Remove the fold whose header is `start`, permanently (both its closed
    /// state and its definition). Returns `true` if found.
    pub fn delete_fold_at(&mut self, start: usize) -> bool {
        let len = self.folds.len();
        self.folds.retain(|f| f.start != start);
        let def_len = self.fold_defs.len();
        self.fold_defs.retain(|f| f.start != start);
        len != self.folds.len() || def_len != self.fold_defs.len()
    }

    /// Remove all folds whose headers fall within `start..=end`, permanently
    /// (both closed state and definition).
    pub fn delete_folds_in_range(&mut self, start: usize, end: usize) {
        self.folds.retain(|f| !(f.start >= start && f.start <= end));
        self.fold_defs
            .retain(|f| !(f.start >= start && f.start <= end));
    }

    /// Open (remove) all folds whose headers fall within `start..=end`.
    pub fn open_folds_in_range(&mut self, start: usize, end: usize) {
        self.folds.retain(|f| !(f.start >= start && f.start <= end));
    }

    /// Advance `count` visible lines forward from `from`, skipping fold bodies.
    /// Returns the resulting line index, clamped to `max_line`.
    pub fn next_visible_line(&self, from: usize, count: usize, max_line: usize) -> usize {
        let mut line = from;
        let mut remaining = count;
        while remaining > 0 && line < max_line {
            line += 1;
            // If we landed inside a fold body, jump past it. Several closed
            // folds can contain the same line when they're nested (#1006) —
            // take the outermost (largest `end`), not just the first match,
            // or a nested fold's own end would land the cursor back inside
            // its still-closed parent.
            if let Some(end) = self
                .folds
                .iter()
                .filter(|f| line > f.start && line <= f.end)
                .map(|f| f.end)
                .max()
            {
                line = end + 1;
            }
            if line > max_line {
                return max_line;
            }
            remaining -= 1;
        }
        line.min(max_line)
    }

    /// Go back `count` visible lines from `from`, skipping fold bodies.
    /// Returns the resulting line index.
    pub fn prev_visible_line(&self, from: usize, count: usize) -> usize {
        let mut line = from;
        let mut remaining = count;
        while remaining > 0 && line > 0 {
            line -= 1;
            // If we landed inside a fold body, jump to the fold header —
            // outermost (smallest `start`) match, see `next_visible_line`.
            if let Some(start) = self
                .folds
                .iter()
                .filter(|f| line > f.start && line <= f.end)
                .map(|f| f.start)
                .min()
            {
                line = start;
            }
            remaining -= 1;
        }
        line
    }

    /// Ensure the cursor is visible within the viewport, adjusting scroll_top.
    ///
    /// Production code uses `Engine::ensure_cursor_visible` instead,
    /// which accounts for bottom-chrome rows (quickfix, terminal,
    /// etc.) that may have opened in the current tick. This method
    /// stays on `View` for test ergonomics.
    #[allow(dead_code)]
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

    #[test]
    fn test_delete_fold_at() {
        let mut view = View::new();
        view.close_fold(2, 5);
        view.close_fold(8, 12);
        assert!(view.delete_fold_at(2));
        assert_eq!(view.folds.len(), 1);
        assert_eq!(view.folds[0].start, 8);
        // Deleting non-existent fold returns false
        assert!(!view.delete_fold_at(99));
    }

    #[test]
    fn test_delete_folds_in_range() {
        let mut view = View::new();
        view.close_fold(2, 5);
        view.close_fold(8, 12);
        view.close_fold(15, 20);
        view.delete_folds_in_range(2, 12);
        assert_eq!(view.folds.len(), 1);
        assert_eq!(view.folds[0].start, 15);
    }

    #[test]
    fn test_open_folds_in_range() {
        let mut view = View::new();
        view.close_fold(2, 5);
        view.close_fold(3, 4); // nested inside
        view.close_fold(10, 15);
        view.open_folds_in_range(2, 5);
        assert_eq!(view.folds.len(), 1);
        assert_eq!(view.folds[0].start, 10);
    }
}
