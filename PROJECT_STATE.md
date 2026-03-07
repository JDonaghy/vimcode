# VimCode Project State

**Last updated:** Mar 6, 2026 (Session 139 — comprehensive z-commands) | **Tests:** 2494

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 139 are in **SESSION_HISTORY.md**.

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

**Session 139 — Comprehensive z-commands (2494 tests):**
Implemented 15 missing z-commands to bring z-command coverage from 7/22 (32%) to 22/23 (96%). New fold commands: `zM` (close all), `zA`/`zO`/`zC` (recursive toggle/open/close), `zd`/`zD` (delete fold/recursive), `zf{motion}` (fold-create operator with j/k/G/gg/{/} motions), `zF` (fold N lines), `zv` (open to show cursor), `zx` (recompute). Scroll+first-non-blank: `z<CR>`/`z.`/`z-`. Horizontal scroll: `zh`/`zl` (with count), `zH`/`zL` (half-screen). Added 3 View helper methods (`delete_fold_at`, `delete_folds_in_range`, `open_folds_in_range`), 33 integration tests in `tests/z_commands.rs`.

> Sessions 138 and earlier archived in **SESSION_HISTORY.md**.
