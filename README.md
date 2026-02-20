# VimCode

High-performance Vim+VSCode hybrid editor in Rust. Modal editing meets modern UX, no GPU required.

## Vision

- **First-class Vim mode** — deeply integrated, not a plugin
- **Cross-platform** — GTK4 desktop UI + full terminal (TUI) backend
- **CPU rendering** — Cairo/Pango (works in VMs, remote desktops, SSH)
- **Clean architecture** — platform-agnostic core, 563 tests, zero async runtime

## Building

**Prerequisites (GTK backend):**
```bash
# Debian/Ubuntu
sudo apt install libgtk-4-dev libpango1.0-dev

# Fedora
sudo dnf install gtk4-devel pango-devel

# Arch
sudo pacman -S gtk4 pango
```

```bash
cargo build
cargo run -- <file>          # GTK window
cargo run --bin vimcode-tui  # Terminal UI
cargo test -- --test-threads=1
cargo clippy -- -D warnings
cargo fmt
```

---

## Features

### Vim Editing

**Modes**
- Normal, Insert, Visual (character), Visual Line, Visual Block, Command, Search — 7 modes total

**Navigation**
- `hjkl` — character movement
- `w` / `b` / `e` / `ge` — forward/backward word start/end
- `{` / `}` — paragraph backward/forward
- `gg` / `G` — first/last line; `{N}gg` / `{N}G` — go to line N
- `0` / `$` — line start/end
- `f{c}` / `F{c}` / `t{c}` / `T{c}` — find/till character; `;` / `,` repeat
- `%` — jump to matching bracket (`(`, `)`, `[`, `]`, `{`, `}`)
- `Ctrl-D` / `Ctrl-U` — half-page down/up
- `Ctrl-F` / `Ctrl-B` — full-page down/up

**Operators** (combine with any motion or text object)
- `d` — delete
- `c` — change (delete + enter Insert)
- `y` — yank (copy)

**Standalone commands**
- `x` / `X` — delete character under/before cursor
- `dd` / `D` — delete line / delete to end of line
- `cc` / `C` — change line / change to end of line
- `yy` / `Y` — yank line
- `s` / `S` — substitute character / substitute line
- `r{c}` — replace character(s) under cursor
- `p` / `P` — paste after/before cursor
- `u` / `Ctrl-R` — undo/redo
- `U` — undo all changes on current line
- `.` — repeat last change
- `~` / (visual `u` / `U`) — toggle/lower/upper case

**Text objects**
- `iw` / `aw` — inner/around word
- `i"` / `a"`, `i'` / `a'` — inner/around quotes
- `i(` / `a(`, `i[` / `a[`, `i{` / `a{` — inner/around brackets
- `ip` / `ap` — inner/around paragraph (contiguous non-blank lines); `ap` includes trailing blank lines
- `is` / `as` — inner/around sentence (`.`/`!`/`?`-terminated); `as` includes trailing whitespace
- `it` / `at` — inner/around HTML/XML tag (`dit` deletes content, `dat` deletes element; case-insensitive, nesting-aware)

**Count prefix** — prepend any number to multiply: `5j`, `3dd`, `10yy`, `2w`, etc.

**Insert mode**
- `i` / `I` — insert at cursor / line start
- `a` / `A` — append at cursor / line end
- `o` / `O` — open line below/above
- `Ctrl-N` / `Ctrl-P` — word completion (cycles through buffer words)
- `Backspace` — delete left; joins lines at start of line
- Tab key — inserts spaces (width = `tabstop`) or literal `\t` (when `noexpandtab`)
- **Auto-indent** — Enter/`o`/`O` copy leading whitespace from current line

**Visual mode**
- `v` — character selection; `V` — line selection; `Ctrl-V` — block selection
- All operators work on selection: `d`, `c`, `y`, `u`, `U`, `~`
- Block mode: rectangular selections, change/delete/yank uniform columns
- `gv` — reselect last visual selection (via `ge` → visual restore)

**Search**
- `/` — forward incremental search (real-time highlight as you type)
- `?` — backward incremental search
- `n` / `N` — next/previous match (direction-aware)
- Escape cancels and restores cursor position

**Marks**
- `m{a-z}` — set file-local mark
- `'{a-z}` — jump to mark line
- `` `{a-z} `` — jump to exact mark position
- Marks stored per-buffer

**Macros**
- `q{a-z}` — start recording into register; `q` — stop
- `@{a-z}` — play back; `@@` — repeat last; `{N}@{a}` — play N times
- Records all keys: navigation, Ctrl combos, special keys, Insert mode content, search

**Registers**
- `"` — unnamed (default)
- `"{a-z}` — named registers (`"ay` yank into `a`, `"ap` paste from `a`)
- Registers preserve linewise/characterwise type

**Find/Replace**
- `:s/pattern/replacement/[flags]` — substitute on current line
- `:%s/pattern/replacement/[flags]` — all lines
- `:'<,'>s/...` — visual selection range
- Flags: `g` (global), `i` (case-insensitive)
- `Ctrl-F` — VSCode-style dialog (live search, replace, replace all)
- Full undo/redo support

**Code Folding**
- `za` — toggle fold; `zo` — open; `zc` — close; `zR` — open all
- Indentation-based fold detection
- `+` / `-` gutter indicators; entire gutter column is clickable
- Fold state is per-window (two windows on same buffer can have different folds)

**Hunk navigation (diff buffers)**
- `]c` / `[c` — jump to next/previous `@@` hunk in a `:Gdiff` buffer

---

### Multi-File Editing

**Buffers**
- `:bn` / `:bp` — next/previous buffer
- `:b#` — alternate buffer
- `:ls` — list buffers (shows `[Preview]` suffix for preview tabs)
- `:bd` — delete buffer

**Windows**
- `:split` / `:vsplit` — horizontal/vertical split
- `Ctrl-W h/j/k/l` — move focus between panes
- `Ctrl-W w` — cycle focus; `Ctrl-W c` — close; `Ctrl-W o` — close others
- `Ctrl-W s/v` — split (same as `:split`/`:vsplit`)

**Tabs**
- `:tabnew` — new tab; `:tabclose` — close tab
- `gt` / `gT` or `g` + `t` / `T` — next/previous tab

**Quit / Save**
- `:w` — save; `:wq` — save and quit
- `:q` — close tab (quits if last tab; blocked if dirty)
- `:q!` — force-close tab
- `:qa` / `:qa!` — close all tabs (blocked / force)
- `Ctrl-S` — save in any mode without changing mode

---

### Project Search

- `Ctrl-Shift-F` — open search panel (or click the search icon in the activity bar)
- Type a query and press `Enter` to search all text files under the project root
- Respects `.gitignore` rules (powered by the `ignore` crate — same walker as ripgrep)
- Hidden files/directories and binary files are skipped; results capped at 10,000
- Results are grouped by file (`filename.rs`) then listed as `  42: matched line text`
- **Toggle buttons** (VS Code style):
  - `Aa` — Match Case (case-sensitive search)
  - `Ab|` — Match Whole Word (`\b` word boundaries)
  - `.*` — Use Regular Expression (full regex syntax)
- **Replace across files:** type replacement text in the Replace input; click "Replace All" (GTK) or press `Enter` in the replace box / `Alt+H` (TUI) to substitute all matches on disk
  - Regex mode: `$1`, `$2` capture group backreferences work in replacement text
  - Literal mode: `$` in replacement is treated literally (no backreference expansion)
  - Files with unsaved changes (dirty buffers) are skipped and reported in the status message
  - Open buffers for modified files are automatically reloaded from disk after replace
- **GTK:** click toggle buttons below the search input; click a result to open the file; `Tab` or click to switch between search/replace inputs
- **TUI:** `Alt+C` (case), `Alt+W` (whole word), `Alt+R` (regex), `Alt+H` (replace all); `Tab` to switch between search/replace inputs; `j`/`k` to navigate results; `Enter` to open

---

### Fuzzy File Finder

- `Ctrl-P` (Normal mode) — open the Telescope-style fuzzy file picker
- A centered floating modal appears over the editor
- Type to instantly filter all project files by fuzzy subsequence match
- Word-boundary matches (after `/`, `_`, `-`, `.`) are scored higher
- `Ctrl-N` / `↓` and `Ctrl-P` / `↑` — navigate results; `Enter` — open selected file; `Escape` — close
- Results capped at 50; hidden dirs (`.git`, etc.) and `target/` are excluded

---

### Live Grep

- `Ctrl-G` (Normal mode) — open the Telescope-style live grep modal
- A centered floating two-column modal appears over the editor
- Type to instantly search file *contents* across the entire project (live-as-you-type, query ≥ 2 chars)
- Left pane shows results in `filename.rs:N: snippet` format; right pane shows ±5 context lines around the match
- Match line is highlighted in the preview pane
- `Ctrl-N` / `↓` and `Ctrl-P` / `↑` — navigate results; preview updates as you move; `Enter` — open file at match line; `Escape` — close
- Results capped at 200; uses `.gitignore`-aware search (same engine as project search panel)

---

### File Explorer

- `Ctrl-B` — toggle sidebar; `Ctrl-Shift-E` — focus explorer
- Tree view with Nerd Font file-type icons
- `j` / `k` — navigate; `l` or `Enter` — open file/expand; `h` — collapse
- `a` — create file; `A` — create folder; `D` — delete; `R` — refresh
- **Preview mode:**
  - Single-click → preview tab (italic/dimmed, replaced by next single-click)
  - Double-click → permanent tab
  - Edit or save → auto-promotes to permanent
  - `:ls` shows `[Preview]` suffix
- Active file highlighted; parent folders auto-expanded

---

### Git Integration

**Gutter markers**
- `▌` in green — added lines; `▌` in yellow — modified lines
- Refreshed automatically on file open and save

**Status bar**
- Current branch name shown as `[branch-name]`

**Commands**
| Command | Aliases | Description |
|---------|---------|-------------|
| `:Gdiff` | `:Gd` | Open unified diff in vertical split |
| `:Gstatus` | `:Gs` | Open `git status` in vertical split |
| `:Gadd` | `:Ga` | Stage current file (`git add`) |
| `:Gadd!` | `:Ga!` | Stage all changes (`git add -A`) |
| `:Gcommit <msg>` | `:Gc <msg>` | Commit with message |
| `:Gpush` | `:Gp` | Push current branch |
| `:Gblame` | `:Gb` | Open `git blame` in scroll-synced vertical split |
| `:Ghs` | `:Ghunk` | Stage hunk under cursor (in a `:Gdiff` buffer) |

**Hunk staging workflow**
1. `:Gdiff` — open diff in a vertical split
2. `]c` / `[c` — navigate between hunks
3. `gs` or `:Ghs` — stage the hunk under the cursor via `git apply --cached`

---

### LSP Support (Language Server Protocol)

Automatic language server integration — open a file and diagnostics, completions, go-to-definition, and hover just work if the appropriate server is on `PATH`.

**Built-in server registry** (auto-detected on `PATH`):

| Language | Server |
|----------|--------|
| Rust | `rust-analyzer` |
| Python | `pyright-langserver` |
| JavaScript / TypeScript | `typescript-language-server` |
| Go | `gopls` |
| C / C++ | `clangd` |

**Features:**
- **Inline diagnostics** — wavy underlines (GTK) / colored underlines (TUI) with severity-colored gutter icons
- **Diagnostic navigation** — `]d` / `[d` jump to next/previous diagnostic
- **LSP completions** — `Ctrl-Space` in insert mode triggers server completions (merges with existing buffer word completion)
- **Go-to-definition** — `gd` jumps to the definition of the symbol under the cursor
- **Hover info** — `K` shows type/documentation popup above the cursor
- **Diagnostic counts** — `E:N W:N` shown in status bar

**Commands:**

| Command | Action |
|---------|--------|
| `:LspInfo` | Show running servers and their status |
| `:LspRestart` | Restart server for current file type |
| `:LspStop` | Stop server for current file type |

**Settings:**
- `:set lsp` / `:set nolsp` — enable/disable LSP (default: enabled)
- Custom servers in `settings.json`:
```json
{
    "lsp_servers": [
        { "command": "lua-language-server", "args": [], "languages": ["lua"] }
    ]
}
```

---

### Settings (`:set` command)

Runtime changes are written through to `~/.config/vimcode/settings.json` immediately.

| Option | Aliases | Default | Description |
|--------|---------|---------|-------------|
| `number` / `nonumber` | `nu` | on | Absolute line numbers |
| `relativenumber` / `norelativenumber` | `rnu` | off | Relative line numbers (`number` + `relativenumber` = hybrid) |
| `expandtab` / `noexpandtab` | `et` | on | Tab key inserts spaces |
| `tabstop=N` | `ts` | 4 | Width of Tab key / tab display |
| `shiftwidth=N` | `sw` | 4 | Indent width for `>>` / `<<` |
| `autoindent` / `noautoindent` | `ai` | on | Copy indent from current line on Enter/o/O |
| `incsearch` / `noincsearch` | `is` | on | Incremental search as you type |
| `lsp` / `nolsp` | | on | Enable/disable LSP language servers |

- `:set option?` — query current value (e.g. `:set ts?` → `tabstop=4`)
- `:set` (no args) — show one-line summary of all settings
- `:config reload` — reload settings file from disk

---

### Session Persistence

All state lives in `~/.config/vimcode/`:

| File | Contents |
|------|----------|
| `settings.json` | Editor options |
| `session.json` | Open files, cursor/scroll positions, history, window geometry |

- **Open files restored on startup** — each file reopened in its own tab; files closed via `:q` are excluded next session
- **Cursor + scroll position** — restored per file on reopen
- **Command history** — Up/Down arrows in command mode; max 100 entries; `Ctrl-R` reverse incremental search
- **Search history** — Up/Down arrows in search mode; max 100 entries
- **Tab auto-completion** in command mode
- **Window geometry** — size saved on close, restored on startup
- **Explorer visibility** — open/closed state persisted

---

### Rendering

**Syntax highlighting** (Tree-sitter, auto-detected by extension)
- Rust (`.rs`), Python (`.py`), JavaScript (`.js`/`.ts`), Go (`.go`), C++ (`.cpp`/`.hpp`/`.h`)

**Line numbers** — absolute / relative / hybrid (both on = hybrid)

**Scrollbars** (GTK + TUI)
- Per-window vertical scrollbar with cursor position indicator
- Per-window horizontal scrollbar (shown when content is wider than viewport)
- Scrollbar click-to-jump and drag support

**Font** — configurable family and size via `settings.json`

---

### TUI Backend (Terminal UI)

Full editor in the terminal via ratatui + crossterm — feature-parity with GTK.

- **Layout:** activity bar (3 cols) | sidebar | editor area; status line + command line full-width at bottom
- **Sidebar:** same file explorer as GTK with Nerd Font icons
- **Mouse support:** click-to-position, window switching, scroll wheel (targets pane under cursor), scrollbar click-to-jump and drag; drag event coalescing for smooth scrollbar tracking
- **Sidebar resize:** drag separator column; `Alt+Left` / `Alt+Right` keyboard resize (min 15, max 60 cols)
- **Scrollbars:** `█` / `░` thumb/track in uniform grey; vsplit separator doubles as left-pane vertical scrollbar; horizontal scrollbar row when content wider than viewport; `┘` corner when both axes present
- **Scroll sync:** `:Gblame` pairs stay in sync across keyboard nav and mouse events
- **Frame rate cap:** renders limited to ~60fps so rapid LSP or search events don't peg the CPU
- **Cursor shapes:** bar `|` in Insert mode, underline `_` in replace (`r`)

---

## Key Reference

### Normal Mode

| Key | Action |
|-----|--------|
| `hjkl` | Move left/down/up/right |
| `w` / `b` / `e` / `ge` | Word forward/back start/end |
| `{` / `}` | Paragraph backward/forward |
| `gg` / `G` | First / last line |
| `0` / `$` | Line start / end |
| `f{c}` / `t{c}` | Find / till char (`;` `,` repeat) |
| `%` | Jump to matching bracket |
| `zz` / `zt` / `zb` | Scroll cursor to center / top / bottom |
| `Ctrl-O` / `Ctrl-I` | Jump list back / forward |
| `Ctrl-D` / `Ctrl-U` | Half-page down / up |
| `i` / `I` / `a` / `A` | Insert (cursor / line-start / append / line-end) |
| `o` / `O` | Open line below / above |
| `x` / `dd` / `D` | Delete char / line / to EOL |
| `yy` / `Y` | Yank line |
| `p` / `P` | Paste after / before |
| `u` / `Ctrl-R` | Undo / redo |
| `U` | Undo all changes on line |
| `.` | Repeat last change |
| `r{c}` | Replace character |
| `~` | Toggle case of char under cursor (count supported) |
| `J` | Join lines (collapse next line's whitespace to one space) |
| `>>` / `<<` | Indent / dedent line(s) by `shiftwidth` |
| `*` / `#` | Search forward / backward for word under cursor |
| `v` / `V` / `Ctrl-V` | Visual / Visual Line / Visual Block |
| `/` / `?` | Search forward / backward |
| `n` / `N` | Next / previous match |
| `m{a-z}` / `'{a-z}` | Set mark / jump to mark |
| `q{a-z}` / `@{a-z}` | Record macro / play macro |
| `gt` / `gT` | Next / previous tab |
| `gd` | Go to definition (LSP) |
| `gs` | Stage hunk (in `:Gdiff` buffer) |
| `K` | Show hover info (LSP) |
| `]c` / `[c` | Next / previous hunk |
| `]d` / `[d` | Next / previous diagnostic (LSP) |
| `za` / `zo` / `zc` / `zR` | Fold toggle / open / close / open all |
| `Ctrl-W h/j/k/l` | Focus window left/down/up/right |
| `Ctrl-W w` / `c` / `o` | Cycle / close / close-others |
| `Ctrl-P` | Open fuzzy file finder |
| `Ctrl-G` | Open live grep modal (search file contents) |

### Command Mode

| Command | Action |
|---------|--------|
| `:w` / `:wq` | Save / save and quit |
| `:q` / `:q!` / `:qa` / `:qa!` | Quit / force / all / force-all |
| `:e <file>` | Open file |
| `:split` / `:vsplit` | Horizontal / vertical split |
| `:tabnew` / `:tabclose` | New tab / close tab |
| `:bn` / `:bp` / `:b#` | Buffer next / prev / alternate |
| `:ls` / `:bd` | List buffers / delete buffer |
| `:s/pat/rep/[gi]` | Substitute on line |
| `:%s/pat/rep/[gi]` | Substitute all lines |
| `:norm[al][!] {keys}` | Execute normal-mode keys on current line |
| `:[range]norm {keys}` | Execute on range (`%` all, `N,M` lines, `'<,'>` visual) |
| `:set [option]` | Change / query setting |
| `:Gdiff` / `:Gstatus` | Git diff / status |
| `:Gadd` / `:Gadd!` | Stage file / stage all |
| `:Gcommit <msg>` | Commit |
| `:Gpush` | Push |
| `:Gblame` | Blame (scroll-synced split) |
| `:Ghs` / `:Ghunk` | Stage hunk under cursor |
| `:LspInfo` | Show running LSP servers |
| `:LspRestart` | Restart server for current language |
| `:LspStop` | Stop server for current language |
| `:config reload` | Reload settings from disk |

---

## Architecture

```
src/
├── main.rs          (~3700 lines)  GTK4/Relm4 UI, rendering, sidebar resize, fuzzy popup
├── tui_main.rs      (~3100 lines)  ratatui/crossterm TUI backend, fuzzy popup
├── render.rs        (~1220 lines)  Platform-agnostic ScreenLayout bridge
├── icons.rs            (~30 lines)  Nerd Font file-type icons (GTK + TUI)
└── core/            (~16700 lines)  Zero GTK/rendering deps — fully testable
    ├── engine.rs                    Orchestrator: keys, commands, git, macros, LSP, project search/replace, fuzzy finder
    ├── lsp.rs                       LSP protocol transport + single-server client (request ID tracking, JSON-RPC framing)
    ├── lsp_manager.rs               Multi-server coordinator with initialization guards + built-in registry
    ├── project_search.rs            Regex/case/whole-word search + replace (ignore + regex crates)
    ├── buffer_manager.rs            Buffer lifecycle, undo/redo stacks
    ├── buffer.rs                    Rope-based text storage (ropey)
    ├── settings.rs                  JSON config, :set parsing
    ├── session.rs                   Session state persistence
    ├── git.rs                       Git subprocesses: diff, blame, stage_hunk
    └── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs
```

**Design rule:** `src/core/` has zero GTK/rendering dependencies and is testable in isolation.

## Tech Stack

| Component | Library |
|-----------|---------|
| Language | Rust 2021 |
| GTK UI | GTK4 + Relm4 |
| TUI UI | ratatui 0.27 + crossterm |
| Rendering | Pango + Cairo (CPU, no GPU) |
| Text | Ropey (rope data structure) |
| Parsing | Tree-sitter |
| LSP | lsp-types (protocol definitions) |
| Config | serde + serde_json |

## License

TBD
