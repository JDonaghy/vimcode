# VimCode Project State

**Last updated:** Apr 14, 2026 (Session 278 — Find/replace hit regions, colorcolumn, `x`+`.` fix) | **Tests:** 1416 (lib)

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

**Session 278 — Find/replace hit regions, colorcolumn, `x`+`.` fix, CLAUDE.md rules:**

1. **Centralized find/replace hit-test geometry** — `FindReplaceClickTarget` enum (13 variants), `FrHitRegion` struct, `compute_find_replace_hit_regions()` in `engine/mod.rs`. All 3 backends use shared hit regions instead of computing geometry independently.
2. **Shared click dispatch** — `Engine::handle_find_replace_click()` eliminates ~30 lines of duplicated dispatch per backend.
3. **TUI mouse rewrite** — Hit-region-based dispatch, `fr_input_dragging` for drag-to-select, double-click word select via `find_word_boundaries()`.
4. **GTK click handler fixed** — Hit regions fix 3 geometry mismatches (`info_w`, `popup_y`, toggle widths).
5. **Win-GUI migrated to shared dispatch** — Pixel hit-testing preserved, action dispatch shared.
6. **Visual selection preserved during Ctrl+F** — Frozen via `find_replace_visual_end`, cleared on overlay close.
7. **Dynamic panel width** — Input field shrinks for long match counts, ensuring ≡/× buttons always visible.
8. **`:set colorcolumn` implemented** — `colorcolumn_positions()` parses comma-separated columns with `+N`/`-N` relative offsets. `colorcolumn_bg` theme color derived from `background.colorcolumn_tint()`. Rendered in all 3 backends. VSCode import: `editorRuler.foreground`. 10 new tests.
9. **`x` with count + `.` repeat fixed** — `repeat_last_change()` was looping `final_count` times with `change.count` inside (4×4=16 instead of 4). Fixed for both `x` and `dd`. 3 new tests.
10. **CLAUDE.md rules elevated** — Theme colors (never hardcode hex), testing (never `cargo test` with win-gui), hit region pattern.
11. **Crate extraction roadmap** — 7 future plan items. Vim conformance roadmap — 5 audit items.
12. **27 new tests** — find/replace (14), colorcolumn (10), `x`+`.` repeat (3). Total: 1416.

> Session 277 and earlier in **SESSION_HISTORY.md**.
