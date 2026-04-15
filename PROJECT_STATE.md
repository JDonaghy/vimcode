# VimCode Project State

**Last updated:** Apr 15, 2026 (Session 280 — Fix 6 Vim deviations, Neovim conformance harness) | **Tests:** 1463 (lib) + 31 (nvim conformance)

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 279 are in **SESSION_HISTORY.md**.

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

**Session 280 — Fix 6 Vim deviations (#28-#33), Neovim conformance harness:**

1. **Fix #31: `2d2w` count multiplication** — Added `operator_count` field to Engine. When operator is set, count is saved separately; `take_count()` multiplies operator_count × motion_count. `2d2w` now correctly deletes 4 words.
2. **Fix #32: `<G` outdent** — Was a `send_keys()` test parser bug: `<G` was treated as a special key. Fixed parser to require closing `>`. Engine was already correct.
3. **Fix #30: `di</da<` angle bracket text objects** — Added `'<' | '>'` match arm to `find_text_object_range()` using existing `find_bracket_object()`.
4. **Fix #29: `da"/da'` trailing whitespace** — `find_quote_object()` for `modifier == 'a'` now includes trailing whitespace (or leading if no trailing), matching Vim spec.
5. **Fix #28: `d}/d{` paragraph boundary** — `d{` range is `[target, cursor-1]` (blank line deleted, cursor line preserved). `d}` range is `[cursor, target-1]` (cursor line deleted, blank line preserved). Verified against Neovim headless.
6. **Fix #33: Cursor position after `c+Esc`** — Added standard Vim cursor-left-on-Esc to insert mode Escape handler.
7. **Neovim conformance test harness** — `tests/nvim_conformance.rs`: 31 automated tests that run key sequences through both Neovim (headless) and VimCode, comparing buffer content + cursor position. Just add entries to `CASES` array — no manual testing needed. Requires `nvim` on PATH; skips gracefully if missing.

> Session 279 and earlier in **SESSION_HISTORY.md**.
