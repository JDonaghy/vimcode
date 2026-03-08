# VimCode Project State

**Last updated:** Mar 7, 2026 (Session 147 — TUI interactive settings panel) | **Tests:** 2677

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

**Session 147 — TUI interactive settings panel (2677 tests):**
Replaced the read-only TUI settings panel with a full interactive form matching the GTK version. Moved `SettingType`/`SettingDef`/`SETTING_DEFS` from `render.rs` to `settings.rs` (core-accessible). New `DynamicEnum` variant for runtime-computed options (colorscheme now includes custom VSCode themes from `~/.config/vimcode/themes/`). Engine fields: `settings_has_focus`, `settings_selected`, `settings_scroll_top`, `settings_query`, `settings_input_active`, `settings_editing`, `settings_edit_buf`, `settings_collapsed`. `handle_settings_key()` supports three input modes: search filter, inline string/int editing, and normal navigation (j/k, Space/Enter toggle bools, Enter/l/h cycle enums, Enter opens inline edit). `settings_paste()` for Ctrl+V clipboard into search/edit fields. TUI renders: header, `/` search bar with cursor, scrollable categorized form (`[✓]`/`[ ]` bools, `value ▸` enums, right-aligned ints/strings), inline editing with cursor, scrollbar. 10 integration tests.

**Session 146 — Breadcrumbs bar (2667 tests):**
VSCode-like breadcrumbs bar showing file path segments + tree-sitter symbol hierarchy (e.g. `src › core › engine.rs › Engine › handle_key`) below the tab bar. `BreadcrumbSymbol` struct + `Syntax::enclosing_scopes()` walks parent chain for 10 languages (Rust/Python/JS/TS/Go/C/C++/Java/C#/Ruby). `BreadcrumbSegment`/`BreadcrumbBar` render structs. `breadcrumb_bg/fg/active_fg` theme colors in all 4 built-in themes + VSCode theme loader. `Settings.breadcrumbs: bool` (default true, `:set breadcrumbs`/`:set nobreadcrumbs`). Each editor group gets its own breadcrumb bar. Space reserved via doubled `tab_bar_height` when enabled. GTK `draw_breadcrumb_bar()` + TUI `render_breadcrumb_bar()`. 14 new tests (11 integration + 3 unit).

**Session 145 — VSCode theme loader, TUI crash fix, sidebar navigation (2650 tests):**
VSCode theme support: drop `.json` theme files into `~/.config/vimcode/themes/`, apply with `:colorscheme <name>`. `Theme::from_vscode_json(path)` parses VSCode `colors` (~25 UI keys) + `tokenColors` (~15 TextMate scopes), maps to our 55-field Theme struct. `Color::try_from_hex()` (non-panicking, supports #rrggbb/#rrggbbaa/#rgb), `Color::lighten()`/`darken()` for deriving missing colors, `strip_json_comments()` for JSONC. `Theme::available_names()` now returns built-in + custom themes from disk. `:colorscheme` command updated to accept/list custom themes. 4 unit tests for theme loader. **Crash fix**: `byte_to_char_idx` in TUI panicked on multi-byte UTF-8 chars (e.g. `─`); now uses `floor_char_boundary()` to snap to valid char boundaries. **Swap recovery fix**: R/D/A keys didn't work in TUI because `handle_swap_recovery_key` only checked `key_name` (empty in TUI for regular chars); now also checks `unicode`. Message prompt was cleared on keypress; now preserved when `swap_recovery` is pending. **TUI sidebar navigation**: `Ctrl-W h/l` navigates between toolbar→sidebar→editor (extends Vim window navigation). `sidebar_sel_bg`/`sidebar_sel_bg_inactive` theme colors for focused/unfocused selection. Clicking editor area clears all sidebar/toolbar focus. `toolbar_focused`/`pending_ctrl_w` on `TuiSidebar`. `window_nav_overflow` on Engine signals leftmost/rightmost boundary hits.

> Sessions 144 and earlier archived in **SESSION_HISTORY.md**.
