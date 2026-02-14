# VimCode Project State

Last updated: February 2026

## Overview

VimCode is a Vim-like code editor built in Rust with GTK4/Relm4. The goal is to create a VS Code-like editor with a first-class Vim mode that is cross-platform, fast, and does not require GPU acceleration.

## Current Status: Yank/Paste with Named Registers

The editor now supports yank and paste (`y`, `yy`, `Y`, `p`, `P`) with named registers (`"a`-`"z`), plus undo/redo and the full buffer/window/tab model.

### What Works Today

**Multiple Buffers**
- Buffers persist in memory until explicitly deleted
- `:bn` / `:bp` — Next/previous buffer
- `:b#` — Alternate buffer (like Vim's Ctrl-^)
- `:b <n>` — Switch to buffer by number
- `:b <name>` — Switch to buffer by partial filename match
- `:ls` / `:buffers` — List all open buffers with status flags
- `:bd` / `:bd!` — Delete buffer (with force option)

**Window Splits**
- `:split` / `:sp [file]` — Horizontal split
- `:vsplit` / `:vsp [file]` — Vertical split
- `:close` / `:clo` — Close current window
- `:only` / `:on` — Close all other windows
- `Ctrl-W s` — Horizontal split
- `Ctrl-W v` — Vertical split
- `Ctrl-W w` — Cycle to next window
- `Ctrl-W h/j/k/l` — Move to window in direction
- `Ctrl-W c` — Close window
- `Ctrl-W o` — Close other windows
- Per-window status bars when multiple windows visible
- Separator lines between windows

**Tabs**
- `:tabnew [file]` / `:tabe [file]` — New tab
- `:tabclose` / `:tabc` — Close current tab
- `:tabnext` / `:tabn` — Next tab
- `:tabprev` / `:tabp` — Previous tab
- `gt` — Next tab
- `gT` — Previous tab
- Tab bar shows when multiple tabs exist

**File Operations**
- Open files from CLI: `cargo run -- myfile.rs`
- New file creation (vim-style): non-existent paths start as empty buffers
- Save with `:w`
- Open different file with `:e filename`
- Quit with `:q` (blocked if dirty), `:q!` (force), `:wq` or `:x` (save+quit)
- Dirty indicator `[+]` in status bar and tab bar

**Four Modes**
- **Normal** — navigation and commands (block cursor)
- **Insert** — text input (line cursor)
- **Command** — `:` commands with command-line input
- **Search** — `/` search with command-line input

**Normal Mode Commands**
| Key | Action |
|-----|--------|
| `h` `j` `k` `l` | Character/line movement |
| `w` `b` `e` | Word motions (forward, backward, end) |
| `0` `$` | Line start/end |
| `gg` `G` | File start/end |
| `gt` `gT` | Next/previous tab |
| `i` `I` `a` `A` | Insert/append modes |
| `o` `O` | Open line below/above |
| `x` | Delete character |
| `dd` | Delete line |
| `D` | Delete to end of line |
| `u` | Undo |
| `Ctrl-r` | Redo |
| `n` `N` | Next/previous search match |
| `y` | Start yank (followed by `y` for line) |
| `yy` `Y` | Yank current line |
| `p` | Paste after cursor/below line |
| `P` | Paste before cursor/above line |
| `"x` | Select register `x` for next yank/delete/paste |
| `/` | Enter search mode |
| `:` | Enter command mode |
| `Ctrl-D` `Ctrl-U` | Half-page down/up |
| `Ctrl-F` `Ctrl-B` | Full-page down/up |
| `Ctrl-W` + key | Window commands |
| Arrow keys, Home, End | Navigation |

**Insert Mode**
- Full text input (all printable characters)
- Backspace (joins lines when at column 0)
- Delete, Tab (4 spaces), Return
- Arrow keys, Home, End navigation

**Command Mode (`:` commands)**
| Command | Action |
|---------|--------|
| `:w` | Save file |
| `:q` / `:q!` | Quit / force quit |
| `:wq` / `:x` | Save and quit |
| `:e <file>` | Open file |
| `:<number>` | Jump to line |
| `:bn` / `:bp` | Next/previous buffer |
| `:b#` / `:b <n>` | Alternate buffer / buffer by number |
| `:ls` / `:buffers` | List buffers |
| `:bd` / `:bd!` | Delete buffer |
| `:split` / `:vsplit` | Split window |
| `:close` / `:only` | Close window(s) |
| `:tabnew` / `:tabclose` | Tab management |
| `:tabnext` / `:tabprev` | Tab navigation |

**Search**
- `/` to enter search mode, type query, Enter to execute
- `n` / `N` to cycle through matches (wraps around)
- Status message: "match N of M" or "Pattern not found: xyz"

**UI**
- Tab bar (shown when multiple tabs)
- Multiple window rendering with split layouts
- Per-window status bars (shown when multiple windows)
- Window separator lines
- Global status line: `-- MODE -- filename [+]     Ln N, Col N  (M lines)`
- Command line: shows `:cmd` or `/query` during input, status messages otherwise
- Syntax highlighting for Rust (Tree-sitter)

**Yank/Paste/Registers**
- `yy` / `Y` — Yank current line (linewise)
- `p` — Paste after cursor (characterwise) or below line (linewise)
- `P` — Paste before cursor or above line
- `"x` prefix — Select named register (`a`-`z`) for next operation
- Delete operations (`x`, `dd`, `D`) also fill the register
- Unnamed register (`"`) always receives deleted/yanked text

**Test Suite**
- 88 passing tests covering all major functionality
- Clippy-clean, formatted with rustfmt

---

## File Structure

```
vimcode/
├── Cargo.toml              # Dependencies: gtk4, relm4, pangocairo, ropey, tree-sitter
├── README.md               # Project overview and roadmap
├── AGENTS.md               # AI agent instructions
├── PROJECT_STATE.md        # This file
└── src/
    ├── main.rs             # GTK4/Relm4 UI, window, input handling, rendering (~550 lines)
    └── core/               # Platform-agnostic editor logic
        ├── mod.rs          # Module declarations (~15 lines)
        ├── engine.rs       # Engine struct, orchestrates buffers/windows/tabs (~2200 lines)
        ├── buffer.rs       # Rope-based text storage, file I/O (~120 lines)
        ├── buffer_manager.rs # BufferManager: owns all buffers, tracks recent files (~360 lines)
        ├── cursor.rs       # Cursor position struct (~11 lines)
        ├── mode.rs         # Mode enum: Normal, Insert, Command, Search (~7 lines)
        ├── syntax.rs       # Tree-sitter parsing for highlights (~60 lines)
        ├── view.rs         # View: per-window cursor and scroll state (~70 lines)
        ├── window.rs       # Window, WindowLayout (split tree), WindowRect (~280 lines)
        └── tab.rs          # Tab: window layout collection (~70 lines)

Total: ~3,700 lines of Rust
```

### Architecture Rules

1. **`src/core/`** is strictly platform-agnostic — no GTK, Relm4, or rendering dependencies
2. **`src/main.rs`** handles all UI concerns — it calls into `core` and renders results
3. **`EngineAction`** enum allows core to signal UI actions (quit, save, open file) without platform dependencies
4. **Tests** live in `#[cfg(test)] mod tests` blocks at the bottom of each source file

### Key Data Model

```
Engine
├── BufferManager
│   └── HashMap<BufferId, BufferState>  # All open buffers
│       └── BufferState: buffer, file_path, dirty, syntax, highlights
├── windows: HashMap<WindowId, Window>  # All windows across all tabs
│   └── Window: buffer_id, view (cursor, scroll)
├── tabs: Vec<Tab>                      # Tab pages
│   └── Tab: WindowLayout (tree), active_window
└── Global state: mode, command_buffer, search, message
```

---

## Tech Stack

| Component | Library | Purpose |
|-----------|---------|---------|
| Language | Rust 2021 | Core language |
| UI Framework | GTK4 + Relm4 | Window, input, widget management |
| Rendering | Pango + Cairo | CPU-based text rendering |
| Text Storage | Ropey | Efficient rope data structure |
| Parsing | Tree-sitter | Syntax highlighting |

---

## Pending Roadmap

### High Priority (Core Vim Experience)

- [x] **Undo/redo** (`u`, `Ctrl-r`) — DONE
- [x] **Yank and paste** (`y`, `yy`, `Y`, `p`, `P`) with named registers — DONE
- [ ] **Visual mode** (character `v`, line `V`, block `Ctrl-V`)
- [ ] **More motions** (`ge`, `f`/`F`/`t`/`T` find char, `%` matching bracket)
- [ ] **More delete/change** (`dw`, `cw`, `c`, `C`, `s`, `S`)
- [ ] **Text objects** (`iw`, `aw`, `i"`, `a(`, etc.)
- [ ] **Repeat** (`.`) — repeat last change
- [ ] **Reverse search** (`?`)
- [ ] **Line numbers** (absolute and relative)

### Medium Priority (Editor Features)

- [x] **Multiple buffers / tabs** — DONE
- [x] **Registers** (named clipboards `"a`-`"z`) — DONE
- [ ] **Marks** (`m` to set, `'` to jump)
- [ ] **Macros** (`q` to record, `@` to play)
- [ ] **`:s` substitute** command
- [ ] **Incremental search** (highlight as you type)
- [ ] **Search highlighting** (highlight all matches in viewport)
- [ ] **File type detection** (auto-detect language for syntax)
- [ ] **Additional Tree-sitter grammars** (Python, JS/TS, Go, C/C++)

### VS Code Mode (Future)

- [ ] Keybinding mode switcher (Vim ↔ VS Code)
- [ ] Standard shortcuts (`Ctrl-C`, `Ctrl-V`, `Ctrl-Z`, `Ctrl-S`, etc.)
- [ ] Multi-cursor editing (`Ctrl-D`, `Alt-Click`)
- [ ] `Ctrl-P` quick file open (recent_files tracking already in place)
- [ ] `Ctrl-Shift-P` command palette

### UI Enhancements (Future)

- [ ] Minimap
- [ ] Side panel / file explorer
- [ ] Theme support (load color schemes)
- [ ] Configurable font/size
- [x] **Split panes** — DONE

### Performance (Future)

- [ ] Incremental syntax parsing (don't re-parse entire file)
- [ ] Large file handling (100K+ lines)
- [ ] Benchmarks

### Cross-Platform (Future)

- [ ] macOS testing
- [ ] Windows testing
- [ ] Platform-specific keybindings (Cmd vs Ctrl)

---

## Known Issues / Technical Debt

1. **Syntax re-parsing**: Currently re-parses the entire file on every buffer change. Should use Tree-sitter's incremental parsing.
2. **Hardcoded theme**: Colors are hardcoded in rendering functions. Should be configurable.
3. **Window direction navigation**: `Ctrl-W h/j/k/l` currently just cycles; should navigate by geometry.
4. **Search is basic**: No regex support, no incremental highlighting.

---

## Development Commands

```bash
cargo build              # Compile
cargo run -- <file>      # Run with a file
cargo test               # Run all 88 tests
cargo test <name>        # Run specific test
cargo clippy -- -D warnings   # Lint (must pass)
cargo fmt                # Format code
```

---

## Session History

### Session: Yank/Paste with Registers (Current)

Implemented Vim-style yank and paste with named registers:

1. **Data structures** (`engine.rs`):
   - `registers: HashMap<char, (String, bool)>` — stores content and linewise flag
   - `selected_register: Option<char>` — set by `"x` prefix

2. **Key bindings**:
   - `yy` / `Y` — Yank current line (linewise)
   - `p` — Paste after cursor (characterwise) or below line (linewise)
   - `P` — Paste before cursor or above line
   - `"x` — Select named register for next operation

3. **Vim-compatible behavior**:
   - Delete operations (`x`, `dd`, `D`) fill the register
   - Named register also copies to unnamed register (`"`)
   - Linewise content always ends with newline

4. **Tests**: 13 new tests (88 total), all passing
   - Yank: `yy`, `Y`, last line without newline
   - Paste: `p`/`P` linewise and characterwise
   - Delete fills register: `x`, `dd`, `D`
   - Named registers: yank to `"a`, paste from `"a`
   - Workflow: delete-and-paste, empty register handling

### Session: Undo/Redo

Implemented Vim-style undo/redo with operation-based tracking:

1. **Data structures** (`buffer_manager.rs`):
   - `EditOp` enum — Insert/Delete operations with position and text
   - `UndoEntry` — Group of operations + cursor position before edit
   - Added `undo_stack`, `redo_stack`, `current_undo_group` to `BufferState`

2. **Undo group lifecycle**:
   - Normal mode commands (x, dd, D) create single-op undo groups
   - Insert mode creates one undo group for entire session (i→typing→Escape)
   - `o`/`O` start a group that includes the newline + subsequent typing

3. **Key bindings**:
   - `u` — Undo (restores cursor position)
   - `Ctrl-r` — Redo
   - Status messages: "Already at oldest/newest change"

4. **Tests**: 10 new tests (75 total), all passing
   - Insert mode undo, x/dd/D undo, o undo
   - Redo after undo, redo cleared on new edit
   - Multiple undos, cursor position restoration

### Session: Multiple Buffers, Windows, and Tabs

Implemented full Vim buffer/window/tab model:

1. **New data structures**:
   - `BufferId`, `WindowId`, `TabId` — unique identifiers
   - `View` — per-window cursor and scroll state
   - `Window` — viewport into a buffer
   - `WindowLayout` — binary split tree for window arrangement
   - `Tab` — collection of windows with layout
   - `BufferManager` — owns all buffers, tracks alternate buffer and recent files

2. **Engine refactoring**:
   - Moved buffer/cursor/scroll from Engine fields to Window/View
   - Added facade methods for backward compatibility
   - BufferManager owns all BufferState instances

3. **Buffer commands**: `:bn`, `:bp`, `:b#`, `:b <n>`, `:ls`, `:bd`

4. **Window commands**: `:split`, `:vsplit`, `:close`, `:only`, `Ctrl-W` family

5. **Tab commands**: `:tabnew`, `:tabclose`, `:tabnext`, `:tabprev`, `gt`, `gT`

6. **UI rendering**:
   - Tab bar (conditional)
   - Multi-window layout with recursive rect calculation
   - Per-window status bars
   - Window separator lines

7. **Tests**: 26 new tests (65 total), all passing

### Session: Rudimentary Vim Experience

Implemented 8 tasks to bring VimCode from a demo to a usable editor:

1. **File I/O** — CLI args, `Buffer::from_file()`, `Engine::save()`, dirty flag
2. **Mode expansion** — Added Command and Search modes to the `Mode` enum
3. **Command execution** — `:w`, `:q`, `:wq`, `:q!`, `:e`, `:<number>`
4. **Search** — `/` search, `n`/`N` navigation, match counting
5. **Viewport scrolling** — `scroll_top`, `ensure_cursor_visible()`, Ctrl-D/U/F/B
6. **Status line UI** — Two-line bar with mode, filename, position, command input
7. **Vim commands** — `w`/`b`/`e`, `dd`/`D`, `A`/`I`, `gg`/`G`
8. **Tests** — 27 new tests (39 total), all passing

### Earlier Sessions

- Initial GTK4/Relm4 setup with DrawingArea
- Basic Normal/Insert mode switching
- `h`/`j`/`k`/`l` navigation with bounds checking
- Syntax highlighting with Tree-sitter
- Cursor rendering with Pango font metrics
- Fixed `#[track]` vs `#[watch]` redraw issue
- Fixed GTK key name handling for punctuation
