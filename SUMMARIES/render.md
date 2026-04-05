# src/render.rs — 7,384 lines

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
- `CommandLineData` / `WildmenuData` — command line + completion
- `DiffPeekPopup` — inline diff hunk popup
- `DiffToolbarData` — diff view toolbar

## Key Functions
- `build_screen_layout(engine, theme, rects, line_height, char_width)` — main layout builder (~3,300 lines)
- `format_button_label(label, hotkey)` — dialog button label formatter
- `visual_rows_for_line(len, cols)` — wrapped line row count
- `Theme::from_name(name)` — theme lookup by name
- `Theme::from_vscode_json(path)` — import VSCode JSON theme
- `Theme::scope_color(scope)` — tree-sitter scope to color mapping
- `Theme::semantic_token_style(type, modifiers)` — semantic token styling

## Constants
- `MENU_STRUCTURE` — 7-menu application menu definition
- `DEBUG_BUTTONS` — 7 debug toolbar button definitions
- `SETTING_DEFS` — settings sidebar category/item definitions
- `PALETTE_COMMANDS` — ~65 command palette entries
