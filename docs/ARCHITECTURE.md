# Architecture Reference

Load this file when working on code structure, adding files, or navigating unfamiliar modules.

## Directory Layout

### Crate layout: everything is in the library (#657)

`[lib] vimcode_core` (`src/lib.rs`) owns **all** the code: `core`, `icons`,
`render`, `tui_main`, and `gtk` (the last behind the `gui` feature). The two
binaries are thin shims that parse argv and call in:

| Target | File | Role |
|---|---|---|
| `[lib] vimcode_core` | `src/lib.rs` | every module; `pub mod gtk` and `pub mod app` are `#[cfg(feature = "gui")]` |
| `[[bin]] vimcode` | `src/main.rs` | `required-features = ["gui"]`; `use vimcode_core::{gtk, tui_main}` |
| `[[bin]] vcd` | `src/tui_bin.rs` | TUI-only; `use vimcode_core::tui_main` |

Before #657, `render`/`tui_main`/`gtk` were private `mod`s **inside the
binaries**, so `tests/*.rs` — a separate crate linking only against the lib —
could see nothing but `core` + `icons`. That is why every black-box test in
this repo is in-crate: it had to be. It also blocked the coordinator's oracle
loop, which hardcodes a `tests/acceptance/` integration-test directory. If you
are adding a module, add it to `src/lib.rs`, not to a bin.

The `test-support` feature (off by default) re-exports the two black-box
harnesses for that external crate: `gtk::testing::{Harness, harness}` (the #646
headless `GtkDriver` wrapper) and `tui_main::testing::TuiShellApp` (for
quadraui's `driver_with_shell`). See `tests/acceptance.rs` and
`tests/acceptance/ms-example/contract.md`.

### The two backends, and what is left in them

Both backends are `quadraui::ShellApp` impls driven by `run_with_shell`; neither
owns a main loop (`fn event_loop` was deleted by #634). Since #751–#766 they route
every routing/composition *decision* through `render.rs` — `src/gtk/mod.rs` alone
makes 424 `render::` calls — so when you are looking for "where is X decided", the
answer is almost always `render.rs`, not here.

**Do not add feature logic to either directory** (`CLAUDE.md`, Platform-Neutrality
Rule). Current production size and the north-star target are tracked in
[`GOALS.md`](../GOALS.md); regenerate with
`python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs`.

**What genuinely has to stay per-backend is enumerated in
[`IRREDUCIBLE_SURFACE.md`](IRREDUCIBLE_SURFACE.md)** — three facts plus ~246 lines
(1.3%) that name a toolkit type. If you are about to write per-backend code because
"the backends just differ here", check that list first: the answer is usually that
they don't.

### The shell application (`src/app.rs`)

`struct App`, its inherent `impl` blocks and `impl quadraui::ShellApp for App`
(`setup`/`render_content`/`handle`/`tick`), plus `DeferredQueue` /
`DeferredAction` / `GtkAccelHost` and the GDK-key → `quadraui::UiEvent`
mappers. #785 (stage 1 of #47, the native macOS GUI) hoisted all ~6,900 lines
out of `src/gtk/mod.rs`, which shrank from 9,650 to ~2,580 production lines.

The module is **still `#[cfg(feature = "gui")]`** — the move is the mechanical
half of the split, not the end of it. `src/app.rs`'s own module doc enumerates
the three things that still tie it to GTK (four platform-typed fields, ~11
platform hook call sites, and the `crate::gtk::{click, css, util}` dependency)
so the next stage does not have to re-derive them.

### GTK directory (`src/gtk/`)

| File | What goes here |
|------|---------------|
| `mod.rs` | `run()`, `build_shell_config()`, tab-bar/scrollbar geometry helpers, GTK font + close-glyph metrics. `App` itself moved to `src/app.rs` (#785) and is re-exported here as `crate::gtk::App` so the submodules keep resolving `super::App`. |
| `click.rs` | `pixel_to_click_target()`, tab-bar hit resolution (Pango vs char-cell), gutter actions, mouse click/double-click/drag entry points |
| `css.rs` | `make_theme_css()`, `STATIC_CSS`, `load_css()` — genuinely GTK-only |
| `util.rs` | `open_url()`, bundled Nerd Font install, GTK utilities |
| `testing.rs` | The headless `GtkDriver` black-box harness (#646), behind `test-support` |
| `backend.rs`, `events.rs`, `services.rs`, `explorer.rs` | Re-export / placeholder shims only — the real implementations were lifted into `quadraui::gtk::*` (#270) and `engine/explorer_ops.rs` |

> `draw.rs` and `tree.rs` no longer exist. `draw.rs` (all the `draw_*` free
> functions) was deleted by #669–#672 once GTK's live path painted every
> `ScreenLayout` field; explorer state moved to the engine.

### TUI directory (`src/tui_main/`)

| File | What goes here |
|------|---------------|
| `shell_app.rs` | `TuiShellApp` — `impl ShellApp for TuiShellApp`, the live TUI. The largest file here. |
| `mouse.rs` | `handle_mouse()` — click/drag/scroll routing into the shared `render::` routers |
| `render_impl.rs` | `build_screen_for_shell_content()`, window/separator/divider painting, tab drag overlay + tooltip, picker popup |
| `panels.rs` | Sidebar panel rendering (activity bar, explorer, git, debug, extensions, AI, search, terminal) |
| `mod.rs` | `run()`, module wiring, clipboard, key translation, cell helpers |
| `quadraui_tui.rs` | The few remaining `draw_*` wrappers not yet routed through a `Backend::draw_*` trait method |
| `backend.rs`, `events.rs`, `services.rs` | Re-export / placeholder shims — real implementations live in `quadraui::tui::*` (#268) |

> `draw_frame()` no longer exists — #766 deleted it as the last
> raw-`ratatui::Frame` path, along with its three test-only helpers.
> `build_screen_for_tui()` survives only under `#[cfg(test)]`.

### Win-GUI

**There is no `src/win_gui/` directory.** The Direct2D/Win32 backend was removed
from this repo on 2026-05-11 (`3e4bcff`). It will be re-added as a thin wrapper
once quadraui ships its Windows backend (quadraui#19–#31, quadraui#580). Open
`Win-GUI:` issues on *this* tracker describe the deleted backend and belong
upstream — see `PROJECT_STATE.md`, "Milestone hygiene".

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
