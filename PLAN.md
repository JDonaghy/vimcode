# VimCode Implementation Plan

## Recently Completed (Session 52)

### ✅ :norm Command
- **`:norm[al][!] {keys}`** — execute normal-mode keystrokes on a line range; `!` accepted and treated identically
- **Ranges:** no range (current line), `%` (all lines), `'<,'>` (visual selection), `N,M` (1-based numeric)
- **Key decoding:** local decode loop (does not touch `macro_playback_queue`); supports `<CR>`, `<BS>`, `<C-x>`, `<Left>`/`<Right>`/etc.
- **Single undo:** all changes from `:norm` collapsed into one undo entry (undo with single `u`); achieved by recording undo-stack depth before execution and merging new entries after
- **Trim fix:** norm check runs before `cmd.trim()` so trailing spaces in keys (e.g. `I// `) are preserved
- **Free helpers:** `try_parse_norm()` and `norm_numeric_range_end()` (module-level)
- File changes: `src/core/engine.rs` (`execute_norm_command` method, dispatch in `execute_command`, 2 free helpers, 9 new tests; `UndoEntry` added to imports)
- Tests: 535 → 544 total

---

## Recently Completed (Session 51)

### ✅ it/at Tag Text Objects
- **`it` (inner tag)** — selects content between nearest enclosing HTML/XML open+close tag pair; works with all operators (`d`, `c`, `y`) and visual mode (`v`)
- **`at` (around tag)** — selects the full element including opening and closing tags
- **Algorithm:** backward scan for nearest `<tagname>` open tag, forward scan to matching `</tagname>` with nesting depth tracking; cursor must be within element extent
- **Case-insensitive:** `<DIV>text</div>` treated as a valid pair
- **Nested tags:** `<div><div>inner</div>outer</div>` — cursor in inner selects only inner content
- **Attributes:** `<div class="foo">content</div>` — attribute values with `"` or `'` handled correctly
- **Self-closing / comments skipped:** `<br/>`, `<!--...-->`, `<!DOCTYPE>`, `<?...?>` not treated as enclosing tags
- File changes: `src/core/engine.rs` (`find_tag_text_object` method, `'t'` arm in `find_text_object_range`, 9 new tests)
- Tests: 526 → 535 total

---

## Recently Completed (Session 50)

### ✅ CPU Performance Fixes
- **Cached `max_col`:** `BufferState` now stores `max_col: usize`; initialized in both constructors; computed once in `update_syntax()` instead of O(N_lines) scan per render frame in `render.rs`
- **60fps frame rate cap:** TUI event loop limits renders to ~60fps via `min_frame = Duration::from_millis(16)` and `last_draw: Instant`; eliminates uncapped rendering from rapid LSP/search events
- File changes: `src/core/buffer_manager.rs` (max_col field + compute in update_syntax), `src/render.rs` (use cached max_col), `src/tui_main.rs` (frame rate gate + Instant import)
- Tests: no change (526 total)

---

## Recently Completed (Session 49)

### ✅ 6 High-Priority Vim Features
- **Toggle case:** `~` toggles case of char(s) under cursor; count support (5~); dot-repeatable; visual `~` for selections
- **Scroll cursor:** `zz` (center), `zt` (top), `zb` (bottom) — adjusts `scroll_top` without moving cursor
- **Join lines:** `J` joins next line, collapses leading whitespace to one space (no space before `)`, `]`, `}`); count; dot-repeatable
- **Search word under cursor:** `*` (forward) / `#` (backward) with whole-word boundaries; `n`/`N` continue bounded search; clears on new `/`/`?`
- **Jump list:** `Ctrl-O` (back) / `Ctrl-I` (forward); max 100 entries; cross-file; push on G, gg, /, n, N, %, {, }, gd, *, #
- **Indent/dedent:** `>>` / `<<` indent/dedent count lines by `shiftwidth`; visual `>`/`<`; dot-repeatable; respects `expandtab`
- File changes: `engine.rs` (+600 lines, 6 new ChangeOp variants, 6 new helper sets, 31 new tests), `README.md`, `PROJECT_STATE.md`, `PLAN.md`
- Tests: 495 → 526 total

---

## Recently Completed (Session 48)

### ✅ LSP Bug Fixes + TUI Performance Optimizations
- **Protocol compliance:** `notify_did_open` returns `Result<(), String>` with descriptive errors; initialization guards on all notification methods prevent premature `didOpen`/`didChange`/`didSave`/`didClose`
- **Deterministic response routing:** `pending_requests: Arc<Mutex<HashMap<i64, String>>>` maps request ID → method name; reader thread uses this for correct routing instead of fragile content-based guessing
- **Server request handling:** reader thread now responds to server-initiated requests (e.g. `window/workDoneProgress/create`) with `{"result": null}`; error responses generate proper events with empty data
- **Diagnostic flood optimization:** events capped at 50 per `poll_lsp()` call; pre-computed visible buffer paths (computed once, not per-event); only trigger redraw for diagnostics affecting visible buffers
- **Path mismatch fix:** LSP diagnostics keyed by absolute URI-derived paths; buffer `file_path` may be relative; added `canonicalize()` at lookup points in `render.rs`, `diagnostic_counts()`, `jump_next_diagnostic()`, `jump_prev_diagnostic()`
- **TUI on-demand rendering:** `needs_redraw` flag eliminates unconditional 50 FPS rendering; adaptive poll timeout (1ms when redraw pending, 50ms when idle)
- **Idle-only background work:** `lsp_flush_changes()`, `poll_lsp()`, `poll_project_search()`, `poll_project_replace()` moved to only run when no input is pending — prevents blocking pipe writes during typing
- **stderr fix:** reverted `Stdio::inherit()` to `Stdio::null()` for child process stderr (rust-analyzer stderr was corrupting TUI display)
- File changes: `lsp.rs` (750→1186 lines), `lsp_manager.rs` (340→394 lines), `engine.rs` (+400 lines), `tui_main.rs` (+80 lines), `render.rs` (+100 lines)
- Tests: no change (495 total)

---

## Recently Completed (Session 47)

### ✅ LSP Support (Language Server Protocol)
- **New files:** `src/core/lsp.rs` (~750 lines), `src/core/lsp_manager.rs` (~340 lines)
- **Dependency:** `lsp-types = "0.97"` (protocol type definitions, no runtime)
- **Architecture:** lightweight custom LSP client using `std::thread` + `std::sync::mpsc` (same pattern as project search); no tokio/async runtime
- **Built-in server registry:** rust-analyzer, pyright-langserver, typescript-language-server, gopls, clangd — auto-detected on `PATH`
- **Protocol transport:** `LspServer::start()` spawns subprocess, reader thread parses JSON-RPC frames, dispatches `LspEvent`s via channel; full document sync
- **Multi-server coordinator:** `LspManager` routes notifications/requests by language ID, lazy-starts servers on first use
- **Engine integration:** 4 hook points (open/change/save/close); `poll_lsp()` processes diagnostics/completions/definition/hover events; debounced `didChange` via dirty buffer tracking
- **Key bindings:** `]d`/`[d` (diagnostic nav), `gd` (go-to-definition), `K` (hover), `Ctrl-Space` (LSP completions)
- **Commands:** `:LspInfo`, `:LspRestart`, `:LspStop`
- **Settings:** `lsp_enabled` bool + `lsp_servers` array; `:set lsp`/`:set nolsp` toggle
- **Render layer:** `DiagnosticMark` + `HoverPopup` types; `diagnostic_gutter` map; diagnostic/hover theme colours
- **GTK backend:** wavy underlines via Cairo curves, colored gutter dots, hover popup, LSP poll in SearchPollTick, shutdown on quit
- **TUI backend:** colored underlines + E/W/I/H gutter chars, hover popup, LSP poll in event loop, shutdown on quit
- **Status bar:** `E:N W:N` diagnostic counts in right section
- Tests: 37 new (27 lsp-protocol + 10 lsp-engine); 458→495 total

---

## Recently Completed (Session 46)

### ✅ TUI Scrollbar Drag Fix
- **Immediate h-scroll**: Removed deferred `pending_h_scroll` mechanism — h-scrollbar drag now calls `set_scroll_left_for_window` immediately during drag (matching v-scrollbar behaviour)
- **Drag event coalescing**: After processing a `MouseEventKind::Drag` event, drains all additional queued drag events via `ct_event::poll(Duration::ZERO)`, keeping only the final mouse position; benefits all drag operations (h-scrollbar, v-scrollbar, sidebar resize)
- **Unified scrollbar colour**: V-scrollbar thumb changed from `theme.status_fg` (`#e5e5e5`) to `Rgb(128, 128, 128)` grey to match h-scrollbar
- **Cleanup**: Removed `pending_h_scroll` parameter from `handle_mouse`, `draw_frame`, `render_all_windows`, `render_window`, and `render_h_scrollbar` signatures
- Tests: no change (458 total)

---

## Recently Completed (Session 45)

### ✅ Replace Across Files
- **`replace_in_project()`** in `project_search.rs`: walks files with `ignore` crate, applies `regex::replace_all`, writes back only changed files; `NoExpand` wrapper prevents `$` interpretation in literal mode; files in `skip_paths` are skipped and reported
- **`ReplaceResult` struct**: `replacement_count`, `file_count`, `skipped_files`, `modified_files`
- **`build_search_regex()` refactor**: extracted shared regex builder from `search_in_project` for reuse by both search and replace
- **Engine integration**: `project_replace_text` field; `start_project_replace` (async) / `poll_project_replace` / `run_project_replace` (sync); `apply_replace_result` reloads open buffers, clears undo stacks, refreshes git diff, builds status message with skip info
- **GTK**: Replace `Entry` + "Replace All" button between toggle row and status label; `ProjectReplaceTextChanged` / `ProjectReplaceAll` messages; replace poll piggybacked on `SearchPollTick`
- **TUI**: `replace_input_focused` field; `Tab` switches between search/replace inputs; `Enter` in replace box triggers replace; `Alt+H` shortcut; new `[Replace…]` input row (row 2); all layout offsets shifted +1; mouse click routing updated
- **Tests**: 14 new (9 project_search replace tests + 5 engine replace tests); 444→458 total

---

## Recently Completed (Session 44)

### ✅ Enhanced Project Search (Regex, Whole Word, Case Toggle + Performance)
- **`ignore` crate walker**: Replaced hand-rolled `walk_dir` with `ignore::WalkBuilder` (same as ripgrep) — respects `.gitignore`, skips `target/`, binary detection via UTF-8 decode
- **`regex` crate matching**: `SearchOptions` struct with `case_sensitive`, `whole_word`, `use_regex` toggles; builds `regex::Regex` from query + options; invalid regex returns `SearchError`
- **Result cap**: Max 10,000 matches to prevent memory issues; status message shows "(capped at 10000)" when hit
- **Engine integration**: `project_search_options` field; async channel changed to `Result<Vec<ProjectMatch>, SearchError>`; 3 toggle methods
- **GTK**: 3 `ToggleButton` widgets (`Aa`, `Ab|`, `.*`) with CSS styling; 3 new `Msg` variants
- **TUI**: `Alt+C`/`Alt+W`/`Alt+R` toggles in both input and results mode; toggle indicator row with active/inactive coloring
- **Tests**: 6 new (case-sensitive, whole-word, regex, invalid-regex, whole-word+regex combo, gitignore-respected); 438→444 total

---

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
- [x] **Enhanced project search** — regex/case/whole-word toggles; `.gitignore`-aware via `ignore` crate; 10k result cap; GTK toggle buttons + TUI Alt+C/W/R
- [x] **VSCode-style replace across files** — replace all matches in project; skip dirty buffers; reload open buffers; regex capture group backreferences
- [ ] **`:grep` / `:vimgrep`** — project-wide search, populate quickfix list *(lower priority)*
- [ ] **Quickfix window** — `:copen`, `:cn`, `:cp` navigation *(lower priority)*
- [x] **`it`/`at` tag text objects** — inner/around HTML/XML tag

### Big Features
- [x] **LSP support** — completions, go-to-definition, hover, diagnostics (session 47 + 48 bug fixes)
- [x] **`gd` / `gD`** — go-to-definition via LSP
- [x] **`:norm`** — execute normal command on a range of lines
- [ ] **Fuzzy finder / Telescope-style** — live fuzzy file + buffer + symbol search in a floating panel *(consider after VSCode search)*
- [ ] **Multiple cursors**
- [ ] **Themes / plugin system**
