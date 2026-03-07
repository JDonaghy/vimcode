# VimCode Project State

**Last updated:** Mar 6, 2026 (Session 140 — Vim compat batch: 29 commands) | **Tests:** 2545

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 140 are in **SESSION_HISTORY.md**.

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

**Session 140 — Vim compatibility batch: 29 new commands (2545 tests):**
Implemented 29 missing Vim commands in two tiers, raising VIM_COMPATIBILITY.md from 319/411 (78%) to 348/411 (85%). **Tier 1 (quick wins):** `+`/`-`/`_` line motions, `|` column motion, `gp`/`gP` paste with cursor after, `@:` repeat last ex command, backtick text objects (`` i` ``/`` a` ``), insert `CTRL-E`/`CTRL-Y` (char below/above), visual `r{char}`, `&` repeat last `:s`, `CTRL-W q`/`n`. **Tier 2 (medium effort):** `CTRL-W +`/`-`/`<`/`>`/`=`/`_`/`|` resize/equalize/maximize, `[{`/`]}`/`[(`/`])` unmatched bracket jumps, `[m`/`]m`/`[M`/`]M` method navigation, `[[`/`]]`/`[]`/`][` section navigation. Added `last_ex_command`/`last_substitute` fields to Engine, `set_all_ratios()` to GroupLayout, 45 integration tests in `tests/vim_compat_batch.rs`. Sections now at 100%: Text Objects (16/16).

> Sessions 139 and earlier archived in **SESSION_HISTORY.md**.
