# src/core/engine/mod.rs — 3,624 lines

Core engine definition. Contains the `Engine` struct (all editor state), enums, types, `new()` constructor, free functions, and `mod` declarations for all submodules.

## Key Types
- `Engine` — main editor state struct (~830 fields covering buffers, windows, groups, mode, LSP, DAP, search, terminal, plugins, etc.)
- `EngineAction` — enum returned by key handlers (None, Quit, OpenFile, Redraw, etc.)
- `Mode` — editor mode (Normal, Insert, Visual, VisualLine, VisualBlock, Command, Search, Replace)
- `PickerSource` / `PickerItem` / `PickerAction` — unified picker types (includes `CommandCenter` source, `GotoLine(usize)` and `GotoSymbol(PathBuf, usize, usize)` actions; `PickerItem` has `depth`/`expandable`/`expanded` for tree view)
- `StatusAction` — enum for clickable status bar segments (GoToLine, ChangeLanguage, ChangeIndentation, ChangeLineEnding, ChangeEncoding, SwitchBranch, LspInfo, ToggleSidebar, TogglePanel, ToggleMenuBar, DismissNotifications)
- `NotificationKind` — enum for background operation types (LspInstall, LspIndexing, ExtensionInstall, GitOperation, ProjectSearch, ProjectReplace)
- `Notification` — background operation tracking (id, kind, message, done, created_at, done_at)
- `Dialog` / `DialogButton` / `DialogInput` — modal dialog system
- `PaletteCommand` — command palette entry (includes "Go: Command Center")
- `DiffLine` / `AlignedDiffEntry` — diff display types
- `TabDragState` — tab drag-and-drop state
- `ContextMenuState` / `ContextMenuItem` / `ContextMenuTarget` — context menus (Tab, ExplorerFile, ExplorerDir, Editor, EditorActionMenu, ExtPanel)
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
- `Engine::notify(kind, msg)` — push in-progress notification, returns ID
- `Engine::notify_done(id, msg)` — mark notification as done by ID
- `Engine::notify_done_by_kind(kind, msg)` — mark all notifications of a kind as done
- `Engine::dismiss_notification(id)` — remove notification by ID
- `Engine::dismiss_done_notifications()` — remove all completed notifications
- `Engine::tick_notifications()` — auto-dismiss completed notifications after 5s timeout
- `normalize_ex_command(input)` — abbreviation expansion for ex commands
- `build_aligned_diff(left, right)` — side-by-side diff alignment
- `lcs_diff(a, b)` — LCS-based line diff
- `is_safe_url(url)` — URL safety check for link opening

## Submodules
keys, motions, execute, visual, buffers, windows, accessors, search, source_control, lsp_ops, ext_panel, panels, plugins, dap_ops, vscode, picker, terminal_ops, spell_ops, tests
