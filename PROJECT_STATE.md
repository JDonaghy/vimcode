# VimCode Project State

**Last updated:** Feb 16, 2026

## Status

**Incremental Search COMPLETE:** Real-time search as you type (324 tests passing)

### Core Vim (Complete)
- Seven modes (Normal/Insert/Visual/Visual Line/Visual Block/Command/Search)
- Navigation (hjkl, w/b/e, {}, gg/G, f/F/t/T, %, 0/$, Ctrl-D/U/F/B)
- Operators (d/c/y with motions, x/dd/D/s/S/C, r for replace char)
- Text objects (iw/aw, quotes, brackets)
- Registers (unnamed + a-z)
- Undo/redo (u/Ctrl-R), undo line (U), repeat (.), count prefix
- Visual modes (v/V/Ctrl-V with y/d/c/u/U, rectangular block selections, case change)
- **Search:**
  - Forward search: `/` + pattern, `n` for next, `N` for previous
  - Reverse search: `?` + pattern, `n` for previous, `N` for next
  - **Incremental search:** Real-time updates as you type, Escape to cancel
  - Direction-aware navigation (n/N respect last search direction)
- **Marks:**
  - Set marks: `m{a-z}` for file-local marks
  - Jump to line: `'{a-z}` jumps to mark line
  - Jump to position: `` `{a-z}`` jumps to exact mark position
  - Marks stored per buffer
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
- **Session Persistence:**
  - Command history (Up/Down arrows, max 100, persisted)
  - Search history (Up/Down arrows, max 100, persisted)
  - Tab auto-completion in command mode
  - Window geometry persistence (size restored on startup)
  - Explorer visibility state (persisted across sessions)
  - Session state at `~/.config/vimcode/session.json`
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
- **Line numbers (FIXED):** Absolute/relative/hybrid modes now render correctly with proper visibility
- Tab bar, single global status line, command line
- Mouse click positioning (pixel-perfect)
- **Scrollbars (FIXED):** Per-window vertical/horizontal scrollbars with cursor indicators
- **Font configuration:** Customizable font family and size

### Settings
- `~/.config/vimcode/settings.json` (auto-created with defaults on first run)
- LineNumberMode (None/Absolute/Relative/Hybrid)
- Font family and size (hot-reload on save)
- Explorer visibility on startup (default: hidden)
- Incremental search (default: enabled, set to false to disable)
- `:config reload` runtime refresh
- File watcher for automatic reload

## File Structure
```
vimcode/
├── src/
│   ├── main.rs (~2400 lines) — GTK4/Relm4 UI, rendering, find dialog
│   └── core/ (~10,100 lines) — Platform-agnostic logic
│       ├── engine.rs (~10,100 lines) — Orchestrates everything, find/replace, macros, history
│       ├── buffer_manager.rs (~600 lines) — Buffer lifecycle
│       ├── buffer.rs (~120 lines) — Rope-based storage
│       ├── session.rs (~170 lines) — Session state persistence (NEW)
│       ├── settings.rs (~190 lines) — JSON persistence, auto-init
│       ├── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
│       └── Tests: 324 passing (9 find/replace, 14 macro, 5 session, 4 reverse search, 7 replace char, 5 undo line, 8 case change, 6 marks, 5 incremental search tests)
└── Total: ~12,200 lines
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
cargo test    # 324 tests
cargo clippy -- -D warnings
cargo fmt
```

## Roadmap (High Priority)
- [x] **Visual block mode (Ctrl-V)** — COMPLETE
- [x] **Macros (q, @)** — COMPLETE
- [x] **Find/Replace (:s + Ctrl-F)** — COMPLETE
- [x] **Session Persistence** — COMPLETE
- [x] **Reverse search (?)** — COMPLETE
- [x] **Replace character (r)** — COMPLETE
- [x] **Undo line (U)** — COMPLETE
- [x] **Visual mode case change (u/U)** — COMPLETE
- [x] **Marks (m, ')** — COMPLETE
- [x] **Incremental search** — COMPLETE
- [ ] More grammars (Python/JS/Go/C++)

## Recent Work
**Session 25:** Marks + Incremental Search + Visual Mode Case Change complete — `m{a-z}` to set marks, `'` and `` ` `` to jump to marks; real-time incremental search as you type with Escape to cancel; `u`/`U` commands in visual mode for case transformation (313→324 tests, 6 marks + 5 incremental search + 8 case change tests).
**Session 25 (earlier):** Visual Mode Case Change complete — `u`/`U` commands in visual mode (character, line, and block) for lowercase/uppercase transformation with proper undo/redo support (305→313 tests, 8 case change tests).
**Session 24:** Reverse Search + Replace Character + Undo Line complete — `?` command for backward search with direction-aware `n`/`N` navigation; `r` command to replace character(s) with count/repeat support; `U` command to restore current line to original state (284→300 tests, 4 reverse search + 7 replace char + 5 undo line tests).
**Session 23:** Session Persistence complete — CRITICAL line numbers bug fixed (Absolute mode now visible), command/search history with Up/Down arrows (max 100, persisted), Tab auto-completion, window geometry persistence, explorer visibility state (279→284 tests, 5 session tests). Session state at ~/.config/vimcode/session.json.
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
