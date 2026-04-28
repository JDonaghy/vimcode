# GTK Helper Files

## src/gtk/backend.rs — 647 lines
**Phase B.5: `quadraui::Backend` trait impl for GTK.** Plumbing-only today; runtime migration tracked at #249.
- `GtkBackend` struct — owns `Rc<RefCell<>>` handles for `ModalStack`, `DragState`, an event queue (`VecDeque<UiEvent>`); accelerator registry; viewport; cached theme + line height; frame-scope pointers (`*const Context`, `*const pango::Layout`)
- `enter_frame_scope(cr, layout, |b| ...)` — stashes type-erased pointers to cairo + pango layout for the closure duration so trait `draw_*` methods reach them
- 4 trait `draw_*` methods (palette/list/tree/form) implemented; 5 (status_bar/tab_bar/activity_bar/terminal/text_display) stubbed pending quadraui trait extension
- `events_handle()`, `push_event()`, `is_modal_open()` API surface for B.5b producers
- `apply_accelerators()`, `match_keypress()` — same shape as TuiBackend; runs on every wait_events drain
- 5 unit tests (cross-backend `paint_overlays` against MockBackend, modal-handle round-trip, accelerator register/unregister, push_event round-trip, is_modal_open tracking)

## src/gtk/services.rs — 96 lines
**Phase B.5: `quadraui::PlatformServices` impl for GTK.**
- `GtkPlatformServices.clipboard().write_text(text)` wired to `gdk::Display::default()?.clipboard().set_text(text)`
- `open_url(url)` wired to `gtk4::gio::AppInfo::launch_default_for_uri`
- `read_text` / file dialogs / notifications stay stubbed — GTK's APIs are async, trait shape is sync; vimcode's existing engine clipboard plumbing covers the read path
- `platform_name()` returns `"gtk"`

## src/gtk/events.rs — 332 lines
**Phase B.5: GDK→`UiEvent` translation helpers.** `#![allow(dead_code)]` — no producers wired yet (B5b.1 in #249).
- `gdk_key_to_uievent()`, `gdk_button_to_mouse_down/up()`, `gdk_motion_to_uievent()`, `gdk_scroll_to_uievent()`, `gdk_resize_to_uievent()`
- `gdk_modifiers_to_quadraui()`, `gdk_keyname_to_named_key()`, `gdk_button_to_quadraui()`
- 10 unit tests covering modifier translation, named keys (Esc/Enter/F1…F12 with KP_* aliases), button mapping, scroll y-axis flip, resize

## src/gtk/click.rs — 563 lines
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

## src/gtk/explorer.rs — 196 lines
Explorer panel state + row tree building for the GTK sidebar (post-Phase-A.2b migration from native `gtk4::TreeView` to a `DrawingArea` rendered via `quadraui_gtk::draw_tree`).
- Tree row collection + expansion state
- Git status and LSP diagnostic indicator computation per row
- Name validation for rename/create operations
