# Vim Compatibility Index

Systematic checklist of Vim commands mapped against `vim-index.txt`. Each command is marked:
- **Status**: ✅ implemented | ⚠️ partial | ❌ not implemented | N/A not applicable
- **Notes**: VimCode-specific behavior or equivalent

> **VimCode does not implement VimScript.** Extension and scripting is handled via the built-in Lua 5.4 plugin system. The goal is full Vim *keybinding* and *editing* compatibility, not a VimScript runtime. Commands that only make sense in the context of VimScript (`:let`, `:if`, `:function`, `:autocmd`, `:source`, `:execute`, `:call`, etc.) are marked N/A.

See [README.md](README.md) for full feature documentation.

---

## Insert Mode

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `<Esc>` | End insert mode, back to Normal | ✅ | Also collapses multi-cursors |
| `<CR>` | Insert newline | ✅ | Auto-indent when `autoindent` enabled |
| `<BS>` | Delete character before cursor | ✅ | Joins lines at col 0; multi-cursor |
| `<Del>` | Delete character under cursor | ✅ | Multi-cursor support |
| `<Tab>` | Insert tab or spaces | ✅ | Respects `expandtab`/`tabstop`; also accepts completion/ghost text |
| `<Up>/<Down>/<Left>/<Right>` | Cursor movement | ✅ | |
| `<Home>/<End>` | Line start/end | ✅ | |
| `CTRL-H` | Delete character before cursor | ⚠️ | Terminal maps to `<BS>` |
| `CTRL-W` | Delete word before cursor | ✅ | |
| `CTRL-U` | Delete to start of line | ✅ | |
| `CTRL-T` | Indent current line by shiftwidth | ✅ | |
| `CTRL-D` | Dedent current line by shiftwidth | ✅ | |
| `CTRL-R {reg}` | Insert contents of register | ✅ | Two-key sequence |
| `CTRL-N` | Next completion match | ✅ | Auto-popup + manual completion |
| `CTRL-P` | Previous completion match | ✅ | Auto-popup + manual completion |
| `CTRL-O` | Execute one Normal command | ⚠️ | Switches to Normal; no auto-return to Insert |
| `CTRL-E` | Insert character below cursor | ✅ | |
| `CTRL-Y` | Insert character above cursor | ✅ | |
| `CTRL-A` | Insert previously inserted text | ✅ | |
| `CTRL-@` | Insert prev text and stop insert | ❌ | |
| `CTRL-V {char}` | Insert literal character | ❌ | |
| `CTRL-K {c1}{c2}` | Enter digraph | N/A | No digraph support planned |
| `CTRL-X ...` | Completion sub-modes | ❌ | VimCode uses auto-popup + LSP instead |
| `CTRL-]` | Trigger abbreviation | N/A | No abbreviations |
| `CTRL-G u` | Break undo sequence | ✅ | |
| `CTRL-G j/k` | Move cursor down/up | ✅ | |

**Insert mode: 18/24 (75%)**

---

## Normal Mode — Movement

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `h` | Left | ✅ | Count supported |
| `j` | Down | ✅ | Count supported |
| `k` | Up | ✅ | Count supported |
| `l` | Right | ✅ | Count supported |
| `w` | Word forward | ✅ | |
| `b` | Word backward | ✅ | |
| `e` | End of word | ✅ | |
| `ge` | End of previous word | ✅ | |
| `W` | WORD forward | ✅ | |
| `B` | WORD backward | ✅ | |
| `E` | End of WORD | ✅ | |
| `gE` | End of previous WORD | ✅ | |
| `0` | Line start | ✅ | |
| `^` | First non-blank | ✅ | |
| `$` | Line end | ✅ | |
| `g_` | Last non-blank | ✅ | |
| `g0` | Start of screen line | ✅ | Wrap mode |
| `gm` | Middle of screen line | ✅ | |
| `gM` | Middle of text line | ✅ | |
| `f{char}` | Find char forward | ✅ | |
| `F{char}` | Find char backward | ✅ | |
| `t{char}` | Till char forward | ✅ | |
| `T{char}` | Till char backward | ✅ | |
| `;` | Repeat last f/t/F/T | ✅ | |
| `,` | Repeat last f/t/F/T reversed | ✅ | |
| `gg` | Go to first line | ✅ | `{N}gg` goes to line N |
| `G` | Go to last line | ✅ | `{N}G` goes to line N |
| `{` | Paragraph backward | ✅ | |
| `}` | Paragraph forward | ✅ | |
| `(` | Sentence backward | ✅ | |
| `)` | Sentence forward | ✅ | |
| `H` | Screen top | ✅ | |
| `M` | Screen middle | ✅ | |
| `L` | Screen bottom | ✅ | |
| `%` | Matching bracket | ✅ | Forward search when not on bracket |
| `+` | First char N lines down | ✅ | |
| `-` | First char N lines up | ✅ | |
| `_` | First char N-1 lines down | ✅ | |
| `\|` | Go to column N | ✅ | |
| `N%` | Go to N% of file | ✅ | |
| `gj` | Down screen line | ✅ | Wrap mode |
| `gk` | Up screen line | ✅ | Wrap mode |
| `CTRL-D` | Half-page down | ✅ | |
| `CTRL-U` | Half-page up | ✅ | |
| `CTRL-F` | Page down | ✅ | |
| `CTRL-B` | Page up | ✅ | |
| `CTRL-E` | Scroll down one line | ✅ | |
| `CTRL-Y` | Scroll up one line | ✅ | |

**Movement: 48/48 (100%)**

---

## Normal Mode — Editing

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `i` | Insert before cursor | ✅ | |
| `I` | Insert at first non-blank | ✅ | |
| `a` | Append after cursor | ✅ | |
| `A` | Append at end of line | ✅ | |
| `o` | Open line below | ✅ | Auto-indent |
| `O` | Open line above | ✅ | Auto-indent |
| `x` | Delete char under cursor | ✅ | Count supported |
| `X` | Delete char before cursor | ✅ | |
| `d{motion}` | Delete | ✅ | All motions supported |
| `dd` | Delete line(s) | ✅ | Count supported |
| `D` | Delete to end of line | ✅ | |
| `c{motion}` | Change | ✅ | All motions supported |
| `cc` | Change line(s) | ✅ | |
| `C` | Change to end of line | ✅ | |
| `s` | Substitute character | ✅ | |
| `S` | Substitute line | ✅ | |
| `y{motion}` | Yank | ✅ | All motions supported |
| `yy` | Yank line(s) | ✅ | |
| `Y` | Yank line(s) | ✅ | |
| `p` | Paste after | ✅ | Linewise/charwise aware |
| `P` | Paste before | ✅ | Linewise/charwise aware |
| `]p` | Paste with indent adjust | ✅ | |
| `[p` | Paste before with indent | ✅ | |
| `gp` | Put, leave cursor after | ✅ | |
| `gP` | Put before, leave cursor after | ✅ | |
| `r{char}` | Replace character | ✅ | Count supported |
| `R` | Replace mode | ✅ | Overtype until Escape |
| `J` | Join lines | ✅ | |
| `gJ` | Join lines without space | ✅ | |
| `u` | Undo | ✅ | |
| `U` | Undo line | ✅ | |
| `CTRL-R` | Redo | ✅ | |
| `.` | Repeat last change | ✅ | |
| `~` | Toggle case | ✅ | Count supported |
| `g~{motion}` | Toggle case operator | ✅ | All motions |
| `gu{motion}` | Lowercase operator | ✅ | All motions |
| `gU{motion}` | Uppercase operator | ✅ | All motions |
| `>{motion}` | Indent | ✅ | |
| `>>` | Indent line | ✅ | |
| `<{motion}` | Dedent | ✅ | |
| `<<` | Dedent line | ✅ | |
| `={motion}` | Auto-indent | ✅ | |
| `==` | Auto-indent line | ✅ | |
| `CTRL-A` | Increment number | ✅ | |
| `CTRL-X` | Decrement number | ✅ | |
| `gq{motion}` | Format text | ✅ | Reflows to textwidth |
| `gw{motion}` | Format text, keep cursor | ✅ | Reflows to textwidth |
| `!{motion}{filter}` | Filter through command | ❌ | |
| `&` | Repeat last `:s` | ✅ | |
| `g&` | Repeat last `:s` on all lines | ✅ | |

**Editing: 50/50 (100%)**

---

## Normal Mode — Search & Marks

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `/pattern` | Search forward | ✅ | Incremental highlight |
| `?pattern` | Search backward | ✅ | Incremental highlight |
| `n` | Next match | ✅ | Direction-aware |
| `N` | Previous match | ✅ | |
| `*` | Search word forward | ✅ | Word-bounded |
| `#` | Search word backward | ✅ | Word-bounded |
| `g*` | Search word forward | ✅ | Partial match |
| `g#` | Search word backward | ✅ | Partial match |
| `gn` | Select next match | ✅ | |
| `gN` | Select prev match | ✅ | |
| `m{a-z}` | Set local mark | ✅ | |
| `m{A-Z}` | Set global mark | ✅ | |
| `'{a-z}` | Jump to mark line | ✅ | |
| `` `{a-z} `` | Jump to mark position | ✅ | |
| `'{A-Z}` | Jump to global mark line | ✅ | |
| `` `{A-Z} `` | Jump to global mark position | ✅ | |
| `''` | Jump to prev position line | ✅ | |
| ` `` ` | Jump to prev position | ✅ | |
| `'.` | Jump to last edit line | ✅ | |
| `` `. `` | Jump to last edit position | ✅ | |
| `'<` / `'>` | Jump to visual selection | ✅ | |
| `CTRL-O` | Jump list back | ✅ | |
| `CTRL-I` | Jump list forward | ✅ | |
| `g;` | Older change position | ✅ | |
| `g,` | Newer change position | ✅ | |
| `g'` / `` g` `` | Mark jump without jumplist | ❌ | |

**Search & Marks: 25/26 (96%)**

---

## Normal Mode — Other

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `q{a-z}` | Record macro | ✅ | |
| `q` | Stop recording | ✅ | |
| `@{a-z}` | Play macro | ✅ | Count supported |
| `@@` | Repeat last macro | ✅ | |
| `@:` | Repeat last ex command | ✅ | |
| `"{reg}` | Use register | ✅ | a-z, A-Z, 0-9, special |
| `v` | Enter Visual mode | ✅ | |
| `V` | Enter Visual Line mode | ✅ | |
| `CTRL-V` | Enter Visual Block mode | ✅ | |
| `gv` | Reselect last visual | ✅ | |
| `:` | Enter Command mode | ✅ | |
| `gt` | Next tab | ✅ | |
| `gT` | Previous tab | ✅ | |
| `gd` | Go to definition | ✅ | LSP-based |
| `gf` | Go to file under cursor | ✅ | |
| `gF` | Go to file at line | ✅ | |
| `K` | Show hover info | ✅ | LSP-based |
| `ga` | Print ASCII value | ✅ | |
| `g8` | Print UTF-8 hex bytes | ✅ | |
| `go` | Go to byte N | ✅ | |
| `gx` | Open URL/file externally | ✅ | Opens in default browser/app |
| `gi` | Insert at last insert pos | ❌ | `gi` is LSP go-to-implementation |
| `gI` | Insert at column 1 | ✅ | |
| `g?{motion}` | ROT13 encode | ❌ | |
| `CTRL-^` | Edit alternate file | ✅ | |
| `CTRL-]` | Tag jump | ⚠️ | `gd` (LSP) provides equivalent |
| `CTRL-G` | Show file info | ❌ | `:file` command works |
| `CTRL-L` | Redraw screen | ✅ | Clears message |
| `do` | Diff obtain | ❌ | |
| `dp` | Diff put | ❌ | |
| `q:` | Command-line window | ❌ | |
| `q/` / `q?` | Search history window | ❌ | |
| `cgn` | Change next match | ✅ | Repeat with `.` |

**Other: 28/33 (85%)**

---

## Text Objects

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `iw` / `aw` | Inner/around word | ✅ | |
| `iW` / `aW` | Inner/around WORD | ✅ | |
| `is` / `as` | Inner/around sentence | ✅ | |
| `ip` / `ap` | Inner/around paragraph | ✅ | |
| `i"` / `a"` | Inner/around double quotes | ✅ | |
| `i'` / `a'` | Inner/around single quotes | ✅ | |
| `` i` `` / `` a` `` | Inner/around backticks | ✅ | |
| `i(` / `a(` | Inner/around parentheses | ✅ | `ib`/`ab` alias |
| `i)` / `a)` | Inner/around parentheses | ✅ | |
| `i{` / `a{` | Inner/around braces | ✅ | `iB`/`aB` alias |
| `i}` / `a}` | Inner/around braces | ✅ | |
| `i[` / `a[` | Inner/around brackets | ✅ | |
| `i]` / `a]` | Inner/around brackets | ✅ | |
| `i<` / `a<` | Inner/around angle brackets | ✅ | |
| `i>` / `a>` | Inner/around angle brackets | ✅ | |
| `it` / `at` | Inner/around HTML/XML tag | ✅ | Case-insensitive, nesting-aware |

**Text objects: 16/16 (100%)**

---

## g-Commands

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `gg` | Go to first line / line N | ✅ | |
| `g_` | Last non-blank char | ✅ | |
| `g0` | Start of screen line | ✅ | |
| `gj` / `gk` | Visual line up/down | ✅ | Wrap mode |
| `gE` | End of previous WORD | ✅ | |
| `ge` | End of previous word | ✅ | |
| `gn` / `gN` | Select next/prev match | ✅ | |
| `g*` / `g#` | Partial word search | ✅ | |
| `gv` | Reselect visual | ✅ | |
| `gd` | Go to definition | ✅ | LSP-based |
| `gf` | Go to file under cursor | ✅ | |
| `gF` | Go to file at line | ✅ | |
| `gt` / `gT` | Next/prev tab | ✅ | |
| `g~{motion}` | Toggle case | ✅ | |
| `gu{motion}` | Lowercase | ✅ | |
| `gU{motion}` | Uppercase | ✅ | |
| `gJ` | Join without space | ✅ | |
| `g;` / `g,` | Change list nav | ✅ | |
| `g.` | Go to last change | ✅ | |
| `gp` / `gP` | Put, cursor after | ✅ | |
| `gq{motion}` | Format text | ✅ | Reflows to textwidth |
| `gw{motion}` | Format, keep cursor | ✅ | Reflows to textwidth |
| `gx` | Open URL externally | ✅ | Opens in default browser/app |
| `ga` | Print ASCII value | ✅ | |
| `g8` | Print UTF-8 hex | ✅ | |
| `go` | Go to byte N | ✅ | |
| `gi` | Insert at last insert pos | ❌ | `gi` is LSP go-to-implementation |
| `gI` | Insert at column 1 | ✅ | |
| `gm` / `gM` | Middle of screen/text line | ✅ | |
| `g?{motion}` | ROT13 encode | ❌ | |
| `g+` / `g-` | Undo tree newer/older | ❌ | |
| `gR` / `gr` | Virtual replace mode | ❌ | |
| `g'` / `` g` `` | Mark without jumplist | ✅ | |
| `g&` | Repeat `:s` all lines | ✅ | |
| `gH` / `gV` | Select mode | N/A | No Select mode |

**g-commands: 30/34 (88%)**

---

## z-Commands

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `zz` | Center cursor on screen | ✅ | |
| `zt` | Cursor to top of screen | ✅ | |
| `zb` | Cursor to bottom of screen | ✅ | |
| `z<CR>` | Top of screen + first non-blank | ✅ | |
| `z.` | Center + first non-blank | ✅ | |
| `z-` | Bottom + first non-blank | ✅ | |
| `za` | Toggle fold | ✅ | |
| `zo` | Open fold | ✅ | |
| `zc` | Close fold | ✅ | |
| `zR` | Open all folds | ✅ | |
| `zM` | Close all folds | ✅ | |
| `zA` | Toggle fold recursively | ✅ | |
| `zO` | Open fold recursively | ✅ | |
| `zC` | Close fold recursively | ✅ | |
| `zd` | Delete fold | ✅ | |
| `zD` | Delete fold recursively | ✅ | |
| `zf{motion}` | Create fold | ✅ | Supports j/k/G/gg/{/} motions |
| `zF` | Create fold for N lines | ✅ | |
| `zv` | Open folds to show cursor | ✅ | |
| `zx` | Recompute folds | ✅ | |
| `zj` / `zk` | Next/prev fold | ✅ | |
| `zh` / `zl` | Scroll horizontally | ✅ | With count support |
| `zH` / `zL` | Scroll half-screen horiz. | ✅ | |
| `ze` / `zs` | Scroll to cursor right/left | ✅ | |
| `z=` | Spelling suggestions | N/A | No spell check |
| `zg` / `zw` / `zG` / `zW` | Spelling word lists | N/A | No spell check |

**z-commands: 23/23 (100%)** (excluding N/A)

---

## Window Commands (CTRL-W)

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `CTRL-W h` | Focus window left | ✅ | |
| `CTRL-W j` | Focus window below | ✅ | |
| `CTRL-W k` | Focus window above | ✅ | |
| `CTRL-W l` | Focus window right | ✅ | |
| `CTRL-W w` | Focus next window | ✅ | Cycle |
| `CTRL-W W` | Focus previous window | ✅ | |
| `CTRL-W c` | Close window | ✅ | |
| `CTRL-W o` | Close other windows | ✅ | |
| `CTRL-W s` | Split horizontally | ✅ | |
| `CTRL-W v` | Split vertically | ✅ | |
| `CTRL-W e/E` | Split editor group right/down | ✅ | VimCode extension |
| `CTRL-W +` | Increase height | ✅ | Editor group splits |
| `CTRL-W -` | Decrease height | ✅ | Editor group splits |
| `CTRL-W <` | Decrease width | ✅ | Editor group splits |
| `CTRL-W >` | Increase width | ✅ | Editor group splits |
| `CTRL-W =` | Equalize sizes | ✅ | All group splits |
| `CTRL-W _` | Maximize height | ✅ | Editor group splits |
| `CTRL-W \|` | Maximize width | ✅ | Editor group splits |
| `CTRL-W H` | Move window to far left | ❌ | |
| `CTRL-W J` | Move window to far bottom | ❌ | |
| `CTRL-W K` | Move window to far top | ❌ | |
| `CTRL-W L` | Move window to far right | ❌ | |
| `CTRL-W T` | Move window to new tab | ❌ | |
| `CTRL-W x` | Exchange with next window | ❌ | |
| `CTRL-W r` | Rotate windows downward | ❌ | |
| `CTRL-W R` | Rotate windows upward | ❌ | |
| `CTRL-W p` | Go to previous window | ✅ | |
| `CTRL-W n` | New window | ✅ | Runs `:new` |
| `CTRL-W t` | Go to top window | ✅ | |
| `CTRL-W b` | Go to bottom window | ✅ | |
| `CTRL-W q` | Quit window | ✅ | Alias for `CTRL-W c` |
| `CTRL-W ]` | Split + tag jump | N/A | LSP `gd` instead |
| `CTRL-W f` | Split + open file | ✅ | |
| `CTRL-W d` | Split + go to definition | ✅ | LSP-based |

**Window commands: 25/31 (81%)** (excluding N/A)

---

## Bracket Commands ([ and ])

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `]c` / `[c` | Next/prev git hunk | ✅ | Git integration |
| `]d` / `[d` | Next/prev diagnostic | ✅ | LSP integration |
| `]p` / `[p` | Paste with indent adjust | ✅ | |
| `[[` / `]]` | Section backward/forward | ✅ | `{` in column 0 |
| `[]` / `][` | Section end backward/forward | ✅ | `}` in column 0 |
| `[m` / `]m` | Method start backward/forward | ✅ | |
| `[M` / `]M` | Method end backward/forward | ✅ | |
| `[{` / `]}` | Unmatched `{` / `}` | ✅ | Depth-tracked |
| `[(` / `])` | Unmatched `(` / `)` | ✅ | Depth-tracked |
| `[*` / `]*` | Comment start/end | ❌ | |
| `[/` / `]/` | C comment start/end | ❌ | |
| `[#` / `]#` | Preprocessor directive | ❌ | |
| `[z` / `]z` | Fold start/end | ❌ | |
| `[s` / `]s` | Spelling errors | N/A | No spell check |

**Bracket commands: 12/13 (92%)** (excluding N/A)

---

## Operator-Pending Mode

Operators `d`, `c`, `y`, `>`, `<`, `=`, `g~`, `gu`, `gU` all accept these motions:

| Motion | Description | Status | Notes |
|--------|-------------|--------|-------|
| `w` / `b` / `e` / `ge` | Word motions | ✅ | |
| `W` / `B` / `E` / `gE` | WORD motions | ✅ | |
| `0` / `^` / `$` / `g_` | Line boundary | ✅ | |
| `f`/`t`/`F`/`T` + char | Find/till | ✅ | |
| `;` / `,` | Repeat find | ✅ | |
| `h` / `j` / `k` / `l` | Character/line | ✅ | |
| `{` / `}` | Paragraph | ✅ | |
| `(` / `)` | Sentence | ✅ | |
| `H` / `M` / `L` | Screen position | ✅ | |
| `gg` / `G` | File position | ✅ | |
| `%` | Matching bracket | ✅ | |
| `iw`/`aw`/`iW`/`aW` | Word text objects | ✅ | |
| `i"`/`a"`/`i'`/`a'` | Quote text objects | ✅ | |
| `i(`/`a(`/`i{`/`a{`/`i[`/`a[` | Bracket text objects | ✅ | |
| `ip`/`ap`/`is`/`as` | Paragraph/sentence | ✅ | |
| `it`/`at` | Tag text objects | ✅ | |
| `i<`/`a<` | Angle bracket objects | ✅ | |
| `` i` ``/`` a` `` | Backtick text objects | ✅ | |
| `o_v` | Force charwise | ❌ | |
| `o_V` | Force linewise | ❌ | |
| `o_CTRL-V` | Force blockwise | ❌ | |

**Operator-pending: 18/21 (86%)**

---

## Visual Mode

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `v` | Enter/toggle Visual | ✅ | |
| `V` | Enter/toggle Visual Line | ✅ | |
| `CTRL-V` | Enter/toggle Visual Block | ✅ | |
| `o` | Swap to other end | ✅ | |
| `O` | Swap to other corner (block) | ✅ | |
| `gv` | Reselect last visual | ✅ | |
| `d` / `x` | Delete selection | ✅ | |
| `c` / `s` | Change selection | ✅ | |
| `y` | Yank selection | ✅ | |
| `>` / `<` | Indent/dedent selection | ✅ | |
| `~` | Toggle case | ✅ | |
| `u` | Lowercase selection | ✅ | |
| `U` | Uppercase selection | ✅ | |
| `=` | Auto-indent selection | ✅ | |
| `p` / `P` | Paste over selection | ✅ | |
| `:` | Enter command with range | ✅ | `'<,'>` pre-filled |
| `J` | Join selected lines | ✅ | |
| `gJ` | Join without space | ✅ | |
| `%` | Jump to matching bracket | ✅ | Extends selection |
| `r{char}` | Replace all selected chars | ✅ | Visual/VisualLine/VisualBlock |
| `I` (block) | Block insert | ❌ | |
| `A` (block) | Block append | ❌ | |
| `gq` | Format selection | ✅ | |
| `g CTRL-A` | Sequential increment | ✅ | |
| `g CTRL-X` | Sequential decrement | ✅ | |
| Movement keys | Extend selection | ✅ | All motions work |

**Visual mode: 24/26 (92%)**

---

## Ex Commands

### Core Vim Ex Commands

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `:w` / `:write` | Save | ✅ | |
| `:q` / `:quit` | Quit | ✅ | |
| `:q!` | Force quit | ✅ | |
| `:wq` / `:x` | Save and quit | ✅ | |
| `:qa` / `:qa!` | Quit all | ✅ | |
| `:wa` | Write all | ✅ | |
| `:wqa` / `:xa` | Write all and quit | ✅ | |
| `:e {file}` / `:edit` | Open file | ✅ | |
| `:enew` | New empty buffer | ✅ | |
| `:bn` / `:bp` | Buffer next/prev | ✅ | |
| `:b#` | Alternate buffer | ✅ | |
| `:b {N}` | Go to buffer N | ⚠️ | By number only, not by name |
| `:bd` / `:bdelete` | Delete buffer | ✅ | |
| `:ls` / `:buffers` | List buffers | ✅ | |
| `:split` / `:sp` | Horizontal split | ✅ | |
| `:vsplit` / `:vs` | Vertical split | ✅ | |
| `:close` | Close window | ✅ | |
| `:only` | Close other windows | ✅ | |
| `:new` | New buffer in h-split | ✅ | |
| `:vnew` | New buffer in v-split | ✅ | |
| `:tabnew` / `:tabe` | New tab | ✅ | |
| `:tabclose` | Close tab | ✅ | |
| `:tabnext` / `:tabprevious` | Next/prev tab | ✅ | |
| `:tabmove` | Move tab | ✅ | |
| `:s/pat/rep/[flags]` | Substitute | ✅ | `g`, `i` flags |
| `:%s/pat/rep/` | Substitute all lines | ✅ | |
| `:g/pat/cmd` | Global command | ✅ | |
| `:v/pat/cmd` | Inverse global | ✅ | |
| `:d` / `:delete` | Delete lines | ✅ | |
| `:m` / `:move` | Move lines | ✅ | |
| `:t` / `:co` / `:copy` | Copy lines | ✅ | |
| `:j` / `:join` | Join lines | ✅ | |
| `:y` / `:yank` | Yank lines | ✅ | |
| `:pu` / `:put` | Put register | ✅ | |
| `:sort` | Sort lines | ✅ | `n`/`r`/`u`/`i` flags |
| `:norm` / `:normal` | Execute normal keys | ✅ | Range support, `!` variant |
| `:noh` / `:nohlsearch` | Clear highlight | ✅ | |
| `:set {option}` | Set option | ✅ | Full `:set` syntax |
| `:r {file}` / `:read` | Read file into buffer | ✅ | |
| `:!{cmd}` | Execute shell command | ✅ | |
| `:reg` / `:registers` | Display registers | ✅ | |
| `:marks` | Display marks | ✅ | |
| `:jumps` | Display jump list | ✅ | |
| `:changes` | Display change list | ✅ | |
| `:history` | Display command history | ✅ | |
| `:echo {text}` | Display message | ✅ | |
| `:pwd` | Print directory | ✅ | |
| `:file` | Show file info | ✅ | |
| `:>` / `:<` | Indent/dedent | ✅ | |
| `:=` | Display line number | ✅ | |
| `:#` / `:number` / `:print` | Print line | ✅ | |
| `:ma` / `:mark` | Set mark | ✅ | |
| `:retab` | Convert tabs/spaces | ✅ | |
| `:saveas {file}` | Save as | ✅ | |
| `:update` | Save if modified | ✅ | |
| `:cquit` | Quit with error code | ✅ | |
| `:version` | Show version | ✅ | |
| `:help` / `:h` | Show help | ✅ | |
| `:windo {cmd}` | Execute in all windows | ✅ | |
| `:bufdo {cmd}` | Execute in all buffers | ✅ | |
| `:tabdo {cmd}` | Execute in all tabs | ✅ | |
| `:diffsplit` / `:diffthis` / `:diffoff` | Diff commands | ✅ | |
| `:grep` / `:vimgrep` | Project search | ✅ | Quickfix integration |
| `:copen` / `:cclose` | Quickfix open/close | ✅ | |
| `:cn` / `:cp` / `:cc` | Quickfix navigation | ✅ | |
| `:cd {path}` | Change directory | ✅ | |
| `:colorscheme` | Change theme | ✅ | 4 built-in themes |
| `:map` / `:nmap` / `:imap` | Key mappings | ❌ | Lua `vimcode.keymap()` instead |
| `:make` | Run build | ✅ | Delegates to `!make` |
| `:b {name}` | Buffer by name | ✅ | Partial name match |
| `:ab` / `:abbreviate` | Abbreviations | N/A | No abbreviation support |
| `:let` / `:if` / `:while` / `:function` | VimScript | N/A | Lua plugins instead |
| `:autocmd` / `:au` | Auto commands | N/A | Lua `vimcode.on()` instead |
| `:source` | Source vim file | N/A | Lua plugins instead |
| `:execute` / `:call` | VimScript exec | N/A | Lua plugins instead |
| `:syntax` / `:highlight` | Syntax commands | N/A | Tree-sitter + LSP semantic tokens |
| `:scriptnames` | List scripts | N/A | |
| `:mkexrc` / `:mkvimrc` | Save config | N/A | `settings.json` instead |

**Ex commands: 67/68 (99%)** (excluding N/A)

### VimCode-Specific Ex Commands

These are not in Vim but are part of VimCode:

| Command | Description |
|---------|-------------|
| `:Gdiff` / `:Gd` | Git diff split |
| `:Gstatus` / `:Gs` | Git status |
| `:Gadd` / `:Ga` | Git add |
| `:Gcommit` / `:Gc` | Git commit |
| `:Gpush` / `:Gp` | Git push |
| `:Gpull` / `:Gpl` | Git pull |
| `:Gfetch` / `:Gf` | Git fetch |
| `:Gblame` / `:Gb` | Git blame |
| `:Ghs` / `:Ghunk` | Stage hunk |
| `:GWorktreeAdd/Remove` | Git worktree management |
| `:LspInfo/Restart/Stop` | LSP management |
| `:Lformat` | LSP format |
| `:Rename` | LSP rename |
| `:DapInstall` / `:DapInfo` | DAP management |
| `:DapEval` / `:DapWatch` | Debug expressions |
| `:DapCondition/HitCondition/LogMessage` | Conditional breakpoints |
| `:ExtInstall/Remove/Enable/Disable` | Extension management |
| `:Plugin list/reload/enable/disable` | Plugin management |
| `:AI` / `:AiClear` | AI chat |
| `:MarkdownPreview` | Markdown preview |
| `:OpenFolder/Workspace/Recent` | Workspace management |
| `:EditorGroup*` | Editor group management |
| `:Settings` | Open settings editor |
| `:config reload` | Reload config |
| `:terminal` | Integrated terminal |

---

## Summary

| Category | Implemented | Total | Coverage |
|----------|-------------|-------|----------|
| Insert Mode | 15 | 24 | 63% |
| Movement | 46 | 47 | 98% |
| Editing | 47 | 50 | 94% |
| Search & Marks | 25 | 26 | 96% |
| Normal — Other | 21 | 33 | 64% |
| Text Objects | 16 | 16 | 100% |
| g-Commands | 20 | 34 | 59% |
| z-Commands | 22 | 23 | 96% |
| Window (CTRL-W) | 20 | 31 | 65% |
| Bracket ([ / ]) | 12 | 13 | 92% |
| Operator-Pending | 18 | 21 | 86% |
| Visual Mode | 21 | 26 | 81% |
| Ex Commands | 65 | 68 | 96% |
| **Total** | **348** | **411** | **85%** |

N/A commands (VimScript, digraphs, spelling, etc.) are excluded from totals.

### Priority Missing Commands

**Highest impact** (commonly used):
- `CTRL-O` in insert (full implementation — currently switches to Normal without auto-return)
- `gq{motion}` (format text)
- Visual block `I`/`A` (block insert/append)
- `g&` (repeat last `:s` on all lines)
- `CTRL-W H/J/K/L` (move window to far left/bottom/top/right)
- `N%` (go to N% of file)
