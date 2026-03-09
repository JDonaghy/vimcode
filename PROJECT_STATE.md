# VimCode Project State

**Last updated:** Mar 9, 2026 (Session 157 — VSCode Mode Fixes + Build Portability) | **Tests:** 2941

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

**Session 157 — VSCode Mode Fixes + Build Portability (2941 tests):**
Fixed auto-pairs, bracket matching, and `update_bracket_match()` not running in VSCode mode (early return in `handle_key()` bypassed all three). Added auto-pair insert/skip-over/backspace-delete logic to `handle_vscode_key()`. Added `update_bracket_match()` call at end of `handle_vscode_key()`. 4 new VSCode-mode auto-pair tests. **Build portability**: `vcd` TUI binary now statically linked with musl (`--target x86_64-unknown-linux-musl`) — runs on any Linux without glibc version issues (Ubuntu 22.04+, CentOS 7+, Alpine). Fixed Flatpak build: replaced `floor_char_boundary` (Rust 1.82+) with `is_char_boundary` loop, replaced `is_none_or` (Rust 1.82+) with `map_or(true, ...)` for GNOME SDK 47 Rust ~1.80 compat. Updated `release.yml` workflow. Released v0.3.1.

**Session 156 — IDE Polish: Indent Guides, Bracket Matching, Auto-Pairs (2937 tests):**
Three visual/editing polish features: (1) **Indent guides** — vertical `│` lines at each tabstop in TUI, thin Cairo lines in GTK; controlled by `indent_guides` setting (default on); active guide at cursor scope highlighted brighter; blank lines bridge surrounding indent levels. (2) **Bracket pair highlighting** — when cursor is on `(){}[]`, both brackets get a distinct background (`bracket_match_bg` theme color); `bracket_match` field on Engine updated at end of `handle_key()`; `match_brackets` setting (default on). (3) **Auto-close brackets/quotes** — typing `([{"'\`` in Insert mode inserts matching closer with cursor between; typing closer when next char matches skips over it; Backspace between a pair deletes both; smart context for quotes (only pair after whitespace/brackets/BOL); `auto_pairs` setting (default on). All three features have `:set` toggle support, settings UI entries, and theme colors across all 4 built-in themes. 29 integration tests in `tests/ide_polish.rs`.

> Sessions 155 and earlier archived in **SESSION_HISTORY.md**.
