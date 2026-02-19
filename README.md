# VimCode

High-performance Vim+VSCode hybrid editor in Rust. Modal editing meets modern UX, no GPU required.

## Vision

- **First-class Vim mode** — deeply integrated, not a plugin
- **Cross-platform** — GTK4 desktop UI + full terminal (TUI) backend
- **CPU rendering** — Cairo/Pango (works in VMs, remote desktops, SSH)
- **Clean architecture** — platform-agnostic core, 438 tests

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
- Search is case-insensitive; hidden files/directories (`.git`, etc.) and binary files are skipped
- Results are grouped by file (`filename.rs`) then listed as `  42: matched line text`
- **GTK:** click a result row to open the file at that line
- **TUI:** `j`/`k` to navigate results; `Enter` to open file at matched line; any printable char while in results mode switches back to input mode

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

### Settings (`:set` command)

Runtime changes are written through to `~/.config/vimcode/settings.json` immediately.

| Option | Aliases | Default | Description |
|--------|---------|---------|-------------|
| `number` / `nonumber` | `nu` | on | Absolute line numbers |
| `relativenumber` / `norelativenumber` | `rnu` | off | Relative line numbers (`number` + `relativenumber` = hybrid) |
| `expandtab` / `noexpandtab` | `et` | on | Tab key inserts spaces |
| `tabstop=N` | `ts` | 4 | Width of Tab key / tab display |
| `shiftwidth=N` | `sw` | 4 | Indent width (for future `>>` / `<<`) |
| `autoindent` / `noautoindent` | `ai` | on | Copy indent from current line on Enter/o/O |
| `incsearch` / `noincsearch` | `is` | on | Incremental search as you type |

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
- **Mouse support:** click-to-position, window switching, scroll wheel (targets pane under cursor), scrollbar click-to-jump and drag
- **Sidebar resize:** drag separator column; `Alt+Left` / `Alt+Right` keyboard resize (min 15, max 60 cols)
- **Scrollbars:** `█` / `░` thumb/track; vsplit separator doubles as left-pane vertical scrollbar; horizontal scrollbar row when content wider than viewport; `┘` corner when both axes present
- **Scroll sync:** `:Gblame` pairs stay in sync across keyboard nav and mouse events
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
| `v` / `V` / `Ctrl-V` | Visual / Visual Line / Visual Block |
| `/` / `?` | Search forward / backward |
| `n` / `N` | Next / previous match |
| `m{a-z}` / `'{a-z}` | Set mark / jump to mark |
| `q{a-z}` / `@{a-z}` | Record macro / play macro |
| `gt` / `gT` | Next / previous tab |
| `gs` | Stage hunk (in `:Gdiff` buffer) |
| `]c` / `[c` | Next / previous hunk |
| `za` / `zo` / `zc` / `zR` | Fold toggle / open / close / open all |
| `Ctrl-W h/j/k/l` | Focus window left/down/up/right |
| `Ctrl-W w` / `c` / `o` | Cycle / close / close-others |

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
| `:set [option]` | Change / query setting |
| `:Gdiff` / `:Gstatus` | Git diff / status |
| `:Gadd` / `:Gadd!` | Stage file / stage all |
| `:Gcommit <msg>` | Commit |
| `:Gpush` | Push |
| `:Gblame` | Blame (scroll-synced split) |
| `:Ghs` / `:Ghunk` | Stage hunk under cursor |
| `:config reload` | Reload settings from disk |

---

## Architecture

```
src/
├── main.rs          (~3260 lines)  GTK4/Relm4 UI, rendering, sidebar resize, search panel
├── tui_main.rs      (~2620 lines)  ratatui/crossterm TUI backend, search panel
├── render.rs        (~1080 lines)  Platform-agnostic ScreenLayout bridge
├── icons.rs            (~30 lines)  Nerd Font file-type icons (GTK + TUI)
└── core/            (~17150 lines)  Zero GTK/rendering deps — fully testable
    ├── engine.rs                    Orchestrator: keys, commands, git, macros, project search
    ├── project_search.rs            Recursive file search (case-insensitive, no regex)
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
| Config | serde + serde_json |

## License

TBD
