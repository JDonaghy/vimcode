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

> All sessions through 259 archived in **SESSION_HISTORY.md**.

- **Session 259**: README revamp — updated intro/status (beta label + disclaimer), Platforms table, Windows download/build instructions, Architecture with win_gui/ + all current line counts (~128K total), Tech Stack with windows-rs/D2D/DWrite/notify, Extensions panel mentions, vimcode.org reference, removed duplicate commands, updated Acknowledgements.
