# VimCode Project State

**Last updated:** Mar 8, 2026 (Session 151 — Tab drag-to-split + tab bar draw fix + new logo) | **Tests:** 2760

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 150 are in **SESSION_HISTORY.md**.

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

**Session 151 — Tab drag-to-split + tab bar draw fix + new logo (2760 tests):**
VSCode-style tab drag-and-drop: drag a tab to the edge of a group to create a new editor group split; drag to center to move tab between groups; drag within tab bar to reorder. New core types: `DropZone` enum (Center/Split/TabReorder/None) in `window.rs`, `TabDragState` struct in `engine.rs`. 7 new engine methods: `tab_drag_begin`, `tab_drag_cancel`, `tab_drag_drop`, `move_tab_to_target_group`, `move_tab_to_new_split`, `reorder_tab_in_group`, `close_group_by_id`. GTK: 8px dead-zone drag detection from tab clicks, `compute_tab_drop_zone()` with 20% edge margins for split zones, `draw_tab_drag_overlay()` with blue highlight + ghost label. **Tab bar draw order fix**: moved tab bar + breadcrumb drawing AFTER window drawing so tab bars are never overwritten by window backgrounds in multi-group layouts; dividers draw before tab bars so vertical dividers don't bleed through tab bar backgrounds. **New logo**: `vim-code.svg` gradient VC logo replaces old icon files; removed `vimcode-color.png`, `vimcode-color.svg`, `vimcode.png`, `vimcode.svg`, `asset-pack.jpg`; updated Flatpak icon. 15 integration tests in `tests/tab_drag.rs`.

> Sessions 150 and earlier archived in **SESSION_HISTORY.md**.
