# VimCode Project State

**Last updated:** Mar 6, 2026 (Session 141 — Vim compat batch 2: 27 commands) | **Tests:** 2583

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 141 are in **SESSION_HISTORY.md**.

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

**Session 141 — Vim compatibility batch 2: 27 new commands (2583 tests):**
Implemented 27 more missing Vim commands, raising VIM_COMPATIBILITY.md from 348/403 (85%) to 380/403 (94%). **Tier 1 (quick wins):** `ga` ASCII value, `g8` UTF-8 bytes, `go` byte offset, `gm`/`gM` middle of screen/text, `gI` insert at column 1, `gx` open URL, `g'`/`` g` `` mark without jumplist, `g&` repeat `:s` globally, `CTRL-^` alternate buffer, `CTRL-L` redraw, `N%` go to N% of file, `zs`/`ze` scroll cursor to left/right edge, `:b {name}` buffer by name, `:make`. **Tier 2 (medium effort):** `gq{motion}`/`gw{motion}` format operators (with text object support), `CTRL-W p`/`t`/`b` window navigation, `CTRL-W f`/`d` split+open/definition, insert `CTRL-A` repeat last insertion, insert `CTRL-G u`/`j`/`k` break undo/move, visual `gq`/`g CTRL-A`/`g CTRL-X`. Added `prev_active_group`/`insert_ctrl_g_pending` fields, `format_lines()` method, 38 integration tests in `tests/vim_compat_batch2.rs`. Sections now at 100%: Movement (48/48), Editing (50/50), z-commands (23/23).

**Session 140 — Vim compatibility batch: 29 new commands (2545 tests):**
Implemented 29 missing Vim commands in two tiers. **Tier 1:** `+`/`-`/`_` line motions, `|` column motion, `gp`/`gP` paste with cursor after, `@:` repeat last ex command, backtick text objects, insert `CTRL-E`/`CTRL-Y`, visual `r{char}`, `&` repeat last `:s`, `CTRL-W q`/`n`. **Tier 2:** `CTRL-W +`/`-`/`<`/`>`/`=`/`_`/`|` resize/equalize/maximize, `[{`/`]}`/`[(`/`])` unmatched bracket jumps, `[m`/`]m`/`[M`/`]M` method navigation, `[[`/`]]`/`[]`/`][` section navigation.

> Sessions 139 and earlier archived in **SESSION_HISTORY.md**.
