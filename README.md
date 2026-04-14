# VimCode

**[vimcode.org](https://vimcode.org)** | [Documentation](https://github.com/JDonaghy/vimcode/wiki) | [Releases](https://github.com/JDonaghy/vimcode/releases)

A Vim+VSCode hybrid editor written in Rust. 137K lines of code, 5,495 tests, four rendering backends.

### Who’s this for?

If you want Vim’s modal editing with VSCode’s UI — or VSCode’s ease of use with Vim’s power — VimCode bridges both worlds. Run it in a terminal and it looks like Vim; run it in a window and it looks like VSCode. Press `Alt-M` to switch between Vim mode and VSCode mode at any time.

VimCode takes a batteries-included approach: **LSP**, **DAP**, integrated terminal, git panel, AI assistant, spell checking, and 20 tree-sitter grammars all work out of the box. Language extensions for 17 languages are available from the [vimcode-ext](https://github.com/JDonaghy/vimcode-ext) registry — browse and install them from the built-in **Extensions panel** or with `:ExtInstall`.

Like Neovim, VimCode supports **Lua 5.4** for extensions — but with its own API, not VimScript or Neovim’s API. Unlike Zed, VimCode does not require a dedicated GPU. GTK4 uses hardware compositing when available but falls back gracefully to software rendering, and the native Windows backend uses Direct2D. VimCode works everywhere: desktops, VMs, remote sessions, headless servers.

### Platforms

| Platform | GUI | TUI |
|----------|-----|-----|
| **Linux** | GTK4 + Cairo + Pango | ratatui + crossterm |
| **macOS** | GTK4 via Homebrew | ratatui + crossterm |
| **Windows** | Native Win32 + Direct2D + DirectWrite (**alpha**) | ratatui + crossterm |

### Status — Beta

VimCode is currently in **beta**. It is under active development with the assistance of **Claude Code** and is used daily as a primary editor. While it is stable enough for daily use, you may encounter bugs or incomplete features. **Back up your work and use version control.** Bug reports and feature requests are welcome — please include enough detail to reproduce the problem and describe the expected behavior.

For screenshots and more information, visit **[vimcode.org](https://vimcode.org)**.

---

## Documentation

For detailed how-to guides and configuration references, see the **[VimCode Wiki](https://github.com/JDonaghy/vimcode/wiki)**:

- [Getting Started](https://github.com/JDonaghy/vimcode/wiki/Getting-Started) — installation, first launch, essential keys
- [Key Remapping](https://github.com/JDonaghy/vimcode/wiki/Key-Remapping) — customize keybindings in Vim and VSCode modes
- [Settings Reference](https://github.com/JDonaghy/vimcode/wiki/Settings-Reference) — all configurable options
- [Extension Development](https://github.com/JDonaghy/vimcode/wiki/Extension-Development) — write extensions with manifest.toml and Lua scripts
- [Lua Plugin API](https://github.com/JDonaghy/vimcode/wiki/Lua-Plugin-API) — full `vimcode.*` API reference
- [Theme Customization](https://github.com/JDonaghy/vimcode/wiki/Theme-Customization) — built-in themes and VSCode theme import
- [DAP Debugger Setup](https://github.com/JDonaghy/vimcode/wiki/DAP-Debugger-Setup) — configure and use the debugger
- [LSP Configuration](https://github.com/JDonaghy/vimcode/wiki/LSP-Configuration) — language server setup and custom servers

## Vision

- **First-class Vim mode** — deeply integrated modal editing, not a plugin bolted onto a different editor
- **Cross-platform** — GTK4 on Linux/macOS, native Win32+Direct2D on Windows, full TUI everywhere
- **No GPU required** — Cairo/Pango and Direct2D/DirectWrite rendering; hardware compositing when available, software fallback always works (VMs, remote desktops, SSH)
- **Clean architecture** — platform-agnostic core (`src/core/`), 5,495 tests, zero async runtime dependency

> **Note:** VimCode does not implement VimScript. Extension and scripting is handled via
> the built-in Lua 5.4 plugin system. The goal is full Vim *keybinding* and *editing*
> compatibility, not a VimScript runtime. For a detailed Vim compatibility checklist, see [VIM_COMPATIBILITY.md](VIM_COMPATIBILITY.md).

## Download

Pre-built packages are published automatically on every release. For terminal use on any platform, the **`vcd`** binary is the recommended option — it has no GUI dependencies and works everywhere.

**[→ Download latest release](../../releases/tag/latest)**

### Linux

**Option A — `.deb` package (recommended for Ubuntu/Debian)**
```bash
sudo dpkg -i vimcode_*.deb
sudo apt -f install   # pulls in any missing GTK4 runtime libraries
```
Requires **Ubuntu 24.04+** or any distro with **GTK 4.10+**. The `.deb` handles all runtime dependencies automatically.

**Option B — raw binary**
```bash
# First install runtime dependencies:
sudo apt install libgtk-4-1 libglib2.0-0 libpango-1.0-0 libcairo2
# Then run:
chmod +x vimcode-linux-x86_64
./vimcode-linux-x86_64
```

**Option C — Flatpak**
```bash
flatpak install vimcode.flatpak
flatpak run io.github.jdonaghy.VimCode
```
The Flatpak bundles GTK4 and all dependencies — works on any Linux distro with Flatpak installed.

> **Note:** Ubuntu 22.04 ships GTK 4.6 which is too old for the `.deb` and raw binary; use the Flatpak or upgrade to 24.04+.

### macOS

```bash
brew tap JDonaghy/vimcode
brew install vimcode    # GTK4 GUI + TUI (installs both `vimcode` and `vcd`)
brew install vcd        # TUI only (no GTK4 dependency)
```

### Windows

> **Note:** The native Windows GUI is in **alpha** — several features (scrollbars, tab clicks, preview tabs) are not yet implemented. The TUI build is recommended for Windows users until the native GUI matures. See [Known Bugs](BUGS.md) for details.

**Option A — Native GUI** (`vimcode-win.exe`)
Download `vimcode-win-x86_64.exe` from the release page. No dependencies required — Direct2D and DirectWrite are built into Windows.

**Option B — TUI** (`vcd.exe`)
Download `vcd-windows-x86_64.exe` from the release page (rename to `vcd.exe` for convenience). Run it in Windows Terminal, PowerShell, or cmd.exe. No dependencies required. This is the same `vcd` binary used on Linux and macOS.

---

## Building from Source

### Prerequisites

The default build produces the **GTK4 GUI** + **TUI** binary. The **native Windows GUI** is built separately with a Cargo feature flag.

| Platform | GTK4 GUI deps |
|---|---|
| Ubuntu/Debian | `sudo apt install libgtk-4-dev build-essential pkg-config` |
| Fedora | `sudo dnf install gtk4-devel gcc pkg-config` |
| Arch | `sudo pacman -S gtk4 base-devel pkgconf` |
| openSUSE | `sudo zypper install gtk4-devel gcc pkg-config` |
| macOS | `brew install gtk4 pkg-config` |

**Platform notes:**
- **TUI-only mode** (`--tui` or `-t`) works without GTK4 — only a terminal emulator is needed
- **Windows native GUI** does not require GTK4; it uses the Win32 API directly (see below)
- **Nerd Font icons:** VimCode uses Nerd Font icons throughout the UI. **GTK mode** bundles a Nerd Font icon subset and works out of the box. **TUI mode** requires a [Nerd Font](https://www.nerdfonts.com/) (e.g. JetBrainsMono Nerd Font) as your terminal font. If your terminal font lacks Nerd Font glyphs, set `"use_nerd_fonts": false` in `settings.json` (or `:set nonerdfonts`) to switch all icons to ASCII/Unicode fallbacks.

### Build & run

```bash
# Linux / macOS (GTK4 GUI + TUI)
cargo build
cargo run -- <file>                         # GTK window
cargo run -- --tui <file>                   # Terminal UI (alias: -t)
cargo run -- --tui --debug /tmp/v.log       # TUI with debug log
cargo run -- --version                      # Print version and exit (alias: -V)

# Windows — Native GUI (Direct2D + DirectWrite, no GTK4 needed)
cargo build --features win-gui --bin vimcode-win
cargo run --features win-gui --bin vimcode-win

# Windows — TUI only (no GTK4 needed)
cargo build --no-default-features
cargo run --no-default-features -- <file>

# Tests & linting
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
- `` i` `` / `` a` `` — inner/around backticks
- `ie` / `ae` — inner/around LaTeX `\begin{env}...\end{env}` (nested-aware, LaTeX buffers only)
- `ic` / `ac` — inner/around LaTeX `\command{...}` (LaTeX buffers only)
- `i$` / `a$` — inner/around LaTeX math (`$...$`, `$$...$$`, `\[...\]`, `\(...\)`; LaTeX buffers only)

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
- `Ctrl-E` — insert character from line below
- `Ctrl-Y` — insert character from line above
- `Ctrl-O` — execute one Normal command, then auto-return to Insert
- `Ctrl-@` — insert previously inserted text and exit Insert mode
- `Ctrl-V {char}` — insert next character literally (Tab, Return, any key)

**Visual mode**
- `v` — character selection; `V` — line selection; `Ctrl-V` — block selection
- All operators work on selection: `d`, `c`, `y`, `u`, `U`, `~`, `p`/`P` (paste replaces selection)
- `"{reg}p` — paste from named register over selection (deleted text goes to unnamed register)
- Block mode: rectangular selections, change/delete/yank uniform columns
- `I` (block) — insert text at left edge of block (applied to all lines on Escape)
- `A` (block) — append text after right edge of block (applied to all lines on Escape)
- `o` — swap cursor to opposite end of selection (character/line visual)
- `O` — swap cursor to opposite column corner (visual block)
- `gv` — reselect last visual selection
- `r{char}` — replace all selected characters with `{char}`

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
- `Ctrl-Shift-V` — paste system clipboard in Normal/Visual/Insert/Command/Search mode (GTK + TUI with keyboard enhancement)

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

**Spell Checking**
- `:set spell` / `:set nospell` — enable/disable; also togglable via "Toggle Spell Check" in the command palette
- Misspelled words shown with a cyan dotted underline (wavy dots in GTK, colored underline in TUI)
- **Tree-sitter aware:** in code files only comments and strings are checked; in plain-text and Markdown files the whole buffer is checked
- `]s` / `[s` — jump to next/previous spelling error
- `z=` — show spelling suggestions for word under cursor (displayed in status bar)
- `zg` — add word under cursor to the user dictionary (`~/.config/vimcode/user.dic`)
- `zw` — mark word as wrong (add to wrong-word list)
- Bundled en_US dictionary (Hunspell-compatible, compiled into the binary); `spelllang` setting selects the language (only `en_US` currently bundled)

**Hunk navigation (diff buffers)**
- `]c` / `[c` — jump to next/previous change region (uses diff results in side-by-side view, `@@` headers in unified diff, git diff markers otherwise)

---

### Multi-File Editing

VimCode has three spatial layers that combine Vim and VSCode concepts:
- **Windows** — Vim-style splits *within* a single tab (`:split`, `:vsplit`, `Ctrl-W s/v`)
- **Tabs** — pages within an editor group, like Vim tabs or browser tabs (`gt`/`gT`, `:tabnew`)
- **Editor Groups** — VSCode-style side-by-side tab bars (`Ctrl+\`, `Ctrl-W e/E`), each with its own set of tabs

The tab context menu offers both: "Split Right/Down" creates a Vim window split inside the current tab, while "Split Right/Down to New Group" creates a new editor group with its own tab bar. Each tab bar also has a `…` (more actions) button at the right edge with Close All, Close Others, Close Saved, Close to Right/Left, Toggle Word Wrap, Change Language Mode, and Reveal in File Explorer.

**Buffers**
- `:bn` / `:bp` — next/previous buffer
- `:b#` — alternate buffer
- `:ls` — list buffers (shows `[Preview]` suffix for preview tabs)
- `:bd` — delete buffer

**Windows** (splits within the current tab — not to be confused with Editor Groups)
- `:split` / `:vsplit` — horizontal/vertical split
- `:close` — close window; `:only` — close all other windows
- `Ctrl-W h/j/k/l` — move focus between panes (or `:wincmd h/j/k/l`)
- `Ctrl-W w` — cycle focus; `Ctrl-W c` — close; `Ctrl-W o` — close others
- `Ctrl-W s/v` — split (same as `:split`/`:vsplit`)

**Tabs**
- `:tabnew` — new tab; `:tabclose` — close tab
- `gt` / `gT` or `g` + `t` / `T` — next/previous tab
- `Ctrl+Tab` / `Ctrl+Shift+Tab` — MRU tab switcher popup (cycles most-recently-used tabs; Enter confirms, Escape cancels); release modifier to auto-confirm (GTK)
- `Alt+t` — MRU tab switcher (works in both TUI and GTK; hold Alt and press `t` to cycle; release Alt or wait 500ms to confirm in TUI)

**Editor Groups / Tab Groups (VSCode-style split panes, recursive)**
- `Ctrl+\` — split editor right (any group can be split again for nested layouts)
- `Ctrl-W e` / `Ctrl-W E` — split editor right / down
- `Ctrl+1` through `Ctrl+9` — focus group by position (tree order)
- `:EditorGroupFocus` / `:egf` — cycle focus to the next group
- `:EditorGroupClose` / `:egc` — close the active group (sibling promoted)
- `:EditorGroupMoveTab` / `:egmt` — move the current tab to the next group
- `Alt+,` / `Alt+.` — shrink / expand the active group (both GTK and TUI)
- Drag any group divider — resize that specific split (both GTK and TUI)
- Drag a tab to another group's tab bar — move it there (both GTK and TUI)
- Drag a tab to the edge of an editor area — create a new split (both GTK and TUI)

**Quit / Save**
- `:w` — save; `:wq` — save and quit
- `:q` — close tab (quits if last tab; blocked if dirty unless visible in another split)
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

### Unified Picker (Telescope-Style)

VimCode uses a unified picker system for file finding, live grep, and command palette. All pickers share the same UI and keybindings. Fuzzy match characters are highlighted in the results.

#### Command Center

Click the search box in the menu bar (or run `:CommandCenter`) to open the unified picker. Type a prefix to switch modes:

| Prefix | Mode |
|--------|------|
| _(none)_ | Fuzzy file search (same as `Ctrl-P`) |
| `>` | Command palette (same as `Ctrl-Shift-P`) |
| `@` | Go to symbol in current file — expandable tree view (LSP `documentSymbol`) |
| `#` | Workspace symbol search (LSP `workspace/symbol`) |
| `:` | Go to line number |
| `%` | Search for text in project (live grep) |
| `debug` | Start debugging (show launch configurations) |
| `task` | Run a task (from tasks.json) |
| `?` | Show available prefix modes |

#### Fuzzy File Finder

- `Ctrl-P` or `<leader>sf` (Normal mode) — open the fuzzy file picker
- A centered floating modal appears over the editor
- Type to instantly filter all project files by fuzzy subsequence match
- Word-boundary matches (after `/`, `_`, `-`, `.`) are scored higher; `.gitignore`-aware via `ignore` crate
- `Ctrl-N` / `↓` and `Ctrl-P` / `↑` — navigate results; `Enter` — open selected file; `Escape` — close

#### Live Grep

- `Ctrl-Shift-F` or `<leader>sg` (Normal mode) — open the live grep picker
- `<leader>sw` — open live grep pre-filled with the word under the cursor
- A centered floating two-column modal appears over the editor
- Type to instantly search file *contents* across the entire project (live-as-you-type, query ≥ 2 chars)
- Left pane shows results in `filename.rs:N: snippet` format; right pane shows ±5 context lines around the match
- Match line is highlighted in the preview pane
- `Ctrl-N` / `↓` and `Ctrl-P` / `↑` — navigate results; preview updates as you move; `Enter` — open file at match line; `Escape` — close
- Results capped at 200; uses `.gitignore`-aware search (same engine as project search panel)

#### Command Palette

- `Ctrl-Shift-P` / `F1` or `<leader>sp` (Normal mode) — open the command palette picker
- Lists all commands with descriptions and current keybindings; type to fuzzy-filter

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

Built-in Debug Adapter Protocol support with a VSCode-like UI. Open the debug sidebar (click the bug icon), set breakpoints with `F9`, and press `F5` to start debugging. Supports 6 adapters: codelldb (Rust/C/C++), debugpy (Python), delve (Go), js-debug (JS/TS), java-debug (Java), netcoredbg (C#). Install with `:DapInstall <lang>`.

The debug sidebar has four sections: Variables, Watch, Call Stack, and Breakpoints. Breakpoints support conditions, hit counts, and logpoints. A `launch.json` is auto-generated on first run.

| Key | Action |
|-----|--------|
| `F5` | Start / continue |
| `Shift+F5` | Stop |
| `F9` | Toggle breakpoint |
| `F10` / `F11` | Step over / into |
| `Shift+F11` | Step out |

For full details on adapters, launch.json, conditional breakpoints, and the debug sidebar, see the **[DAP Debugger Setup](https://github.com/JDonaghy/vimcode/wiki/DAP-Debugger-Setup)** wiki page.

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
- **Right-click context menu:** New File, New Folder, Rename, Delete, Copy Path, Copy Relative Path, Open to Side, Open to Side (vsplit), Select for Compare / Compare with Selected (opens diff view), Reveal in File Manager; tab bar: Close, Close Others, Close to Right, Close Saved, Split Right/Down; editor area: Go to Definition, Go to References, Rename Symbol, Open Changes, Cut, Copy, Paste, Open to Side (vsplit), Command Palette
- **Preview mode:**
  - Single-click → preview tab (italic/dimmed, replaced by next single-click)
  - Double-click → permanent tab
  - Edit or save → auto-promotes to permanent
  - `:ls` shows `[Preview]` suffix
- **Buffer indicators** — right-aligned badges on explorer rows: git status (`M`/`A`/`?`/`D`/`R`), LSP error count (red), warning count (yellow); capped at `9+`; configurable per-extension error source filtering; updated live in both GTK and TUI
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
| `:Gdiffsplit` | `:Gds` | Side-by-side diff: HEAD (read-only) left, working copy right |
| `:Gstatus` | `:Gs` | Open `git status` in vertical split |
| `:Gadd` | `:Ga` | Stage current file (`git add`) |
| `:Gadd!` | `:Ga!` | Stage all changes (`git add -A`) |
| `:Gcommit <msg>` | `:Gc <msg>` | Commit with message |
| `:Gpush` | `:Gp` | Push current branch |
| `:Gpull` | `:Gpl` | Pull current branch |
| `:Gfetch` | `:Gf` | Fetch |
| `:Gblame` | `:Gb` | Open `git blame` in scroll-synced vertical split |
| `:Gswitch <branch>` | `:Gsw` | Switch to an existing branch |
| `:Gbranch <name>` | | Create a new branch and switch to it |
| `:Gbranches` | | Open branch picker (fuzzy-filter, click status bar branch) |
| `:Ghs` | `:Ghunk` | Stage hunk under cursor (in a `:Gdiff` buffer) |
| `:Gshow <hash>` | | Show commit in Git Log panel (navigates and expands) |

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
- `Enter` — inserts a newline (multi-line commit messages; input box grows in height)
- `Ctrl+Enter` — commits staged changes with the typed message (clears message on success)

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

**Branch operations:**
- `b` — open branch picker (fuzzy-filter, `Enter` to switch, `Escape` to cancel)
- `B` — create a new branch (type name, `Enter` to create + switch)

**Remote operations (from panel):**
- `p` — push current branch
- `P` — pull current branch
- `f` — fetch

**Help:**
- `?` — show keybindings help dialog (any key closes it)

**Worktree and remote commands:**

| Command | Alias | Action |
|---------|-------|--------|
| `:GWorktreeAdd <branch> <path>` | — | Add a new git worktree at `<path>` for `<branch>` |
| `:GWorktreeRemove <path>` | — | Remove the worktree at `<path>` |
| `:Gpull` | `:Gpl` | Pull current branch |
| `:Gfetch` | `:Gf` | Fetch |

---

### Git Log Panel

The Git Log panel (provided by the `git-insights` extension) shows branches, commit history, and stashes in a dedicated sidebar panel. Commits are expandable tree nodes — expanding a commit reveals the files it changed.

**Commit interaction:**
- `Tab` — expand/collapse commit to show changed files
- `o` — open side-by-side diff for the selected file (within an expanded commit)
- `y` — copy commit hash (on a commit) or file path (on a file)
- `b` — open commit in browser (GitHub/GitLab)
- `r` — refresh the log
- `d` — pop stash (on a stash entry)
- `p` — push stash
- `/` — activate search/filter input field
- `K` / `Enter` — show hover popup with commit details (author, date, message, stat)

**Blame-to-panel navigation:** The `:Gshow <hash>` command navigates to the Git Log panel and expands the specified commit, replacing the old scratch-buffer behavior. This is also triggered by "Open Commit" links in blame hover popups.

---

### Workspaces

A `.vimcode-workspace` file at the project root captures per-project settings and enables session restoration. Workspace settings overlay your global `settings.json`. Sessions (open files, cursor positions) are stored per-directory and restored automatically.

**Commands:** `:OpenFolder <path>`, `:OpenWorkspace <path>`, `:SaveWorkspaceAs <path>`, `:cd <path>`, `:OpenRecent`

See the **[Settings Reference](https://github.com/JDonaghy/vimcode/wiki/Settings-Reference)** wiki page for workspace file format and session details.

---

### Lua Plugin Extensions

VimCode embeds Lua 5.4 (fully vendored — no system Lua required). Plugins live in `~/.config/vimcode/plugins/` as `.lua` files or directories with `init.lua`. The `vimcode.*` global provides APIs for buffer manipulation, event hooks, custom commands, keymaps, settings, git operations, and custom sidebar panels.

**Example** (`~/.config/vimcode/plugins/hello.lua`):
```lua
vimcode.command("Hello", function(args)
  vimcode.message("Hello from Lua! " .. args)
end)

vimcode.on("save", function(path)
  vimcode.message("Saved: " .. path)
end)
```

**Command URIs** — Hover popup markdown supports `[label](command:Name?args)` links that dispatch to plugin commands registered with `vimcode.command()`. Arguments are percent-decoded automatically.

**Commands:** `:Plugin list` | `:Plugin reload` | `:Plugin enable <name>` | `:Plugin disable <name>`

For the full API reference, see the **[Lua Plugin API](https://github.com/JDonaghy/vimcode/wiki/Lua-Plugin-API)** wiki page.

---

### Language Extensions

Language extensions bundle an LSP server, optional DAP debugger, and Lua scripts into a single package. Browse and install extensions from the **Extensions panel** (click the extensions icon in the activity bar) or install directly with `:ExtInstall <name>`. Extensions are fetched from the [vimcode-ext](https://github.com/JDonaghy/vimcode-ext) registry and cached locally.

| Extension | Language | LSP | DAP |
|-----------|----------|-----|-----|
| `rust` | Rust | rust-analyzer | codelldb |
| `python` | Python | pyright | debugpy |
| `javascript` | JS / TypeScript | typescript-language-server | — |
| `go` | Go | gopls | delve |
| `cpp` | C / C++ | clangd | codelldb |
| `csharp` | C# / .NET | csharp-ls | netcoredbg |
| `java` | Java | jdtls | — |
| `php` | PHP | intelephense | — |
| `ruby` | Ruby | ruby-lsp | — |
| `bash` | Bash | bash-language-server | — |
| `json` | JSON | vscode-json-languageserver | — |
| `xml` | XML | lemminx | — |
| `yaml` | YAML | yaml-language-server | — |
| `markdown` | Markdown | marksman | — |
| `terraform` | Terraform | terraform-ls | — |
| `bicep` | Bicep | bicep-langserver | — |
| `git-insights` | (all files) | — | — |

**Commands:** `:ExtInstall <name>` | `:ExtRemove <name>` | `:ExtList` | `:ExtEnable <name>` | `:ExtDisable <name>` | `:ExtRefresh`

Browse extensions in the sidebar (click the extensions icon in the activity bar). You can also develop and test extensions locally — see the **[Extension Development](https://github.com/JDonaghy/vimcode/wiki/Extension-Development)** wiki page.

---

### AI Assistant

Built-in AI chat panel supporting Anthropic Claude, OpenAI, or local Ollama. Click the chat icon in the activity bar to open. Configure `ai_provider` and `ai_api_key` in `settings.json` (or set `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` env vars).

- `i` — enter input mode; type and press `Enter` to send
- `:AI <message>` — send from command mode; `:AiClear` — clear history
- **AI inline completions** — set `ai_completions: true` for ghost-text suggestions in insert mode (`Tab` accepts, `Alt-]`/`Alt-[` cycle alternatives)

---

### LSP Support (Language Server Protocol)

Automatic language server integration — open a file and diagnostics, completions, go-to-definition, and hover just work if the server is on `PATH`. Install language support via `:ExtInstall <lang>`.

**Features:** inline diagnostics, `]d`/`[d` navigation, auto-popup completions (`Ctrl-Space` manual trigger), `gd` definition, `gr` references, `gi` implementation, `gy` type definition, `K` hover, `gh` editor hover popup, signature help, `<leader>gf` format, `<leader>rn` rename, `<leader>ca` code actions, lightbulb gutter indicator, semantic token highlighting.

**Commands:** `:LspInfo` | `:LspRestart` | `:LspStop` | `:Lformat` | `:Rename <name>` | `:CodeAction`

For custom server configuration and troubleshooting, see the **[LSP Configuration](https://github.com/JDonaghy/vimcode/wiki/LSP-Configuration)** wiki page.

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
| `hidesingletab` / `nohidesingletab` | `hst` | off | Hide tab bar when editor group has only one tab |
| `ignorecase` / `noignorecase` | `ic` | off | Case-insensitive search |
| `smartcase` / `nosmartcase` | `scs` | off | Override `ignorecase` when pattern has uppercase |
| `scrolloff=N` | `so` | 0 | Lines to keep above/below cursor when scrolling |
| `cursorline` / `nocursorline` | `cul` | on | Highlight the line the cursor is on |
| `windowstatusline` / `nowindowstatusline` | `wsl` | on | Per-window status line instead of single global bar (includes layout toggle icons) |
| `statuslineaboveterminal` / `nostatuslineaboveterminal` | `slat` | on | Show active window's status line above the terminal panel instead of inside each window |
| `colorcolumn=N` | `cc` | "" | Comma-list of column guides to highlight |
| `textwidth=N` | `tw` | 0 | Auto-wrap inserted text at column N (0=off) |
| `wrap` / `nowrap` | | off | Soft-wrap long lines at viewport edge |
| `splitbelow` / `nosplitbelow` | `sb` | off | Horizontal splits open below current window |
| `splitright` / `nosplitright` | `spr` | off | Vertical splits open to right of current window |
| `autoread` / `noautoread` | `ar` | on | Automatically reload files modified on disk |
| `lsp` / `nolsp` | | on | Enable/disable LSP language servers |
| `formatonsave` / `noformatonsave` | `fos` | off | Auto-format buffer via LSP before saving |
| `spell` / `nospell` | | off | Enable spell checking (wavy underline on misspelled words) |
| `spelllang=XX` | | `en_US` | Spell check language (currently only `en_US` is bundled) |
| `explorersortcaseinsensitive` / `noexplorersortcaseinsensitive` | `esci` | on | Case-insensitive sorting in the file explorer |
| `mode=vim` / `mode=vscode` | | vim | Editor mode (see **VSCode Mode** below) |

- `:set option?` — query current value; `:set option!` — toggle boolean; `:set` — show all
- `:Settings` — open `settings.json` for direct editing
- `:colorscheme <name>` — switch theme (`onedark`, `gruvbox-dark`, `tokyo-night`, `solarized-dark`, `vscode-dark`, `vscode-light`, or custom VSCode `.json` themes from `~/.config/vimcode/themes/`)
- **Settings sidebar** — click the gear icon for a VSCode-style interactive form

Additional settings (AI, terminal, swap files, indent guides, etc.), configurable key bindings (`panel_keys`, `explorer_keys`, `completion_keys`), and user key mappings are documented in the **[Settings Reference](https://github.com/JDonaghy/vimcode/wiki/Settings-Reference)** and **[Key Remapping](https://github.com/JDonaghy/vimcode/wiki/Key-Remapping)** wiki pages.

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
- `Ctrl-/` — toggle line comment (language-aware, 46+ languages)
- `Shift+Arrow` — extend selection one character/line at a time
- `Ctrl+Arrow` — move by word
- `Ctrl+Shift+Arrow` — extend selection by word
- `Home` — smart home (first non-whitespace; again → col 0)
- `Shift+Home` / `Shift+End` — extend selection to line start/end
- `Escape` — clear selection (stays in insert)
- `Ctrl-Q` — quit
- `F1` — command palette (search and run any command — also works in Vim mode)
- `F10` — toggle menu bar visibility
- Typing while a selection is active **replaces** the selection
- Status bar shows `EDIT  F1:palette  Alt-M:vim` (or `SELECT` when text is selected, `COMMAND` in command bar)

**Line Operations (Phase 1):**
- `Alt+Up` / `Alt+Down` — move line or selection up/down
- `Alt+Shift+Up` / `Alt+Shift+Down` — duplicate line or selection up/down
- `Ctrl+Shift+K` — delete entire line
- `Ctrl+Enter` — insert blank line below (cursor stays)
- `Ctrl+Shift+Enter` — insert blank line above (cursor stays)
- `Ctrl+L` — select current line (repeat to extend)

**Multi-Cursor + Indentation (Phase 2):**
- `Ctrl+D` — select word under cursor; repeat to add next occurrence
- `Ctrl+Shift+L` — select all occurrences of the current word
- `Ctrl+]` — indent line/selection
- `Ctrl+[` — outdent line/selection
- `Shift+Tab` — outdent

**Quick Navigation + Panels (Phase 3):**
- `Ctrl+G` — go to line number
- `Ctrl+P` — quick file open (fuzzy finder)
- `Ctrl+Shift+P` / `F1` — command palette
- `Ctrl+B` — toggle sidebar
- `Ctrl+J` — toggle terminal panel
- `` Ctrl+` `` — toggle terminal panel
- `Ctrl+,` — open settings

VSCode mode supports the same `:map` remapping system as Vim mode — see the **[Key Remapping](https://github.com/JDonaghy/vimcode/wiki/Key-Remapping)** wiki page. To open the keymaps editor in VSCode mode, use `F1` → search "Keymaps" → Enter (or type `:Keymaps` in the command bar). The `editor_mode` setting is persisted in `settings.json`.

---

### Session Persistence

All state lives in `~/.config/vimcode/`. Open files, cursor positions, command/search history, window geometry, and explorer state are restored on startup. Per-project sessions are stored separately when using workspaces. See the **[Settings Reference](https://github.com/JDonaghy/vimcode/wiki/Settings-Reference)** wiki page for details.

---

### Rendering

All three GUI/TUI backends consume the same `ScreenLayout` abstraction from `render.rs` — shared hit-testing, key-binding matching, and scrollbar geometry ensure consistent behavior across platforms.

**Syntax highlighting** (Tree-sitter, auto-detected by extension)
- Rust, Python, JavaScript, TypeScript/TSX, Go, C, C++, C#, Java, Ruby, Bash, Lua, JSON, TOML, CSS, YAML, HTML, Markdown, LaTeX, LaTeX
- LSP semantic token overlay (22 token types) enhances tree-sitter colors when available

**Line numbers** — absolute / relative / hybrid (both on = hybrid)

**Scrollbars** (all backends)
- Per-window vertical scrollbar with cursor position indicator
- Per-window horizontal scrollbar (shown when content is wider than viewport)
- Scrollbar click-to-jump and drag support

**Per-window status line** — mode, filename, branch, filetype, indentation, encoding, line ending, Ln:Col, LSP status; clickable segments open pickers (language, indentation, line ending, branch); layout toggle icons (sidebar, terminal, menu bar) with Nerd Font glyphs or `[S]`/`[T]`/`[M]` fallbacks — dimmed when inactive

**Font** — configurable family and size via `settings.json`

---

### TUI Backend (Terminal UI)

Full editor in the terminal via ratatui + crossterm — feature-parity with the GUI backends. Runs on Linux, macOS, and Windows.

- **Layout:** activity bar (3 cols) | sidebar | editor area; status line + command line full-width at bottom
- **Sidebar:** same file explorer as GTK with Nerd Font icons
- **Mouse support:** click-to-position, double-click word select, click-and-drag visual selection, window switching, scroll wheel (targets pane under cursor), scrollbar click-to-jump and drag; drag event coalescing for smooth scrollbar tracking; bracketed paste support; click branch name in status bar to open branch picker
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
| `+` / `-` | First non-blank of next / previous line |
| `_` | First non-blank of Nth-1 line down |
| `\|` | Go to column N |
| `f{c}` / `t{c}` | Find / till char (`;` `,` repeat) |
| `%` | Jump to matching bracket |
| `zz` / `zt` / `zb` | Scroll cursor to center / top / bottom |
| `z<CR>` / `z.` / `z-` | Scroll top/center/bottom + first non-blank |
| `zh` / `zl` | Scroll horizontally left / right (with count) |
| `zH` / `zL` | Scroll half-screen horizontally left / right |
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
| `=` operator | Auto-indent range (`==` current line, `=G` to end, `=gg` whole file) |
| `d`/`c`/`y` + motion | Full operator+motion support: `dj`/`dk`/`dG`/`dgg`/`d{`/`d}`/`d(`/`d)`/`dW`/`dB`/`dE`/`d^`/`dh`/`dl`/`dH`/`dM`/`dL`/`df`/`dt`/`dF`/`dT`/`d;`/`d,`/`dge` |
| `g~`/`gu`/`gU` + motion | Case operators: all motions (`g~j`, `guw`, `gUG`, `gufx`, etc.) |
| `gcc` / `gc` (visual) | Toggle line comments (core feature — 46+ languages, block comments for HTML/CSS/XML) |
| `>`/`<` + motion | Indent/dedent: all motions (`>j`, `>G`, `>}`, etc.) |
| `gp` / `gP` | Paste after / before, leave cursor after pasted text |
| `]p` / `[p` | Paste after / before with indent adjusted to current line |
| `&` | Repeat last `:s` substitution on current line |
| `g&` | Repeat last `:s` on all lines |
| `@:` | Repeat last ex command |
| `ga` | Print ASCII value of character under cursor |
| `g8` | Print UTF-8 hex bytes of character under cursor |
| `go` | Go to byte offset N in file |
| `gm` | Move cursor to middle of screen line |
| `gM` | Move cursor to middle of text line |
| `gI` | Insert at column 1 (absolute beginning of line) |
| `gx` | Open URL/file under cursor in default application |
| `g'`/`` g` `` | Jump to mark without pushing to jump list |
| `gq{motion}` | Format text (reflow to textwidth) |
| `gw{motion}` | Format text, keep cursor position |
| `g?{motion}` | ROT13 encode text |
| `!{motion}{cmd}` | Filter lines through external command |
| `N%` | Go to N% of file (e.g. `50%` goes to middle) |
| `CTRL-^` | Switch to alternate (last edited) buffer |
| `CTRL-L` | Clear message / redraw screen |
| `>>` / `<<` | Indent / dedent line(s) by `shiftwidth` |
| `*` / `#` | Search forward / backward for word under cursor (word-bounded) |
| `g*` / `g#` | Search forward / backward for word under cursor (partial match) |
| `gf` | Open file path under cursor |
| `v` / `V` / `Ctrl-V` | Visual / Visual Line / Visual Block |
| `/` / `?` | Search forward / backward |
| `n` / `N` | Next / previous match |
| `m{a-z}` / `'{a-z}` | Set mark / jump to mark |
| `q{a-z}` / `@{a-z}` | Record macro / play macro |
| `q:` | Open command-line history window (Enter executes, `q` closes) |
| `q/` / `q?` | Open search history window (Enter searches, `q` closes) |
| `gt` / `gT` | Next / previous tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | MRU tab switcher (forward / backward) |
| `Alt+t` | MRU tab switcher (TUI + GTK compatible) |
| `Ctrl+Alt+Left` / `Ctrl+Alt+Right` | Navigate back / forward through tab history |
| `gd` | Go to definition (LSP) |
| `gr` | Find references (LSP) — multiple results open quickfix |
| `gi` | Insert at last insert position |
| `<leader>gi` | Go to implementation (LSP) |
| `gy` | Go to type definition (LSP) |
| `gs` | Stage hunk (in `:Gdiff` buffer) |
| `gD` | Diff peek — preview hunk popup with Revert/Stage |
| `gh` | Editor hover popup — aggregates diagnostics, annotations, plugin content, and LSP hover at cursor; `y`/Ctrl-C copies selected text (or all text if no selection); mouse drag to select |
| `gR` | Enter virtual replace mode (expands tabs to spaces when overwriting) |
| `g+` / `g-` | Go to newer / older text state (chronological undo timeline) |
| `K` | Show hover info (LSP) |
| `]c` / `[c` | Next / previous change (works on real files + diff buffers) |
| `]d` / `[d` | Next / previous diagnostic (LSP) |
| `[[` / `]]` | Section backward / forward (`{` in col 0; LaTeX: `\section`/`\chapter`/etc.) |
| `[]` / `][` | Section end backward / forward (`}` in col 0; LaTeX: `\end{}`) |
| `[m` / `]m` | Method start backward / forward (LaTeX: `\begin{}`) |
| `[M` / `]M` | Method end backward / forward (LaTeX: `\end{}`) |
| `[{` / `]}` | Jump to unmatched `{` / `}` |
| `[(` / `])` | Jump to unmatched `(` / `)` |
| `[*` / `]*` | Jump to comment block start / end (`/*`/`*/`) |
| `[#` / `]#` | Jump to previous / next unmatched `#if`/`#else`/`#endif` preprocessor directive |
| `[z` / `]z` | Jump to fold start / end |
| `do` | Diff obtain (pull line from other diff window) |
| `dp` | Diff put (push line to other diff window) |
| `<leader>gf` | LSP format current buffer (Space=leader by default) |
| `<leader>rn` | LSP rename symbol — pre-fills `:Rename <word>` |
| `<leader>ca` | Show LSP code actions for current line |
| `<leader>sf` | Open fuzzy file finder (same as Ctrl-P) |
| `<leader>sg` | Open live grep picker (same as Ctrl-Shift-F) |
| `<leader>sw` | Grep word under cursor |
| `<leader>sb` | Open buffer picker (fuzzy search open buffers) |
| `<leader>sk` | Search key bindings (fuzzy-filterable reference) |
| `<leader>so` | Go to symbol in editor (document outline via LSP) |
| `<leader>b` | Enter breadcrumb focus mode (h/l navigate, Enter opens scoped picker) |
| `<leader>sp` | Open command palette (same as Ctrl-Shift-P) |
| `za` / `zo` / `zc` / `zR` | Fold toggle / open / close / open all |
| `zA` / `zO` / `zC` | Fold toggle / open / close recursively |
| `zM` | Close all folds |
| `zd` / `zD` | Delete fold / delete fold recursively |
| `zf{motion}` / `zF` | Create fold (operator) / create fold for N lines |
| `zv` | Open folds to make cursor visible |
| `zx` | Recompute folds (open all + close all) |
| `zj` / `zk` | Move to next / previous fold |
| `zs` / `ze` | Scroll cursor to left / right edge of screen |
| `]s` / `[s` | Next / previous spelling error |
| `z=` | Show spelling suggestions for word under cursor |
| `zg` | Add word under cursor to user dictionary |
| `zw` | Mark word under cursor as misspelled (add to wrong-word list) |
| `Ctrl-W h/j/k/l` | Focus window left/down/up/right (`:wincmd h/j/k/l`) |
| `Ctrl-W w` / `c` / `o` / `q` / `n` | Cycle / close / close-others / quit / new (`:wincmd w/c/o/q/n`) |
| `Ctrl-W +` / `-` / `>` / `<` | Resize split height/width (`:wincmd +/-/>/<`) |
| `Ctrl-W =` | Equalize all split sizes (`:wincmd =`) |
| `Ctrl-W _` / `\|` | Maximize split height / width (`:wincmd _/\|`) |
| `Ctrl-W p` / `t` / `b` | Previous / top / bottom editor group (`:wincmd p/t/b`) |
| `Ctrl-W H` / `J` / `K` / `L` | Move window to far left/bottom/top/right (`:wincmd H/J/K/L`) |
| `Ctrl-W T` | Move window to new editor group (`:wincmd T`) |
| `Ctrl-W x` | Exchange with next window (`:wincmd x`) |
| `Ctrl-W r` / `R` | Rotate windows forward / backward (`:wincmd r/R`) |
| `Ctrl-W f` | Split and open file under cursor (`:wincmd f`) |
| `Ctrl-W d` | Split and go to definition (LSP) (`:wincmd d`) |
| `Ctrl-P` | Open fuzzy file finder |
| `Ctrl-Shift-F` | Open live grep picker |
| `Ctrl-G` | Show file info (name, line, col, %) |
| `Ctrl-Shift-P` / `F1` | Command palette |
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

All ex commands support Vim-style abbreviations (e.g., `:j` for `:join`, `:y` for `:yank`, `:ve` for `:version`). The shortest unambiguous prefix works.

| Command | Action |
|---------|--------|
| `:w` / `:wq` | Save / save and quit |
| `:wa` | Write all dirty buffers |
| `:wqa` / `:xa` | Write all and quit |
| `:q` / `:q!` / `:qa` / `:qa!` | Quit / force / all / force-all |
| `:e <file>` | Open file |
| `:e!` | Reload current file from disk (discard changes) |
| `:split` / `:vsplit` | Horizontal / vertical split |
| `:tabnew` / `:tabclose` | New tab / close tab |
| `:tabs` / `:TabSwitcher` | Open MRU tab switcher popup |
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
| `:j[oin]` | Join current line with the next (remove newline) |
| `:y[ank] [reg]` | Yank current line into register (default `"`) |
| `:pu[t] [reg]` | Put register contents after current line |
| `:>` / `:<` | Indent / dedent current line by one shiftwidth |
| `:=` | Display current line number |
| `:#` / `:nu[mber]` / `:p[rint]` | Print current line with line number |
| `:ma[rk] {a-z}` / `:k{a-z}` | Set mark at current line |
| `:pw[d]` | Print working directory |
| `:f[ile]` | Show current file name and info |
| `:ene[w]` | Open a new empty buffer |
| `:new` / `:vne[w]` | Open new buffer in horizontal / vertical split |
| `:close` / `:only` | Close current window / close all other windows |
| `:winc[md] {char}` | Execute window command (e.g., `:wincmd h` = `Ctrl-W h`) |
| `:up[date]` | Write buffer only if modified |
| `:sav[eas] {file}` | Save buffer to a new file path |
| `:ve[rsion]` | Show VimCode version info |
| `:ret[ab][!] [N]` | Re-apply tabstop: convert indentation (! = retab entire buffer) |
| `:cq[uit]` | Quit with non-zero exit code (error) |
| `:windo {cmd}` | Execute command in every window |
| `:bufdo {cmd}` | Execute command in every buffer |
| `:tabdo {cmd}` | Execute command in every tab |
| `:di[splay]` | Display register contents (alias for `:reg`) |
| `:set [option]` | Change / query setting |
| `:set spell` / `:set nospell` | Enable / disable spell checking |
| `:noh` / `:nohlsearch` | Clear current search highlight |
| `:echo {text}` | Display a message in the status bar |
| `:reg` / `:registers` | Display register contents |
| `:marks` | Display all set marks |
| `:jumps` | Display jump list |
| `:changes` | Display change list |
| `:history` | Display command history |
| `:make [args]` | Run `make` with optional arguments |
| `:b {name}` | Switch to buffer matching partial file name |
| `:!{cmd}` | Execute shell command and show output |
| `:r {file}` | Read file contents into buffer after cursor line |
| `:tabmove [N]` | Move current tab to position N (1-based, 0 = end) |
| `:navback` | Navigate to previous tab in history |
| `:navforward` | Navigate to next tab in history |
| `:Gdiff` / `:Gdiffsplit` | Git diff (unified / side-by-side) |
| `:Gstatus` | Git status |
| `:Gadd` / `:Gadd!` | Stage file / stage all |
| `:Gcommit <msg>` | Commit |
| `:Gpush` | Push |
| `:Gpull` / `:Gpl` | Pull |
| `:Gfetch` / `:Gf` | Fetch |
| `:Gblame` | Blame (scroll-synced split) |
| `:Gswitch <branch>` / `:Gsw` | Switch to existing branch |
| `:Gbranch <name>` | Create new branch and switch to it |
| `:Gbranches` | Open branch picker (status bar click also works) |
| `:Ghs` / `:Ghunk` | Stage hunk under cursor |
| `:Gshow <hash>` | Show commit in Git Log panel (navigates and expands) |
| `:DiffPeek` | Open diff hunk peek popup at cursor (revert/stage) |
| `:GWorktreeAdd <branch> <path>` | Add git worktree |
| `:GWorktreeRemove <path>` | Remove git worktree |
| `:OpenFolder <path>` | Open folder (clears buffers, loads per-project session) |
| `:OpenWorkspace <path>` | Open `.vimcode-workspace` file |
| `:SaveWorkspaceAs <path>` | Save current folder as workspace file |
| `:OpenRecent` | Open recent workspaces picker |
| `:cd <path>` | Change working directory |
| `:diffsplit <file>` | Open file in vsplit with diff highlighting |
| `:diffthis` | Mark current window as diff participant (two calls activate diff) |
| `:DiffNext` | Jump to next change in diff view |
| `:DiffPrev` | Jump to previous change in diff view |
| `:DiffToggleContext` | Toggle hiding unchanged sections in diff view |
| `:diffoff` | Clear diff highlighting |
| `:grep <pat>` / `:vimgrep <pat>` | Search project, populate quickfix list |
| `:GrepWord` | Grep the word under cursor (same as `<leader>sw`) |
| `:Buffers` | Open buffer picker (same as `<leader>sb`) |
| `:copen` / `:ccl` | Open / close quickfix panel |
| `:cn` / `:cp` | Next / previous quickfix item |
| `:cc N` | Jump to Nth quickfix item (1-based) |
| `:LspInfo` | Show running LSP servers |
| `:LspRestart` | Restart server for current language |
| `:LspStop` | Stop server for current language |
| `:LspInstall <lang>` | Install LSP server for language via Mason |
| `:Lformat` | Format buffer via LSP |
| `:Rename <newname>` | Rename symbol under cursor across workspace |
| `:CodeAction` | Show LSP code actions for current line |
| `:def` | Go to definition (LSP) |
| `:refs` | Find references (LSP) |
| `:hover` | Show hover info (LSP) |
| `:LspImpl` | Go to implementation (LSP) |
| `:LspTypedef` | Go to type definition (LSP) |
| `:nextdiag` / `:prevdiag` | Jump to next / previous LSP diagnostic |
| `:nexthunk` / `:prevhunk` | Jump to next / previous git hunk |
| `:fuzzy` | Open fuzzy file finder |
| `:CommandCenter` | Open Command Center (unified picker with prefix modes) |
| `:sidebar` | Toggle sidebar |
| `:palette` | Open command palette |
| `:Comment [N]` | Toggle comment on N lines (46+ languages; `:Commentary` alias) |
| `:DapInstall <lang>` | Install debug adapter for language |
| `:DapInfo` | Show detected DAP adapters |
| `:DapEval <expr>` | Evaluate expression in current debug frame |
| `:DapWatch <expr>` | Add watch expression to debug sidebar |
| `:EditorGroupSplit` / `:egsp` | Split editor right (new editor group) |
| `:EditorGroupSplitDown` / `:egspd` | Split editor down |
| `:EditorGroupClose` / `:egc` | Close active editor group |
| `:EditorGroupFocus` / `:egf` | Toggle focus between editor groups |
| `:EditorGroupMoveTab` / `:egmt` | Move current tab to other editor group |
| `:Plugin list` | List loaded plugins |
| `:Plugin reload` | Reload plugins from disk |
| `:Plugin enable <name>` | Enable a plugin |
| `:Plugin disable <name>` | Disable a plugin |
| `:map` / `:map n K :cmd` | List keymaps / add a key mapping |
| `:unmap n K` | Remove a key mapping |
| `:Keymaps` | Open keymaps editor (scratch buffer, one per line, `:w` saves to settings) |
| `:ExtInstall <name>` | Install a language extension (LSP + DAP + Lua scripts) |
| `:ExtList` | List available extensions and their install status |
| `:ExtEnable <name>` | Re-enable a disabled extension |
| `:ExtDisable <name>` | Disable an extension (suppress install prompts) |
| `:config reload` | Reload settings from disk |
| `:AI <message>` | Send a message to the AI assistant |
| `:AiClear` | Clear the AI conversation history |
| `:MarkdownPreview` / `:MdPreview` | Open side-by-side styled markdown preview (live-updates on edit, scroll sync, scaled headings in GTK) |
| `:Explore [dir]` / `:Ex [dir]` | Open netrw-style in-buffer directory listing |
| `:Sexplore [dir]` / `:Sex [dir]` | Horizontal split + netrw directory listing |
| `:Vexplore [dir]` / `:Vex [dir]` | Vertical split + netrw directory listing |
| `:help [topic]` / `:h [topic]` | Show help (topics: explorer, keys, commands) |

---

## Architecture

```
src/                  (~137,000 lines total)
├── main.rs              (~57 lines)  Thin CLI dispatcher → gtk::run() or tui_main::run()
├── win_gui_bin.rs       (~36 lines)  Windows native GUI entry point → win_gui::run()
├── gtk/             (~18,156 lines)  GTK4/Relm4 UI backend (Linux + macOS)
│   ├── mod.rs       (~10,029 lines)  App struct, Msg enum, SimpleComponent, geometry helpers, run()
│   ├── draw.rs       (~5,975 lines)  All draw_* rendering functions + Pango attrs
│   ├── click.rs        (~622 lines)  Mouse click/drag/double-click handlers
│   ├── css.rs          (~553 lines)  Theme CSS generation + static CSS
│   ├── util.rs         (~474 lines)  GTK key mapping, settings form builders, URL/icon helpers
│   └── tree.rs         (~503 lines)  File tree construction, expansion tracking, name validation
├── tui_main/        (~14,814 lines)  ratatui/crossterm TUI backend (all platforms)
│   ├── mod.rs        (~4,173 lines)  Structs, event_loop, key translation, clipboard, run()
│   ├── panels.rs     (~4,040 lines)  Activity bar, sidebar, status/command lines, all panel renders
│   ├── render_impl.rs(~3,947 lines)  draw_frame orchestrator, tab bar, editor windows, popups
│   └── mouse.rs      (~2,654 lines)  All mouse click/drag/scroll interaction handling
├── win_gui/         (~10,877 lines)  Native Windows GUI backend (Win32 + Direct2D + DirectWrite)
│   ├── mod.rs        (~6,263 lines)  HWND, D2D render target, event loop, DWM title bar, IME, font install
│   ├── draw.rs       (~4,614 lines)  Direct2D rendering: editor, tabs, sidebar, popups, scrollbar
│   └── input.rs        (~217 lines)  Keyboard and mouse input translation (if present)
├── render.rs         (~9,645 lines)  Platform-agnostic ScreenLayout bridge + shared hit-testing geometry
├── icons.rs           (~160 lines)  Icon registry with Nerd Font + ASCII fallback
└── core/            (~81,824 lines)  Zero GUI/rendering deps — fully testable in isolation
    ├── engine/      (~59,947 lines)  Orchestrator: 20 submodules (keys, motions, buffers, tests, …)
    ├── lsp.rs        (~2,923 lines)  LSP protocol transport + single-server client
    ├── lsp_manager.rs(~1,105 lines)  Multi-server coordinator + semantic token legends
    ├── git.rs        (~2,550 lines)  Git subprocesses: diff, blame, stage, worktrees, log, branches
    ├── settings.rs   (~2,336 lines)  JSON config, :set parsing, key bindings, SETTING_DEFS
    ├── plugin.rs     (~1,936 lines)  Lua 5.4 plugin manager (vendored; vimcode.* API; panel API)
    ├── syntax.rs     (~1,854 lines)  Tree-sitter highlighting for 20 languages (incl. LaTeX via vendored grammar)
    ├── dap_manager.rs(~1,427 lines)  DAP multi-adapter coordinator + launch.json + tasks.json
    ├── buffer_manager.rs(~1,018 lines)  Buffer lifecycle, undo/redo stacks, semantic tokens
    ├── dap.rs          (~719 lines)  DAP protocol transport + event routing
    ├── markdown.rs     (~705 lines)  Markdown → styled plain text converter (pulldown-cmark)
    ├── session.rs      (~782 lines)  Session state persistence + per-workspace paths
    ├── project_search.rs(~631 lines)  Regex/case/whole-word search + replace (ignore + regex crates)
    ├── terminal.rs     (~410 lines)  PTY-backed terminal pane (portable-pty + vt100)
    ├── ai.rs           (~384 lines)  AI provider integration (Anthropic/OpenAI/Ollama)
    ├── spell.rs        (~379 lines)  Spell checker (Hunspell; tree-sitter-aware; LaTeX-aware)
    └── window.rs, tab.rs, view.rs, buffer.rs, cursor.rs, mode.rs, … (~2,718 lines)
```

**Design rule:** `src/core/` has zero GTK/rendering dependencies and is testable in isolation. All four backends (GTK, TUI, Windows native, future macOS native) consume the same `ScreenLayout` abstraction from `render.rs`.

`dictionaries/` — bundled en_US Hunspell dictionary files (`.aff` + `.dic`) compiled into the binary via `include_bytes!`.


## Acknowledgements

*   **Bram Moolenaar (RIP):** The original author of Vim. If you use VimCode and want to give something back, consider honoring his legacy by donating to a charity. See [moolenaar.net/Charityware.html](https://www.moolenaar.net).
*   **Bill Joy:** The creator of the original vi editor.
*   **The VSCode Team:** For setting the standard on what a modern editor UX should look like.
*   **Boris Cherny and the Claude Code team:** VimCode is built with the assistance of [Claude Code](https://claude.ai/claude-code).


## Tech Stack

| Component | Library |
|-----------|---------|
| Language | Rust 2021 |
| GTK UI | GTK4 + Relm4 (Linux, macOS) |
| Windows UI | windows-rs + Direct2D + DirectWrite (native Win32) |
| TUI | ratatui 0.29 + crossterm (all platforms) |
| Text rendering | Pango + Cairo (GTK), DirectWrite (Windows) |
| Text storage | Ropey (rope data structure) |
| Parsing | Tree-sitter (20 languages incl. LaTeX, Lua, Markdown) |
| LSP | lsp-types (protocol definitions) |
| Config | serde + serde_json |
| Plugins | mlua 0.9 (Lua 5.4, vendored) |
| Spell check | spellbook 0.4 (pure-Rust Hunspell parser) |
| File watching | notify (cross-platform filesystem events) |

## License

MIT
