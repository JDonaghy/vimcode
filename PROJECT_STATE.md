# VimCode Project State

**Last updated:** Apr 15, 2026 (Session 279 — Vim conformance matrix tests, `:set` audit) | **Tests:** 1463 (lib)

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 278 are in **SESSION_HISTORY.md**.

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

**Session 279 — Vim conformance matrix tests, `:set` option audit:**

1. **Operator × motion matrix tests (Phase 1)** — 29 new parametric tests with 93 total test cases covering all 7 chunks: `d` × 14 motions + 16 text objects, `c` × 8 motions + 6 text objects, `y` × 7 motions + linewise + text objects, `>>` / `<<` × line motions, `gU`/`gu`/`g~` × 16 cases, count variations (3dw, d3w, 3dd), dot-repeat (dw., dd., x., 3x., cw+text., >>., 2dw., 3dd.).
2. **Test infrastructure** — `send_keys(engine, "d2w")` helper parsing key strings including `<Esc>`, `<CR>`, `<C-x>`, and literal `<`/`>`. `setup_engine(text, line, col)` helper.
3. **Bug fix: `ge`/`gE` motion** — `move_word_end_backward()` went back two words instead of one. Completely rewrote: go to start of current word → back one → skip whitespace → land at end of previous word. Also fixed `move_bigword_end_backward()`.
4. **Bug fix: leader key intercepted `df<space>`** — Default leader (space) hijacked any operator+find+space combo. Added `pending_operator`/`pending_find_operator`/`pending_text_object` guards.
5. **Bug fix: `dw`/`de`/`db` dot-repeat broken** — `apply_operator_with_motion()` never set `last_change`, and `repeat_last_change()` only handled `Motion::Right` (x) and `Motion::DeleteLine` (dd). Added recording + `WordForward`/`WordBackward`/`WordEnd` handlers.
6. **Bug fix: `send_keys` parsed `<<` as special key** — Fixed by only treating `<` as sequence opener when followed by uppercase letter.
7. **`:set` option audit (Phase 2)** — 18 new tests: round-trip test (33 settings), behavior tests (expand_tab, shift_width, ignorecase, smartcase, scrolloff, splitbelow, splitright, auto_pairs, hlsearch), ex-command tests (`:set` toggle, `:set key=val`, `:set key?`, error handling).
8. **6 Vim deviations documented** — d}/d{ paragraph boundary, da"/da' trailing space, di</da< not implemented, 2d2w count multiplication, <G motion, cursor after c+Esc.
9. **47 new tests** — operator matrix (29), `:set` audit (18). Total: 1463.

> Session 278 and earlier in **SESSION_HISTORY.md**.
