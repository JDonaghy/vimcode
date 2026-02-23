# VimCode Project State

**Last updated:** Feb 23, 2026 (Session 75) | **Tests:** 638

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 72 are in **SESSION_HISTORY.md**.

---

## Recent Work

**Session 75:** Terminal deep history, real PTY resize, and CWD fix — `TerminalPane` gains `history: VecDeque<Vec<HistCell>>` ring buffer (capacity configurable via `terminal_scrollback_lines` setting, default 5000). `process_with_capture()` splits PTY output into ≤rows-newline chunks; `capture_scrolled_rows()` safely reads just-scrolled-off rows via `set_scrollback(N ≤ rows_len)`, stores as `HistCell` rows, then restores `set_scrollback(0)`. `build_terminal_panel()` merges: at scroll_offset N, rows 0..N from `history[hist_len-N+i]`, rows N..rows from `screen.cell(i-N)`. `terminal_find_update_matches()` now searches `history` VecDeque directly. `TerminalPane` stores `master: Box<dyn MasterPty+Send>`; `resize()` calls `self.master.resize(PtySize{…})` for real SIGWINCH. GTK `Msg::Resize` propagates to terminal. TUI `Event::Resize` uses `session.terminal_panel_rows` (was hardcoded 12). `terminal_new_tab()` passes `self.cwd.clone()` → `cmd.cwd(cwd)`. 638 tests.

**Session 74:** Terminal find bug fixes — (1) Find now scans scrollback history: `terminal_find_matches` changed to `Vec<(usize, u16, u16)>` (required_offset, row, col); `terminal_find_update_matches()` scans at both `max_offset` and `0`, deduplicating by absolute line; `terminal_find_next/prev()` calls `set_scroll_offset()`; `build_terminal_panel()` uses `visible_row = mr + current_offset - moffset`. (2) GTK full-width background fill + `CacheFontMetrics` auto-resize. 638 tests.

**Session 73:** Terminal find bar — Ctrl+F while terminal has focus opens inline find bar in toolbar row (replaces tab strip). Case-insensitive; active match orange, others amber. Enter/Shift+Enter navigate; Escape/Ctrl+F close. Engine: 4 new fields + 7 methods. render.rs: `TerminalCell` +2 booleans; `TerminalPanel` +4 find fields. GTK + TUI: routing, toolbar, cell colors. 638 tests.

**Session 72:** Terminal multiple tabs + auto-close — `terminal_panes: Vec<TerminalPane>` + `terminal_active: usize`. `terminal_new_tab()` / `terminal_close_active_tab()` / `terminal_switch_tab()`. Alt-1–9 switches tabs. `poll_terminal()` auto-removes exited panes; panel closes when last exits. 638 tests.

---

## File Structure

```
src/
├── main.rs          (~5470 lines)  GTK4/Relm4 UI, rendering, all panels
├── tui_main.rs      (~4705 lines)  ratatui/crossterm TUI backend
├── render.rs        (~1650 lines)  Platform-agnostic ScreenLayout bridge
├── icons.rs            (~30 lines)  Nerd Font file-type icons
└── core/            (~24,100 lines)  Zero GTK/rendering deps — fully testable
    ├── engine.rs    (~19,920 lines)  Orchestrator: keys, commands, all features
    ├── terminal.rs     (~320 lines)  PTY-backed terminal pane (portable-pty + vt100)
    ├── lsp.rs        (~1,200 lines)  LSP protocol transport + single-server client
    ├── lsp_manager.rs  (~400 lines)  Multi-server coordinator + built-in registry
    ├── project_search.rs (~630 lines)  Regex/case/word search + replace
    ├── buffer_manager.rs (~600 lines)  Buffer lifecycle, undo/redo
    ├── buffer.rs       (~120 lines)  Rope-based text storage (ropey)
    ├── session.rs      (~180 lines)  Session state persistence
    ├── settings.rs   (~1,030 lines)  JSON config, :set parsing, key bindings
    ├── git.rs          (~635 lines)  git subprocess integration
    └── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
Tests: 638 passing
Total: ~35,000 lines
```

---

## Roadmap

### Completed
- [x] Core Vim (7 modes, navigation, operators, text objects, macros, marks, registers, undo, visual, search, folding)
- [x] Find/Replace (:s + Ctrl-F dialog with replace-all)
- [x] Session persistence (files, cursor/scroll, history, geometry)
- [x] Multi-language syntax highlighting (Tree-sitter: Rust/Python/JS/Go/C++)
- [x] TUI backend (ratatui + crossterm, feature-parity with GTK)
- [x] Nerd Font icons (GTK + TUI)
- [x] Sidebar: file explorer with CRUD, rename, move, drag-and-drop, preview mode, auto-refresh
- [x] Sidebar resize (drag + Alt+Left/Right in TUI)
- [x] Mouse support (GTK + TUI: click, drag, scroll, scrollbar)
- [x] Scrollbars: vertical + horizontal, per-window, draggable (GTK + TUI)
- [x] Code folding (za/zo/zc/zR, indentation-based)
- [x] Git: gutter markers, branch in status, :Gdiff/:Gstatus/:Gadd/:Gcommit/:Gpush/:Gblame
- [x] Stage hunks (]c/[c navigation, gs/:Ghs via git apply --cached)
- [x] File diff (:diffthis/:diffoff/:diffsplit, LCS algorithm)
- [x] :set command (runtime settings, write-through to settings.json)
- [x] Auto-indent + auto-popup completion (buffer words + LSP async)
- [x] Configurable key bindings (panel_keys, explorer_keys, completion_keys)
- [x] VSCode mode (Alt-M toggle, always-insert, Ctrl shortcuts, Shift+Arrow selection)
- [x] Project search + replace (regex/case/word toggles, .gitignore-aware, replace-all)
- [x] Fuzzy file finder (Ctrl-P, Telescope-style)
- [x] Live grep (Ctrl-G, two-column modal, ±5 context)
- [x] Quickfix window (:grep/:copen/:cn/:cp/:cc)
- [x] :norm command (range execution, undo merge)
- [x] LSP support (completions, go-to-definition, hover, diagnostics, auto-detect)
- [x] Integrated terminal (PTY, VT100/256-color, multiple tabs, scrollback, find, resize)

### Planned / Ideas
- [ ] Terminal button bar (add/split/trash icons, horizontal split-view)
- [ ] More Tree-sitter grammars (TypeScript, HTML, CSS, JSON, YAML, Lua)
- [ ] Vim `%` (matchit-style tag jumping)
- [ ] `:set wrap` / line-wrap rendering
- [ ] Remote editing (SSH/SFTP)
