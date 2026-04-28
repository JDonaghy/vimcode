# TUI Backend Modules

## src/tui_main/backend.rs — 685 lines
**Phase B.4: `quadraui::Backend` trait impl for TUI.** Load-bearing — every native event flows through this.
- `TuiBackend` struct — owns the cached viewport, `ModalStack`, `DragState`, accelerator registry, `TuiPlatformServices`, frame-scope pointer for `&mut Frame<'_>`, current theme
- `enter_frame_scope(frame, |b| ...)` — stashes type-erased `*mut Frame` for the closure duration so trait `draw_*` methods reach it
- `wait_events(timeout)` / `poll_events()` — translate crossterm events through `events::crossterm_to_uievents` then run `apply_accelerators` to rewrite registered key bindings as `UiEvent::Accelerator`
- 4 trait `draw_*` methods (palette/list/tree/form) implemented; 5 stubbed pending quadraui trait extension (status_bar/tab_bar/activity_bar/terminal/text_display)
- `apply_accelerators` / `match_keypress` / `parse_binding` / `named_key_to_binding_name` — registered binding match against incoming key events
- 11 unit tests (cross-backend MockBackend `paint_overlays`, modal stack, accelerator round-trip, named keys, modifier matching, widget-scope skip, etc.)

## src/tui_main/services.rs — 76 lines
**Phase B.4: `quadraui::PlatformServices` impl for TUI** — stub for now. `TuiPlatformServices` + `TuiClipboard` (no-op). Engine clipboard plumbing in `mod.rs::setup_tui_clipboard` still owns the real read/write closures; trait surface is forward-compat.

## src/tui_main/events.rs — 669 lines
**Phase B.4: crossterm↔`UiEvent` translation.** Every native event reaches the engine through this layer.
- `crossterm_to_uievents(Event)` — forward translation; `Vec<UiEvent>` for future composite events
- `crossterm_key_to_uievent`, `crossterm_mouse_to_uievent` — per-event-kind helpers
- `uievent_to_crossterm()` — inverse synth used by Stage 5b to feed legacy handlers without re-decoding
- `synth_keyevent`, `synth_mouseevent` — inverse helpers
- 19 unit tests (15 forward + 4 round-trip)

## src/tui_main/mod.rs — 4,237 lines
TUI application shell using ratatui + crossterm. Contains setup, event loop, key translation, clipboard, and cell rendering helpers.
- `run(file_path, debug_log)` — entry point; sets up terminal, runs event loop, restores terminal on exit
- `event_loop(terminal, engine)` — main loop: poll events, dispatch keys, call draw_frame, poll async (LSP/DAP/terminal/search)
- `sync_sidebar_focus(sidebar, engine)` — sync engine `explorer_has_focus`/`search_has_focus` from TUI sidebar state
- Key translation from crossterm `KeyEvent` to engine `(key_name, ctrl, unicode)` format
- `debug_log!` macro + `--debug` flag for TUI debugging
- Clipboard via `copypasta_ext::x11_bin::ClipboardContext`
- Keyboard enhancement flags for Kitty/WezTerm (disambiguate Ctrl+Shift combos)

## src/tui_main/render_impl.rs — 3,944 lines
All TUI rendering. Converts `ScreenLayout` into ratatui `Frame` draws.
- `draw_frame(frame, engine, theme)` — top-level render function
- `build_screen_for_tui(engine, cols, rows)` — compute layout geometry
- Tab bar rendering per editor group
- Editor window rendering (syntax spans → ratatui Spans)
- Popup rendering: completion, hover, picker, dialog, context menu, diff peek, signature help
- Per-window status line rendering (`render_window_status_line`)
- Global status line + command line + wildmenu rendering
- Menu bar + dropdown rendering (centered nav arrows + Command Center search box)
- Debug toolbar rendering

## src/tui_main/panels.rs — 4,034 lines
Sidebar panel rendering for all TUI panels.
- Activity bar (icon column, panel switching)
- Explorer file tree with git/diagnostic indicators + inline new-entry rows (`render_new_entry_row()`)
- Source control panel (staged/unstaged files, commit input, worktrees)
- Debug sidebar (call stack, variables, watch, breakpoints)
- Extensions marketplace panel (installed/available, search, install/remove)
- AI chat panel (messages, input)
- Search panel (query input, results list, replace)
- Terminal panel (cell grid, tab bar, toolbar, find bar)
- Extension dynamic panels (Lua-registered panels)
- Panel hover popup rendering

## src/tui_main/mouse.rs — 2,892 lines
All TUI mouse interaction handling.
- `handle_mouse(event, engine, layout)` — top-level mouse dispatcher
- Activity bar clicks (panel switching)
- Explorer tree clicks (file open, expand/collapse, context menu)
- Editor clicks (cursor placement, selection, drag)
- Tab bar clicks (tab switch, close button, action menu `…` button, drag between groups)
- Sidebar resize drag (Alt+Left/Right or mouse drag on border)
- Scrollbar drag (vertical + horizontal, editor + panel, picker popup)
- Status bar clicks (branch name click opens branch picker)
- Terminal panel clicks
- Source control / extensions / debug sidebar clicks
- Group divider drag to resize editor groups
- Context menu click handling
