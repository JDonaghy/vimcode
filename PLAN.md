# VimCode Implementation Plan

## Recently Completed (Session 43)

### ✅ Search Panel Bug Fixes
- **GTK CSS fix**: Changed CSS selectors from `listbox` / `listbox row` to `.search-results-list` / `.search-results-list > row` — GTK4 uses `list` as the CSS node name for `GtkListBox`, so the old selectors never matched; replaced `.search-results-scroll > viewport` with `.search-results-scroll` on the ScrolledWindow itself
- **GTK startup crash fix**: `sync_scrollbar` called during initial `connect_resize` with near-zero dimensions caused `(rect.height - 10.0) as i32` to be negative, rejected by GTK; added early return guard (`da_width < 20.0 || da_height < 20.0`) and clamped `.max(0)`
- **TUI search scrollbar drag**: New `SidebarScrollDrag` struct for drag state; `Down` click on search scrollbar column arms drag; `Drag` event proportionally scrolls `search_scroll_top`; `Up` clears drag
- **TUI j/k scroll-into-view**: `j`/`k` in search results now call `ensure_search_selection_visible` to keep the selected result in the viewport

---

## Recently Completed (Session 42)

### ✅ Search Panel Polish + CI Fix
- **TUI scroll redesign**: `search_scroll_top` is now an independent viewport offset driven by scroll wheel/scrollbar clicks; selection only adjusts scroll when it leaves the viewport (mirrors how Explorer and Editor scrolling work)
- **TUI scrollbar interactivity**: Explorer scrollbar column click → jump-scroll (`sidebar.scroll_top`); Search scrollbar column click → jump-scroll (`sidebar.search_scroll_top`); scroll wheel in sidebar area scrolls Explorer or Search content
- **GTK dark background**: `.search-results-scroll > viewport` CSS targets the internal GTK viewport widget; `.search-results-list label { color: #cccccc; }` fixes grey text; `set_overlay_scrolling: false` makes scrollbar always visible
- **Threaded search**: `engine.start_project_search(PathBuf)` spawns a thread and stores `Receiver`; `engine.poll_project_search() -> bool` checks for results non-blocking; GTK polls via `glib::timeout_add_local(50ms)`; TUI polls each frame
- **CI clippy fix**: Two `map_or(false, ...)` → `is_some_and(...)` in `engine.rs` (lint added in Rust 1.84+)
- **Tests**: 4 new engine-level project search tests (434 → 438 tests)

---

## Recently Completed (Session 41)

### ✅ VSCode-Style Project Search Panel
- Ctrl-Shift-F (GTK + TUI) opens Search panel in sidebar
- `src/core/project_search.rs`: `ProjectMatch` struct + `search_in_project(root, query)`
  - Recursive walk, skips hidden (`.`) dirs/files, skips binary (non-UTF-8) files
  - Case-insensitive literal match; sorted by file path then line number
- Engine: 3 new fields (`project_search_query/results/selected`) + 3 new methods
- GTK: Search button in activity bar enabled; `gtk4::Entry` + `gtk4::ListBox`; file-header rows + result rows; click opens file at matched line
- TUI: `TuiPanel::Search`; `search_input_mode` bool; `render_search_panel()`; input/results keyboard modes; j/k navigation; Enter opens file
- Activity bar row order: Explorer (0) → Search (1) → Settings (2)
- Tests: 5 new (429→434)

---

## Recently Completed (Session 40)

### ✅ Paragraph and Sentence Text Objects
- `ip` / `ap` — inner/around paragraph (contiguous non-blank lines); `ap` includes trailing blank lines
- `is` / `as` — inner/around sentence (`.`/`!`/`?`-terminated); `as` includes trailing whitespace
- Both work with all operators: `d`, `c`, `y`, `v` (visual selection)
- `ip` on a blank line selects the contiguous blank-line block
- Paragraph boundary (blank line) also terminates a sentence
- Tests: 9 new (420→429)

---

## Recently Completed (Session 39)

### ✅ Stage Hunks
- `]c` / `[c` — jump to next/previous `@@` hunk header in diff buffer
- `gs` (via `g` + `s` pending key) — stage hunk under cursor using `git apply --cached`
- `:Ghs` / `:Ghunk` — command-line aliases for stage hunk
- `Hunk` struct + `parse_diff_hunks()` in `git.rs` — pure string parsing, no subprocess
- `run_git_stdin()` — pipes patch text to git subprocess stdin
- `stage_hunk()` — builds minimal patch and feeds it to `git apply --cached -`
- `BufferState.source_file: Option<PathBuf>` — set by `:Gdiff` so hunk staging knows which file to patch
- After staging: refreshes gutter markers on the source buffer if it's open
- Tests: 10 new (410→420)

---

## Recently Completed (Session 38)

### ✅ :set Command
- Vim-compatible `:set option`, `:set nooption`, `:set option=N`, `:set option?`, `:set` (show all)
- Write-through: every change immediately saved to `settings.json`
- Boolean options: `number`/`nu`, `relativenumber`/`rnu`, `expandtab`/`et`, `autoindent`/`ai`, `incsearch`/`is`
- Numeric options: `tabstop`/`ts`, `shiftwidth`/`sw`
- `number` + `relativenumber` → Hybrid line number mode (vim-accurate)
- New settings fields: `expand_tab` (default true), `tabstop` (default 4), `shift_width` (default 4)
- Tab key uses `expand_tab`/`tabstop` instead of hardcoded 4 spaces
- Tests: 22 new (388→410)

---

## Recently Completed (Session 37)

### ✅ Auto-Indent
- Enter/`o`/`O` in insert mode copies leading whitespace of current line to new line
- Controlled by `auto_indent` setting (default: true)
- Tests: 5 new (369→374)

### ✅ Completion Menu (Ctrl-N / Ctrl-P)
- In insert mode: scans buffer for words matching prefix at cursor
- Floating popup (max 10 candidates), cycles on repeated Ctrl-N/P
- Any other key dismisses and accepts current candidate
- GTK: Cairo/Pango popup; TUI: ratatui buffer cells with border
- New engine fields: `completion_candidates`, `completion_idx`, `completion_start_col`
- New render types: `CompletionMenu` + four completion colours in `Theme`
- Tests: 4 new (374→378)

### ✅ Quit / Save Commands
- `:q` — closes current tab; quits if it's the last one (blocked if dirty)
- `:q!` — force-closes current tab; force-quits if last
- `:qa` — quit all (blocked if any dirty buffer)
- `:qa!` — force-quit all
- `Ctrl-S` — save current buffer in any mode without changing mode
- Tests: 9 new (378→387)

### ✅ Session Restore Fix
- Each file in `open_files` restored into its own tab on startup
- Previously-active file's tab is focused
- `open_file_paths()` filters to window-visible buffers so files closed via
  `:q` are not re-opened next session
- Tests: 1 new (387→388)

---

## Recently Completed (Sessions 29–36)

### ✅ TUI Backend (Sessions 29–30)
- Full ratatui/crossterm terminal UI with sidebar, mouse, scrollbars, resize

### ✅ Code Folding (Session 31)
- `za`/`zo`/`zc`/`zR`; gutter indicators; clickable gutter column

### ✅ Session File Restore (Session 32)
- Open file list saved on quit and restored on next launch

### ✅ Git Integration (Sessions 33–35)
- Gutter markers, branch in status bar, `:Gdiff`, `:Gstatus`, `:Gadd`, `:Gcommit`, `:Gpush`, `:Gblame`

### ✅ Explorer Preview (Session 35)
- Single-click → preview tab (italic); double-click → permanent

### ✅ Scrollbar Polish (Session 36)
- Per-window vertical + horizontal scrollbars in TUI; drag support; scroll sync

---

## Roadmap

### Git
- [x] **Stage hunks** — `]c`/`[c` navigation, `gs`/`:Ghs` to stage hunk under cursor

### Editor Features
- [x] **`:set` command** — runtime setting changes; write-through to settings.json; number/rnu/expandtab/tabstop/shiftwidth/autoindent/incsearch; query with `?`
- [x] **`ip`/`ap` paragraph text objects** — inner/around paragraph (contiguous non-blank lines)
- [x] **`is`/`as` sentence text objects** — inner/around sentence (`.`/`!`/`?`-delimited)
- [ ] **VSCode-style search & replace across files** — Ctrl-Shift-F panel; find in project, replace all, show matches list; no shell dependency
- [ ] **`:grep` / `:vimgrep`** — project-wide search, populate quickfix list *(lower priority)*
- [ ] **Quickfix window** — `:copen`, `:cn`, `:cp` navigation *(lower priority)*
- [ ] **`it`/`at` tag text objects** — inner/around HTML/XML tag

### Big Features
- [ ] **LSP support** — completions, go-to-definition, hover, diagnostics
- [ ] **`gd` / `gD`** — go-to-definition (ctags/ripgrep stub before LSP)
- [ ] **`:norm`** — execute normal command on a range of lines
- [ ] **Fuzzy finder / Telescope-style** — live fuzzy file + buffer + symbol search in a floating panel *(consider after VSCode search)*
- [ ] **Multiple cursors**
- [ ] **Themes / plugin system**
