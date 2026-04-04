# src/gtk/draw.rs — 5,944 lines

All Cairo/Pango drawing functions for the GTK backend. Each `draw_*` function renders one UI component onto a Cairo context using data from `ScreenLayout`.

## Draw Functions
- `draw_editor` — main editor area (all windows, gutters, text, cursors)
- `draw_window` — single editor window with syntax-highlighted lines
- `draw_visual_selection` — visual mode selection overlay
- `draw_tab_bar` — tab strip per editor group with scroll offset + `…` action menu button
- `draw_breadcrumb_bar` — file path breadcrumbs below tab bar
- `draw_h_scrollbars` — horizontal scrollbars
- `draw_tab_drag_overlay` — drag indicator when moving tabs
- `draw_window_status_bar` — per-window status bar with styled segments
- `draw_window_separators` — dividers between split windows
- `draw_completion_popup` — LSP/word completion dropdown
- `draw_hover_popup` — LSP hover information popup
- `draw_editor_hover_popup` — editor hover with markdown rendering
- `draw_diff_peek_popup` — inline diff hunk preview
- `draw_signature_popup` — function signature help
- `draw_picker_popup` — unified fuzzy finder (files/grep/commands)
- `draw_tab_switcher_popup` — Ctrl-Tab tab switcher
- `draw_dialog_popup` — modal dialog with buttons
- `draw_bottom_panel_tabs` — terminal/output panel tab strip
- `draw_debug_output` — debug output panel
- `draw_debug_sidebar` — DAP variables/call stack/watch/breakpoints
- `draw_quickfix_panel` — quickfix list
- `draw_terminal_panel` — integrated terminal
- `draw_terminal_cells` — terminal cell grid rendering
- `draw_status_line` — bottom status bar
- `draw_wildmenu` — command-line completion menu
- `draw_command_line` — ex command / search input line
- `draw_menu_bar` — application menu bar with centered nav arrows (◀ ▶) and Command Center search box; returns `(back_x, back_end, fwd_x, fwd_end, unit_end)` for click hit-testing
- `draw_menu_dropdown` — menu dropdown overlay
- `draw_source_control_panel` — git source control sidebar
- `draw_ext_dyn_panel` — Lua extension panels
- `draw_panel_hover_popup` — hover popup for panel items
- `draw_ext_sidebar` — extensions marketplace sidebar
- `draw_ai_sidebar` — AI chat panel
- `draw_debug_toolbar` — debug control buttons
