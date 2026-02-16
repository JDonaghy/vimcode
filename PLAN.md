# VimCode Implementation Plan

## Recently Completed

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

### High Priority
- [ ] Reverse search (?)
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
