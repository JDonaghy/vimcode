# VimCode Project State

**Last updated:** Mar 31, 2026 (Session 237 — VSCode undo coalescing, bicep comments, :$ EOF, buffer picker, crash logging, keybindings picker, smart indent + auto-detect) | **Tests:** 5038

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 237 are in **SESSION_HISTORY.md**.

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

> All sessions through 237 archived in **SESSION_HISTORY.md**.
