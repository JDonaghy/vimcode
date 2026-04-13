# VimCode Project State

**Last updated:** Apr 13, 2026 (Session 272 — Win-GUI git panel rendering parity, click/hover/double-click interactivity, tab scroll-into-view) | **Tests:** 5478

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 268 are in **SESSION_HISTORY.md**.

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

**Session 272 — Win-GUI git panel rendering parity + tab scroll-into-view:**

**Win-GUI git panel full renderer (rewrite):**
1. **`draw_git_panel()` rewritten** (~300 lines, was ~100) — Full panel background fill, themed header with branch + ahead/behind, commit input box (multi-line with cursor), button row (Commit/Push/Pull/Sync with focus/hover states), 4 collapsible sections (Staged Changes, Changes, Worktrees, Recent Commits), selection highlight on focused item, file status coloring (added/deleted/modified), path truncation with ellipsis, scrollbar, hint bar ("Press '?' for help"), branch picker popup (search + create modes), help dialog overlay with 15 keybindings.

**Git panel click interactivity:**
2. **Section item clicks** — Pixel-based Y coordinate mapping to `sc_visual_row_to_flat()` for section headers (expand/collapse) and file items (select).
3. **Commit input click** — Clicking the commit message area activates input mode.
4. **Button row clicks** — Commit/Push/Pull/Sync buttons dispatch via `handle_sc_key()`.
5. **Double-click on files** — Opens diff split (or file for untracked) via `handle_sc_key("Return", ...)`.
6. **Button hover tracking** — `on_mouse_move()` tracks mouse over button row, updates `sc_button_hovered` for visual highlight feedback.
7. **Panel hover dwell** — `panel_hover_mouse_move("source_control", ...)` called on mouse move over section items, enabling commit log hover popups with full commit details. Dismiss on mouse-out (unless over popup).

**Tab bar scroll-into-view (Win-GUI bug fix):**
8. **`set_tab_visible_count()` reporting** — Win-GUI now reports available tab bar width (in character columns) to the engine after each paint frame. Calls `ensure_all_groups_tabs_visible()` so newly opened tabs (e.g. from git diff) scroll into view. Previously `tab_bar_width` was never set, defaulting to `usize::MAX`, so `ensure_active_tab_visible()` was a no-op.

> Sessions 271 and earlier in **SESSION_HISTORY.md**.
