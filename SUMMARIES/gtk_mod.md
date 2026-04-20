# src/gtk/mod.rs — 10,070 lines

GTK4/Relm4 application shell. Defines the `App` struct, `Msg` enum, and `SimpleComponent` impl (init/view/update). Contains the main event loop, window setup, input handling, and all GTK widget wiring.

## Key Types
- `App` — main application struct holding `Engine`, drawing areas, gesture controllers, timers, and UI state
- `Msg` — message enum with hundreds of variants for all GTK events (key press, mouse, LSP, DAP, terminal, timer ticks, etc.)
- `SidebarPanel` — enum for sidebar panel switching (Explorer, Search, Git, Debug, Extensions, AI, ExtPanel)
- `TuiPanel` — sidebar panel enum for activity bar routing

## Key Methods (SimpleComponent)
- `update()` — dispatcher (~430 lines) routing `Msg` variants to 19 helper methods
- `init()` — GTK window setup, drawing areas, gesture controllers, CSS loading
- `view()` — Relm4 view macro defining the widget tree

## Helper Methods (impl App, called from update)
- `handle_key_press()` — keyboard input, mode switching, engine key dispatch
- `handle_poll_tick()` — timer-driven polling (LSP, DAP, terminal, search, extensions)
- `handle_mouse_click_msg()` — left click dispatching via `pixel_to_click_target()` (includes status bar branch click handler)
- `handle_mouse_drag_msg()` — mouse drag (tab, scrollbar, sidebar resize, text selection)
- `handle_mouse_up_msg()` — mouse release, tab drop, sidebar divider drop
- `show_action_menu_popover()` — editor action menu (`…` button) popover with gio::Menu
- `handle_tab_right_click()` — tab context menu (close, split, copy path)
- `handle_editor_right_click()` — editor context menu (cut, copy, paste, LSP actions)
- `handle_terminal_msg()` — terminal toggle, tabs, mouse, find, clipboard
- `handle_menu_msg()` — menu bar open/close/activate/highlight
- `handle_debug_sidebar_msg()` — DAP sidebar click/key/scroll
- `handle_sc_sidebar_msg()` — source control sidebar click/motion/key
- `handle_ext_sidebar_msg()` — extensions sidebar key/click
- `handle_ext_panel_msg()` — extension detail panel key/click/scroll/hover
- `handle_settings_msg()` — settings sidebar key/click/scroll (Phase A.3c-2)
- `handle_ai_sidebar_msg()` — AI sidebar key/click
- `handle_sidebar_panel_msg()` — sidebar toggle/panel switch
- `handle_explorer_msg()` — file tree open/preview/create/delete/refresh/focus/activate-selected; also routes `ExplorerKey/Click/RightClick/Scroll` + `PromptRenameFile/NewFile/NewFolder` (Phase A.2b-2: native TreeView → DrawingArea)
- `handle_find_replace_msg()` — find/replace dialog and window resize
- `handle_file_ops_msg()` — rename, move, copy path, diff, clipboard paste, window close
- `handle_dialog_msg()` — window minimize/maximize/close, file/folder dialogs, quit confirm
- `terminal_cols()` — utility to compute terminal column count from drawing area width

## Free Functions
- `map_gtk_key_name(key_name) -> &str` — canonical GDK→engine key name mapping
- `map_gtk_key_with_unicode(key_name, unicode) -> (String, Option<char>)` — key mapping with unicode passthrough

## Utility Methods (impl App)
- `focus_editor_if_needed(still_focused)` — grab editor focus when sidebar loses focus
- `dispatch_engine_action(action, sender, is_macro)` — unified EngineAction handler (quit, open file, terminal, etc.)
- `save_session_and_exit()` — save session state and exit process
- `reveal_path_in_explorer(target)` — expand ancestors + scroll-to + redraw explorer DA (A.2b-2 replacement for `highlight_file_in_tree`)
- `refresh_explorer()` — rebuild the flat-row list from disk and queue redraw
- `explorer_viewport_rows()` / `explorer_row_at(y)` / `explorer_move_selection(delta)` / `queue_explorer_draw()` — explorer DA geometry + selection helpers
- `handle_explorer_da_key/click/right_click` — DA input handlers
- `show_explorer_context_menu(x, y, target, is_dir, sender)` — builds the 14-action right-click PopoverMenu at DA-local coords
- `prompt_for_name(title, prompt, initial, on_confirm)` — modal rename/new-file/new-folder dialog (A.2b-2 deferred inline-editing path)

## Entry Point
- `run(file_path)` — creates Relm4 app and launches GTK main loop
