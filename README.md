# VimCode

A high-performance, cross-platform code editor built in Rust. VimCode aims to combine the power of Vim's modal editing with the usability and feature set of VS Code — without relying on GPU acceleration.

## Vision

VimCode's long-term goal is to be a full-featured code editor that:

- **Provides a first-class Vim mode** with accurate, deeply-integrated modal editing — not a bolted-on plugin.
- **Provides a VS Code mode** where keybindings and behavior match VS Code defaults, so users can switch seamlessly.
- **Runs cross-platform** on Linux, macOS, and Windows.
- **Stays fast** by using CPU-based rendering (Cairo/Pango), making it reliable in VMs, remote desktops, and environments without GPU access.
- **Maintains a clean architecture** with a strict separation between the editor engine (platform-agnostic core logic) and the UI layer.

## Current Status

VimCode now supports a functional Vim-like workflow with **multiple buffers, split windows, and tabs** — the core primitives for editing multiple files.

### What works today

- **Four modes** — Normal, Insert, Command (`:`) and Search (`/`)
- **Multiple buffers** — Open multiple files, switch with `:bn`/`:bp`/`:b#`/`:b <n>`
- **Split windows** — `:split`, `:vsplit`, `Ctrl-W` commands
- **Tabs** — `:tabnew`, `:tabclose`, `gt`/`gT` navigation
- **File I/O** — Open from CLI, `:w` save, `:e` open, `:q` quit with dirty-buffer protection
- **Navigation** — `h`/`j`/`k`/`l`, `w`/`b`/`e` words, `gg`/`G`, `0`/`$`, `Ctrl-D`/`Ctrl-U`
- **Editing** — `i`/`a`/`o`/`O`/`I`/`A` insert modes, `x`/`dd`/`D` delete
- **Search** — `/` forward search, `n`/`N` next/previous match
- **Syntax highlighting** — Tree-sitter for Rust
- **65 passing tests**, clippy-clean

### Key Commands

| Normal Mode | Action |
|-------------|--------|
| `h` `j` `k` `l` | Character/line movement |
| `w` `b` `e` | Word motions |
| `gg` `G` | File start/end |
| `0` `$` | Line start/end |
| `i` `I` `a` `A` `o` `O` | Enter insert mode |
| `x` `dd` `D` | Delete char/line/to-EOL |
| `n` `N` | Search next/prev |
| `gt` `gT` | Next/prev tab |
| `Ctrl-W s` | Horizontal split |
| `Ctrl-W v` | Vertical split |
| `Ctrl-W w` | Cycle windows |
| `Ctrl-W c` | Close window |
| `/` | Search |
| `:` | Command mode |

| Command | Action |
|---------|--------|
| `:w` | Save |
| `:q` `:q!` | Quit / force quit |
| `:e <file>` | Open file |
| `:bn` `:bp` `:b#` | Buffer navigation |
| `:ls` | List buffers |
| `:bd` | Delete buffer |
| `:split` `:vsplit` | Split window |
| `:tabnew` `:tabclose` | Tab management |

## Roadmap

### High Priority (Core Vim)
- [ ] Undo/redo (`u`, `Ctrl-r`)
- [ ] Yank and paste (`y`, `yy`, `p`, `P`)
- [ ] Visual mode (`v`, `V`, `Ctrl-V`)
- [ ] More motions (`f`/`F`/`t`/`T`, `%`)
- [ ] Change commands (`c`, `cw`, `C`)
- [ ] Text objects (`iw`, `aw`, `i"`, `a(`)
- [ ] Repeat (`.`)
- [ ] Line numbers

### Medium Priority
- [x] Multiple buffers / tabs ✓
- [x] Split windows ✓
- [ ] Registers
- [ ] Marks (`m`, `'`)
- [ ] Macros (`q`, `@`)
- [ ] `:s` substitute
- [ ] Search highlighting
- [ ] More Tree-sitter grammars

### Future
- [ ] VS Code keybinding mode
- [ ] Multi-cursor editing
- [ ] `Ctrl-P` file finder
- [ ] Command palette
- [ ] LSP integration
- [ ] File explorer
- [ ] Themes

## Architecture

```
src/
├── main.rs                 # GTK4/Relm4 UI, rendering (~550 lines)
└── core/                   # Platform-agnostic logic (~3,150 lines)
    ├── engine.rs           # Orchestrates buffers, windows, tabs, commands
    ├── buffer.rs           # Rope-based text storage
    ├── buffer_manager.rs   # Manages all open buffers
    ├── view.rs             # Per-window cursor and scroll state
    ├── window.rs           # Window layout (binary split tree)
    ├── tab.rs              # Tab pages
    ├── cursor.rs           # Cursor position
    ├── mode.rs             # Mode enum
    └── syntax.rs           # Tree-sitter highlighting
```

**Key design rule:** Everything in `src/core/` is platform-agnostic — no GTK, Relm4, or rendering dependencies. This keeps the editor logic independently testable.

## Tech Stack

| Component | Library |
|-----------|---------|
| Language | Rust 2021 |
| UI | GTK4 + Relm4 |
| Text Engine | Ropey |
| Parsing | Tree-sitter |
| Rendering | Pango + Cairo (CPU-based) |

## Building

### Prerequisites

- Rust toolchain (stable)
- GTK4 development libraries

```bash
# Debian/Ubuntu
sudo apt install libgtk-4-dev libpango1.0-dev

# Fedora
sudo dnf install gtk4-devel pango-devel

# Arch
sudo pacman -S gtk4 pango
```

### Build and Run

```bash
cargo build              # Compile
cargo run -- <file>      # Run with a file
cargo test               # Run 65 tests
cargo clippy -- -D warnings   # Lint
cargo fmt                # Format
```

## License

TBD
