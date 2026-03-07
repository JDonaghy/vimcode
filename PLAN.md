# VimCode Implementation Plan

> Session history archived in **SESSION_HISTORY.md**. Recent work summary in **PROJECT_STATE.md**.

---

## Recently Completed
- **Session 137**: Operator+motion completeness — `df`/`dt`/`dF`/`dT`/`d;`/`d,`/`dj`/`dk`/`dG`/`dgg`/`d{`/`d}`/`d(`/`d)`/`dW`/`dB`/`dE`/`d^`/`dh`/`dl`/`dH`/`dM`/`dL`/`dge` (all operators: `d`/`c`/`y`/`g~`/`gu`/`gU`/`>`/`<`/`=`), generic `apply_charwise_operator`/`apply_linewise_operator` helpers, 56 integration tests
- **Session 136**: `normalize_ex_command()` system (57-entry abbreviation table), ~20 new ex commands (`:join`, `:yank`, `:put`, `:>/<`, `:=`, `:#`, `:mark`/`:k`, `:pwd`, `:file`, `:enew`, `:update`, `:version`, `:print`, `:number`, `:new`, `:vnew`, `:retab`, `:cquit`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, `:display`), `:copy` conflict fix, `QuitWithError` action, 71 integration tests
- **Session 135**: `show_hidden_files` setting (explorer/fuzzy/folder picker), LSP format undo fix, stale highlighting after format fix
- **Session 134**: Search highlight refresh after edits, Escape clears highlights, extra line number fix, markdown preview wrap, TUI/GTK viewport layout fixes, 13 integration tests

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
- [x] **Themes / plugin system** — named color themes selectable via `:colorscheme`; 4 built-in themes: onedark (default), gruvbox-dark, tokyo-night, solarized-dark (session 116)
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
- [x] **Settings sidebar (GTK)** — native GTK form with 30 settings in 7 categories, search, Adwaita dark theme (session 117b/117c)

### Extension System
- [x] **Extension mechanism** — Lua 5.4 plugin sandbox; plugins register commands/keymaps/hooks, read/write buffer text, show messages; `~/.config/vimcode/plugins/` auto-loaded; bundled language-pack extensions + GitHub registry; `:ExtInstall/:ExtList/:ExtEnable/:ExtDisable/:ExtRemove` (sessions 98, 113–114)

### AI Integration
- [x] **AI assistant panel** — sidebar chat panel; configurable provider (Anthropic Claude, OpenAI, Ollama local); `ai_provider`/`ai_api_key`/`ai_model`/`ai_base_url` in settings; activity bar chat icon opens panel; multi-turn conversation; `:AI <msg>` and `:AiClear` commands (session 118)
- [x] **AI inline completions** — ghost-text completions from AI provider interleaved with LSP ghost text; separate `ai_completions` setting (default false to avoid unexpected API costs); debounced after 500ms idle in insert mode; Tab accepts whole suggestion, `Alt-]`/`Alt-[` cycle through alternatives
- [x] **Swap file crash recovery** — Vim-like swap files (`~/.config/vimcode/swap/`); FNV-1a path hashing; atomic writes (`.tmp` + rename); PID-based stale detection; `[R]ecover/[D]elete/[A]bort` recovery dialog; `:set swapfile`/`:set updatetime=N`; periodic writes via `tick_swap_files()`; cleanup on shutdown
