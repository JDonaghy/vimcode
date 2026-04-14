# VimCode Project State

**Last updated:** Apr 13, 2026 (Session 274 — Phase 2d behavioral parity tests, clippy CI fix) | **Tests:** 5494

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 272 are in **SESSION_HISTORY.md**.

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

**Session 274 — Phase 2d behavioral parity tests, clippy CI fix:**

1. **Phase 2d behavioral backend parity tests** — 16 new end-to-end tests in `render.rs` that simulate user interaction sequences and verify engine state transitions. Covers: tab click/switch, tab close (clean + dirty gate), context menu lifecycle (open/confirm/dismiss), explorer/tab/editor context menu targets, double-click word selection, editor hover lifecycle (show/focus/scroll/dismiss), sidebar focus toggle + clear, terminal new/close/split, tab drag-drop to create splits, preview tab promotion via goto_tab, preview reuse invariant, mouse click cursor movement.
2. **Clippy CI fix** — `return true` → `true` in `cargo_bin_probe_ok()` non-Windows cfg block (`lsp_manager.rs:51`). Fixed `needless_return` lint that broke the Linux CI build.
3. **Updated `/complete-push` command** — Now requires clippy to pass on all feature configurations before pushing.

> Sessions 273 and earlier in **SESSION_HISTORY.md**.
