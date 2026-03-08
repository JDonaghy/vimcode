# VimCode Project State

**Last updated:** Mar 7, 2026 (Session 145 — VSCode themes, crash fix, sidebar nav) | **Tests:** 2650

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 144 are in **SESSION_HISTORY.md**.

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

**Session 145 — VSCode theme loader, TUI crash fix, sidebar navigation (2650 tests):**
VSCode theme support: drop `.json` theme files into `~/.config/vimcode/themes/`, apply with `:colorscheme <name>`. `Theme::from_vscode_json(path)` parses VSCode `colors` (~25 UI keys) + `tokenColors` (~15 TextMate scopes), maps to our 55-field Theme struct. `Color::try_from_hex()` (non-panicking, supports #rrggbb/#rrggbbaa/#rgb), `Color::lighten()`/`darken()` for deriving missing colors, `strip_json_comments()` for JSONC. `Theme::available_names()` now returns built-in + custom themes from disk. `:colorscheme` command updated to accept/list custom themes. 4 unit tests for theme loader. **Crash fix**: `byte_to_char_idx` in TUI panicked on multi-byte UTF-8 chars (e.g. `─`); now uses `floor_char_boundary()` to snap to valid char boundaries. **Swap recovery fix**: R/D/A keys didn't work in TUI because `handle_swap_recovery_key` only checked `key_name` (empty in TUI for regular chars); now also checks `unicode`. Message prompt was cleared on keypress; now preserved when `swap_recovery` is pending. **TUI sidebar navigation**: `Ctrl-W h/l` navigates between toolbar→sidebar→editor (extends Vim window navigation). `sidebar_sel_bg`/`sidebar_sel_bg_inactive` theme colors for focused/unfocused selection. Clicking editor area clears all sidebar/toolbar focus. `toolbar_focused`/`pending_ctrl_w` on `TuiSidebar`. `window_nav_overflow` on Engine signals leftmost/rightmost boundary hits.

**Session 144 — Vim compatibility batch 4: 10 commands (2642 tests):**
Implemented 10 more missing Vim commands, raising VIM_COMPATIBILITY.md from 400/414 (97%) to 406/414 (98%). `Ctrl-G` show file info, `gi` insert at last insert position, `Ctrl-W r/R` rotate windows, `[*/]*/[/]/]` comment nav, `do/dp` diff obtain/put, `o_CTRL-V` force blockwise. 21 integration tests in `tests/vim_compat_batch4.rs`.

> Sessions 143 and earlier archived in **SESSION_HISTORY.md**.
