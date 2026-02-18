# VimCode Implementation Plan

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
- [ ] **`:grep` / `:vimgrep`** — project-wide search, populate quickfix list
- [ ] **Quickfix window** — `:copen`, `:cn`, `:cp` navigation
- [ ] **More text objects** — `is`/`as` sentence, `ip`/`ap` paragraph, `it`/`at` tag

### Big Features
- [ ] **LSP support** — completions, go-to-definition, hover, diagnostics
- [ ] **`gd` / `gD`** — go-to-definition (ctags/ripgrep stub before LSP)
- [ ] **`:norm`** — execute normal command on a range of lines
- [ ] **Multiple cursors**
- [ ] **Themes / plugin system**
