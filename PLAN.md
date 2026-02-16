# VimCode Implementation Plan

## Recently Completed

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
- [ ] Reverse search (`?`) displays "/" in command line instead of "?"
- [ ] `cw` at the last word in a line immediately deletes the newline character
- [ ] `ce` incorrectly deletes the character before the word

### High Priority
- [ ] Visual mode case change (u/U in visual mode for lowercase/uppercase)
- [ ] Marks (m, ')
- [ ] Incremental search
- [ ] More grammars (Python/JS/Go/C++)

### Session Persistence Enhancements
- [ ] Window position (x, y) persistence - requires platform-specific code
- [ ] Cursor position persistence per file
- [ ] Scroll position persistence per file
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
