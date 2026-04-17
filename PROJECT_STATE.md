# VimCode Project State

**Last updated:** Apr 17, 2026 (Session 295 — Phase 5 begins, g-prefix audit, 4 gaps filed) | **Tests:** 1904 (lib) + 414 (nvim conformance)

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

**Session 295 — Phase 5 (#26) begins: `g`-prefix coverage audit:**

1. **Started Phase 5 `:help` coverage audit** — a new kind of conformance work that catches missing features rather than behavioural bugs. Scope: walk Vim's documentation section by section.
2. **First slice: `g`-prefix normal-mode commands** — 58 commands total. ✅ 36 implemented, 🟡 2 partial, ❌ 14 not implemented, ⏭️ 6 intentionally skipped (Ex mode, Select mode, mouse tag nav, debug features, redundant with VimCode's status bar).
3. **4 gap issues filed** for actionable missing features:
   - **#120** `gF` — edit file + jump to line number (the \`:N\` suffix case)
   - **#121** `g@` — user-definable operator via \`operatorfunc\` (enables plugin-defined operators)
   - **#122** `g<Tab>` — jump to last-accessed tab
   - **#123** Screen-line motions: \`g\$\`, \`g0\`, \`g^\`, \`g<End>\`, \`g<Home>\`
4. **New `COVERAGE_PHASE5.md`** — living document tracking the Phase 5 audit by slice.

**Session 294 — Fix #114: ex-command numeric line addresses are now 1-based:**

1. **Fix #114 — `parse_line_address` now 1-based for bare numbers** — Matches Vim's convention throughout. `":3"` → index 2, `":0"` → index 0 (used by copy/move as "before line 1"). Relative addresses (`+N`, `-N`, `.`, `$`) unchanged — they were already correct.
2. **Added `dest_is_zero` special case** to `execute_copy_command` (the single-line form) so `:copy 0` inserts at top, matching the existing range-version behaviour.
3. **6 existing tests updated** to encode 1-based semantics (`:1,2co3`, `:1m3`, `:t3`, `:m3`, `:co2`, `:copy 2`); 4 new tests added covering 1-based specifically and the `:N m 0` / `:copy 0` special case.

**Session 293 — Fix #116 visual block virtual-end append:**

1. **Fix #116: `<C-v>jj$A<text>` now appends at each line's actual end** — Extended `visual_block_insert_info` tuple with a `virtual_end: bool` flag. When `$` is pressed in visual block mode (sets `visual_dollar = true`), `A` captures that flag and the Esc handler appends `<text>` at each selected line's own end instead of the captured column. Correctly clarified the ignored test's keystroke sequence — the virtual-end trigger is `<C-v>...$A`, not `$<C-v>...A`.

**Session 292 — Phase 4 batch 16 (#25), 18 new conformance tests, #116 filed:**

1. **Phase 4 batch 16: 18 new Neovim-verified tests** — Covering visual block `I` (insert prefix), visual block `A` (append suffix), `:noh` clears search highlights, `:r` on nonexistent file, `:retab` tab-to-spaces, lowercase marks (`ma` / `'a` / `` `a ``), `n` with no prior search, word motions at EOF/BOF, visual indent/dedent (`V>`, `V<`), `5rX` count-prefix replace, `dap` delete-around-paragraph, `.` repeat last change, `u`/`<C-r>` undo-redo, `:set tabstop?` query.
2. **#116 filed** — Visual block started with `$<C-v>jjA` should virtual-append at each line's actual end (Vim behavior). VimCode uses the starting cursor column, so appending on a longer line inserts mid-word instead of at the end.

**Session 291 — Fix #112: ranged/concat/bang ex-command forms, #114 filed:**

1. **Fix #112 — Ranged `:copy`/`:move`, concat `:tN`/`:mN`/`:coN`, `:sort!`** — Added `execute_copy_range()` and `execute_move_range()` helpers with 1-based range semantics matching Vim. Extended `try_execute_ranged_command()` to dispatch `m`/`move`/`t`/`co`/`copy` keywords. Added `split_cmd_and_arg()` helper that matches a command name followed by a valid separator (digit/space/sign/`./$`). Accepted `:sort!` bang as reverse synonym.
2. **#114 filed** — VimCode's `parse_line_address` treats numeric dest as 0-based, but Vim uses 1-based throughout. Fix requires auditing existing callers; scoped as a separate issue so this PR stays focused.
3. **12 new unit tests** covering all new forms + regressions (`:0` still goes to line 0, `:sort r` still works).

**Session 290 — Phase 4 batch 15 (#25), 23 new conformance tests, #112 filed:**

1. **Phase 4 batch 15: 23 new Neovim-verified tests** — Covering `:copy`/`:move` (simple form), `:sort` (basic and reverse via `r` flag), `:sort u` unique, `gi` restart insert, `gv` reselect last visual, jump list (`<C-o>`/`<C-i>`), change list (`g;`), `:enew`, window move (`<C-w>H`), case operators (`gUw`, `guiw`, `g~w`), count+operator (`3dw`, `2cwXYZ`), text object edges (`daw` at word boundary, `das`), `:set number`/`nonumber`, `:pwd`.
2. **#112 filed** — Collected deviations discovered during mining: ranged `:copy`/`:move` forms don't accept range prefixes; `:t<N>` / `:m<N>` / `:co<N>` concatenated forms not recognized; `:sort!` bang not parsed (users must use `:sort r` for reverse).

**Session 289 — Phase 4 batch 14 (#25) + fixes for #109 and #110:**

1. **Phase 4 batch 14: 25 new Neovim-verified tests** — Covering areas still uncovered: named registers (`"ayy`/`"ap`/`"Ayy`/`"add`), folding (`zf`/`zR`/`zd`), window splits (`<C-w>s/v/w/q/o`, `:split`, `:vsplit`), `:echo`, `:w` error case, word-end motions (`e`, `ge`), increment/decrement edge cases, search history, numeric `:N` and `:N,M` ranges.
2. **Fix #109: Ctrl-A/Ctrl-X now parse hex (`0x..`) numbers correctly** — Added hex-prefix detection in `increment_number_at_cursor()` so cursor landing on or before the leading `0` of `0x09` now increments as hex → `0x0a` instead of decimal `1x09`. Also covers `-0x..`. 2 extra tests added (cursor-inside-hex, decrement).
3. **Fix #110: Yank to named register no longer overwrites register 0** — Updated `set_yank_register()` to only update `"0` when the target is the unnamed register (`"`). Matches Vim's `:help registers` semantics.
4. **Closed #60** housekeeping (PR #106 was already merged but issue wasn't auto-closed).

**Session 288 — #107 git_branch_changed plugin event (follow-up to #60):**

1. **Fire `git_branch_changed` plugin event** from `tick_git_branch()` when an external branch change is detected. Plugins (e.g. git-insights panel) can now subscribe via `vimcode.on("git_branch_changed", fn)` and refresh their UI instead of going stale.
2. **No new Lua API surface** — plugins already have `vimcode.git.branch()` to re-query state on the event.
3. **2 new unit tests**: plugin event fires on change, does NOT fire when branch unchanged (1815 → 1817 lib tests).
4. **EXTENSIONS.md updated** with the new event.

**Session 287 — Fix #60 Git branch status bar refresh:**

1. **Fix #60: Status bar now detects external branch changes** — Added `tick_git_branch()` method on Engine that polls `git::current_branch()` at most once per 2 seconds and returns `true` if the branch changed. Wired into all three backends (GTK, TUI, Win-GUI) via their existing tick loops; a detected change triggers a redraw.
2. **2 new unit tests** (rate-limit + change detection) — 1813 → 1815 lib tests.

**Session 286 — Fix #101 Replace mode Esc cursor position:**

1. **Fix #101: Replace mode cursor stepback on Esc** — `handle_replace_key` Esc handler in `src/core/engine/motions.rs` was missing the cursor-step-back that Insert mode already had. Added the same `col > 0 → col -= 1` logic. Also covers `gR` virtual replace.
2. **2 previously-ignored tests now passing** (1811 → 1813 lib, 11 → 9 ignored).

**Session 285 — Phase 4 batch 13 (#25), 25 new conformance tests, 0 new deviations:**

1. **Phase 4 batch 13: 25 new Neovim-verified tests** — Covering substitute (`:s/`, `:%s/`, flags `g`/`i`, empty replacement, no-match), global (`:g/pat/d`, `:v/pat/d`), tab navigation (`:tabnew`, `gt` cycle), G-motions (`dG`, `dgg`, `yG`), bigword motions (`W`, `B`, `E`, `gE`), f/F with count, comma-reverse, `%` bracket matching, register `"1` (last delete), linewise paste (`yyp`, `yyP`).
2. **All 25 tests pass on first run** — no new deviations discovered in these areas.

**Session 284 — Phase 4 batch 12 (#25), 27 new conformance tests, 1 new deviation (#101):**

1. **Phase 4 batch 12: 27 new Neovim-verified tests** — Covering areas previously under-tested: search (`/`, `?`, `n`, `N`, count prefix, wrap-around), scroll commands (`zz`, `<C-d>`, `<C-u>`, `<C-b>`), number increment/decrement (`<C-a>`, `<C-x>` with count and negatives), replace mode (`R`, `r<CR>`, `3rX`), case change (`gUU`, `guw`, `gUw`), and count+motion combos (`5l`, `3j`, `3dd`, `3yy+p`).
2. **1 new deviation documented (#101)**: Replace mode cursor lands at col+1 after `<Esc>` instead of on the last replaced char (Vim behavior). Documented as 2 ignored tests.

**Session 283 — Fix 3 Vim deviations (#97, #98, #99):**

1. **Fix #97: Visual line J now joins selected lines** — Added `J` handler in visual mode operator dispatch. `VjjJ` correctly joins all selected lines.
2. **Fix #98: :%join range now supported** — Added `%` range prefix handling in `execute_command()`. Also supports `%d` and `%y`.
3. **Fix #99: Ctrl-U in insert mode respects insert point** — Added `insert_enter_col` field to track where insert mode was entered. Ctrl-U now deletes only back to that boundary instead of line start.
4. **Closed #65** (already fixed in session 282, issue left open).
5. **4 previously-ignored tests now passing** (1757 → 1761 lib tests).

**Session 282 — Insert paste fix (#65), Phase 4 batches 10-11 (#25), 8 deviations fixed:**

1. **Fix #65: Ctrl-V paste in insert mode added cumulative indentation** — `paste_in_insert_mode()` was applying auto-indent to each pasted line, causing a staircase effect. Fixed by suppressing auto-indent during paste (pasted text already has its own whitespace).
2. **Phase 4 batches 10-11 (#25): 58 new Neovim-mined tests** — Mined from test_undo.vim, test_change.vim, test_put.vim, test_marks.vim, test_registers.vim, test_join.vim. Covering: undo/redo (5), put/paste (7), change operations (11), text objects (9), marks (4), registers (6), macros (2), join edge cases (5), insert mode keys (3), changelist navigation (2).
3. **Fixed 8 Vim deviations**: Vc/Vjc visual line change ate trailing newline; r\<CR\> was a no-op; S didn't preserve indent; daw at end of line didn't consume leading whitespace; tick mark jump ('a) went to col 0 instead of first non-blank; feed_keys didn't drain macro playback queue; updated test_visual_line_change to correct Vim expectation.
4. **3 new deviations documented** (ignored tests): visual J in line mode, :%join range not supported, Ctrl-U in insert deletes to line start instead of insert start.

> Session 281 and earlier in **SESSION_HISTORY.md**.
