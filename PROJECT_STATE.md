# VimCode Project State

**Last updated:** Apr 14, 2026 (Session 276 — Unified find/replace overlay) | **Tests:** 1383 (lib)

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 275 are in **SESSION_HISTORY.md**.

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

**Session 276 — Unified find/replace overlay (Ctrl+F):**

1. **Engine-level find/replace overlay** — New `FindReplacePanel` in `ScreenLayout`, rendered identically by all 3 backends (GTK Cairo, TUI ratatui, Win-GUI Direct2D). Replaces the GTK-only Revealer find dialog. VSCode-style layout: `[▶] [input] [Aa][ab][.*] [N of M] [↑][↓][≡][×]` find row, `[  ] [input] [AB] [R1][R*]` replace row. Positioned at top-right of active editor group (not window).
2. **GTK Revealer cleanup** — Removed the native GTK Entry/Button find dialog (Revealer widget, 8 Msg variants, handler code, CSS classes). Ctrl+F now passes through to the engine.
3. **Features:** Incremental search, replace current/all, case/whole-word/regex toggles, preserve-case toggle, find-in-selection (≡), match count, chevron expand/collapse for replace row, ↑/↓ navigation buttons, × close button, `ctrl_f_action` setting. Ctrl+Z undo passthrough. Ctrl+A select-all in input fields. Visual selection pre-fills find box (single-line) or auto-enables find-in-selection (multi-line). Regex uses multiline mode (`^`/`$` match line boundaries). Edit > Find and Edit > Replace menu items updated.
4. **Win-GUI mouse interactions** — Drag-to-select in input fields (`fr_input_dragging` state), double-click word select, cached `FindReplaceRect` for pixel-accurate click handling. Selection highlight rendering in both find and replace inputs.
5. **Nerd Font icons** — `FIND_REPLACE` (`\u{eb3c}`), `FIND_REPLACE_ALL` (`\u{eb3d}`), `FIND_IN_SEL` (`\u{eb54}`), `FIND_CLOSE` (`\u{ea76}`) with ASCII fallbacks.
6. **Known TUI/GTK gaps** — Mouse drag-select, double-click word select, and accurate ≡/× click handling only work in Win-GUI. TUI/GTK need porting (see BUGS.md and PLAN.md). 13 new tests.

> Session 275 and earlier in **SESSION_HISTORY.md**.
