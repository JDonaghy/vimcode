# VimCode Project State

**Last updated:** Mar 6, 2026 (Session 142 — Vim compat batch 3: 15 commands) | **Tests:** 2612

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 142 are in **SESSION_HISTORY.md**.

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

**Session 142 — Vim compatibility batch 3: 15 new commands (2612 tests):**
Implemented 15 more missing Vim commands, raising VIM_COMPATIBILITY.md from 380/403 (94%) to 400/414 (97%). `g?{motion}` ROT13 encode (with text objects), `CTRL-@` insert previous text + exit, `CTRL-V {char}` insert literal character, `CTRL-O` auto-return to Insert after one Normal command, `!{motion}{filter}` filter through external command, `CTRL-W H/J/K/L` move window to far edge, `CTRL-W T` move window to new group, `CTRL-W x` exchange windows, visual block `I`/`A` (insert/append applied to all block lines on Escape), `o_v`/`o_V` force charwise/linewise motion mode. Added `insert_ctrl_o_active`, `insert_ctrl_v_pending`, `visual_block_insert_info`, `force_motion_mode` fields. Enhanced `apply_operator_text_object()` with case/ROT13/indent/filter support. 29 integration tests in `tests/vim_compat_batch3.rs`. Sections now at 100%: Window commands (31/31), Visual mode (26/26), Editing (51/51).

**Session 141 — Vim compatibility batch 2: 27 new commands (2583 tests):**
Implemented 27 more missing Vim commands, raising VIM_COMPATIBILITY.md from 348/403 (85%) to 380/403 (94%). **Tier 1 (quick wins):** `ga` ASCII value, `g8` UTF-8 bytes, `go` byte offset, `gm`/`gM` middle of screen/text, `gI` insert at column 1, `gx` open URL, `g'`/`` g` `` mark without jumplist, `g&` repeat `:s` globally, `CTRL-^` alternate buffer, `CTRL-L` redraw, `N%` go to N% of file, `zs`/`ze` scroll cursor to left/right edge, `:b {name}` buffer by name, `:make`. **Tier 2 (medium effort):** `gq{motion}`/`gw{motion}` format operators (with text object support), `CTRL-W p`/`t`/`b` window navigation, `CTRL-W f`/`d` split+open/definition, insert `CTRL-A` repeat last insertion, insert `CTRL-G u`/`j`/`k` break undo/move, visual `gq`/`g CTRL-A`/`g CTRL-X`. Added `prev_active_group`/`insert_ctrl_g_pending` fields, `format_lines()` method, 38 integration tests in `tests/vim_compat_batch2.rs`. Sections now at 100%: Movement (48/48), Editing (50/50), z-commands (23/23).

> Sessions 140 and earlier archived in **SESSION_HISTORY.md**.
