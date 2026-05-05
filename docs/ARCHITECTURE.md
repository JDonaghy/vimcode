# Architecture Reference

Load this file when working on code structure, adding files, or navigating unfamiliar modules.

## Directory Layout

### GTK directory (`src/gtk/`)

| File | What goes here |
|------|---------------|
| `mod.rs` | App struct, Msg enum, `SimpleComponent` impl (view/init/update), `impl App`, geometry helpers |
| `draw.rs` | All `draw_*` free functions (editor, panels, popups, sidebars) |
| `click.rs` | `ClickTarget` enum, `pixel_to_click_target()`, mouse click/drag/double-click handlers |
| `css.rs` | `make_theme_css()`, `STATIC_CSS`, `load_css()` |
| `util.rs` | `matches_gtk_key()`, settings form builders, GTK utilities, icon install |
| `tree.rs` | File tree building/expansion/indicators, name prompt/validation |

### TUI directory (`src/tui_main/`)

| File | What goes here |
|------|---------------|
| `mod.rs` | Structs, `run()`, `event_loop()`, clipboard, key translation, cell helpers |
| `render_impl.rs` | `draw_frame()`, `build_screen_for_tui()`, tab bar, editor/popup rendering |
| `panels.rs` | Sidebar panel rendering (activity bar, explorer, git, debug, extensions, AI, search, terminal) |
| `mouse.rs` | `handle_mouse()` — all click/drag/scroll interactions |

### Win-GUI directory (`src/win_gui/`)

Native Windows backend using `windows-rs` + Direct2D + DirectWrite. Behind `win-gui` Cargo feature. Consumes `ScreenLayout` from `render.rs` — same pattern as GTK/TUI. Some features are still missing (see BUGS.md for known Win-GUI gaps).

| File | What goes here |
|------|---------------|
| `mod.rs` | Win32 window creation, D2D/DWrite setup, event loop, keyboard/mouse handling, rendering |

### Engine directory (`src/core/engine/`)

The Engine is split into focused submodules. Each file adds `impl Engine` blocks — Rust resolves methods across files transparently.

| File | What goes here |
|------|---------------|
| `mod.rs` | Types, enums, `Engine` struct def, `new()`, free functions, `mod` declarations |
| `keys.rs` | `handle_key`, `handle_normal_key`, `handle_pending_key`, operator motions, macros, repeat, user keymaps |
| `insert.rs` | *(future)* `handle_insert_key`, `handle_replace_key` — currently in keys.rs |
| `command.rs` | *(future)* `handle_command_key`, `handle_search_key` — currently in keys.rs |
| `visual.rs` | `handle_visual_key`, visual helpers, multi-cursor |
| `execute.rs` | `execute_command()` — the ex-command dispatcher |
| `motions.rs` | Cursor movement, word/paragraph/scroll, bracket nav, join, indent, jump list |
| `buffers.rs` | File I/O, syntax update, undo/redo, git diff, markdown preview, netrw, workspace |
| `windows.rs` | Window/tab/group splits, focus, resize, session restore |
| `accessors.rs` | Group/buffer/window facades |
| `search.rs` | Project search/replace, search highlighting |
| `source_control.rs` | All `sc_*` methods, `handle_sc_*` key handlers |
| `lsp_ops.rs` | All `lsp_*` methods, code actions, diagnostics, hover, completion |
| `ext_panel.rs` | `ext_*` methods, `handle_ext_*`, extension + settings panel |
| `panels.rs` | AI (`ai_*`), dialog system, swap files |
| `plugins.rs` | Plugin init, event dispatch, command/keymap hooks |
| `dap_ops.rs` | DAP/debug: poll_dap, breakpoints, sidebar, stepping |
| `vscode.rs` | VSCode mode, menu bar methods |
| `picker.rs` | Fuzzy score, unified picker, quickfix |
| `terminal_ops.rs` | All `terminal_*` methods |
| `spell_ops.rs` | Spell checking methods |
| `tests.rs` | All test functions + helpers |

**File size rule:** No single file should exceed ~5,000 lines. If a submodule grows past that, split it further (e.g. `keys.rs` → `keys.rs` + `insert.rs` + `command.rs`). Place new `impl Engine` methods in the submodule matching their responsibility — never dump unrelated methods into `mod.rs`.

**Submodule conventions:**
- Each submodule starts with `use super::*;` to import all engine types
- Methods that other submodules call must be `pub(crate) fn`, not `fn`
- References to sibling core modules use `crate::core::module::` (not `super::module::`, which would look inside engine/)
- Free functions used across submodules stay in `mod.rs` and are accessed via `super::function_name`

## Data Model
```
Engine
├── BufferManager { HashMap<BufferId, BufferState> }
│   └── BufferState { buffer: Buffer, file_path, dirty, syntax, undo/redo }
├── windows: HashMap<WindowId, Window { buffer_id, view }>
├── tabs: Vec<Tab { layout: WindowLayout (binary tree), active_window }>
├── registers: HashMap<char, (String, bool)>  # (content, is_linewise)
└── State: mode, command_buffer, message, search_*, pending_key, pending_operator
```

**Concepts:** Buffer (in-memory file) | Window (viewport+cursor) | Tab (window layout) | Multiple windows can show same buffer.
