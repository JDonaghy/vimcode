# VimCode Project State

**Last updated:** Mar 12, 2026 (Session 173 — Diff View Fixes: Aligned Scroll Sync + Auto-Filter) | **Tests:** 4214

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 173 are in **SESSION_HISTORY.md**.

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

**Session 173 — Diff View Fixes: Aligned Scroll Sync + Auto-Filter (4214 tests):**
Aligned-position-aware scroll sync for diff windows: `sync_scroll_binds()` now maps scroll positions through `diff_aligned` sequences. Auto-enable `diff_unchanged_hidden` + `diff_apply_folds()` in all diff entry points. `is_in_diff_view()` checks all groups. Render fix: `build_rendered_window` advances `aligned_idx` past folded lines. Known remaining issues in BUGS.md. 1 new test.

> Sessions 172 and earlier archived in **SESSION_HISTORY.md**.
