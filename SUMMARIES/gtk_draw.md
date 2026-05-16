# src/gtk/draw.rs — 3,882 lines

All Cairo/Pango drawing functions for the GTK backend. Each `draw_*` function renders one UI component onto a Cairo context using data from `ScreenLayout`.

## Draw Functions
- `draw_editor` — main editor area (all windows, gutters, text, cursors)
- `draw_window` — single editor window; **collapsed to ~25-line delegator post #276** (calls `quadraui::gtk::draw_editor` via `render::to_q_editor` + `q_theme()`). The pre-#276 paint body + `build_pango_attrs` + `draw_visual_selection` helpers now live in `quadraui/src/gtk/editor.rs`.
- `draw_tab_bar` — tab strip per editor group with scroll offset + `…` action menu button
- `draw_breadcrumb_bar` — file path breadcrumbs below tab bar
- `draw_h_scrollbars` — horizontal scrollbars
- `draw_tab_drag_overlay` — drag indicator when moving tabs
- `draw_window_status_bar` — per-window status bar with styled segments
- `draw_window_separators` — dividers between split windows
- `draw_completion_popup` — LSP/word completion dropdown; **delegator post #285** (calls `quadraui::gtk::draw_completions` via the `quadraui_gtk::draw_completions` shim + `Completions::layout()`). Body math now lives in `quadraui/src/gtk/completions.rs`.
- `draw_hover_popup` — LSP hover information popup
- `draw_editor_hover_popup` — editor hover with markdown rendering
- `draw_diff_peek_popup` — inline diff hunk preview
- `draw_signature_popup` — function signature help
- `draw_picker_popup` — unified fuzzy finder (files/grep/commands)
- `draw_tab_switcher_popup` — Ctrl-Tab tab switcher
- `draw_dialog_popup` — modal dialog with buttons
- `draw_bottom_panel_tabs` — terminal/output panel tab strip
- `draw_debug_output` — debug output panel
- `draw_debug_sidebar` — DAP variables/call stack/watch/breakpoints; **delegator post #281** (chrome paints panel header + Run/Stop button + section titles + per-section scrollbar overlays; per-section item rendering goes through `Backend::draw_tree` × 4 via `render::debug_sidebar_section_to_tree_view`). Click handler walks pixel coordinates with `1.4 × line_height` per item row to match `quadraui::gtk::draw_tree`'s row measure.
- `draw_quickfix_panel` — quickfix list
- `draw_terminal_panel` — integrated terminal
- `draw_terminal_cells` — terminal cell grid rendering
- `draw_status_line` — bottom status bar
- `draw_wildmenu` — command-line completion menu
- `draw_command_line` — ex command / search input line
- `draw_menu_bar` — application menu bar with centered nav arrows (◀ ▶) and Command Center search box; returns `(back_x, back_end, fwd_x, fwd_end, unit_end)` for click hit-testing
- `draw_menu_dropdown` — menu dropdown overlay
- `draw_source_control_panel` — git source control sidebar
- `draw_settings_panel` — settings sidebar (header + search + `quadraui_gtk::draw_form` + scrollbar + footer)
- `draw_explorer_panel` — file explorer sidebar (Phase A.2b-1 scaffolding; inert until sub-phase 2 wires it in). Calls `quadraui_gtk::draw_tree` + 8px Cairo scrollbar overlay matching `draw_settings_panel`.
- `draw_ext_dyn_panel` — Lua extension panels
- `draw_panel_hover_popup` — hover popup for panel items
- `draw_ext_sidebar` — extensions marketplace sidebar; **delegator post #280** (chrome paints panel header + search input + focus border; section headers and items go through `Backend::draw_tree` via `render::ext_sidebar_to_tree_view`, using `Decoration::Header` for INSTALLED/AVAILABLE titles).
- `draw_ai_sidebar` — AI chat panel
- `draw_debug_toolbar` — debug control buttons
