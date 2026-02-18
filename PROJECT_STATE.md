# VimCode Project State

**Last updated:** Feb 18, 2026 (Session 40)

## Status

**TUI Sidebar + Icons + Mouse COMPLETE:** Nerd Font icons throughout, TUI sidebar with CRUD, full mouse support, resize bar (346 tests passing)

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
  - **Ctrl-R reverse history search** in command mode (incremental, cycles through matches)
  - Tab auto-completion in command mode
  - Window geometry persistence (size restored on startup)
  - Explorer visibility state (persisted across sessions)
  - Cursor + scroll position per file (restored on reopen)
  - **Open file list restored on startup** — each previously-open file restored into its own tab; active file focused; files explicitly closed via `:q` are excluded from the next session
  - Session state at `~/.config/vimcode/session.json`
- Buffers (:bn/:bp/:b#/:ls/:bd)
- Windows (:split/:vsplit, Ctrl-W)
- Tabs (:tabnew/:tabclose, gt/gT)
- **Quit / save commands:** `:q` closes current tab (quits if last); `:q!` force-closes; `:qa` / `:qa!` close all; `Ctrl-S` saves in any mode

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
- Syntax highlighting (Tree-sitter, Rust/Python/JavaScript/Go/C++, auto-detected by extension)
- **Line numbers (FIXED):** Absolute/relative/hybrid modes now render correctly with proper visibility
- Tab bar, single global status line, command line
- Mouse click positioning (pixel-perfect) — both GTK and TUI
- **Scrollbars:** Per-window vertical/horizontal scrollbars with cursor indicators (GTK + TUI); horizontal scrollbar driven by `max_col` field in `RenderedWindow`
- **Font configuration:** Customizable font family and size
- **Nerd Font icons:** File-type icons in both GTK sidebar and TUI sidebar (`src/icons.rs`)
- **Code folding:** `+`/`-` gutter indicators; entire gutter column is clickable in both GTK and TUI
- **Git integration:** `▌` gutter markers (green=added, yellow=modified); branch name in status bar; `:Gdiff`/`:Gd` command opens unified diff in vertical split; `:Gblame`/`:Gb` command opens `git blame` annotation in a scroll-synced vertical split; **Stage hunks:** `]c`/`[c` navigate between `@@` hunks, `gs`/`:Ghs`/`:Ghunk` stages the hunk under the cursor via `git apply --cached`

### TUI Backend (`src/tui_main.rs`)
- Full editor in terminal via ratatui 0.27 + crossterm
- **Sidebar:** File explorer tree (Ctrl-B toggle, Ctrl-Shift-E focus), j/k navigation, l/Enter open, h collapse, a/A/D CRUD, R refresh
- **Activity bar:** 3-col strip with Explorer / Settings panel icons (Nerd Font)
- **Layout:** activity bar | sidebar | editor col (with its own tab bar); status + cmd full-width at bottom
- **Mouse support:** click-to-position cursor, window switching, scroll wheel (targets window under cursor), scrollbar click-to-jump and drag (vertical + horizontal)
- **Resize bar:** drag separator column to resize sidebar; Alt+Left/Alt+Right keyboard resize; min 15, max 60 cols
- **Vertical scrollbars:** per-window `█`/`░` in rightmost column; in vsplit the separator column doubles as left-pane scrollbar
- **Horizontal scrollbars:** `█`/`░` thumb/track in last row when content is wider than viewport; `┘` corner when both scrollbars present
- **Per-window viewport:** each split pane tracks its own viewport_lines/cols for correct `ensure_cursor_visible` in hsplit/vsplit
- **Scroll sync:** `sync_scroll_binds()` called after keyboard nav and all mouse scroll/drag events (`:Gblame` pairs stay in sync)
- Cursor shapes: bar in insert, underline in replace-r

### Settings
- `~/.config/vimcode/settings.json` (auto-created with defaults on first run)
- LineNumberMode (None/Absolute/Relative/Hybrid)
- Font family and size (hot-reload on save)
- Explorer visibility on startup (default: hidden)
- Incremental search (default: enabled, set to false to disable)
- Auto-indent (default: enabled — Enter/o/O copy leading whitespace from current line)
- **`:set` command** — runtime changes to options (write-through to settings.json):
  - `number`/`nonumber`, `relativenumber`/`norelativenumber` — line number mode
  - `expandtab`/`noexpandtab` (alias `et`) — Tab inserts spaces vs literal tab
  - `tabstop=N` (alias `ts`) — spaces per Tab key press / tab display width
  - `shiftwidth=N` (alias `sw`) — indent width for future `>>` / `<<`
  - `autoindent`/`noautoindent` (alias `ai`), `incsearch`/`noincsearch` (alias `is`)
  - `:set option?` — query current value without changing it
  - `:set` (no args) — show all settings summary
- `:config reload` runtime refresh
- File watcher for automatic reload

## File Structure
```
vimcode/
├── src/
│   ├── main.rs (~3100 lines) — GTK4/Relm4 UI, rendering, find dialog, sidebar resize
│   ├── tui_main.rs (~1050 lines) — ratatui/crossterm TUI backend, sidebar, mouse
│   ├── icons.rs (~30 lines) — Nerd Font file-type icons (shared by GTK + TUI)
│   ├── render.rs (~360 lines) — Platform-agnostic rendering abstraction (ScreenLayout, max_col)
│   └── core/ (~11,400 lines) — Platform-agnostic logic
│       ├── engine.rs (~11,400 lines) — Orchestrates everything, find/replace, macros, history
│       ├── buffer_manager.rs (~600 lines) — Buffer lifecycle
│       ├── buffer.rs (~120 lines) — Rope-based storage
│       ├── session.rs (~175 lines) — Session state persistence (sidebar_width added)
│       ├── settings.rs (~360 lines) — JSON persistence, auto-init, :set parsing
│       ├── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
│       ├── git.rs (~310 lines) — git subprocess integration (diff/blame/hunk parsing, branch detection, stage_hunk)
│       └── Tests: 429 passing (9 find/replace, 14 macro, 8 session, 4 reverse search, 7 replace char, 5 undo line, 8 case change, 6 marks, 5 incremental search, 12 syntax/language, 6 history search, 11 fold tests, 12 git tests, 4 sidebar-preview tests, 5 auto-indent tests, 4 completion tests, 9 quit/ctrl-s tests, 1 session-restore test, 22 set-command tests, 10 hunk-staging tests, 9 text-object tests)
└── Total: ~16,800 lines
```

## Architecture
- **`src/core/`:** No GTK/Relm4/rendering deps (testable in isolation)
- **`src/main.rs`:** All UI/rendering
- **EngineAction:** Core signals UI actions without platform coupling

## Tech Stack
| Component | Library |
|-----------|---------|
| Language | Rust 2021 |
| GTK UI | GTK4 + Relm4 |
| TUI UI | ratatui 0.27 + crossterm |
| Rendering | Pango + Cairo (GTK) / ratatui cells (TUI) |
| Text | Ropey |
| Parsing | Tree-sitter |
| Config | serde + serde_json |

## Commands
```bash
cargo build
cargo run -- <file>
cargo test -- --test-threads=1    # 429 tests
cargo clippy -- -D warnings
cargo fmt
```

## Roadmap

### Completed
- [x] **Visual block mode (Ctrl-V)**
- [x] **Macros (q, @)**
- [x] **Find/Replace (:s + Ctrl-F)**
- [x] **Session Persistence**
- [x] **Reverse search (?)**
- [x] **Replace character (r)**
- [x] **Undo line (U)**
- [x] **Visual mode case change (u/U)**
- [x] **Marks (m, ')**
- [x] **Incremental search**
- [x] **More grammars (Python/JS/Go/C++)**
- [x] **TUI backend (ratatui)**
- [x] **Nerd Font icons (GTK + TUI)**
- [x] **TUI sidebar with CRUD**
- [x] **Mouse support in TUI**
- [x] **Sidebar resize bar (GTK + TUI)**
- [x] **Code Folding (za/zo/zc/zR)**
- [x] **Git: gutter markers, branch in status bar, :Gdiff**
- [x] **Git: :Gstatus, :Gadd[!], :Gcommit, :Gpush**
- [x] **Explorer bug fix: click opens new tab (not replace current)**
- [x] **:Gblame / :Gb** — git blame in a scroll-synced vertical split
- [x] **Explorer preview fix: single-click opens preview tab (italic/dimmed); double-click makes permanent**
- [x] **Horizontal scrollbar fix: per-window visible-column calculation using real Pango char_width**
- [x] **TUI scrollbar polish: vsplit left-pane separator-as-scrollbar, h-scrollbar row, drag support, scroll sync via mouse, per-pane viewport**

### Git (next)
- [x] **Stage hunks** — `]c`/`[c` hunk navigation, `gs`/`:Ghs` stages hunk under cursor via `git apply --cached`

### Editor features
- [x] **Auto-indent** — copies current line's leading whitespace on Enter/o/O; `auto_indent` setting (default: true)
- [x] **Completion menu** — Ctrl-N/Ctrl-P word completion from buffer in insert mode; floating popup in GTK + TUI
- [x] **:set command** — runtime setting changes; write-through to settings.json; number/rnu/expandtab/tabstop/shiftwidth/autoindent/incsearch; query with `?`
- [x] **`ip`/`ap` + `is`/`as` text objects** — paragraph and sentence text objects for all operators and visual mode
- [ ] **VSCode-style search & replace across files** — Ctrl-Shift-F panel; find in project, replace all, show matches list
- [ ] **:grep / :vimgrep** — project-wide search, populate quickfix list *(lower priority)*
- [ ] **Quickfix window** — `:copen`, `:cn`, `:cp` quickfix navigation *(lower priority)*
- [ ] **`it`/`at` tag text objects** — inner/around HTML/XML tag

### Big features
- [ ] **LSP support** — completions, go-to-definition, hover, diagnostics
- [ ] **`gd` / `gD`** — go-to-definition (ctags/ripgrep stub before LSP)
- [ ] **`:norm`** — execute normal command on a range of lines

## Recent Work
**Session 40:** Paragraph and sentence text objects — `ip`/`ap` (inner/around paragraph) and `is`/`as` (inner/around sentence) added to `find_text_object_range` in `engine.rs`. `find_paragraph_object`: scans up/down from cursor while lines share blank/non-blank type; `ap` extends to include trailing (or leading) blank lines. `find_sentence_object`: scans backward for previous `.`/`!`/`?`+whitespace end, forward to next; paragraph boundaries also terminate sentences; `as` includes trailing whitespace. Both work with all operators (`d`/`c`/`y`) and visual mode (`v`). (420→429 tests, 9 new: 5 paragraph + 4 sentence tests.)

**Session 39:** Stage hunks — interactive hunk staging from a `:Gdiff` buffer. New `Hunk` struct and `parse_diff_hunks()` in `git.rs` (pure string parsing, no I/O); `run_git_stdin()` pipes text to git subprocess stdin; `stage_hunk()` builds a minimal patch and runs `git apply --cached -`. `BufferState.source_file` (new field) records which file a diff buffer was generated from. In `engine.rs`: `jump_next_hunk()`/`jump_prev_hunk()` scan for `@@` lines, `cmd_git_stage_hunk()` identifies the hunk under the cursor and stages it. Key wiring: `]c`/`[c` navigate hunks; `gs` (pending `g` + `s`) and `:Ghs`/`:Ghunk` stage the hunk. After staging, gutter markers on the source buffer are refreshed automatically. (410→420 tests, 10 new: 4 hunk-parse unit tests + 6 engine integration tests.)

**Session 38:** `:set` command — vim-compatible runtime setting changes that write through to `settings.json` immediately (VSCode-friendly). New settings fields: `expand_tab` (default true), `tabstop` (default 4), `shift_width` (default 4). Supported syntax: `:set` (show all), `:set number`/`:set nonumber`, `:set tabstop=2`, `:set ts?` (query). Boolean options: `number`/`nu`, `relativenumber`/`rnu`, `expandtab`/`et`, `autoindent`/`ai`, `incsearch`/`is`. Numeric options: `tabstop`/`ts`, `shiftwidth`/`sw`. Line number options interact vim-style: `number` + `relativenumber` = Hybrid mode. Tab key now respects `expand_tab`/`tabstop` instead of hardcoded 4 spaces. (388→410 tests, 22 new.)

**Session 37 (cont):** Session restore + quit fixes — `:q` closes the current tab (quits when it's the last one); `:q!` force-closes; `:qa`/`:qa!` quit all. `Ctrl-S` saves in any mode. Session restore now opens each saved file in its own tab and focuses the previously-active file. `open_file_paths()` filters to window-visible buffers only so files explicitly closed via `:q` are not restored on next launch. (387→388 tests, 1 new session-restore test.)

**Session 37:** Auto-indent + Completion menu + Quit/Save — Auto-indent: pressing Enter/`o`/`O` in insert mode copies the leading whitespace of the current line to the new line; controlled by `auto_indent` setting (default: true). Completion menu: Ctrl-N/Ctrl-P in insert mode scans the current buffer for words matching the prefix at the cursor, shows a floating popup (max 10 items), cycles through them on repeated presses; any other key dismisses and accepts. GTK renders popup via Cairo/Pango; TUI renders via ratatui buffer cells. New engine fields: `completion_candidates`, `completion_idx`, `completion_start_col`. New render types: `CompletionMenu` + four completion colours in `Theme`. (369→388 tests total.)

**Session 36:** TUI scrollbar overhaul + GTK h-scroll fix — TUI vsplit separator now renders `█`/`░` scrollbar chars for the left pane (the separator column IS the left-pane vertical scrollbar; click-to-jump already worked, now it looks right too). Added horizontal scrollbar row (`█`/`░` thumb/track) at the bottom of every TUI window when content is wider than the viewport; `┘` corner when both axes have scrollbars. Vertical scrollbar shortens by 1 row when h-scrollbar is present. All TUI scrollbar thumbs are draggable (unified `ScrollDragState` with `is_horizontal` flag). Scroll wheel now scrolls whichever pane the mouse is over, not always the active one. `sync_scroll_binds()` called after every mouse scroll/drag so `:Gblame` pairs stay in sync. Per-window `set_viewport_for_window` called each frame so `ensure_cursor_visible` uses the actual pane width — fixes horizontal scrolling in vsplit. GTK `HorizontalScrollbarChanged` now routes through new `set_scroll_left_for_window` so dragging a non-active pane's h-scrollbar works. New engine methods: `set_viewport_for_window`, `set_scroll_top_for_window`, `set_scroll_left_for_window`. `max_col` (max line length across full buffer) added to `RenderedWindow`. (369 tests, no change.)
**Session 35:** `:Gblame` + explorer preview fix + scrollbar fixes — Added `:Gblame`/`:Gb` command: runs `git blame --porcelain`, formats output as `<hash> (<author> <date> <lineno>) <content>`, opens in a vertical split with scroll-bound sync so both panes stay in step during keyboard nav and scrollbar drag. Fixed a latent bug in `:Gdiff`/`:Gstatus`/`:Gblame` that deleted the original buffer after splitting (leaving left pane as [No Name]). Fixed explorer single-click regression introduced in session 34: single-click now calls `open_file_preview` (new engine method) which opens a preview tab that is replaced by the next single-click; double-click still calls `open_file_in_tab` for permanent open. Fixed horizontal scrollbar `page_size` incorrectly using the full-editor `viewport_cols` value — now computed per-window from the real Pango `char_width` (cached via `CacheFontMetrics` message), minus the gutter and vertical scrollbar pixels. Fixed vertical scroll sync not firing when the GTK scrollbar is dragged (scrollbar events bypass `process_key`; `sync_scroll_binds` is now also called in `VerticalScrollbarChanged`). (365→369 tests, 4 new sidebar-preview tests + 5 new blame/epoch tests.)
**Session 34:** Explorer tab bug fix + extended git commands — sidebar clicks now call `open_file_in_tab` (new engine method): switches to existing tab if file is already open, else creates a new tab; never replaces current tab's content. Added `:Gstatus`/`:Gs` (git status in vsplit), `:Gadd`/`:Gadd!` (stage current file or all), `:Gcommit <msg>` (commit with message, refreshes diff markers), `:Gpush` (push to remote). All git helpers in `src/core/git.rs`. Roadmap updated with full backlog. (360 tests, no change.)
**Session 33:** Git integration complete — `src/core/git.rs` (new) with subprocess-based diff parsing; `▌` gutter markers in green (Added) or yellow (Modified) in both GTK and TUI backends; current branch name shown in status bar as `[branch]`; `:Gdiff`/`:Gd` command opens unified diff in vertical split; `has_git_diff` flag drives the extra gutter column; TUI fold-click detection fixed to use `any()` so it works when the git column is prepended (357→360 tests, 3 new git diff parser tests).
**Session 32:** Session file restore + fold click polish — open file list (and active buffer) now saved on quit and restored on next launch; entire gutter width is clickable for fold toggle in both GTK and TUI (was pixel-precise single column); GTK gutter text has 3px left padding gap (357 tests, no change).
**Session 31:** Code Folding complete — indentation-based manual folding with `za` (toggle), `zo` (open), `zc` (close), `zR` (open all); fold state stored in `View` (per-window); `move_down`/`move_up` skip hidden lines; `+`/`-` gutter indicators (block-opener heuristic, always visible regardless of line number mode); clickable gutter column; fold-aware rendering in both GTK + TUI backends via `render.rs` (346→357 tests, 11 new fold tests).
**Session 30:** Nerd Font Icons + TUI Sidebar + Mouse + Resize complete — `src/icons.rs` shared icon module; GTK activity bar, toolbar, and file tree all use Nerd Font glyphs; TUI sidebar with full file explorer (j/k/l/h/Enter, CRUD via a/A/D/R, Ctrl-B toggle, Ctrl-Shift-E focus); TUI activity bar (Explorer/Settings panels); drag-to-resize sidebar in GTK (GestureDrag, saved to session) and TUI (mouse drag + Alt+Left/Alt+Right); full TUI mouse support: click-to-position, scroll wheel, scrollbar click-to-jump; per-window `█`/`░` scrollbars in TUI (346 tests, no change).
**Session 29:** TUI backend (Stage 2) + rendering abstraction — `src/render.rs` ScreenLayout bridge; ratatui/crossterm TUI entry point; cursor shapes (bar insert, underline replace-r); Ctrl key combos; viewport sync (346 tests, no change).
**Session 28:** Ctrl-R Command History Search complete — reverse incremental search through command history in command mode; Ctrl-R activates, typing narrows matches, Ctrl-R again cycles to older entries, Escape/Ctrl-G cancels, Enter executes (340→346 tests, 6 new tests).
**Session 27:** Cursor + Scroll Position Persistence complete — reopening a file restores exact cursor line/col and scroll position; positions saved on buffer switch and at quit; also fixed settings file watcher feedback loop freeze and `r` + digit bug (pending key check now runs before count accumulation) (336→340 tests, 3 new session tests).
**Session 26:** Multi-Language Syntax Highlighting complete — Python, JavaScript, Go, C++ support via Tree-sitter. Language auto-detected from file extension. New `SyntaxLanguage` enum, `Syntax::new_from_path()` constructor, buffers now get correct highlighting when opened (324→336 tests, 12 new tests).
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
