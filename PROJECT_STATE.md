# VimCode Project State

**Last updated:** Mar 14, 2026 (Session 180b — Spell Checker Bug Fixes + UI Polish) | **Tests:** 4316

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 180 are in **SESSION_HISTORY.md**.

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

### Session 180b — Spell Checker Bug Fixes + UI Polish (Mar 14, 2026)
- **z= suggestions**: numbered list UI with single-key selection (1-9, a-z), like Neovim; `spell_suggestions` state intercepts keys at top of `handle_key()`
- **Markdown spell checking**: fixed `has_syntax` detection — was using `!highlights.is_empty()` (wrong: all files get Rust parser as fallback); now uses `SyntaxLanguage::from_path()` to check if file has recognized syntax
- **Undo/dirty tracking**: spell replacements now use `delete_with_undo()`/`insert_with_undo()` + `set_dirty(true)` instead of raw buffer ops
- **GTK scrollbar width**: halved from 10px to 5px (scrollbar widget + cursor indicator + margin + height)
- **Text overflow behind scrollbar**: subtracted 5px scrollbar width from `render_viewport_cols` in `render.rs`
- **Group divider grab**: fixed hit-test and drag handler bounds — was using `height - 2.0 * line_height` instead of properly subtracting wildmenu/debug toolbar/quickfix/terminal panel heights to match actual editor bounds
- 2 new tests (4316 total)

> Sessions 180 and earlier archived in **SESSION_HISTORY.md**.
