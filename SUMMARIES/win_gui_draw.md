# src/win_gui/draw.rs — Win-GUI Rendering (4,521 lines)

Direct2D rendering of `ScreenLayout`. Consumes platform-agnostic layout and paints via D2D render target + DirectWrite.

## Key Types
- `DrawContext` — rt, dwrite, format, ui_format, icon_format, theme, char_width, line_height, editor_left

## Key Functions
- `draw_frame(layout)` — main render: menu bar, caption buttons, tab bars, breadcrumbs, editor windows, status, command line, cursors, popups, terminal
- `draw_tab_bar(layout)` — single-group tab bar with diff toolbar reserve
- `draw_group_tab_bar(gtb)` — multi-group tab bar from GroupTabBar bounds
- `draw_tabs(tabs, max_width)` — renders tab labels, close buttons, accent; stops at max_width
- `draw_editor_window(rw)` — clipped editor: gutter, line content, selection, diagnostics, fold markers
- `draw_sidebar(sidebar, screen, engine)` — activity bar icons (fixed + ext panels), panel background, panel content dispatch
- `draw_ext_panel(screen, engine)` — extension panel: header, search input, flat rows (sections/items/badges/actions), scrollbar, help popup
- `draw_explorer_panel()` — file tree with icons, indent, expand/collapse
- `draw_git_panel()` — full source control panel: header, commit input, button row, 4 collapsible sections, selection highlight, scrollbar, branch picker popup, help dialog
- `draw_search_panel()` — query/replace inputs, toggle indicators, results
- `draw_ai_panel()` — conversation messages, input box
- `draw_settings_panel()` — categories, setting rows, search
- `draw_breadcrumb_bar(bc)` — path + symbol segments; skips empty segments
- `draw_terminal(term)` — terminal cells, toolbar, find bar
- `draw_menu_bar(data)` — menu labels, nav arrows, command center
- `draw_text/draw_ui_text/draw_icon_text` — monospace/proportional/icon font rendering
- `mono_text_width(text)` — approximate width using char_width * char count
- `parse_badge_color_d2d(color)` — hex/named color to Color for badge rendering
