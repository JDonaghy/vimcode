# VimCode Project State

**Last updated:** Feb 19, 2026 (Session 48)

## Status

**LSP Support STABLE:** Language server auto-detection, diagnostics (inline underlines + gutter icons), completions (Ctrl-Space), go-to-definition (gd), hover (K), diagnostic navigation (]d/[d); protocol compliance fixes + TUI performance optimizations — 495 tests passing

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

### Project Search + Replace (Complete)
- VSCode-style search panel accessed via Search icon in activity bar or Ctrl-Shift-F
- Case-insensitive literal search across all text files under the project root
- Grouped results list (file headers + `line: text` rows)
- GTK: Entry input + ListBox results; click row opens file at that line
- TUI: `[query]` input box; Enter to search; j/k navigate results; Enter opens file
- **Replace across files:** Replace input + "Replace All" button (GTK) / Enter in replace box / Alt+H (TUI); skips dirty buffers; reloads open buffers; regex capture group backreferences in regex mode; literal `$` in literal mode

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

### LSP Support (Language Server Protocol)
- Automatic language server detection — open a file and features light up if the server is on `PATH`
- Built-in registry: rust-analyzer, pyright-langserver, typescript-language-server, gopls, clangd
- Custom servers configurable via `settings.json` `lsp_servers` array
- **Diagnostics:** inline underlines (wavy in GTK, colored in TUI) + severity-colored gutter icons (dots in GTK, E/W/I/H chars in TUI)
- **Diagnostic navigation:** `]d` / `[d` jump to next/prev diagnostic with wrap-around
- **LSP completions:** `Ctrl-Space` in insert mode; feeds into existing completion popup
- **Go-to-definition:** `gd` opens the definition file and jumps to the target line/column
- **Hover:** `K` shows type/documentation popup above cursor; dismissed on any keypress
- **Diagnostic counts:** `E:N W:N` in status bar right section
- **Commands:** `:LspInfo`, `:LspRestart`, `:LspStop`
- **Settings:** `lsp` boolean (default true); `:set lsp` / `:set nolsp` toggle
- Full document sync (simple + correct; incremental sync is a future optimization)
- Debounced `didChange` via dirty buffer tracking, flushed only during idle periods (no pending input)
- Deterministic response routing via `pending_requests` map (request ID → method name)
- LSP initialization guards: no `didOpen`/`didChange`/`didSave`/`didClose` until server handshake complete
- Server-initiated requests (e.g. `window/workDoneProgress/create`) receive proper responses
- Error responses from server generate proper events with empty data (no silent drops)
- Diagnostic flood optimization: events capped at 50 per poll, only redraw for visible buffers
- Path canonicalization: diagnostics keyed by absolute paths; lookups canonicalize buffer paths
- Pure `std::thread` + `std::sync::mpsc` — no tokio/async runtime

### TUI Backend (`src/tui_main.rs`)
- Full editor in terminal via ratatui 0.27 + crossterm
- **Sidebar:** File explorer tree (Ctrl-B toggle, Ctrl-Shift-E focus), j/k navigation, l/Enter open, h collapse, a/A/D CRUD, R refresh; **Search panel** (Ctrl-Shift-F): query input, Enter to run, j/k results, Enter opens file
- **Activity bar:** 3-col strip with Explorer / Search / Settings panel icons (Nerd Font)
- **Layout:** activity bar | sidebar | editor col (with its own tab bar); status + cmd full-width at bottom
- **Mouse support:** click-to-position cursor, window switching, scroll wheel (targets window under cursor), scrollbar click-to-jump and drag (vertical + horizontal)
- **Resize bar:** drag separator column to resize sidebar; Alt+Left/Alt+Right keyboard resize; min 15, max 60 cols
- **Vertical scrollbars:** per-window `█`/`░` in rightmost column; in vsplit the separator column doubles as left-pane scrollbar
- **Horizontal scrollbars:** `█`/`░` thumb/track in last row when content is wider than viewport; `┘` corner when both scrollbars present
- **Per-window viewport:** each split pane tracks its own viewport_lines/cols for correct `ensure_cursor_visible` in hsplit/vsplit
- **Scroll sync:** `sync_scroll_binds()` called after keyboard nav and all mouse scroll/drag events (`:Gblame` pairs stay in sync)
- **Drag event coalescing:** consecutive `MouseEventKind::Drag` events are coalesced (only the final position is rendered), eliminating render-per-pixel lag on all drag operations
- **Idle-only background work**: `lsp_flush_changes()`, `poll_lsp()`, `poll_project_search()`, `poll_project_replace()` only run when no input is pending (prevents blocking pipe writes during typing)
- **On-demand rendering**: `needs_redraw` flag skips rendering when nothing changed; adaptive poll timeout (1ms when redraw pending, 50ms idle)
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
│   ├── main.rs (~3600 lines) — GTK4/Relm4 UI, rendering, find dialog, sidebar resize, search/replace panel
│   ├── tui_main.rs (~2900 lines) — ratatui/crossterm TUI backend, sidebar, mouse, search/replace panel
│   ├── icons.rs (~30 lines) — Nerd Font file-type icons (shared by GTK + TUI)
│   ├── render.rs (~1200 lines) — Platform-agnostic rendering abstraction (ScreenLayout, max_col)
│   └── core/ (~20,200 lines) — Platform-agnostic logic
│       ├── engine.rs (~14,600 lines) — Orchestrates everything, find/replace, macros, history, LSP, project search/replace
│       ├── lsp.rs (~1,200 lines) — LSP protocol transport + single-server client (request ID tracking)
│       ├── lsp_manager.rs (~400 lines) — Multi-server coordinator with initialization guards
│       ├── project_search.rs (~630 lines) — Regex/case/whole-word search + replace (ignore + regex crates)
│       ├── buffer_manager.rs (~600 lines) — Buffer lifecycle
│       ├── buffer.rs (~120 lines) — Rope-based storage
│       ├── session.rs (~175 lines) — Session state persistence (sidebar_width added)
│       ├── settings.rs (~640 lines) — JSON persistence, auto-init, :set parsing
│       ├── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
│       ├── git.rs (~635 lines) — git subprocess integration (diff/blame/hunk parsing, branch detection, stage_hunk)
│       └── Tests: 495 passing (9 find/replace, 14 macro, 8 session, 4 reverse search, 7 replace char, 5 undo line, 8 case change, 6 marks, 5 incremental search, 12 syntax/language, 6 history search, 11 fold tests, 12 git tests, 4 sidebar-preview tests, 5 auto-indent tests, 4 completion tests, 9 quit/ctrl-s tests, 1 session-restore test, 22 set-command tests, 10 hunk-staging tests, 9 text-object tests, 24 project-search tests, 5 engine-replace tests, 27 lsp-protocol tests, 10 lsp-engine tests)
└── Total: ~20,900 lines
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
| LSP | lsp-types 0.97 |
| Config | serde + serde_json |

## Commands
```bash
cargo build
cargo run -- <file>
cargo test -- --test-threads=1    # 495 tests
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
- [x] **TUI scrollbar drag fix: immediate h-scroll (no deferred apply), drag event coalescing, unified grey scrollbar color**
- [x] **LSP bug fixes + TUI performance optimizations** — protocol compliance, needs_redraw, idle-only background work

### Git (next)
- [x] **Stage hunks** — `]c`/`[c` hunk navigation, `gs`/`:Ghs` stages hunk under cursor via `git apply --cached`

### Editor features
- [x] **Auto-indent** — copies current line's leading whitespace on Enter/o/O; `auto_indent` setting (default: true)
- [x] **Completion menu** — Ctrl-N/Ctrl-P word completion from buffer in insert mode; floating popup in GTK + TUI
- [x] **:set command** — runtime setting changes; write-through to settings.json; number/rnu/expandtab/tabstop/shiftwidth/autoindent/incsearch; query with `?`
- [x] **`ip`/`ap` + `is`/`as` text objects** — paragraph and sentence text objects for all operators and visual mode
- [x] **VSCode-style project search** — Ctrl-Shift-F panel; regex/case/whole-word toggles; `.gitignore`-aware (ignore crate); grouped results, click to open
- [ ] **:grep / :vimgrep** — project-wide search, populate quickfix list *(lower priority)*
- [ ] **Quickfix window** — `:copen`, `:cn`, `:cp` quickfix navigation *(lower priority)*
- [ ] **`it`/`at` tag text objects** — inner/around HTML/XML tag

### Big features
- [x] **LSP support** — completions (Ctrl-Space), go-to-definition (gd), hover (K), diagnostics (]d/[d), auto-detect servers on PATH
- [ ] **`:norm`** — execute normal command on a range of lines

## Recent Work
**Session 48:** LSP bug fixes + TUI performance — fixed full LSP lifecycle: `notify_did_open` now returns `Result<(), String>` with descriptive errors; added `pending_requests: Arc<Mutex<HashMap<i64, String>>>` for deterministic response routing (no more guessing response type by content); added initialization guards on all notification methods (no `didOpen`/`didChange`/`didSave`/`didClose` before server handshake completes); reader thread now responds to server-initiated requests (`window/workDoneProgress/create`) and handles error responses properly. Performance fixes: diagnostic flood optimization (pre-computed visible paths set, events capped at 50/poll, only redraw for visible buffers); fixed path mismatch between LSP diagnostic keys (absolute) and buffer file_path (relative) via canonicalization at lookup points in render.rs, diagnostic_counts(), and jump_next/prev_diagnostic(). TUI: added `needs_redraw` flag for on-demand rendering (was unconditional 50 FPS); moved all background work (LSP flush/poll, search poll, replace poll) to idle-only periods (no pending input) — eliminated blocking pipe writes during typing; adaptive poll timeout (1ms when redraw pending, 50ms idle). Reverted `Stdio::inherit()` to `Stdio::null()` for child process stderr (rust-analyzer stderr was corrupting TUI display). (495 tests, no change.)

**Session 47:** LSP support — full Language Server Protocol integration using lightweight custom client (`std::thread` + `mpsc`, no tokio). New files: `src/core/lsp.rs` (~750 lines, protocol transport + single-server client with 27 unit tests), `src/core/lsp_manager.rs` (~340 lines, multi-server coordinator with built-in registry for rust-analyzer/pyright/typescript-language-server/gopls/clangd). Engine gains LSP fields and lifecycle hooks (didOpen/didChange/didSave/didClose), `poll_lsp()` event processing, diagnostic navigation (`]d`/`[d`), go-to-definition (`gd`), hover popup (`K`), LSP completion (`Ctrl-Space`). Render layer: `DiagnosticMark` + `HoverPopup` types, diagnostic_gutter map, theme colors (error/warning/info/hint/hover). GTK backend: wavy underlines via Cairo curves, colored gutter dots, hover popup, LSP poll in SearchPollTick, shutdown on quit. TUI backend: colored underlines + E/W/I/H gutter chars, hover popup, LSP poll in event loop, shutdown on quit. Settings: `lsp_enabled` bool + `lsp_servers` array for custom servers; `:set lsp`/`:set nolsp`; `:LspInfo`/`:LspRestart`/`:LspStop` commands. (458→495 tests, 37 new.)

**Session 46:** TUI scrollbar drag fix — removed deferred `pending_h_scroll` mechanism so h-scrollbar drag updates immediately (matching v-scrollbar behaviour); added drag event coalescing (consecutive `MouseEventKind::Drag` events are drained via `poll(Duration::ZERO)`, only the final position is rendered) benefiting all drag operations (h-scrollbar, v-scrollbar, sidebar resize); unified scrollbar thumb colour to `Rgb(128, 128, 128)` grey across vertical and horizontal scrollbars. (458 tests, no change.)

**Session 45:** Replace across files — added `replace_in_project()` to `project_search.rs`: walks files via `ignore` crate, applies `regex::replace_all`, writes back only changed files; uses `NoExpand` wrapper in literal mode to prevent `$1` backreference expansion; files in `skip_paths` (dirty buffers) are skipped and reported. New `ReplaceResult` struct with counts and file lists. Extracted `build_search_regex()` helper shared by search and replace. Engine gains `project_replace_text`, `start_project_replace` (async), `poll_project_replace`, `apply_replace_result` (reloads open buffers, clears undo stacks, refreshes git diff). GTK: Replace `Entry` + "Replace All" button; 2 new `Msg` variants; replace poll piggybacked on `SearchPollTick`. TUI: `replace_input_focused` field; `Tab` switches inputs; `Enter` in replace box triggers replace; `Alt+H` shortcut; new `[Replace…]` row; all layout offsets shifted +1; mouse handling updated. (444→458 tests, 14 new: 9 replace + 5 engine.)

**Session 44:** Enhanced project search — rewrote `project_search.rs` to use the `ignore` crate (same walker as ripgrep) for `.gitignore` support and the `regex` crate for pattern matching. Added `SearchOptions` struct with three toggles: case-sensitive (`Aa`), whole word (`Ab|`), and regex (`.*`). Results capped at 10,000 with status message indication. Engine gains `project_search_options` field and 3 toggle methods; async search thread now sends `Result<Vec<ProjectMatch>, SearchError>` for invalid-regex error handling. GTK: 3 `ToggleButton` widgets with CSS styling (dim inactive / blue active) between search input and status label; 3 new `Msg` variants. TUI: `Alt+C`/`Alt+W`/`Alt+R` toggle keys in both input and results mode; toggle indicator row replaces blank separator with active/inactive coloring and hint text. (438→444 tests, 6 new: case-sensitive, whole-word, regex, invalid-regex, whole-word+regex combo, gitignore-respected.)

**Session 43:** Search panel bug fixes — GTK: fixed search results appearing with light background and grey text by correcting CSS selectors (`listbox` → `.search-results-list`, `.search-results-list > row`; GTK4 uses `list` as the CSS node name, not `listbox`); fixed startup crash in `sync_scrollbar` when initial resize fires with near-zero dimensions (added early return guard and clamped `height-request` to non-negative). TUI: added scrollbar drag support for search results (new `SidebarScrollDrag` struct); `j`/`k` in search results now call `ensure_search_selection_visible` to keep selection in viewport. (438 tests, no change.)

**Session 42:** Search panel polish + CI fix — TUI: redesigned search scroll to track viewport independently of selection (`search_scroll_top` driven by scroll wheel/scrollbar click; selection only adjusts scroll when it leaves the viewport); scrollbar column clicks now jump-scroll both Explorer and Search panels; scroll wheel scrolls sidebar content; removed unused `DisplayRow.result` field. GTK: dark background fixed via `.search-results-scroll > viewport { background-color: #252526; }` and `.search-results-list label { color: #cccccc; }`; overlay scrolling disabled on search results ScrolledWindow so scrollbar is always visible. Both backends: search now runs on a background thread (`start_project_search` + `poll_project_search` — 50 ms latency); GTK polls via `glib::timeout_add_local`; TUI polls each frame before `ct_event::poll()`. CI clippy fix: two `map_or(false, ...)` → `is_some_and(...)` in `engine.rs` (new `unnecessary_map_or` lint in Rust 1.84+). Also: 4 new engine-level project-search tests covering sync, empty query, select prev/next, and async poll. (434 → 438 tests.)

**Session 41:** VSCode-style project search — new `src/core/project_search.rs` with `ProjectMatch` struct and `search_in_project()` (recursive walk, skip hidden/binary, case-insensitive, sorted by path then line). Engine gets 3 new fields (`project_search_query`, `project_search_results`, `project_search_selected`) and 3 new methods (`run_project_search`, `project_search_select_next/prev`). GTK: Search activity bar button enabled (Ctrl-Shift-F), Search panel with `gtk4::Entry` input + `gtk4::ListBox` results (file-header rows + result rows, click opens file at matched line). TUI: `TuiPanel::Search` added; activity bar gains Search icon at row 1 (Settings moves to row 2); `search_input_mode` field on `TuiSidebar`; `render_search_panel()` renders `[query]` input box, status line, scrollable results grouped by file; keyboard handling for input mode (type/Backspace/Enter) and results mode (j/k/Enter). (429→434 tests, 5 new project-search unit tests.)

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
