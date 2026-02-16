# VimCode Project State

**Last updated:** Feb 16, 2026

## Status

**Find/Replace COMPLETE:** Vim :s command + VSCode Ctrl-F dialog with undo support (279 tests passing)

### Core Vim (Complete)
- Seven modes (Normal/Insert/Visual/Visual Line/Visual Block/Command/Search)
- Navigation (hjkl, w/b/e, {}, gg/G, f/F/t/T, %, 0/$, Ctrl-D/U/F/B)
- Operators (d/c/y with motions, x/dd/D/s/S/C)
- Text objects (iw/aw, quotes, brackets)
- Registers (unnamed + a-z)
- Undo/redo, repeat (.), count prefix
- Visual modes (v/V/Ctrl-V with y/d/c, rectangular block selections)
- Search (/, n/N)
- **Find/Replace:**
  - Vim :s command (`:s/pattern/replacement/[flags]`)
  - Ranges: current line, :%s (all lines), :'<,'> (visual selection)
  - Flags: g (global), i (case-insensitive)
  - VSCode-style Ctrl-F dialog (live search, replace, replace all)
  - Proper undo/redo support
- **Macros:**
  - Record: q<register>, stop with q
  - Playback: @<register>, @@ to repeat, count prefix (5@a)
  - Captures ALL keys: navigation, arrows, Ctrl keys, special keys, insert mode, search
  - Vim-style encoding: `<Left>`, `<C-D>`, `<CR>`, etc.
  - Future-proof: automatically captures any new features
- Buffers (:bn/:bp/:b#/:ls/:bd)
- Windows (:split/:vsplit, Ctrl-W)
- Tabs (:tabnew/:tabclose, gt/gT)

### File Explorer (Complete)
- VSCode-style sidebar (Ctrl-B toggle, Ctrl-Shift-E focus)
- Tree view with icons, expand/collapse folders
- CRUD operations (create/delete files/folders)
- **Preview mode (NEW):**
  - Single-click → preview (italic/dimmed tab, replaceable)
  - Double-click → permanent
  - Edit/save → auto-promote
  - `:ls` shows [Preview] suffix
- Active file highlighting, auto-expand parents
- Error handling, name validation

### Rendering
- Syntax highlighting (Tree-sitter, Rust)
- Line numbers (absolute/relative/hybrid via settings.json)
- Tab bar, single global status line, command line
- Mouse click positioning (pixel-perfect)
- **Scrollbars (FIXED):** Per-window vertical/horizontal scrollbars with cursor indicators
- **Font configuration:** Customizable font family and size

### Settings
- `~/.config/vimcode/settings.json` (auto-created with defaults on first run)
- LineNumberMode (None/Absolute/Relative/Hybrid)
- Font family and size (hot-reload on save)
- `:config reload` runtime refresh
- File watcher for automatic reload

## File Structure
```
vimcode/
├── src/
│   ├── main.rs (~2300 lines) — GTK4/Relm4 UI, rendering, find dialog
│   └── core/ (~9700 lines) — Platform-agnostic logic
│       ├── engine.rs (~9700 lines) — Orchestrates everything, find/replace, macros
│       ├── buffer_manager.rs (~600 lines) — Buffer lifecycle
│       ├── buffer.rs (~120 lines) — Rope-based storage
│       ├── settings.rs (~180 lines) — JSON persistence, auto-init
│       ├── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
│       └── Tests: 279 passing (9 find/replace tests, 14 macro tests)
└── Total: ~11,700 lines
```

## Architecture
- **`src/core/`:** No GTK/Relm4/rendering deps (testable in isolation)
- **`src/main.rs`:** All UI/rendering
- **EngineAction:** Core signals UI actions without platform coupling

## Tech Stack
| Component | Library |
|-----------|---------|
| Language | Rust 2021 |
| UI | GTK4 + Relm4 |
| Rendering | Pango + Cairo (CPU) |
| Text | Ropey |
| Parsing | Tree-sitter |
| Config | serde + serde_json |

## Commands
```bash
cargo build
cargo run -- <file>
cargo test    # 279 tests
cargo clippy -- -D warnings
cargo fmt
```

## Roadmap (High Priority)
- [x] **Visual block mode (Ctrl-V)** — COMPLETE
- [x] **Macros (q, @)** — COMPLETE
- [x] **Find/Replace (:s + Ctrl-F)** — COMPLETE
- [ ] Reverse search (?)
- [ ] Marks (m, ')
- [ ] Incremental search
- [ ] More grammars (Python/JS/Go/C++)

## Recent Work
**Session 22:** Find/Replace complete — Vim :s command (current line, %s all lines, '<,'> visual selection with g/i flags), VSCode Ctrl-F dialog (live search, replace, replace all), proper undo/redo with insert_with_undo (269→279 tests, 9 find/replace tests).
**Session 21:** Macros (q, @) complete — Full keystroke recording (navigation, Ctrl keys, special keys, arrows), Vim-style encoding, playback with count prefix, @@ repeat, recursion protection (256→269 tests, 14 macro tests).
**Session 20:** Critical bug fixes — Scrollbars visible, explorer button working, settings auto-init/reload, single status line (256 tests).
**Session 19:** Visual block mode (Ctrl-V) complete — rectangular selections (242→255 tests).
**Session 18:** Phase 4 complete — Preview mode (242 tests).
**Session 17:** Phase 3 complete — Focus, highlighting, errors (232 tests).
**Session 16:** Phase 2 complete — File tree + CRUD (232 tests).
**Session 15:** Phase 1 complete — Activity bar + sidebar (232 tests).
**Session 12:** High-priority motions (154→214 tests).
**Session 11:** Line numbers & config (146→154 tests).
