# VimCode Project State

**Last updated:** Mar 8, 2026 (Session 152 — Visual paste, TUI bug fixes) | **Tests:** 2768

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 151 are in **SESSION_HISTORY.md**.

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

**Session 152 — Visual paste + TUI bug fixes (2768 tests):**
**Visual paste**: `p`/`P` in Visual/VisualLine/VisualBlock mode replaces selection with register content (deleted text stored in unnamed register); `"x` register selection in visual mode; named register paste (`"ap`); `Ctrl+Shift+V` system clipboard paste in Normal/Visual modes for both TUI (keyboard enhancement) and GTK. **TUI tab bar fix**: multi-group tab bar y-coordinate used `bounds.y - 1` which was wrong when breadcrumbs enabled (`tab_bar_height=2`); fixed to use `bounds.y - tab_bar_height`; same fix for mouse click handler. **TUI sidebar reveal fix**: `focus_window_direction()` only set `window_nav_overflow` for single-group layouts; with multiple editor groups, Ctrl-W h/l now navigates between adjacent groups first, and only signals overflow (sidebar reveal) when at the leftmost/rightmost group. **Test fix**: `test_restore_session_files_opens_separate_tabs` failed because `swap_scan_stale()` opened stale swap files as extra tabs; fixed by disabling swap files in test. 8 integration tests in `tests/visual_mode.rs`.

> Sessions 151 and earlier archived in **SESSION_HISTORY.md**.
