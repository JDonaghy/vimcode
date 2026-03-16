# VimCode Project State

**Last updated:** Mar 15, 2026 (Session 184 — Right-Click Context Menus) | **Tests:** 4460

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 183 are in **SESSION_HISTORY.md**.

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

### Session 184 — Right-Click Context Menus (Mar 15, 2026)
- **Explorer right-click context menu**: Different menus for files vs folders (matching VSCode). File menu: Open to Side, Open Containing Folder, Select for Compare, Copy Path, Copy Relative Path, Rename, Delete. Folder menu: New File, New Folder, Open Containing Folder, Find in Folder, Copy Path, Copy Relative Path, Rename, Delete.
- **Tab bar right-click context menu**: Close, Close Others, Close to Right, Close Saved, Copy Path, Copy Relative Path, Reveal in File Explorer, Split Right, Split Down. Disabled items when not applicable (e.g., Close Others with 1 tab).
- **Engine data model**: `ContextMenuState` / `ContextMenuTarget` structs; `open_explorer_context_menu()` / `open_tab_context_menu()` / `handle_context_menu_key()` methods; `context_menu: Option<ContextMenuState>` engine field.
- **TUI rendering**: `render_context_menu_popup()` with box-drawing borders; mouse hover highlighting via `MouseEventKind::Moved` handler; left-click confirms, right-click/Escape dismisses.
- **GTK rendering**: `PopoverMenu::from_model()` with `gio::Menu` sections + `SimpleActionGroup` actions; native hover highlighting; `swap_ctx_popover()` pattern for lifecycle management; suppressed non-fatal `gtk_css_node_insert_after` GTK4 assertion via GLib log handler.
- **render.rs**: `ContextMenuPanel` / `ContextMenuRenderItem` structs; `build_context_menu_panel()` produces platform-agnostic data.
- 38 new tests (4460 total); `tests/context_menu.rs` integration test file.

> Sessions 183 and earlier archived in **SESSION_HISTORY.md**.
