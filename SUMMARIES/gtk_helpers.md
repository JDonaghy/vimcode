# GTK Helper Files

## src/gtk/click.rs — 575 lines
Mouse click/drag/double-click handling for GTK backend. Maps pixel coordinates to logical click targets.
- `ClickTarget` — enum: TabBar, Editor, Gutter, Scrollbar, Sidebar, StatusLine, etc.
- `pixel_to_click_target()` — converts (x,y) pixel position to a `ClickTarget`
- Click/drag/double-click handler functions dispatched from `App::update()`

## src/gtk/css.rs — 525 lines
GTK CSS theme generation and loading.
- `make_theme_css(theme)` — generates dynamic CSS string from a `Theme` struct
- `STATIC_CSS` — constant CSS for non-theme-dependent styles
- `load_css()` — applies CSS to GTK display

## src/gtk/util.rs — 468 lines
GTK utility functions and settings form builders.
- `matches_gtk_key(key_name, target)` — key name matching helper
- Settings form builder functions for the settings sidebar (GTK native widgets)
- `install_icon()` — installs Nerd Font icons into GTK icon theme

## src/gtk/tree.rs — 433 lines
File explorer tree building with GTK TreeStore.
- Tree node building/expansion for file explorer sidebar
- Git status and LSP diagnostic indicator computation per tree row
- `update_tree_indicators()` — refreshes git/diagnostic badges on tree nodes
- Name prompt/validation for rename operations
