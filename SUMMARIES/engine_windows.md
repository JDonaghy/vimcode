# src/core/engine/windows.rs — 3,396 lines

Window/tab/editor-group management, splits, focus, resize, tab drag-and-drop, tab navigation history, and session restore.

## Window Operations
- `split_window(direction)` — create horizontal/vertical split
- `close_window(id)` — close window, handle last-window logic; detects empty tab after diff pair cleanup
- `focus_window(id)` — switch active window
- `resize_window(direction, delta)` — resize split
- `cycle_windows()` — Ctrl-W w/W window cycling

## Tab Operations
- `new_tab()` — create new tab
- `close_tab(idx)` — close tab with confirmation if dirty; cleans up nav history entries
- `close_tab_confirm(idx)` — close with save prompt
- `close_all_tabs()` — close all tabs in active group
- `next_tab()` / `prev_tab()` — gt/gT tab cycling
- `goto_tab(n)` — go to tab by number
- `move_tab(delta)` — reorder tabs
- `ensure_active_tab_visible()` — adjust `tab_scroll_offset` so active tab is on-screen

## Tab Navigation History
- `tab_nav_push()` — record current tab in history; dedup consecutive, truncate forward, 100-entry bound
- `tab_nav_back()` / `tab_nav_forward()` — navigate through tab access history
- `tab_nav_switch_to(group_id, tab_id)` — resolve TabId to index and switch
- `tab_nav_can_go_back()` / `tab_nav_can_go_forward()` — bool accessors for UI state

## Editor Groups
- `split_editor_group(direction)` — create new editor group
- `close_editor_group(id)` — remove editor group
- `focus_group_by_index(n)` — Ctrl+1-9 group focus
- `move_tab_to_group(tab_idx, target_group)` — drag tab between groups
- `resize_group_split(delta)` — resize group divider
- `calculate_group_window_rects(bounds)` — layout calculation; adjusts rects for hidden tab bars via `adjust_group_rects_for_hidden_tabs`

## Context Menus
- `open_tab_context_menu(group_id, tab_idx, x, y)` — right-click tab menu
- `open_editor_action_menu(group_id, x, y)` — `…` button dropdown (Close All/Others/Saved/Right/Left, Toggle Wrap, Change Language, Reveal)
- `context_menu_confirm()` — dispatch selected context menu action
- `handle_context_menu_key(key)` — keyboard navigation for context menus

## Session
- `save_session()` — persist open tabs/groups/layout to disk
- `restore_session()` — reload previous session state
- `session_to_state()` / `state_to_session()` — serialize/deserialize
