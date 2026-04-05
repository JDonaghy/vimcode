# VimCode Project State

**Last updated:** Apr 5, 2026 (Session 253 — Notification / progress indicator) | **Tests:** 5313

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 253 are in **SESSION_HISTORY.md**.

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

> All sessions through 253 archived in **SESSION_HISTORY.md**.

- **Session 253**: Notification / progress indicator — `Notification` struct + `NotificationKind` enum on Engine; `notify()`/`notify_done()`/`notify_done_by_kind()`/`tick_notifications()` lifecycle; spinner animation (⠋⠙⠹…) for in-progress, bell icon (󰂞) for completed; auto-dismiss after 5s; `StatusAction::DismissNotifications` click-to-clear; rendered as `StatusSegment` in per-window status bar (between Ln:Col and layout toggles); GTK+TUI backends animate via poll tick + short poll timeout; wired up for LSP install, project search, project replace; 9 new tests.

- **Session 252**: TUI spell underline bleed fix — `set_cell()`/`set_cell_wide()` in TUI backend only reset char/fg/bg but not `cell.modifier` or `cell.underline_color`, so spell check underlines bled through into picker/fuzzy finder overlays. Fixed by resetting `modifier` to `Modifier::empty()` and `underline_color` to `RColor::Reset` in all three cell-setting functions. Added remote editing research item to PLAN.md.
