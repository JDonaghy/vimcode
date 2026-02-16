# VimCode

High-performance Vim+VSCode hybrid editor in Rust. Modal editing meets modern UX, no GPU required.

## Vision

- **First-class Vim mode** — deeply integrated, not a plugin
- **VS Code mode** — matching keybindings/behavior (future)
- **Cross-platform** — Linux, macOS, Windows
- **CPU rendering** — Cairo/Pango (works in VMs, remote desktops)
- **Clean architecture** — platform-agnostic core

## Status

**Session Persistence complete** — 284 tests passing, production-ready

### Working Features

**Vim Core:**
- 7 modes (Normal/Insert/Visual/Visual Line/Visual Block/Command/Search)
- Navigation (hjkl, w/b/e, {}, gg/G, f/F/t/T, %, 0/$, Ctrl-D/U/F/B)
- Operators (d/c/y + motions, x/dd/D/s/S/C)
- Text objects (iw/aw, quotes, brackets)
- Registers (unnamed + a-z), undo/redo, repeat (.)
- Count prefix (5j, 3dd, 10yy)
- Visual modes (v/V/Ctrl-V with y/d/c, rectangular block selections)
- Search (/, n/N)
- **Macros:** Record (q), playback (@), repeat (@@), count prefix (5@a)
- **Find/Replace:** Vim :s command + VSCode Ctrl-F dialog with undo support

**Multi-file:**
- Buffers (:bn/:bp/:b#/:ls/:bd)
- Windows (:split/:vsplit, Ctrl-W commands)
- Tabs (:tabnew/:tabclose, gt/gT)

**File Explorer (VSCode-style):**
- Sidebar (Ctrl-B toggle, Ctrl-Shift-E focus)
- Tree view, CRUD operations
- **Preview mode:**
  - Single-click → preview (italic/dimmed, replaceable)
  - Double-click → permanent
  - Edit/save → auto-promote
  - `:ls` shows [Preview]

**UI:**
- Syntax highlighting (Tree-sitter, Rust)
- Line numbers (absolute/relative/hybrid)
- Tab bar, status lines, mouse click

**Settings:** `~/.config/vimcode/settings.json`, `:config reload`

**Session Persistence:**
- Command/search history (Up/Down arrows, Tab auto-complete)
- Window geometry persistence
- Explorer visibility state
- Session state at `~/.config/vimcode/session.json`

### Key Commands

| Normal | Action | Visual | Action |
|--------|--------|--------|--------|
| `hjkl` | Move | `hjkl/w/b/e` | Extend selection |
| `w/b/e` | Word motions | `y/d/c` | Yank/delete/change |
| `{}/gg/G` | Paragraph/file | `v/V/Esc` | Switch mode/exit |
| `0/$` | Line start/end | | |
| `v/V` | Visual mode | **Command** | **Action** |
| `i/I/a/A/o/O` | Insert | `:w/:q/:q!` | Save/quit |
| `x/dd/D` | Delete | `:e <file>` | Open |
| `yy/Y` | Yank | `:bn/:bp/:b#` | Buffer nav |
| `p/P` | Paste | `:ls/:bd` | List/delete buffer |
| `"x` | Register | `:split/:vsplit` | Split window |
| `u/Ctrl-r` | Undo/redo | `:tabnew/:tabclose` | Tab mgmt |
| `n/N` | Search next/prev | | |
| `gt/gT` | Tab next/prev | **UI** | **Action** |
| `Ctrl-W s/v/w/c` | Split/cycle/close | `Ctrl-B` | Toggle sidebar |
| `/` | Search | `Ctrl-Shift-E` | Focus explorer |
| `:` | Command | `Esc` (explorer) | Focus editor |

## Roadmap

**Completed:**
- [x] Visual block (Ctrl-V)
- [x] Macros (q, @)
- [x] :s substitute + Ctrl-F find/replace
- [x] Session persistence (history, window geometry)

**High Priority:**
- [ ] Reverse search (?)
- [ ] Marks (m, ')
- [ ] Incremental search

**Future:**
- [ ] VS Code keybinding mode
- [ ] Multi-cursor
- [ ] Ctrl-P file finder
- [ ] LSP integration
- [ ] Themes

## Architecture

```
src/
├── main.rs          # GTK4/Relm4 UI (~2400 lines)
└── core/            # Platform-agnostic (~10,100 lines)
    ├── engine.rs    # Orchestrator (~10,100 lines)
    ├── buffer_manager.rs, buffer.rs, settings.rs, session.rs
    └── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
```

**Design rule:** `src/core/` has zero GTK/rendering deps (independently testable).

## Tech Stack

| Component | Library |
|-----------|---------|
| Language | Rust 2021 |
| UI | GTK4 + Relm4 |
| Rendering | Pango + Cairo (CPU) |
| Text | Ropey |
| Parsing | Tree-sitter |

## Building

**Prerequisites:**
```bash
# Debian/Ubuntu
sudo apt install libgtk-4-dev libpango1.0-dev

# Fedora
sudo dnf install gtk4-devel pango-devel

# Arch
sudo pacman -S gtk4 pango
```

**Build:**
```bash
cargo build
cargo run -- <file>
cargo test              # 284 tests
cargo clippy -- -D warnings
cargo fmt
```

## License

TBD
