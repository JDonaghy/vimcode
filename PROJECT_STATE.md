# VimCode Project State

**Last updated:** Feb 2026

## Status

**Phase 4 COMPLETE:** Preview mode for file explorer (242 tests passing)

### Core Vim (Complete)
- Six modes (Normal/Insert/Visual/Visual Line/Command/Search)
- Navigation (hjkl, w/b/e, {}, gg/G, f/F/t/T, %, 0/$, Ctrl-D/U/F/B)
- Operators (d/c/y with motions, x/dd/D/s/S/C)
- Text objects (iw/aw, quotes, brackets)
- Registers (unnamed + a-z)
- Undo/redo, repeat (.), count prefix
- Visual mode (v/V with y/d/c)
- Search (/, n/N)
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
- Tab bar, status lines, command line
- Mouse click positioning (pixel-perfect)

### Settings
- `~/.config/vimcode/settings.json`
- LineNumberMode (None/Absolute/Relative/Hybrid)
- `:config reload` runtime refresh

## File Structure
```
vimcode/
├── src/
│   ├── main.rs (~1100 lines) — GTK4/Relm4 UI, rendering
│   └── core/ (~8200 lines) — Platform-agnostic logic
│       ├── engine.rs (~8000 lines) — Orchestrates everything
│       ├── buffer_manager.rs (~600 lines) — Buffer lifecycle
│       ├── buffer.rs (~120 lines) — Rope-based storage
│       ├── settings.rs (~160 lines) — JSON persistence
│       ├── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
│       └── Tests: 242 passing
└── Total: ~9,800 lines
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
cargo test    # 242 tests
cargo clippy -- -D warnings
cargo fmt
```

## Roadmap (High Priority)
- [ ] Visual block mode (Ctrl-V)
- [ ] Reverse search (?)
- [ ] Marks (m, ')
- [ ] Macros (q, @)
- [ ] :s substitute
- [ ] Incremental search
- [ ] More grammars (Python/JS/Go/C++)

## Recent Work
**Session 18:** Phase 4 complete — Preview mode (242 tests).
**Session 17:** Phase 3 complete — Focus, highlighting, errors (232 tests).
**Session 16:** Phase 2 complete — File tree + CRUD (232 tests).
**Session 15:** Phase 1 complete — Activity bar + sidebar (232 tests).
**Session 12:** High-priority motions (154→214 tests).
**Session 11:** Line numbers & config (146→154 tests).
**Session 10:** Count repetition (115→146 tests).
