# src/gtk/mod.rs — 9,258 lines

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
- `handle_mouse_click_msg()` — left click dispatching via `pixel_to_click_target()`
- `handle_mouse_drag_msg()` — mouse drag (tab, scrollbar, sidebar resize, text selection)
- `handle_mouse_up_msg()` — mouse release, tab drop, sidebar divider drop
- `handle_tab_right_click()` — tab context menu (close, split, copy path)
- `handle_editor_right_click()` — editor context menu (cut, copy, paste, LSP actions)
- `handle_terminal_msg()` — terminal toggle, tabs, mouse, find, clipboard
- `handle_menu_msg()` — menu bar open/close/activate/highlight
- `handle_debug_sidebar_msg()` — DAP sidebar click/key/scroll
- `handle_sc_sidebar_msg()` — source control sidebar click/motion/key
- `handle_ext_sidebar_msg()` — extensions sidebar key/click
- `handle_ext_panel_msg()` — extension detail panel key/click/scroll/hover
- `handle_ai_sidebar_msg()` — AI sidebar key/click
- `handle_sidebar_panel_msg()` — sidebar toggle/panel switch
- `handle_explorer_msg()` — file tree open/preview/create/delete/refresh/focus
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

## Entry Point
- `run(file_path)` — creates Relm4 app and launches GTK main loop
