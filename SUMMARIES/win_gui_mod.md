# src/win_gui/mod.rs — Win-GUI Backend (5,963 lines)

Native Windows backend using windows-rs + Direct2D + DirectWrite. Behind `win-gui` Cargo feature.

## Key Types
- `SidebarPanel` enum — Explorer, Search, Debug, Git, Extensions, Ai, Settings
- `WinSidebar` — sidebar state: visible, active_panel, panel_width, explorer rows, scroll, ext_panel_name
- `ExplorerRow` — depth, name, path, is_dir, is_expanded
- `AppState` — engine, sidebar, render target, D2D/DWrite factories, fonts, cached slots, popup rects
- `TranslatedKey` — key_name, unicode, modifiers from WM_KEYDOWN

## Key Functions
- `run(file_path)` — entry point: creates window, D2D setup, message loop
- `wnd_proc()` — Win32 message dispatch (WM_PAINT, WM_KEYDOWN, WM_CHAR, WM_MOUSE*, WM_TIMER, etc.)
- `on_paint()` — computes window rects, builds ScreenLayout, calls draw_frame + draw_sidebar
- `on_key_down()` — keyboard routing: ext panel, extensions, settings, search, AI, git, editor
- `on_char()` — WM_CHAR routing for plain letter keys (same panel priority as on_key_down)
- `on_mouse_down()` — click routing: dialog, context menu, title bar, nav arrows, activity bar, sidebar (explorer, git panel zones, extensions, search, debug, AI, ext panel), scrollbar, editor
- `on_mouse_dblclick()` — double-click: explorer (open permanent), git panel (open diff), extensions (open readme), editor (word select)
- `on_mouse_wheel()` — scroll routing: picker, hover, sidebar (ext panel, settings, AI, search, explorer), editor
- `on_tick()` — periodic: poll_lsp, poll_dap, poll_terminal, poll_async_shells, ext_panel_focus_pending, poll_panel_hover, sidebar refresh, file watcher, notifications, theme/font hot-reload
- `on_nchittest()` — frameless title bar: 8 resize zones, title bar drag, menu bar HTCLIENT
