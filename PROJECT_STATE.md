# VimCode Project State

**Last updated:** Apr 14, 2026 (Session 278 — Find/replace hit regions + shared dispatch) | **Tests:** 1403 (lib)

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 277 are in **SESSION_HISTORY.md**.

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

**Session 278 — Find/replace hit regions, shared dispatch, visual selection preservation:**

1. **Centralized find/replace hit-test geometry** — `FindReplaceClickTarget` enum (13 variants), `FrHitRegion` struct, and `compute_find_replace_hit_regions()` in `engine/mod.rs` compute clickable regions in abstract char-cell units. All 3 backends now use the same hit regions instead of computing geometry independently.
2. **Shared click dispatch** — `Engine::handle_find_replace_click()` in `search.rs` dispatches all find/replace mouse actions. Eliminates ~30 lines of duplicated match-on-index dispatch per backend.
3. **TUI mouse rewrite** — Replaced 145-line independent geometry block with hit-region-based dispatch. Added `fr_input_dragging` for drag-to-select, double-click word select via shared `find_word_boundaries()`.
4. **GTK click handler fixed** — Rewrote using hit regions. Fixes 3 geometry mismatches: `info_w` (60→80), `popup_y` calculation, toggle button widths (char_width×len → Pango measurement alignment).
5. **Win-GUI migrated to shared dispatch** — Pixel hit-testing unchanged (already working), but action dispatch uses `handle_find_replace_click()`.
6. **Visual selection preserved during Ctrl+F** — Opening find/replace from Visual mode keeps the selection highlight visible (frozen via `find_replace_visual_end`). Cursor jumps to matches but selection stays fixed. Cleared on overlay close.
7. **Dynamic panel width for match count** — Find/replace panel input field shrinks dynamically when match count string is long (e.g. "62 of 112"), ensuring ≡ and × buttons always fit within the panel. Fixes TUI button overflow.
8. **Crate extraction roadmap** — Added 7 future plan items: hit regions for tab bar, status bar, sidebar, context menus; abstract event layer; `vimcode-core` crate extraction; proof-of-concept SQL client.
9. **14 new tests** — `find_word_boundaries` (5), `handle_find_replace_click` (5), hit region computation (3), visual selection pre-fill (1).

> Session 277 and earlier in **SESSION_HISTORY.md**.
