# VimCode Project State

**Last updated:** Apr 4, 2026 (Session 246 — Explorer overhaul, diagnostic filtering, tree UX) | **Tests:** 5292

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 246 are in **SESSION_HISTORY.md**.

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

> All sessions through 246 archived in **SESSION_HISTORY.md**.

- **Session 246**: Explorer overhaul, diagnostic filtering, tree UX. Removed explorer toolbar + TUI "EXPLORER" header row. Right-click in empty explorer space → root context menu. Inline rename: stem pre-selection, Ctrl-C/V/X/A, horizontal scroll, both backends. Diagnostic source filtering: `ignore_error_sources` now filters at storage time (not just explorer counts); `refilter_diagnostics()` on registry update; `ext_refresh()` at startup; `initialization_options` field on `LspConfig`/`LspServerConfig` for per-server LSP init config. Explorer tree UX: `explorer_file_fg` theme field (muted grey for file names); TUI indent guide lines (`│`); GTK name column ellipsizes with `...` + Fixed sizing; case-insensitive sort (`explorer_sort_case_insensitive` setting, default true). Fix: `LineEnding::detect()` byte-boundary crash on multi-byte chars at 8KB boundary. Bug fixes: GTK inline rename/new-file cancelled by `update_tree_indicators` (skip while `is_editing()`), SIGSEGV on marker rows, popover focus steal, TUI context menu from empty space, GTK rename stem selection. 10 new tests.
- **Session 245**: Editor action menu (`⋯`) button, richer tree-sitter highlighting, explorer color overhaul. Action menu: 8-item dropdown at right edge of each tab bar; both GTK (PopoverMenu) and TUI. Tree-sitter: 12 new Theme fields; all 20 language queries expanded; keywords split into storage vs control flow; fixed tree-sitter reparse; insert mode immediate re-parse. Explorer: same base color for files/dirs; git status + diagnostics propagate recursively to parents; GTK indicator column in own TreeViewColumn. 7 new tests.
