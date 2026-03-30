# src/core/engine/mod.rs — 3,415 lines

Core engine definition. Contains the `Engine` struct (all editor state), enums, types, `new()` constructor, free functions, and `mod` declarations for all submodules.

## Key Types
- `Engine` — main editor state struct (~830 fields covering buffers, windows, groups, mode, LSP, DAP, search, terminal, plugins, etc.)
- `EngineAction` — enum returned by key handlers (None, Quit, OpenFile, Redraw, etc.)
- `Mode` — editor mode (Normal, Insert, Visual, VisualLine, VisualBlock, Command, Search, Replace)
- `PickerSource` / `PickerItem` / `PickerAction` — unified picker types (includes `CommandCenter` source, `GotoLine(usize)` and `GotoSymbol(PathBuf, usize, usize)` actions)
- `Dialog` / `DialogButton` / `DialogInput` — modal dialog system
- `PaletteCommand` — command palette entry (includes "Go: Command Center")
- `DiffLine` / `AlignedDiffEntry` — diff display types
- `TabDragState` — tab drag-and-drop state
- `ContextMenuState` / `ContextMenuItem` — right-click context menus
- `PanelHoverPopup` / `EditorHoverPopup` — hover popup state
- `EditorGroup` — tab group with own tab list + `tab_scroll_offset` for overflow scrolling
- `UserKeymap` — user-defined key remapping
- `DiffPeekState` — inline diff peek popup state
- `SwapRecovery` — crash recovery swap file state
- `SettingsRow` — settings panel row identifier
- `ExplorerNewEntryState` — inline new-file/folder creation state (parent_dir, input, cursor, is_folder)

## Key Functions
- `Engine::new()` — constructor, initializes all state
- `Engine::open(path)` — create engine with a file open
- `normalize_ex_command(input)` — abbreviation expansion for ex commands
- `build_aligned_diff(left, right)` — side-by-side diff alignment
- `lcs_diff(a, b)` — LCS-based line diff
- `is_safe_url(url)` — URL safety check for link opening

## Submodules
keys, motions, execute, visual, buffers, windows, accessors, search, source_control, lsp_ops, ext_panel, panels, plugins, dap_ops, vscode, picker, terminal_ops, spell_ops, tests
