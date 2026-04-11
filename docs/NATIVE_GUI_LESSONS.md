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
