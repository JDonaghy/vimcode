# VimCode Project State

**Last updated:** Mar 13, 2026 (Session 174 — Bug Fixes: Dialog System, Completion, Diff, Find Panel) | **Tests:** 4254

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 174 are in **SESSION_HISTORY.md**.

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

**Session 174 — Bug Fixes: Dialog System, Completion, Diff, Find Panel (4254 tests):**
Dismissable modal dialog system (`Dialog`/`DialogButton` structs, `show_dialog()`/`handle_dialog_key()`/`process_dialog_result()`) replacing status-bar messages for swap recovery. Stderr suppression in TUI clipboard init (RAII `StderrGuard` with `dup2`). Removed 6 `eprintln!` calls from core modules. Fixed sticky completion popup (new `dismiss_completion()` cancels pending LSP request; mode check in CompletionResponse handler). Fixed diff view padding with fold filtering (skip padding when `diff_unchanged_hidden`). Fixed diff view not working on large files (removed `MAX_LINES: 5000` guard in `lcs_diff()`). Fixed Find Panel text input in GTK (detect Entry focus, return `Propagation::Proceed`). Fixed Visual Mode Ctrl-D/U (`!ctrl` guards). Fixed undo/redo not notifying LSP. Verified diff toolbar renders on both group tab bars. 40 new tests.

> Sessions 173 and earlier archived in **SESSION_HISTORY.md**.
