# VimCode Project State

**Last updated:** Apr 12, 2026 (Session 271 — Win-GUI extension panels, breadcrumb/tooltip UNC fix, Nerd Font auto-detect, TUI icon fallbacks) | **Tests:** 5478

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

**Session 271 — Win-GUI extension panels + Nerd Font auto-detect + breadcrumb/tooltip fixes:**

**Win-GUI extension panel support (new feature):**
1. **`draw_ext_panel()`** — Full ext panel renderer: header, search input, flat row list (sections with expand/collapse, tree items with chevrons/icons/indent), selection highlight, badges with hex/named colors, action buttons, scrollbar, help popup overlay.
2. **Activity bar ext panel icons** — Dynamic icons rendered after fixed panels; Nerd Font glyphs mapped to Segoe MDL2 equivalents (git→History icon, terminal→CommandPrompt, etc.).
3. **Activity bar click handler** — Restructured to handle 3 zones: fixed panels (rows 0-5), ext panel icons (rows 6+), settings gear (bottom). Ext panel clicks set `ext_panel_active`, `ext_panel_has_focus`, fire `panel_focus` event.
4. **Sidebar content clicks** — Maps click row to flat index, sets selection, fires "Return" event for ext panels.
5. **Keyboard routing** — Full handler in `on_key_down()` and `on_char()`: input mode (Escape/Return/Backspace/chars) and navigation mode (j/k/g/G/Tab/Return/Escape/`/`/`?`).
6. **Scroll wheel** — Ext panel scroll when `ext_panel_name` is set.
7. **`ext_panel_focus_pending` + `poll_async_shells` + `poll_panel_hover`** — Added to `on_tick()` to enable plugin-driven panel reveals.

**Rendering fixes (3 bugs):**
8. **Breadcrumb `?C:` prefix** — `build_breadcrumbs_for_group()` now strips UNC prefix (`\\?\`) from both file path and cwd.
9. **Tab tooltip UNC prefix** — TUI `tab_tooltip_at_col()` now strips UNC prefix. Also fixed `~/` separator to use `MAIN_SEPARATOR` (backslash on Windows).
10. **Empty breadcrumb covering tab bar** — `draw_breadcrumb_bar()` now returns early when segments are empty, preventing the breadcrumb background from painting over the tab bar for scratch/diff buffers.

**Diff toolbar overlap (1 fix):**
11. **Tabs stop before diff toolbar** — `draw_tabs()` now respects `max_width` and stops drawing tabs before they extend under the diff toolbar. Both single-group and multi-group paths reserve space.

**Nerd Font auto-detect (cross-platform):**
12. **`detect_nerd_font_windows()`** — Scans user + system font directories for files with "Nerd" in the name. Shared via `icons.rs`, used by both TUI and Win-GUI startup.
13. **Default `use_nerd_fonts = false` on Windows** — `default_use_nerd_fonts()` returns `!cfg!(target_os = "windows")`.
14. **TUI activity bar icons** — Replaced 9 hardcoded Nerd Font codepoints with `icons::ICON.c()` calls that respect `use_nerd_fonts` flag.
15. **Startup message** — "No Nerd Font detected — using fallback icons" shown after async init completes (survives ext_refresh callback).

**Session 270 — Win-GUI bug blitz (9 fixes — panel routing, resize, subprocess, nav):**

**Subprocess hiding (2 bugs):**
1. **`hidden_command()` helper** — Added `pub fn hidden_command(program)` in `git.rs` that creates a `Command` with `CREATE_NO_WINDOW` on Windows. Replaced all 6 `Command::new("curl")` calls in `registry.rs` (3) and `ai.rs` (3) to prevent console window flashes during extension install and AI API calls.

**Window resize (1 bug):**
2. **Full NCHITTEST border handling** — `on_nchittest()` now handles all 8 resize zones (HTLEFT, HTRIGHT, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT + existing top 3). Window can now be resized from any edge or corner.

**Keyboard routing (3 bugs):**
3. **Search panel** — Full keyboard handler in `on_key_down()` and `on_char()`: input mode (typing into search/replace, Tab to toggle, Enter to execute, Backspace, Ctrl+V paste, Down to results), results mode (j/k navigation, Enter to open, Escape to exit), Alt+C/W/R/H option toggles. Added `search_input_mode`, `replace_input_focused`, `search_scroll_top` to `WinSidebar`.
4. **AI panel** — Full keyboard routing through `engine.handle_ai_panel_key()`: navigation mode (j/k/G/g/i/q), input mode (text entry, cursor, Enter submit, Ctrl+V paste).
5. **Git panel** — Full keyboard routing through `engine.handle_sc_key()`: navigation mode (j/k/s/S/d/D/c/p/P/f/b/B/?/Tab/Enter/r/q), commit input mode, branch picker, help dialog.

**Click handlers (1 bug):**
6. **Nav arrow clicks** — Hit-test ◀/▶ arrows in the title bar → `tab_nav_back()`/`tab_nav_forward()`. Also added command center search box click → `open_command_center()`. Geometry matches `draw_menu_bar()` in draw.rs.

**Scroll routing (1 bug):**
7. **Panel-specific scroll** — Sidebar mouse wheel now dispatches by active panel: Settings → `settings_scroll_top`, AI → `ai_scroll_top`, Search → `search_scroll_top`, Explorer → `sidebar.scroll_top`.

**Activity bar (1 bug):**
8. **Search/Debug focus on click** — Activity bar click handler now sets `search_has_focus`/`search_input_mode` for Search panel and `dap_sidebar_has_focus` for Debug panel (were missing from the `match clicked_panel` block).

> Sessions 270 and earlier in **SESSION_HISTORY.md**.
