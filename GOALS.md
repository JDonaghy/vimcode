# Current Goal — North Star

> **The living, primary objective for vimcode and every agent that works on it.**
> This is *meta-level*: above any single issue or session. Both humans and agents
> may edit it as priorities evolve — keep it short, current, and re-date the Status
> line. The `Platform-Neutrality Rule` at the top of `CLAUDE.md` is the *operational
> rule*; **this file is the source of truth for *intent* and *sequencing*.**
>
> _Last updated: 2026-09-05 (issue #827 correction pass — the 2026-09-03 revision
> went stale within 81 minutes of its own edit and stayed that way for two days;
> see "#47 was closed without its blocker" below). Milestone #7 is **0 open**._

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

Production lines, `#[cfg(test)]` excluded, all columns measured with the same
script (`scripts/prod_lines.py`) so they are comparable to each other:

| | 2026-05-01 | 2026-07-01 | pre-chain 2026-08-31 | pre-#785 2026-09-03 | **post-#785, now @ `ee26268`** |
|---|---|---|---|---|---|
| `src/gtk/` | 18,969 | 13,675 | 12,526 | 9,650 | **2,607** |
| `src/tui_main/` | 14,649 | 10,358 | 11,125 | 10,345 | **10,366** |
| `src/app.rs` (hoisted out of `src/gtk/` by #785) | — | — | — | — | **7,131** |
| **all three files** | 33,618 | 24,033 | 23,651 | 19,995 (2 files) | **20,104** |
| `src/render.rs` (shared) | 10,574 | 12,807 | 15,009 | 21,405 | **21,405** |

(All five 2026-09 revisions confirmed by re-running `prod_lines.py` against
`git archive ee26268`; the pre-#785 column is a snapshot at an earlier commit,
not a stale guess — it's what the tree actually was before #785 moved
`struct App`.)

**Projected vs. actual, over the chain (08-31 → 09-03):**

| | projected | actual |
|---|---|---|
| Backends | −8,700 … −9,500, landing near 14,000–15,000 | **−3,656, landing at 19,995** |
| `render.rs` | +4,000 … +5,000 | **+6,396** |
| Net across the three | ≈ −4,000 | **+2,740** |

The chain removed roughly **40% of the low end** of its own estimate, and the
shared engine grew *more* than projected. Where the reduction actually came from:

| File | pre-chain | now | Δ |
|---|---|---|---|
| `src/gtk/mod.rs` | 10,518 | 7,684 | **−2,834** |
| `src/tui_main/panels.rs` | 1,554 | 1,208 | −346 |
| `src/tui_main/mouse.rs` | 3,211 | 2,895 | −316 |
| `src/tui_main/shell_app.rs` | 4,109 | 3,989 | −120 |
| everything else | | | ≈ −40 net |

`gtk/mod.rs` alone is 78% of the cut. Notably `tui_main/mouse.rs` — the file #733
was sized against at −3,000…−3,500 — lost **316 lines**.

**Attribution correction (#827): the −3,656 is mostly dead-code removal, not
convergence.** Of the −3,656 backend reduction, roughly **−2,825** is #731
(orphan Relm4 handles) and #732 (the GTK `Msg` bus) deleting code outright —
that is almost all of the −2,834 booked against `src/gtk/mod.rs` above.
Convergence proper (#751–#766 actually sharing logic through `render.rs`)
moved something closer to **900 lines** out of the backends, while
`render.rs` absorbed roughly **7,000**. State it plainly: the chain did not
converge anywhere near as much as the raw −3,656/+6,396 pair implies: most of
the shrinkage is code that was simply dead.

> **#785 (stage 1 of #47) moved the mass, it did not delete it.** `struct App`,
> its `impl` blocks and `impl quadraui::ShellApp for App` were hoisted verbatim
> out of `src/gtk/mod.rs` into a new top-level `src/app.rs`. `src/gtk/`
> reads **2,607** against the pre-#785 9,650 column above and `src/app.rs`
> reads **7,131** — both regenerated at `ee26268` in the table above — but the
> *total* is essentially unchanged (module doc and re-stated imports account
> for the difference). Nothing here got smaller; a ~6,900-line block that was
> filed under "GTK backend" is now filed under "shell application", where a
> second native backend can reach it. `src/app.rs` is still `gui`-gated: its
> module doc enumerates the four platform-typed fields, ~11 platform hook call
> sites and the `crate::gtk::{click, css, util}` dependency that have to go
> before the gate can.

> **Correcting the record.** The figure this file previously carried as
> "`src/gtk/` = 12,588 at 2026-09-01" was measured *before* #727/#728/#730 landed;
> it matches the pre-chain 08-31 column above, not the 09-01 tree. The 05-01 and
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

Done: **[`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md)** (2026-09-03,
corrected 2026-09-05 per #827 — the folder-picker verdict below was wrong).
The headline, because it changes how the rest of this goal should be planned:

- The **nine** recorded verdicts reduce to **three distinct facts**, of which **two are
  genuinely irreducible** — px-vs-cell frame metrics, and GTK's menu bar *being* its CSD
  titlebar (#552). A fourth candidate, the TUI-only folder picker, was recorded as
  irreducible on the theory that GTK's native `GtkFileChooser` has no shared
  counterpart — wrong: `quadraui::compose::FolderPickerController` has existed since
  2026-05-25 and its module doc explicitly tells vimcode to delete the local copy and
  rewire both backends through it. That verdict is struck, not counted.
- **Only 1.3% of the two backends names a native toolkit type** — 246 production lines
  out of 19,429 (`scripts/native_lines.py`; ~25% undercount on the GTK side for stored
  widget handles, so call it under 4% even pessimistically).
- **So platform-specificity is not what is keeping 19,995 lines in the backends.**
  `src/gtk/mod.rs` (7,684) and `src/tui_main/shell_app.rs` (3,989) are two
  implementations of the same four `ShellApp` entry points. The chain converged the
  *decisions* those implementations make; it did not converge the implementations.

**Plan accordingly: this is ordinary duplication, not a platform-porting problem.** A
plan that sizes it as the latter will keep missing its projection the way #751–#766 did.

**The third verdict was mislabelled** — `tui_main/mouse.rs:1620` (command-line text
selection) reads as a decision but its own text says the fix is a
`CommandLineLayout::hit_test` in quadraui. That is a *blocked* convergence whose blocker
was never filed; `CommandLineLayout` does not exist in quadraui and **#194** is the
open consumer-side symptom. Second instance in a week of the #47 failure mode — see the
milestone-discipline rule at the bottom of this file, which applies to in-code comments
too: **a comment naming a missing upstream API is an unfiled issue, and grep will not
find it for you.**

### 2. "The duplication moved down a level" is largely refuted (#827)

The previous revision of this file claimed quadraui#481/#482 held a large, unqueued
mass of cross-backend duplication one level down. Re-checked against quadraui's own
pinned rev (`42e0f8f`) on 2026-09-05: most of the individual claims do not hold up.

| Claim (previous revision) | Reality at pin `42e0f8f` |
|---|---|
| `EventOutcome` declared twice verbatim | Declared **once** — `quadraui/src/runtime.rs:95` (quadraui#496, closed 09-02) |
| `shell_runner` 45 identical lines ×2 | Four runners of 4–7 non-comment lines each, all delegating to shared `shell_adapter.rs::build_shell_adapter` |
| 1,671 byte-identical lines gtk↔macos | Function-level duplication is ~85 lines. quadraui#481's own correction comment (09-03 17:45Z) withdrew the headline number as "idiom coincidence" |
| UTF-8 fix: 7 private copies, 0 public | **Public since 2026-08-15** — `text_util.rs:51-107`, re-exported from `lib.rs` (quadraui#503) |
| `gtk_tree_layout`/`mac_tree_layout` twins | Both 1-line wrappers over `primitives/layout_metrics.rs:60 tree_layout` (quadraui#499, 09-02) |
| "no `desktop/`" | `quadraui/src/desktop.rs` exists (754 lines) since 09-02 (quadraui#498) — the original claim grepped for a directory that had been renamed |
| #482 "holds the mass" | **All eight children #503–#510 are closed.** #482 has zero comments and is a hollow epic |

**Still true:** macOS dispatches `WindowResized` undebounced (`macos/run.rs:544-561`)
while TUI/GTK use the shared `ResizeDebouncer`. That is a real, small, still-open gap —
just not the "65% duplicated" epic the previous revision described. quadraui#481/#482
remain open and un-milestoned, but do not plan against their headline numbers; re-audit
the specific claim you need before acting on it.

### 3. #47's blocker was filed and cleared — this section was stale for two days (#827)

**Corrected 2026-09-05.** The previous revision said *"No open quadraui issue
mentions `modal_stack_handle` or `drag_state_handle`"* and called filing one "the
single most actionable item on this page." That stopped being true within hours of
being written:

- **quadraui#699** was filed 2026-09-03 **16:38Z**, into quadraui milestone **#9**,
  and **closed 17:11Z** (PR#700 / `88345fb`). A follow-up, **#704**, closed 21:41Z.
- **vimcode#47 was reopened 16:38Z** and is **OPEN**, in milestone **#5**, right now
  — it was never re-closed.
- The commit that last touched this file (`5e2c7cc`) landed **18:32Z — 81 minutes
  after #699 had already closed** — and still said the blocker was unfiled. The
  finding sat in `PLAN.md` for less than a day before it was acted on; the doc that
  said otherwise just never got re-read against events.
- quadraui milestone #9 was never closed — it's **open** (0 open / 7 closed issues
  in it). The previous instruction to "re-open" it was acting on a wrong premise.

**What actually happened, and what's still open:** quadraui#699/#704 gave every
backend a symmetric Rc-handle API (`modal_stack_handle()` / `drag_state_handle()`),
removing the `Backend`-trait asymmetry that blocked Stage 1. vimcode has already
started consuming it — **#811** (this branch's own history) bumped the quadraui pin
to `4ff2a64` and ported the four TUI-side call sites off the now-removed
`drag_and_modal_mut`. **vimcode#47 itself is still open** (Stage 1 — moving `App`'s
remaining GTK-specific call sites onto the new API, see `PLAN.md`) and is the actual
next actionable item here, not a re-filing task.

**The "44 call sites" figure was also wrong**, independent of the above. It came
from `grep -n 'self\.backend\.' src/gtk/mod.rs` — every use of the `backend` field,
not just the two Rc-handle methods. The real count, measured at `ee26268`
(`grep -n 'modal_stack_handle\|drag_state_handle' src/app.rs`, minus the two
doc-comment mentions of the method names): **19** — `modal_stack_handle` ×12,
`drag_state_handle` ×7. (Also note the field moved: by `ee26268` this code lives in
`src/app.rs`, not `src/gtk/mod.rs` — #785 had already hoisted it. Fixed in `PLAN.md`
and `PROJECT_STATE.md` too, which is where the 44 figure originates.)

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
| **#47** | Native macOS GUI, as a thin wrapper | 🔓 **Reopened 2026-09-03, OPEN in milestone #5.** Blocker resolved (quadraui#699/#704); #811 already ported the TUI side. Stage 1 (GTK side) is the actual next work — see `PLAN.md`. |
| **quadraui#481 / #482** | Duplication one level down — largely refuted, see §2 above | 🔓 Open, un-milestoned. Don't plan against their headline numbers. |

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

## Status (2026-09-05, corrected per #827)

- ✅ **Milestone #7 is 0 open.** The 09-01 critical path plus 16 slices all landed.
- ✅ **The oracle loop is live here** (#657) and `draw_frame` is gone (#766).
- 📉 **The audit is run and it missed its projection** — backends −3,656 against a
  −8,700…−9,500 projection; the three files net **+2,740** lines. But ~−2,825 of
  that −3,656 is #731/#732 dead-code removal, not convergence — convergence proper
  moved roughly **900 lines** out of the backends while `render.rs` absorbed **~7,000**.
- 🔓 **#47 is open again, in milestone #5.** Its blocker (quadraui#699/#704) closed
  2026-09-03; #811 already ported the TUI side onto the new API. Stage 1 (GTK side)
  is the actual next actionable item — not a re-filing task.
- 🔓 **quadraui#481 / #482 remain open and un-milestoned**, but most of their
  headline duplication claims were refuted 2026-09-05 (see §2) — the one confirmed
  live gap is macOS's undebounced `WindowResized`.
- ✅ **The irreducible surface is aggregated** — [`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md),
  corrected 2026-09-05 (the folder-picker verdict was wrong; struck). **Two** of the
  three remaining facts are genuinely irreducible; only **1.3%** of the backends
  names a native toolkit type — the rest is duplication, and the goal should be
  planned as such.

## How to use this doc

- **Line numbers:** this file cites none, on purpose — locate by symbol
  (`grep -n "impl quadraui::ShellApp for App" src/app.rs`). Counts here are
  evidence measured on a named revision; **regenerate them with
  `python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs src/app.rs`**
  rather than trusting the table.
- **Agents:** treat this as the standing objective behind all planning and triage.
  Item 3's blocker is cleared — the next move is **vimcode#47 Stage 1** (see
  `PLAN.md`), then items 1, 2 and 4 above. Never write new per-backend code
  (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui API,
  **delete** the old backend code in the same PR.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status.
- **Milestone discipline:** new "delete vimcode's bespoke X for shared Y" work →
  **#7 Platform-Neutral**. A quadraui gap that *blocks* a #7 issue → file it on
  `JDonaghy/quadraui` and re-open **quadraui milestone #9 "vimcode Platform-Neutral
  blockers"**. **A #7 issue that turns out to be supply-blocked must be left open
  behind that blocker, not closed** — #47 is the cautionary example.
