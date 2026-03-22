# VimCode Project State

**Last updated:** Mar 21, 2026 (Session 203 — VSCode Mode Git Insights + Hover Popup Fixes) | **Tests:** 4649

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 203 are in **SESSION_HISTORY.md**.

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

### Session 203 — VSCode Mode Git Insights + Hover Popup Fixes (Mar 21, 2026)
- **VSCode edit mode git insights**: `fire_cursor_move_hook()` added to all exit paths in `handle_vscode_key()` so Lua plugins (blame.lua) receive `cursor_move` events; annotation rendering gate in `render.rs` updated to allow VSCode mode (Insert + VSCode); hover dwell gates in both GTK and TUI backends updated to include VSCode mode
- **GTK hover popup word wrapping**: Pango word wrapping (`WrapMode::WordChar`) replaces fixed-width overflow; pixel-based height cap; Cairo clip for bounds
- **Stale LSP hover fix**: `lsp_hover_text` cleared on dismiss and mouse-off, preventing cached hover from following clicks
- **GTK hover popup click-to-focus**: `editor_hover_popup_rect` caches popup bounds from draw; clicks on popup set focus (blue border, keyboard control); clicks outside dismiss
- **20Hz SearchPollTick dismiss race fix**: Skip `editor_hover_mouse_move()` when mouse is within popup bounds, preventing continuous dismiss cycle that made popups unclickable
- 4649 total tests (13 new from test count delta)

### Session 202 — Panel Event Enhancements (Mar 21, 2026)
- **`panel_double_click` event**: Fires on double-click of extension panel items (both GTK and TUI backends); separate from `panel_select` to allow plugins to distinguish single-click from double-click
- **`panel_context_menu` event**: Fires on right-click of extension panel items; new GTK button-3 gesture + TUI `MouseButton::Right` handling; `ContextMenuTarget::ExtPanel` variant; plugins can build custom context menus in response
- **`panel_input` event + input field**: `/` activates per-panel input field for search/filtering; typing fires `panel_input` on every keystroke for live filtering; Escape/Return deactivates; `ext_panel_input_text`/`ext_panel_input_active` engine state; Lua API: `vimcode.panel.get_input(name)` / `vimcode.panel.set_input(name, text)`; `panel_input_snapshot` in `PluginCallContext`; `ExtPanelData.input_text/input_active` in render layer
- 10 new tests in `tests/ext_panel.rs`; 4636 total

> Sessions 201 and earlier archived in **SESSION_HISTORY.md**.
