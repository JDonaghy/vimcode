# TUI Backend Modules

## src/tui_main/mod.rs — 4,175 lines
TUI application shell using ratatui + crossterm. Contains setup, event loop, key translation, clipboard, and cell rendering helpers.
- `run(file_path, debug_log)` — entry point; sets up terminal, runs event loop, restores terminal on exit
- `event_loop(terminal, engine)` — main loop: poll events, dispatch keys, call draw_frame, poll async (LSP/DAP/terminal/search)
- `sync_sidebar_focus(sidebar, engine)` — sync engine `explorer_has_focus`/`search_has_focus` from TUI sidebar state
- Key translation from crossterm `KeyEvent` to engine `(key_name, ctrl, unicode)` format
- `debug_log!` macro + `--debug` flag for TUI debugging
- Clipboard via `copypasta_ext::x11_bin::ClipboardContext`
- Keyboard enhancement flags for Kitty/WezTerm (disambiguate Ctrl+Shift combos)

## src/tui_main/render_impl.rs — 3,777 lines
All TUI rendering. Converts `ScreenLayout` into ratatui `Frame` draws.
- `draw_frame(frame, engine, theme)` — top-level render function
- `build_screen_for_tui(engine, cols, rows)` — compute layout geometry
- Tab bar rendering per editor group
- Editor window rendering (syntax spans → ratatui Spans)
- Popup rendering: completion, hover, picker, dialog, context menu, diff peek, signature help
- Status line + command line + wildmenu rendering
- Menu bar + dropdown rendering
- Debug toolbar rendering

## src/tui_main/panels.rs — 3,931 lines
Sidebar panel rendering for all TUI panels.
- Activity bar (icon column, panel switching)
- Explorer file tree with git/diagnostic indicators
- Source control panel (staged/unstaged files, commit input, worktrees)
- Debug sidebar (call stack, variables, watch, breakpoints)
- Extensions marketplace panel (installed/available, search, install/remove)
- AI chat panel (messages, input)
- Search panel (query input, results list, replace)
- Terminal panel (cell grid, tab bar, toolbar, find bar)
- Extension dynamic panels (Lua-registered panels)
- Panel hover popup rendering

## src/tui_main/mouse.rs — 2,385 lines
All TUI mouse interaction handling.
- `handle_mouse(event, engine, layout)` — top-level mouse dispatcher
- Activity bar clicks (panel switching)
- Explorer tree clicks (file open, expand/collapse, context menu)
- Editor clicks (cursor placement, selection, drag)
- Tab bar clicks (tab switch, close button, drag between groups)
- Sidebar resize drag (Alt+Left/Right or mouse drag on border)
- Scrollbar drag (vertical + horizontal, editor + panel)
- Terminal panel clicks
- Source control / extensions / debug sidebar clicks
- Group divider drag to resize editor groups
- Context menu click handling
