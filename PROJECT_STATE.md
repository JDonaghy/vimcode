# VimCode Project State

**Last updated:** Mar 10, 2026 (Session 162 — Bulk paste fix) | **Tests:** 4003

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 161 are in **SESSION_HISTORY.md**.

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

**Session 162 — Bulk paste performance fix (4003 tests):**
Fixed critical performance bug: pasting large text in insert mode caused UI freeze / 100% CPU. Root cause was `Event::Paste` (TUI) and `ClipboardPasteToInput` (GTK) feeding each character individually through `handle_key()`, triggering ~N tree-sitter reparses, bracket match scans, auto-completion scans, etc. for an N-character paste. New `Engine::paste_in_insert_mode(text)` method does a single bulk `insert_with_undo()` and runs all expensive post-processing once. Also added safety guard in `compute_word_wrap_segments()` (`pos = break_at.max(pos + 1)`) to prevent potential infinite loops. 8 new tests in `tests/paste_insert.rs`.

> Sessions 161 and earlier archived in **SESSION_HISTORY.md**.
