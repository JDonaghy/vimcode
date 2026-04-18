# Native GUI Backend Lessons (from Win-GUI)

Hard-won lessons from building the Win-GUI backend (Sessions 264-268). Apply when building the macOS native GUI (Core Graphics + Core Text) or any future backend.

## 1. Reserved space = ALL chrome, not just one element

**The bug:** `draw_group_tab_bar` positioned tabs at `bounds.y - tab_bar_height`, but `bounds.y` was offset by `tab_bar_height + breadcrumb_height`. Tabs drew at the breadcrumb position, hidden behind editor content.

**The rule:** When `calculate_group_rects` reserves space above the content area, that space includes EVERY element (tab bar + breadcrumb row). Any code positioning elements relative to `bounds.y` must subtract the **full** reserved height, not just one component. The reserved height is passed as `tab_bar_height` to `calculate_group_rects` and may include breadcrumbs.

**How to apply:** In the macOS backend, when drawing group tab bars, compute the breadcrumb offset: `let bc_offset = if has_breadcrumb { line_height } else { 0.0 }; let tab_y = bounds.y - tab_bar_h - bc_offset;`. Check every place that uses `GroupTabBar.bounds.y` — there are at least 3 in Win-GUI: tab bar drawing, tab slot caching (for clicks), and tab drag overlay.

## 2. Test multi-group scenarios explicitly

**The bug class:** Many Win-GUI features worked perfectly in single-group mode but broke with 2+ groups because the multi-group code path was never exercised visually or in tests.

**The rule:** After implementing any layout feature, test with: 1 group, 2 groups vertical, 2 groups horizontal, 3+ groups. Also test with breadcrumbs on AND off — many bugs only manifest in one combination.

**How to apply:** Write `test_multi_group_window_rects_cover_all_groups`-style tests that create 2+ groups and verify all geometry is valid (non-zero bounds, no overlaps). These are pure engine tests that run without a GUI.

## 3. `build_screen_layout` is the single source of truth — backends must not recompute

**The pattern:** `build_screen_layout()` in `render.rs` produces `ScreenLayout` with all the data backends need. When backends recompute positions (e.g., tab bar Y from hardcoded constants), they drift from the layout's geometry.

**How to apply:** The macOS backend should consume `ScreenLayout` fields directly for positions, not recompute from constants. If you need a position, derive it from the layout struct, not from `line_height * SOME_MULT`.

## 4. The "single-group path vs multi-group path" split is a bug factory

**The pattern:** Every feature has two code paths: `if editor_group_split.is_some()` (multi-group) vs the `else` (single-group). The single-group path is exercised constantly and well-tested. The multi-group path is used rarely and accumulates bugs.

**How to apply:** Consider unifying the two paths where possible. If `editor_group_split` always existed (even for 1 group), there'd be no split. Alternatively, ensure every feature's multi-group path has a test.

## 5. Click hit-testing must match drawing exactly

**The bug:** Tab slots were cached with the wrong Y position (same breadcrumb offset bug), so clicks on the visible tab bar didn't register — they hit an invisible row above.

**The rule:** Drawing code and click-handling code must use identical geometry calculations. Extract shared constants or compute positions once and pass to both.

**How to apply:** In the macOS backend, consider a `GroupTabBarLayout` struct computed once per frame that both drawing and click handling consume, rather than recomputing independently.

## 6. Win-GUI bugs that the macOS backend should avoid from day one

| Bug | Root cause | Prevention |
|-----|-----------|------------|
| Tab close skips dirty check | Backend called `close_tab()` directly | Always check `engine.dirty()` first |
| Picker not mouse-interactive | No `picker_open` check in click handler | Add picker intercept as first check |
| Dialog buttons not clickable | No dialog click handling | Add dialog as highest-z-order click target |
| QuitWithUnsaved ignored | `handle_action` returned false | Handle via engine dialog system |
| Scroll doesn't skip folds | Raw `scroll_top` arithmetic | Use `scroll_down_visible()`/`scroll_up_visible()` |
| Picker scroll not intercepted | Scroll reached editor behind picker | Check `picker_open` before editor scroll |
| VSCode selection not cleared | Missing `vscode_clear_selection()` | Call before `mouse_click` in VSCode mode |
| Cursor not in viewport after scroll | No post-scroll cursor adjustment | Clamp cursor with scrolloff after scroll |
| Terminal tab clicks missing | Only toolbar buttons, not tab labels | Match tab label geometry from draw code |
| Tabs hidden with 2+ groups | Breadcrumb offset missing from tab Y | Subtract full reserved height (tab + breadcrumb) |
| Terminal steals keyboard focus | `terminal_has_focus` not cleared on editor click | Clear ALL focus flags when entering any click zone |
| Tab accent on all groups | `is_active_group` parameter ignored in draw code | Pass group-active state through to all drawing functions |
| Sidebar focus persists after editor click | `clear_sidebar_focus()` not called on editor click | Always call `clear_sidebar_focus()` when clicking editor or terminal |
| Dialog text/buttons overflow | Hardcoded 400px width, buttons wider than dialog | Auto-size dialog width from content (buttons + body + title) |

## 7. Focus state management (Category A bugs)

**The rule:** Every click zone must clear ALL competing focus flags. There are ~7 focus-related fields:
- `terminal_has_focus`, `sidebar.has_focus`, `explorer_has_focus`, `search_has_focus`, `ai_has_focus`, `dap_sidebar_has_focus`, `settings_has_focus`

**When clicking the editor:** Clear `sidebar.has_focus`, `terminal_has_focus`, AND call `engine.clear_sidebar_focus()` (clears all engine-side focus flags).

**When clicking the terminal:** Clear `sidebar.has_focus` and call `engine.clear_sidebar_focus()`, then set `terminal_has_focus = true`.

**When clicking a sidebar panel:** The activity bar handler already calls `clear_sidebar_focus()` before setting the specific panel's flag. Good pattern to follow.

**Detection strategy for new backends:** Grep for every `_has_focus = true` setter. For each one, verify there's a corresponding clearing in ALL competing click paths (editor, terminal, sidebar, popups).

## 8. Popup/dialog clipping and sizing (Category D bugs)

**The rule:** All popup-style drawing (dialogs, pickers, context menus, tooltips) must:
1. **Auto-size** width based on content — never hardcode a width that might be too small
2. **Clamp** position to screen edges (right AND left AND bottom)
3. **Clip** text rendering to the popup bounds (use `PushAxisAlignedClip` on D2D or equivalent)
4. **Match** drawing geometry and click-handler geometry exactly — extract to a shared function if possible

**Common mistake:** Using `draw_text` with unlimited width (10000px in D2D). Text bleeds past popup borders. Add clipping rects around popup content areas.

## 9. Backend checklist for new features

When implementing a new backend, verify each of these independently:
- [ ] All `EngineAction` variants handled (esp. `QuitWithUnsaved`, `OpenTerminal`)
- [ ] Picker intercepts all mouse events when open (click + scroll)
- [ ] Dialog intercepts all mouse events when open (highest z-order)
- [ ] Context menu intercepts clicks when open
- [ ] Scroll uses fold-aware methods
- [ ] Cursor stays in viewport after scroll
- [ ] `vscode_clear_selection()` called on click in VSCode mode
- [ ] Tab close checks dirty before closing
- [ ] Multi-group rendering works (tab bars, breadcrumbs, drag overlays)
- [ ] Breadcrumb offset accounted for in ALL multi-group positioning code
- [ ] ALL focus flags cleared on competing click paths (editor/terminal/sidebar)
- [ ] `clear_sidebar_focus()` called on editor click AND terminal click
- [ ] Tab accent only drawn for active group (pass `is_active_group` through)
- [ ] Dialog/popup width auto-sized from content, not hardcoded
- [ ] All popups clamped to screen bounds
- [ ] All popup text clipped to popup bounds
- [ ] Context menu items highlight on mouse hover (not just on click)
- [ ] Tab slot bounds clipped to group bounds (no overflow into adjacent groups)
- [ ] Tab slot geometry matches draw code font (proportional vs monospace)
- [ ] Menu bar click/hover geometry matches draw code font
- [ ] Mouse cursor changes: I-beam over editor, arrow over chrome, resize near dividers
- [ ] Clipboard sync: register → system clipboard after yank, system → register before paste
- [ ] `clipboard_paste()` works on the target platform (Windows needs PowerShell, not xclip)
- [ ] Ctrl+V in insert mode pastes clipboard (not literal character insert)
- [ ] Sidebar keyboard routing: generic handler only for Explorer, other panels through `handle_key()`
- [ ] Extension panel keys (`i`/`d`/`u`/`r`/`/`) mapped as named keys, not `("char", Some('i'))`
- [ ] Path display strips platform-specific prefixes (`\\?\` on Windows)
- [ ] Terminal click sets focus AND starts text selection (mouse drag → `TermSelection`)
- [ ] Terminal paste: Ctrl+V reads clipboard/registers, writes to PTY with bracketed paste
- [ ] Terminal copy: Ctrl+Y/Ctrl+Shift+C copies selection, auto-copy on mouse release

## 10. Interaction parity is harder than rendering parity (Session 269)

**The pattern:** The Win-GUI was built render-first: every `ScreenLayout` field had a corresponding draw call. But interactions (clicks, hovers, drags, keyboard routing) had no parity verification. This led to ~20 interaction bugs discovered through manual testing.

**Bug classes discovered:**

| Class | Description | Example | Prevention |
|-------|------------|---------|------------|
| **Missing hover** | Element renders but has no mouse-move tracking | Context menu items, tab tooltips | For every interactive element in TUI `mouse.rs`, add equivalent `on_mouse_move` handler |
| **Key swallowing** | Generic handler intercepts keys meant for specific panels | Sidebar j/k handled Explorer-style for Git panel | Guard generic handlers with `active_panel == Explorer` |
| **Font mismatch** | Click detection uses monospace width but draw uses proportional font | Tab close button, menu bar labels | Use `measure_ui_text_width()` everywhere the draw uses `measure_ui_text()` |
| **Bounds overflow** | Cached hit zones extend past visual boundaries | Tab slots from group 1 overlap group 2 | Clip cached slots to parent bounds |
| **Platform clipboard** | Engine static methods use Linux-only tools | `clipboard_paste()` uses xclip on Windows | Add `#[cfg(target_os)]` branches for all clipboard operations |
| **Path display** | Platform path prefixes leak into UI | `\\?\C:\...` in tooltips and context menu actions | Use `strip_unc_prefix()` (or macOS equivalent) at all display points |

**Systematic audit approach:**
1. Grep every `on_mouse_move` handler in TUI `mouse.rs` — verify Win-GUI (or new backend) has the equivalent
2. Grep every `_has_focus = true` setter — verify competing click paths clear it
3. Grep every `chars().count() * cw` in click handling — verify the draw code uses the same width calculation
4. Grep every `clipboard_paste()` call — verify it works on the target platform
5. Grep every `.display()` or `.to_string_lossy()` on a `PathBuf` — verify no platform prefix leaks

**For macOS port:** Run this audit BEFORE the first manual test, not after. Fix the classes proactively. The Win-GUI experience shows that fixing bugs one-by-one is 3-4x slower than fixing them by class.

## 11. Extension panel click geometry must match fractional draw layout

**The bug:** The extension panel draw code uses fractional Y positions (`1.5 * lh` for first header, `0.3 * lh` gap between sections), but the click handler used integer row indices. Clicks hit the wrong items or missed entirely.

**The rule:** When a draw function uses non-integer spacing (fractional line heights, padding, gaps), the click handler must replicate the exact same Y arithmetic. Don't approximate with integer rows.

**How to apply:** Extract the Y layout computation into a shared function or replicate the draw's arithmetic exactly in the click handler. If the draw adds `lh * 0.3` gap, the click handler must too.

## 12. Terminal integration requires three interaction layers

**The rule:** A terminal panel needs all three layers to be usable:
1. **Focus management** — clicking terminal sets focus, clicking elsewhere clears it. Must handle both single-pane and split-pane cases.
2. **Keyboard routing** — when focused: Escape returns to editor, Ctrl+V pastes, Ctrl+Y copies, all other keys forward to PTY. Must intercept BEFORE generic key handlers.
3. **Mouse selection** — mouse-down starts `TermSelection`, drag updates it, release auto-copies to clipboard. Must track drag state separately from editor text drag.

**Common mistakes from Win-GUI:**
- Terminal content click only handled split-pane case (single-pane fell through to editor)
- No paste/copy keyboard shortcuts in terminal focus
- No mouse selection or auto-copy on release
