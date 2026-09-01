# Current Goal — North Star

> **The living, primary objective for vimcode and every agent that works on it.**
> This is *meta-level*: above any single issue or session. Both humans and agents
> may edit it as priorities evolve — keep it short, current, and re-date the Status
> line. The `Platform-Neutrality Rule` at the top of `CLAUDE.md` is the *operational
> rule*; **this file is the source of truth for *intent* and *sequencing*.**
>
> _Last updated: 2026-09-01_

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
| **#7 Platform-Neutral** | `JDonaghy/vimcode` | **vimcode adopts a shipped quadraui API and deletes its bespoke per-backend code.** The *consume* side — this north star's execution surface. |

A typical feature flows: gap found → quadraui issue (#5 / quadraui repo) → infra
lands → **vimcode-side adoption issue (#7) deletes the old per-backend code.** The
recurring failure mode this doc exists to fix: **infra lands in quadraui but the #7
adoption issue never gets picked up**, so the bespoke code lingers as tech debt.

> quadraui milestone **#9 "vimcode Platform-Neutral blockers"** is **CLOSED OUT**
> (0 open / 5 closed) — #223/#224/#225/#375 all shipped. There is currently no
> quadraui work gating a #7 issue except quadraui#596/#597 (see #658 below). When a
> new #7 issue turns out to be supply-blocked, file the gap on `JDonaghy/quadraui`
> and re-open that milestone rather than letting the block go untracked.

## ✅ The two structural migrations are DONE

Both backends now run the same quadraui-owned loop. This was the bulk of the north
star's architectural work and it closed on **2026-08-26**:

| Epic | Milestone | Outcome |
|---|---|---|
| **#448** GTK event dispatch → `ShellApp::handle(UiEvent)` | #8 | ✅ Closed. Relm4 stripped (#540); regressions D–J all fixed. `impl ShellApp for App` at `src/gtk/mod.rs:8123`. |
| **#595** TUI → `ShellApp` + `run_with_shell` | #9 | ✅ Closed. All ten stages landed; **`fn event_loop` no longer exists in `src/`** (#634). `TuiShellApp` at `src/tui_main/shell_app.rs:1251`. |

Every issue in the old "Ready now" table — #512, #449, #454, #459, #477, #478,
#133, #479, #481, #493, #508, #515, #480 — is **closed**. #7 Platform-Neutral now
stands at 29 closed.

## Critical path — everything known is now filed and queued

As of **2026-09-01** there is no un-issue-shaped work left in the identified surface, and
the whole of it sits on the drive queue as two parallel branches:

```
quadraui#666 (running)
  ├─ vimcode#727 → #728 → #730 → #593 → #731 → #732 → #733 → #734 → #735 → #657
  └─ quadraui#596 → #597 → vimcode#658
```

Strict chains, because every vimcode entry declares `src/gtk/mod.rs` — they cannot run
concurrently, and `coord`'s #2247 overlap predictor enforces it. The two branches are in
different repos, so they run in parallel and never stale each other's verdicts.

| Order | Issue | Scope |
|---|---|---|
| 1 | **#730** | `#592-E` — paint `screen.ai_panel` on GTK. The 14th and last field from #592's table; #670 deferred it to a follow-up nobody filed. **Closes #592.** |
| 2 | **#593** | `Ctrl+V` on GTK. Unblocked by #672; the #646 `GtkDriver` supersedes its old "needs live smoke" plan. Smallest user-visible fix in the chain. |
| 3 | **#731** | 22 Relm4-era widget handles that are permanently `None`, guarding ~103 unreachable arms. **Also re-derives #723**, whose landed fix targets a `gtk4::Scrollbar` that is never constructed. |
| 4 | **#732** | Retire the GTK `Msg` bus — 124 variants, 301 sites, 684-line `dispatch`. |
| 5 | **#733** | Converge the two mouse routers — ~4,800 lines, one precedence ladder written twice. |
| 6 | **#734** | Converge keyboard dispatch — ~2,000 lines; the TUI half's 19 `mirrors mod.rs:NNNN` pointers are all stale. |
| 7 | **#735** | Converge frame composition — ~4,500 lines. The hard one; units, raw-`Buffer` residue and painter model all differ. |
| 8 | **#657** | The oracle loop. Deliberately last — Stage 1 rewrites every `crate::` path in the three modules the chain is about to shrink by ~9,000 lines. |
| ∥ | **quadraui#596 → #597 → #658** | The preview tier. #596/#597 were open, unassigned and **in nobody's queue** — the exact supply-side trap this doc exists to catch. Queued 2026-09-01. |

### The five pockets, and where each went

The 2026-08-26 revision listed "residual per-backend surface — not yet issue-shaped." All
of it is now issue-shaped, plus one it missed:

| Pocket | Size | Issue |
|---|---|---|
| Mouse/click routing | ~4,800 lines | #733 |
| GTK `Msg` bus | 124 variants / 301 sites | #732 |
| Frame composition | ~4,500 lines | #735 |
| Orphaned Relm4 widget handles | 22 fields / ~103 arms | #731 |
| **Keyboard dispatch** | ~2,000 lines | **#734** — missed by the 08-26 revision entirely |

### What "done" will and will not mean

Production lines, `#[cfg(test)]` excluded:

| | 2026-05-01 | 2026-07-01 | 2026-09-01 |
|---|---|---|---|
| `src/gtk/` | 18,979 | 13,388 | **12,588** |
| `src/tui_main/` | 14,657 | 10,305 | **11,135** |
| `src/render.rs` (shared) | 10,547 | 12,690 | **15,110** |

The May→July drop was real; **since July 1 the backends have been flat** (23,693 →
23,723) while `render.rs` grew +2,420. New work goes shared — the Platform-Neutrality
Rule is holding — but the existing mass stopped coming down, and `draw.rs`'s −2,327 was
cancelled by ordinary feature growth.

Draining the chain should remove roughly **8,700–9,500 production lines** from the two
backends (#731 ~1,000, #732 ~1,100, #733 ~3,000–3,500, #734 ~1,200, #735 ~2,500), landing
them near **14,000–15,000** with perhaps +4,000–5,000 added to `render.rs`.

**That is a 38% cut, and it is not "thin event-to-engine wiring."** What it does buy is
that every *decision* — which surface was hit, which handler owns a key, what order a
frame is composed in — is stated once. What remains after that has **not been
enumerated**: rasteriser adapters, `src/gtk/css.rs` (508 lines), window/CSD wiring,
clipboard provider setup, font metrics. Some of that is legitimately platform-specific
and should stay. **Re-run the audit when #735 lands** rather than assuming the chain
finishes the job — the honest position today is that we have sized the known duplication,
not the whole remainder.

## Architecture milestones — beyond primitive-by-primitive adoption

| Issue | What | Status |
|---|---|---|
| **quadraui#465** | macOS backend: `ShellApp` + `run_with_shell` composition support — the macOS analogue of what #595 did for TUI, and the stated gate on "the macOS port is a thin wrapper." | ✅ **CLOSED 2026-08-31** (`bd92d6f` + `434e1d6`). The gate is cleared. |
| **#657** | Put vimcode on the oracle loop | 🔨 Queued after #735 |
| **#146** | Lua plugin API → quadraui primitives | ⚠️ Weakest fit of the original #7 seeding; **not de-dup work**. Recommend moving out of #7 to a feature milestone. |

### ⚠️ Two open decisions

**1. The #657 policy freeze.** #657's body opens by declaring that as of 2026-08-10 "no
further vimcode *bug-fix* dispatch happens until vimcode is on the oracle loop." That has
comprehensively **not** been honoured — 20+ bug-fix/feature issues merged between
2026-08-26 and 09-01. Either it is retired (delete the paragraph) or it is real, in which
case #657 moves to the **front** of the chain and everything else waits. Queued at the
tail on the assumption it is retired; one `drive-queue` re-chain reverses that.

**2. Accepting worker-authored verification for the whole chain.** #657 is last, so every
fix ahead of it is verified by tests its own author wrote — the exact failure mode #657
exists to close, with #553 as the in-repo proof it is not hypothetical. This is a
deliberate trade (promoting the modules first means rewriting `crate::` paths across code
about to be deleted, then re-resolving on every subsequent PR), **not an oversight.**

## Status (2026-09-01)

- ✅ **#592's four children all landed** (#669–#672) and `src/gtk/draw.rs` is deleted.
  The epic itself stays **open** on `ai_panel` (#730) — 13 of 14.
- ✅ **Dedup sweep closed:** #621, #659, #660, #536. **#676** recovered the Command Center.
- ✅ **Both `ShellApp` migrations closed**; `event_loop()` is gone.
- 🔨 **13 queue entries cover everything known**, in two parallel branches (above).
- 🆕 **A second orphan pocket was found** — 22 permanently-`None` widget handles, same
  class as `draw.rs` but invisible to #672's `allow(dead_code)` criterion because they
  are *read*, never *written*. It has already cost a landed fix (#723).
- ⚠️ **Two operator decisions open** (above): the freeze, and worker-authored verification.
- ✅ **quadraui#465 landed 2026-08-31** — the supply-side gate on a thin macOS wrapper is
  **cleared**. Nothing on the quadraui side now blocks starting a macOS backend; what
  remains is vimcode-side, and it is the same chain above (a "thin wrapper" is only thin
  once there is no feature logic left in GTK/TUI to re-implement).
- 📋 **No vimcode issue exists for the macOS backend itself.** #47 ("Native macOS GUI")
  predates quadraui and needs re-scoping against `run_with_shell` before it means
  anything. Not queued — deliberately, until the chain above has shrunk what a wrapper
  would have to wrap.

## How to use this doc

- **Agents:** treat this as the standing objective behind all planning and triage.
  Work the drive-queue chain in "Critical path" in order; never write new
  per-backend code (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui
  API, **delete** the old backend code in the same PR — a migration that leaves both
  paths is not done.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status. The two
  open decisions above are yours, not a worker's — resolve them in this file so the next
  agent inherits the answer rather than the question.
- **When the chain drains:** re-run the sizing audit before declaring the north star met.
  "Everything known is queued" is not "everything is known" — see "What 'done' will and
  will not mean".
- **Milestone discipline:** new "delete vimcode's bespoke X for shared Y" work →
  **#7 Platform-Neutral**. A quadraui gap that *blocks* a #7 issue → file it on
  `JDonaghy/quadraui` and re-open **quadraui milestone #9 "vimcode Platform-Neutral
  blockers"** (general quadraui-build that isn't a #7 blocker → vimcode #5 / the
  quadraui repo's own milestones).
