# VimCode Project State

**Last updated:** Apr 5, 2026 (Session 251 — Layout toggle buttons) | **Tests:** 5304

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 251 are in **SESSION_HISTORY.md**.

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

> All sessions through 251 archived in **SESSION_HISTORY.md**.

- **Session 251**: Layout toggle buttons — clickable nerd-font icon segments (󰘖/󰆍/󰍜 with `[S]`/`[P]`/`[M]` ASCII fallbacks) in per-window status bar to toggle sidebar, terminal panel, and menu bar visibility; dim when inactive. GTK status bar click hit-testing overhauled: Pango-measured `StatusSegmentMap` cache replaces `char_width`-based approximation (fixes nerd font glyph width mismatch). Menu bar toggle hidden in GTK (title bar can't be hidden). `EngineAction::OpenTerminal` returned when no PTY panes exist. `menu_bar_toggleable` engine field. 4 new tests (5304 total).
