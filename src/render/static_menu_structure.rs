use super::*;

// ─── Static menu structure ────────────────────────────────────────────────────

/// Static description of every top-level menu and its items.
/// Layout: (menu_name, alt_key_char, items).
/// Used by both backends to render the menu bar and by the engine to dispatch actions.
pub static MENU_STRUCTURE: &[(&str, char, &[MenuItemData])] = &[
    (
        "File",
        'f',
        &[
            MenuItemData {
                label: "New Tab",
                // The shortcut field in PALETTE_COMMANDS does not reflect the
                // actual keybinding, which is live-bound to the terminal toggle.
                shortcut: "",
                vscode_shortcut: "",
                action: "tabnew",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Open File…",
                shortcut: "",
                vscode_shortcut: "",
                action: "open_file_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Open Folder…",
                shortcut: "",
                vscode_shortcut: "",
                action: "open_folder_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Open Recent…",
                shortcut: "",
                vscode_shortcut: "",
                action: "openrecent",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Open Workspace From File…",
                shortcut: "",
                vscode_shortcut: "",
                action: "open_workspace_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Save Workspace As…",
                shortcut: "",
                vscode_shortcut: "",
                action: "save_workspace_as_dialog",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Save",
                shortcut: "Ctrl+S",
                vscode_shortcut: "",
                action: "w",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Save As",
                shortcut: "",
                vscode_shortcut: "",
                action: "saveas",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Quit",
                shortcut: "",
                vscode_shortcut: "Ctrl+Q",
                action: "quit_menu",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Edit",
        'e',
        &[
            MenuItemData {
                label: "Undo",
                shortcut: "u",
                vscode_shortcut: "Ctrl+Z",
                action: "undo",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Redo",
                shortcut: "Ctrl+R",
                vscode_shortcut: "Ctrl+Y",
                action: "redo",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Cut",
                shortcut: "",
                vscode_shortcut: "Ctrl+X",
                action: "cut",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Copy",
                shortcut: "",
                vscode_shortcut: "Ctrl+C",
                action: "clipboard_copy",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Paste",
                shortcut: "",
                vscode_shortcut: "Ctrl+V",
                action: "paste",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Find",
                shortcut: "Ctrl+F",
                vscode_shortcut: "",
                action: "find",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Replace",
                shortcut: "",
                vscode_shortcut: "Ctrl+H",
                action: "replace",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        // VS Code's menu bar is File · Edit · Selection · View · Go · Run ·
        // Terminal · Help — `Selection` sits here, between `Edit`
        // and `View`. Only entries with a backing engine command are
        // listed; VS Code's Expand/Shrink Selection, Copy Line Up/Down,
        // Duplicate Selection, Add Cursors to Line Ends, Add Previous
        // Occurrence and Column Selection Mode have no vimcode equivalent
        // yet and are deliberately left out rather than wired to dead
        // actions.
        "Selection",
        's',
        &[
            MenuItemData {
                label: "Select All",
                shortcut: "",
                vscode_shortcut: "Ctrl+A",
                action: "select_all",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Move Line Up",
                shortcut: "",
                vscode_shortcut: "Alt+Up",
                action: "MoveLineUp",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Move Line Down",
                shortcut: "",
                vscode_shortcut: "Alt+Down",
                action: "MoveLineDown",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                // The real VS Code chord for this command is
                // Ctrl+Alt+Up, not Alt+Shift+Up — that chord now duplicates
                // the line instead (`render::route_alt_key`'s `ctrl`
                // parameter; see `Engine::vscode_copy_line_up`).
                label: "Add Cursor Above",
                shortcut: "",
                vscode_shortcut: "Ctrl+Alt+Up",
                action: "add_cursor_above",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Add Cursor Below",
                shortcut: "",
                vscode_shortcut: "Ctrl+Alt+Down",
                action: "add_cursor_below",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Add Next Occurrence",
                shortcut: "",
                vscode_shortcut: "Ctrl+D",
                action: "add_next_occurrence",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Select All Occurrences",
                shortcut: "",
                vscode_shortcut: "Ctrl+Shift+L",
                action: "select_all_occurrences",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "View",
        'v',
        &[
            MenuItemData {
                label: "Toggle Sidebar",
                shortcut: "Ctrl+B",
                vscode_shortcut: "",
                action: "sidebar",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Toggle Terminal",
                shortcut: "Ctrl+T",
                vscode_shortcut: "",
                action: "terminal",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Zoom In",
                shortcut: "Ctrl++",
                vscode_shortcut: "",
                action: "zoomin",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Zoom Out",
                shortcut: "Ctrl+-",
                vscode_shortcut: "",
                action: "zoomout",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Command Palette",
                shortcut: "Ctrl+Shift+P",
                vscode_shortcut: "",
                action: "palette",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Split Editor Right",
                shortcut: "Ctrl+\\",
                vscode_shortcut: "",
                action: "EditorGroupSplit",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Split Editor Down",
                shortcut: "Ctrl-W E",
                vscode_shortcut: "",
                action: "EditorGroupSplitDown",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Close Editor Group",
                shortcut: "",
                vscode_shortcut: "",
                action: "EditorGroupClose",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Word Wrap",
                shortcut: "",
                vscode_shortcut: "Alt+Z",
                action: "set_wrap_toggle",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Go",
        'g',
        &[
            MenuItemData {
                label: "Go to File",
                shortcut: "Ctrl+P",
                vscode_shortcut: "",
                action: "fuzzy",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Go to Line",
                shortcut: "",
                vscode_shortcut: "",
                action: "goto",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Go to Definition",
                // `gd` is Vim's local-declaration motion, not this LSP
                // command (:h gd) — the tag-jump `Ctrl-]` invokes the server.
                shortcut: "Ctrl+]",
                vscode_shortcut: "F12",
                action: "def",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Find References",
                shortcut: "gr",
                vscode_shortcut: "Shift+F12",
                action: "refs",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Back",
                shortcut: "Ctrl+O",
                vscode_shortcut: "",
                action: "back",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Forward",
                shortcut: "Ctrl+I",
                vscode_shortcut: "",
                action: "fwd",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Run",
        'r',
        &[
            MenuItemData {
                label: "Start Debugging",
                shortcut: "F5",
                vscode_shortcut: "",
                action: "debug",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Continue",
                shortcut: "F5",
                vscode_shortcut: "",
                action: "continue",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Pause",
                shortcut: "F6",
                vscode_shortcut: "",
                action: "pause",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Stop",
                shortcut: "Shift+F5",
                vscode_shortcut: "",
                action: "stop",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Step Over",
                shortcut: "F10",
                vscode_shortcut: "",
                action: "stepover",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Step Into",
                shortcut: "F11",
                vscode_shortcut: "",
                action: "stepin",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Step Out",
                shortcut: "Shift+F11",
                vscode_shortcut: "",
                action: "stepout",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "",
                shortcut: "",
                vscode_shortcut: "",
                action: "",
                enabled: false,
                separator: true,
            },
            MenuItemData {
                label: "Toggle Breakpoint",
                shortcut: "F9",
                vscode_shortcut: "",
                action: "brkpt",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Terminal",
        't',
        &[
            MenuItemData {
                label: "New Terminal",
                shortcut: "",
                vscode_shortcut: "",
                action: "terminal",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "Close Terminal",
                shortcut: "",
                vscode_shortcut: "",
                action: "termkill",
                enabled: true,
                separator: false,
            },
        ],
    ),
    (
        "Help",
        'h',
        &[
            MenuItemData {
                label: "Key Bindings",
                shortcut: "",
                vscode_shortcut: "",
                action: "Keybindings",
                enabled: true,
                separator: false,
            },
            MenuItemData {
                label: "About",
                shortcut: "",
                vscode_shortcut: "",
                action: "about",
                enabled: true,
                separator: false,
            },
        ],
    ),
];

/// Build `Vec<MenuDef>` from `MENU_STRUCTURE` for `quadraui::MenuSystem`.
/// `is_vscode_mode` selects which shortcut variant to display.
pub fn build_menu_defs(is_vscode_mode: bool) -> Vec<quadraui::MenuDef> {
    MENU_STRUCTURE
        .iter()
        .map(|(name, _alt, items)| quadraui::MenuDef {
            id: quadraui::WidgetId::new(*name),
            // Labels have no `&` because menu-bar mnemonics should only underline
            // while Alt is held. quadraui's alt_char_byte_range now returns None
            // when a label carries no `&`, preventing unconditional underlining.
            label: name.to_string(),
            disabled: false,
            items: items
                .iter()
                .map(|item| {
                    if item.separator {
                        return quadraui::ContextMenuItem::default();
                    }
                    let shortcut = if is_vscode_mode && !item.vscode_shortcut.is_empty() {
                        item.vscode_shortcut
                    } else {
                        item.shortcut
                    };
                    quadraui::ContextMenuItem {
                        id: Some(quadraui::WidgetId::new(item.action)),
                        label: quadraui::StyledText::plain(item.label.to_string()),
                        detail: if shortcut.is_empty() {
                            None
                        } else {
                            Some(quadraui::StyledText::plain(shortcut.to_string()))
                        },
                        disabled: !item.enabled,
                        // Every `shortcut`/`vscode_shortcut` in
                        // `MENU_STRUCTURE` is already plus-style
                        // (`"Ctrl+S"`) — exactly what
                        // `quadraui::parse_key_binding` accepts — so this
                        // is free once parseable. `detail` above still wins
                        // for the *drawn* dropdown's display text (an
                        // in-window `ContextMenuItem`'s doc comment says
                        // so); `key_equivalent` is what the macOS NSMenu
                        // installer (`Backend::install_menu_bar`) reads to
                        // wire the real Cmd-key shortcut, so a native menu
                        // bar's items get working accelerators without
                        // GTK/TUI's drawn dropdown changing at all.
                        key_equivalent: if shortcut.is_empty() {
                            None
                        } else {
                            quadraui::parse_key_binding(shortcut).map(|_| quadraui::Accelerator {
                                id: quadraui::AcceleratorId::new(item.action),
                                binding: quadraui::KeyBinding::Literal(shortcut.to_string()),
                                scope: quadraui::AcceleratorScope::Global,
                                label: None,
                            })
                        },
                        ..Default::default()
                    }
                })
                .collect(),
        })
        .collect()
}

/// Convert [`build_menu_defs`]'s output into a [`quadraui::MenuBar`] for
/// [`quadraui::Backend::install_menu_bar`].
///
/// Pure and backend-neutral: any backend that declares
/// `BackendCaps::native_menu` (macOS's `MacBackend` today) can hand its
/// result straight to `install_menu_bar` instead of the app drawing its own
/// in-window `MenuSystem` row. Each top-level `MenuDef` becomes a
/// `MenuBarItem` whose `submenu` is the *same* `ContextMenuItem` list the
/// drawn dropdown already uses — including the `key_equivalent` populated
/// above — so the native menu and the in-window one can never drift apart
/// (one source of truth, not two).
///
/// #902 (native right-click context menus via `Backend::show_context_menu`)
/// can reuse a `MenuDef`'s `items: Vec<ContextMenuItem>` directly for its own
/// conversion — the per-item shape (including `key_equivalent`) is already
/// exactly what `show_context_menu` needs; nothing here is menu-bar-specific
/// below the top level.
pub fn menu_defs_to_menu_bar(defs: &[quadraui::MenuDef]) -> quadraui::MenuBar {
    quadraui::MenuBar {
        id: quadraui::WidgetId::new("menu_bar"),
        items: defs
            .iter()
            .map(|def| quadraui::MenuBarItem {
                id: def.id,
                label: def.label.clone(),
                disabled: def.disabled,
                submenu: Some(def.items.clone()),
            })
            .collect(),
        open_item: None,
        focused_item: None,
    }
}

/// Fold extension-registered commands declaring `menu = "<name>"` (via
/// `vimcode.command(name, fn, { menu = "File", ... })`) into `defs`, the
/// output of [`build_menu_defs`]. Each entry becomes a `ContextMenuItem`
/// appended to the matching top-level menu (matched by label, case-
/// sensitive, e.g. `"File"`); a `menu` name with no matching built-in menu
/// gets its own new top-level `MenuDef` appended at the end, so a plugin can
/// introduce an entirely new menu-bar menu, not just extend an existing one.
///
/// Selecting a plugin menu item dispatches through the same
/// `quadraui::MenuEvent::Activated(id)` → `Engine::dispatch_menu_action` →
/// `Engine::execute_command` path every built-in menu item already uses —
/// `id` is just the registered command name, so no new dispatch code is
/// needed.
pub fn append_plugin_menu_entries(
    defs: &mut Vec<quadraui::MenuDef>,
    entries: &[crate::core::plugin::PluginCommandEntry],
) {
    for entry in entries {
        let Some(menu_name) = entry.menu.as_ref() else {
            continue;
        };
        let label = entry.title.clone().unwrap_or_else(|| entry.name.clone());
        let item = quadraui::ContextMenuItem {
            id: Some(quadraui::WidgetId::new(&entry.name)),
            label: quadraui::StyledText::plain(label),
            detail: None,
            disabled: false,
            key_equivalent: None,
            ..Default::default()
        };
        match defs.iter_mut().find(|def| &def.label == menu_name) {
            Some(def) => def.items.push(item),
            None => defs.push(quadraui::MenuDef {
                id: quadraui::WidgetId::new(menu_name.as_str()),
                label: menu_name.clone(),
                disabled: false,
                items: vec![item],
            }),
        }
    }
}

/// Static debug toolbar button definitions.
/// Icons use the Unicode fallback glyphs (▶ ⏸ ⏹ ↻ etc.) which render
/// correctly in both TUI (any font) and GTK (no Nerd Font subset needed).
pub static DEBUG_BUTTONS: &[DebugButton] = &[
    DebugButton {
        icon: icons::DBG_CONTINUE.fallback,
        label: "Continue",
        key_hint: "F5",
        action: "continue",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_PAUSE.fallback,
        label: "Pause",
        key_hint: "F6",
        action: "pause",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_STOP.fallback,
        label: "Stop",
        key_hint: "Shift+F5",
        action: "stop",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_RESTART.fallback,
        label: "Restart",
        key_hint: "Ctrl+Shift+F5",
        action: "restart",
        enabled: true,
    },
    // separator goes here (rendered between index 3 and 4)
    DebugButton {
        icon: icons::DBG_STEP_OVER.fallback,
        label: "Step Over",
        key_hint: "F10",
        action: "stepover",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_RESTART.fallback,
        label: "Step Into",
        key_hint: "F11",
        action: "stepin",
        enabled: true,
    },
    DebugButton {
        icon: icons::DBG_STEP_OUT.fallback,
        label: "Step Out",
        key_hint: "Shift+F11",
        action: "stepout",
        enabled: true,
    },
];
