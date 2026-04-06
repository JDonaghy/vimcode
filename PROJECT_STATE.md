# VimCode Project State

**Last updated:** Apr 6, 2026 (Session 254 — Windows TUI builds + bug fixes) | **Tests:** 5313

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 254 are in **SESSION_HISTORY.md**.

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

> All sessions through 254 archived in **SESSION_HISTORY.md**.

- **Session 254**: Windows TUI builds + bug fixes — `CREATE_NEW_PROCESS_GROUP` for LSP/DAP on Windows; Windows clipboard (powershell), URL opener (`cmd /c start`), swap PID check (`tasklist`); CI + release workflow for `vcd-windows-x86_64.exe`; tree-sitter-latex static link fix; 8 Windows test fixes; flatpak cargo-sources.json regen; picker preview stale chars fix; insert mode click past EOL fix; scrollbar drag cursor fix; git panel discard confirm dialog.

- **Session 253**: Notification / progress indicator — spinner/bell in per-window status bar for background ops; auto-dismiss after 5s; click-to-clear; 9 new tests.
