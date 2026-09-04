# Vim-compatibility coverage analysis — vimcode `develop` @ `ee26268`

Goal under test: *a hard-core Vim user should be able to use the TUI version of vimcode and barely notice it is not Vim.*

Method: (1) four parallel inventories of every Vim-facing test file plus a cross-check of every ✅ in `VIM_COMPATIBILITY.md` against the test corpus; (2) an extended copy of `tests/nvim_conformance.rs` (Neovim 0.11.5 as oracle, no hand-authored expectations) grown from 31 to **1,432 cases** and run against the engine; (3) synthesis against the goal. Everything below is from this session's runs; nothing was committed.

---

## 1. Verdict

**Coverage is not comprehensive enough for the stated goal, and the gap is larger than "thin coverage" — the oracle run shows the engine itself diverges from Vim on a large fraction of daily-use behaviour that no existing test catches.** Of 1,432 Neovim-oracle probes, **808 pass and 624 fail (43.6%)**, after three harness artifacts were identified and removed (fixture undo history, mapping-vs-typed key semantics, viewport scroll sync — see §2.1). The failures are not exotic: `x` on an empty line joins lines; `w`/`b`/`ge` skip empty lines; `j`/`k` over a short line forgets the column; `cw` on whitespace or punctuation deletes to end-of-line; `dj` on the last line deletes it; `.` after `A`/`I`/`o`/`O`/`cc`/`C`/`s`/`p`/`>>` repeats the wrong thing; `"_dd` clobbers the unnamed register; `/` and `:s` have **no regex engine at all** (`str::find` / `str::replace`), so `/^foo`, `/\<foo\>`, `:%s/\s\+$//`, `:s/\(a\)\(b\)/\2\1/` are all silently wrong; `:s` ignores every range except `%`; `:t$`/`:m$` splice text onto the last line; `<C-f>` opens find/replace; `<C-h>`/`<C-j>`/`<C-c>` in insert mode type `h`/`j`/`c` in a real terminal; `0099<C-x>` produces `1777777777777777777777`. The existing 1,297 integration tests and ~1,620 engine unit tests did not catch any of these because (a) only 31 assertions in the repo are oracle-backed, (b) the `VIM_COMPATIBILITY.md` checklist marks 99% ✅ while 14 rows have zero test evidence, three ✅ features are unimplemented, and (c) a measurable share of existing tests are tautological or permissive (they accept the wrong answer — e.g. `tests/operator_motions.rs:1047 test_dj_at_last_line_noop_or_delete_last` tolerates exactly the `dj` bug found here). The single cheapest lever is the one already in the repo: grow `tests/nvim_conformance.rs` and make it gate CI.

---

## 2. What was done, and what to trust

### 2.1 Harness changes made in the analysis worktree (not merged; needed for any of §3 to be reproducible)

All in `tests/nvim_conformance.rs`:

| Change | Why | Effect if omitted |
|---|---|---|
| `vim.o.undolevels = -1` before `nvim_buf_set_lines`, restored to `1000` after | `set_lines` is itself an undo step, so `u` in the oracle undid the fixture (buffer became `""`) | 41 spurious "undo" failures |
| `feedkeys(..., "ntx")` instead of `"nx"` (`t` = handle as typed) | With `"nx"` keys count as mapping-sourced: `q` recording captures nothing (every `@a` was a no-op in nvim) and undo is not synced between commands (`xxxu` undid all three) | Every macro probe and most undo probes invalid |
| Capture `nvim_win_get_height(0)` in the result JSON and `engine.set_viewport_lines(rows)` | `H`/`M`/`L`/`<C-d>`/`zt` need identical window heights | Screen-relative motions incomparable |
| `engine.ensure_cursor_visible()` after placing the start cursor | `nvim_win_set_cursor` scrolls the window; the engine's raw cursor write does not | 12 spurious scroll failures (`H` from line 30 returned line 1) |
| Pump `macro_playback_queue` after every key (`advance_macro_playback` loop, capped) | The UI normally pumps it; the harness must too | `@a` never executes on the vimcode side |
| Per-case `setup` Lua field (`cs(...)` constructor) | Lets a probe pin a Vim-vs-Neovim default (`startofline`, `joinspaces`, `nrformats`, `smarttab`) | Cannot distinguish "vimcode differs from Vim" from "Neovim differs from Vim" |
| `PROBE_FILTER=<label-substring>` env var; failure lines tagged `BUF` / `CUR` / `BUF+CUR` | Iteration speed; triage | — |
| Multiple `const` arrays (`CASES_OP`, `CASES_DOT`, …) flattened by the runner | 1,400 cases in one literal is unworkable | — |

Run with: `cargo test --no-default-features --test nvim_conformance -- --nocapture` (≈3 min for 1,432 cases). Raw outputs: `scratchpad/probe_run3.txt` (authoritative), `probe_run1.txt`/`probe_run2.txt` (pre-fix, for the artifact history).

### 2.2 Known limits of the oracle comparison (do **not** file these as deviations)

- **Neovim ≠ Vim defaults.** Neovim ships `nostartofline`, `nojoinspaces`, `nrformats=bin,hex`, `smarttab`, `autoindent`; Vim ships `startofline`, `joinspaces`, `nrformats=bin,octal,hex`, `nosmarttab`. Probes that depend on these were run with an explicit `setup` (labels containing `sol`, `joinspaces`, `nf=octal`, `nosmarttab`). Where vimcode matches Neovim but not Vim, it is a **policy decision** the maintainer must make (§6.2); this report treats Neovim as the oracle as instructed.
- **Column is byte-based in nvim, char-based in vimcode.** All probes are ASCII except `ins:C-v u00e9` (which fails for a real reason — `<C-v>u` is unimplemented — but any future multibyte case needs a byte→char conversion in the harness).
- **Trailing newline.** ropey has none; nvim's last line is `""`. The harness trims both and tolerates a one-line cursor difference at EOF (unchanged from develop).
- **`U` (line-undo)** in nvim still sees the fixture write as "the change on this line", so `undo:U`/`undo:UU` results are contaminated — listed as *unverified*.
- **Three cases skipped by design** (labels ending `skip`) are placeholders and are neither passes nor failures of interest.
- Probes were **not** re-verified one-by-one in an interactive nvim; each *is* an nvim run, and every headline item in §3 was additionally sanity-checked against documented Vim behaviour (`:h`). Items where I have residual doubt are marked *medium* or *unverified*.

### 2.3 Result totals

| Category (label prefix) | Probes | Fail | Fail % | Category (label prefix) | Probes | Fail | Fail % |
|---|---|---|---|---|---|---|---|
| `op` operators × motions | 217 | 71 | 33% | `sub` `:s` | 66 | 56 | **85%** |
| `word` motions | 154 | 23 | 15% | `g` `:g`/`:v` | 20 | 19 | **95%** |
| `ex` other ex commands | 113 | 66 | **58%** | `search` `/ ? * n N` | 78 | 50 | **64%** |
| `to` text objects | 109 | 46 | 42% | `scroll` | 74 | 28 | 38% |
| `misc` | 99 | 36 | 36% | `dot` `.` repeat | 53 | 38 | **72%** |
| `vis` visual | 98 | 42 | 43% | `vb` visual block | 52 | 31 | 60% |
| `ins` insert-mode keys | 83 | 40 | 48% | `num` `<C-a>`/`<C-x>` | 52 | 24 | 46% |
| `reg` registers | 44 | 16 | 36% | `undo` | 42 | 11 | 26% |
| `mark` | 30 | 11 | 37% | `jump` | 25 | 9 | 36% |
| `mac` macros | 23 | 7 | 30% | **Total** | **1,432** | **624** | **43.6%** |

---

## 3. Confirmed deviations (headline set)

Impact = how likely a hard-core Vim user is to hit it in a normal editing session: **H** daily, **M** weekly, **L** occasional. Confidence: **high** = nvim result matches documented Vim behaviour and the vimcode result is clearly wrong; **medium** = real but minor or partly a defaults question; **unverified** = harness caveat applies. The complete machine-generated table of all 621 parsed failures is in **Appendix A**; labels there match the `[label]` column here.

### 3.1 Search and substitute — no regex engine (H, high)

`Engine::run_search` (`src/core/engine/execute.rs:3015`) is `str::find` on the literal query; `execute_substitute_command` (`execute.rs:2941`) splits on every `/` and calls `str::replace`. Consequently every metacharacter is literal.

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `search:/^foo` | `/^foo<CR>` | `["a foo","foo b"]` | (2,1) | (1,1) no match |
| `search:/foo$` | `/foo$<CR>` | `["foo a","a foo"]` | (2,3) | (1,1) |
| `search:/\<foo\>` | `/\<foo\><CR>` | `["foobar foo"]` | (1,8) | (1,1) |
| `search:/\d\+`, `/[bc]a`, `/a\{2}`, `/\v`, `/foo\|bar`, `/a\.c`, `/ab*c`, `/\s`, `/\(foo\)\1`, `/\zs`, `/\ze`, `/\c`, `/\C`, `/o\nb` | — | — | match | no match / wrong |
| `search:/pat/e` (+`e+1`,`e-1`,`b+2`,`s-1`,`+1`,`-1`,`;`) | `/bar/e<CR>` | `["foo bar"]` | (1,7) | (1,1) — offsets unsupported |
| `search://` / `/<CR>` | repeat last pattern | `["foo x foo x foo"]` | (1,13) | (1,1) / (1,7) |
| `search:3/pat` | count | `["a foo foo foo"]` | (1,11) | (1,3) |
| `search:* cursor not on word` | `*` | `["  foo bar foo"]`@(1,1) | (1,11) | no move — `*` on whitespace should use next keyword |
| `search:* then :s//` / `sub:empty pattern last search` | `*:%s//X/g` | `["foo bar foo"]` | `X bar X` | unchanged — empty pattern ≠ last search |
| `search:scs lower` | `:set ic scs<CR>/foo` | `["x FOO Foo foo"]` | (1,3) | (1,11) — smartcase inverted for lowercase query |
| `search:* with ic scs` | `*` | `["Foo foo Foo"]` | (1,5) | (1,9) — `*` must ignore smartcase |
| `search:gd` | `gd` | `["int x = 1;","y = x;"]`@(2,5) | (1,5) | no move |
| `search:gn selects`, `cgn .`, `dgn`, `gN` | `cgnX<Esc>..` | `["foo bar foo baz foo"]` | `X bar X baz X` | `XXX bar foo baz foo` — `gn` targets wrong match |
| `sub:2,3`, `.,+1`, `.,$`, `'a,'b`, `'<,'>` explicit | `:2,3s/a/b/` | `["a","a","a","a"]` | lines 2–3 changed | **nothing changes** — only `%` and bare ranges work |
| `sub:backrefs`, `\v groups`, `\u\1 swap words` | `:s/\(a\)\(b\)/\2\1/` | `["ab"]` | `ba` | unchanged |
| `sub:& in replacement`, `\0`, `\U&`, `\u&`, `\L`, `\U..\E`, `~`, `\&`, `\~` | `:s/foo/[&]/` | `["foo"]` | `[foo]` | `[&]` literal |
| `sub:\r newline`, `\t` | `:s/,/\r/` | `["a,b"]` | `a` / `b` | `a\rb` literal backslash-r |
| `sub:alternation`, `\zs`, `\ze`, `\{2}`, `\{-}`, `\w\+`, `\< \>`, `[] class`, `.*`, `literal dot`, `\n multiline`, `^ anchor`, `$ anchor`, `\s\+$`, `x* g on empty` | `:%s/\s\+$//e` | `["a  ","b "]` | trailing ws stripped | unchanged |
| `sub:# delimiter`, `\/ escaped slash`, `bar chain` | `:s#/#-#` | `["a/b"]` | `a-b` | unchanged |
| `sub:n flag` | `:s/a/b/gn` | `["a a"]` | unchanged (count only) | `b b` — `n` ignored |
| `sub:& flag`, `:&&`, `:&`, `misc:& after &&`, `misc:g& after range` | `:s/a/b/g` `j` `:&&` | `["a a","a a"]` | `b b`/`b b` | second line untouched / flags wrong |
| `sub:I flag with ic`, `ic applies` | `:set ic<CR>:s/a/b/g` | `["A a"]` | `b b` | `A b` — `ignorecase` not honoured by `:s` |
| `sub:count`, `range + count` | `:s/a/b/ 2` | 4×`a` | lines 1–2 | line 1 only |
| `sub:%` / `%g cursor` / `cursor after` / `%s cursor at end` / `ex:s cursor col` | `:%s/a/b/` | 3×`a`@(1,1) | cursor (3,1) | (1,1) — `:s` never moves the cursor |
| `sub:no replacement` | `:s/b<CR>` | `["abc"]` | `ac` | unchanged |
| `ex:s sets last search for n` | `:s/a/x/<CR>n` | `["a","b","a"]` | (3,1) | (1,1) |

### 3.2 `:g`, `:v` and the ex address parser (H for `:g`, M for the rest; high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `g:!` | `:g!/a/d` | `["a","b","a","c"]` | `a`/`a` | unchanged (`:v` works, `:g!` does not) |
| `g:range` | `:2,3g/a/s/a/b/` | 3×`a` | `a`/`b`/`b` | unchanged |
| `g:m0 reverse` | `:g/^/m0` | `["1","2","3"]` | `3`/`2`/`1` | unchanged — classic idiom broken |
| `g:^$ d` | `:g/^$/d` | `["a","","b","",""]` | `a`/`b` | `a`/`""`/`b` — `^$` literal |
| `g:t$`, `+1d`, `j`, `.,+1j`, `d with count`, `copy to end`, `normal with count`, `s// reuse pattern`, `delimiter` | `:g/a/t$` | `["a","b"]` | `a`/`b`/`a` | `a`/`ba` |
| `g:d`, `g:s`, `g:normal`, `g:cursor after` | `:g/a/d` | `["a","b","a","c"]` | cursor on last affected line (2,1) | cursor unchanged (1,1) |
| `ex:t$`, `:m$`, `:1,2t$`, `:1co$`, `:2,3m$`, `:1,2t.` | `:t$` | `["a","b"]` | `a`/`b`/`a` | **`a`/`ba`** — copy/move to `$` is spliced into the last line's text |
| `ex:m-2` | `:m-2` | `["a","b","c"]`@3 | `a`/`c`/`b` | `c`/`a`/`b` |
| `ex:/foo/`, `?a?`, `/foo/+1`, `/foo/d`, `/a/,/b/d`, `.,/foo/d` | `:/foo/d` | `["a","foo","b"]` | `a`/`b` | unchanged — pattern addresses unsupported |
| `ex:2;+1d`, `2,+1d`, `$-1d`, `.,.+1d`, `.+2` | `:$-1d` | `["a","b","c"]` | `a`/`c` | unchanged — offsets unsupported |
| `ex:'<,'>d after v`, `*d after visual` | `vj<Esc>:'<,'>d` | `["a","b","c"]` | `c` | unchanged |
| `ex:d a then "ap`, `d 2`, `d _`, `y A append` | `:2d a<CR>"ap` | `["a","b","c"]` | `a`/`c`/`b` | register/count args ignored |
| `ex:pu!`, `2put`, `0put` | `yy:pu!` | `["a","b"]` | `a`/`a`/`b` | unchanged |
| `ex:1,3j`, `j!`, `j 3`, `2j 3`, `%j` cursor | `:1,3j` | `["a","b","c"]` | `a b c` | unchanged |
| `ex:>>`, `2,3>`, `> 2`, `< 2`, `>>>`, `2> 2`, cursor after `:>`/`:<` | `:>>` | `["a"]` | 8 spaces | unchanged; `:>` leaves cursor at col 1 (Vim: first non-blank) |
| `ex:2,3sort`, `sort /pat/ r` | `:2,3sort` | `["c","b","a"]` | `c`/`a`/`b` | unchanged |
| `ex:retab!`, `retab 2`, `retab` cursor | `:retab 2` on `\ta` (ts=4) | — | 4 spaces | 2 spaces |
| `ex:2ka 'a`, `2mark a` | `:2ka<CR>'a` | 3 lines | (2,1) | (1,1) |
| `ex:le`, `le 4`, `ri 10`, `ce 10` | `:ri 10` | `["a"]` | 9 spaces + `a` | unchanged |
| `ex:r !echo`, `%!sort` passes; `:r !` fails | `:r !echo hi` | `["a"]` | `a`/`hi` | unchanged |
| `ex:normal` family (cursor only) | `:normal Ax` | `["a","b"]` | (1,2) | (1,3) — cursor left past EOL after `:normal` |
| `misc:: with count`, `3:s`, `2:normal` | `3:d<CR>` | 4 lines | `d` | `b`/`c`/`d` — count before `:` should become `.,.+2` |

### 3.3 Dot repeat (H, high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `dot:A; j .` | `A;<Esc>j.` | `["a","b"]` | `a;`/`b;` | `a;`/**`;b`** — `.` of `A` inserts at cursor, not EOL |
| `dot:I .` | `Ix<Esc>j.` | | `xa`/`xb` | cursor wrong (buffer right) |
| `dot:o .`, `O .`, `ofoo<CR>bar .` | `ob<Esc>.` | `["a"]` | `a`/`b`/`b` | `a`/**`bb`** — `.` of `o` does not open a line |
| `dot:cc j .`, `C j .`, `s l .`, `ct, .`, `ciw w .`, `cw . next word`, `vec .`, `ciw then . at eol` | `ccX<Esc>j.` | `["a","b"]` | `X`/`X` | `X`/**`Xb`** — `.` of a change re-inserts text but does not delete |
| `dot:3Ax j .`, `3Ax j 2.`, `dot:cw with count 2.`, `dfx count override`, `>> 2.` (`dot:3x 2.`, `x 3.`, `2dd 3.` pass — count override works for `x`/`dd` but not for `A`/`cw`/`df`/`>>`) | `3Ax<Esc>j.` | `["a","b"]` | `axxx`/`bxxx` | `ax`/`xb` — count on `A` ignored **and** `.` misplaced |
| `dot:yyp .`, `yy3p .`, `p charwise .`, `xp .`, `"ayy "ap .`, `"1p . .` | `yyp.` | `["a"]` | 3×`a` | 2×`a` — `p` is not repeatable |
| `dot:vlld .`, `Vjd .`, `vjd . charwise`, `dap .`, `diw . .`, `df. .` | `Vjd.` | 5 lines | `e` | `c`/`d`/`e` — `.` after a visual/text-object op repeats a 1-unit op |
| `dot:Vj> j .`, `>ip .` | `Vj>j.` | 4 lines | lines 2–3 +1 level | line 2 +2 levels — wrong region |
| `dot:R .` | `Rxy<Esc>ll.` | `["abcdef"]` | `xycxyf` | `xycdef` |
| `dot:<C-v>jIx .`, `vb:d then .`, `r then .`, `c then .` | `<C-v>jIx<Esc>jj.` | 4×`ab` | all four `xab` | last line untouched |
| `dot:@:`, `misc:dot after @:` | `:d<CR>@:` | `["a","b","c"]` | `b`/`c` | `c` — `@:` runs twice |
| `dot:g&` | `:s/a/b/g<CR>g&` | | cursor (2,1) | (1,1) |
| `num:C-a .`, `3C-a .`, `3C-a 2.` | `<C-a>.` | `["x 5"]` | `x 7` | `x 6` — `<C-a>` not repeatable |
| `mac:@a then .` | `qaxq@a.` | `["abcd"]` | `cd` | `d` — `.` after `@a` repeats the macro, not the last change |

### 3.4 Motions and cursor memory (H, high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `word:w onto blank line`, `w over multiple blank lines`, `ww …`, `w over punct then blank` | `w` | `["foo","","bar"]`@(1,1) | (2,1) — empty line is a word | (3,1) |
| `word:b onto blank line`, `ge onto blank` | `b` | `["foo","","bar"]`@(3,1) | (2,1) | (1,1) / (1,3) |
| `word:jj col memory`, `$jj`, `$ then j to longer`, `dd then j col`, `ins:Down Down col memory` | `jj` | `["abcdef","ab","abcdef"]`@(1,5) | (3,5) | **(3,2)** — desired column lost crossing a short line; `$` does not stick to EOL |
| `word:2$` | `2$` | `["ab","cd","ef"]` | (2,2) | (1,2) — count on `$` ignored |
| `word:<CR>` | `<CR>` | `["  a","  b"]` | (2,3) | (1,1) — `<CR>` is not a motion |
| `word:}}}`, `}} multiple blanks`, `} from blank`, `} whitespace-only line not blank`, `( para`, `{ at start` | `}` | `["a","   ","b",""]` | (4,1) | (2,4) — whitespace-only line treated as paragraph boundary |
| `word:% in quotes` | `%` | `["\"(\" )"]`@(1,2) | (1,2) no match | (1,5) |
| `op:t; then ; repeat`, `t;;;`, `dt;;`, `t, ; ,`, `T, then ;` | `t;;` | `["foo; bar; baz"]` | (1,8) — `;` after `t` jumps past the adjacent char (cpo-;) | (1,3) stuck |
| `scroll:C-f` (and every `<C-f>` case) | `<C-f>` | 60 lines | (21,1) | (1,1) — **`<C-f>` opens find/replace by default** (`settings.ctrl_f_action`) |
| `scroll:5C-d C-d`, `3C-d sets scroll then C-u`, `3<C-d> on short` | `5<C-d><C-d>` | 60 lines | (11,1) — count sets `'scroll'` | (60,1) |
| `scroll:C-b`, `2<C-b>`, `C-b after G then H/L` | `<C-b>` | @60 | (40,1) — 2-line overlap | (38,1) |
| `scroll:so=5 30G H`, `so=5 30G L`, `so=5 C-e` | `:set so=5<CR><C-e>` | 60 lines | (7,1) | (2,1) — `scrolloff` ignored for `<C-e>`/`H`/`L` |
| `scroll:M from 30`, `G M`, `dM`, `M on short buffer`, `zzH`, `z.H` | `M` | 5 lines | (3,1) | (5,1) — off-by-one / wrong middle |
| `scroll:50% H`, `C-d then H` | `50%H` | 60 lines | (9,1) — long jumps centre the cursor | (19,1) — minimal scroll |
| `scroll:C-d col sol`, `word:gg indented (sol)`, `G indented (sol)`, `op:>> cursor sol` | `gg` | `["  a","b"]`@2 | (1,3) with `startofline` | (1,1) — see §6.2 policy |

### 3.5 Operators (H, high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `op:x on empty line` | `x` | `["","x"]`@(1,1) | unchanged | **`x`** — the newline is deleted |
| `op:cw on whitespace`, `cw on punctuation`, `misc:cw on space at eol`, `word:cw at eol punct` | `cwX<Esc>` | `["foo   bar"]`@(1,4) | `fooXbar` | **`fooX`** — deletes to EOL; at EOL joins the next line |
| `op:2dw across line end`, `3dw crossing lines` | `2dw` | `["a b","c d"]`@(1,3) | `a d` | `a `/`c d` — count stops at EOL |
| `op:dj last line`, `dk first line`, `misc:2dd on last` | `dj` | `["a","b"]`@(2,1) | unchanged (motion fails) | `a` — line deleted |
| `op:cc keeps indent`, `2cc`, `S keeps indent`, `misc:cc on last line indent` | `ccX<Esc>` | `["    foo"]` | `    X` | `X` — autoindent not applied by `cc`/`S` |
| `op:d/pat`, `d/pat/e`, `d?pat`, `dn`, `d/pat multiline`, `d/pat/+1`, `search:c/pat`, `y/pat`, `vis:v/pat d`, `vnd` | `d/baz<CR>` | `["foo bar baz"]` | `baz` | **`fz`/`oo bar baz`** — `/` is not an operator-pending motion; keys leak into insert |
| `op:d\`a`, `y\`a`, `mark:c\`a`, `vis:v\`a d` | `` magg0d`a `` | `["abc def","ghi jkl"]`@(2,4) | ` jkl` | unchanged — backtick mark as motion unsupported |
| `op:d% before paren` | `d%` | `["foo(a, b) bar"]`@(1,1) | ` bar` | unchanged — `%` off-bracket does not search forward |
| `op:Y is linewise` passes; `vis:vjY` fails | `vjYGp` | 3 lines | lines appended | `ai` — `Y`/`D`/`X`/`C`/`S`/`R` in charwise visual unimplemented |
| `op:J next blank` (`J cursor col`, `J with tab indent`, `3J`, `gJ` pass) | `J` | `["a","","b"]` | joins without adding a space | see appendix |
| `op:r<CR>`, `3r<CR>`, `misc:r Tab`, `vis:v_r CR`, `vb:jr<CR>` | `r<Tab>` | `["ab"]` | 4 spaces + `b` (et) | unchanged |
| `op:R past eol`, `R BS restores`, `2R`, `misc:R at eol then BS` | `Rxyz<BS><BS><BS><BS><Esc>` | `["ab"]`@2 | `ab` | `axyz` — `<BS>` in Replace mode does not restore |
| `op:3s beyond eol`, `5r beyond eol`, `5~ past eol`, `misc:3ix mid-line`, `2Ix`, `2ox`, `ins:i with count and Esc cursor`, `A with count and CR` | `3ix<Esc>` | `["abc"]`@(1,3) | `abxxxc` | `abxc` — insert counts ignored unless at col 1 |
| `op:>> noet ts4`, `>>>> noet ts4`, `existing tab noet`, `<< mixed tab space` | `:set ts=4 noet<CR>>>` | `["a"]` | `\ta` | see appendix — `noexpandtab` shifting uses spaces |
| `op:=G braces`, `=ip flat`, `==`, `vis:Vj=` | `=ip` | `["  a","      b","c"]` | `a`/`b`/`c` (C-indent) | unchanged — `=` is a no-op on plain text; visual `=` unimplemented |
| `op:gqq tw20`, `gqip`, `gwip cursor`, `Vgq`, `misc:gqgq`, `gwgw` | `:set tw=20<CR>gqq` | long line | wrapped at 20 | see appendix — `gq` cursor/`gqgq` alias/`gw` wrong |
| `op:!!tr`, `!Gsort` passes; `misc:g?g?` fails | `g?g?` | `["ab"]` | `no` | unchanged — doubled-operator aliases (`g?g?`,`gqgq`,`gwgw`,`gUgU`,`gugu` …) partly missing |
| `op:2d then Esc` (`misc:2d then Esc`) | `2d<Esc>x` | `["abc"]`@(1,2) | `ac` | `a` — **pending count leaks past `<Esc>`** |
| `op:dvj`, `dVw`, `dve`, `dv$`, `d<C-v>j` | forced motions | — | — | see appendix |

### 3.6 Registers (H, high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `reg:"_dd then p`, `viw"_dP`, `ex:d _` | `yyj"_ddp` | `["a","b","c"]` | `a`/`c`/`a` | `a`/`c`/**`b`** — black-hole delete overwrites `"` (the `viw"_dP` idiom pastes the wrong text) |
| `reg:". insert register`, `"/ last search`, `"= expr`, `C-r = in insert` | `ifoo<Esc>".p` | `["ab"]` | `foofooab` | `fooab` — `".`, `"/`, `"=` empty |
| `reg:"ayw "Ayw "ap`, `"1 after cc`, `"adw does not set "-`, `dw does not touch "1`, `d/ goes to "1`, `d% goes to "1`, `dn goes to "1`, `dd yy "1p unchanged` (passes), `p from "1 then u then "2p` | `"add"Add"ap` passes; `"aywx"ap` passes | — | — | append to charwise register joins with newline; `"1` not set by `cc`; `d%`/`d/`/`dn` don't go to `"1`; named-register small delete leaks into `"-` |
| `reg:3"ap`, `misc:p with count linewise cursor`, `2gp`, `P count`, `yy3p` order (`misc:2yy 3p`, `vis:Vjy then p count`) | `2yy3p` | `["a","b"]` | `a b a b a b` interleaved | `a a a a b b b b` — multi-line register pasted with count is grouped per line |
| `reg:i C-r a linewise`, `ins:C-r register linewise mid line`, `C-r with tab in register` | `"ayyjA<C-r>a<Esc>` | `["a","b"]` | cursor (3,1) | (2,2) |

### 3.7 Insert mode (H for autopairs/`<C-w>`/`<CR>`/Tab; high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `ins:( no autopair`, `" no autopair`, `{ CR no autopair`, `[ no autopair` | `i(<Esc>` | `["a"]` | `(a` | **`()a`** — `auto_pairs` defaults on |
| `ins:typing prefix then Tab` | `ofo<Tab>x` | `["foo bar"]` | `fo  x` | `foox` — Tab accepts the auto-completion popup |
| `ins:C-p completion` | `A<C-p>` | `["foo","fob","f"]` | `fob` (nearest above) | `foo` |
| `ins:C-w at line start joins`, `C-w punctuation`, `C-w over existing text` | `A<C-w><Esc>` on `foo.bar` | | `foo.` | **empty line** — `<C-w>` deletes whole line on punctuation |
| `ins:C-u before start`, `C-u twice`, `C-u with indent` | `A<C-u><Esc>` | `["ab"]` | `` | `ab` |
| `ins:CR then Esc removes autoindent`, `CR CR keeps prev line empty`, `op:o esc removes indent`, `ins:CR mid-line`, `CR on indented mid` | `A<CR><Esc>` | `["    foo"]` | `    foo`/`` | `    foo`/`    ` — **trailing autoindent whitespace left behind** |
| `ins:Tab mid line ts4`, `ts8`, `after 2 chars`, `Tab at start (smarttab)`, `insert Tab then BS` | `A<Tab>x` | `["a"]` (ts=4, et) | `a   x` (to next tabstop) | `a    x` (always `sw` spaces) |
| `ins:BS over indent (nvim smarttab)`, `BS mid indent` | `i<BS>` | `["    a"]`@5 | `a` (smarttab) | `   a` — see §6.2 (Vim default is one space) |
| `ins:0 C-d` | `A0<C-d>` | `["    a"]` | `a` | `a0` |
| `ins:C-o $ then type`, `A C-o h`, `C-o with count`, `C-o p`, `C-o :s` | `i<C-o>$x` | `["foo"]` | `foox` | `foxo` — `<C-o>` at EOL / with count / with `:` broken |
| `ins:C-v 065`, `C-v x41`, `C-v u00e9` | `i<C-v>065` | `["a"]` | `Aa` | `065a` |
| `ins:C-h as BS`, `C-j newline`, `misc:C-c in insert`, `C-[ in insert` | `a<C-h><Esc>` | `["abc"]`@3 | `ab` | **`abch`** — see §6.3 (TUI delivers these exact key events) |
| `ins:BS join with autoindent`, `A then BS past insert start` (passes) | `i<BS>×5` | `["a","    b"]`@(2,5) | `b` | `ab` |
| `undo:arrow breaks undo`, `A xyz u cursor`, `2u after insert ×3`, `u after R`, `u after <C-v>I`, `u after visual d`, `u after :%s cursor` | `ifoo<Left>bar<Esc>u` | `["ab"]` | `fooab` (arrow splits undo) | `ab` |

### 3.8 Visual and visual-block (H, high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `vis:vf,d`, `vt,d` (passes) | `vf,d` | `["a,b,c"]` | `b,c` | `,b,c` — inclusive `f` end not included |
| `vis:vjD`, `vjX`, `vjY p`, `vjC`, `vjS`, `vjR`, `v s`, `v_gJ`, `Vj=` | `vjD` | 3 lines | `ghi` | unchanged — uppercase ops, `s`, `gJ`, `=` unimplemented in visual |
| `vis:v3iwd`, `v2awd`, `V3>`, `V2<`, `vip then ip extends`, `viwd on whitespace`, `vawd at eol` (passes) | `v3iwd` | `["a b c d"]` | ` c d` | ` b c d` — counts/repeat on visual text objects |
| `vis:gv after Vjd`, `v then gv toggles`, `misc:gv after p` | `Vjdgvd` | 5 lines | `d`/`e` | `e` |
| `vis:vlp linewise reg`, `Vp charwise reg` (passes) | `yyjjvlp` | `["a","b","xyz"]` | `a`/`b`/``/`a`/`z` | `a`/`b`/`a`/`z` |
| `vis:Vjd`, `Vd cursor`, `Vr-`, `VGd`, `vjy then P`, `v$y p`, `vjd then p`, `vjy count 2p`, `vjy "0 then p`, `vjgq`, `vip on last para` | cursor after visual op | — | — | column/line wrong in 11 cases |
| `vis:vggd`, `v^d`, `v ip on blank`, `v ap trailing` | `vggd` | `["abc","def"]`@(2,2) | `af` | `f` |
| `vis:vjd . charwise`, `vec .` | see §3.3 | | | |
| `vb:jlcX`, `jc with multi chars`, `jCX`, `jD`, `jsX`, `jj$d`, `ragged d`, `I on short line skipped`, `jIx with CR`, `2I`, `jr<CR>`, `j>`, `j<` | `<C-v>jlcX<Esc>` | `["abc","def"]`@(1,2) | `aX`/`dX` | `Xa`/`d` — block `c` broken; block `>` shifts whole line (`a    bc` expected) |
| `vb:jy then Gp`, `jy then P`, `jjy p at eol`, `jjy p on shorter`, `jly then p`, `jjp block over block`, `vb yank then p` | `<C-v>jy$P` | `["ab","cd"]` | `aab`/`ccd` | `aa`/`cb`/`cd` — **blockwise register pasted as lines** |
| `vb:jjAx`, `jj$Ax`, `A on short line padded`, `A on empty middle line`, `$A on empty middle line`, `cursor after y`, `g C-a` | cursor after block op | — | (1,2) | (1,3) |
| `vb:v then C-v switch` etc. pass | | | | |

### 3.9 Text objects (M–H, high)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `to:daw on whitespace`, `diw on whitespace`, `diw punctuation`, `daw punctuation`, `daw leading whitespace`, `ciw on whitespace`, `daw on multi spaces at start`, `diw at eol on space`, `daw on only whitespace line` | `diw` | `["foo.bar"]`@(1,4) | `foobar` | `.bar` — punctuation/whitespace runs are not words |
| `to:d2aw`, `d3iw`, `c2aw`, `2daw`, `d5aw too many`, `daw at end with count`, `d3i( count 3 too many`, `d2i(`, `d2it`, `misc:3ciw`, `2d2aw` | `d2aw` | `["a b c d"]` | `c d` | `b c d` — **counts on text objects ignored** |
| `to:dap`, `d2ap`, `dip on blank lines`, `dap on blank`, `cip`, `dap cursor after`, `yap cursor`, `yip cursor`, `dap trailing no blank` | `dap` | `["a","b","","c"]` | `c` | `` — `ap` eats the following paragraph |
| `to:di( on )`, `di( before paren same line`, `di( across lines`, `di( with newline after open`, `di( empty`, `ci{ multiline`, `yi{ cursor multiline`, `di< nested`, `yi( cursor`, `ya( cursor` | `di(` | `["f(a, b)"]`@(1,7) on `)` | `f()` | unchanged |
| `to:di" on closing`, `di" before quotes`, `da" before quotes`, `yi" cursor`, `ci' then .` | `di"` | `["x \"ab\" y"]`@(1,1) | `x "" y` (searches forward) | unchanged |
| `to:das last sentence`, `dis on whitespace between`, `daw leading space only`, `>ap` | `das` | `["One two.  Three four."]`@12 | `One two.` | `One two.  ` |

### 3.10 Marks, jumps, macros, numbers (M, high unless noted)

| label | keys | start | nvim | vimcode |
|---|---|---|---|---|
| `mark:mark shifts after O`, `'a after text insert above`, `` `a after line join `` | `maggOx<Esc>'a` | 3 lines @2 | (3,1) | (2,1) — marks do not track line insert/delete |
| `mark:mark on deleted line` | `maddgg'a` | | error, stays (1,1) | (2,1) |
| `mark:'' toggles`, `` `` after '' ``, `'a then '' back`, `jump:'a C-o`, `C-o after ''` | `3G''''` | 4 lines | (3,1) | (1,1) — `''` is not a toggle; mark jumps don't push the jumplist (`keys.rs:2568` never calls `push_jump_location`) |
| `` mark:`^ ``, `'[ after >>`, `` `[ `] after :s `` (passes) | `jAx<Esc>gg\`^` | | (2,3) | (1,1) |
| `jump:n C-o`, `% C-o`, `C-o after (`, `C-o after j 20 lines`, `3<C-o>`, `g; g; g,`, `g; after 2 changes same line` | `20j<C-o>` | 60 lines | (1,1) — wait, `j` is *not* a jump | (21,1) — vimcode pushed a jump for `20j`; `n`/`%`/`(` push none |
| `mac:10@a stops at failure` | `qa0f,xjq10@a` | 4 lines, line 3 has no `,` | stops at line 3 | continues, deletes wrong chars — the `100@q` idiom is unsafe |
| `mac:recursive` | `qaqqaA!<Esc>j@aq@a` | 4 lines | stops at EOF | **runs until the iteration cap** (16,666 `!` on the last line) — unbounded in the UI |
| `mac:qA append`, `count inside`, `macro with ci(`, `q register letter uppercase Q`, `"ay then @a` | `qaxqqAjq@a` | | `b`/`d`/`ef` | `b`/`cd`/`ef` — `qA` append unsupported; `2dw` inside a macro deletes the line |
| `num:leading zeros 0099 C-x`, `C-x on 0 leading zeros 000`, `leading zeros 009`, `octal not default 007`, `binary 0b101`, `C-a on 99999999999999999999` | `<C-x>` on `0099` | | `0098` | **`1777777777777777777777`** — octal parsing + wraparound corrupts the number |
| `num:hex C-x below zero`, `hex 0xaB`, `0X0f`, `-0x1` | `<C-x>` on `0x0` | | `0xffffffffffffffff` | `0x1` |
| `num:V C-a`, `v C-a partial`, `C-v block C-a`, `V C-a only first number per line`, `V C-a skips lines without numbers`, `V C-a cursor`, `v C-a on -5 in visual` | `Vjj<C-a>` | 3×`1` | 3×`2` | unchanged — visual `<C-a>` only works as `g<C-a>` (and cursor ends on last line) |

### 3.11 Unverified / harness-tainted (do not file without a manual check)

`undo:U`, `undo:UU` (nvim's `U` sees the fixture write); `op:J after period (vim joinspaces)` and every `(sol)` / `nosmarttab` / `nf=octal` variant (policy, §6.2); `g:normal @a` in run 1 (fixed by `t` flag; passes in run 3); `ins:C-v u00e9` cursor column (byte vs char — the buffer mismatch itself is real).

---

## 4. Coverage gap analysis

### 4.1 Structural

1. **Only 31 assertions in the repo are oracle-backed** (`tests/nvim_conformance.rs`), and they cover paragraph motions, count multiplication, three text objects, `cw<Esc>`, `>`/`<`, and basic `d`/`y`/`p`. Everything in §3 was invisible to them. The 414 `test_nvim_*` unit tests (`src/core/engine/tests.rs`, "verified against Neovim 0.12.1") are hand-transcribed values — useful, but a transcription is only as good as the transcriber's scenario, and none of them exercise ranges in `:s`, regex metacharacters, `.` after `A`/`o`, `"_`, blank-line word motions, or column memory.
2. **The command-line path is essentially untested black-box.** `ex_commands.rs` (71) and `command_mode.rs` (27) call `Engine::execute_command` directly via `exec()`; `run_cmd()` (`:` → type → `<CR>`) appears twice in the whole suite. History, `<C-r>`, `<C-w>`, `<C-u>`, `<C-b>`/`<C-e>`, Esc-cancel are uncovered.
3. **Assertion quality.** The inventories flagged ~45 tests as tautological or permissive (cannot fail, or accept both right and wrong answers). Representative: `operator_motions.rs:915` (`=G` asserts buffer non-empty), `:1047` (`dj` "noop_or_delete_last"), `:675`/`:690`/`:718` (paragraph/sentence `contains`), `normal_mode.rs:284` (`"_` test passes with `"_` ignored — and §3.6 shows `"_` *is* broken), `vim_compat_batch.rs:127` (`@:` never asserted), `:295` (`C-w =` cannot fail), `vim_compat_batch2.rs:375` (`gw` cursor test with unused `_saved_col`), `:501` (`gx` no assertions), `vim_compat_batch3.rs:208` (`C-w x` result discarded), `vim_compat_batch4.rs:149`/`:157` (`]/`,`[/` pass unimplemented), `vim_features.rs:84`/`:511`/`:542`, `wincmd.rs` ×7 "command recognized" checks, `new_vim_features.rs` ×6 message-header checks. Full list with line numbers in the inventory appendix (B).
4. **STATE-only assertions where buffer/cursor was the point**: ~40 locations (Appendix B). Notably `gp`/`gP` never assert both buffer and cursor, `g'`/`` g` `` never check the jumplist (their only distinguishing feature), `:put` never asserts cursor, `gi` never asserts the column.
5. **Zero coverage in `tests/`** (integration): dot-repeat of anything but `dd`/`cw`/`ce`; macros beyond `qa dd q @a`; visual block `d`/`y`/`p`/`c`/`r`/`$`; `n`/`N` after `?`; search offsets; `\v`; `g*`/`g#` cursor; `:s` flags `c n e &`, `:&&`, `~`, backrefs, case modifiers; `:g` with anything but `d`/`s`; `$` as an ex address; `:d`/`:r`/`:x`/`:ls`; command-line editing keys; `gq`/`gw` cursor; `=`; `<C-v>` numeric codes; `'A`/`` `A `` cross-buffer; mark adjustment; `<C-o>`/`<C-i>` beyond one hop; `2d3w`-style double counts; counts on text objects; multibyte; tabs.
6. **The 15 `test_matrix_*` tables in `engine/tests.rs` are the right shape** (operator × motion), but their expected values are hand-written; the probe shows the matrix has holes exactly where the hand-written value was wrong (whitespace `cw`, `2dw` across EOL, `dj` at last line).

### 4.2 By category (untested → weakly tested → adequately tested)

| Area | State of existing tests | Oracle result |
|---|---|---|
| Search regex, offsets, `//` | untested (search.rs has 16 cursor tests on literal words) | 64% fail; no regex engine |
| `:s` ranges/flags/regex/replacement specials | 3 trivial calls in ex/command files; a few flag tests in `search.rs` | 85% fail |
| `:g`/`:v` | 4 tests, `d` and `s` only | 95% fail |
| Ex addresses (patterns, offsets, marks, `$`, `;`) | untested | all fail |
| Dot repeat | 3 tests (`dd.`, `cw.`, `ce.`) + 30 unit tests | 72% fail |
| Column memory / `curswant` | 1 comment in `mod.rs:2394`, no test | fails |
| Word motions over blank lines / punctuation | untested | fails |
| Insert-mode control keys | 10 tests, each one input | 48% fail |
| Auto-pairs / completion popup vs Vim keys | untested as a Vim-compat concern | fails |
| Visual uppercase ops, `=`, `gJ`, `s` | doc says ✅; unimplemented (`keys.rs:6097` has no arm) | fails |
| Visual block ops beyond `I`/`A` | 4 tests | 60% fail |
| Text-object counts, whitespace/punct objects, `ap` boundaries | 3 shallow tests in `normal_mode.rs` + unit matrix | 42% fail |
| Registers `"_`, `".`, `"/`, `"=`, `"1` rules | 7 STATE tests in `vim_features.rs` | 36% fail |
| Marks tracking edits, `''` toggle, jumplist from marks/`n`/`%` | 4 cursor tests | 37% fail |
| Macros: `qA`, failure stop, recursion | 1 line-count test + 15 unit tests | 30% fail |
| `<C-a>`/`<C-x>` formats | 4 tests | 46% fail |
| Scrolling (`'scroll'`, `<C-b>` overlap, `scrolloff`, `M`) | STATE tests on `scroll_top` | 38% fail |
| Basic `d`/`y`/`c` with `w e b $ 0 f t` on one line | well covered (operator_motions.rs) | passes |
| `f`/`t`/`;`/`,` (except `;` after `t`) | well covered | passes except cpo-`;` |
| Commentary, netrw listing, `[#`/`]#`, `g?` | well covered (exact buffers) | n/a |

### 4.3 `VIM_COMPATIBILITY.md` trustworthiness

- Claims 422/424 (100%); 423 ✅ marks. **14 ✅ rows have no test anywhere** (insert-mode arrows and Home/End, `gj`, `gk`, `(`, `[p`, `gN`, `` `A ``, ` `` `, `` `. ``, visual `~`, visual `=`, visual `gJ`, operator-pending `;`/`,`) and 5 more are half-covered.
- **Three ✅ rows are not implemented**: visual `=`, visual `s`, visual `gJ` (`src/core/engine/keys.rs:6097` handler has no arm; `:6549–6640` has no `gJ`). The probe confirms all three.
- **Nine ✅ rows are partial in ways the doc does not say**: `dH`/`dM`/`dL` discard the count (`keys.rs:3606/3618/3632`), `M` never consumes its count (`:1192`), `gm`/`gM` ignore count and scroll (`:2086/:2093`), `<C-f>` is find/replace unless `ctrl_f_action=page_down` (`:327/:675`), `'a`/`` `a `` never push the jumplist (`:2568–2700`), `zf` supports 6 motions.
- The doc's own source (`vim-index.txt`) is git-ignored and absent from the repo (`.gitignore:5`; a copy exists only at `~/src/vimcode/vim-index.txt`), so the checklist cannot be regenerated or diffed from a clean clone.
- The doc also marks `/pattern`, `:s`, `:g` as ✅ with no qualifier — after §3.1 that is the most misleading entry: the *syntax* is accepted but the semantics are literal-string.
- **Assessment: the ✅ column measures "the key is dispatched", not "behaves like Vim". Treat it as a feature index, not a compatibility statement.** Nothing cross-checks it; the oracle harness is the natural cross-check (see Issue 6).

---

## 5. TUI-specific risks a core-engine test cannot catch

Findings from `src/tui_main/mod.rs:3106 translate_key` (no unit tests exist for it) and crossterm 0.29's parser (`event/sys/unix/parse.rs:92–110`):

1. **`<C-h>` types `h` in insert mode.** Crossterm maps byte `0x08` to `Char('h')+CONTROL` (only `0x7F` is `Backspace`). `translate_key` forwards it as `("h", Some('h'), ctrl=true)`; `handle_insert_key` (`keys.rs:4622`) has no `ctrl && "h"` arm and falls through to inserting `unicode`. Confirmed by probe `ins:C-h as BS` → `abch`. Terminals configured with `stty erase ^H` send `0x08` for the **Backspace key itself**.
2. **`<C-j>` types `j`**: `0x0A` in raw mode → `Char('j')+CONTROL`; Vim treats it as `<NL>` (newline in insert, `j` in normal). Probe `ins:C-j newline` → `abj`.
3. **`<C-c>` types `c` in insert mode** (probe `misc:C-c in insert` → `xcxabc`); Vim leaves insert mode. No TUI-level intercept exists for the editor pane (the only `Ctrl+C` handler is the message-line selection copy).
4. **`<C-[>`** is fine in a terminal (it *is* `0x1B`), but any engine-level path that receives `ctrl+'['` inserts `[` — relevant to GTK, and to kitty-protocol terminals where `<C-[>` may be delivered distinctly (`keyboard_enhanced`).
5. **`<Esc>` + fast next key.** Crossterm folds `ESC` followed by a byte in the same read into `KeyModifiers::ALT`; `translate_key` ignores `ALT` for ordinary chars and delivers the bare char — so `<Esc>j` typed fast in insert mode can lose the `<Esc>` and insert `j`. Vim solves this with `ttimeoutlen`. Untested and unmitigated; needs a pty-level test.
6. `<C-i>` → `Tab`, `<C-m>` → `Enter`, `<C-@>` → `Char(' ')+CONTROL` → `"space"` are handled. `<C-\>`/`<C-/>`/`<C-]>` have explicit non-enhanced mappings (`0x1C`–`0x1F` → `'4'`–`'7'`), which is correct but again untested.
7. **`<C-f>` opens find/replace** by default in normal mode — not a key-encoding issue but the first thing a Vim user presses to page down.
8. The `TuiDriver` harness (`src/tui_main/shell_app.rs`) exists but no test feeds raw byte sequences through crossterm's parser into `translate_key`; the byte→`KeyEvent`→`(name, unicode, ctrl)` chain is the untested seam.

---

## 6. Proposed issues

Priority = likelihood a hard-core Vim user notices (P0 within minutes, P1 within a session, P2 within a week). Sizes: S ≤ ½ day, M 1–2 days, L 3+ days. All are self-contained; each includes the probe labels to reproduce (`PROBE_FILTER=<label>` with the harness from Issue 1).

### Issue 1 — Land the extended Neovim-oracle harness and make it a CI gate  (P0, M)

**Problem.** `tests/nvim_conformance.rs` is the only oracle-backed test and has 31 cases; it also has three fidelity bugs that make macro, undo and scroll cases unusable (fixture undo history, `feedkeys` `"nx"` vs `"ntx"`, no `ensure_cursor_visible` after cursor placement). Issue #795 already covers "nvim not installed in CI"; this issue is the harness itself.

**Files.** `tests/nvim_conformance.rs`; `.github/workflows/*` only to install `nvim` (coordinate with #795).

**Do.**
1. Apply the seven harness changes in §2.1 verbatim (they are in this session's worktree copy; diff is small and self-explanatory). Keep `Case` backwards compatible via the `c()`/`cs()` `const fn` constructors.
2. Split `CASES` into per-area `const` arrays (`CASES_OP`, `CASES_DOT`, `CASES_UNDO`, `CASES_REG`, `CASES_MAC`, `CASES_MARK`, `CASES_SEARCH`, `CASES_EX`, `CASES_INS`, `CASES_VIS`, `CASES_VB`, `CASES_NUM`, `CASES_SCROLL`, `CASES_WORD`, `CASES_TO`, `CASES_MISC`) flattened by the runner.
3. Add an **expected-failure list** (`KNOWN_DEVIATIONS: &[&str]` of labels): a known-failing label that *passes* fails the test (so fixes must delete their entry), and an unlisted failure fails the test. This lets the 1,432-case set land green today and shrink monotonically.
4. Add `PROBE_FILTER` and the `BUF`/`CUR` tagging to the failure printout.
5. Document in `CLAUDE.md` Testing section: "a Vim-behaviour PR must add oracle cases; hand-written expectations are second-class."

**Acceptance.** `cargo test --no-default-features --test nvim_conformance` runs all cases, prints per-category totals, is green with the known-deviation list, and turns red if any listed label starts passing or any unlisted label fails. Macro (`mac:qaxjq @a`), undo (`undo:xxx u`), and scroll (`scroll:H from 30`) cases pass on the nvim side.

### Issue 2 — `/` search and `:s` have no regex engine: implement Vim-pattern → `regex` translation  (P0, L)

**Problem.** `Engine::run_search` (`src/core/engine/execute.rs:3015`) uses `str::find`; `execute_substitute_command` (`execute.rs:2941`) splits on `/` and uses `str::replace`. Every metacharacter is literal: `^ $ . * [] \< \> \+ \{n} \| \( \) \1 \zs \ze \c \C \v \V \s \d \w \n` and search offsets (`/e`, `/b+2`, `/+1`, `;`). `:s` also ignores `ignorecase`, `n`/`&`/`I` flags, `:&&`, `~`, replacement specials (`& \0 \1 \U \u \L \E \r \t \& \~ \/`), empty pattern (= last search), alternate delimiters, `|` chaining, counts, and never moves the cursor.

**Files.** `src/core/engine/execute.rs` (`run_search`, `search_next/prev`, `execute_substitute_command`, `replace_*_in_string`); a new `src/core/vim_regex.rs` (translator, unit-tested); `src/core/engine/keys.rs` for `/pat/e` offsets and `d/pat` operator-pending (see Issue 3).

**Do.** Write a Vim-magic → Rust `regex` translator covering magic/nomagic/very-magic/very-nomagic, `\<`/`\>` → `\b`, `\{n,m}`/`\{-}`, `\zs`/`\ze` (via capture + post-processing), `\c`/`\C`, `\n` (multi-line search over the whole rope text), and `~`. Apply `ignorecase`/`smartcase` at compile time. Replacement string expansion per `:h sub-replace-special`. Cursor after `:s` = start of last substituted line. Search offsets per `:h search-offset`. `//` and `/<CR>` reuse the last pattern; `:s//x/` reuses it too; `*`/`#` set it with `\<\>`.

**Oracle cases to add / un-skip** (all in the worktree harness): every `search:*` and `sub:*` label in Appendix A; specifically `search:/^foo`, `/foo$`, `/\<foo\>`, `/\d\+`, `/[bc]a`, `/a\{2}`, `/\v`, `/foo\|bar`, `/a\.c`, `/\zs`, `/\ze`, `/\c`, `/pat/e`, `/pat/e+1`, `/pat/b+2`, `/pat/+1 linewise`, `/pat/;/pat2/`, `//`, `3/pat`, `scs lower`, `* with ic scs`, `* then :s//`; `sub:2,3`, `.,+1`, `.,$`, `'a,'b`, `'<,'> explicit`, `backrefs`, `& in replacement`, `\U&`, `\r newline`, `alternation`, `\zs`, `~ prev replacement`, `# delimiter`, `empty pattern last search`, `count`, `trailing ws`, `^ anchor`, `$ anchor`, `.*`, `bar chain`, `cursor after`, `\n multiline`, `n flag`, `& flag`, `&&`, `I flag with ic`, `ic applies`.

**Acceptance.** All listed labels removed from `KNOWN_DEVIATIONS` and green; translator unit tests for each Vim atom; `tests/search.rs` gains `n`/`N`-after-`?` and regex cases asserting cursor.

### Issue 3 — Operator-pending `/`, `?`, `n`, `N`, `` ` ``, `%`-search, and `<CR>`/`2$` motions; `x`/`cw`/`dj` edge cases  (P0, M)

**Problem.** `d/pat<CR>`, `c/pat`, `y/pat`, `dn`, `` d`a ``, `v/pat`, `vn` are not motions — the keys leak into insert mode (`op:d/pat` → `fz`/`oo bar baz`). `x` on an empty line deletes the newline (`op:x on empty line`). `cw` on whitespace/punctuation/EOL deletes to end of line or joins lines (`op:cw on whitespace`, `cw on punctuation`, `misc:cw on space at eol`, `word:cw at eol punct`). `dj` on the last line / `dk` on the first deletes a line instead of failing (`op:dj last line`, `dk first line`, `misc:2dd on last`). `2dw`/`3dw` stop at EOL (`op:2dw across line end`). `d%` off-bracket does not search forward (`op:d% before paren`). `<CR>` is not a motion; `2$` ignores its count; `;` after `t` does not skip the adjacent match (`op:t; then ; repeat`); a pending count survives `<Esc>` (`misc:2d then Esc` → `2d<Esc>x` deletes two chars); `cc`/`S` drop autoindent (`op:cc keeps indent`).

**Files.** `src/core/engine/keys.rs` (operator-pending dispatch, `x`, `cw` special-case, count reset on `<Esc>`), `src/core/engine/motions.rs` (`w` motion for `cw`, `%` forward search, `;` cpo semantics, `<CR>`/`+`, `$` count).

**Acceptance.** Labels above green in the oracle harness; plus a regression case for each in `tests/operator_motions.rs` asserting exact buffer + cursor (replace the permissive `test_dj_at_last_line_noop_or_delete_last` at `:1047` with the Vim answer).

### Issue 4 — Dot-repeat records the wrong change for `A I o O cc C s S R p >> <C-a>`, visual ops, counts, and `@:`  (P0, M)

**Problem.** `.` after `A`/`I`/`o`/`O` re-inserts at the cursor instead of re-running the command (`dot:A; j .` → `;b`; `dot:o .` → `bb`); after any `c`-family change it re-inserts text without deleting (`dot:cc j .` → `Xb`, `dot:ct, .`, `dot:cw . next word`, `dot:vec .`); `p`, `<C-a>`, `xp` are not repeatable (`dot:yyp .`, `num:C-a .`); a new count on `.` is ignored or multiplied wrongly (`dot:3x 2.` passes but `dot:3Ax j 2.`, `dot:>> 2.`, `dot:2dd 3.`, `dot:dfx count override` fail); `.` after a visual or text-object operator repeats a one-unit op (`dot:Vjd .`, `vlld .`, `dap .`, `diw . .`, `vjd . charwise`); block-mode `I`/`c`/`d`/`r` do not repeat (`vb:*then .`); `@:` re-executes twice (`dot:@:`); `.` after `@a` replays the macro (`mac:@a then .`); insert-mode `<C-w>` inside a repeated insert is wrong (`dot:i<C-w> .`).

**Files.** `src/core/engine/keys.rs` (the `last_change`/repeat recording and replay), `src/core/engine/execute.rs` for `@:`.

**Do.** Record the *command* (operator, motion/text-object with its extent, register, count, inserted text, and for visual ops the selection size in lines/cols) rather than the inserted text; replay with the new count replacing the old. Follow `:h .` and `:h visual-repeat`.

**Acceptance.** All `dot:*` labels green; `tests/normal_mode.rs` gains one exact-buffer dot test per command family (`A`, `o`, `cc`, `C`, `s`, `p`, `>>`, `<C-a>`, `Vjd`, `dap`, `<C-v>I`).

### Issue 5 — Motion column memory, blank-line word motions, paragraph boundaries, `'scroll'`/`scrolloff`/`<C-b>`/`M`, and `<C-f>` default  (P1, M)

**Problem.** `j`/`k` across a shorter line lose the desired column and `$` does not stick (`word:jj col memory`, `$jj`, `$ then j to longer`, `dd then j col`, `ins:Down Down col memory`). `w`/`b`/`ge` skip empty lines (`word:w onto blank line`, `b onto blank line`, `ge onto blank`, `w over multiple blank lines`); `}`/`{` treat whitespace-only lines as paragraph boundaries and are off-by-one from a blank line (`word:} whitespace-only line not blank`, `} from blank`, `}}}`, `( para`). `<C-f>` opens find/replace (`scroll:C-f`, `C-f on short buffer`) — make `page_down` the default in Vim mode, keep the option for VSCode mode. `N<C-d>`/`N<C-u>` must set `'scroll'` not multiply (`scroll:5C-d C-d`, `3C-d sets scroll then C-u`); `<C-b>` keeps a 2-line overlap (`scroll:C-b`, `2<C-b>`); `scrolloff` is ignored by `<C-e>`/`H`/`L` (`scroll:so=5 *`); `M` is off by one and wrong on short buffers (`scroll:M *`, `GM`, `dM`); long jumps should centre the cursor (`scroll:50% H`, `C-d then H`). `%` inside quotes (`word:% in quotes`).

**Files.** `src/core/engine/motions.rs` (`curswant`-style desired column — `mod.rs:2394` already documents the intent; word/paragraph motions), `src/core/engine/keys.rs` (`<C-f>` default, `'scroll'` state), `src/core/settings.rs` (`ctrl_f_action` default per editor mode).

**Acceptance.** Labels green; `tests/normal_mode.rs` gains `jj` over a short line, `$jj`, `w` onto a blank line, `}` with a whitespace-only line, each asserting cursor; `scroll` cases assert cursor after `H`/`L` with `scrolloff`.

### Issue 6 — Registers, marks, jumplist and macro semantics  (P1, M)

**Problem.** `"_` writes the unnamed register (`reg:"_dd then p`, `viw"_dP`, `ex:d _`); `".`, `"/`, `"=` are empty (`reg:". insert register`, `"/ last search`, `"= expr`, `C-r = in insert`); `"1` is not set by `cc` or by `d%`/`d/`/`dn` (`reg:"1 after cc`, `d% goes to "1`, `d/ goes to "1`, `dn goes to "1`); `"Ayw` appends with a newline (`reg:"ayw "Ayw "ap`); `"adw` leaks into `"-`; multi-line register with a paste count is grouped per line instead of repeated (`misc:2yy 3p`, `vis:Vjy then p count`); paste-count cursor (`reg:3"ap`, `misc:P count`, `yy3p`, `2gp`). Marks do not track inserted/deleted lines (`mark:mark shifts after O`, `'a after text insert above`, `` `a after line join ``, `mark on deleted line`); `''` does not toggle and mark jumps don't push the jumplist (`mark:'' toggles`, `` `` after '' ``, `'a then '' back`, `jump:'a C-o`, `C-o after ''`) — `keys.rs:2568–2700` never calls `push_jump_location`; `n`, `%`, `(` don't push jumps while `20j` does (`jump:n C-o`, `% C-o`, `C-o after (`, `C-o after j 20 lines`); `3<C-o>` ignores the count; `` `^ ``, `'[` after `>` unset. Macros: `qA` append unsupported (`mac:qA append`); `N@a` does not stop on failure (`mac:10@a stops at failure`); a recursive macro does not terminate at EOF and spins to the iteration cap (`mac:recursive`); `2dw` inside a macro deletes the line (`mac:count inside`); `ci(` in a macro is a no-op (`mac:macro with ci(`).

**Files.** `src/core/engine/keys.rs` (register write rules per `:h registers`, `push_jump_location` in the mark handler, macro failure propagation), `src/core/engine/motions.rs` (mark adjustment on line insert/delete — hook the buffer edit path), `src/core/engine/mod.rs` (`Registers` for `.`, `/`, `=`).

**Acceptance.** Labels green; `tests/vim_features.rs` register tests rewritten to round-trip through `p` and assert exact buffer (replace `normal_mode.rs:284 test_black_hole_register` which passes with `"_` ignored); a mark-adjustment test and a `''` toggle test; a `100@a`-stops test and a recursive-macro-terminates test.

### Issue 7 — Insert-mode Vim keys vs. IDE defaults: `<C-h>`/`<C-j>`/`<C-c>`, autoindent cleanup, Tab-to-tabstop, `<C-w>`/`<C-u>`/`<C-o>`/`<C-v>`, autopairs and completion popup  (P1, M)

**Problem.** In a real terminal crossterm delivers `<C-h>` as `ctrl+h`, `<C-j>` as `ctrl+j`, `<C-c>` as `ctrl+c`; `handle_insert_key` (`keys.rs:4622`) inserts the literal letter for any ctrl combo it doesn't know (`ins:C-h as BS` → `abch`, `ins:C-j newline` → `abj`, `misc:C-c in insert` → `xcxabc`, `misc:C-[ in insert`). Leaving a freshly auto-indented line empty must remove the indent (`ins:CR then Esc removes autoindent`, `CR CR keeps prev line empty` — trailing whitespace is left in the file); `<CR>` mid-line with `autoindent` should strip the leading whitespace of the new line (`ins:CR mid-line`). `<Tab>` with `expandtab` should go to the next tabstop, not always insert `shiftwidth` spaces (`ins:Tab mid line ts4`, `ts8`, `after 2 chars`, `insert Tab then BS`). `<C-w>` on punctuation deletes the whole line and at col 1 doesn't join (`ins:C-w punctuation`, `C-w at line start joins`); `<C-u>` before the insert start is a no-op (`ins:C-u before start`, `C-u with indent`); `0<C-d>`; `<C-o>` at EOL / with count / with `:` (`ins:C-o $ then type`, `A C-o h`, `C-o with count`, `C-o :s`); `<C-v>065`/`x41`/`u00e9`; `<C-p>` picks the wrong candidate; `<Tab>` after a word prefix accepts the completion popup instead of inserting a tab (`ins:typing prefix then Tab`); `auto_pairs` defaults on (`ins:( no autopair` → `()a`). Insert counts are ignored unless at col 1 (`misc:count on i then esc col`, `2Ix`, `2ox`, `ins:A with count and CR`). Arrow keys should split the undo block (`undo:arrow breaks undo`).

**Files.** `src/core/engine/keys.rs` (`handle_insert_key`: explicit arms for `ctrl+h`→BS, `ctrl+j`/`ctrl+m`→CR, `ctrl+c`/`ctrl+[`→Esc, and a final `if ctrl { return }` so unknown ctrl combos never insert text), `src/core/settings.rs` (`auto_pairs` default off in Vim mode / on in VSCode mode; completion popup must not capture `<Tab>`/`<CR>` in Vim mode unless explicitly navigated), `src/tui_main/mod.rs:3106 translate_key` (add unit tests feeding crossterm `KeyEvent`s for `0x08`, `0x0A`, `0x03`, `0x1B`, `0x7F`).

**Acceptance.** Labels green; new `#[cfg(test)]` for `translate_key` in `src/tui_main/mod.rs` covering the byte→name table; a `TuiDriver` test in `src/tui_main/shell_app.rs` that presses `Ctrl+h` in insert mode and asserts the rendered line lost a character (black-box, per CLAUDE.md).

### Issue 8 — Visual-mode operators, visual-block editing, text-object counts, and `<C-a>` number formats  (P2, L)

**Problem.** Charwise-visual `D X Y C S R s gJ =` are unimplemented (`vis:vjD` … `vis:Vj=`; doc marks `=`/`s`/`gJ` ✅); `vf,d` excludes the found char; counts on visual text objects and `V3>`/`V2<` ignored; `gv` after a delete/`p` wrong; cursor after `Vjd`/`Vd`/`Vr-`/`VGd`/`vjy` wrong; linewise register into a charwise selection (`vis:vlp linewise reg`). Block mode: `c`/`C`/`D`/`s`/`$d`/ragged `d`/`I` on short lines/`2I`/`r<CR>`/`>`/`<` wrong (`vb:*`); **blockwise yank is pasted as lines** (`vb:jy then P` and 6 siblings). Text objects: `iw`/`aw` on whitespace and punctuation, counts (`d2aw`, `d3iw`, `2daw`, `d2i(`, `d2it`), `ap`/`ip` boundaries and cursor, `di(` on `)` / forward search / across lines / empty, `di"` on the closing quote / before the quotes, `ci{` multiline, `das` boundaries (`to:*`). Numbers: octal/leading-zero corruption (`num:leading zeros 0099 C-x` → `1777777777777777777777`, `C-x on 0 leading zeros 000`, `leading zeros 009`, `binary 0b101`, `C-a on 99999999999999999999`), hex case/wraparound (`num:hex 0xaB`, `hex C-x below zero`, `0X0f`, `-0x1`), visual `<C-a>` without `g` and charwise/blockwise visual (`num:V C-a` and siblings). Also `dH`/`dM`/`dL` count discard and `M` count leak (`keys.rs:3606/3618/3632/1192`), `gm`/`gM` count, `=` operator (`op:=G braces`, `=ip flat`, `==`), `gq`/`gw` cursor and doubled aliases (`misc:g?g?`, `gqgq`, `gwgw`), `r<CR>`/`r<Tab>`, Replace-mode `<BS>` restore (`op:R BS restores`, `misc:R at eol then BS`).

**Files.** `src/core/engine/keys.rs` (`handle_visual_key` `:6097`, block ops, `<C-a>` parsing — use `i64` with `nrformats` semantics and width-preserving leading zeros), `src/core/engine/motions.rs` (text objects `:1743`, `:2182`), register type must carry `Blockwise` (currently `(String, bool)`; `tests/common/mod.rs assert_register` needs a third variant).

**Acceptance.** Labels green; `tests/visual_mode.rs` gains exact-buffer tests for each uppercase op and for blockwise `y`+`p`; `assert_register` extended to blockwise; a number-format table test in `tests/new_vim_features.rs` (`007`, `009`, `0099`, `0x0`, `0xaB`, `0b101`, `-0x1`) with expected values from the oracle.

---

## 7. Recommended sequencing

1. **Issue 1 first, alone, small PR.** It converts this report's 621 rows into a living known-deviation list and stops regressions from the day it lands. Every later issue's acceptance criterion is "delete labels from that list", which keeps reviewers honest without hand-written expectations. Pair it with #795 (install nvim in CI) or it protects nothing in CI.
2. **Issue 2 (regex) and Issue 3 (operator-pending / `x` / `cw` / `dj`) in parallel** — they touch different files (`execute.rs`+new module vs `keys.rs`/`motions.rs`) and are the two things a Vim user hits in the first minute (`/^` and `cw`). Issue 3 should also un-permissive the five tautological operator tests it touches.
3. **Issue 4 (dot repeat)** next — it depends on Issue 3's operator-pending refactor (the change record needs the motion extent) and is the highest-frequency remaining breakage.
4. **Issue 7 (insert keys / TUI encoding)** — independent of 2–4, small blast radius, and the `<C-h>`/`<C-c>` items are embarrassing in a terminal; can run concurrently with 2/3 on another machine since it is confined to `handle_insert_key`, `settings.rs` defaults, and `translate_key`.
5. **Issue 5 (column memory, blank-line motions, scrolling)** and **Issue 6 (registers/marks/macros)** — both P1, independent of each other; schedule after 2–4 because their fixes are localised and their tests are simple cursor assertions.
6. **Issue 8** last: it is the long tail (visual/block/text-object/number edge cases) and benefits from the register-type change in Issue 6 landing first.
7. Throughout: **do not add hand-authored expectations for Vim behaviour without an oracle case beside them**, and delete the checklist's ✅ for visual `=`/`s`/`gJ` until implemented. Consider replacing `VIM_COMPATIBILITY.md`'s percentages with a number derived from the known-deviation list (`1 - |KNOWN_DEVIATIONS| / |cases|`), which is currently **56.4%**, not 99%.

---

## Appendix A — full deviation table (run 3, 621 of 624 parsed; the 3 unparsed — `search:/foo\|bar`, `search:/[bc]a`, `sub:[] class`, whose labels contain `]` — are covered in §3.1 and visible in `probe_run3.txt`)

Columns: label; keys; start buffer @ (line,col); buffer (`nvim=`…`vc=`…, omitted when identical); cursor; mismatch kind. `LONG(60 lines)` is the 60-line `L01 a … L60 h` fixture.

| label | keys | start | buffer | cursor | mismatch |
|---|---|---|---|---|---|
| `op:2dw across line end` | `"2dw"` | `["a b", "c d"]`@(1,3) | nvim="a d" vc="a \nc d" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `op:3dw crossing lines` | `"3dw"` | `["one two", "three four"]`@(1,5) | nvim="one " vc="one \nthree four" | nvim=(1,4) vc=(1,4) | BUF |
| `op:cw on whitespace` | `"cwX<Esc>"` | `["foo   bar"]`@(1,4) | nvim="fooXbar" vc="fooX" | nvim=(1,4) vc=(1,4) | BUF |
| `op:cw on punctuation` | `"cwX<Esc>"` | `["foo.bar"]`@(1,4) | nvim="fooXbar" vc="fooX" | nvim=(1,4) vc=(1,4) | BUF |
| `op:cc keeps indent` | `"ccX<Esc>"` | `["    foo", "bar"]`@(1,6) | nvim="    X\nbar" vc="X\nbar" | nvim=(1,5) vc=(1,1) | BUF+CUR |
| `op:2cc` | `"2ccX<Esc>"` | `["  a", "  b", "c"]`@(1,1) | nvim="  X\nc" vc="X\n  b\nc" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `op:d% before paren` | `"d%"` | `["foo(a, b) bar"]`@(1,1) | nvim=" bar" vc="foo(a, b) bar" | nvim=(1,1) vc=(1,1) | BUF |
| `op:d/pat` | `"d/baz<CR>"` | `["foo bar baz"]`@(1,1) | nvim="baz" vc="fz\noo bar baz" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `op:d/pat/e` | `"d/bar/e<CR>"` | `["foo bar baz"]`@(1,1) | nvim=" baz" vc="fr/e\noo bar baz" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `op:d?pat` | `"d?foo<CR>"` | `["foo bar baz"]`@(1,9) | nvim="baz" vc="foo bar baz" | nvim=(1,1) vc=(3,1) | BUF+CUR |
| `op:dn` | `"/foo<CR>ggdn"` | `["foo bar foo baz"]`@(1,1) | nvim="foo baz" vc="foo bar foo baz" | nvim=(1,1) vc=(1,1) | BUF |
| `op:d/pat multiline` | `"d/ccc<CR>"` | `["aaa", "bbb", "ccc"]`@(1,2) | nvim="a\nccc" vc="c\n\nbbb\nccc" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `op:d/pat/+1 linewise` | `"d/bbb/+1<CR>"` | `["aaa", "bbb", "ccc", "ddd"]`@(1,1) | nvim="ddd" vc="aaa\nbbb\nccc\nddd" | nvim=(1,1) vc=(1,1) | BUF |
| `op:d/pat to col1 exclusive rule` | `"d/ccc<CR>"` | `["aaa", "bbb", "ccc"]`@(1,1) | nvim="ccc" vc="c\n\nbbb\nccc" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `op:t; then ; repeat` | `"t;;"` | `["foo; bar; baz"]`@(1,1) | (buffer identical) | nvim=(1,8) vc=(1,3) | CUR |
| `op:t; then ; then ;` | `"t;;;"` | `["a;b;c;d"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `op:t, ; ,` | `"t,;,"` | `["a,b,c,d"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `op:T, then ;` | `"T,;"` | `["a,b,c,d"]`@(1,7) | (buffer identical) | nvim=(1,5) vc=(1,7) | CUR |
| `op:x on empty line` | `"x"` | `["", "x"]`@(1,1) | nvim="\nx" vc="x" | nvim=(1,1) vc=(1,1) | BUF |
| `op:dj last line` | `"dj"` | `["a", "b"]`@(2,1) | nvim="a\nb" vc="a" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `op:dk first line` | `"dk"` | `["a", "b"]`@(1,1) | nvim="a\nb" vc="b" | nvim=(1,1) vc=(1,1) | BUF |
| `op:d`a` | `"magg0d`a"` | `["abc def", "ghi jkl"]`@(2,4) | nvim=" jkl" vc="abc def\nghi jkl" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `op:y`a cursor` | `"magg0y`a"` | `["abc def", "ghi jkl"]`@(2,4) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `op:yiw cursor` | `"yiw"` | `["foo bar"]`@(1,6) | (buffer identical) | nvim=(1,5) vc=(1,6) | CUR |
| `op:yy 3p` | `"yy3p"` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(4,1) | CUR |
| `op:p linewise cursor first nonblank` | `"yyjp"` | `["  a", "b"]`@(1,1) | (buffer identical) | nvim=(3,3) vc=(3,1) | CUR |
| `op:P linewise cursor` | `"yyjP"` | `["  a", "b"]`@(1,1) | (buffer identical) | nvim=(2,3) vc=(2,1) | CUR |
| `op:p charwise multiline` | `"vjy$p"` | `["abc", "def", "ghi"]`@(1,2) | (buffer identical) | nvim=(1,4) vc=(1,8) | CUR |
| `op:P charwise multiline` | `"vjyP"` | `["abc", "def", "ghi"]`@(1,2) | (buffer identical) | nvim=(1,2) vc=(1,6) | CUR |
| `op:5dd from last line` | `"5dd"` | `["a", "b", "c"]`@(3,1) | nvim="a\nb\nc" vc="a\nb" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `op:J after period (vim joinspaces)` | `"J"` | `["end.", "next"]`@(1,1) | nvim="end.  next" vc="end. next" | nvim=(1,5) vc=(1,5) | BUF |
| `op:J next starts with )` | `"J"` | `["foo(", "  )"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,4) | CUR |
| `op:J next blank` | `"J"` | `["a", "", "b"]`@(1,1) | nvim="a\nb" vc="a \nb" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `op:J current ends with space` | `"J"` | `["a ", "b"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,2) | CUR |
| `op:5r beyond eol` | `"5rx"` | `["abc"]`@(1,2) | nvim="abc" vc="axx" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `op:r<CR>` | `"r<CR>"` | `["abc def"]`@(1,4) | (buffer identical) | nvim=(2,1) vc=(1,3) | CUR |
| `op:3r<CR>` | `"3r<CR>"` | `["abcdef"]`@(1,2) | nvim="a\nef" vc="a\n\n\nef" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `op:R BS restores` | `"Rxyz<BS><BS><Esc>"` | `["abcdef"]`@(1,2) | nvim="axcdef" vc="axyzef" | nvim=(1,2) vc=(1,2) | BUF |
| `op:2R` | `"2Rxy<Esc>"` | `["abcdef"]`@(1,1) | nvim="xyxyef" vc="xycdef" | nvim=(1,4) vc=(1,2) | BUF+CUR |
| `op:R <CR>` | `"Rx<CR>y<Esc>"` | `["abcdef"]`@(1,2) | nvim="ax\nydef" vc="axydef" | nvim=(2,1) vc=(1,3) | BUF+CUR |
| `op:g~~ cursor` | `"g~~"` | `["aBc dEf"]`@(1,5) | (buffer identical) | nvim=(1,1) vc=(1,5) | CUR |
| `op:guu` | `"guu"` | `["ABC"]`@(1,3) | (buffer identical) | nvim=(1,1) vc=(1,3) | CUR |
| `op:g~iw` | `"g~iw"` | `["aBc dEf"]`@(1,6) | (buffer identical) | nvim=(1,5) vc=(1,6) | CUR |
| `op:gUap` | `"gUap"` | `["abc", "def", "", "ghi"]`@(1,2) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `op:3gUw` | `"3gUw"` | `["a b c d"]`@(1,1) | nvim="A B C d" vc="A B C D" | nvim=(1,1) vc=(1,1) | BUF |
| `op:gUiw then w .` | `"gUiww."` | `["ab cd"]`@(1,1) | nvim="AB CD" vc="AB cd" | nvim=(1,4) vc=(1,4) | BUF |
| `op:3>> skips blank` | `"3>>"` | `["a", "", "b"]`@(1,1) | nvim="    a\n\n    b" vc="    a\n    \n    b" | nvim=(1,1) vc=(1,1) | BUF |
| `op:>> cursor sol` | `">>"` | `["  abc"]`@(1,4) | (buffer identical) | nvim=(1,7) vc=(1,4) | CUR |
| `op:V2>` | `"V2>"` | `["a", "b"]`@(1,1) | nvim="        a\nb" vc="    a\nb" | nvim=(1,1) vc=(1,1) | BUF |
| `op:3>> j .` | `"3>>j."` | `["a", "b", "c", "d", "e"]`@(1,1) | nvim="    a\n        b\n        c\n    d\ne" vc="    a\n                b\n                c\n           ... | nvim=(2,1) vc=(2,1) | BUF |
| `op:>> noet ts4` | `":set ts=4 noet<CR>>>"` | `["a"]`@(1,1) | nvim="\ta" vc="    a" | nvim=(1,1) vc=(1,1) | BUF |
| `op:>>>> noet ts4` | `":set ts=4 noet<CR>>>>>"` | `["a"]`@(1,1) | nvim="\t\ta" vc="        a" | nvim=(1,1) vc=(1,1) | BUF |
| `op:>> existing tab noet` | `":set ts=4 noet<CR>>>"` | `["\ta"]`@(1,1) | nvim="\t\ta" vc="    \ta" | nvim=(1,1) vc=(1,1) | BUF |
| `op:<< mixed tab space` | `":set ts=4 noet<CR><<"` | `["\t  a"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `op:gqq tw20` | `":set tw=20<CR>gqq"` | `["one two three four five six seven e...`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `op:gqip tw20` | `":set tw=20<CR>gqip"` | `["one two three four five six seven e...`@(1,1) | (buffer identical) | nvim=(4,1) vc=(1,1) | CUR |
| `op:gqq cursor` | `":set tw=20<CR>gqq"` | `["one two three four five six seven e...`@(1,5) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `op:gqq indented` | `":set tw=20<CR>gqq"` | `["    one two three four five six"]`@(1,1) | nvim="    one two three\n    four five six" vc="one two three four\nfive six" | nvim=(2,5) vc=(1,1) | BUF+CUR |
| `op:Vgq` | `":set tw=10<CR>Vgq"` | `["one two three four five six"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `op:o esc removes indent` | `"o<Esc>"` | `["    foo", "bar"]`@(1,1) | nvim="    foo\n\nbar" vc="    foo\n    \nbar" | nvim=(2,1) vc=(2,4) | BUF+CUR |
| `op:o x CR esc no trailing ws` | `"ox<CR><Esc>"` | `["    foo"]`@(1,1) | nvim="    foo\n    x\n" vc="    foo\n    x\n    " | nvim=(3,1) vc=(3,4) | BUF+CUR |
| `op:5i` | `"5ix<Esc>"` | `["a"]`@(1,1) | nvim="xxxxxa" vc="xa" | nvim=(1,5) vc=(1,1) | BUF+CUR |
| `op:3a` | `"3a-<Esc>"` | `["ab"]`@(1,1) | nvim="a---b" vc="a-b" | nvim=(1,4) vc=(1,2) | BUF+CUR |
| `op:3A` | `"3Ax<Esc>"` | `["ab"]`@(1,1) | nvim="abxxx" vc="abx" | nvim=(1,5) vc=(1,3) | BUF+CUR |
| `op:2I` | `"2Ix<Esc>"` | `["  ab"]`@(1,4) | nvim="  xxab" vc="  xab" | nvim=(1,4) vc=(1,3) | BUF+CUR |
| `op:3iab` | `"3iab<Esc>"` | `["a"]`@(1,1) | nvim="abababa" vc="aba" | nvim=(1,6) vc=(1,2) | BUF+CUR |
| `op:2i with CR` | `"2ix<CR><Esc>"` | `["a"]`@(1,1) | nvim="x\nx\na" vc="x\na" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `op:cw on empty line` | `"cwX<Esc>"` | `["", "a"]`@(1,1) | nvim="X\na" vc="X" | nvim=(1,1) vc=(1,1) | BUF |
| `op:dvj charwise force` | `"dvj"` | `["abc", "def"]`@(1,2) | nvim="aef" vc="" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `op:dve exclusive force` | `"dve"` | `["abc def"]`@(1,1) | nvim="c def" vc=" def" | nvim=(1,1) vc=(1,1) | BUF |
| `op:dv$` | `"dv$"` | `["abc def"]`@(1,2) | nvim="af" vc="a" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `dot:cw . next word` | `"cwX<Esc>w."` | `["foo bar baz"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,4) | CUR |
| `dot:A; j .` | `"A;<Esc>j."` | `["a", "b"]`@(1,1) | nvim="a;\nb;" vc="a;\n;b" | nvim=(2,2) vc=(2,2) | BUF |
| `dot:ciw w .` | `"ciwX<Esc>w."` | `["foo bar"]`@(1,1) | nvim="X X" vc="X Xbar" | nvim=(1,3) vc=(1,4) | BUF+CUR |
| `dot:yyp .` | `"yyp."` | `["a"]`@(1,1) | nvim="a\na\na" vc="a\na" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `dot:yy3p .` | `"yy3p."` | `["a"]`@(1,1) | nvim="a\na\na\na\na\na\na" vc="a\na\na\na" | nvim=(3,1) vc=(4,1) | BUF+CUR |
| `dot:o .` | `"ob<Esc>."` | `["a"]`@(1,1) | nvim="a\nb\nb" vc="a\nbb" | nvim=(3,1) vc=(2,2) | BUF+CUR |
| `dot:O .` | `"Ob<Esc>."` | `["a"]`@(1,1) | nvim="b\nb\na" vc="bb\na" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `dot:vlld .` | `"vlld."` | `["abcdefgh"]`@(1,1) | nvim="gh" vc="defgh" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:Vjd .` | `"Vjd."` | `["a", "b", "c", "d", "e"]`@(1,1) | nvim="e" vc="c\nd\ne" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:Vj> j .` | `"Vj>j."` | `["a", "b", "c", "d"]`@(1,1) | nvim="    a\n        b\n    c\nd" vc="    a\n            b\n        c\nd" | nvim=(2,1) vc=(2,1) | BUF |
| `dot:dap .` | `"dap."` | `["a", "", "b", "", "c"]`@(1,1) | nvim="c" vc="b\n\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:ifoo .` | `"ifoo<Esc>."` | `["ab"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,6) | CUR |
| `dot:3Ax j .` | `"3Ax<Esc>j."` | `["a", "b"]`@(1,1) | nvim="axxx\nbxxx" vc="ax\nxb" | nvim=(2,4) vc=(2,2) | BUF+CUR |
| `dot:3Ax j 2.` | `"3Ax<Esc>j2."` | `["a", "b"]`@(1,1) | nvim="axxx\nbxx" vc="ax\nxxb" | nvim=(2,3) vc=(2,3) | BUF |
| `dot:cc j .` | `"ccX<Esc>j."` | `["a", "b"]`@(1,1) | nvim="X\nX" vc="X\nXb" | nvim=(2,1) vc=(2,2) | BUF+CUR |
| `dot:s l .` | `"sX<Esc>l."` | `["abcd"]`@(1,1) | nvim="XXcd" vc="XXbcd" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `dot:C j .` | `"CX<Esc>j."` | `["abc", "def"]`@(1,2) | nvim="aX\ndX" vc="aX\ndXef" | nvim=(2,2) vc=(2,3) | BUF+CUR |
| `dot:ct, .` | `"ct,X<Esc>ll."` | `["a,b,c"]`@(1,1) | nvim="X,X,c" vc="X,Xb,c" | nvim=(1,3) vc=(1,4) | BUF+CUR |
| `dot:df. .` | `"df.."` | `["a.b.c.d"]`@(1,1) | nvim="c.d" vc="b.c.d" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:R .` | `"Rxy<Esc>ll."` | `["abcdef"]`@(1,1) | nvim="xycxyf" vc="xycdef" | nvim=(1,5) vc=(1,4) | BUF+CUR |
| `dot:diw . .` | `"diw.."` | `["foo bar baz"]`@(1,1) | nvim=" baz" vc=" bar baz" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:g&` | `":s/a/b/g<CR>g&"` | `["a a", "a a"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `dot:"ayy "ap .` | `"\"ayyj\"ap."` | `["a", "b"]`@(1,1) | nvim="a\nb\na\na" vc="a\nb\na" | nvim=(4,1) vc=(3,1) | BUF+CUR |
| `dot:"1p . . increments` | `"dddddd\"1p.."` | `["a", "b", "c", "d"]`@(1,1) | nvim="d\nc\nb\na" vc="" | nvim=(4,1) vc=(1,1) | BUF+CUR |
| `dot:>ip .` | `">ip}j."` | `["a", "b", "", "c", "d"]`@(1,1) | nvim="    a\n    b\n\n    c\n    d" vc="    a\n    b\n\nc\nd" | nvim=(4,1) vc=(4,1) | BUF |
| `dot:I .` | `"Ix<Esc>j."` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(2,2) | CUR |
| `dot:i<C-w> .` | `"i<C-w>X<Esc>j$."` | `["ab cd", "ef gh"]`@(1,6) | nvim="ab Xd\nef Xh" vc="ab X\nef gXh" | nvim=(2,4) vc=(2,6) | BUF+CUR |
| `dot:vec .` | `"vecX<Esc>w."` | `["foo bar baz"]`@(1,1) | nvim="X X baz" vc="X Xbar baz" | nvim=(1,3) vc=(1,4) | BUF+CUR |
| `dot:vjd . charwise` | `"vjd."` | `["a1", "b2", "c3", "d4", "e5"]`@(1,1) | nvim="3\nd4\ne5" vc="2\nc3\nd4\ne5" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:<C-v>jIx .` | `"<C-v>jIx<Esc>jj."` | `["ab", "ab", "ab", "ab"]`@(1,1) | nvim="xab\nxab\nxab\nxab" vc="xab\nxab\nxab\nab" | nvim=(3,1) vc=(3,2) | BUF+CUR |
| `dot:cw with count 2.` | `"cwX<Esc>w2."` | `["a b c d e"]`@(1,1) | nvim="X X d e" vc="X X c d e" | nvim=(1,3) vc=(1,4) | BUF+CUR |
| `dot:dfx count override` | `"df.2."` | `["a.b.c.d.e"]`@(1,1) | nvim="d.e" vc="b.c.d.e" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:gUw .` | `"gUww."` | `["ab cd"]`@(1,1) | nvim="AB CD" vc="AB " | nvim=(1,4) vc=(1,3) | BUF+CUR |
| `dot:>> 2.` | `">>2."` | `["a"]`@(1,1) | nvim="    a" vc="            a" | nvim=(1,1) vc=(1,1) | BUF |
| `dot:ofoo<CR>bar .` | `"ofoo<CR>bar<Esc>."` | `["a"]`@(1,1) | nvim="a\nfoo\nbar\nfoo\nbar" vc="a\nfoo\nbafoo\nbarr" | nvim=(5,3) vc=(4,4) | BUF+CUR |
| `dot:p charwise .` | `"ylp."` | `["ab"]`@(1,1) | nvim="aaab" vc="aab" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `dot:xp .` | `"xp."` | `["abcd"]`@(1,1) | nvim="baacd" vc="bcd" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `dot:ciw then . at eol` | `"ciwX<Esc>$."` | `["ab cd"]`@(1,1) | nvim="X X" vc="X cXd" | nvim=(1,3) vc=(1,5) | BUF+CUR |
| `undo:xxxx 3u` | `"xxxx3u"` | `["abcdef"]`@(1,1) | nvim="bcdef" vc="def" | nvim=(1,1) vc=(1,1) | BUF |
| `undo:U` | `"xxxU"` | `["abcdef"]`@(1,1) | nvim="" vc="abcdef" | nvim=(1,1) vc=(1,1) | BUF |
| `undo:UU` | `"xxxUU"` | `["abcdef"]`@(1,1) | nvim="def" vc="abcdef" | nvim=(1,1) vc=(1,1) | BUF |
| `undo:A xyz u cursor` | `"A xyz<Esc>u"` | `["abc"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `undo:arrow breaks undo` | `"ifoo<Left>bar<Esc>u"` | `["ab"]`@(1,1) | nvim="fooab" vc="ab" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `undo:u after :%s cursor` | `":%s/a/b/<CR>u"` | `["a", "a", "a"]`@(3,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `undo:u after visual d` | `"vlldu"` | `["abcdef"]`@(1,2) | (buffer identical) | nvim=(1,2) vc=(1,4) | CUR |
| `undo:u restores cursor after :g` | `":g/a/d<CR>u"` | `["a", "b", "a"]`@(1,1) | nvim="a\nb\na" vc="b" | nvim=(1,1) vc=(1,1) | BUF |
| `undo:u after R` | `"Rxyz<Esc>u"` | `["abcdef"]`@(1,2) | nvim="abcdef" vc="axydef" | nvim=(1,2) vc=(1,4) | BUF+CUR |
| `undo:u after <C-v>I` | `"<C-v>jIx<Esc>u"` | `["ab", "ab"]`@(1,1) | nvim="ab\nab" vc="xab\nab" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `undo:2u after insert ×3` | `"Ax<Esc>Ay<Esc>Az<Esc>2u"` | `["a"]`@(1,1) | nvim="ax" vc="axy" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `reg:"ayw "Ayw "ap` | `"\"ayww\"Ayw$\"ap"` | `["foo bar"]`@(1,1) | nvim="foo barfoo bar" vc="foo barfoo \nbar" | nvim=(1,14) vc=(1,15) | BUF+CUR |
| `reg:d/ goes to "1` | `"d/baz<CR>$\"1p"` | `["foo bar baz"]`@(1,1) | nvim="bazfoo bar " vc="fz\n$\"1poo bar baz" | nvim=(1,11) vc=(2,5) | BUF+CUR |
| `reg:d% goes to "1` | `"d%$\"1p"` | `["(ab) cd"]`@(1,1) | nvim=" cd(ab)" vc=" cd" | nvim=(1,7) vc=(1,3) | BUF+CUR |
| `reg:dn goes to "1` | `"/ab<CR>ggdn$\"1p"` | `["x ab x ab"]`@(1,1) | nvim="x abab x " vc="x ab x ab" | nvim=(1,9) vc=(1,9) | BUF |
| `reg:"_dd then p` | `"yyj\"_ddp"` | `["a", "b", "c"]`@(1,1) | nvim="a\nc\na" vc="a\nc\nb" | nvim=(3,1) vc=(3,1) | BUF |
| `reg:3"ap` | `"\"ayy3\"ap"` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(4,1) | CUR |
| `reg:". insert register` | `"ifoo<Esc>\".p"` | `["ab"]`@(1,1) | nvim="foofooab" vc="fooab" | nvim=(1,6) vc=(1,3) | BUF+CUR |
| `reg:": last cmd` | `":s/a/b/<CR>\":p"` | `["a a"]`@(1,1) | nvim="bs/a/b/ a" vc="b a" | nvim=(1,7) vc=(1,1) | BUF+CUR |
| `reg:"/ last search` | `"/bar<CR>\"/P"` | `["foo bar"]`@(1,1) | nvim="foo barbar" vc="foo bar" | nvim=(1,7) vc=(1,5) | BUF+CUR |
| `reg:i C-r a linewise` | `"\"ayyjA<C-r>a<Esc>"` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(2,2) | CUR |
| `reg:"adw does not set "-` | `"\"adw\"-p"` | `["foo bar"]`@(1,1) | nvim="bar" vc="bfoo ar" | nvim=(1,1) vc=(1,5) | BUF+CUR |
| `reg:viw"_dP` | `"yiwwviw\"_dP"` | `["foo bar"]`@(1,1) | nvim="foofoo " vc="foobar " | nvim=(1,6) vc=(1,6) | BUF |
| `reg:"1 after cc` | `"ccX<Esc>j\"1p"` | `["a", "b"]`@(1,1) | nvim="X\nb\na" vc="X\nb" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `reg:"= expr` | `"\"=1+1<CR>p"` | `["a"]`@(1,1) | nvim="a2" vc="a" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `reg:C-r = in insert` | `"A<C-r>=2*3<CR><Esc>"` | `["a"]`@(1,1) | nvim="a6" vc="a2*3" | nvim=(1,2) vc=(2,1) | BUF+CUR |
| `reg:paste count charwise multiline` | `"vjy2p"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,9) | CUR |
| `mac:10@a stops at failure` | `"qa0f,xjq10@a"` | `["a,b", "c,d", "e f", "g,h"]`@(1,1) | nvim="ab\ncd\ne f\ng,h" vc="ab\ncd\n f" | nvim=(3,1) vc=(4,1) | BUF+CUR |
| `mac:qA append` | `"qaxqqAjq@a"` | `["ab", "cd", "ef"]`@(1,1) | nvim="b\nd\nef" vc="b\ncd\nef" | nvim=(3,1) vc=(2,2) | BUF+CUR |
| `mac:recursive` | `"qaqqaA!<Esc>j@aq@a"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a!\nb!\nc!\nd!" vc="a!\nb!\nc!\nd!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!... | nvim=(4,2) vc=(4,16666) | BUF+CUR |
| `mac:count inside` | `"qa2dwq@a"` | `["a b c d e"]`@(1,1) | nvim="e" vc="" | nvim=(1,1) vc=(1,1) | BUF |
| `mac:"ay then @a executes text` | `"\"ay$j@a"` | `["ix<Esc>", "b"]`@(1,1) | (buffer identical) | nvim=(2,6) vc=(2,7) | CUR |
| `mac:macro with ci(` | `"qaci(X<Esc>jq@a"` | `["f(a)", "g(b)"]`@(1,1) | nvim="f(X)\ng(X)" vc="f(a)\ng(b)" | nvim=(2,3) vc=(2,1) | BUF+CUR |
| `mac:q register letter uppercase Q` | `"qQxq@Q"` | `["ab", "cd"]`@(1,1) | nvim="\ncd" vc="b\ncd" | nvim=(1,1) vc=(1,1) | BUF |
| `mark:mark shifts after O` | `"maggOx<Esc>'a"` | `["a", "b", "c"]`@(2,1) | (buffer identical) | nvim=(3,1) vc=(2,1) | CUR |
| `mark:mark on deleted line` | `"maddgg'a"` | `["a", "b", "c"]`@(2,1) | (buffer identical) | nvim=(1,1) vc=(2,1) | CUR |
| `mark:`^` | `"jAx<Esc>gg`^"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(2,3) vc=(1,1) | CUR |
| `mark:'' toggles` | `"3G''''"` | `["a", "b", "c", "d"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `mark:c`a` | `"ma0c`aX<Esc>"` | `["abc def"]`@(1,5) | nvim="Xdef" vc="aXbc def" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `mark:`a after line join` | `"makJ`a"` | `["ab", "cd"]`@(2,2) | (buffer identical) | nvim=(1,5) vc=(2,1) | CUR |
| `mark:'a after text insert above` | `"maggOx<CR>y<Esc>'a"` | `["a", "b"]`@(2,1) | (buffer identical) | nvim=(4,1) vc=(2,1) | CUR |
| `mark:`` after ''` | `"G''``"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `mark:'a then '' back` | `"magg'a''"` | `["a", "b", "c", "d"]`@(4,1) | (buffer identical) | nvim=(1,1) vc=(4,1) | CUR |
| `mark:'[ after >>` | `">jgg'["` | `["a", "b", "c"]`@(2,1) | (buffer identical) | nvim=(2,5) vc=(1,1) | CUR |
| `jump:g; g; g,` | `"xjjxggg;g;g,"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `jump:n C-o` | `"/foo<CR>n<C-o>"` | `["foo", "foo", "foo"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `jump:% C-o` | `"%<C-o>"` | `["(abc)"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `jump:'a C-o` | `"magg'a<C-o>"` | `["a", "b", "c"]`@(3,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `jump:C-o after j 20 lines` | `"20j<C-o>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(1,1) vc=(21,1) | CUR |
| `jump:C-o after (` | `"(<C-o>"` | `["A b. C d."]`@(1,8) | (buffer identical) | nvim=(1,6) vc=(1,8) | CUR |
| `jump:g; after 2 changes same line` | `"x$xgg0g;g;"` | `["abcdef"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `jump:3<C-o>` | `"2G3G4G5G3<C-o>"` | `["1", "2", "3", "4", "5", "6"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(4,1) | CUR |
| `jump:C-o after ''` | `"3G''<C-o>"` | `["a", "b", "c", "d"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `search:* cursor not on word` | `"*"` | `["  foo bar foo"]`@(1,1) | (buffer identical) | nvim=(1,11) vc=(1,1) | CUR |
| `search:* on punctuation` | `"*"` | `["a.b a.b"]`@(1,2) | (buffer identical) | nvim=(1,7) vc=(1,2) | CUR |
| `search:* then :s//` | `"*:%s//X/g<CR>"` | `["foo bar foo"]`@(1,1) | nvim="X bar X" vc="foo bar foo" | nvim=(1,1) vc=(1,9) | BUF+CUR |
| `search:/pat/e` | `"/bar/e<CR>"` | `["foo bar"]`@(1,1) | (buffer identical) | nvim=(1,7) vc=(1,1) | CUR |
| `search:/pat/e+1` | `"/bar/e+1<CR>"` | `["foo bar baz"]`@(1,1) | (buffer identical) | nvim=(1,8) vc=(1,1) | CUR |
| `search:/pat/e-1` | `"/bar/e-1<CR>"` | `["foo bar baz"]`@(1,1) | (buffer identical) | nvim=(1,6) vc=(1,1) | CUR |
| `search:/pat/b+2` | `"/bar/b+2<CR>"` | `["foo bar baz"]`@(1,1) | (buffer identical) | nvim=(1,7) vc=(1,1) | CUR |
| `search:/pat/s-1` | `"/bar/s-1<CR>"` | `["foo bar baz"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `search:/pat/+1 linewise` | `"/foo/+1<CR>"` | `["a", "foo", "b", "c"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `search:/pat/-1` | `"/foo/-1<CR>"` | `["a", "b", "foo"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `search:/pat/e then n keeps offset` | `"/bar/e<CR>n"` | `["foo bar foo bar"]`@(1,1) | (buffer identical) | nvim=(1,15) vc=(1,1) | CUR |
| `search:/\v` | `"/\\vo+<CR>"` | `["fooo bar"]`@(1,3) | (buffer identical) | nvim=(1,2) vc=(1,3) | CUR |
| `search:/foo\\|bar` | `"/foo\\\|bar<CR>"` | `["xx bar foo"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `search:/\<foo\>` | `"/\\<foo\\><CR>"` | `["foobar foo"]`@(1,1) | (buffer identical) | nvim=(1,8) vc=(1,1) | CUR |
| `search:/^foo` | `"/^foo<CR>"` | `["a foo", "foo b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `search:/foo$` | `"/foo$<CR>"` | `["foo a", "a foo"]`@(1,1) | (buffer identical) | nvim=(2,3) vc=(1,1) | CUR |
| `search:/a\{2}` | `"/a\\{2}<CR>"` | `["a aa aaa"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `search:/\d\+` | `"/\\d\\+<CR>"` | `["ab 123 cd"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `search:scs lower` | `":set ic scs<CR>/foo<CR>"` | `["x FOO Foo foo"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,11) | CUR |
| `search:\c` | `"/\\cfoo<CR>"` | `["x FOO foo"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `search:\C with ic` | `":set ic<CR>/\\CFOO<CR>"` | `["x foo FOO"]`@(1,1) | (buffer identical) | nvim=(1,7) vc=(1,1) | CUR |
| `search:* with ic scs` | `":set ic scs<CR>*"` | `["Foo foo Foo"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,9) | CUR |
| `search:// repeat` | `"/foo<CR>//<CR>"` | `["foo x foo x foo"]`@(1,1) | (buffer identical) | nvim=(1,13) vc=(1,1) | CUR |
| `search:/<CR> repeat` | `"/foo<CR>/<CR>"` | `["foo x foo x foo"]`@(1,1) | (buffer identical) | nvim=(1,13) vc=(1,7) | CUR |
| `search:3/pat` | `"3/foo<CR>"` | `["a foo foo foo"]`@(1,1) | (buffer identical) | nvim=(1,11) vc=(1,3) | CUR |
| `search:/pat/;/pat2/` | `"/foo/;/bar<CR>"` | `["a foo b bar"]`@(1,1) | (buffer identical) | nvim=(1,9) vc=(1,1) | CUR |
| `search:?pat?e` | `"?bar?e<CR>"` | `["foo bar baz"]`@(1,11) | (buffer identical) | nvim=(1,7) vc=(1,11) | CUR |
| `search:c/pat` | `"c/baz<CR>X<Esc>"` | `["foo bar baz"]`@(1,1) | nvim="Xbaz" vc="fz\nXoo bar baz" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `search:y/pat cursor` | `"y/baz<CR>"` | `["foo bar baz"]`@(1,5) | nvim="foo bar baz" vc="fz\noo bar baz" | nvim=(1,5) vc=(2,1) | BUF+CUR |
| `search:/o\nb multiline` | `"/o\\nb<CR>"` | `["foo", "bar"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `search:/ then :s//` | `"/foo<CR>:s//X/g<CR>"` | `["foo bar foo"]`@(1,1) | nvim="X bar X" vc="foo bar foo" | nvim=(1,1) vc=(1,1) | BUF |
| `search:\zs` | `"/foo\\zsbar<CR>"` | `["foobar"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `search:\ze` | `"/foo\\zebar<CR>"` | `["xbar foobar"]`@(1,1) | (buffer identical) | nvim=(1,6) vc=(1,1) | CUR |
| `search:/. literal dot` | `"/a\\.c<CR>"` | `["abc a.c"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `search:/ with * quantifier` | `"/ab*c<CR>"` | `["ac abc abbc"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `search:/ with \s` | `"/\\s<CR>"` | `["a\tb c"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,1) | CUR |
| `search:/ then ? then n` | `"/bar<CR>?bar<CR>n"` | `["bar", "foo", "bar", "bar"]`@(1,1) | (buffer identical) | nvim=(4,1) vc=(3,1) | CUR |
| `search:d/pat/+0? linewise` | `"d/foo/0<CR>"` | `["a", "foo", "b"]`@(1,1) | nvim="b" vc="a\n/0\n\nfoo\nb" | nvim=(1,1) vc=(3,1) | BUF+CUR |
| `search:/\(foo\)\1` | `"/\\(foo\\)\\1<CR>"` | `["foo foofoo"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `search:/\w\+ from col1` | `"/\\w\\+<CR>"` | `["foo bar"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `search:/$ empty match` | `"/$<CR>"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,1) | CUR |
| `search:/^ empty match` | `"/^<CR>"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `search:/\n at eol` | `"/\\n<CR>"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,1) | CUR |
| `search:/ upper V with ic` | `":set ic<CR>/ABC<CR>"` | `["abc ABC"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `search:gd` | `"gd"` | `["int x = 1;", "y = x;"]`@(2,5) | (buffer identical) | nvim=(1,5) vc=(2,5) | CUR |
| `search:gn selects` | `"/foo<CR>ggcgnX<Esc>"` | `["foo bar foo"]`@(1,1) | nvim="foo bar X" vc="X bar foo" | nvim=(1,9) vc=(1,1) | BUF+CUR |
| `search:cgn .` | `"/foo<CR>ggcgnX<Esc>.."` | `["foo bar foo baz foo"]`@(1,1) | nvim="X bar X baz X" vc="XXX bar foo baz foo" | nvim=(1,1) vc=(1,3) | BUF+CUR |
| `search:dgn` | `"/foo<CR>ggdgn"` | `["foo bar foo"]`@(1,1) | nvim="foo bar " vc=" bar foo" | nvim=(1,8) vc=(1,1) | BUF+CUR |
| `search:gN` | `"/foo<CR>gNd"` | `["foo bar foo"]`@(1,11) | nvim=" bar foo" vc="foo bar " | nvim=(1,1) vc=(1,8) | BUF+CUR |
| `sub:%` | `":%s/a/b/<CR>"` | `["a", "a", "a"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `sub:%g cursor` | `":%s/a/x/g<CR>"` | `["a a", "b", "a a"]`@(2,1) | (buffer identical) | nvim=(3,1) vc=(2,1) | CUR |
| `sub:2,3` | `":2,3s/a/b/<CR>"` | `["a", "a", "a", "a"]`@(1,1) | nvim="a\nb\nb\na" vc="a\na\na\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `sub:.,+1` | `":.,+1s/a/b/<CR>"` | `["a", "a", "a", "a"]`@(2,1) | nvim="a\nb\nb\na" vc="a\na\na\na" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `sub:.,$` | `":.,$s/a/b/<CR>"` | `["a", "a", "a", "a"]`@(3,1) | nvim="a\na\nb\nb" vc="a\na\na\na" | nvim=(4,1) vc=(3,1) | BUF+CUR |
| `sub:'a,'b` | `"majjmbgg:'a,'bs/a/b/<CR>"` | `["a", "a", "a", "a"]`@(1,1) | nvim="b\nb\nb\na" vc="a\na\na\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `sub:'<,'> explicit` | `"Vj<Esc>:'<,'>s/a/b/<CR>"` | `["a", "a", "a", "a"]`@(2,1) | nvim="a\nb\nb\na" vc="a\na\na\na" | nvim=(3,1) vc=(3,1) | BUF |
| `sub:ic applies` | `":set ic<CR>:s/a/b/g<CR>"` | `["A a"]`@(1,1) | nvim="b b" vc="A b" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:n flag` | `":s/a/b/gn<CR>"` | `["a a"]`@(1,1) | nvim="a a" vc="b b" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:& flag` | `":s/a/b/g<CR>j:s/a/c/&<CR>"` | `["a a", "a a"]`@(1,1) | nvim="b b\nc c" vc="b b\nc a" | nvim=(2,1) vc=(2,1) | BUF |
| `sub:&&` | `":s/a/b/g<CR>j:&&<CR>"` | `["a a", "a a"]`@(1,1) | nvim="b b\nb b" vc="b b\na a" | nvim=(2,1) vc=(2,1) | BUF |
| `sub:& cmd` | `":s/a/b/g<CR>j:&<CR>"` | `["a a", "a a"]`@(1,1) | nvim="b b\nb a" vc="b b\na a" | nvim=(2,1) vc=(2,1) | BUF |
| `sub:backrefs` | `":s/\\(a\\)\\(b\\)/\\2\\1/<CR>"` | `["ab"]`@(1,1) | nvim="ba" vc="ab" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\v groups` | `":s/\\v(a)(b)/\\2\\1/<CR>"` | `["ab"]`@(1,1) | nvim="ba" vc="ab" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:& in replacement` | `":s/foo/[&]/<CR>"` | `["foo"]`@(1,1) | nvim="[foo]" vc="[&]" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\0` | `":s/foo/[\\0]/<CR>"` | `["foo"]`@(1,1) | nvim="[foo]" vc="[\\0]" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\U&` | `":s/foo/\\U&/<CR>"` | `["foo"]`@(1,1) | nvim="FOO" vc="\\U&" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\u&` | `":s/foo/\\u&/<CR>"` | `["foo"]`@(1,1) | nvim="Foo" vc="\\u&" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\L` | `":s/FOO/\\L&/<CR>"` | `["FOO"]`@(1,1) | nvim="foo" vc="\\L&" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\U..\E` | `":s/foo/\\U&\\E-x/<CR>"` | `["foo bar"]`@(1,1) | nvim="FOO-x bar" vc="\\U&\\E-x bar" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\r newline` | `":s/,/\\r/<CR>"` | `["a,b"]`@(1,1) | nvim="a\nb" vc="a\\rb" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:\t` | `":s/ /\\t/<CR>"` | `["a b"]`@(1,1) | nvim="a\tb" vc="a\\tb" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:alternation` | `":s/a\\\|c/x/g<CR>"` | `["a b c"]`@(1,1) | nvim="x b x" vc="a b c" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\zs` | `":s/foo\\zsbar/X/<CR>"` | `["foobar"]`@(1,1) | nvim="fooX" vc="foobar" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\ze` | `":s/foo\\zebar/X/<CR>"` | `["foobar"]`@(1,1) | nvim="Xbar" vc="foobar" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:~ prev replacement` | `":s/a/x/<CR>:s/b/~y/<CR>"` | `["a b"]`@(1,1) | nvim="x xy" vc="x ~y" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:# delimiter` | `":s#/#-#<CR>"` | `["a/b"]`@(1,1) | nvim="a-b" vc="a/b" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:no replacement` | `":s/b<CR>"` | `["abc"]`@(1,1) | nvim="ac" vc="abc" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:empty pattern last search` | `"/foo<CR>:s//X/g<CR>"` | `["foo bar foo"]`@(1,1) | nvim="X bar X" vc="foo bar foo" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:count` | `":s/a/b/ 2<CR>"` | `["a", "a", "a", "a"]`@(1,1) | nvim="b\nb\na\na" vc="b\na\na\na" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:range + count` | `":2s/a/b/ 2<CR>"` | `["a", "a", "a", "a"]`@(1,1) | nvim="a\nb\nb\na" vc="a\na\na\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `sub:trailing ws` | `":%s/\\s\\+$//e<CR>"` | `["a  ", "b "]`@(1,1) | nvim="a\nb" vc="a  \nb " | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:^ anchor` | `":%s/^/> /<CR>"` | `["ab", "cd"]`@(1,1) | nvim="> ab\n> cd" vc="ab\ncd" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:$ anchor` | `":%s/$/;/<CR>"` | `["ab", "cd"]`@(1,1) | nvim="ab;\ncd;" vc="ab\ncd" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:.*` | `":s/.*/[&]/<CR>"` | `["abc"]`@(1,1) | nvim="[abc]" vc="abc" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:literal dot` | `":s/\\./,/g<CR>"` | `["a.b.c"]`@(1,1) | nvim="a,b,c" vc="a.b.c" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:bar chain` | `":s/a/x/\|s/b/y/<CR>"` | `["a b"]`@(1,1) | nvim="x y" vc="x b" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:cursor after` | `":2s/a/y/<CR>"` | `["x", "a b a"]`@(1,1) | nvim="x\ny b a" vc="x\na b a" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:\n multiline` | `":%s/a\\nb/X/<CR>"` | `["a", "b"]`@(1,1) | nvim="X" vc="a\nb" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\{2}` | `":s/a\\{2}/X/<CR>"` | `["aaa"]`@(1,1) | nvim="Xa" vc="aaa" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\w\+` | `":s/\\w\\+/X/g<CR>"` | `["foo bar"]`@(1,1) | nvim="X X" vc="foo bar" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\< \>` | `":s/\\<foo\\>/X/g<CR>"` | `["foo foobar"]`@(1,1) | nvim="X foobar" vc="foo foobar" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\{-}` | `":s/a\\{-1,}/X/<CR>"` | `["aaa"]`@(1,1) | nvim="Xaa" vc="aaa" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\u\1 swap words` | `":s/\\(\\w\\+\\) \\(\\w\\+\\)/\\u\\2 \\u\\1/<CR>"` | `["foo bar"]`@(1,1) | nvim="Bar Foo" vc="foo bar" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:%s cursor at end` | `":%s/a/x/<CR>"` | `["a", "b", "a"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `sub:s on empty match ^` | `":s/^/x/g<CR>"` | `["abc"]`@(1,1) | nvim="xabc" vc="abc" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:$ anchor g` | `":s/$/;/g<CR>"` | `["ab"]`@(1,1) | nvim="ab;" vc="ab" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:x* g on empty` | `":s/x*/-/g<CR>"` | `["abc"]`@(1,1) | nvim="-a-b-c" vc="abc" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:& with \n in pattern` | `":%s/\\n//<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="abc" vc="a\nb\nc" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `sub:\r in middle then cursor` | `":s/b/\\r/<CR>"` | `["abc"]`@(1,1) | nvim="a\nc" vc="a\\rc" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `sub:\= escaped slash` | `":s/\\//-/<CR>"` | `["a/b"]`@(1,1) | nvim="a-b" vc="a/b" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:\/ in replacement` | `":s/-/\\//<CR>"` | `["a-b"]`@(1,1) | nvim="a/b" vc="a\\b" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:& literal via \&` | `":s/foo/\\&/<CR>"` | `["foo"]`@(1,1) | nvim="&" vc="\\&" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:~ literal via \~` | `":s/a/\\~/<CR>"` | `["a"]`@(1,1) | nvim="~" vc="\\~" | nvim=(1,1) vc=(1,1) | BUF |
| `sub:whole line` | `":s/\\v(\\w+) (\\w+)/\\2 \\1/<CR>"` | `["hello world"]`@(1,1) | nvim="world hello" vc="hello world" | nvim=(1,1) vc=(1,1) | BUF |
| `g:d` | `":g/a/d<CR>"` | `["a", "b", "a", "c"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `g:s` | `":g/a/s/x/y/<CR>"` | `["a x", "b x", "a x"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `g:!` | `":g!/a/d<CR>"` | `["a", "b", "a", "c"]`@(1,1) | nvim="a\na" vc="a\nb\na\nc" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `g:normal` | `":g/a/normal Ax<CR>"` | `["a", "b", "a"]`@(1,1) | (buffer identical) | nvim=(3,2) vc=(1,3) | CUR |
| `g:m0 reverse` | `":g/^/m0<CR>"` | `["1", "2", "3"]`@(1,1) | nvim="3\n2\n1" vc="1\n2\n3" | nvim=(1,1) vc=(1,1) | BUF |
| `g:^$ d` | `":g/^$/d<CR>"` | `["a", "", "b", "", ""]`@(1,1) | nvim="a\nb" vc="a\n\nb" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `g:t$` | `":g/a/t$<CR>"` | `["a", "b"]`@(1,1) | nvim="a\nb\na" vc="a\nba" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `g:cursor after` | `":g/a/s/a/x/<CR>"` | `["a", "b", "a", "c"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `g:j` | `":g/a/j<CR>"` | `["a", "b", "a", "b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,2) | CUR |
| `g:range` | `":2,3g/a/s/a/b/<CR>"` | `["a", "a", "a"]`@(1,1) | nvim="a\nb\nb" vc="a\na\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `g:delimiter` | `":g#a#d<CR>"` | `["a", "b"]`@(1,1) | nvim="b" vc="a\nb" | nvim=(1,1) vc=(1,1) | BUF |
| `g:normal dd` | `":g/a/normal dd<CR>"` | `["a", "b", "a", "c"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `g:+1d` | `":g/a/+1d<CR>"` | `["a", "x", "a", "y"]`@(1,1) | nvim="a\na" vc="a\nx\na\ny" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `g:s// reuse pattern` | `":g/a/s//x/<CR>"` | `["a", "b", "a"]`@(1,1) | nvim="x\nb\nx" vc="a\nb\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `g:normal with count` | `":g/./normal 2x<CR>"` | `["ab", "cd"]`@(1,1) | nvim="\n" vc="ab\ncd" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `g:copy to end reversed order` | `":g/./t.<CR>"` | `["a", "b"]`@(1,1) | nvim="a\na\nb\nb" vc="a\nb" | nvim=(4,1) vc=(1,1) | BUF+CUR |
| `g:d with count` | `":g/[ab]/d 2<CR>"` | `["a", "1", "2", "b", "3", "4"]`@(1,1) | nvim="2\n4" vc="a\n1\n2\nb\n3\n4" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `g:normal @a` | `"qaAx<Esc>qu:g/a/normal @a<CR>"` | `["a", "b", "a"]`@(1,1) | nvim="ax\nb\nax" vc="axx\nb\na" | nvim=(3,2) vc=(1,3) | BUF+CUR |
| `g:.,+1j` | `":g/a\\\|c/.,+1j<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a b\nc d" vc="a\nb\nc\nd" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:t$` | `":t$<CR>"` | `["a", "b"]`@(1,1) | nvim="a\nb\na" vc="a\nba" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `ex:m$` | `":m$<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="b\nc\na" vc="b\nca" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `ex:m-2` | `":m-2<CR>"` | `["a", "b", "c"]`@(3,1) | nvim="a\nc\nb" vc="c\na\nb" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:2,3m$` | `":2,3m$<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a\nd\nb\nc" vc="a\ndb\nc" | nvim=(4,1) vc=(3,1) | BUF+CUR |
| `ex:1,2t$` | `":1,2t$<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="a\nb\nc\na\nb" vc="a\nb\nca\nb" | nvim=(5,1) vc=(4,1) | BUF+CUR |
| `ex:1co$` | `":1co$<CR>"` | `["a", "b"]`@(1,1) | nvim="a\nb\na" vc="a\nba" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `ex:d a then "ap` | `":2d a<CR>\"ap"` | `["a", "b", "c"]`@(1,1) | nvim="a\nc\nb" vc="a\nb\nc" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `ex:d 2` | `":d 2<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="c" vc="a\nb\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:pu!` | `"yy:pu!<CR>"` | `["a", "b"]`@(1,1) | nvim="a\na\nb" vc="a\nb" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:2put` | `"yy:2put<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="a\nb\na\nc" vc="a\nb\nc" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `ex:0put` | `"yy:0put<CR>"` | `["a", "b"]`@(2,1) | nvim="b\na\nb" vc="a\nb" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `ex:j` | `":j<CR>"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `ex:1,3j` | `":1,3j<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="a b c" vc="a\nb\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:j!` | `":j!<CR>"` | `["a", "  b"]`@(1,1) | nvim="a  b" vc="a\n  b" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:j 3` | `":j 3<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a b c\nd" vc="a\nb\nc\nd" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:>` | `":><CR>"` | `["a"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `ex:>>` | `":>><CR>"` | `["a"]`@(1,1) | nvim="        a" vc="a" | nvim=(1,9) vc=(1,1) | BUF+CUR |
| `ex:2,3>` | `":2,3><CR>"` | `["a", "b", "c"]`@(1,1) | nvim="a\n    b\n    c" vc="a\nb\nc" | nvim=(3,5) vc=(1,1) | BUF+CUR |
| `ex:<` | `":<<CR>"` | `["        a"]`@(1,1) | (buffer identical) | nvim=(1,5) vc=(1,1) | CUR |
| `ex:> 2` | `":> 2<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="    a\n    b\nc" vc="a\nb\nc" | nvim=(2,5) vc=(1,1) | BUF+CUR |
| `ex:2,3sort` | `":2,3sort<CR>"` | `["c", "b", "a"]`@(1,1) | nvim="c\na\nb" vc="c\nb\na" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:sort /pat/ r` | `":sort /\\d/ r<CR>"` | `["b 2", "a 1"]`@(1,1) | nvim="a 1\nb 2" vc="b 2\na 1" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:retab` | `":set ts=4<CR>:retab<CR>"` | `["\ta"]`@(1,1) | (buffer identical) | nvim=(1,4) vc=(1,1) | CUR |
| `ex:retab!` | `":set noet ts=4<CR>:retab!<CR>"` | `["    a"]`@(1,1) | nvim="\ta" vc="    a" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:retab 2` | `":set ts=4<CR>:retab 2<CR>"` | `["\ta"]`@(1,1) | nvim="    a" vc="  a" | nvim=(1,4) vc=(1,1) | BUF+CUR |
| `ex:/foo/` | `":/foo/<CR>"` | `["a", "b", "foo"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `ex:?a?` | `":?a?<CR>"` | `["a", "b", "c"]`@(3,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `ex:/foo/+1` | `":/foo/+1<CR>"` | `["a", "foo", "b"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `ex:/foo/d` | `":/foo/d<CR>"` | `["a", "foo", "b"]`@(1,1) | nvim="a\nb" vc="a\nfoo\nb" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:/a/,/b/d` | `":/a/,/b/d<CR>"` | `["x", "a", "y", "b", "z"]`@(1,1) | nvim="x\nz" vc="x\na\ny\nb\nz" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:.,/foo/d` | `":.,/foo/d<CR>"` | `["a", "b", "foo", "c"]`@(1,1) | nvim="c" vc="a\nb\nfoo\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:%j` | `":%j<CR>"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,4) | CUR |
| `ex:2ka 'a` | `":2ka<CR>'a"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `ex:2mark a` | `":2mark a<CR>'a"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `ex:le` | `":le<CR>"` | `["    a"]`@(1,1) | nvim="a" vc="    a" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:le 4` | `":le 4<CR>"` | `["a"]`@(1,1) | nvim="    a" vc="a" | nvim=(1,5) vc=(1,1) | BUF+CUR |
| `ex:ri 10` | `":ri 10<CR>"` | `["a"]`@(1,1) | nvim="         a" vc="a" | nvim=(1,10) vc=(1,1) | BUF+CUR |
| `ex:ce 10` | `":ce 10<CR>"` | `["a"]`@(1,1) | nvim="    a" vc="a" | nvim=(1,5) vc=(1,1) | BUF+CUR |
| `ex:normal Ax` | `":normal Ax<CR>"` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,3) | CUR |
| `ex:%normal Ax` | `":%normal Ax<CR>"` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(2,2) vc=(2,3) | CUR |
| `ex:2,3normal I-` | `":2,3normal I-<CR>"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(3,2) | CUR |
| `ex:normal! Ax` | `":normal! Ax<CR>"` | `["a"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,3) | CUR |
| `ex:normal cursor` | `":2normal $<CR>"` | `["abc", "def"]`@(1,1) | (buffer identical) | nvim=(2,3) vc=(1,1) | CUR |
| `ex:r !echo` | `":r !echo hi<CR>"` | `["a"]`@(1,1) | nvim="a\nhi" vc="a" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:2;+1d` | `":2;+1d<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a\nd" vc="a\nb\nc\nd" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:2,+1d` | `":2,+1d<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a\nc\nd" vc="a\nb\nc\nd" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:$-1d` | `":$-1d<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="a\nc" vc="a\nb\nc" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:.+2` | `":.+2<CR>"` | `["1", "2", "3", "4"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `ex:cursor after :t$` | `":t$<CR>"` | `["a", "b"]`@(1,1) | nvim="a\nb\na" vc="a\nba" | nvim=(3,1) vc=(2,1) | BUF+CUR |
| `ex:cursor after :>` | `":><CR>"` | `["  a"]`@(1,2) | (buffer identical) | nvim=(1,7) vc=(1,2) | CUR |
| `ex:cursor after :j` | `":j<CR>"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `ex:cursor after :%normal` | `":%normal Ax<CR>"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(3,2) vc=(3,3) | CUR |
| `ex:cursor after :g/d` | `":g/a/d<CR>"` | `["a", "b", "a", "c"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `ex:d _` | `"yyj:d _<CR>p"` | `["a", "b"]`@(1,1) | nvim="a\na" vc="a\nb\na" | nvim=(2,1) vc=(3,1) | BUF+CUR |
| `ex:y A append` | `":y a<CR>j:y A<CR>\"ap"` | `["a", "b"]`@(1,1) | nvim="a\nb\na\nb" vc="a\nb\na" | nvim=(3,1) vc=(3,1) | BUF |
| `ex:.,.+1d` | `":.,.+1d<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="c" vc="a\nb\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `ex:'<,'>d after v` | `"vj<Esc>:'<,'>d<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="c" vc="a\nb\nc" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `ex:*d after visual` | `"Vj<Esc>:*d<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="c" vc="a\nb\nc" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `ex:s sets last search for n` | `":s/a/x/<CR>n"` | `["a", "b", "a"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(1,1) | CUR |
| `ex:s cursor col` | `":s/a/b/<CR>"` | `["xx a"]`@(1,4) | (buffer identical) | nvim=(1,1) vc=(1,4) | CUR |
| `ex:%s/x/y/g with \r cursor` | `":s/,/\\r/g<CR>"` | `["a,b,c"]`@(1,1) | nvim="a\nb\nc" vc="a\\rb\\rc" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `ex:2>3? shift count` | `":2> 2<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="a\n    b\n    c\nd" vc="a\nb\nc\nd" | nvim=(3,5) vc=(1,1) | BUF+CUR |
| `ex:< 2` | `":< 2<CR>"` | `["    a", "    b", "    c"]`@(1,1) | nvim="a\nb\n    c" vc="    a\n    b\n    c" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:>>> 3 levels` | `":>>><CR>"` | `["a"]`@(1,1) | nvim="            a" vc="a" | nvim=(1,13) vc=(1,1) | BUF+CUR |
| `ex:j with range and count` | `":2j 3<CR>"` | `["a", "b", "c", "d", "e"]`@(1,1) | nvim="a\nb c d\ne" vc="a\nb\nc\nd\ne" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `ex:t with range dest .` | `":1,2t.<CR>"` | `["a", "b", "c"]`@(3,1) | nvim="a\nb\nc\na\nb" vc="a\nb\nca\nb" | nvim=(5,1) vc=(4,1) | BUF+CUR |
| `ins:C-w at line start joins` | `"i<C-w><Esc>"` | `["ab", "cd"]`@(2,1) | nvim="abcd" vc="ab\ncd" | nvim=(1,2) vc=(2,1) | BUF+CUR |
| `ins:C-w punctuation` | `"A<C-w><Esc>"` | `["foo.bar"]`@(1,8) | nvim="foo." vc="" | nvim=(1,4) vc=(1,1) | BUF+CUR |
| `ins:C-u before start` | `"A<C-u><Esc>"` | `["ab"]`@(1,1) | nvim="" vc="ab" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `ins:C-u twice` | `"Afoo<C-u><C-u><Esc>"` | `["ab"]`@(1,1) | nvim="" vc="ab" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `ins:C-u with indent` | `"A<C-u><Esc>"` | `["    ab"]`@(1,1) | nvim="    " vc="    ab" | nvim=(1,4) vc=(1,6) | BUF+CUR |
| `ins:BS over indent (nvim smarttab)` | `"i<BS><Esc>"` | `["    a"]`@(1,5) | nvim="a" vc="   a" | nvim=(1,1) vc=(1,3) | BUF+CUR |
| `ins:BS mid indent` | `"i<BS><Esc>"` | `["      a"]`@(1,4) | nvim="   a" vc="     a" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `ins:Tab at start (smarttab)` | `":set ts=8<CR>i<Tab><Esc>"` | `["x"]`@(1,1) | nvim="    x" vc="        x" | nvim=(1,4) vc=(1,8) | BUF+CUR |
| `ins:Tab mid line ts4` | `"A<Tab>x<Esc>"` | `["a"]`@(1,1) | nvim="a   x" vc="a    x" | nvim=(1,5) vc=(1,6) | BUF+CUR |
| `ins:Tab mid line ts8` | `":set ts=8<CR>A<Tab>x<Esc>"` | `["a"]`@(1,1) | nvim="a       x" vc="a        x" | nvim=(1,9) vc=(1,10) | BUF+CUR |
| `ins:Tab after 2 chars ts4` | `"A<Tab>x<Esc>"` | `["ab"]`@(1,1) | nvim="ab  x" vc="ab    x" | nvim=(1,5) vc=(1,7) | BUF+CUR |
| `ins:0 C-d` | `"A0<C-d><Esc>"` | `["    a"]`@(1,1) | nvim="a" vc="a0" | nvim=(1,1) vc=(1,2) | BUF+CUR |
| `ins:C-o $ then type` | `"i<C-o>$x<Esc>"` | `["foo"]`@(1,1) | nvim="foox" vc="foxo" | nvim=(1,4) vc=(1,3) | BUF+CUR |
| `ins:A C-o h` | `"A<C-o>hx<Esc>"` | `["foo"]`@(1,1) | nvim="fxoo" vc="foxo" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `ins:C-o with count` | `"i<C-o>2wx<Esc>"` | `["a b c d"]`@(1,1) | nvim="a b xc d" vc="wxa b c d" | nvim=(1,5) vc=(1,2) | BUF+CUR |
| `ins:C-o p` | `"yli<C-o>p<Esc>"` | `["ab"]`@(1,1) | (buffer identical) | nvim=(1,2) vc=(1,1) | CUR |
| `ins:C-o :s` | `"A<C-o>:s/a/b/<CR>x<Esc>"` | `["a a"]`@(1,1) | nvim="xb a" vc="b a" | nvim=(1,1) vc=(1,4) | BUF+CUR |
| `ins:C-v 065` | `"i<C-v>065<Esc>"` | `["a"]`@(1,1) | nvim="Aa" vc="065a" | nvim=(1,1) vc=(1,3) | BUF+CUR |
| `ins:C-v x41` | `"i<C-v>x41<Esc>"` | `["a"]`@(1,1) | nvim="Aa" vc="x41a" | nvim=(1,1) vc=(1,3) | BUF+CUR |
| `ins:CR mid-line` | `"i<CR><Esc>"` | `["foo bar"]`@(1,4) | nvim="foo\nbar" vc="foo\n bar" | nvim=(2,1) vc=(2,1) | BUF |
| `ins:CR on indented mid` | `"i<CR><Esc>"` | `["  foo bar"]`@(1,6) | nvim="  foo\n  bar" vc="  foo\n   bar" | nvim=(2,2) vc=(2,2) | BUF |
| `ins:CR then Esc removes autoindent` | `"A<CR><Esc>"` | `["    foo"]`@(1,8) | nvim="    foo\n" vc="    foo\n    " | nvim=(2,1) vc=(2,4) | BUF+CUR |
| `ins:CR CR keeps prev line empty` | `"A<CR><CR>x<Esc>"` | `["    foo"]`@(1,8) | nvim="    foo\n\n    x" vc="    foo\n    \n    x" | nvim=(3,5) vc=(3,5) | BUF |
| `ins:Down Down col memory` | `"i<Down><Down>X<Esc>"` | `["abcdef", "ab", "abcdef"]`@(1,5) | nvim="abcdef\nab\nabcdXef" vc="abcdef\nab\nabXcdef" | nvim=(3,5) vc=(3,3) | BUF+CUR |
| `ins:C-h as BS` | `"a<C-h><Esc>"` | `["abc"]`@(1,3) | nvim="ab" vc="abch" | nvim=(1,2) vc=(1,4) | BUF+CUR |
| `ins:C-j newline` | `"a<C-j><Esc>"` | `["ab"]`@(1,2) | nvim="ab\n" vc="abj" | nvim=(2,1) vc=(1,3) | BUF+CUR |
| `ins:( no autopair` | `"i(<Esc>"` | `["a"]`@(1,1) | nvim="(a" vc="()a" | nvim=(1,1) vc=(1,1) | BUF |
| `ins:" no autopair` | `"i\"<Esc>"` | `["a"]`@(1,1) | nvim="\"a" vc="\"\"a" | nvim=(1,1) vc=(1,1) | BUF |
| `ins:{ CR no autopair` | `"i{<CR><Esc>"` | `["a"]`@(1,1) | nvim="{\na" vc="{\n}a" | nvim=(2,1) vc=(2,1) | BUF |
| `ins:[ no autopair` | `"A[<Esc>"` | `["a"]`@(1,1) | nvim="a[" vc="a[]" | nvim=(1,2) vc=(1,2) | BUF |
| `ins:C-p completion` | `"A<C-p><Esc>"` | `["foo", "fob", "f"]`@(3,1) | nvim="foo\nfob\nfob" vc="foo\nfob\nfoo" | nvim=(3,3) vc=(3,3) | BUF |
| `ins:typing prefix then Tab` | `"ofo<Tab>x<Esc>"` | `["foo bar"]`@(1,1) | nvim="foo bar\nfo  x" vc="foo bar\nfoox" | nvim=(2,5) vc=(2,4) | BUF+CUR |
| `ins:C-v u00e9` | `"i<C-v>u00e9<Esc>"` | `["a"]`@(1,1) | nvim="éa" vc="u00e9a" | nvim=(1,1) vc=(1,5) | BUF+CUR |
| `ins:insert Tab then BS (sts)` | `"A<Tab><BS>x<Esc>"` | `["a"]`@(1,1) | nvim="a  x" vc="a   x" | nvim=(1,4) vc=(1,5) | BUF+CUR |
| `ins:i with count and Esc cursor` | `"2ix<Esc>"` | `["abc"]`@(1,2) | nvim="axxbc" vc="axbc" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `ins:A with count and CR` | `"2Ax<CR><Esc>"` | `["a"]`@(1,1) | nvim="ax\nx\n" vc="ax" | nvim=(3,1) vc=(2,1) | BUF |
| `ins:C-r register linewise mid line` | `"yyjli<C-r>\"<Esc>"` | `["a", "bc"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(2,2) | CUR |
| `ins:C-r with tab in register` | `"yyjA<C-r>\"<Esc>"` | `["a\tb", "c"]`@(1,1) | nvim="a\tb\nca  b\n" vc="a\tb\nca\tb" | nvim=(3,1) vc=(2,4) | BUF+CUR |
| `ins:BS join with autoindent` | `"i<BS><BS><BS><BS><BS><Esc>"` | `["a", "    b"]`@(2,5) | nvim="b" vc="ab" | nvim=(1,1) vc=(1,1) | BUF |
| `ins:i then Esc then . twice` | `"ix<Esc>.."` | `["a"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,3) | CUR |
| `vis:Vjd` | `"Vjd"` | `["abc", "def", "ghi"]`@(1,2) | (buffer identical) | nvim=(1,2) vc=(1,1) | CUR |
| `vis:v$y p` | `"v$yjp"` | `["abc", "def"]`@(1,1) | (buffer identical) | nvim=(2,2) vc=(2,5) | CUR |
| `vis:gv after Vjd` | `"Vjdgvd"` | `["a", "b", "c", "d", "e"]`@(1,1) | nvim="d\ne" vc="e" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:vggd` | `"vggd"` | `["abc", "def"]`@(2,2) | nvim="af" vc="f" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `vis:v^d` | `"v^d"` | `["   abc"]`@(1,6) | nvim="   " vc="   ab" | nvim=(1,3) vc=(1,5) | BUF+CUR |
| `vis:Vr-` | `"Vr-"` | `["abc"]`@(1,2) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `vis:vjD` | `"vjD"` | `["abc", "def", "ghi"]`@(1,2) | nvim="ghi" vc="abc\ndef\nghi" | nvim=(1,2) vc=(2,2) | BUF+CUR |
| `vis:vjX` | `"vjX"` | `["abc", "def", "ghi"]`@(1,2) | nvim="ghi" vc="abc\ndef\nghi" | nvim=(1,2) vc=(2,2) | BUF+CUR |
| `vis:vjY p` | `"vjYGp"` | `["abc", "def", "ghi"]`@(1,2) | nvim="abc\ndef\nghi\nabc\ndef" vc="ai" | nvim=(4,1) vc=(1,2) | BUF+CUR |
| `vis:vjC` | `"vjCX<Esc>"` | `["abc", "def", "ghi"]`@(1,2) | nvim="X\nghi" vc="abc\ndef\nghi" | nvim=(1,1) vc=(2,2) | BUF+CUR |
| `vis:vjS` | `"vjSX<Esc>"` | `["abc", "def", "ghi"]`@(1,2) | nvim="X\nghi" vc="abc\ndef\nghi" | nvim=(1,1) vc=(2,2) | BUF+CUR |
| `vis:vjR` | `"vjRX<Esc>"` | `["abc", "def", "ghi"]`@(1,2) | nvim="X\nghi" vc="abc\ndef\nghi" | nvim=(1,1) vc=(2,2) | BUF+CUR |
| `vis:vlp linewise reg` | `"yyjjvlp"` | `["a", "b", "xyz"]`@(1,1) | nvim="a\nb\n\na\nz" vc="a\nb\na\nz" | nvim=(4,1) vc=(3,1) | BUF+CUR |
| `vis:v3iwd` | `"v3iwd"` | `["a b c d"]`@(1,1) | nvim=" c d" vc=" b c d" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:v2awd` | `"v2awd"` | `["a b c d"]`@(1,1) | nvim="c d" vc="b c d" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:Vj:normal` | `"Vj:normal Ax<CR>"` | `["a", "b", "c"]`@(1,1) | (buffer identical) | nvim=(2,2) vc=(2,3) | CUR |
| `vis:Vj=` | `"Vj="` | `["  a", "    b"]`@(1,1) | nvim="a\nb" vc="  a\n    b" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `vis:Vd cursor` | `"Vd"` | `["  a", "  b"]`@(1,3) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `vis:viwd on whitespace` | `"viwd"` | `["a   b"]`@(1,2) | nvim="ab" vc="a  b" | nvim=(1,2) vc=(1,2) | BUF |
| `vis:vjy then P` | `"vjyP"` | `["abc", "def"]`@(1,2) | (buffer identical) | nvim=(1,2) vc=(1,6) | CUR |
| `vis:V3>` | `"V3>"` | `["a"]`@(1,1) | nvim="            a" vc="    a" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:vip then ip extends` | `"vipipd"` | `["a", "", "b", "", "c"]`@(1,1) | nvim="b\n\nc" vc="\nb\n\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:vf,d` | `"vf,d"` | `["a,b,c"]`@(1,1) | nvim="b,c" vc=",b,c" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:v/pat d` | `"v/baz<CR>d"` | `["foo bar baz"]`@(1,1) | nvim="az" vc="oo bar baz" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:vnd` | `"/foo<CR>vnd"` | `["foo x foo y foo"]`@(1,1) | nvim="foo x oo" vc="oo x foo y foo" | nvim=(1,7) vc=(1,1) | BUF+CUR |
| `vis:v'a? mark d` | `"magg0v`ad"` | `["abc", "def"]`@(2,2) | nvim="f" vc="abc\ndef" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:vjc then u` | `"vjcX<Esc>u"` | `["abc", "def"]`@(1,2) | nvim="abc\ndef" vc="af" | nvim=(1,2) vc=(1,2) | BUF |
| `vis:vjgq` | `":set tw=10<CR>Vjgq"` | `["one two three four five six", "seven"]`@(1,1) | (buffer identical) | nvim=(4,1) vc=(1,1) | CUR |
| `vis:vjy "0 then p` | `"vjy\"0P"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,4) | CUR |
| `vis:V G d cursor` | `"VGd"` | `["a", "b", "c"]`@(2,1) | (buffer identical) | nvim=(1,1) vc=(2,1) | CUR |
| `vis:v s` | `"vlsX<Esc>"` | `["abcd"]`@(1,2) | nvim="aXd" vc="abcd" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `vis:vjy count 2p` | `"vjy$2p"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,10) | CUR |
| `vis:v then gv toggles` | `"vl<Esc>$vgvd"` | `["abcdef"]`@(1,1) | nvim="cdef" vc="abcdef" | nvim=(1,1) vc=(1,6) | BUF+CUR |
| `vis:v_gJ` | `"VjgJ"` | `["a", "  b", "c"]`@(1,1) | nvim="a  b\nc" vc="a\n  b\nc" | nvim=(1,2) vc=(2,1) | BUF+CUR |
| `vis:v_r CR` | `"vr<CR>"` | `["abc"]`@(1,2) | nvim="a\rc" vc="abc" | nvim=(1,2) vc=(1,2) | BUF |
| `vis:v ip on blank` | `"vipd"` | `["a", "", "", "b"]`@(2,1) | nvim="a\nb" vc="a" | nvim=(2,1) vc=(2,1) | BUF |
| `vis:v ap trailing` | `"vapd"` | `["a", "", "b", "c"]`@(3,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `vis:vjd then p` | `"vjdp"` | `["abc", "def", "ghi"]`@(1,2) | (buffer identical) | nvim=(1,3) vc=(1,7) | CUR |
| `vis:Vjy then p count` | `"Vjy2p"` | `["a", "b"]`@(1,1) | nvim="a\na\nb\na\nb\nb" vc="a\na\na\nb\nb\nb" | nvim=(2,1) vc=(3,1) | BUF+CUR |
| `vis:vip on last para no trailing` | `"vipd"` | `["a", "", "b", "c"]`@(4,1) | nvim="a\n" vc="a\n\nb" | nvim=(2,1) vc=(4,1) | BUF+CUR |
| `vis:v then < count` | `"V2<"` | `["        a"]`@(1,1) | nvim="a" vc="    a" | nvim=(1,1) vc=(1,1) | BUF |
| `vis:v with $ then j then y p` | `"v$jyGp"` | `["ab", "abcd", "abc"]`@(1,1) | (buffer identical) | nvim=(3,2) vc=(3,9) | CUR |
| `vb:jjAx` | `"<C-v>jjAx<Esc>"` | `["abc", "def", "ghi"]`@(1,2) | (buffer identical) | nvim=(1,2) vc=(1,3) | CUR |
| `vb:jj$Ax` | `"<C-v>jj$Ax<Esc>"` | `["ab", "abcd", "a"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,3) | CUR |
| `vb:jlcX` | `"<C-v>jlcX<Esc>"` | `["abc", "def"]`@(1,2) | nvim="aX\ndX" vc="Xa\nd" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `vb:j>` | `"<C-v>j>"` | `["abc", "def"]`@(1,2) | nvim="a    bc\nd    ef" vc="    abc\n    def" | nvim=(1,2) vc=(1,2) | BUF |
| `vb:jy then Gp` | `"<C-v>jyGp"` | `["ab", "cd", "", "xy"]`@(1,1) | nvim="ab\ncd\n\nxay\n c" vc="ab\ncd\n\nxa\ncy" | nvim=(4,2) vc=(4,4) | BUF+CUR |
| `vb:jy then P` | `"<C-v>jy$P"` | `["ab", "cd"]`@(1,1) | nvim="aab\nccd" vc="aa\ncb\ncd" | nvim=(1,2) vc=(1,4) | BUF+CUR |
| `vb:ragged d` | `"<C-v>jjlld"` | `["abcdef", "ab", "abcdef"]`@(1,3) | nvim="abf\nab\nabf" vc="abef\nababef" | nvim=(1,3) vc=(1,3) | BUF |
| `vb:I on short line skipped` | `"<C-v>jjIx<Esc>"` | `["abcdef", "ab", "abcdef"]`@(1,4) | nvim="abcxdef\nab\nabcxdef" vc="axbcdef\naxb\naxbcdef" | nvim=(1,4) vc=(1,2) | BUF+CUR |
| `vb:A on short line padded` | `"<C-v>jjAx<Esc>"` | `["abcdef", "ab", "abcdef"]`@(1,4) | (buffer identical) | nvim=(1,4) vc=(1,5) | CUR |
| `vb:jj$d` | `"<C-v>jj$d"` | `["abcdef", "ab", "abcd"]`@(1,3) | nvim="ab\nab\nab" vc="abef\nabab" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `vb:jsX` | `"<C-v>jsX<Esc>"` | `["abc", "def"]`@(1,2) | nvim="aXc\ndXf" vc="abc\ndef" | nvim=(1,2) vc=(2,2) | BUF+CUR |
| `vb:jCX` | `"<C-v>jCX<Esc>"` | `["abcdef", "abcdef"]`@(1,3) | nvim="abX\nabX" vc="abcdef\nabcdef" | nvim=(1,3) vc=(2,3) | BUF+CUR |
| `vb:jD` | `"<C-v>jD"` | `["abcdef", "abcdef"]`@(1,3) | nvim="ab\nab" vc="abcdef\nabcdef" | nvim=(1,2) vc=(2,3) | BUF+CUR |
| `vb:jIx then .` | `"<C-v>jIx<Esc>jj."` | `["ab", "ab", "ab", "ab"]`@(1,1) | nvim="xab\nxab\nxab\nxab" vc="xab\nxab\nxab\nab" | nvim=(3,1) vc=(3,2) | BUF+CUR |
| `vb:A on empty middle line` | `"<C-v>jjAx<Esc>"` | `["ab", "", "ab"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `vb:$A on empty middle line` | `"<C-v>jj$Ax<Esc>"` | `["ab", "", "ab"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(1,3) | CUR |
| `vb:jjy p at eol` | `"<C-v>jjy$p"` | `["ab", "cd", "ef"]`@(1,1) | nvim="aba\ncdc\nefe" vc="aba\nc\ne\ncd\nef" | nvim=(1,3) vc=(1,7) | BUF+CUR |
| `vb:jjy p on shorter` | `"<C-v>jjyGp"` | `["abc", "abc", "x"]`@(1,2) | nvim="abc\nabc\nxb\n b\n " vc="abc\nabc\nxab\nab\nx" | nvim=(3,2) vc=(3,8) | BUF+CUR |
| `vb:jly then p` | `"<C-v>jly$p"` | `["abc", "def"]`@(1,1) | nvim="abcab\ndefde" vc="abcab\nde\ndef" | nvim=(1,4) vc=(1,8) | BUF+CUR |
| `vb:jIx with CR` | `"<C-v>jIx<CR><Esc>"` | `["ab", "ab"]`@(1,1) | nvim="x\nab\nab" vc="x\nx\nab\nab" | nvim=(2,1) vc=(2,1) | BUF |
| `vb:jc with multi chars` | `"<C-v>jlcXYZ<Esc>"` | `["abcd", "abcd"]`@(1,2) | nvim="aXYZd\naXYZd" vc="aXYZd\nad" | nvim=(1,4) vc=(1,4) | BUF |
| `vb:j< ` | `"<C-v>j<"` | `["    ab", "    ab"]`@(1,5) | nvim="    ab\n    ab" vc="ab\nab" | nvim=(1,5) vc=(1,5) | BUF |
| `vb:jr<CR>` | `"<C-v>jr<CR>"` | `["abc", "def"]`@(1,2) | nvim="a\nc\nd\nf" vc="abc\ndef" | nvim=(1,1) vc=(2,2) | BUF+CUR |
| `vb:jjAx then u` | `"<C-v>jjAx<Esc>u"` | `["ab", "ab", "ab"]`@(1,1) | nvim="ab\nab\nab" vc="axb\nab\nab" | nvim=(1,2) vc=(1,3) | BUF+CUR |
| `vb:I with count? 2I` | `"<C-v>j2Ix<Esc>"` | `["ab", "ab"]`@(1,1) | nvim="xxab\nxxab" vc="xab\nxab" | nvim=(1,1) vc=(1,1) | BUF |
| `vb:jjp block over block` | `"<C-v>jy2j<C-v>jp"` | `["ab", "cd", "ef", "gh"]`@(1,1) | nvim="ab\ncd\naf\nch" vc="ab\ncd\na\ncf\nh" | nvim=(3,1) vc=(4,1) | BUF+CUR |
| `vb:vb yank then p linewise reg? P` | `"<C-v>jyP"` | `["ab", "cd"]`@(1,1) | nvim="aab\nccd" vc="a\ncab\ncd" | nvim=(1,1) vc=(1,3) | BUF+CUR |
| `vb:d then .` | `"<C-v>jdjj."` | `["abcd", "abcd", "abcd", "abcd"]`@(1,1) | nvim="bcd\nbcd\nbcd\nbcd" vc="bcd\nbcd\nabcd\nabcd" | nvim=(3,1) vc=(3,1) | BUF |
| `vb:r then .` | `"<C-v>jlrxjj."` | `["abcd", "abcd", "abcd", "abcd"]`@(1,1) | nvim="xxcd\nxxcd\nxxcd\nxxcd" vc="xxcd\nxxcd\nabcd\nabcd" | nvim=(3,1) vc=(3,1) | BUF |
| `vb:c then .` | `"<C-v>jcX<Esc>jj."` | `["abcd", "abcd", "abcd", "abcd"]`@(1,1) | nvim="Xbcd\nXbcd\nXbcd\nXbcd" vc="Xbcd\nbcd\nXabcd\nabcd" | nvim=(3,1) vc=(3,2) | BUF+CUR |
| `vb:g C-a` | `"<C-v>jjg<C-a>"` | `["1", "1", "1"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `num:hex 0xaB` | `"<C-a>"` | `["0xaB"]`@(1,1) | nvim="0xAC" vc="0xac" | nvim=(1,4) vc=(1,4) | BUF |
| `num:hex C-x below zero` | `"<C-x>"` | `["0x0"]`@(1,1) | nvim="0xffffffffffffffff" vc="0x1" | nvim=(1,18) vc=(1,3) | BUF+CUR |
| `num:octal not default 007` | `"<C-a>"` | `["007"]`@(1,1) | nvim="008" vc="10" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `num:octal nf=octal 007` | `"<C-a>"` | `["007"]`@(1,1) | nvim="010" vc="10" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `num:binary 0b101` | `"<C-a>"` | `["0b101"]`@(1,1) | nvim="0b110" vc="1" | nvim=(1,5) vc=(1,1) | BUF+CUR |
| `num:leading zeros 009` | `"<C-a>"` | `["009"]`@(1,1) | nvim="010" vc="1" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `num:leading zeros 0099 C-x` | `"<C-x>"` | `["0099"]`@(1,1) | nvim="0098" vc="1777777777777777777777" | nvim=(1,4) vc=(1,22) | BUF+CUR |
| `num:C-a .` | `"<C-a>."` | `["x 5"]`@(1,1) | nvim="x 7" vc="x 6" | nvim=(1,3) vc=(1,3) | BUF |
| `num:3C-a .` | `"3<C-a>."` | `["x 5"]`@(1,1) | nvim="x 11" vc="x 8" | nvim=(1,4) vc=(1,3) | BUF+CUR |
| `num:3C-a 2.` | `"3<C-a>2."` | `["x 5"]`@(1,1) | nvim="x 10" vc="x 8" | nvim=(1,4) vc=(1,3) | BUF+CUR |
| `num:V C-a` | `"Vjj<C-a>"` | `["1", "1", "1"]`@(1,1) | nvim="2\n2\n2" vc="1\n1\n1" | nvim=(1,1) vc=(3,1) | BUF+CUR |
| `num:V g C-a` | `"Vjjg<C-a>"` | `["1", "1", "1"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `num:V 2g C-a` | `"Vjj2g<C-a>"` | `["1", "1", "1"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `num:v C-a partial` | `"vj<C-a>"` | `["1 1", "1 1"]`@(1,1) | nvim="2 1\n2 1" vc="1 1\n1 1" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `num:C-v block C-a` | `"<C-v>j<C-a>"` | `["1 1", "1 1"]`@(1,3) | nvim="1 2\n1 2" vc="1 1\n1 1" | nvim=(1,3) vc=(2,3) | BUF+CUR |
| `num:alpha` | `"<C-a>"` | `["a"]`@(1,1) | nvim="b" vc="a" | nvim=(1,1) vc=(1,1) | BUF |
| `num:C-a on 99999999999999999999 overflow` | `"<C-a>"` | `["99999999999999999999"]`@(1,1) | nvim="18446744073709551615" vc="1" | nvim=(1,20) vc=(1,1) | BUF+CUR |
| `num:V C-a skips lines without numbers` | `"Vjjg<C-a>"` | `["1", "x", "1"]`@(1,1) | nvim="2\nx\n3" vc="2\nx\n4" | nvim=(1,1) vc=(3,1) | BUF+CUR |
| `num:V C-a only first number per line` | `"Vj<C-a>"` | `["1 2", "3 4"]`@(1,1) | nvim="2 2\n4 4" vc="1 2\n3 4" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `num:C-a 0x with uppercase X` | `"<C-a>"` | `["0X0f"]`@(1,1) | nvim="0X10" vc="0x10" | nvim=(1,4) vc=(1,4) | BUF |
| `num:C-a on negative hex? -0x1` | `"<C-a>"` | `["-0x1"]`@(1,1) | nvim="-0x2" vc="-0x0" | nvim=(1,4) vc=(1,4) | BUF |
| `num:C-x on 0 leading zeros 000` | `"<C-x>"` | `["000"]`@(1,1) | nvim="-001" vc="1777777777777777777777" | nvim=(1,4) vc=(1,22) | BUF+CUR |
| `num:V C-a cursor` | `"Vj<C-a>"` | `["1", "1"]`@(1,1) | nvim="2\n2" vc="1\n1" | nvim=(1,1) vc=(2,1) | BUF+CUR |
| `num:v C-a on -5 in visual (no minus)` | `"vl<C-a>"` | `["x -5"]`@(1,4) | nvim="x -6" vc="x -5" | nvim=(1,4) vc=(1,4) | BUF |
| `scroll:C-f` | `"<C-f>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(21,1) vc=(1,1) | CUR |
| `scroll:C-b` | `"<C-b>"` | `LONG(60 lines)`@(60,1) | (buffer identical) | nvim=(40,1) vc=(38,1) | CUR |
| `scroll:M from 30` | `"M"` | `LONG(60 lines)`@(30,1) | (buffer identical) | nvim=(19,1) vc=(20,1) | CUR |
| `scroll:zzH` | `"zzH"` | `LONG(60 lines)`@(20,1) | (buffer identical) | nvim=(10,1) vc=(9,1) | CUR |
| `scroll:z.H` | `"z.H"` | `LONG(60 lines)`@(20,1) | (buffer identical) | nvim=(10,1) vc=(9,1) | CUR |
| `scroll:5C-d C-d` | `"5<C-d><C-d>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(11,1) vc=(60,1) | CUR |
| `scroll:C-f C-f` | `"<C-f><C-f>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(41,1) vc=(1,1) | CUR |
| `scroll:C-f C-b` | `"<C-f><C-b>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(22,1) vc=(1,1) | CUR |
| `scroll:G M` | `"GM"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(49,1) vc=(50,1) | CUR |
| `scroll:dM` | `"dM"` | `LONG(60 lines)`@(1,1) | nvim="L12 l\nL13 m\nL14 n\nL15 o\nL16 p\nL17 q\nL18 r\nL19 s\n... vc="L13 m\nL14 n\nL15 o\nL16 p\nL17 q\nL18 r\nL19 s\nL20 t\n... | nvim=(1,1) vc=(1,1) | BUF |
| `scroll:C-d col sol` | `"<C-d>"` | `LONG(60 lines)`@(1,3) | (buffer identical) | nvim=(12,1) vc=(12,3) | CUR |
| `scroll:so=5 30G H` | `":set so=5<CR>30GH"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(19,1) vc=(14,1) | CUR |
| `scroll:so=5 30G L` | `":set so=5<CR>30GL"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(30,1) vc=(35,1) | CUR |
| `scroll:so=5 C-e` | `":set so=5<CR><C-e>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(7,1) vc=(2,1) | CUR |
| `scroll:C-d then H L` | `"<C-d>H"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(12,1) vc=(1,1) | CUR |
| `scroll:C-d then L` | `"<C-d>L"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(33,1) vc=(22,1) | CUR |
| `scroll:C-f then H` | `"<C-f>H"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(21,1) vc=(1,1) | CUR |
| `scroll:C-f then L` | `"<C-f>L"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(42,1) vc=(1,1) | CUR |
| `scroll:C-b after G then H` | `"<C-b>H"` | `LONG(60 lines)`@(60,1) | (buffer identical) | nvim=(19,1) vc=(38,1) | CUR |
| `scroll:C-b after G then L` | `"<C-b>L"` | `LONG(60 lines)`@(60,1) | (buffer identical) | nvim=(40,1) vc=(59,1) | CUR |
| `scroll:50% H` | `"50%H"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(9,1) vc=(19,1) | CUR |
| `scroll:30G zz H L` | `"30GzzHjL"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(41,1) vc=(40,1) | CUR |
| `scroll:3C-d sets scroll then C-u` | `"3<C-d><C-u>"` | `LONG(60 lines)`@(30,1) | (buffer identical) | nvim=(30,1) vc=(49,1) | CUR |
| `scroll:3<C-f>` | `"3<C-f>"` | `LONG(60 lines)`@(1,1) | (buffer identical) | nvim=(60,1) vc=(1,1) | CUR |
| `scroll:2<C-b>` | `"2<C-b>"` | `LONG(60 lines)`@(60,1) | (buffer identical) | nvim=(22,1) vc=(16,1) | CUR |
| `scroll:M on short buffer` | `"M"` | `["a", "b", "c", "d", "e"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(5,1) | CUR |
| `scroll:C-f on short buffer` | `"<C-f>"` | `["a", "b", "c", "d", "e"]`@(1,1) | (buffer identical) | nvim=(5,1) vc=(1,1) | CUR |
| `scroll:3<C-d> on short` | `"3<C-d>"` | `["a", "b", "c", "d", "e", "f", "g", "h"]`@(1,1) | (buffer identical) | nvim=(4,1) vc=(8,1) | CUR |
| `word:w onto blank line` | `"w"` | `["foo", "", "bar"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(3,1) | CUR |
| `word:ge onto blank` | `"ge"` | `["foo", "", "bar"]`@(3,1) | (buffer identical) | nvim=(2,1) vc=(1,3) | CUR |
| `word:b onto blank line` | `"b"` | `["foo", "", "bar"]`@(3,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `word:w over multiple blank lines` | `"w"` | `["a", "", "", "b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(4,1) | CUR |
| `word:ww over multiple blank lines` | `"ww"` | `["a", "", "", "b"]`@(1,1) | (buffer identical) | nvim=(3,1) vc=(4,1) | CUR |
| `word:( para` | `"("` | `["a", "", "b"]`@(3,1) | (buffer identical) | nvim=(2,1) vc=(1,1) | CUR |
| `word:}}}` | `"}}}"` | `["a", "", "b", "", "c"]`@(1,1) | (buffer identical) | nvim=(5,1) vc=(5,2) | CUR |
| `word:}} multiple blanks` | `"}}"` | `["a", "", "", "b"]`@(1,1) | (buffer identical) | nvim=(4,1) vc=(3,1) | CUR |
| `word:} from blank` | `"}"` | `["a", "", "", "b"]`@(2,1) | (buffer identical) | nvim=(4,1) vc=(3,1) | CUR |
| `word:{ at start` | `"{"` | `["a", "b"]`@(2,1) | (buffer identical) | nvim=(1,1) vc=(1,2) | CUR |
| `word:} whitespace-only line not blank` | `"}"` | `["a", "   ", "b", ""]`@(1,1) | (buffer identical) | nvim=(4,1) vc=(2,4) | CUR |
| `word:% in quotes` | `"%"` | `["\"(\" )"]`@(1,2) | (buffer identical) | nvim=(1,2) vc=(1,5) | CUR |
| `word:<CR>` | `"<CR>"` | `["  a", "  b"]`@(1,1) | (buffer identical) | nvim=(2,3) vc=(1,1) | CUR |
| `word:2$` | `"2$"` | `["ab", "cd", "ef"]`@(1,1) | (buffer identical) | nvim=(2,2) vc=(1,2) | CUR |
| `word:$jj` | `"$jj"` | `["abcdef", "ab", "abcdef"]`@(1,1) | (buffer identical) | nvim=(3,6) vc=(3,2) | CUR |
| `word:jj col memory` | `"jj"` | `["abcdef", "ab", "abcdef"]`@(1,5) | (buffer identical) | nvim=(3,5) vc=(3,2) | CUR |
| `word:gg indented (sol)` | `"gg"` | `["  a", "b"]`@(2,1) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `word:G indented (sol)` | `"G"` | `["a", "  b"]`@(1,1) | (buffer identical) | nvim=(2,3) vc=(2,1) | CUR |
| `word:$ then j to longer` | `"$j"` | `["ab", "abcdef"]`@(1,1) | (buffer identical) | nvim=(2,6) vc=(2,2) | CUR |
| `word:dd then j col? (nosol)` | `"ddj"` | `["abcdef", "abcdef", "abcdef"]`@(1,3) | (buffer identical) | nvim=(2,3) vc=(2,1) | CUR |
| `word:5G then j col (sol)` | `"2Gj"` | `["abcdef", "abcdef", "abcdef"]`@(1,3) | (buffer identical) | nvim=(3,1) vc=(3,3) | CUR |
| `word:w over punct then blank` | `"w"` | `["a.", "", "b"]`@(1,2) | (buffer identical) | nvim=(2,1) vc=(3,1) | CUR |
| `word:cw at eol punct` | `"cwX<Esc>"` | `["a.", "b"]`@(1,2) | nvim="aX\nb" vc="aX" | nvim=(1,2) vc=(1,2) | BUF |
| `to:daw on whitespace` | `"daw"` | `["foo  bar"]`@(1,4) | nvim="foo" vc="bar" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `to:diw on whitespace` | `"diw"` | `["foo  bar"]`@(1,4) | nvim="foobar" vc="foo  bar" | nvim=(1,4) vc=(1,4) | BUF |
| `to:diw punctuation` | `"diw"` | `["foo.bar"]`@(1,4) | nvim="foobar" vc=".bar" | nvim=(1,4) vc=(1,1) | BUF+CUR |
| `to:daw punctuation` | `"daw"` | `["foo.bar"]`@(1,4) | nvim="foobar" vc=".bar" | nvim=(1,4) vc=(1,1) | BUF+CUR |
| `to:d2aw` | `"d2aw"` | `["a b c d"]`@(1,1) | nvim="c d" vc="b c d" | nvim=(1,1) vc=(1,1) | BUF |
| `to:d3iw` | `"d3iw"` | `["a b c d"]`@(1,1) | nvim=" c d" vc=" b c d" | nvim=(1,1) vc=(1,1) | BUF |
| `to:c2aw` | `"c2awX<Esc>"` | `["a b c d"]`@(1,1) | nvim="Xc d" vc="Xb c d" | nvim=(1,1) vc=(1,1) | BUF |
| `to:daw leading space only` | `"daw"` | `["  foo"]`@(1,3) | nvim="  " vc="" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `to:das last sentence` | `"das"` | `["One two.  Three four."]`@(1,12) | nvim="One two." vc="One two.  " | nvim=(1,8) vc=(1,10) | BUF+CUR |
| `to:dis on whitespace between` | `"dis"` | `["One two.  Three four."]`@(1,10) | nvim="One two.Three four." vc="One two.  " | nvim=(1,9) vc=(1,10) | BUF+CUR |
| `to:dap` | `"dap"` | `["a", "b", "", "c"]`@(1,1) | nvim="c" vc="" | nvim=(1,1) vc=(1,1) | BUF |
| `to:dap trailing no blank` | `"dap"` | `["a", "", "b", "c"]`@(3,1) | (buffer identical) | nvim=(1,1) vc=(3,1) | CUR |
| `to:dip on blank lines` | `"dip"` | `["a", "", "", "b"]`@(2,1) | nvim="a\nb" vc="a" | nvim=(2,1) vc=(2,1) | BUF |
| `to:dap on blank` | `"dap"` | `["a", "", "", "b"]`@(2,1) | (buffer identical) | nvim=(1,1) vc=(2,1) | CUR |
| `to:d2ap` | `"d2ap"` | `["a", "", "b", "", "c"]`@(1,1) | nvim="c" vc="b\n\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `to:yap cursor` | `"yap"` | `["a", "", "b", "c"]`@(3,1) | (buffer identical) | nvim=(2,1) vc=(3,1) | CUR |
| `to:yip cursor` | `"yip"` | `["a", "", "b", "c"]`@(4,1) | (buffer identical) | nvim=(3,1) vc=(4,1) | CUR |
| `to:di( on )` | `"di("` | `["f(a, b)"]`@(1,7) | nvim="f()" vc="f(a, b)" | nvim=(1,3) vc=(1,7) | BUF+CUR |
| `to:d2i(` | `"d2i("` | `["f(a, (b), c)"]`@(1,7) | nvim="f()" vc="f(a, (), c)" | nvim=(1,3) vc=(1,7) | BUF+CUR |
| `to:di( before paren same line` | `"di("` | `["x f(a)"]`@(1,1) | nvim="x f()" vc="x f(a)" | nvim=(1,5) vc=(1,1) | BUF+CUR |
| `to:ci{ multiline` | `"ci{X<Esc>"` | `["{", "  a", "  b", "}"]`@(2,3) | nvim="{\n  X\n}" vc="{\nX}" | nvim=(2,3) vc=(2,1) | BUF+CUR |
| `to:yi{ cursor multiline` | `"yi{"` | `["{", "  a", "  b", "}"]`@(3,3) | (buffer identical) | nvim=(2,1) vc=(3,3) | CUR |
| `to:di" on closing` | `"di\""` | `["x \"ab\" y"]`@(1,6) | nvim="x \"\" y" vc="x \"ab\" y" | nvim=(1,4) vc=(1,6) | BUF+CUR |
| `to:di" before quotes` | `"di\""` | `["x \"ab\" y"]`@(1,1) | nvim="x \"\" y" vc="x \"ab\" y" | nvim=(1,4) vc=(1,1) | BUF+CUR |
| `to:da" before quotes` | `"da\""` | `["x \"ab\" y"]`@(1,1) | nvim="x y" vc="x \"ab\" y" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `to:yi" cursor` | `"yi\""` | `["x \"ab\" y"]`@(1,5) | (buffer identical) | nvim=(1,4) vc=(1,5) | CUR |
| `to:d2it` | `"d2it"` | `["<a><b>x</b></a>"]`@(1,7) | nvim="<a></a>" vc="<a><b></b></a>" | nvim=(1,4) vc=(1,7) | BUF+CUR |
| `to:d5aw too many` | `"d5aw"` | `["a b"]`@(1,1) | nvim="a b" vc="b" | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `to:daw leading whitespace` | `"daw"` | `["  foo bar"]`@(1,1) | nvim=" bar" vc="foo bar" | nvim=(1,1) vc=(1,1) | BUF |
| `to:ciw on whitespace` | `"ciwX<Esc>"` | `["a   b"]`@(1,3) | nvim="aXb" vc="a  b" | nvim=(1,2) vc=(1,2) | BUF |
| `to:cip` | `"cipX<Esc>"` | `["a", "b", "", "c"]`@(1,1) | nvim="X\n\nc" vc="X\nc" | nvim=(1,1) vc=(1,1) | BUF |
| `to:di( across lines` | `"di("` | `["f(a,", "  b)"]`@(1,3) | nvim="f()" vc="f(a,\n  b)" | nvim=(1,3) vc=(1,3) | BUF |
| `to:yi( cursor` | `"yi("` | `["f(a, b)"]`@(1,5) | (buffer identical) | nvim=(1,3) vc=(1,5) | CUR |
| `to:ya( cursor` | `"ya("` | `["f(a, b)"]`@(1,5) | (buffer identical) | nvim=(1,2) vc=(1,5) | CUR |
| `to:2daw` | `"2daw"` | `["a b c d"]`@(1,1) | nvim="c d" vc="b c d" | nvim=(1,1) vc=(1,1) | BUF |
| `to:daw at end with count` | `"d2aw"` | `["a b c"]`@(1,5) | nvim="a b c" vc="a b" | nvim=(1,5) vc=(1,3) | BUF+CUR |
| `to:di< nested` | `"di<"` | `["<a<b>c>"]`@(1,5) | nvim="<a<>c>" vc="<>" | nvim=(1,4) vc=(1,2) | BUF+CUR |
| `to:di( empty` | `"di("` | `["f()"]`@(1,2) | (buffer identical) | nvim=(1,3) vc=(1,2) | CUR |
| `to:di( with newline after open` | `"di("` | `["f(", "a", "b)"]`@(2,1) | nvim="f(\n)" vc="f(\nb)" | nvim=(2,1) vc=(2,1) | BUF |
| `to:diw at eol on space` | `"diw"` | `["foo "]`@(1,4) | nvim="foo" vc="foo " | nvim=(1,3) vc=(1,4) | BUF+CUR |
| `to:daw on multi spaces at start` | `"daw"` | `["   foo"]`@(1,1) | nvim="" vc="foo" | nvim=(1,1) vc=(1,1) | BUF |
| `to:>ap` | `">ap"` | `["a", "b", "", "c"]`@(1,1) | nvim="    a\n    b\n\nc" vc="    a\n    b\n    \n    c" | nvim=(1,1) vc=(1,1) | BUF |
| `to:ci' then .` | `"ci'X<Esc>4l."` | `["'a' 'b'"]`@(1,2) | nvim="'X' 'X'" vc="'X' 'Xb'" | nvim=(1,6) vc=(1,7) | BUF+CUR |
| `to:di( count 3 too many` | `"d3i("` | `["(a)"]`@(1,2) | nvim="(a)" vc="()" | nvim=(1,2) vc=(1,2) | BUF |
| `to:daw on only whitespace line` | `"daw"` | `["   "]`@(1,2) | nvim="   " vc=" " | nvim=(1,3) vc=(1,1) | BUF+CUR |
| `to:dap cursor after` | `"dap"` | `["a", "", "b", "c", "", "d"]`@(3,1) | nvim="a\n\nd" vc="a" | nvim=(3,1) vc=(3,1) | BUF |
| `misc:yyP cursor` | `"yyP"` | `["  a", "b"]`@(1,3) | (buffer identical) | nvim=(1,3) vc=(1,1) | CUR |
| `misc:p multi-line charwise cursor` | `"vjy$p"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,3) vc=(1,6) | CUR |
| `misc:gp charwise multi` | `"vjy$gp"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(2,2) | CUR |
| `misc:2gp` | `"yy2gp"` | `["a", "b"]`@(1,1) | nvim="a\na\na\nb" vc="a\na\nb\na" | nvim=(4,1) vc=(4,1) | BUF |
| `misc:p with count linewise cursor` | `"yy3p"` | `["a", "b"]`@(1,1) | (buffer identical) | nvim=(2,1) vc=(4,1) | CUR |
| `misc:P count` | `"yl3P"` | `["ab"]`@(1,2) | (buffer identical) | nvim=(1,4) vc=(1,2) | CUR |
| `misc:count on i then esc col` | `"3ix<Esc>"` | `["abc"]`@(1,3) | nvim="abxxxc" vc="abxc" | nvim=(1,5) vc=(1,3) | BUF+CUR |
| `misc:r Tab` | `"r<Tab>"` | `["ab"]`@(1,1) | nvim="    b" vc="ab" | nvim=(1,4) vc=(1,1) | BUF+CUR |
| `misc:R at eol then BS` | `"Rxyz<BS><BS><BS><BS><Esc>"` | `["ab"]`@(1,2) | nvim="ab" vc="axyz" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:cw on space at eol` | `"cwX<Esc>"` | `["ab "]`@(1,3) | nvim="abX" vc="a " | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `misc:C then .` | `"CX<Esc>j0."` | `["abc", "def"]`@(1,2) | nvim="aX\nX" vc="aX\nXdef" | nvim=(2,1) vc=(2,2) | BUF+CUR |
| `misc:cc then p` | `"ccX<Esc>jp"` | `["  a", "b"]`@(1,1) | nvim="  X\nb\n  a" vc="X\nb  a" | nvim=(3,3) vc=(2,4) | BUF+CUR |
| `misc:S then P` | `"SX<Esc>jP"` | `["a", "b"]`@(1,1) | nvim="X\na\nb" vc="X\nab" | nvim=(2,1) vc=(2,1) | BUF |
| `misc:& after &&` | `":s/a/b/g<CR>j&"` | `["a a a", "a a a"]`@(1,1) | nvim="b b b\nb a a" vc="b b b\nb b b" | nvim=(2,1) vc=(2,1) | BUF |
| `misc:g& after range` | `":1s/a/b/<CR>g&"` | `["a", "a", "a"]`@(1,1) | nvim="b\nb\nb" vc="a\na\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `misc:2d then Esc` | `"2d<Esc>x"` | `["abc"]`@(1,2) | nvim="ac" vc="a" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `misc:: with count` | `"3:d<CR>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="d" vc="b\nc\nd" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:3:s` | `"3:s/a/b/<CR>"` | `["a", "a", "a", "a"]`@(1,1) | nvim="b\nb\nb\na" vc="b\na\na\na" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `misc:gv after p` | `"yyjVpgvd"` | `["ab", "cd"]`@(1,1) | (buffer identical) | nvim=(1,1) vc=(2,1) | CUR |
| `misc:C-c in insert` | `"ix<C-c>x"` | `["abc"]`@(1,1) | nvim="abc" vc="xcxabc" | nvim=(1,1) vc=(1,4) | BUF+CUR |
| `misc:C-[ in insert` | `"ix<C-[>x"` | `["abc"]`@(1,1) | nvim="abc" vc="x[x]abc" | nvim=(1,1) vc=(1,4) | BUF+CUR |
| `misc:count then : then range` | `"2:normal Ax<CR>"` | `["a", "b", "c"]`@(1,1) | nvim="ax\nbx\nc" vc="ax\nb\nc" | nvim=(2,2) vc=(1,3) | BUF+CUR |
| `misc:c3c` | `"c3cX<Esc>"` | `["a", "b", "c", "d"]`@(1,1) | nvim="X\nd" vc="X\nb\nc\nd" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:g?g?` | `"g?g?"` | `["ab"]`@(1,1) | nvim="no" vc="ab" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:gqgq` | `":set tw=10<CR>gqgq"` | `["one two three four five six"]`@(1,1) | nvim="one two\nthree four\nfive six" vc="one two three four five six" | nvim=(3,1) vc=(1,1) | BUF+CUR |
| `misc:gwgw` | `":set tw=10<CR>gwgw"` | `["one two three four five six"]`@(1,8) | nvim="one two\nthree four\nfive six" vc="one two three four five six" | nvim=(1,7) vc=(1,8) | BUF+CUR |
| `misc:. after :g normal` | `":1,2g/./normal x<CR>G."` | `["ab", "cd", "ef"]`@(1,1) | nvim="b\nd\nf" vc="ab\ncd\nef" | nvim=(3,1) vc=(3,1) | BUF |
| `misc:xp then . ` | `"xp."` | `["abcd"]`@(1,1) | nvim="baacd" vc="bcd" | nvim=(1,3) vc=(1,2) | BUF+CUR |
| `misc:count with text object c` | `"3ciwX<Esc>"` | `["a b c d"]`@(1,1) | nvim="X c d" vc="X b c d" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:count before and after with textobj` | `"2d2aw"` | `["a b c d e f g"]`@(1,1) | nvim="e f g" vc="b c d e f g" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:2yy 3p` | `"2yy3p"` | `["a", "b"]`@(1,1) | nvim="a\na\nb\na\nb\na\nb\nb" vc="a\na\na\na\nb\nb\nb\nb" | nvim=(2,1) vc=(4,1) | BUF+CUR |
| `misc:2dd on last` | `"2dd"` | `["a", "b"]`@(2,1) | nvim="a\nb" vc="a" | nvim=(2,1) vc=(1,1) | BUF+CUR |
| `misc:cc on last line indent` | `"ccX<Esc>"` | `["a", "  b"]`@(2,3) | nvim="a\n  X" vc="a\nX" | nvim=(2,3) vc=(2,1) | BUF+CUR |
| `misc:cc with count beyond` | `"5ccX<Esc>"` | `["a", "b"]`@(1,1) | nvim="X" vc="X\nb" | nvim=(1,1) vc=(1,1) | BUF |
| `misc:count then i on line start` | `"2Ix<Esc>"` | `["ab"]`@(1,1) | nvim="xxab" vc="xab" | nvim=(1,2) vc=(1,1) | BUF+CUR |
| `misc:count then o with indent` | `"2ox<Esc>"` | `["  a"]`@(1,1) | nvim="  a\n  x\n  x" vc="  a\n  x\nx" | nvim=(3,3) vc=(3,1) | BUF+CUR |


---

## Appendix B — existing-test quality findings (from the four file inventories)

Paths relative to the repo root. "Tautological" = cannot fail against the bug it names; "permissive" = accepts both the right and the wrong answer; "STATE-only" = asserts an internal field where the observable buffer/cursor was the point.

### B.1 Tautological / no-assertion tests

| Location | Test | Why it cannot fail |
|---|---|---|
| `tests/operator_motions.rs:915` | `test_eq_G_auto_indent_file` | asserts buffer non-empty only |
| `tests/operator_motions.rs:1047` | `test_dj_at_last_line_noop_or_delete_last` | accepts both; oracle says no-op, engine deletes |
| `tests/operator_motions.rs:675`, `:690`, `:704`, `:718` | `d}` `d{` `y}` `d)` | `contains`/`starts_with` disjunctions |
| `tests/operator_motions.rs:616`, `:661`, `:853`, `:1076` | `dG` `cG` `dL` `dgg` | `starts_with` / "at minimum" |
| `tests/operator_motions.rs:39`, `:61`, `:80` | `yw…p` family | `contains("hello")` true before the paste |
| `tests/operator_motions.rs:367`, `:378`, `:390` | `p`/`P` charwise | assert `cursor.line == 0` only; `:378` named "cursor at end of pasted text" checks no column |
| `tests/normal_mode.rs:284` | `test_black_hole_register` | `contains("keep")` passes with `"_` ignored — and `"_` **is** broken (§3.6) |
| `tests/normal_mode.rs:343`, `:381`, `:430` | multi-undo, macro, dot | line counts only |
| `tests/visual_mode.rs:26` | `test_visual_block_mode` | enters and escapes only |
| `tests/visual_mode.rs:87` | `test_visual_delete_chars` | accepts inclusive **or** exclusive |
| `tests/z_commands.rs:223`, `:236`, `:247`, `:184`, `:331` | `z<CR>` `z.` `z-` `zx` fold+`j` | scroll half never asserted / no-op passes |
| `tests/vim_compat_batch.rs:127` | `test_at_colon_repeat_ex_command` | no assertion after `@:` |
| `tests/vim_compat_batch.rs:295` | `test_ctrl_w_equal_equalize` | ratio is 0.5 before and after |
| `tests/vim_compat_batch.rs:261`, `:278` | `C-w +` / `C-w -` | `assert_ne!` only; direction untested |
| `tests/vim_compat_batch.rs:97` | `test_gp_paste_after_charwise` | cannot distinguish `gp` from `p` |
| `tests/vim_compat_batch2.rs:375` | `test_gw_keeps_cursor` | `_saved_col` unused; line is invariant 0 |
| `tests/vim_compat_batch2.rs:501` | `test_gx_no_crash` | zero assertions |
| `tests/vim_compat_batch2.rs:257`, `tests/vim_compat_batch3.rs:193` | `C-w d`, `C-w T` | `len() >= before` |
| `tests/vim_compat_batch2.rs:473`, `:487`, `tests/vim_compat_batch4.rs:256` | `g'` `` g` `` | jumplist (the only difference from `'`) never read |
| `tests/vim_compat_batch3.rs:208` | `test_ctrl_w_x_exchange_windows` | result discarded (`let _ = …`) |
| `tests/vim_compat_batch3.rs:183` | `C-w h` | `mode == Normal` on one window |
| `tests/vim_compat_batch3.rs:88` | "ctrl_v_insert_literal_escape" | never sends Escape |
| `tests/vim_compat_batch3.rs:290` | `yvj` | register non-empty only (exact `"aaa\nb"` never checked) |
| `tests/vim_compat_batch3.rs:151`, `:160` | `!j`, `3!!` | freezes `1,2!` / `.,3!` (Vim: `.,.+1!`) |
| `tests/vim_compat_batch4.rs:149`, `:157` | `]/` `[/` | cursor already on the answer |
| `tests/vim_compat_batch4.rs:64`, `:39` | `gi` | mode only / no column |
| `tests/vim_compat_batch4.rs:108` | `C-w R` | 2 windows: `R` ≡ `r` |
| `tests/vim_compat_batch4.rs:215` | `y<C-v>j` | `contains('a')`; blockwise type never asserted |
| `tests/vim_compat_batch4.rs:357`, `:388`, `:409` | `g-` variants | `"bbb" \|\| "aaa"` |
| `tests/vim_compat_batch4.rs:242` | leader `gi` | mode already Normal |
| `tests/vim_features.rs:84` | `gUU` on uppercase | no-op passes |
| `tests/vim_features.rs:511`, `:542` | `g;g;`, `g,` | inequalities a no-op satisfies |
| `tests/vim_features.rs:104`, `:119` | `gn`, `cgn` | `col >= 8`, `!starts_with("foo")`; no `.` repeat (and oracle shows `cgn` targets the wrong match) |
| `tests/new_vim_features.rs:149`, `:251`, `:353`, `:365`, `:374`, `:387`, `:418` | `gf`, `==`, `:wa`, `:marks`, `:jumps`, `:changes`, `:tabmove` | header substrings / non-empty message |
| `tests/ex_commands.rs:145`, `:168`, `:318`, `:359`, `:510`, `:522`, `:563`, `:601`, `:609` | normalizer/abbrev tests | `!starts_with("Not an editor command")` |
| `tests/wincmd.rs:88`, `:99`, `:108`, `:288`, `:299`, `:310`, `:321`, `:332`, `:343`, `:354`, `:432` | resize/equalize/maximize; 7 "command recognized" | window count only / `!contains("Unknown command")` |

### B.2 STATE-only assertions where buffer/cursor was the meaningful check

`tests/ex_commands.rs:292` (`:mark`), `:504` (`:windo`), `:171`/`:533` (`:e` content never loaded), `:348` (`:update` never re-read), `:214` (`:yank` never pasted), `:236` (`:put` cursor); `tests/new_vim_features.rs:101` (`C-e` cursor clamp), `:154` (`g*` cursor), `:279` (`]p` indent); `tests/vim_features.rs:172` (`gv` extent); `tests/vim_compat_batch2.rs:151` (`ze`/`zs` cursor), `:143` (`C-^` alternate set by hand); `tests/vim_compat_batch3.rs:109` (`i_CTRL-O $` column), `:323` (`C-o dd` buffer); `tests/vim_compat_batch4.rs:177` (`do`/`dp` error path only); `tests/netrw.rs:245` (read-only `i`); `tests/z_commands.rs:60` (`zC` range).

### B.3 Command-line path

`tests/ex_commands.rs` and `tests/command_mode.rs` use `exec()` (direct `execute_command`) for all 98 tests; `run_cmd()` appears at `tests/wincmd.rs:131` and `tests/new_vim_features.rs:395` only. Nothing exercises `:`-line editing keys, history, or Esc-cancel.

### B.4 Duplicates

`:noh` ×3 (`ex_commands.rs:566`, `new_vim_features.rs:328`, `:337`); `:colo onedark` ×2 (`ex_commands.rs:16`, `:550`); `:se tabstop` ×2 (`:56`, `:543`); `g'a` ×2 (`vim_compat_batch2.rs:473`, `vim_compat_batch4.rs:256`); `gqj` ×2 (`vim_compat_batch2.rs:361`, `:528`).

### B.5 In-crate unit tests (for scale)

`src/core/engine/tests.rs`: 1,621 tests (414 `test_nvim_*` with hand-transcribed Neovim 0.12.1 values, 30 `test_matrix_*` operator×motion tables); other `src/core/` modules: 442. Predominantly buffer+cursor assertions (468 / 479 occurrences), with ~90 register and ~134 message assertions. None cover the §3 headline items.

## Appendix C — worktree state

Analysis worktree: `/home/john/src/vimcode/.claude/worktrees/agent-a0ef291fdaf7cef3b`, reset to `develop` @ `ee26268` (it had been checked out at `5105f03`, 303 commits behind, with the pre-#691 path-dep `Cargo.toml`, which is why the first baseline build failed on `vcd`). Only `tests/nvim_conformance.rs` is modified (the probe harness + 1,432 cases). Nothing committed, pushed, or filed. Raw runs and the table generator are in the scratchpad (`probe_run{1,2,3}.txt`, `compact.py`, `deviations_table.md`).
