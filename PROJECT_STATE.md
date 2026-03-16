# VimCode Project State

**Last updated:** Mar 16, 2026 (Session 187 — Tab Context Menu Splits Fix) | **Tests:** 4498

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 186 are in **SESSION_HISTORY.md**.

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

### Session 187 — Tab Context Menu Splits Fix (Mar 16, 2026)
- **Fixed GTK/TUI split inconsistency**: GTK tab context menu "Split Right"/"Split Down" was calling `open_editor_group()` (creating new editor groups) while engine's `context_menu_confirm()` called `split_window()` (Vim window splits). Fixed GTK to call `split_window()` matching the engine.
- **Added 4 split options to tab context menu**: "Split Right" and "Split Down" create Vim window splits within the current tab; "Split Right to New Group" and "Split Down to New Group" create new editor groups (VSCode-style). Both backends now behave identically.
- **README clarified**: Added 3-layer explainer (Windows/Tabs/Editor Groups) in Multi-File Editing section; renamed "Editor Groups" to "Editor Groups / Tab Groups"; added clarifying note to Windows section.
- 4 new tests in `tests/context_menu.rs`.

> Sessions 186 and earlier archived in **SESSION_HISTORY.md**.
