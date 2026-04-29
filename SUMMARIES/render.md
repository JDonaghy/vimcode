# src/render.rs — ~11,941 lines

Platform-agnostic rendering abstraction. Transforms engine state into `ScreenLayout` consumed by both GTK and TUI backends. Contains all themes, render data structs, and the main layout builder.

## Key Types — Colors & Styling
- `Color` — RGB color with hex parsing, lighten/darken, `cursorline_tint()`, Cairo/Pango conversion
- `Style` — fg/bg/bold/italic/underline
- `StyledSpan` — text span with style + column range
- `Theme` — complete color scheme (95+ color fields incl. `scrollbar_thumb`, `scrollbar_track`, `terminal_bg`, `activity_bar_fg`); 6 built-ins + VSCode JSON import; `scope_color()` maps 23 tree-sitter capture names; `semantic_token_style()` handles LSP semantic tokens with `controlFlow` modifier

## Key Types — Editor Content
- `RenderedLine` — single visual line with spans, gutter, diagnostics, git markers, fold state, wrap info
- `StatusAction` — re-exported from core; action enum for clickable status segments (GoToLine, ChangeLanguage, etc.)
- `StatusSegment` — styled segment of a per-window status line (text, fg, bg, bold, action)
- `WindowStatusLine` — per-window status bar (left/right segment vectors)
- `RenderedWindow` — complete window render data (lines, cursor, selection, scrollbars, `status_line: Option<WindowStatusLine>`, etc.)
- `CursorPos` / `CursorShape` — cursor position and shape
- `SelectionRange` / `SelectionKind` — visual selection data
- `DiagnosticMark` / `SpellMark` — underline markers

## Key Types — UI Components
- `ScreenLayout` — top-level render output consumed by backends; `status_branch_range: Option<(usize, usize)>` for click detection
- `EditorGroupSplitData` — group layout with tab bars and windows
- `GroupTabBar` / `TabInfo` — tab strip data (includes `tab_scroll_offset`)
- `BreadcrumbBar` / `BreadcrumbSegment` — file path breadcrumbs
- `CompletionMenu` — LSP/word completion dropdown
- `HoverPopup` / `EditorHoverPopupData` — hover information popup
- `SignatureHelp` — function signature popup
- `PickerPanel` / `PickerPanelItem` — fuzzy finder panel
- `QuickfixPanel` — quickfix list
- `SourceControlData` / `ScFileItem` / `ScLogItem` — git panel data
- `ExtSidebarData` / `ExtSidebarItem` — extensions panel data
- `ExtPanelData` / `ExtPanelSectionData` — Lua extension panels
- `PanelHoverPopupData` — panel hover popup
- `AiPanelData` / `AiPanelMessage` — AI chat panel
- `DebugSidebarData` / `DebugSidebarItem` — DAP debug panel
- `TerminalPanel` / `TerminalCell` — terminal rendering
- `MenuBarData` / `MenuItemData` — menu bar + dropdown
- `DebugToolbarData` / `DebugButton` — debug control buttons
- `DialogPanel` / `ContextMenuPanel` — dialogs and context menus
- `FindReplacePanel` — Ctrl+F find/replace overlay (query, replacement, toggles, match_info, sel_anchor, group_bounds, panel_width, hit_regions)
- `FindReplaceClickTarget` — re-exported from engine; click target enum for shared dispatch (13 variants)
- `FrHitRegion` — re-exported from engine; hit region in char-cell units
- `FR_PANEL_WIDTH` — re-exported from engine; default panel width constant
- `compute_find_replace_hit_regions()` — re-exported from engine; computes hit regions for find/replace overlay
- `CommandLineData` / `WildmenuData` — command line + completion
- `DiffPeekPopup` — inline diff hunk popup
- `DiffToolbarData` — diff view toolbar

## Key Types — Shared Hit-Testing & Geometry
- `ClickTarget` — semantic editor click target enum (TabBar, Gutter, BufferPos, SplitButton, CloseTab, StatusBarAction, etc.) — moved from gtk/click.rs for multi-backend sharing

## Key Types — Backend Parity
- `UiElement` (27 variants) — every renderable element a backend must draw; source of truth for rendering parity
- `UiAction` (26 variants) — every user interaction a backend must handle; source of truth for click/mouse parity
- `collect_expected_ui_elements(layout)` — walks ScreenLayout to list expected rendered elements
- `all_required_ui_actions()` — canonical list of all required interaction handlers
- `collect_ui_actions_tui()` / `collect_ui_actions_wingui()` — per-backend action lists for parity testing
- `collect_ui_elements_tui(layout)` / `collect_ui_elements_wingui(layout)` — per-backend element lists

## Key Functions
- `build_screen_layout(engine, theme, rects, line_height, char_width)` — main layout builder (~3,300 lines)
- `to_q_editor(rw)` — boundary adapter `&RenderedWindow → quadraui::Editor` (#276 Stage 1C). Plus `to_q_editor_line`, `to_q_styled_span`, `to_q_cursor_pos`, `to_q_cursor_shape`, `to_q_selection`, `to_q_severity`, `to_q_git_status`, `to_q_diff_line`, `to_q_diagnostic_mark`, `to_q_spell_mark` per-type converters.
- `format_button_label(label, hotkey)` — dialog button label formatter
- `visual_rows_for_line(len, cols)` — wrapped line row count
- `Theme::from_name(name)` — theme lookup by name
- `Theme::from_vscode_json(path)` — import VSCode JSON theme
- `Theme::scope_color(scope)` — tree-sitter scope to color mapping
- `Theme::semantic_token_style(type, modifiers)` — semantic token styling

## Shared Geometry Helpers (multi-backend)
- `tab_row_height_px(line_height)` — tab row height as ceil(line_height * 1.6)
- `tab_bar_height_px(line_height, breadcrumbs)` — tab bar + optional breadcrumb row
- `status_bar_height_px(line_height, per_window_status, has_wildmenu)` — global status bar height
- `editor_bottom_px(total_height, ...)` — Y coordinate where editor area ends (accounts for all chrome)
- `scrollbar_click_to_scroll_top(click_pos, track_len, total_lines, viewport_lines)` — maps scrollbar click to scroll position
- `display_col_to_buffer_col(line_text, x_offset, tabstop, scroll_left)` — tab-aware column conversion
- `is_tab_close_click(col_in_tab, tab_width, close_cols)` — detects close button zone in tab
- `matches_key_binding(binding, ctrl, shift, alt, key_char, ...)` — backend-agnostic Vim key notation matcher

## Constants
- `MENU_STRUCTURE` — 7-menu application menu definition
- `DEBUG_BUTTONS` — 7 debug toolbar button definitions
- `SETTING_DEFS` — settings sidebar category/item definitions
- `PALETTE_COMMANDS` — ~65 command palette entries
