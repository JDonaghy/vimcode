# VimCode Project State

**Last updated:** Mar 13, 2026 (Session 176 — GTK Performance: Lazy Tree + Open Folder Fix) | **Tests:** 4266

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 176 are in **SESSION_HISTORY.md**.

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

**Session 176 — GTK Performance: Lazy Tree + Open Folder Fix (4266 tests):**
GTK explorer tree lazy loading: replaced eager recursive `build_file_tree()` with `build_file_tree_shallow()` that populates one directory level at a time with dummy placeholder children; `tree_row_expanded()` replaces dummies with real children on demand via `row-expanded` signal. Fixes multi-second startup when opening in large directories (e.g., home). Open Folder fix: `open_folder()` now calls `std::env::set_current_dir()` to update process working directory; `RefreshFileTree` handler uses `engine.cwd` instead of `std::env::current_dir()`. `highlight_file_in_tree` rewritten to walk path components, expanding ancestors lazily.

> Sessions 175 and earlier archived in **SESSION_HISTORY.md**.
