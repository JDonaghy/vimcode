# Current Goal — North Star

> **The living, primary objective for vimcode and every agent that works on it.**
> This is *meta-level*: above any single issue or session. Both humans and agents
> may edit it as priorities evolve — keep it short, current, and re-date the Status
> line. The `Platform-Neutrality Rule` at the top of `CLAUDE.md` is the *operational
> rule*; **this file is the source of truth for *intent* and *sequencing*.**
>
> _Last updated: 2026-09-03 — the queue drained; the mandated post-#735 audit is
> now run and its numbers are below. Milestone #7 is **0 open**._

## 🎯 North star

**Eliminate all platform-specific code from vimcode and lift it into quadraui.**
`src/gtk/` and `src/tui_main/` should shrink to thin event-to-engine wiring — every
layout, hit-test, paint, and dispatch decision lives in the shared engine
(`render.rs` / `src/core/`) or in a quadraui primitive that both backends call in
1–3 lines. The end state: adding a feature touches `render.rs` once, not each
backend; a new backend (macOS, Windows) is a thin wrapper with no feature logic.

This is the direct corollary of the **Platform-Neutrality Rule** in `CLAUDE.md`:
that rule stops *new* per-backend code from being written; this goal *deletes the
existing* per-backend code that predates quadraui.

## Why this matters

- **Correctness through one code path.** Most live bugs are cross-backend
  divergence — GTK does X, TUI does Y, they drift. One shared implementation kills
  the whole class (see the dozens of open `GTK:`/`TUI:` bug issues; each is a
  symptom of a duplicated surface).
- **Leverage.** Every line lifted into quadraui is reused by coord-tui, kubeui, the
  future macOS/Windows backends, and any other consumer — vimcode stops paying to
  maintain a private UI toolkit.
- **The macOS/Windows backends are gated on this.** A native backend can only be a
  "thin wrapper" if there is no feature logic left in the existing backends to
  re-implement.

## The two-sided model — build vs. adopt

The work splits cleanly across **two milestones**. Don't conflate them:

| Milestone | Repo | What it is |
|---|---|---|
| **#5 Cross-Platform UI Crate** | `JDonaghy/vimcode` (tracking) + `JDonaghy/quadraui` | **Build quadraui itself** — new primitives, validation consumers, the macOS/Windows backends. The *supply* side. |
| **#7 Platform-Neutral** | `JDonaghy/vimcode` | **vimcode adopts a shipped quadraui API and deletes its bespoke per-backend code.** The *consume* side. |

A typical feature flows: gap found → quadraui issue (#5 / quadraui repo) → infra
lands → **vimcode-side adoption issue (#7) deletes the old per-backend code.** The
recurring failure mode this doc exists to fix: **infra lands in quadraui but the #7
adoption issue never gets picked up**, so the bespoke code lingers as tech debt.
**It just happened again in a new shape — see "#47 was closed without its blocker"
below.**

## ✅ Milestone #7 is drained — 0 open

Everything the 2026-09-01 audit filed has landed. `#730` (`ai_panel` paint),
`#593` (GTK `Ctrl+V`), `#731` (orphan Relm4 handles), `#732` (the GTK `Msg` bus),
`#733`/`#734`/`#735` (mouse, keyboard, frame composition — each split into the
slice chains below), `#657` (the oracle loop), `#658` (preview tier), plus `#480`,
`#550` and `#551`. `#146` moved out to **#4 Editor Features**, as this file
recommended — it is an addition, not a deletion.

| Convergence | Slices that actually did the work |
|---|---|
| Mouse routing (#733) | #751 → #756 |
| Keyboard dispatch (#734) | #757 → #762 |
| Frame composition (#735) | #763 → #766 |

Two structural landmarks fell with them:

- **`#657` shipped `[lib] vimcode_core`** (`eb745e2`) — `render`, `tui_main` and
  `gtk` are promoted out of the binaries, and `tests/acceptance/` is sealed. The
  oracle loop is available to this repo for the first time.
- **`#766` deleted `draw_frame`** (`eedebf8`) — the last raw-`ratatui::Frame`
  path. Both backends now compose one `FrameOp` sequence and walk it.

Also closed earlier in the arc and still true: `fn event_loop` does not exist in
`src/`; `src/gtk/draw.rs` is deleted; both `ShellApp` migrations (#448, #595) are
closed.

## 📏 The post-#735 audit — run, and it missed its projection

The previous revision of this file said: *"re-run the sizing audit when #735 lands
rather than assuming the chain finishes the job."* Done, on `develop @ eedebf8`.

Production lines, `#[cfg(test)]` excluded, all five columns measured with the same
script (`scripts/prod_lines.py`) so they are comparable to each other:

| | 2026-05-01 | 2026-07-01 | 08-31 `f867817` | **pre-chain** `6875315` | **now** `eedebf8` |
|---|---|---|---|---|---|
| `src/gtk/` | 18,969 | 13,675 | 12,526 | 9,765 | **9,650** |
| `src/tui_main/` | 14,649 | 10,358 | 11,125 | 10,958 | **10,345** |
| **both backends** | 33,618 | 24,033 | 23,651 | 20,723 | **19,995** |
| `src/render.rs` (shared) | 10,574 | 12,807 | 15,009 | 15,558 | **21,405** |

The **pre-chain** column is `6875315`, the last #732 commit — the true point before
#733/#734/#735 and slices #751–#766 began. Everything between the 08-31 and pre-chain
columns is **#722–#732**, which was dead-code deletion, not convergence.

**Projected vs. actual, over the convergence chain (`6875315` → `eedebf8`):**

| | projected | actual |
|---|---|---|
| Backends | −8,700 … −9,500, landing near 14,000–15,000 | **−728, landing at 19,995** |
| `render.rs` | +4,000 … +5,000 | **+5,847** |
| Net across the three | ≈ −4,000 | **+5,119** |

**The chain missed its projection by roughly 12×, not 2.4×.** An earlier revision of
this file credited it with −3,656; that number silently included **−2,928 from
#722–#732**, which was dead-code deletion (#731 alone `−1,432/+245`; the #732 tranches
`−1,837/+83`, `−535/+461`, `−529/+485` in `gtk/mod.rs`). Deleting unreachable code and
converging duplicated code are different activities and must not be pooled.

**The mechanism, visible in the diff:** moving a *decision* into `render.rs` leaves
every *apply* body in place at its original size, now preceded by a
`MouseDragState`/`ModalOverlayState` literal (30–60 lines per call site) and a
"#NNN moved this" comment. The `FrameOp`/`EditorOp`/`BottomOp` machinery added three
enums, three order constants, three composers, three validators and ~150 lines of doc.
A 12-variant `match` is not shorter than 12 `if` blocks.

> **Correcting the record.** The figure this file previously carried as
> "`src/gtk/` = 12,588 at 2026-09-01" was measured *before* #727/#728/#730 landed;
> it matches the 08-31 column above, not the 09-01 tree — and the 09-01 tree is in
> fact the `6875315` pre-chain column. The 05-01 and
> 07-01 figures differ from the previously recorded ones by 10–290 lines for the
> same reason — inconsistent measurement points. **Regenerate with the script, do
> not trust a number typed into prose.**

### What the chain *did* buy

Every *decision* — which surface was hit, which handler owns a key, what order a
frame is composed in — is now stated once, in `render.rs`, and both backends walk
it. Delegation density is high: `src/gtk/mod.rs` makes 424 `render::` calls. That
is a real and durable correctness win, and it is the reason the net line count went
up: the shared op-sequence machinery (`FrameOp`/`compose_frame`, the routers) costs
more lines than the duplicate pair it replaced.

**What it did not buy is the north star's stated end state.** 19,995 lines across
two backends is not "thin event-to-engine wiring", and nobody should plan as though
the remaining gap is small.

## 🔭 What actually remains

### 1. The irreducible surface — ✅ aggregated, and it is small

Done: **[`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md)** (2026-09-03).
The headline, because it changes how the rest of this goal should be planned:

- The **nine** recorded verdicts reduce to **four distinct facts**, of which **three are
  genuinely irreducible** — the TUI-only folder picker (GTK uses a native
  `GtkFileChooser`), px-vs-cell frame metrics, and GTK's menu bar *being* its CSD
  titlebar (#552).
- **Only 1.3% of the two backends names a native toolkit type** — 246 production lines
  out of 19,429 (`scripts/native_lines.py`; ~25% undercount on the GTK side for stored
  widget handles, so call it under 4% even pessimistically).
- **So platform-specificity is not what is keeping 19,995 lines in the backends.**
  `src/gtk/mod.rs` (7,684) and `src/tui_main/shell_app.rs` (3,989) are two
  implementations of the same four `ShellApp` entry points. The chain converged the
  *decisions* those implementations make; it did not converge the implementations.

**Plan accordingly: this is ordinary duplication, not a platform-porting problem.** A
plan that sizes it as the latter will keep missing its projection the way #751–#766 did.

**The fourth verdict was mislabelled** — `tui_main/mouse.rs:1620` (command-line text
selection) reads as a decision but its own text says the fix is a
`CommandLineLayout::hit_test` in quadraui. That is a *blocked* convergence whose blocker
was never filed; `CommandLineLayout` does not exist in quadraui and **#194** is the
open consumer-side symptom. Second instance in a week of the #47 failure mode — see the
milestone-discipline rule at the bottom of this file, which applies to in-code comments
too: **a comment naming a missing upstream API is an unfiled issue, and grep will not
find it for you.**

### 2. The duplication moved down a level, into quadraui — and it is unqueued

The two epics that now hold the mass of the remaining cross-backend duplication are
**open, un-milestoned, and in nobody's queue**:

- **quadraui#481** — *"shared runtime core: one implementation of the duplicated 65%
  across backends."* 1,671 non-trivial lines byte-identical between `gtk/*.rs` and
  `macos/*.rs`; `EventOutcome` declared twice verbatim; the 120 ms resize debounce
  written twice and absent on macOS; `shell_runner` 45 identical lines ×2. The
  copies have already drifted.
- **quadraui#482** — *"Backend API integrity: units, symmetry, error channel,
  panic-free text paths."* Trait asymmetry, off-trait rasterisers, unit leaks, and a
  UTF-8 boundary fix that exists as **7 private copies and zero public ones**.

This is the same supply-side trap as before, one level down: vimcode's backends got
thinner by pushing logic into a crate whose *own* backends are duplicated.

### 3. #47 was closed without its blocker

**#47 (native macOS GUI) is closed and shipped zero code.** Commit `44882e9`
("re-audit at pickup, no code — Backend-trait Rc-handle gap blocks Stage 1")
recorded the real blocker: `App` calls `GtkBackend::modal_stack_handle()` /
`drag_state_handle()` at 44 call sites in the drag/modal dispatch paths. Those are
**inherent methods on the concrete struct, not on `quadraui::Backend`**, and
`MacBackend`'s trait equivalents return short-lived `&mut` borrows incompatible
with `App`'s stash-then-reuse pattern. Full findings and two candidate API shapes
are in `PLAN.md`.

That commit's own recommendation — *"file this as a quadraui issue before any
vimcode-side Stage 1 code is written"* — **was never carried out.** No open
quadraui issue mentions `modal_stack_handle` or `drag_state_handle`. The finding
now lives only in `PLAN.md`, attached to a **closed** issue, which is exactly where
a later triage pass will not look.

**Action:** file the gap on `JDonaghy/quadraui` (or fold it into quadraui#482, its
natural home), re-open quadraui milestone **#9 "vimcode Platform-Neutral
blockers"**, and re-open vimcode#47 behind it.

### 4. The divergence bug class is not dead

This file's own thesis is that each open `GTK:`/`TUI:` bug is "a symptom of a
duplicated surface". Roughly 44 are still open — #206 (tooltip borders differ),
#420 (completion popup overflow, different failure per backend), #264 (settings
panel at narrow widths), #194 (status-bar selection: GTK can't, TUI has an offset
bug), #233 (dialog border glyphs). Plus milestone #5's cross-backend residue:
#149, #167, #168, #233, #294.

If the convergence had reached far enough, this list would be shrinking. Track
whether it does — it is the only outcome measure this goal has that isn't a line
count.

## Architecture milestones

| Issue | What | Status |
|---|---|---|
| **quadraui#465** | macOS `ShellApp` + `run_with_shell` composition | ✅ Closed 2026-08-31. The supply-side gate is cleared. |
| **#657** | Put vimcode on the oracle loop | ✅ Closed. `[lib] vimcode_core` + sealed `tests/acceptance/`. |
| **#47** | Native macOS GUI, as a thin wrapper | ⚠️ **Closed with no code.** Blocker unfiled — see above. |
| **quadraui#481 / #482** | The remaining duplication, one level down | 🔓 Open, un-milestoned, unqueued. |

### The two decisions this file was holding open — both now moot

**1. The #657 policy freeze.** #657's body declared that no vimcode bug-fix
dispatch happens until vimcode is on the oracle loop. It was never honoured, and
#657 has now landed anyway. The question is retired by events; delete the paragraph
from the issue if it is ever re-opened.

**2. Accepting worker-authored verification for the chain.** The trade was taken:
every fix ahead of #657 was verified by tests its own author wrote. It is now
*unwindable* rather than hypothetical — the sealed suite exists, so the honest
follow-up is to decide whether any of #751–#766 warrants a retro-fitted
oracle-authored test, rather than re-litigating the sequencing.

## Status (2026-09-03)

- ✅ **Milestone #7 is 0 open.** The 09-01 critical path plus 16 slices all landed.
- ✅ **The oracle loop is live here** (#657) and `draw_frame` is gone (#766).
- 📉 **The audit is run and the chain missed by ~12×:** the convergence work itself
  (`6875315`→`eedebf8`) took **−728** off the backends against a −8,700…−9,500
  projection, while `render.rs` grew **+5,847** — net **+5,119**. The −3,656 this
  file used to claim pooled in #722–#732's dead-code deletion.
- 🔎 **And most of what is left is not code.** A function-level audit found **39% of
  the two backends' 11,673 production lines are comments** — 6,958 are code, of which
  roughly **2,000 ± 500** are genuinely the same logic written twice. Converging all
  of it nets **−300 to −1,000** across the three files. **Do not plan a convergence
  campaign here; the returns are not there.**
- ⚠️ **#47 closed with zero code and its blocker filed nowhere** — the single most
  actionable item on this page.
- 🔓 **quadraui#481 / #482 hold the remaining duplication** and are unqueued.
- ✅ **The irreducible surface is aggregated** — [`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md).
  Only **1.3%** of the backends is platform-bound; the rest is duplication, and the goal
  should be planned as such.

## How to use this doc

- **Line numbers:** this file cites none, on purpose — locate by symbol
  (`grep -n "impl quadraui::ShellApp for App" src/gtk/mod.rs`). Counts here are
  evidence measured on a named revision; **regenerate them with
  `python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs`** rather than
  trusting the table.
- **Agents:** treat this as the standing objective behind all planning and triage.
  There is no queue to work right now — the next move is item 3 above (file the
  quadraui gap), then items 1 and 2. Never write new per-backend code
  (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui API,
  **delete** the old backend code in the same PR.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status.
- **Milestone discipline:** new "delete vimcode's bespoke X for shared Y" work →
  **#7 Platform-Neutral**. A quadraui gap that *blocks* a #7 issue → file it on
  `JDonaghy/quadraui` and re-open **quadraui milestone #9 "vimcode Platform-Neutral
  blockers"**. **A #7 issue that turns out to be supply-blocked must be left open
  behind that blocker, not closed** — #47 is the cautionary example.
