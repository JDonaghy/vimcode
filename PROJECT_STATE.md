# VimCode Project State

**Last updated:** Apr 4, 2026 (Session 250 — Marksman LSP status fix) | **Tests:** 5300

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 250 are in **SESSION_HISTORY.md**.

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

> All sessions through 250 archived in **SESSION_HISTORY.md**.

- **Session 250**: Marksman LSP status indicator fix. `mark_server_responded()` was only called on non-empty hover/definition responses, leaving servers like Marksman (no semantic tokens, often empty hover) stuck on "Initializing". Fixed by marking responsive on `Initialized` event and removing empty-result guards on hover/definition handlers. 2 files changed (`panels.rs`, `lsp_manager.rs`).
- **Session 249**: Spell check underline misalignment fix + spell checker initialization fix + CI test fix (5300 total).
