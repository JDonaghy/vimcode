# VimCode Project State

**Last updated:** Mar 11, 2026 (Session 170 — Inline Diff Peek + Enhanced Hunk Nav) | **Tests:** 4105

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 167 are in **SESSION_HISTORY.md**.

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

**Session 170 — Inline Diff Peek + Enhanced Hunk Nav (4105 tests):**
Inline diff preview (VSCode parity): `gD` / `:DiffPeek` / click gutter marker opens floating popup showing hunk diff lines (red=removed, green=added) with `[r] Revert` / `[s] Stage` actions. Deleted-line gutter indicator (`▾` in red) for pure deletions. `]c`/`[c` now navigate changed regions on real source files using `git_diff` markers (previously only worked in diff buffers). New `DiffPeekState`/`DiffPeekPopup` structs, `DiffHunkInfo` with line-range mapping, `compute_file_diff_hunks()`, `hunk_for_line()`, `revert_hunk()` in git.rs. `git_deleted` color added to all 4 themes. Both GTK and TUI backends render popup + detect git gutter clicks. "Git: Peek Change" in command palette. 17 new tests.

> Sessions 169 and earlier archived in **SESSION_HISTORY.md**.
