# VimCode Project State

**Last updated:** Mar 8, 2026 (Session 154 — Keymaps Editor in Settings Panel) | **Tests:** 2820

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 152 are in **SESSION_HISTORY.md**.

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

**Session 154 — Keymaps Editor in Settings Panel (2820 tests):**
"User Keymaps" row in the Settings sidebar panel (new `BufferEditor` setting type) — pressing Enter (or `:Keymaps` command) opens a scratch buffer pre-filled with current keymaps (one per line, `mode keys :command` format). `:w` validates each line, rejects invalid entries with line-specific errors, updates `settings.keymaps`, calls `rebuild_user_keymaps()`, and saves. Tab title shows `[Keymaps]`. Buffer reuse on re-open. GTK "Edit…" button + count label; TUI "N defined ▸" display. 11 integration tests in `tests/keymaps_editor.rs`.

> Sessions 153 and earlier archived in **SESSION_HISTORY.md**.
