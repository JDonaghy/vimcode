# VimCode Implementation Plan

> Session history archived in **SESSION_HISTORY.md**. Recent work summary in **PROJECT_STATE.md**.

---

## Recently Completed
- **Session 159**: Tree-sitter upgrade (0.20→0.24) + YAML/HTML syntax highlighting (17 languages), TUI tab expansion fix, TUI activity bar icon readability, YAML key/value color fix, C# query fixes, v0.3.2
- **Session 158**: VSCode Mode Gap Closure Phases 1–3 — Alt key routing, line operations (move/duplicate/delete/insert), multi-cursor (Ctrl+D/Ctrl+Shift+L with extra selections rendering, same-line char-index correctness), indentation (Ctrl+]/[, Shift+Tab, multi-cursor aware), panel toggles (Ctrl+J/Ctrl+`/Ctrl+B/Ctrl+,), quick navigation (Ctrl+G/P/Shift+P), Ctrl+K chord prefix, GTK terminal mouse fix (+1→+2 for tab bar row), bottom panel sans-serif UI font, 55 tests
- **Session 157**: VSCode mode fixes (auto-pairs, bracket matching, update_bracket_match in handle_vscode_key), build portability (vcd musl static linking, Flatpak Rust 1.80 compat), v0.3.1 release
- **Session 156**: IDE Polish — indent guides (vertical lines at tabstops, active guide highlight, blank line bridging), bracket pair highlighting (matching `(){}[]` background color), auto-close brackets/quotes (skip-over, pair backspace, smart quote context), 3 new settings + theme colors across all 4 themes, GTK+TUI rendering, 29 tests
- **Session 155**: Core Commentary Feature — unified comment toggling into `src/core/comment.rs` (46+ language table, two-pass algorithm, override chain), `:Comment`/`:Commentary` commands, `vimcode.set_comment_style()` plugin API, Ctrl+/ fix for GTK+TUI, VSCode Ctrl+Q/F10, 19+31 tests
- **Session 154**: Keymaps editor in settings panel — `BufferEditor` setting type, scratch buffer with validation, `:Keymaps` command, GTK button + TUI display, 11 tests
- **Session 153**: Richer Lua Plugin API + VimCode Commentary + User Keymaps — Extended plugin API (cursor write, settings access, state queries, buffer insert/delete, register write, 7 new autocmd events, `set_mode()` refactor, visual/command keymap fallbacks); VimCode Commentary bundled extension (gcc/gc/`:Commentary`, 40+ language comment strings, undo support); plugin `set_lines` undo fix; user-configurable keymaps in settings.json (`"mode keys :command"` format, multi-key sequences, override built-in keys, `{count}` substitution); 22 + 17 + 13 = 52 new tests (2801 total)
- **Session 152**: Visual paste — `p`/`P` in visual mode replaces selection with register content; `"x` register selection in visual mode; `Ctrl+Shift+V` clipboard paste in Normal/Visual (TUI+GTK); TUI tab bar fix (breadcrumbs y-offset); multi-group `Ctrl-W h/l` navigates between groups before overflowing to sidebar; pre-existing test fix (`swap_scan_stale`); 8 tests
- **Session 151**: Tab drag-to-split — VSCode-style drag tab to edge for new split, drag to center to move between groups, drag within tab bar to reorder; `DropZone`/`TabDragState` core types, 7 engine methods, GTK overlay rendering; tab bar draw order fix (windows before tab bars, dividers before tab bars); new `vim-code.svg` gradient logo, removed old icon files; 15 tests
- **Session 150**: Tab switcher polish — Alt+t binding (TUI+GTK), modifier-release auto-confirm (GTK polling + TUI timeout), sans-serif UI font for tabs/popup, tab click fix (breadcrumbs y-offset, Pango-measured hit zones, deferred tree highlight)
- **Session 149**: Ctrl+Tab MRU tab switcher (VSCode-style popup, forward/backward cycling, Enter confirms, Escape cancels) + `autohide_panels` TUI setting (auto-hide sidebar/activity bar, Ctrl-W h reveals)
- **Session 148**: Netrw in-buffer file browser — `:Explore`/`:Sexplore`/`:Vexplore` (and `:Ex`/`:Sex`/`:Vex` aliases), Enter opens files/dirs, `-` navigates to parent, header shows current dir, respects `show_hidden_files`, 16 tests
- **Session 147**: TUI interactive settings panel — replaced read-only list with full interactive form (filterable categories, bool toggles, enum cycling, inline string/int editing, Ctrl+V paste, DynamicEnum for colorscheme with custom themes), 10 tests
- **Session 146**: Breadcrumbs bar — file path + tree-sitter symbol hierarchy below tab bar, 10-language scope walking, `breadcrumbs` setting, per-group bars, GTK+TUI rendering, 14 tests
- **Session 145**: VSCode theme loader (drop `.json` in `~/.config/vimcode/themes/`, `:colorscheme name`), TUI crash fix (`byte_to_char_idx` multi-byte UTF-8 panic), swap recovery R/D/A fix for TUI, sidebar keyboard nav (`Ctrl-W h/l` toolbar↔sidebar↔editor), editor click clears sidebar focus, 4 theme tests
- **Session 143**: Bug fixes — `:q` dirty guard allows close when buffer visible in another split, `autoread` setting + file auto-reload detection (2s poll in GTK+TUI), `:new`/`:split` respect `splitbelow`/`splitright`, `:e!` reload from disk, 9 integration tests
- **Session 142**: Vim compat batch 3 — 15 new commands (94% → 97%), g?{motion} ROT13, CTRL-@, CTRL-V {char}, CTRL-O auto-return, !{motion}{filter}, CTRL-W H/J/K/L/T/x, visual block I/A, o_v/o_V force motion, 29 integration tests
- **Session 141**: Vim compat batch 2 — 27 new commands (85% → 94%), gq/gw format operators, ga/g8/go/gm/gM/gI/gx/g'/g`/g&, CTRL-^, CTRL-L, N%, zs/ze, CTRL-W p/t/b/f/d, insert CTRL-A/CTRL-G u/j/k, visual gq/g CTRL-A/g CTRL-X, :make, :b {name}, 38 integration tests
- **Session 140**: Vim compat batch — 29 new commands (78% → 85%), +/-/_/| motions, gp/gP, @:, backtick text objects, insert CTRL-E/Y, visual r{char}, &, CTRL-W resize/equalize/maximize, bracket/section/method navigation, 45 integration tests
- **Session 139**: Comprehensive z-commands — 15 new z-commands (32% → 96%), fold create/delete/recursive, horizontal scroll, 33 integration tests
- **Session 138**: `VIM_COMPATIBILITY.md` — systematic Vim command inventory (304/411 = 74%), VimScript scope note in README.md
- **Session 137**: Operator+motion completeness — all operators with all motions, 56 integration tests
- **Session 136**: Ex command abbreviation system + ~20 new commands, 71 integration tests
- **Session 135**: `show_hidden_files` setting, LSP format undo fix

## Roadmap

### Git
- [x] **Stage hunks** — `]c`/`[c` navigation, `gs`/`:Ghs` to stage hunk under cursor

### Editor Features
- [x] **`:set` command** — runtime setting changes; write-through to settings.json; number/rnu/expandtab/tabstop/shiftwidth/autoindent/incsearch; query with `?`
- [x] **`ip`/`ap` paragraph text objects** — inner/around paragraph (contiguous non-blank lines)
- [x] **`is`/`as` sentence text objects** — inner/around sentence (`.`/`!`/`?`-delimited)
- [x] **Enhanced project search** — regex/case/whole-word toggles; `.gitignore`-aware via `ignore` crate; 10k result cap; GTK toggle buttons + TUI Alt+C/W/R
- [x] **VSCode-style replace across files** — replace all matches in project; skip dirty buffers; reload open buffers; regex capture group backreferences
- [x] **`:grep` / `:vimgrep`** — project-wide search, populate quickfix list
- [x] **Quickfix window** — `:copen`, `:cn`, `:cp` navigation
- [x] **`it`/`at` tag text objects** — inner/around HTML/XML tag
- [x] **Vim-style ex command abbreviations** — `normalize_ex_command()` with 57-entry abbreviation table; ~20 new ex commands (`:join`, `:yank`, `:put`, `:mark`, `:retab`, `:cquit`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, etc.)

### Big Features
- [x] **LSP support** — completions, go-to-definition, hover, diagnostics (session 47 + 48 bug fixes)
- [x] **`gd` / `gD`** — go-to-definition via LSP
- [x] **`:norm`** — execute normal command on a range of lines
- [x] **Fuzzy finder / Telescope-style** — Ctrl-P opens centered file-picker modal with subsequence scoring (session 53)
- [x] **Multiple cursors** — `Alt-D` (configurable) adds cursor at next match of word under cursor; all cursors receive identical keystrokes; Escape collapses to one
- [x] **Themes / plugin system** — named color themes selectable via `:colorscheme`; 4 built-in themes + VSCode `.json` theme import from `~/.config/vimcode/themes/` (sessions 116, 145)
- [x] **LSP semantic tokens** — `textDocument/semanticTokens/full` overlay on tree-sitter; 8 semantic theme colors; binary-search span overlay; legend caching (sessions 131–132)

### Enhanced Editor
- [x] **Autosuggestions (inline ghost text)** — as-you-type completions shown as dimmed ghost text inline after the cursor; sources: buffer word scan (sync) + LSP `textDocument/completion` (async); Tab accepts, any other key dismisses; coexists with Ctrl-N/Ctrl-P popup (ghost hidden when popup active)
- [x] **Edit mode toggle** — `editor_mode` setting (`"vim"` default | `"vscode"`); `:set mode=vscode`; `Alt-M` runtime toggle; Shift+Arrow selection, Ctrl+Arrow word nav, Ctrl-C/X/V/Z/Y/A shortcuts, Ctrl+/ comment toggle, smart Home; status bar shows EDIT/SELECT; session 66

### Terminal & Debugger
- [x] **Integrated terminal** — VSCode-style 13-row bottom panel; `portable-pty` + `vt100`; Ctrl-T toggle + `:term` command; full 256-color cell rendering; mouse selection; Nerd Font toolbar; shell session persists on close (session 68)
- [x] **Terminal: multiple tabs** — tab strip in toolbar; `Vec<TerminalPane>`; Alt-1–9 / click `[N]` to switch; auto-close on shell exit (session 72)
- [x] **Terminal: draggable panel height** — drag header row to resize; `session.terminal_panel_rows` persisted; clamped [5, 30] (session 71)
- [x] **Terminal: scrollback navigation** — `scroll_offset` + vt100 `set_scrollback()`; PgUp/PgDn while focused; scrollbar thumb tracks position (session 70)
- [x] **Terminal: find in panel** — Ctrl+F while terminal focused opens an inline find bar in the toolbar row; live match highlighting (orange active, amber others); Enter/Shift+Enter navigate matches; Escape closes
- [x] **Terminal: button bar (Add / Close)** — `+` creates a new tab; `×`/`󰅖` closes the active tab; click detection in both GTK and TUI backends
- [x] **Terminal: horizontal split view** — `⊞`/`󰤼` toolbar button toggles two panes side-by-side; independent PTY sessions; mouse click or Ctrl-W switches active pane; `│` divider
- [x] **Debugger (DAP)** — Transport + adapter registry + `:DapInstall` (S83); poll loop + breakpoint gutter + stopped-line highlight (S84); variables/call-stack/output panel (S85-86); VSCode-like UI with launch.json (S88); codelldb compat (S89); interactive sidebar + conditional breakpoints (S90)

### UI & Menus
- [x] **VSCode-style menus** — application menu bar (File / Edit / View / Go / Run / Terminal / Help) in GTK; command palette (`Ctrl-Shift-P`) lists all commands + key bindings; fuzzy-searchable; both GTK native menus and TUI pop-up menu overlay (sessions 81–82, 100–101)
- [x] **Command palette** — `Ctrl-Shift-P` floating modal; lists named commands with descriptions and current keybindings; typing filters; Enter executes; shared GTK + TUI (session 101)
- [x] **Settings editor** — `:Settings` opens `settings.json` in an editor tab; Settings sidebar panel shows live values; auto-reload on save in both backends (session 117)
- [x] **Settings sidebar (GTK + TUI)** — interactive form with 32 settings in 8 categories, search, live controls; GTK native widgets (session 117b/117c), TUI interactive form with keyboard nav + inline editing (session 147)

### Extension System
- [x] **Extension mechanism** — Lua 5.4 plugin sandbox; plugins register commands/keymaps/hooks, read/write buffer text, show messages; `~/.config/vimcode/plugins/` auto-loaded; bundled language-pack extensions + GitHub registry; `:ExtInstall/:ExtList/:ExtEnable/:ExtDisable/:ExtRemove` (sessions 98, 113–114)
- [x] **Keymap editor in settings panel** — "User Keymaps" row in the Settings sidebar opens a scratch buffer (one keymap per line, format `mode keys :command`). `:w` validates, updates `settings.keymaps`, calls `rebuild_user_keymaps()`. Also accessible via `:Keymaps` command. Tab shows `[Keymaps]`. GTK button + TUI "N defined ▸". 11 tests. (session 154)

### AI Integration
- [x] **AI assistant panel** — sidebar chat panel; configurable provider (Anthropic Claude, OpenAI, Ollama local); `ai_provider`/`ai_api_key`/`ai_model`/`ai_base_url` in settings; activity bar chat icon opens panel; multi-turn conversation; `:AI <msg>` and `:AiClear` commands (session 118)
- [x] **AI inline completions** — ghost-text completions from AI provider interleaved with LSP ghost text; separate `ai_completions` setting (default false to avoid unexpected API costs); debounced after 500ms idle in insert mode; Tab accepts whole suggestion, `Alt-]`/`Alt-[` cycle through alternatives
- [x] **Swap file crash recovery** — Vim-like swap files (`~/.config/vimcode/swap/`); FNV-1a path hashing; atomic writes (`.tmp` + rename); PID-based stale detection; `[R]ecover/[D]elete/[A]bort` recovery dialog; `:set swapfile`/`:set updatetime=N`; periodic writes via `tick_swap_files()`; cleanup on shutdown
