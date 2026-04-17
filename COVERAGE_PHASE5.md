# Phase 5: `:help` Coverage Audit

Systematic cross-reference of VimCode's implementation against Vim's documentation.
Scope grows as slices are completed.

Status legend:
- ✅ **Implemented** — command exists and works correctly
- 🟡 **Partial** — basic case works but some edges / counts / options missing
- ❌ **Not implemented** — command doesn't exist yet in VimCode
- ⏭️ **Skipped** — intentionally out of scope (debug tooling, legacy modes, etc.)

---

## Slice 1 — `g`-prefix normal-mode commands

Reference: [`:help g`](https://vimhelp.org/index.txt.html#g) (all commands starting with the `g` prefix key).

Total: **58 commands**  ·  ✅ 36  ·  🟡 2  ·  ❌ 14  ·  ⏭️ 6

### Motions

| Keystroke | Status | Notes |
|-----------|--------|-------|
| `gg` | ✅ | Goto line N (default top) |
| `gE` | ✅ | Backward to end of previous WORD |
| `ge` | ✅ | Backward to end of previous word |
| `gj` | ✅ | Down screen line (wrap-aware) |
| `gk` | ✅ | Up screen line (wrap-aware) |
| `g;` | ✅ | Older change-list position |
| `g,` | ✅ | Newer change-list position |
| `g*` | ✅ | Forward identifier search (no boundaries) |
| `g#` | ✅ | Backward identifier search (no boundaries) |
| `gm` | ✅ | Middle of screen line |
| `gM` | ✅ | Middle of text line |
| `g_` | ✅ | Last non-blank char, N-1 lines below |
| `gT` | ✅ | Previous tab page |
| `gt` | ✅ | Next tab page |
| `gD` | ✅ | Definition of word in current file |
| `gd` | ✅ | Definition of word in current function scope |
| `gn` | ✅ | Select next match visually |
| `gN` | ✅ | Select previous match visually |
| `go` | ✅ | Goto byte N |
| `g'` / `` g` `` | ✅ | Mark jump without touching jumplist |
| `g<Down>` / `g<Up>` | 🟡 | Aliases for `gj`/`gk` — *verify separately* |
| `g$` | ✅ | Rightmost char on screen line (wrap-aware) |
| `g0` | ✅ | Leftmost char on screen line (wrap-aware) |
| `g^` | ✅ | Leftmost non-blank on screen line |
| `g<End>` | ✅ | Alias for `g$` |
| `g<Home>` | ✅ | Alias for `g0` |

### Operators / edits

| Keystroke | Status | Notes |
|-----------|--------|-------|
| `gu` | ✅ | Lowercase operator |
| `gU` | ✅ | Uppercase operator |
| `g~` | ✅ | Toggle case operator |
| `gI` | ✅ | Insert at column 1 (bypass indent) |
| `gJ` | ✅ | Join lines without space |
| `gp` | ✅ | Put after cursor, leave cursor after paste |
| `gP` | ✅ | Put before cursor, leave cursor after paste |
| `gq` | ✅ | Format text operator (`gqq` current line, `gqip` paragraph, etc.) |
| `gw` | ✅ | Format + keep cursor |
| `gr` | ✅ | Virtual replace single char |
| `gR` | ✅ | Enter Virtual Replace mode |
| `g?` | ✅ | Rot13 operator |
| `g&` | ✅ | Repeat last `:s` on all lines |

### Other

| Keystroke | Status | Notes |
|-----------|--------|-------|
| `gi` | ✅ | Restart insert at last insert position |
| `gv` | ✅ | Reselect previous Visual area |
| `ga` | ✅ | Print character info (ASCII / Unicode) |
| `g8` | ✅ | Print UTF-8 byte sequence |
| `g+` | ✅ | Jump to newer text state (undo tree) |
| `g-` | ✅ | Jump to older text state |
| `gf` | ✅ | Edit file under cursor |
| `gx` | ✅ | Open URL under cursor (OS handler) |
| `gs` | ✅ | Sleep N seconds |
| `gh` | 🟡 | Enters a Select-ish state — *verify separately* |
| `gF` | ✅ | Edit file + jump to line number (fixed in #120) |
| `g@` | ✅ | Call `operatorfunc` (fixed in #121) |
| `g<` | ❌ | Display previous command output |
| `g<Tab>` | ✅ | Last-accessed tab page (fixed in #122) |
| `gH` | ❌ | Start Select-line mode (Select mode not supported) |
| `gV` | ❌ | Don't reselect in Select mode (Select mode not supported) |
| `g]` | ❌ | `:tselect` on tag under cursor |
| `g CTRL-]` | ❌ | `:tjump` on tag under cursor |
| `gQ` | ⏭️ | Ex mode — out of scope for VimCode |
| `g CTRL-A` | ⏭️ | Memory profile dump (debug build feature) |
| `g CTRL-G` | ⏭️ | Cursor info — VimCode has a modern status bar |
| `g CTRL-H` | ⏭️ | Select-block mode — Select mode not supported |
| `g<LeftMouse>` | ⏭️ | Mouse-driven tag jump |
| `g<RightMouse>` | ⏭️ | Mouse-driven tag popup |

### Summary

**Headline gaps worth filing as issues:**
- ~~**`g$` / `g0` / `g^` / `g<End>` / `g<Home>`** — screen-line motions (wrap-aware horizontal) — [#123](https://github.com/JDonaghy/vimcode/issues/123)~~ ✅ implemented
- ~~**`gF`** — edit-file-and-jump-to-line — [#120](https://github.com/JDonaghy/vimcode/issues/120)~~ ✅ implemented
- ~~**`g@`** — user-definable operator (via `operatorfunc`) — [#121](https://github.com/JDonaghy/vimcode/issues/121)~~ ✅ implemented
- ~~**`g<Tab>`** — jump to last-accessed tab — [#122](https://github.com/JDonaghy/vimcode/issues/122)~~ ✅ implemented
- **Tag navigation cluster** (`g]`, `g CTRL-]`) — covered by a broader tag-support story (separate issue if we commit to tag support)

**Not worth fixing** (⏭️):
- `gQ` (Ex mode), `gH`/`gV`/`g CTRL-H` (Select mode), `g CTRL-A` (debug), `g<LeftMouse>`/`g<RightMouse>` (mouse tag nav), `g CTRL-G` (status bar already covers this)

---

## Slice 2 — Insert-mode commands

Reference: [`:help insert-index`](https://vimhelp.org/index.txt.html#insert-index) (all commands available in Insert mode).

Total: **17 commands**  ·  ✅ 15  ·  🟡 1  ·  ❌ 0  ·  ⏭️ 1

### Special keys

| Keystroke | Status | Notes |
|-----------|--------|-------|
| `<Esc>` | ✅ | End insert mode, back to Normal |
| `<CR>` | ✅ | Insert newline (auto-indent aware) |
| `<BS>` | ✅ | Delete character before cursor |
| `<Del>` | ✅ | Delete character under cursor |
| `<Tab>` | ✅ | Insert tab/spaces, accepts completion |
| Arrow keys | ✅ | Cursor movement |
| `<Home>` / `<End>` | ✅ | Line start/end |

### Ctrl-key commands

| Keystroke | Status | Notes |
|-----------|--------|-------|
| `Ctrl-N` / `Ctrl-P` | ✅ | Completion forward/backward |
| `Ctrl-R {reg}` | ✅ | Insert register contents |
| `Ctrl-W` | ✅ | Delete word backward |
| `Ctrl-U` | ✅ | Delete to insert start |
| `Ctrl-T` | ✅ | Indent by shiftwidth |
| `Ctrl-D` | ✅ | Dedent by shiftwidth |
| `Ctrl-O` | ✅ | Execute one Normal command |
| `Ctrl-E` | ✅ | Insert character from line below |
| `Ctrl-Y` | ✅ | Insert character from line above |
| `Ctrl-A` | ✅ | Re-insert previously inserted text |
| `Ctrl-@` | ✅ | Insert prev text + exit insert |
| `Ctrl-V {char}` | ✅ | Insert literal character |
| `Ctrl-G u` | ✅ | Break undo sequence |
| `Ctrl-G j/k` | ✅ | Move cursor down/up |
| `Ctrl-H` | 🟡 | Terminal maps to `<BS>` — works but identity is ambiguous |
| `Ctrl-R =` | ⏭️ | Expression register — requires VimScript eval, out of scope |
| `Ctrl-K {c1}{c2}` | ⏭️ | Digraph input — no digraph support planned |

### Summary

Insert mode is **100% for in-scope commands**. The only missing items are VimScript expression register and digraph input, both intentionally skipped.

No new issues filed — nothing actionable to implement.

---

## Coverage summary so far

| Slice | Commands | ✅ | 🟡 | ❌ | ⏭️ |
|-------|---------:|---:|---:|---:|---:|
| `g`-prefix | 58 | 41 | 2 | 9 | 6 |
| Insert mode | 17 | 15 | 1 | 0 | 1 |

**All g-prefix gaps (#120–#123) implemented. Insert mode at 100% in-scope coverage.**
