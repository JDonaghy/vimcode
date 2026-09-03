# src/gtk/mod.rs — 7,999 lines (~7,684 production)

The GTK backend's whole application shell: the `App` struct and its
`impl quadraui::ShellApp for App`. **Not** Relm4 — that was stripped by #540 —
and there is no `Msg` enum any more: #732 retired the 124-variant message bus and
its 684-line `dispatch`. There is no `SidebarPanel` enum either (#408/#409
replaced it with `active_panel_id: String` plus lookup tables).

`App` owns almost no decisions. #751–#766 converged mouse routing, keyboard
dispatch and frame composition into `render.rs`; this file makes **424 `render::`
calls** and is mostly the wiring that turns GTK events into engine calls and
`ScreenLayout` into Cairo. When looking for *where something is decided*, look in
`render.rs` first.

## Key Types
- `App` — the application struct: `Engine`, `Rc<RefCell<GtkBackend>>`, drawing
  areas, gesture controllers, timers, UI state. Four genuinely platform-typed
  fields remain (`window`, `css_provider`, `settings_monitor`, `backend`) — see
  `PLAN.md`'s "#47 re-audit findings" for what that costs a macOS port.
- `DeferredAction` / `DeferredQueue` — actions queued during dispatch and drained
  by `tick()`, so a handler can request work without re-entrant borrows.
- `GtkAccelHost` — `impl render::PanelAcceleratorHost`, the shared
  panel-accelerator rung (#761).
- `PendingFileDialog` — native file/folder dialog requests, drained by `tick()`.

## The `ShellApp` impl — the four entry points
- `setup()` — window, CSS, drawing areas, gesture controllers, accelerators
- `render_content()` — composes the frame: `compose_editor_band_rungs`,
  `compose_bottom_band_rungs`, `paint_tab_bars_rung`, `paint_editor_windows_rung`,
  `paint_sidebar_panel_rung`, `paint_editor_popups_rung`, `paint_title_bar_band`
- `handle()` — one `UiEvent` in; routes to the `handle_*` methods below
- `tick()` — timer polling (LSP, DAP, terminal, search, extensions) + queue drains

## Event handlers (`impl App`)
`handle_key_press`, `dispatch_focus_owner_residual`, `run_post_key_epilogue`,
`handle_mouse_click_msg`, `handle_mouse_double_click_msg`, `handle_mouse_drag_msg`,
`handle_mouse_up_msg`, `handle_mouse_scroll_msg`, `handle_ctrl_mouse_click`,
`handle_poll_tick`, `handle_resize`, `handle_tab_right_click`,
`handle_editor_right_click`, `handle_menu_action`, `handle_activity_bar_key`,
`handle_explorer_da_key`.

Routing helpers that delegate into shared code: `route_modal_overlay`,
`route_and_apply_chrome_click`, `route_and_apply_editor_hover_popup`,
`try_route_sidebar_mouse_event`, `route_debug_sidebar_event`,
`route_sc_sidebar_event`, `route_ai_sidebar_event`, `apply_picker_route`,
`apply_context_menu_route`, `dispatch_context_menu_click`,
`dispatch_context_menu_key`.

## Sidebar / window / terminal
`sync_sidebar_from_engine`, `sync_sidebar_widgets`, `toggle_sidebar_panel`,
`switch_panel`, `explorer_action`, `explorer_ui_event`, `refresh_file_tree`,
`toggle_focus_explorer`, `toggle_focus_search`, `window_minimize`,
`window_toggle_maximize`, `window_close`, `show_quit_confirm`,
`show_close_tab_confirm`, `open_file_dialog` / `open_folder_dialog` /
`open_workspace_dialog` / `open_recent_dialog`, `toggle_terminal`,
`toggle_terminal_maximize`, `new_terminal_tab`, `run_command_in_terminal`.

## Free functions
- `run()` — the entry point; builds a shell config and calls
  `quadraui::gtk::shell_runner::run_with_shell`
- `build_shell_config()` — app id, icon, chrome metrics
- `map_gtk_key_name`, `gtk_key_name_to_quadraui`, `map_gtk_key_with_unicode` —
  GDK→engine key naming. Despite the names these take and return plain `&str` /
  `quadraui::UiEvent` and have **zero `gtk4` dependency**.
- `setup_gtk_clipboard()` — wires the engine's clipboard closures once at startup
- `calculate_gutter_width`, `compute_editor_window_rects`, `h_scrollbar_geometry`,
  `h_scrollbar_hit_test`, `tab_hits_to_pixel_hits` — geometry helpers
