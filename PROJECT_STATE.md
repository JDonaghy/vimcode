# VimCode Project State

**Last updated:** Apr 8, 2026 (Session 257 — Win-GUI Phase 4: custom title bar, native file dialogs, IME, file watching, UI font) | **Tests:** 5313

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

> All sessions through 257 archived in **SESSION_HISTORY.md**.

- **Session 257**: Win-GUI Phase 4 — custom frameless title bar (WM_NCCALCSIZE + DwmExtendFrameIntoClientArea + WM_NCHITTEST, min/max/close buttons with hover states, Segoe UI proportional font for menus+tabs matching VSCode), native file dialogs (IFileOpenDialog for Open File/Folder, IFileSaveDialog for Save Workspace As, COM init), IME composition (WM_IME_STARTCOMPOSITION positions candidate window at cursor), cross-platform file watching (notify crate, auto-reload clean buffers, reload/keep dialog for dirty, added to TUI too), dynamic window title, taller title+tab bars with vertical centering. Fixed double RefCell borrow panic in on_mouse_move that silently broke all menu hover code.
