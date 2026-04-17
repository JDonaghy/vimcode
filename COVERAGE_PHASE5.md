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
| `g$` | ❌ | Rightmost char on screen line (wrap-aware) |
| `g0` | ❌ | Leftmost char on screen line (wrap-aware) |
| `g^` | ❌ | Leftmost non-blank on screen line |
| `g<End>` | ❌ | Like `g$` but non-blank |
| `g<Home>` | ❌ | Alias for `g0` |

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
| `gF` | ❌ | Edit file + jump to line number ([#120](https://github.com/JDonaghy/vimcode/issues/120)) |
| `g@` | ❌ | Call `operatorfunc` ([#121](https://github.com/JDonaghy/vimcode/issues/121)) |
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
- **`g$` / `g0` / `g^` / `g<End>` / `g<Home>`** — screen-line motions (wrap-aware horizontal) — [#123](https://github.com/JDonaghy/vimcode/issues/123)
- **`gF`** — edit-file-and-jump-to-line — [#120](https://github.com/JDonaghy/vimcode/issues/120)
- **`g@`** — user-definable operator (via `operatorfunc`) — [#121](https://github.com/JDonaghy/vimcode/issues/121)
- ~~**`g<Tab>`** — jump to last-accessed tab — [#122](https://github.com/JDonaghy/vimcode/issues/122)~~ ✅ implemented
- **Tag navigation cluster** (`g]`, `g CTRL-]`) — covered by a broader tag-support story (separate issue if we commit to tag support)

**Not worth fixing** (⏭️):
- `gQ` (Ex mode), `gH`/`gV`/`g CTRL-H` (Select mode), `g CTRL-A` (debug), `g<LeftMouse>`/`g<RightMouse>` (mouse tag nav), `g CTRL-G` (status bar already covers this)

---

## Coverage summary so far

| Slice | Commands | ✅ | 🟡 | ❌ | ⏭️ |
|-------|---------:|---:|---:|---:|---:|
| `g`-prefix | 58 | 36 | 2 | 14 | 6 |

**Vim conformance on the `g` cluster: ~62% implemented, ~24% missing, ~14% skipped-by-design.**
