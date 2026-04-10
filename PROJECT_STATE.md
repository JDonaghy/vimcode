# VimCode Project State

**Last updated:** Apr 10, 2026 (Session 266 — 10 Win-GUI parity fixes: text rendering, settings panel, status bar, clipping) | **Tests:** 5471

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 264 are in **SESSION_HISTORY.md**.

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

**Session 266 — Win-GUI parity fixes (10 fixes):**
1. **Text rendering truncation** — `draw_styled_line` gap-filling for text between syntax spans
2. **Settings icon clipped/not clickable** — repositioned above bottom chrome, click handler fixed
3. **Settings panel interactive** — full form rendering + keyboard handling (j/k/Enter/Tab///q, editing, paste)
4. **Global status bar over per-window status** — skip when empty; reserve 1 row not 2 for bottom chrome
5. **Per-window status bar segments** — per-segment background colors matching TUI
6. **Editor window clipping** — `PushAxisAlignedClip` prevents text bleeding
7. **Sidebar panel clipping** — clip rect and panel_h use `sidebar_bottom`
8. **Command line descenders clipped** — bottom margin for below-baseline characters
9. **Sidebar/command line background gaps** — panel bg full height; cmd line starts at `editor_left`
10. **Clippy fix** — identical if/else branches in diff toolbar

> All sessions through 264 archived in **SESSION_HISTORY.md**.
