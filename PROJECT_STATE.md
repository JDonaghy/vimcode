# VimCode Project State

**Last updated:** Mar 13, 2026 (Session 175 — Diff View Improvements: Click Handling, Fold-Aware Scrolling, Aligned Folds) | **Tests:** 4263

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 175 are in **SESSION_HISTORY.md**.

---

## Testing Policy

**Every new Vim feature and every bug fix MUST have comprehensive integration tests before the work is considered done.** Subtle bugs (register content, cursor position, newline handling, linewise vs. char-mode paste) are only reliably caught by tests. The process is:

1. Write failing tests that document the expected Vim behavior
2. Implement/fix the feature until all tests pass
3. Run the full suite (`cargo test`) — no regressions allowed

When implementing a new key/command, add tests covering:
- Basic happy path
- Edge cases: start/middle/end of line, start/end of file, empty buffer, count prefix
- Register content (text and `is_linewise` flag)
- Cursor position after the operation
- Interaction with paste (`p`/`P`) to verify the yanked/deleted content behaves correctly

---

## Recent Work

**Session 175 — Diff View Improvements: Click Handling, Fold-Aware Scrolling, Aligned Folds (4263 tests):**
Per-group diff toolbar click handling (GTK `DiffBtnMap`/`SplitBtnMap` replacing single shared cache; TUI `was_active` tracking). Click precedence fix: diff toolbar checked before split buttons in both backends. Split buttons visible on all groups in diff mode. Fold-aware scrolling: `next_visible_line()`/`prev_visible_line()` on View skip fold bodies; Ctrl-D/U/F/B/E/Y + scroll wheel all fold-aware in normal, visual, and both backends. Aligned-sequence fold computation: `diff_apply_folds()` rewritten to use `diff_aligned` (visual row → buffer line mapping) instead of raw `diff_results`, fixing incorrect folds when files have different line counts. `sc_has_focus` cleared on diff commands. TUI diff toolbar glyphs reverted to Nerd Font with 3-col button width. 3 new tests.

> Sessions 174 and earlier archived in **SESSION_HISTORY.md**.
