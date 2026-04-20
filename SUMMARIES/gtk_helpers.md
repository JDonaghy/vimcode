# GTK Helper Files

## src/gtk/click.rs — 697 lines
Mouse click/drag/double-click handling for GTK backend. Maps pixel coordinates to logical click targets.
- `ClickTarget` — enum: TabBar, Gutter, BufferPos, SplitButton, CloseTab, DiffToolbar*, StatusBarAction, ActionMenuButton, NavBack, NavForward, None
- `pixel_to_click_target()` — converts (x,y) pixel position to a `ClickTarget`; per-window status bar segment hit-testing
- `gtk_status_segment_hit_test()` — walks status segments by pixel width to find clicked action
- `handle_mouse_click()` — dispatches click to engine actions; StatusBarAction calls `handle_status_action()`
- Click/drag/double-click handler functions dispatched from `App::update()`

## src/gtk/css.rs — 535 lines
GTK CSS theme generation and loading.
- `make_theme_css(theme)` — generates dynamic CSS string from a `Theme` struct
- `STATIC_CSS` — constant CSS for non-theme-dependent styles
- `load_css()` — applies CSS to GTK display

## src/gtk/util.rs — 276 lines
GTK utility functions.
- `open_url(url)` — open URL in default browser
- `matches_gtk_key(binding, key, state)` — key binding matcher (panel_keys)
- `gtk_key_to_pty_bytes(key_name, unicode, ctrl)` — translate GTK key event to PTY input bytes
- `build_gio_menu_from_engine_items()` — context-menu construction from engine items
- `swap_ctx_popover()` — context-menu popover lifecycle helper
- `install_bundled_icon_font()` / `install_icon_and_desktop()` — icon + desktop entry installation
- `menu_row_count()` — count visible rows in a `gio::Menu`
- `suppress_css_node_warning` — GLib log handler suppressing a known GTK4 assertion
- (Phase A.3c-2: native-widget settings-form builders removed; Settings panel now renders into a DrawingArea via `quadraui_gtk::draw_form`.)

## src/gtk/tree.rs — 449 lines
File explorer tree building with GTK TreeStore.
- Tree node building/expansion for file explorer sidebar
- Git status and LSP diagnostic indicator computation per tree row
- `update_tree_indicators()` — refreshes git/diagnostic badges on tree nodes
- `find_tree_iter_for_path(store, path)` — recursive TreeStore iter lookup by path
- `remove_new_entry_rows(store, iter)` — clean up temporary new-entry marker rows on cancel
- Name validation for rename/create operations
