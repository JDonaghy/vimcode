# src/core/engine/windows.rs — 3,011 lines

Window/tab/editor-group management, splits, focus, resize, tab drag-and-drop, and session restore.

## Window Operations
- `split_window(direction)` — create horizontal/vertical split
- `close_window(id)` — close window, handle last-window logic
- `focus_window(id)` — switch active window
- `resize_window(direction, delta)` — resize split
- `cycle_windows()` — Ctrl-W w/W window cycling

## Tab Operations
- `new_tab()` — create new tab
- `close_tab(idx)` — close tab with confirmation if dirty
- `close_tab_confirm(idx)` — close with save prompt
- `next_tab()` / `prev_tab()` — gt/gT tab cycling
- `goto_tab(n)` — go to tab by number
- `move_tab(delta)` — reorder tabs

## Editor Groups
- `split_editor_group(direction)` — create new editor group
- `close_editor_group(id)` — remove editor group
- `focus_group_by_index(n)` — Ctrl+1-9 group focus
- `move_tab_to_group(tab_idx, target_group)` — drag tab between groups
- `resize_group_split(delta)` — resize group divider
- `calculate_group_window_rects(bounds)` — layout calculation; adjusts rects for hidden tab bars via `adjust_group_rects_for_hidden_tabs`

## Session
- `save_session()` — persist open tabs/groups/layout to disk
- `restore_session()` — reload previous session state
- `session_to_state()` / `state_to_session()` — serialize/deserialize
