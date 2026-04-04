# VimCode Project State

**Last updated:** Apr 3, 2026 (Session 245 — Editor action menu, richer syntax highlighting, explorer colors) | **Tests:** 5282

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 245 are in **SESSION_HISTORY.md**.

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

> All sessions through 245 archived in **SESSION_HISTORY.md**.

- **Session 245**: Editor action menu (`⋯`) button, richer tree-sitter highlighting, explorer color overhaul. Action menu: 8-item dropdown at right edge of each tab bar; both GTK (PopoverMenu) and TUI. Tree-sitter: 12 new Theme fields (`control_flow`, `operator`, `punctuation`, `macro_call`, `attribute`, `lifetime`, `constant`, `escape`, `boolean`, `property`, `parameter`, `module`); all 20 language queries expanded with operators, punctuation, numbers, booleans, method calls, parameters, escape sequences; keywords split into storage vs control flow; `semantic_token_style()` handles `controlFlow` modifier; fixed tree-sitter reparse (always full parse — old tree without `tree.edit()` produced garbled byte offsets); insert mode immediate re-parse instead of 150ms debounce. Explorer: same base color for files/dirs; git status + diagnostics propagate recursively to parents; GTK indicator column in own TreeViewColumn. Fixes: split-down icon, midline ellipsis, GTK tab bar clip, `gtk_editor_bottom()` shared helper, divider drag excluded from tab bars, GTK menu dropdown padding, LSP status no longer downgrades Running→Initializing on empty semantic tokens. 7 new tests.
