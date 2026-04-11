# VimCode Project State

**Last updated:** Apr 10, 2026 (Session 267 — Win-GUI bug blitz: 9 fixes, Phase 2c action parity harness, GTK↔Win-GUI comparison found 12 more bugs) | **Tests:** 5477

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

**Session 267 — Win-GUI bug blitz + parity tests (9 fixes, 6 new tests, 12 bugs found):**
1. **Activity bar icons** — Replaced broken Nerd Font approach with Segoe MDL2 Assets / Segoe Fluent Icons (native Windows icon fonts). 48×48 centered icon cells with dedicated DirectWrite format.
2. **Tab drag-and-drop** — Full implementation: threshold-based drag start, `compute_win_tab_drop_zone()` for reorder/split/merge, visual overlay (blue zone highlight + insertion bar + ghost label), calls engine's `tab_drag_begin()`/`tab_drag_drop()`.
3. **Terminal split** — Split button + add/close buttons in toolbar, split pane rendering with divider, pane focus switching, divider drag resize.
4. **Popup mouse handlers** — `CachedPopupRects` infrastructure. Editor hover (click/dismiss/scroll), panel hover (dismiss), debug toolbar (button clicks via `execute_command`).
5. **Scrollbar theme colors** — Fixed editor scrollbar to use `theme.scrollbar_thumb`/`scrollbar_track` instead of hardcoded alpha values.
6. **Explorer file open** — Single-click now uses `open_file_preview()` (preview tab). Double-click/Enter uses `open_file_in_tab()` (new permanent tab). Was using `switch_window_buffer` which replaced the current buffer.
7. **Context menu z-order + clicks** — Context menu, dialog, notifications now draw after sidebar in `on_paint`. Full click handler: item selection, action dispatch via `handle_context_action()`, outside-click dismiss.
8. **Default shell** — `default_shell()` returns `powershell.exe` on Windows instead of `/bin/bash`.
9. **Phase 2c action parity harness** — `UiAction` enum (26 variants), `all_required_ui_actions()` source of truth, per-backend collectors. 3 parity tests + 3 behavioral contract tests. Systematic GTK↔Win-GUI comparison found 12 additional bugs (4 critical, 5 medium, 3 low — see BUGS.md).

> All sessions through 266 archived in **SESSION_HISTORY.md**.
