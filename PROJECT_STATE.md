# VimCode Project State

**Last updated:** Apr 15, 2026 (Session 281 — Linewise paste fix, Phase 3 Lua API, Phase 4 test mining, 12 deviations fixed) | **Tests:** 1689 (lib) + 31 (nvim conformance)

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

**Session 281 — Fix linewise paste clipboard bug (#64), Phase 3 Lua API (#24):**

1. **Fix #64: Linewise paste (P/p) lost `is_linewise` through clipboard round-trip** — Two bugs: (a) Win-GUI `clipboard_write` passed multi-line text as a `-Value` argument to `Set-Clipboard`, but PowerShell splits on newlines — only the first line was written. Fixed by piping via stdin. (b) `load_clipboard_for_paste()` compared clipboard text with an exact match, but the OS round-trip changes `\n` → `\r\n` and strips trailing newlines. Fixed by normalizing CRLF→LF and comparing without trailing newlines.
2. **Phase 3 (#24): feedkeys, eval, get_lines, set_lines Lua API** — Added 4 Neovim-compatible Lua API functions: `vimcode.feedkeys(keys)` for injecting keystrokes, `vimcode.eval(expr)` for registers/options/cursor, `vimcode.buf.get_lines(start, end)` and `vimcode.buf.set_lines(start, end, lines)` for 0-indexed range buffer access. Public `Engine::feed_keys()` method extracted from test helper. 18 new tests.
3. **Phase 4 first third (#25): Neovim test mining** — 233 tests mined from Neovim's test_normal.vim, test_textobjects.vim, test_visual.vim, test_search.vim across 9 batches. 22 Vim deviations discovered, 12 fixed, 10 remaining as open issues. Fixed cw dot-repeat trailing space. Fixed pre-existing d}/dge/da` integration test expectations.
4. **Fixed 12 Vim deviations**: #64 (linewise paste), #68 (dw empty line), #69 (dd trailing newline), #70 (cw whitespace), #71 (gugu), #73 (J cursor + 3J count), #74 (3rX cursor), #81 (C off-by-one), #83 (gJ cursor), #84 (ci( empty), #86 (J trailing space), #87 (ib/aB aliases).
5. **CI improvements**: Windows TUI now runs integration tests, fixed snapshot and GTK clippy warnings.
6. **Created issues #64-65, #68-90.**

> Session 280 and earlier in **SESSION_HISTORY.md**.
