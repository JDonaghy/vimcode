# VimCode Project State

**Last updated:** Mar 9, 2026 (Session 158 — VSCode Mode Gap Closure Phases 1–3) | **Tests:** 2985

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 157 are in **SESSION_HISTORY.md**.

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

**Session 158 — VSCode Mode Gap Closure Phases 1–3 (2985 tests):**
Implemented ~20 missing VSCode shortcuts across 3 phases. **Phase 1 — Line Operations + Alt Key Routing:** Alt encoding in TUI/GTK backends (Alt+key → `"Alt_Up"` etc. in VSCode mode); `Alt+Up/Down` move line, `Alt+Shift+Up/Down` duplicate line, `Ctrl+Shift+K` delete line, `Ctrl+Enter`/`Ctrl+Shift+Enter` insert blank line below/above, `Ctrl+L` select line (extends on repeat). **Phase 2 — Multi-Cursor + Indentation:** `Ctrl+D` progressive word select + add cursor at next occurrence, `Ctrl+Shift+L` select all occurrences (new `vscode_select_all_occurrences()` with proper visual mode + extra cursors at word end), multi-cursor typing/backspace/delete using char-index descending sort for same-line correctness, extra selections rendering in both backends, `Ctrl+]/[` indent/outdent with multi-cursor support, `Shift+Tab` outdent. **Phase 3 — Panels + Navigation:** `Ctrl+G` go to line (with `ensure_cursor_visible()`), `Ctrl+P` fuzzy finder, `Ctrl+Shift+P` command palette, `Ctrl+B` toggle sidebar, `Ctrl+J`/`` Ctrl+` `` toggle terminal (returns `EngineAction::OpenTerminal` to create pane), `Ctrl+,` open settings, `Ctrl+K` chord prefix (Ctrl+K,Ctrl+F format; Ctrl+K,Ctrl+W close all). **Bug fixes:** GTK terminal panel mouse off-by-one (all `term_px` calculations used `+1` instead of `+2` for tab bar row), GTK terminal + button not working (toolbar row misidentified), GTK `:N` go-to-line not scrolling (missing `ensure_cursor_visible()`). **UI polish:** Bottom panel tab bar and terminal toolbar now use sans-serif `UI_FONT` with uppercase labels ("TERMINAL", "DEBUG CONSOLE") matching VSCode style. 55 integration tests in `tests/vscode_mode.rs`.

> Sessions 157 and earlier archived in **SESSION_HISTORY.md**.
