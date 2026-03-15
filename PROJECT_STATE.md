# VimCode Project State

**Last updated:** Mar 15, 2026 (Session 183 — Vim Compatibility Gap Closure) | **Tests:** 4422

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 182 are in **SESSION_HISTORY.md**.

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

### Session 183 — Vim Compatibility Gap Closure (Mar 15, 2026)
- **`[#`/`]#` preprocessor navigation**: Jump to matching `#if`/`#ifdef`/`#ifndef`/`#else`/`#elif`/`#endif` directives with depth tracking; supports nesting, indented directives, count prefix. `PreprocKind` enum + `jump_preproc_forward()`/`jump_preproc_backward()`/`preproc_directive()` methods.
- **`gR` virtual replace mode**: Enter with `gR`; overwrites characters like `R` but expands tabs to spaces (inserts `tabstop` spaces instead of overwriting with tab character). `virtual_replace: bool` engine field; modified `handle_replace_key()`.
- **`g+`/`g-` timeline undo**: Chronological undo navigation via `undo_timeline: Vec<(String, Cursor)>` on `BufferState`; `record_timeline_snapshot()` captures state after each undo group; `g_earlier()`/`g_later()` navigate the timeline independent of the undo tree.
- **`q:`/`q/`/`q?` command-line window**: Opens command or search history in a scratch buffer tab; Enter on a line executes the command or performs the search; `q` closes the window. `open_cmdline_window()`/`cmdline_window_execute()` methods; `is_cmdline_buf`/`cmdline_is_search` fields on `BufferState`.
- **VIM_COMPATIBILITY.md**: 412/417 → 414/417 (99%). Bracket 13/13 (100%), g-commands 34/34 (100%), Normal Other 32/33 (97%). Only remaining gap: `CTRL-]` (partial via `gd`).
- 31 new tests (4422 total)

> Sessions 182 and earlier archived in **SESSION_HISTORY.md**.
