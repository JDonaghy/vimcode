# VimCode Project State

**Last updated:** Mar 24, 2026 (Session 213 — Unified Picker + Hover Fixes) | **Tests:** 4706

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 212 are in **SESSION_HISTORY.md**.

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

### Session 213 — Unified Picker + Hover Fixes (Mar 24, 2026)
- **Unified Picker Phase 1+2**: Replaced separate fuzzy/grep/palette systems with unified `PickerSource`/`PickerItem`/`PickerAction` types; `.gitignore`-aware file walking via `ignore` crate; fuzzy match highlighting in results; unified `PickerPanel` render struct + single draw function per backend (replacing 3 draw functions each in GTK and TUI)
- **New keybindings**: `<leader>sf` (files), `<leader>sg` (grep), `<leader>sp` (commands); `Ctrl-Shift-F` for live grep (was `Ctrl-G`); `Ctrl-Shift-P` for command palette; all remappable via `panel_keys` settings
- **GTK crash prevention**: `catch_unwind` wrapper around `draw_editor` in `set_draw_func` extern "C" callback; replaced all `.unwrap()` with `.ok()` on Cairo operations (fill/stroke/save/restore/paint)
- **GStrInteriorNulError fix**: `format_button_label` handles `'\0'` hotkey sentinel; sanitized NUL bytes from file content in render
- **Lightbulb duplication fix**: `is_wrap_continuation` guard in both GTK and TUI backends
- **Markdown inline syntax highlighting**: regex-based inline pass for bold/italic/code/links; fenced code backtick delimiter handling; underscore word-boundary requirement prevents false positives
- **Hover popup "Loading..." fix**: Only installed extensions can start LSP servers (built-in registry gated by `all_ext_manifests`); mouse hover sends LSP request silently — popup appears only when content arrives; null-position suppression (`lsp_hover_null_pos`) prevents re-request loops; keyboard hover (`gh`) still shows "Loading..." with 3s auto-dismiss; empty/whitespace hover responses treated as null
- Files changed: `src/core/engine.rs`, `src/core/lsp_manager.rs`, `src/core/settings.rs`, `src/core/syntax.rs`, `src/render.rs`, `src/main.rs`, `src/tui_main.rs`

> Sessions 212 and earlier archived in **SESSION_HISTORY.md**.

