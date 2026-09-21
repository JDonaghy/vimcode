# Pending vimcode issues — drafted, not yet filed

Vimcode worker sessions in this harness are `git`-only (no `gh` access); filing
GitHub issues, including on this repo (`JDonaghy/vimcode`) itself, is a
coordinator/human action. This file is `docs/PENDING_QUADRAUI_ISSUES.md`'s
sibling for gaps found *in this repo*, for the same reason that one exists: a
comment naming a known-but-unfixed deviation is an unfiled issue, and grep
will not find it for you.

**Coordinator/human action:** file each entry below verbatim on
`JDonaghy/vimcode`, then delete its entry here and update the citing
`KNOWN_DEVIATIONS`/code comment with the real issue number.

---

## `ensure_cursor_visible_wrap` never reads `scrolloff` (blocks `scroll:so=5 30G H`, `scroll:so=5 30G L`)

**Title:** `ensure_cursor_visible_wrap` ignores `'scrolloff'`, unlike the
no-wrap vertical-scroll path right next to it

**Body:**

`tests/nvim_conformance.rs`'s `KNOWN_DEVIATIONS` carries two entries —
`"scroll:so=5 30G H"` and `"scroll:so=5 30G L"` — added by #1280. Both cases
predate #1280 and passed on every prior run only because `run_in_vimcode`
never set `'wrap'`, so every case ran against vimcode's actual default
(`Settings::default().wrap == false`) even though the oracle's default (and
Neovim's) is `'wrap'` on. #1280 fixed that mismatch (`run_in_vimcode`/
`oracle_probe` now both force `wrap=true`, matching Neovim), which is what
surfaced this as a real, previously-hidden gap: `ensure_cursor_visible_wrap`
— the vertical scroll-to-cursor path used when `'wrap'` is on
(`src/core/engine/search.rs`) — never reads `self.settings.scrolloff` at
all, unlike `ensure_cursor_visible` (the `'wrap'`-off path a few lines
above it in the same file), which does. `:set so=5` then `30GH`/`30GL` land
one scrolloff-margin short of Neovim as a result.

Fixing it means porting `scrolloff` into the wrap path's visual-row-counting
loop — a real feature addition, since scrolloff needs to be expressed in
*visual* rows there (a wrapped logical line can span more than one screen
row), not buffer lines, unlike the no-wrap path's straightforward line-count
margin.

**Reproduction:** `nvim_conformance`'s `scroll:so=5 30G H` / `scroll:so=5 30G
L` cases, or manually: open a buffer with `LONG` fixture content, `:set
so=5<CR>`, `30G`, then `H` (or `L`) — compare vimcode's resulting topline
against Neovim's.

**Files:** `src/core/engine/search.rs` (`ensure_cursor_visible_wrap`,
`ensure_cursor_visible`), `tests/nvim_conformance.rs` (`KNOWN_DEVIATIONS`).

---

## `at_or_before`/`at_or_after` likely have the same `cursor_after` bug `older`/`newer` had before #1280

**Title:** `BufferManager::at_or_before`/`at_or_after` (`:earlier`/`:later
{N}[smhd]` time-cutoff form) probably land on the wrong cursor, same class
of bug #1280 fixed for `older`/`newer`

**Body:**

#1280 fixed `BufferManager::older()`/`newer()` (backing `g-`/`g+` and the
count-based `:earlier N`/`:later N`) to return the target undo-tree node's
`cursor_before` instead of `cursor_after`, verified against a live
`nvim --headless` oracle via the real input path. The doc comment on
`older`/`newer` explains why: real Neovim lands `g-`/`g+` on the target
node's pre-edit cursor, the same place plain `u` lands, regardless of which
direction you're navigating from.

`at_or_before()`/`at_or_after()` — the *time-cutoff* spec form of
`:earlier {N}[smhd]`/`:later {N}[smhd]`, as opposed to the count-stepped
form — still read `cursor_after` for the target node (`src/core/
buffer_manager.rs`, both functions, a few lines below `older`/`newer`) and
were **not** touched by #1280. This is the same class of bug: landing on a
state via a direct time-cutoff jump, not just a count-stepped one. #1280's
own corpus additions do not exercise this path (no case in `KNOWN_DEVIATIONS`
or the passing corpus drives `:earlier`/`:later` with a time-spec argument
against a non-root undo-tree node), so nothing regresses today — but it is
a real latent gap #1280's own investigation surfaced.

**Suggested fix:** mirror #1280's change — `at_or_before`/`at_or_after`
should return `n.cursor_before` instead of `n.cursor_after`, matching
`older`/`newer`. Needs its own oracle case (a `:later 5m` or similar
time-spec landing on a non-root node, verified against live Neovim) to
prove it before flipping the field, per the same rationale #1280's own doc
comment gives for why the old code's one existing corpus case never caught
this.

**Files:** `src/core/buffer_manager.rs` (`at_or_before`, `at_or_after`).

---
