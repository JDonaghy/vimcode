# VimCode

High-performance Vim+VSCode hybrid editor in Rust. Modal editing meets modern UX, no GPU required.

I started this project to see how far I could get with vibe-coding alone using Claude Code. I'm not going to read the code and I'm not going to review it either because I'd never have had the time for that. I wanted to find out what Claude could do if I just gave it the spec, or a description of a bug, and let it handle the rest. This is an experiment! The jury is still out. So far, however, I'm blown away. 

There's a touch of irony here - using a cli tool to write the editor that I've wanted for years and may never use because editors might not matter anymore. It is not ready for daily use. I'm still not using it for anything. Neovim is my daily driver and that will likely be the case for a while yet. We shall see!

## Vision

- **First-class Vim mode** — deeply integrated, not a plugin
- **Cross-platform** — GTK4 desktop UI + full terminal (TUI) backend
- **CPU rendering** — Cairo/Pango (works in VMs, remote desktops, SSH)
- **Clean architecture** — platform-agnostic core, 2346+ tests, zero async runtime dependency


## Download (Ubuntu)

Pre-built packages are published automatically on every push to `main`:

**[→ Download latest release](../../releases/tag/latest)**

**Option A — `.deb` package (recommended)**
```bash
sudo dpkg -i vimcode_*.deb
sudo apt -f install   # pulls in any missing GTK4 runtime libraries
```

**Option B — raw binary**
```bash
sudo apt install libgtk-4-1 libglib2.0-0 libpango-1.0-0 libcairo2
chmod +x vimcode-linux-x86_64
./vimcode-linux-x86_64
```

Requires **Ubuntu 22.04 or later** (GTK 4.6+). The `.deb` handles all runtime dependencies automatically.

---

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
cargo run -- <file>                         # GTK window
cargo run -- --tui <file>                   # Terminal UI (alias: -t)
cargo run -- --tui --debug /tmp/v.log       # TUI with debug log
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
- `g~{motion}` / `g~~` — toggle case of motion / entire line
- `gu{motion}` / `guu` — lowercase motion / entire line
- `gU{motion}` / `gUU` — uppercase motion / entire line
- `gn` / `gN` — visually select next/prev search match
- `cgn` — change next match (repeat with `.`)
- `g;` / `g,` — jump to previous/next change list position

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
- **Auto-popup completion** — suggestion popup appears automatically as you type; `Tab` accepts highlighted item; `Ctrl-N`/`Ctrl-P` or `Down`/`Up` cycle candidates without inserting; `Left`/`Escape` or any non-completion key dismisses; sources: buffer word scan (sync) + LSP (async)
- `Ctrl-Space` — manually trigger (or re-trigger) completion popup; configurable via `completion_keys.trigger`
- `Ctrl-N` / `Ctrl-P` / `Down` / `Up` — cycle completion candidates (display-only when auto-popup active; Ctrl-N/P inserts immediately when triggered manually)
- `Backspace` — delete left; joins lines at start of line
- Tab key — accepts auto-popup completion if active; otherwise inserts spaces (width = `tabstop`) or literal `\t` (when `noexpandtab`)
- **Auto-indent** — Enter/`o`/`O` copy leading whitespace from current line
- `Ctrl-W` — delete word backward from cursor
- `Ctrl-T` — indent current line by shiftwidth
- `Ctrl-D` — dedent current line by shiftwidth

**Visual mode**
- `v` — character selection; `V` — line selection; `Ctrl-V` — block selection
- All operators work on selection: `d`, `c`, `y`, `u`, `U`, `~`
- Block mode: rectangular selections, change/delete/yank uniform columns
- `o` — swap cursor to opposite end of selection (character/line visual)
- `O` — swap cursor to opposite column corner (visual block)
- `gv` — reselect last visual selection

**Search**
- `/` — forward incremental search (real-time highlight as you type)
- `?` — backward incremental search
- `n` / `N` — next/previous match (direction-aware; re-highlights after Escape)
- `Escape` in normal mode clears search highlights (same as `:noh`)
- `Escape` during search cancels and restores cursor position

**Marks**
- `m{a-z}` — set file-local mark; `m{A-Z}` — set global (cross-file) mark
- `'{a-z}/{A-Z}` — jump to mark line; `` `{a-z}/{A-Z} `` — jump to exact mark position
- `''` / ` `` ` — jump to position before last jump
- `'.` / `` `. `` — jump to last edit position
- `'<` / `'>` — jump to visual selection start/end
- Marks stored per-buffer (lowercase) or globally with filepath (uppercase)

**Macros**
- `q{a-z}` — start recording into register; `q` — stop
- `@{a-z}` — play back; `@@` — repeat last; `{N}@{a}` — play N times
- Records all keys: navigation, Ctrl combos, special keys, Insert mode content, search

**Registers & Clipboard**
- `"` — unnamed (default)
- `"{a-z}` — named registers (`"ay` yank into `a`, `"ap` paste from `a`)
- `"+` / `"*` — system clipboard registers (`"+y` yank to clipboard, `"+p` paste from clipboard)
- `"0` — yank-only register; every yank sets it, deletes do not
- `"1`–`"9` — delete history; each linewise/multi-line delete shifts 1→2→…→9
- `"-` — small-delete register; character-wise deletions less than one full line
- `"%` — current filename (read-only)
- `"/` — last search pattern (read-only)
- `".` — last inserted text (read-only)
- `"_` — black hole register (discard without affecting other registers)
- Registers preserve linewise/characterwise type
- `Ctrl-Shift-V` — paste clipboard in Command/Search/Insert mode (GTK); bracketed paste in TUI

**Find/Replace**
- `:s/pattern/replacement/[flags]` — substitute on current line
- `:%s/pattern/replacement/[flags]` — all lines
- `:'<,'>s/...` — visual selection range
- Flags: `g` (global), `i` (case-insensitive)
- `Ctrl-F` — VSCode-style dialog (live search, replace, replace all)
- Full undo/redo support

**Multiple Cursors**
- `Alt-D` (default) — add a secondary cursor at the next occurrence of the word under the cursor; press again to add the next match
- `Ctrl+Shift+L` (default) — add a cursor at **every** occurrence of the word under the cursor at once
- `Ctrl+Click` — plant a secondary cursor at the clicked position
- Enter insert mode and type — all cursors receive identical edits simultaneously
- `Escape` collapses all extra cursors and exits insert mode
- Keybindings configurable via `panel_keys.add_cursor` and `panel_keys.select_all_matches` in `settings.json`
- `Ctrl+Shift+L` requires a terminal with Kitty keyboard protocol support (Kitty, WezTerm, Alacritty, foot) in TUI mode

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

**Editor Groups (VSCode-style split panes, recursive)**
- `Ctrl+\` — split editor right (any group can be split again for nested layouts)
- `Ctrl-W e` / `Ctrl-W E` — split editor right / down
- `Ctrl+1` through `Ctrl+9` — focus group by position (tree order)
- `:EditorGroupFocus` / `:egf` — cycle focus to the next group
- `:EditorGroupClose` / `:egc` — close the active group (sibling promoted)
- `:EditorGroupMoveTab` / `:egmt` — move the current tab to the next group
- `Alt+,` / `Alt+.` (TUI) — resize the parent split of the active group
- Drag any divider (GTK) — resize that specific split

**Quit / Save**
- `:w` — save; `:wq` — save and quit
- `:q` — close tab (quits if last tab; blocked if dirty)
- `:q!` — force-close tab
- `:qa` / `:qa!` — close all tabs (blocked / force)
- `Ctrl-S` — save in any mode without changing mode

---

### Project Search

- `Alt+F` — focus search panel (or click the search icon in the activity bar)
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

### Quickfix Window

- `:grep <pattern>` / `:vimgrep <pattern>` — search project and populate the quickfix list; opens panel automatically
- `:copen` / `:cope` — open the quickfix panel with focus (shows all matches)
- `:cclose` / `:ccl` — close the quickfix panel
- `:cn` / `:cnext` — jump to next match (opens file, positions cursor)
- `:cp` / `:cprev` / `:cN` — jump to previous match
- `:cc N` — jump to Nth match (1-based)
- The quickfix panel is a **persistent bottom strip** (6 rows) above the status bar — not a floating modal
- When open with focus (`j`/`k`, `Ctrl-N`/`Ctrl-P` → navigate; `Enter` → jump and return focus to editor; `q`/`Escape` → close)

---

### Integrated Terminal

- `Ctrl-T` (Normal mode) — toggle the integrated terminal panel
- `:term` / `:terminal` — open a **new terminal tab** (always spawns a fresh shell, even if the panel is already open)
- The terminal is a **resizable bottom strip** (default 1 toolbar + 12 content rows) above the status bar; drag the header row up/down to resize; height persists across sessions
- Shell is determined by the `$SHELL` environment variable, falling back to `/bin/bash`; starts in the editor's working directory
- Full **ANSI/VT100 color support** — 256-color xterm palette rendered cell-by-cell
- **Multiple terminal tabs** — each tab runs an independent PTY; the toolbar shows `[1] [2] …` labels:
  - `Alt-1` through `Alt-9` (when terminal has focus) — switch to tab N
  - Click a `[N]` tab label in the toolbar — switch to that tab
  - Click the close icon (`󰅖`) — close the active tab; closes the panel if it was the last tab
  - When a shell exits (Ctrl-D, `exit`), its tab closes automatically
- **Mouse selection** — click and drag to select text in the terminal content area
- **Copy / Paste:**
  - `Ctrl-Y` — copy the current mouse selection to the system clipboard
  - `Ctrl-Shift-V` — paste from system clipboard into the running shell (GTK: intercepted by vimcode; TUI: Alacritty/kitty bracketed-paste is forwarded to the PTY automatically)
  - Mouse-release auto-copies the selection to the clipboard (requires `xclip` or `xsel` on Linux/X11)
- **Scrollback** — PageUp / PageDown scroll into history (up to 5 000 rows by default); the scrollbar is draggable; configurable via `"terminal_scrollback_lines"` in `settings.json`
- **Find in terminal** — `Ctrl-F` (while terminal has focus) opens an inline find bar in the toolbar row:
  - Type to set the query; matching text highlights live (orange = active match, amber = other matches)
  - `Enter` — next match; `Shift+Enter` — previous match; `Escape` or `Ctrl-F` — close find bar
  - Search is case-insensitive; covers all visible rows and the full scrollback history
- **Horizontal split** — click `󰤼` in the toolbar (or `Ctrl-W` when split is active) to toggle a side-by-side two-pane view:
  - Click either pane or press `Ctrl-W` to switch keyboard focus between panes
  - Drag the `│` divider left/right to resize the panes; both PTYs are resized on mouse release
- **Nerd Font toolbar** — tab strip + split (`󰤼`) and close (`󰅖`) icons
- **All keys forwarded to shell PTY** — Ctrl-C, Ctrl-D, Ctrl-L, Ctrl-Z, arrow keys, Tab, etc. work as expected
- `Ctrl-T` while the terminal has focus **closes the panel** while keeping all shell sessions alive; reopening restores the same sessions
- When a shell exits (Ctrl-D, `exit`, etc.) its tab closes immediately; the panel closes automatically when the last tab exits; clicking outside the terminal returns focus to the editor

---

### Debugger (DAP)

Built-in Debug Adapter Protocol support with a VSCode-like UI. Open the debug sidebar (click the bug icon in the activity bar), set breakpoints with `F9`, and press `F5` to start debugging.

**Supported adapters** (installed via `:DapInstall <lang>`):

| Language | Adapter | Type |
|----------|---------|------|
| Rust / C / C++ | codelldb | lldb |
| Python | debugpy | debugpy |
| Go | delve | go |
| JavaScript / TypeScript | js-debug | node |
| Java | java-debug | java |
| C# | netcoredbg | coreclr |

**Debug sidebar** — four interactive sections (Tab to switch, j/k to navigate, Enter to act, q/Escape to unfocus):
- **Variables** — local/scope variables with additional scope groups (e.g. Statics, Registers) as expandable headers; Enter expands/collapses nested children (recursive); C# private fields (`_name`, backing fields) automatically grouped under a collapsible **Non-Public Members** node
- **Watch** — user-defined watch expressions (`:DapWatch <expr>`); `x`/`d` removes selected
- **Call Stack** — all stack frames; Enter selects frame and navigates to source; active frame marked with `▶`
- **Breakpoints** — all set breakpoints with conditions shown; Enter jumps to location; `x`/`d` removes selected
- **Mouse** — click a section header to switch; click an item to select and activate it

**Conditional breakpoints** — breakpoints can have expression conditions, hit counts, or log messages:
- `:DapCondition <expr>` — stop only when `<expr>` is truthy (e.g. `:DapCondition x > 10`)
- `:DapHitCondition <count>` — stop after N hits (e.g. `:DapHitCondition >= 5`)
- `:DapLogMessage <msg>` — print message instead of stopping (logpoint)
- Run any command without arguments to clear the condition on the current line's breakpoint

**Bottom panel tabs** — `Terminal` and `Debug Output` tabs; debug output shows adapter diagnostics and program output with a scrollable history (mouse wheel + drag scrollbar; newest output shown at bottom by default).

**launch.json** — generated automatically in `.vimcode/launch.json` on first debug run; supports `${workspaceFolder}` substitution; existing `.vscode/launch.json` files are auto-migrated.

**tasks.json + preLaunchTask** — if a launch configuration has `"preLaunchTask": "build"`, VimCode loads `.vimcode/tasks.json` (auto-migrated from `.vscode/tasks.json`) and runs the matching task before starting the debug adapter. Task output appears in the Debug Output panel; if the task fails the debug session is aborted.

**Gutter indicators:**
- `●` — breakpoint set
- `◆` — conditional breakpoint (has condition or hit count)
- `▶` — current execution line (stopped)
- `◉` — breakpoint + current line

**Keys:**

| Key | Action |
|-----|--------|
| `F5` | Start debugging / continue |
| `Shift+F5` | Stop debugging |
| `F9` | Toggle breakpoint on current line |
| `F10` | Step over |
| `F11` | Step into |
| `Shift+F11` | Step out |
| `F6` | Pause |

**Commands:**

| Command | Action |
|---------|--------|
| `:DapInstall <lang>` | Install debug adapter for language |
| `:DapInfo` | Show detected DAP adapters |
| `:DapEval <expr>` | Evaluate expression in current frame |
| `:DapWatch <expr>` | Add watch expression |
| `:DapCondition [expr]` | Set/clear condition on breakpoint at cursor |
| `:DapHitCondition [count]` | Set/clear hit-count condition on breakpoint |
| `:DapLogMessage [msg]` | Set/clear logpoint on breakpoint at cursor |
| `:DapBottomPanel terminal\|output` | Switch bottom panel tab |

---

### File Explorer

- `Ctrl-B` — toggle sidebar; `Alt+E` — focus explorer; `Alt+F` — focus search panel
- Tree view with Nerd Font file-type icons
- `j` / `k` — navigate; `l` or `Enter` — open file/expand; `h` — collapse
- `a` — create file; `A` — create folder; `D` — delete
- **Root folder entry** — project root shown at top of tree (like VSCode); select it to create files at the top level
- **Auto-refresh** — filesystem changes are detected automatically (no manual refresh needed)
- **Rename:** `F2` (GTK inline) / `r` (TUI prompt) — rename file or folder in-place
- **Move:** Drag-and-drop (GTK) / `M` key prompt (TUI) — move to another folder; full path pre-filled with cursor key editing (Left/Right/Home/End/Delete)
- **Right-click context menu (GTK):** New File, New Folder, Rename, Delete, Copy Path, Select for Diff
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
| `:Gpull` | `:Gpl` | Pull current branch |
| `:Gfetch` | `:Gf` | Fetch |
| `:Gblame` | `:Gb` | Open `git blame` in scroll-synced vertical split |
| `:Ghs` | `:Ghunk` | Stage hunk under cursor (in a `:Gdiff` buffer) |

**Hunk staging workflow**
1. `:Gdiff` — open diff in a vertical split
2. `]c` / `[c` — navigate between hunks
3. `gs` or `:Ghs` — stage the hunk under the cursor via `git apply --cached`

---

### Source Control Panel

Click the git branch icon in the activity bar to open the Source Control panel — a VSCode-style panel showing the full working tree status. The header shows the current branch plus ↑N↓N ahead/behind counts.

**Commit input row** (always visible, below the header):
- `c` — enter commit message input mode (row highlights, `|` cursor appears)
- Type your message; `BackSpace` deletes; `Escape` exits input mode (message is preserved)
- `Enter` — commits staged changes with the typed message (clears message on success)

**Four expandable sections** (Tab to collapse/expand):
- **Staged Changes** — files indexed for the next commit (`A` added, `M` modified, `D` deleted, `R` renamed)
- **Changes** — unstaged modifications and untracked files
- **Worktrees** — all git worktrees with ✓ marking the current one (hidden when no linked worktrees exist)
- **Recent Commits** — last 20 commit messages (`Enter` on an entry shows its hash + message in the status bar)

**Navigation and file actions:**
- `j` / `k` — move selection up/down
- `s` — stage/unstage the selected file; on a **section header**: stage all (Changes) or unstage all (Staged Changes)
- `d` — discard unstaged changes for the selected file (`git checkout -- <path>`)
- `D` — on the **Changes section header**: discard all unstaged changes (`git restore .`)
- `r` — refresh the panel
- `Enter` — open the selected file in the editor / switch to the selected worktree
- `Tab` — collapse/expand the current section
- `q` / `Escape` — return focus to the editor

**Remote operations (from panel):**
- `p` — push current branch
- `P` — pull current branch
- `f` — fetch

**Worktree and remote commands:**

| Command | Alias | Action |
|---------|-------|--------|
| `:GWorktreeAdd <branch> <path>` | — | Add a new git worktree at `<path>` for `<branch>` |
| `:GWorktreeRemove <path>` | — | Remove the worktree at `<path>` |
| `:Gpull` | `:Gpl` | Pull current branch |
| `:Gfetch` | `:Gf` | Fetch |

---

### Workspaces

A `.vimcode-workspace` file at the project root captures folder settings and enables per-project session restoration.

**Opening a folder or workspace:**
- **GTK:** File → "Open Folder…" / "Open Workspace…" / "Open Recent…" → native file dialog or recent-workspaces picker
- **TUI:** same menu actions open a fuzzy directory picker or recent-workspaces list modal
- **Commands:** `:OpenFolder <path>`, `:OpenWorkspace <path>`, `:SaveWorkspaceAs <path>`, `:cd <path>`, `:OpenRecent`

**Workspace file format** (`.vimcode-workspace`):
```json
{
  "version": 1,
  "folders": [{"path": "."}],
  "settings": { "tabstop": 2, "expandtab": true }
}
```
Settings in the workspace file overlay your global `settings.json`.

**Per-project sessions** — the session (open files, cursor/scroll positions) is stored per-directory using a stable hash of the workspace root path (`~/.config/vimcode/sessions/<hash>.json`). The session is saved on quit and restored automatically the next time you open the same folder. Opening a new or different directory always starts with a clean editor — files from other projects are never carried over.

**Settings overlay** — workspace settings in `.vimcode-workspace` are applied on top of your global `settings.json`. When you switch to a different folder, the overlay is reverted so your global settings are restored. Per-folder `.vimcode/settings.json` files work the same way.

---

### Lua Plugin Extensions

VimCode embeds Lua 5.4 (via `mlua`, fully vendored — no system Lua required). Plugins live in `~/.config/vimcode/plugins/` as `.lua` files or directories with `init.lua`.

**API surface** (`vimcode.*` global):

```lua
-- Event hooks
vimcode.on("save",        function(path) end)     -- fired after :w
vimcode.on("open",        function(path) end)     -- fired on file open
vimcode.on("cursor_move", function(line_col) end) -- fired when cursor moves (arg: "line,col")

-- Custom commands / key mappings
vimcode.command("MyCmd", function(args) end)
vimcode.keymap("n", "<leader>x", function() end)   -- normal mode
vimcode.keymap("i", "<C-Space>", function() end)   -- insert mode

-- Editor API
vimcode.message(text)         -- show in status bar
vimcode.cwd()                 -- current working directory string
vimcode.command_run(cmd)      -- execute a VimCode : command
vimcode.async_shell(cmd, event [, opts])  -- run shell command in background thread;
                              -- result delivered as plugin event(event, stdout)
                              -- opts: { stdin = "...", cwd = "..." }

-- Buffer API (current active buffer)
vimcode.buf.lines()              -- all lines as table
vimcode.buf.line(n)              -- line n (1-indexed) or nil
vimcode.buf.set_line(n, text)    -- replace line n
vimcode.buf.path()               -- file path string or nil
vimcode.buf.line_count()         -- integer
vimcode.buf.cursor()             -- {line, col} (1-indexed)
vimcode.buf.annotate_line(n, s)  -- show virtual text after line n
vimcode.buf.clear_annotations()  -- remove all virtual text

-- Git API (synchronous subprocess calls)
vimcode.git.blame_line(n)        -- {hash,author,date,relative_date,message} or nil
vimcode.git.log_file(limit)      -- [{hash,message}, ...] for current file
```

**Example plugin** (`~/.config/vimcode/plugins/hello.lua`):
```lua
vimcode.command("Hello", function(args)
  vimcode.message("Hello from Lua! " .. args)
end)

vimcode.on("save", function(path)
  vimcode.message("Saved: " .. path)
end)
```
Then `:Hello world` shows "Hello from Lua! world" in the status bar.

**Plugin management commands:**

| Command | Action |
|---------|--------|
| `:Plugin list` | Show all loaded plugins and their status |
| `:Plugin reload` | Reload all plugins from disk |
| `:Plugin enable <name>` | Enable a previously disabled plugin |
| `:Plugin disable <name>` | Disable a plugin (persisted in settings) |

Plugins are loaded in alphabetical order on startup. Security: plugins have unrestricted file and process access (same trust model as Neovim).

---

### Language Extensions

Language extensions bundle an LSP server, optional DAP debugger, and Lua scripts into a single named package. When you open a file for a language that has a known extension but no LSP server installed, the status bar shows a one-line hint:

```
No C# Language Support extension — :ExtInstall csharp  (N to dismiss)
```

**Bundled extensions:**

| Extension | Language | LSP | DAP |
|-----------|----------|-----|-----|
| `csharp` | C# / .NET | csharp-ls | netcoredbg |
| `python` | Python | pyright | debugpy |
| `rust` | Rust | rust-analyzer | codelldb |
| `javascript` | JS / TypeScript | typescript-language-server | — |
| `go` | Go | gopls | delve |
| `java` | Java | jdtls | — |
| `cpp` | C / C++ | clangd | codelldb |
| `php` | PHP | intelephense | — |
| `ruby` | Ruby | ruby-lsp | — |
| `bash` | Bash | bash-language-server | — |
| `json` | JSON | vscode-json-languageserver | — |
| `xml` | XML | lemminx | — |
| `yaml` | YAML | yaml-language-server | — |
| `markdown` | Markdown | marksman | — |
| `git-insights` | (all files) | — | — |

**Extensions sidebar panel** — click the extensions icon (󱧅) in the activity bar to open a VSCode-style panel with two sections:
- **INSTALLED** — extensions currently installed; press `Enter` to view info, `d` to remove
- **AVAILABLE** — all bundled and registry extensions; press `Enter` or `i` to install
- `/` — activate search input to filter both sections; `Escape` exits search, `q`/`Escape` unfocuses panel
- `j` / `k` — navigate items; `r` — refresh registry from GitHub; `Tab` — collapse/expand section

**Extension commands:**

| Command | Action |
|---------|--------|
| `:ExtInstall <name>` | Install LSP + DAP + extract Lua scripts |
| `:ExtRemove <name>` | Unmark extension as installed + delete its Lua scripts (LSP binary untouched) |
| `:ExtList` | Show all extensions and their install status |
| `:ExtEnable <name>` | Re-enable a disabled extension |
| `:ExtDisable <name>` | Suppress install prompts for this extension |
| `:ExtRefresh` | Fetch the latest extension list from the GitHub registry |

**Git Insights extension** — when installed, shows inline blame annotations as dim virtual text at the end of the cursor's current line (runs `git blame` asynchronously via `vimcode.async_shell()` so the UI never blocks):

```
42  let result = compute();   Alice • 3 days ago • fix off-by-one
```

Also adds `:GitLog` command to display recent commits for the current file in the status bar.

---

### AI Assistant

Built-in AI chat panel powered by Anthropic Claude, OpenAI, or a local Ollama model. Click the chat icon in the activity bar (or configure a keybinding) to open the panel.

**Supported providers** (configured in `settings.json`):

| Provider | Default model | Notes |
|----------|--------------|-------|
| `anthropic` | `claude-opus-4-5` | Requires `ANTHROPIC_API_KEY` or `ai_api_key` in settings |
| `openai` | `gpt-4o` | Requires `OPENAI_API_KEY` or `ai_api_key` in settings |
| `ollama` | `llama3` | Runs locally; `ai_base_url` defaults to `http://localhost:11434` |

**Usage:**
- Click the chat icon (``) in the activity bar to open the AI sidebar panel
- `i` — enter input mode; type a message and press `Enter` to send
- `j` / `k` — scroll conversation history
- `Escape` / `q` — exit input mode / unfocus panel
- `:AI <message>` — send a message directly from command mode
- `:AiClear` — clear the conversation history

**Settings** (in `settings.json`):

| Key | Default | Description |
|-----|---------|-------------|
| `ai_provider` | `"anthropic"` | AI provider: `"anthropic"`, `"openai"`, or `"ollama"` |
| `ai_api_key` | `""` | API key (falls back to `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` env vars) |
| `ai_model` | `""` | Model override (leave empty for provider default) |
| `ai_base_url` | `""` | Base URL override (used for Ollama; defaults to `http://localhost:11434`) |
| `ai_completions` | `false` | Enable AI inline completions (ghost text) in insert mode |

Responses are fetched asynchronously via a background `curl` subprocess — no async runtime required. The conversation is kept in memory for the session and cleared with `:AiClear`.

#### AI Inline Completions

When `ai_completions` is enabled, VimCode shows ghost text at the cursor while you type in insert mode. After ~250 ms of idle time a fill-in-the-middle request is sent to the configured AI provider; the suggestion appears as dimmed text. Multi-line suggestions show all continuation lines as ghost text beneath the cursor line.

| Key | Action |
|-----|--------|
| `Tab` | Accept the ghost-text suggestion |
| `Alt-]` | Cycle to next alternative suggestion |
| `Alt-[` | Cycle to previous alternative suggestion |
| Any other key | Dismiss the suggestion |

---

### LSP Support (Language Server Protocol)

Automatic language server integration — open a file and diagnostics, completions, go-to-definition, and hover just work if the appropriate server is on `PATH`. LSP initializes on every file-opening path: `:e`, sidebar click, fuzzy finder (Ctrl-P), live grep confirm, `:split`/`:vsplit`, and `:tabnew`.

**Built-in server registry** (auto-detected on `PATH`):

| Language | Server(s) tried in order |
|----------|--------------------------|
| Rust | `rust-analyzer` |
| Python | `pyright-langserver` → `basedpyright-langserver` → `pylsp` → `jedi-language-server` |
| JavaScript / TypeScript | `typescript-language-server` |
| Go | `gopls` |
| C / C++ | `clangd` |

**Features:**
- **Inline diagnostics** — wavy underlines (GTK) / colored underlines (TUI) with severity-colored gutter icons
- **Diagnostic navigation** — `]d` / `[d` jump to next/previous diagnostic
- **LSP completions** — async source for the auto-popup (appears as you type); `Ctrl-Space` manually triggers
- **Go-to-definition** — `gd` jumps to the definition of the symbol under the cursor
- **Find references** — `gr` populates quickfix list with all usage sites; single result jumps directly
- **Go-to-implementation** — `gi` jumps to the implementation of the symbol
- **Go-to-type-definition** — `gy` jumps to the type definition
- **Hover info** — `K` shows type/documentation popup above the cursor
- **Signature help** — popup appears above cursor when typing `(` or `,` in a function call; active parameter highlighted
- **LSP formatting** — `<leader>gf` (or `:Lformat`) formats the whole buffer; single undo step reverts
- **LSP rename** — `<leader>rn` pre-fills `:Rename <word>` in command bar; `:Rename <newname>` renames across all files
- **Semantic token highlighting** — overlays LSP `textDocument/semanticTokens/full` on tree-sitter; 8 distinct colors for parameters, properties, namespaces, enum members, interfaces, type parameters, decorators, and macros; bold for declarations, italic for readonly/static
- **Diagnostic counts** — `E:N W:N` shown in status bar

**Commands:**

| Command | Action |
|---------|--------|
| `:LspInfo` | Show running servers and their status |
| `:LspRestart` | Restart server for current file type |
| `:LspStop` | Stop server for current file type |
| `:LspInstall <lang>` | Redirect to `:ExtInstall <name>` (use `:ExtInstall` directly) |
| `:Lformat` | Format current buffer via LSP |
| `:Rename <name>` | Rename symbol under cursor across all files |

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
| `hlsearch` / `nohlsearch` | `hls` | on | Highlight all search matches |
| `ignorecase` / `noignorecase` | `ic` | off | Case-insensitive search |
| `smartcase` / `nosmartcase` | `scs` | off | Override `ignorecase` when pattern has uppercase |
| `scrolloff=N` | `so` | 0 | Lines to keep above/below cursor when scrolling |
| `cursorline` / `nocursorline` | `cul` | off | Highlight the line the cursor is on |
| `colorcolumn=N` | `cc` | "" | Comma-list of column guides to highlight |
| `textwidth=N` | `tw` | 0 | Auto-wrap inserted text at column N (0=off) |
| `wrap` / `nowrap` | | off | Soft-wrap long lines at viewport edge |
| `splitbelow` / `nosplitbelow` | `sb` | off | Horizontal splits open below current window |
| `splitright` / `nosplitright` | `spr` | off | Vertical splits open to right of current window |
| `lsp` / `nolsp` | | on | Enable/disable LSP language servers |
| `formatonsave` / `noformatonsave` | `fos` | off | Auto-format buffer via LSP before saving |
| `mode=vim` / `mode=vscode` | | vim | Editor mode (see **VSCode Mode** below) |

Additional options (set directly in `settings.json`):

| Key | Default | Description |
|-----|---------|-------------|
| `terminal_scrollback_lines` | `5000` | Rows kept in terminal scrollback history (0 = unlimited) |
| `leader` | `" "` (Space) | Leader key character for `<leader>gf` / `<leader>rn` sequences |
| `extension_registry_url` | GitHub raw URL | URL for the remote extension registry JSON (override for self-hosted) |
| `ai_provider` | `"anthropic"` | AI provider: `"anthropic"`, `"openai"`, or `"ollama"` |
| `ai_api_key` | `""` | API key (falls back to `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` env vars) |
| `ai_model` | `""` | Model override (leave empty for provider default) |
| `ai_base_url` | `""` | Base URL override (used for Ollama; defaults to `http://localhost:11434`) |
| `ai_completions` | `false` | Enable AI inline completions (ghost text) in insert mode |
| `show_hidden_files` | `false` | Show dotfiles in file explorer (`:set showhiddenfiles` / `:set shf`) |
| `swap_file` | `true` | Write swap files for crash recovery (`:set swapfile` / `:set noswapfile`) |
| `updatetime` | `4000` | Milliseconds between swap file writes for dirty buffers (`:set updatetime=N`) |

- `:set option?` — query current value (e.g. `:set ts?` → `tabstop=4`)
- `:set option!` — toggle a boolean option (e.g. `:set wrap!`); `no<option>!` explicitly disables (e.g. `:set nowrap!`)
- `:set` (no args) — show one-line summary of all settings
- `:config reload` — reload settings file from disk
- `:colorscheme <name>` — switch colour theme live (aliases: `gruvbox`, `tokyonight`, `solarized`); `:colorscheme` lists available themes. Themes: `onedark` (default), `gruvbox-dark`, `tokyo-night`, `solarized-dark`.
- `:Settings` — open `settings.json` in a new editor tab for direct editing; saved changes reload automatically in both GTK and TUI backends.
- **Settings sidebar (GTK)** — click the gear icon in the activity bar to open a VSCode-style settings form: searchable list of all settings grouped by category (Appearance, Editor, Search, Workspace, LSP, Terminal, Plugins) with native widgets (Toggle switch, spinner, dropdown, text entry); changes apply and save immediately.

**Panel navigation key bindings** — configurable in `settings.json` under `"panel_keys"`:

| Field | Default | Action |
|-------|---------|--------|
| `toggle_sidebar` | `<C-b>` | Toggle sidebar visibility |
| `focus_explorer` | `<A-e>` | Focus explorer (press again to return to editor) |
| `focus_search` | `<A-f>` | Focus search panel (press again to return to editor) |
| `fuzzy_finder` | `<C-p>` | Open fuzzy file finder |
| `live_grep` | `<C-g>` | Open live grep modal |
| `open_terminal` | `<C-t>` | Toggle integrated terminal panel |
| `add_cursor` | `<A-d>` | Add cursor at next occurrence of word under cursor |

Key notation: `<C-x>` = Ctrl+x, `<A-x>` = Alt+x, `<C-S-x>` = Ctrl+Shift+x.

**Explorer key bindings** — configurable in `settings.json` under `"explorer_keys"`:

| Field | Default | Action |
|-------|---------|--------|
| `new_file` | `a` | New file prompt |
| `new_folder` | `A` | New folder prompt |
| `delete` | `D` | Delete prompt |
| `rename` | `r` | Rename prompt |
| `move_file` | `M` | Move file prompt |

**Completion key bindings** — configurable in `settings.json` under `"completion_keys"`:

| Field | Default | Action |
|-------|---------|--------|
| `trigger` | `<C-Space>` | Manually trigger the completion popup |
| `accept` | `Tab` | Accept the highlighted completion item |

Only specify keys you want to change — unspecified keys keep their defaults.

---

### VSCode Mode

Switch the editor into a **non-modal editing** mode that works like a standard text editor:

- `:set mode=vscode` — activate VSCode mode (from Vim normal mode)
- `Alt-M` — toggle between Vim mode and VSCode mode at any time
- `:set mode=vim` — return to Vim mode

**In VSCode mode:**
- Always in "insert" state — no mode switching
- `Ctrl-C` / `Ctrl-X` — copy / cut (no selection → copies/cuts whole current line)
- `Ctrl-V` — paste
- `Ctrl-Z` / `Ctrl-Y` — undo / redo
- `Ctrl-A` — select all
- `Ctrl-S` — save
- `Ctrl-/` — toggle line comment (`// `)
- `Shift+Arrow` — extend selection one character/line at a time
- `Ctrl+Arrow` — move by word
- `Ctrl+Shift+Arrow` — extend selection by word
- `Home` — smart home (first non-whitespace; again → col 0)
- `Shift+Home` / `Shift+End` — extend selection to line start/end
- `Escape` — clear selection (stays in insert)
- `F1` — open the command bar (run any `:` command, then returns to EDIT mode)
- Typing while a selection is active **replaces** the selection
- Status bar shows `EDIT  F1:cmd  Alt-M:vim` (or `SELECT` when text is selected, `COMMAND` in command bar)

The `editor_mode` setting is persisted in `settings.json`.

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
- Rust, Python, JavaScript, TypeScript/TSX, Go, C, C++, C#, Java, Ruby, Bash, JSON, TOML, CSS

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
- **Mouse support:** click-to-position, double-click word select, click-and-drag visual selection, window switching, scroll wheel (targets pane under cursor), scrollbar click-to-jump and drag; drag event coalescing for smooth scrollbar tracking; bracketed paste support
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
| `W` / `B` / `E` / `gE` | WORD forward/back start/end (whitespace-delimited) |
| `^` / `g_` | First / last non-blank character of line |
| `(` / `)` | Sentence backward / forward |
| `{` / `}` | Paragraph backward/forward |
| `H` / `M` / `L` | Screen top / middle / bottom |
| `gg` / `G` | First / last line |
| `0` / `$` | Line start / end |
| `f{c}` / `t{c}` | Find / till char (`;` `,` repeat) |
| `%` | Jump to matching bracket |
| `zz` / `zt` / `zb` | Scroll cursor to center / top / bottom |
| `Ctrl-O` / `Ctrl-I` | Jump list back / forward |
| `Ctrl-D` / `Ctrl-U` | Half-page down / up |
| `Ctrl-E` / `Ctrl-Y` | Scroll one line down / up (cursor stays) |
| `i` / `I` / `a` / `A` | Insert (cursor / line-start / append / line-end) |
| `o` / `O` | Open line below / above |
| `x` / `dd` / `D` | Delete char / line / to EOL |
| `yy` / `Y` | Yank line |
| `p` / `P` | Paste after / before |
| `u` / `Ctrl-R` | Undo / redo |
| `U` | Undo all changes on line |
| `.` | Repeat last change |
| `r{c}` | Replace character |
| `R` | Replace mode — overtype until `Escape` |
| `~` | Toggle case of char under cursor (count supported) |
| `J` | Join lines (collapse next line's whitespace to one space) |
| `gJ` | Join lines without inserting a space |
| `Ctrl-A` / `Ctrl-X` | Increment / decrement number under cursor |
| `=` operator | Auto-indent range (`==` current line, `gg=G` whole file) |
| `]p` / `[p` | Paste after / before with indent adjusted to current line |
| `>>` / `<<` | Indent / dedent line(s) by `shiftwidth` |
| `*` / `#` | Search forward / backward for word under cursor (word-bounded) |
| `g*` / `g#` | Search forward / backward for word under cursor (partial match) |
| `gf` | Open file path under cursor |
| `v` / `V` / `Ctrl-V` | Visual / Visual Line / Visual Block |
| `/` / `?` | Search forward / backward |
| `n` / `N` | Next / previous match |
| `m{a-z}` / `'{a-z}` | Set mark / jump to mark |
| `q{a-z}` / `@{a-z}` | Record macro / play macro |
| `gt` / `gT` | Next / previous tab |
| `gd` | Go to definition (LSP) |
| `gr` | Find references (LSP) — multiple results open quickfix |
| `gi` | Go to implementation (LSP) |
| `gy` | Go to type definition (LSP) |
| `gs` | Stage hunk (in `:Gdiff` buffer) |
| `K` | Show hover info (LSP) |
| `]c` / `[c` | Next / previous hunk |
| `]d` / `[d` | Next / previous diagnostic (LSP) |
| `<leader>gf` | LSP format current buffer (Space=leader by default) |
| `<leader>rn` | LSP rename symbol — pre-fills `:Rename <word>` |
| `za` / `zo` / `zc` / `zR` | Fold toggle / open / close / open all |
| `Ctrl-W h/j/k/l` | Focus window left/down/up/right |
| `Ctrl-W w` / `c` / `o` | Cycle / close / close-others |
| `Ctrl-P` | Open fuzzy file finder |
| `Ctrl-G` | Open live grep modal (search file contents) |
| `F5` | Start debugging / continue |
| `Shift+F5` | Stop debugging |
| `F6` | Pause debugger |
| `F9` | Toggle breakpoint |
| `F10` | Step over |
| `F11` | Step into |
| `Shift+F11` | Step out |
| `Alt+E` | Focus / unfocus file explorer |
| `Alt+F` | Focus / unfocus search panel |
| `Ctrl+\` | Split editor right (new editor group) |
| `Ctrl+1` / `Ctrl+2` | Focus editor group 0 / 1 |
| `Shift+Alt+F` | Format document (LSP) |
| `Alt+,` / `Alt+.` | Resize group split (TUI) |

### Command Mode

| Command | Action |
|---------|--------|
| `:w` / `:wq` | Save / save and quit |
| `:wa` | Write all dirty buffers |
| `:wqa` / `:xa` | Write all and quit |
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
| `:g/pat/cmd` | Run ex command on every line matching pattern |
| `:v/pat/cmd` | Run ex command on every line NOT matching pattern |
| `:d` / `:delete` | Delete current line (used as `:g/pat/d` subcommand) |
| `:m[ove] {dest}` | Move current line to after line {dest} (0-indexed) |
| `:t {dest}` / `:co[py] {dest}` | Copy current line to after line {dest} (0-indexed) |
| `:sort [n] [r] [u] [i]` | Sort lines: `n`=numeric, `r`=reverse, `u`=unique, `i`=ignorecase |
| `:set [option]` | Change / query setting |
| `:noh` / `:nohlsearch` | Clear current search highlight |
| `:echo {text}` | Display a message in the status bar |
| `:reg` / `:registers` | Display register contents |
| `:marks` | Display all set marks |
| `:jumps` | Display jump list |
| `:changes` | Display change list |
| `:history` | Display command history |
| `:!{cmd}` | Execute shell command and show output |
| `:r {file}` | Read file contents into buffer after cursor line |
| `:tabmove [N]` | Move current tab to position N (0-based, default = end) |
| `:Gdiff` / `:Gstatus` | Git diff / status |
| `:Gadd` / `:Gadd!` | Stage file / stage all |
| `:Gcommit <msg>` | Commit |
| `:Gpush` | Push |
| `:Gpull` / `:Gpl` | Pull |
| `:Gfetch` / `:Gf` | Fetch |
| `:Gblame` | Blame (scroll-synced split) |
| `:Ghs` / `:Ghunk` | Stage hunk under cursor |
| `:GWorktreeAdd <branch> <path>` | Add git worktree |
| `:GWorktreeRemove <path>` | Remove git worktree |
| `:OpenFolder <path>` | Open folder (clears buffers, loads per-project session) |
| `:OpenWorkspace <path>` | Open `.vimcode-workspace` file |
| `:SaveWorkspaceAs <path>` | Save current folder as workspace file |
| `:OpenRecent` | Open recent workspaces picker |
| `:cd <path>` | Change working directory |
| `:diffsplit <file>` | Open file in vsplit with diff highlighting |
| `:diffthis` | Mark current window as diff participant (two calls activate diff) |
| `:diffoff` | Clear diff highlighting |
| `:grep <pat>` / `:vimgrep <pat>` | Search project, populate quickfix list |
| `:copen` / `:ccl` | Open / close quickfix panel |
| `:cn` / `:cp` | Next / previous quickfix item |
| `:cc N` | Jump to Nth quickfix item (1-based) |
| `:LspInfo` | Show running LSP servers |
| `:LspRestart` | Restart server for current language |
| `:LspStop` | Stop server for current language |
| `:LspInstall <lang>` | Install LSP server for language via Mason |
| `:Lformat` | Format buffer via LSP |
| `:Rename <newname>` | Rename symbol under cursor across workspace |
| `:DapInstall <lang>` | Install debug adapter for language |
| `:DapInfo` | Show detected DAP adapters |
| `:DapEval <expr>` | Evaluate expression in current debug frame |
| `:DapWatch <expr>` | Add watch expression to debug sidebar |
| `:GWorktreeAdd <branch> <path>` | Add git worktree |
| `:GWorktreeRemove <path>` | Remove git worktree |
| `:EditorGroupSplit` / `:egsp` | Split editor right (new editor group) |
| `:EditorGroupSplitDown` / `:egspd` | Split editor down |
| `:EditorGroupClose` / `:egc` | Close active editor group |
| `:EditorGroupFocus` / `:egf` | Toggle focus between editor groups |
| `:EditorGroupMoveTab` / `:egmt` | Move current tab to other editor group |
| `:OpenFolder <path>` | Open folder as workspace root |
| `:OpenWorkspace <path>` | Open `.vimcode-workspace` file |
| `:SaveWorkspaceAs <path>` | Save workspace file |
| `:OpenRecent` | Open recent workspaces picker |
| `:cd <path>` | Change working directory |
| `:Plugin list` | List loaded plugins |
| `:Plugin reload` | Reload plugins from disk |
| `:Plugin enable <name>` | Enable a plugin |
| `:Plugin disable <name>` | Disable a plugin |
| `:ExtInstall <name>` | Install a language extension (LSP + DAP + Lua scripts) |
| `:ExtList` | List available extensions and their install status |
| `:ExtEnable <name>` | Re-enable a disabled extension |
| `:ExtDisable <name>` | Disable an extension (suppress install prompts) |
| `:config reload` | Reload settings from disk |
| `:AI <message>` | Send a message to the AI assistant |
| `:AiClear` | Clear the AI conversation history |
| `:MarkdownPreview` / `:MdPreview` | Open side-by-side styled markdown preview (live-updates on edit, scroll sync, scaled headings in GTK) |
| `:help [topic]` / `:h [topic]` | Show help (topics: explorer, keys, commands) |

---

## Architecture

```
src/
├── main.rs          (~10,907 lines)  GTK4/Relm4 UI, rendering, sidebar resize, fuzzy popup, context menu, drag-and-drop
├── tui_main.rs      (~9,124 lines)  ratatui/crossterm TUI backend, fuzzy popup, rename/move prompts
├── render.rs        (~4,364 lines)  Platform-agnostic ScreenLayout bridge (DebugSidebarData, SourceControlData, BottomPanelTabs)
├── icons.rs            (~30 lines)  Nerd Font file-type icons (GTK + TUI)
└── core/            (~29,500 lines)  Zero GTK/rendering deps — fully testable
    ├── engine.rs    (~32,476 lines)  Orchestrator: keys, commands, git, macros, LSP, DAP, plugins, workspaces
    ├── markdown.rs     (~497 lines)  Markdown → styled plain text converter (pulldown-cmark)
    ├── plugin.rs       (~835 lines)  Lua 5.4 plugin manager (mlua vendored; vimcode.* API; async_shell)
    ├── terminal.rs     (~320 lines)  PTY-backed terminal pane (portable-pty + vt100, history ring buffer)
    ├── lsp.rs        (~2,306 lines)  LSP protocol transport + single-server client (request ID tracking, JSON-RPC framing, semantic tokens)
    ├── lsp_manager.rs  (~830 lines)  Multi-server coordinator with initialization guards + built-in registry + semantic legends
    ├── dap.rs          (~671 lines)  DAP protocol transport + event routing + seq→command tracking + BreakpointInfo
    ├── dap_manager.rs  (~1,089 lines)  DAP multi-adapter coordinator + launch.json + tasks.json support + install scripts
    ├── ai.rs               (~336 lines)  AI provider integration (Anthropic/OpenAI/Ollama via curl subprocess)
    ├── project_search.rs (~630 lines)  Regex/case/whole-word search + replace (ignore + regex crates)
    ├── buffer_manager.rs (~707 lines)  Buffer lifecycle, undo/redo stacks, semantic tokens
    ├── buffer.rs       (~120 lines)  Rope-based text storage (ropey)
    ├── settings.rs   (~1,346 lines)  JSON config, :set parsing, key binding notation
    ├── session.rs      (~235 lines)  Session state persistence + per-workspace paths
    ├── git.rs        (~1,000 lines)  Git subprocesses: diff, blame, stage_hunk, SC panel, worktrees, git log
    └── window.rs, tab.rs, view.rs, cursor.rs, mode.rs, syntax.rs (~984 lines)
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
| Plugins | mlua 0.9 (Lua 5.4, vendored) |

## License

MIT
