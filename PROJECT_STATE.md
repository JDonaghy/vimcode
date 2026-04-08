# VimCode Project State

**Last updated:** Apr 8, 2026 (Session 259 — README revamp) | **Tests:** 5391

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 257 are in **SESSION_HISTORY.md**.

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

> All sessions through 258 archived in **SESSION_HISTORY.md**.

- **Session 258**: Multi-backend code sharing — extracted shared hit-testing geometry, key-binding matching, and scrollbar helpers from GTK/TUI/Win-GUI backends into `render.rs`. Moved `ClickTarget` enum to render.rs (public). Added 8 shared helper functions: `tab_row_height_px`, `tab_bar_height_px`, `status_bar_height_px`, `editor_bottom_px`, `scrollbar_click_to_scroll_top`, `display_col_to_buffer_col`, `is_tab_close_click`, `matches_key_binding`. GTK `pixel_to_click_target()` and `matches_gtk_key()`, TUI `matches_tui_key()` and scrollbar click, Win-GUI `scrollbar_hit()` all delegate to shared functions. 7 new tests. Filed 4 pre-existing win-gui bugs (scrollbar not drawn, tab clicks, file open behavior, no preview mode).
