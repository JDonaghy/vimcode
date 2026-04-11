# VimCode Project State

**Last updated:** Apr 11, 2026 (Session 268 — Win-GUI bug fixes: 16 items — all 4 critical + all 5 medium + 7 new bugs found/fixed by systematic audit) | **Tests:** 5478

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 267 are in **SESSION_HISTORY.md**.

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

**Session 268 — Win-GUI bug fixes (16 items — systematic audit + user-reported bugs):**
1. **Tab close dirty check** — `on_mouse_down` now checks `engine.dirty()` before closing a tab. Shows engine dialog (Save & Close / Discard / Cancel) for unsaved buffers. Added `close_tab_confirm` and `quit_unsaved` dialog tags to `process_dialog_result`.
2. **Picker mouse interaction** — When `picker_open` is true, all clicks are intercepted: click on result row selects it, click outside popup dismisses. Scroll wheel navigates picker items with scroll tracking.
3. **Dialog button clicks** — Full button rect hit-testing in `on_mouse_down` (highest z-order, before context menu/popups). Computes dialog geometry matching `draw_dialog`, dispatches `dialog_click_button(idx)`, handles quit actions via `DestroyWindow`. Outside-click dismisses dialog.
4. **QuitWithUnsaved handling** — `handle_action` now shows engine dialog (Save All & Quit / Quit Without Saving / Cancel) instead of silently returning false. Added `WM_CLOSE` handler that checks `has_any_unsaved()` and shows the same dialog, preventing accidental window close with unsaved work.
5. **Fold-aware scrolling** — Replaced raw `view_mut().scroll_top` arithmetic with `scroll_down_visible()`/`scroll_up_visible()` which skip folded regions.
6. **Picker scroll interception** — Scroll wheel checks `picker_open` first and navigates picker results instead of scrolling the editor behind the picker.
7. **VSCode selection clear on click** — Calls `vscode_clear_selection()` before `mouse_click` in editor area when in VSCode edit mode, matching GTK behavior.
8. **Cursor kept in viewport after scroll** — After `scroll_down_visible`/`scroll_up_visible`, cursor is clamped into the visible viewport (respecting `scrolloff`), with `clamp_cursor_col()` and `sync_scroll_binds()`. Matches GTK behavior.
9. **Terminal tab switching by mouse** — Click on numbered terminal tab labels in toolbar now switches `terminal_active`. Matches tab label geometry from draw code (active tab gets icon + name, inactive tabs get just number).
10. **Tabs disappear with second editor group** — `draw_group_tab_bar` subtracted only the tab bar height from `bounds.y`, but the reserved space above the content area also includes the breadcrumb row. Tab bars were drawn at the breadcrumb position, hidden behind editor content. Fixed by accounting for breadcrumb offset in both `draw_group_tab_bar` and `cache_layout` tab slot positions. Same fix applied to tab drag overlay. New test: `test_multi_group_window_rects_cover_all_groups`.
11. **Terminal steals keyboard focus** — `terminal_has_focus` was not cleared when clicking on the editor area. Added `terminal_has_focus = false` in editor click handler (matching GTK/TUI).
12. **Tab accent only on active group** — `draw_tabs` drew the 2px accent bar on all active tabs regardless of group. Added `show_accent` parameter; `draw_group_tab_bar` passes `is_active_group` so only the focused group's active tab shows the accent.
13. **Explorer preview investigated** — Preview system works correctly; `open_file_preview()` reuses the preview tab. Multiple tabs appear only when preview is promoted (clicking tab, editing, saving) — expected VSCode behavior.
14. **Sidebar focus persists after editor click** — Found by systematic audit: `clear_sidebar_focus()` was never called on editor click. Settings/AI/Search/Debug focus flags persisted incorrectly. Fixed.
15. **Sidebar focus persists after terminal click** — Found by systematic audit: clicking terminal panel didn't clear sidebar focus. Fixed by adding `clear_sidebar_focus()` + `sidebar.has_focus = false`.
16. **Dialog text/buttons overflow** — Dialog width was hardcoded at 400px, too narrow for long button labels. Auto-sized from content (buttons + body + title). Click handler updated to match.
- **Native GUI lessons doc** — Created `docs/NATIVE_GUI_LESSONS.md` with 9 sections of lessons from Win-GUI: reserved space, multi-group testing, ScreenLayout as source of truth, focus state management, popup sizing, and 16-item backend checklist. Referenced from CLAUDE.md.

> All sessions through 267 archived in **SESSION_HISTORY.md**.
