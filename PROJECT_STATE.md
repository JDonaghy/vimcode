# VimCode Project State

**Last updated:** Mar 8, 2026 (Session 153 — Richer Lua Plugin API + Commentary + User Keymaps) | **Tests:** 2809

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 152 are in **SESSION_HISTORY.md**.

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

**Session 153 — Richer Lua Plugin API + VimCode Commentary + User Keymaps (2809 tests):**
**Plugin API expansion (Phase 1+2):** Extended `PluginCallContext` with new input/output fields. New Lua APIs: `vimcode.buf.set_cursor(line,col)`, `vimcode.buf.insert_line(n,text)`, `vimcode.buf.delete_line(n)`, `vimcode.opt.get(key)`/`vimcode.opt.set(key,value)`, `vimcode.state.mode()`/`register(char)`/`set_register(char,content,linewise)`/`mark(char)`/`filetype()`. New autocmd events: `BufWrite`, `BufNew`, `BufEnter`, `InsertEnter`, `InsertLeave`, `ModeChanged`, `VimEnter`. Centralized `set_mode()` method fires mode-change events. Visual/command mode keymap fallbacks. Plugin `set_lines` now records undo operations for proper undo support. **VimCode Commentary plugin**: Bundled extension (`extensions/commentary/`) inspired by tpope's vim-commentary. `gcc` toggles comment on current line (count-aware), `gc` in visual mode toggles comment on selection, `:Commentary [N]` command. Comment string auto-detected from 40+ language IDs. Engine-level `toggle_comment_range()` for visual mode with undo group support. 22 plugin API tests + 17 commentary tests in `tests/extensions.rs`. **User-configurable keymaps**: `keymaps: Vec<String>` in settings.json, format `"mode keys :command"`; `UserKeymap` struct parsed at engine init; multi-key sequence support with replay; checked before built-in handlers in `handle_key()`; supports `{count}` substitution; `:map`/`:unmap` runtime commands. 13 tests in `tests/user_keymaps.rs`.

> Sessions 152 and earlier archived in **SESSION_HISTORY.md**.
