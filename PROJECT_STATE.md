# VimCode Project State

**Last updated:** Mar 9, 2026 (Session 156 — IDE Polish: Indent Guides, Bracket Matching, Auto-Pairs) | **Tests:** 2937

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 154 are in **SESSION_HISTORY.md**.

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

**Session 156 — IDE Polish: Indent Guides, Bracket Matching, Auto-Pairs (2937 tests):**
Three visual/editing polish features: (1) **Indent guides** — vertical `│` lines at each tabstop in TUI, thin Cairo lines in GTK; controlled by `indent_guides` setting (default on); active guide at cursor scope highlighted brighter; blank lines bridge surrounding indent levels. (2) **Bracket pair highlighting** — when cursor is on `(){}[]`, both brackets get a distinct background (`bracket_match_bg` theme color); `bracket_match` field on Engine updated at end of `handle_key()`; `match_brackets` setting (default on). (3) **Auto-close brackets/quotes** — typing `([{"'\`` in Insert mode inserts matching closer with cursor between; typing closer when next char matches skips over it; Backspace between a pair deletes both; smart context for quotes (only pair after whitespace/brackets/BOL); `auto_pairs` setting (default on). All three features have `:set` toggle support, settings UI entries, and theme colors across all 4 built-in themes. 29 integration tests in `tests/ide_polish.rs`.

**Session 155 — Core Commentary Feature (2908 tests):**
Unified comment toggling from three separate implementations (Lua plugin, Rust `toggle_comment_range()`, Rust `vscode_toggle_line_comment()`) into a single core module `src/core/comment.rs`. New `CommentStyle`/`CommentStyleOwned` types, `comment_style_for_language()` table covering 46+ languages (including block comments for HTML/CSS/XML), two-pass `compute_toggle_edits()` algorithm, `resolve_comment_style()` override chain (plugin → extension manifest → built-in → fallback `#`). Added `CommentConfig` to `ExtensionManifest` in `extensions.rs`. New `toggle_comment()` method on Engine replaces old `toggle_comment_range()` and `vscode_toggle_line_comment()`. Rewired `gcc`, visual `gc`, and VSCode `Ctrl+/` to use the new core. Added `:Comment` command (`:Commentary` kept as alias). Plugin API: `vimcode.set_comment_style(lang_id, {line, block_open, block_close})`. Fixed Ctrl+/ in GTK (key name `"slash"` not `"/"`) and TUI (crossterm byte 0x1F → `Char('7')` mapping). VSCode mode: added Ctrl+Q quit, F10 menu toggle, menu visible by default. 19 unit tests in `comment.rs`, 31 integration tests in `tests/commentary.rs`.

> Sessions 154 and earlier archived in **SESSION_HISTORY.md**.
