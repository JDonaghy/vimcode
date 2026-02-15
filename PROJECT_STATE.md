# VimCode Project State

Last updated: February 2026

## Overview

VimCode is a Vim-like code editor built in Rust with GTK4/Relm4. The goal is to create a VS Code-like editor with a first-class Vim mode that is cross-platform, fast, and does not require GPU acceleration.

## Current Status: Phase 3 COMPLETE - Integration & Polish

**Phase 1 COMPLETE:** Activity bar + collapsible sidebar with VSCode theme (232 tests)  
**Phase 2 COMPLETE:** File explorer tree view with full CRUD operations (239 tests)  
**Phase 3 COMPLETE:** Integration & Polish - Keybindings, focus management, file highlighting, error handling (232 tests passing)  
**Next:** Advanced features or other priorities (search in files, Git integration, etc.)

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

**Six Modes**
- **Normal** — navigation and commands (block cursor)
- **Insert** — text input (line cursor)
- **Visual** — character-wise visual selection (block cursor)
- **Visual Line** — line-wise visual selection (block cursor)
- **Command** — `:` commands with command-line input
- **Search** — `/` search with command-line input

**Normal Mode Commands**
| Key | Action |
|-----|--------|
| `h` `j` `k` `l` | Character/line movement |
| `w` `b` `e` `ge` | Word motions (forward, backward, end, backward-end) |
| `{` `}` | Paragraph motions (previous/next empty line) |
| `f` `F` `t` `T` | Character find (forward/backward, inclusive/till) |
| `;` `,` | Repeat find (same/opposite direction) |
| `%` | Jump to matching bracket (`, {}, []) |
| `0` `$` | Line start/end |
| `gg` `G` | File start/end |
| `gt` `gT` | Next/previous tab |
| `i` `I` `a` `A` | Insert/append modes |
| `o` `O` | Open line below/above |
| `x` | Delete character |
| `dd` | Delete line |
| `D` | Delete to end of line |
| `dw` `db` `de` | Delete word motions |
| `cw` `cb` `ce` `cc` | Change word/line |
| `s` `S` `C` | Substitute char/line, change to EOL |
| `u` | Undo |
| `Ctrl-r` | Redo |
| `n` `N` | Next/previous search match |
| `y` | Start yank (followed by `y` for line) |
| `yy` `Y` | Yank current line |
| `p` | Paste after cursor/below line |
| `P` | Paste before cursor/above line |
| `"x` | Select register `x` for next yank/delete/paste |
| `v` | Enter character visual mode |
| `V` | Enter line visual mode |
| `.` | Repeat last change |
| `/` | Enter search mode |
| `:` | Enter command mode |
| `Ctrl-D` `Ctrl-U` | Half-page down/up |
| `Ctrl-F` `Ctrl-B` | Full-page down/up |
| `Ctrl-W` + key | Window commands |
| Arrow keys, Home, End | Navigation |

**Visual Mode (Character and Line)**
- Enter with `v` (character) or `V` (line)
- Navigation keys extend selection (h/j/k/l, w/b/e, 0/$, gg/G, {/}, etc.)
- `y` — Yank selection to register
- `d` — Delete selection (with undo)
- `c` — Change selection (delete and enter insert mode)
- `v` — Switch to character mode or exit
- `V` — Switch to line mode or exit
- `Escape` — Return to normal mode
- `"x` — Named registers work with visual operators
- Semi-transparent blue highlight shows selection

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
| `:config reload` | Reload settings from settings.json |

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

**File Explorer (NEW - Complete)**
- Activity bar with file explorer button (📁)
- Collapsible sidebar (Ctrl-B to toggle)
- VSCode-style file tree with icons (📁 folders, 📄 files)
- Double-click to open files
- Click folders to expand/collapse
- Toolbar with file operations:
  - ➕ New file (timestamp-based naming)
  - 📁➕ New folder (timestamp-based naming)
  - 🗑️ Delete selected file/folder
  - 🔄 Refresh tree
- **Ctrl-Shift-E:** Focus file explorer
- **Escape:** Return focus from explorer to editor
- **Auto-focus:** Opening files automatically switches focus to editor
- Active file highlighted in tree with blue selection
- Auto-expand parent folders when highlighting files
- TreeView search disabled (no popup interference)
- Comprehensive error handling with user-friendly messages
- File/folder name validation (no slashes, null chars, reserved names)

**Yank/Paste/Registers**
- `yy` / `Y` — Yank current line (linewise)
- `p` — Paste after cursor (characterwise) or below line (linewise)
- `P` — Paste before cursor or above line
- `"x` prefix — Select named register (`a`-`z`) for next operation
- Delete operations (`x`, `dd`, `D`) also fill the register
- Unnamed register (`"`) always receives deleted/yanked text
- Visual mode yank/delete operations work with registers

**Count-Based Repetition (NEW - Complete)**
- All motion commands: `5j`, `10k`, `3w`, `2b`, `2{`, `3}`, etc.
- Line operations: `3dd`, `5yy`, `10x`, `2D`
- Special commands: `42G`, `2gg`, `3p`, `5n`, `3o`
- Visual mode: `v5j`, `V3k`, `3w` in visual mode
- Digit accumulation: Type "123" → accumulates to 123
- Smart zero handling: `0` alone → column 0, `10j` → count of 10
- 10,000 limit with user-friendly message
- Vim-style right-aligned display in command line
- Count preserved when entering visual mode
- Helper methods: `take_count()` and `peek_count()`

**Settings & Line Numbers (NEW - Complete)**
- Settings struct with LineNumberMode enum (None, Absolute, Relative, Hybrid)
- Load from `~/.config/vimcode/settings.json`, JSON with serde
- Gutter rendering: absolute/relative/hybrid modes, dynamic width
- Current line highlighted yellow (0.9, 0.9, 0.5), others gray (0.5, 0.5, 0.5)
- Per-window rendering with multi-window support
- `:config reload` command to refresh settings at runtime
- Error handling: preserves settings on parse errors, shows descriptive messages

**Character Find Motions (Complete)**
- `f<char>`, `F<char>`, `t<char>`, `T<char>` — Find/till char forward/backward
- `;`, `,` — Repeat find same/opposite direction
- Count support, within-line only

**Delete/Change Operators (Complete)**
- `dw`, `db`, `de`, `cw`, `cb`, `ce`, `cc`, `s`, `S`, `C` with count & register support

**Additional Motions (Complete)**
- `ge` — Backward to end of word (with count support)
- `%` — Jump to matching bracket ((), {}, [])
- Works with operators: `d%`, `c%`, `y%`
- Nested bracket support

**Text Objects (Complete)**
- `iw`/`aw` — inner/around word
- `i"`/`a"`, `i'`/`a'` — inner/around quotes
- `i(`/`a(`, `i{`/`a{`, `i[`/`a[` — inner/around brackets
- Works with operators: `diw`, `ciw`, `yiw`, `da"`, `ci(`, etc.
- Visual mode support: `viw`, `va"`, etc.
- Nested bracket/quote support

**Repeat Command (NEW - Complete)**
- `.` — Repeat last change operation
- Supports insert operations (`i`, `a`, `o`, etc.)
- Supports delete operations (`x`, `dd`)
- Count prefix: `3.` repeats 3 times
- Basic implementation (some edge cases deferred)

**Mouse Click**
- Pixel-perfect positioning using Pango layout measurement
- Real window dimensions and font metrics
- Tab and unicode support
- 18 comprehensive tests covering edge cases

**Test Suite**
- 232 passing tests (18 mouse tests, all core features tested)
- Clippy-clean (with TreeView deprecation warnings allowed)

---

## File Structure

```
vimcode/
├── Cargo.toml              # Dependencies: gtk4, relm4, pangocairo, ropey, tree-sitter, serde
├── README.md               # Project overview and roadmap
├── AGENTS.md               # AI agent instructions
├── PROJECT_STATE.md        # This file
├── PLAN.md                 # Current feature implementation plan
├── PLAN_ARCHIVE_count_repetition.md  # Archived: Count-based repetition (complete)
├── PLAN_ARCHIVE_line_numbers_settings.md  # Archived: Line numbers & settings (complete)
    └── src/
        ├── main.rs             # GTK4/Relm4 UI, window, input, rendering, line numbers (~850 lines)
        └── core/               # Platform-agnostic editor logic
            ├── mod.rs          # Module declarations (~17 lines)
            ├── engine.rs       # Engine struct, orchestrates buffers/windows/tabs (~7490 lines)
        ├── buffer.rs       # Rope-based text storage, file I/O (~120 lines)
        ├── buffer_manager.rs # BufferManager: owns all buffers (~360 lines)
        ├── cursor.rs       # Cursor position struct (~12 lines)
        ├── mode.rs         # Mode enum (~10 lines)
        ├── settings.rs     # Settings struct, JSON I/O (~160 lines)
        ├── syntax.rs       # Tree-sitter parsing (~60 lines)
        ├── view.rs         # View: per-window cursor/scroll (~70 lines)
        ├── window.rs       # Window, WindowLayout, WindowRect (~280 lines)
        └── tab.rs          # Tab: window layout collection (~70 lines)

Total: ~9,800 lines of Rust
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
├── settings: Settings                  # Editor settings
│   └── line_numbers: LineNumberMode    # None, Absolute, Relative, Hybrid
├── last_find: Option<(char, char)>     # Last character find (motion_type, target)
├── pending_operator: Option<char>      # Operator awaiting motion (d, c)
├── last_change: Option<Change>        # Last change for repeat (.)
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
| Serialization | serde + serde_json | Settings persistence |

---

## Pending Roadmap

### High Priority (Core Vim Experience)

- [x] **Undo/redo** (`u`, `Ctrl-r`) — DONE
- [x] **Yank and paste** (`y`, `yy`, `Y`, `p`, `P`) with named registers — DONE
- [x] **Paragraph navigation** (`{`, `}`) — DONE
- [x] **Visual mode** (character `v`, line `V`) — DONE
- [x] **Count-based repetition** (`5j`, `3dd`, `10yy`) — DONE
  - All motion commands, line operations, special commands, and visual mode support count
- [x] **Character find motions** (`f`/`F`/`t`/`T`, `;`, `,`) — DONE
- [x] **More delete/change** (`dw`, `cw`, `c`, `C`, `s`, `S`) — DONE
- [x] **More motions** (`ge`, `%` matching bracket) — DONE
- [x] **Text objects** (`iw`, `aw`, `i"`, `a(`, etc.) — DONE
- [x] **Repeat** (`.`) — DONE (basic implementation)
- [ ] **Visual block mode** (`Ctrl-V` for rectangular selections)
- [ ] **Reverse search** (`?`)
- [x] **Line numbers** (absolute and relative) — DONE
  - All modes implemented: None, Absolute, Relative, Hybrid
  - Controlled by settings.json configuration file
  - Optional: `:set number` and `:set relativenumber` commands (deferred)

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
cargo test               # Run all 232 tests
cargo test <name>        # Run specific test
cargo clippy -- -D warnings   # Lint (must pass)
cargo fmt                # Format code
```

---

## Recent Development Summary

*For detailed session logs, see HISTORY.md*

**Session 17:** Phase 3 COMPLETE (3A-3D) - Integration & Polish (232 tests passing).
  - **3A:** Ctrl-Shift-E keybinding to focus explorer
  - **3B:** Focus management with Escape key to return to editor
  - **3C:** Active file highlighting in tree with auto-expand parents
  - **3D:** Comprehensive error handling with validate_name() and detailed error messages
  - **Focus fixes:** Disabled TreeView search, auto-focus editor on file open, proper navigation keys
  - Technical: Used Rc<RefCell<>> pattern for widget references in Relm4
  - Added #![allow(deprecated)] for TreeView/TreeStore (functional, ListView migration deferred)

**Session 16:** Phase 2A-E complete - Tree display + file opening + expandable folders + toolbar UI (232 tests). 
  - VSCode-style CSS polish: subtle selection with left accent, refined hover, better spacing
  - Fixed: Single column for icon+name (proper indentation), level_indentation=0 (tight spacing)

**Session 15:** Phase 1 COMPLETE (1A-1E) - Activity bar, collapsible sidebar, buttons, active indicator, VSCode CSS theme (232 tests).

**Session 14:** Phase 1A complete - Activity bar and collapsible sidebar layout structure (232 tests).

**Session 13:** Phase 0.5A/B/C complete - Mouse click uses real dimensions, font metrics, pixel-perfect column detection (222 tests).

**Session 12:** High-priority Vim motions complete (5 steps, 154→214 tests). Remaining: Visual block mode, reverse search.

**Session 11:** Line numbers & config reload (146→154 tests). Remaining: `:set` commands.

**Session 10:** Count-based repetition (115→146 tests). All motions, ops, and visual mode support counts.

**Session 9:** Visual mode (98→115 tests). Character (`v`) and line (`V`) modes complete.

**Session 8:** Paragraph navigation `{`/`}` (88→98 tests).

**Session 7:** Yank/paste with registers (75→88 tests).

**Session 6:** Undo/redo (65→75 tests).

**Session 5:** Buffers/windows/tabs (39→65 tests). Multi-buffer, split panes, tab bar complete.

**Session 4:** Rudimentary Vim experience (12→39 tests). File I/O, command/search modes.

**Sessions 1-3:** GTK4/Relm4 setup, Normal/Insert modes, navigation, Tree-sitter, rendering.
