# VimCode Project State

**Last updated:** Mar 8, 2026 (Session 150 — Tab switcher polish + tab click fix) | **Tests:** 2728

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 144 are in **SESSION_HISTORY.md**.

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

**Session 150 — Tab switcher polish + tab click fix (2728 tests):**
Alt+t as universal tab switcher binding (works in both TUI and GTK where Ctrl+Tab is often intercepted). GTK modifier-release detection via 100ms polling of `keyboard.modifier_state()` — releasing Ctrl/Alt auto-confirms selection. TUI uses 500ms timeout after last cycle. Sans-serif UI font (`UI_FONT`) applied to tab bar and tab switcher popup in GTK (matching VSCode style). **Tab click fix**: clicking tabs in GUI mode now works correctly — fixed three bugs: (1) breadcrumbs offset caused click y-region to hit breadcrumb row instead of tab row (`grect.y - line_height` → `grect.y - tab_bar_height`); (2) monospace `char_width` tab measurement replaced with Pango-measured slot positions cached during draw; (3) `editor_bottom` calculation now matches draw layout (accounts for quickfix/terminal/debug toolbar). Tab bar clicks skip expensive `fire_cursor_move_hook()` (git blame subprocess) and defer `highlight_file_in_tree` DFS via 50ms timeout for instant visual response.

**Session 149 — Ctrl+Tab MRU tab switcher + autohide panels (2728 tests):**
VSCode-style MRU tab switcher: Ctrl+Tab opens a popup showing recently accessed tabs in most-recently-used order; Ctrl+Tab cycles forward, Ctrl+Shift+Tab cycles backward, Enter or any non-modifier key confirms selection, Escape cancels. New `autohide_panels` boolean setting (default false, TUI only): when enabled, hides sidebar and activity bar at startup; Ctrl-W h reveals them, and they auto-hide when focus returns to the editor.

> Sessions 149 and earlier archived in **SESSION_HISTORY.md**.
