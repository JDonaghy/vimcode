# VimCode Implementation Plan

## Recently Completed

### ✅ Multi-Language Syntax Highlighting (Session 26)
- Added Python, JavaScript, Go, and C++ syntax highlighting via Tree-sitter
- Language auto-detected from file extension (.py, .js/.jsx/.mjs/.cjs, .go, .cpp/.cc/.cxx/.h/.hpp etc)
- New `SyntaxLanguage` enum with `from_path()` for extension detection
- New `Syntax::new_from_path()` for automatic language selection
- Buffers opened with `BufferState::with_file()` now auto-detect language
- Files with unknown extension fall back to Rust highlighting
- Tests: 324 → 336 passing (12 new tests: language detection + parser tests per language)

### ✅ Incremental Search (Session 25)
- Real-time search as you type
- Cursor jumps to matches immediately while typing
- Escape restores original cursor position
- Backspace updates search results dynamically
- Configurable in settings.json (default: enabled)
- Tests: 319 → 324 passing (5 new tests)

### ✅ Marks (Session 25)
- `m{a-z}` to set file-local marks
- `'{a-z}` to jump to mark line (start of line)
- `` `{a-z}`` to jump to exact mark position (line and column)
- Marks are stored per buffer
- Tests: 313 → 319 passing (6 new tests)

### ✅ Visual Mode Case Change (Session 25)
- `u` command in visual mode to convert selection to lowercase
- `U` command in visual mode to convert selection to uppercase
- Works in all visual modes: character, line, and block
- Proper undo/redo support
- Tests: 305 → 313 passing (8 new tests)

### ✅ Undo Line (Session 24)
- `U` command to undo all changes on the current line
- Tracks original line state when first modified
- Works across multiple operations on the same line
- Properly integrates with undo/redo stack
- Tests: 295 → 300 passing (5 new tests)

### ✅ Replace Character (Session 24)
- `r` command to replace character under cursor
- Count support (e.g., `3rx` replaces 3 chars with 'x')
- Repeat support with `.` command
- Respects line boundaries (doesn't cross newlines)
- Tests: 288 → 295 passing (7 new tests)

### ✅ Reverse Search (Session 24)
- `?` command for backward search
- Direction-aware `n` and `N` navigation
- Tests: 284 → 288 passing (4 new tests)

### ✅ Session Persistence (Session 23)
- **CRITICAL FIX:** Line numbers now visible in Absolute mode
- Command/search history with Up/Down arrow navigation
- Tab auto-completion for commands
- Window geometry persistence (width/height)
- Explorer visibility state persistence
- Session state at `~/.config/vimcode/session.json`
- Tests: 279 → 284 passing

### ✅ Find/Replace (Session 22)
- Vim :s command with ranges and flags
- VSCode-style Ctrl-F dialog
- Replace/Replace All with undo support

### ✅ Macros (Session 21)
- Record (q), playback (@), repeat (@@)
- Full keystroke capture with Vim encoding

### ✅ Visual Block Mode (Session 19)
- Ctrl-V for rectangular selections
- Block yank/delete/change operations

---

## Future Tasks (Roadmap)

### Known Bugs
- [x] ~~Reverse search (`?`) displays "/" in command line instead of "?"~~ - FIXED
- [x] ~~`cw`/`ce` cursor positioning bug (was placing cursor before space instead of after)~~ - FIXED

### High Priority
- [x] ~~Visual mode case change (u/U in visual mode for lowercase/uppercase)~~ - COMPLETE
- [x] ~~Marks (m, ')~~ - COMPLETE
- [x] ~~Incremental search~~ - COMPLETE
- [x] ~~More grammars (Python/JS/Go/C++)~~ - COMPLETE

### Session Persistence Enhancements
- [ ] Window position (x, y) persistence - requires platform-specific code
- [x] ~~Cursor position persistence per file~~ - COMPLETE
- [x] ~~Scroll position persistence per file~~ - COMPLETE
- [ ] Command history search (Ctrl-R style reverse search)
- [ ] Regex-based command completion
- [ ] Multi-session support (save/restore named sessions)

### Medium Priority
- [ ] Multiple cursors
- [ ] Code folding
- [ ] Git integration
- [ ] LSP support

### Low Priority
- [ ] Themes
- [ ] Plugin system

### Major Architectural Enhancements (Deferred)
- [ ] **TUI Mode (Terminal UI)** - requires major refactoring
  - Trait-based rendering abstraction (Cairo vs Ratatui)
  - Input abstraction layer (GTK vs crossterm/termion)
  - Color/theme system extraction
  - ~2000+ lines of changes
  - Benefits: SSH sessions, lightweight environments
  - Keep `src/core/` clean to ease future abstraction
