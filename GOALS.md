# Current Goal — North Star

> **The living, primary objective for vimcode and every agent that works on it.**
> This is *meta-level*: above any single issue or session. Both humans and agents
> may edit it as priorities evolve — keep it short, current, and re-date the Status
> line. The `Platform-Neutrality Rule` at the top of `CLAUDE.md` is the *operational
> rule*; **this file is the source of truth for *intent* and *sequencing*.**
>
> _Last updated: 2026-06-26_

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
| **quadraui #9 "vimcode Platform-Neutral blockers"** | `JDonaghy/quadraui` | **The exact supply-side infra that gates #7 below.** Tightly scoped: only the remaining open blockers (#223 ButtonBar, #224 dual-mode Palette, #225 Dialog rich content, #375 TabGroup drag panic). |
| **#7 Platform-Neutral** | `JDonaghy/vimcode` | **vimcode adopts a shipped quadraui API and deletes its bespoke per-backend code.** The *consume* side — this north star's execution surface. |

A typical feature flows: gap found → quadraui issue (#5 / quadraui repo) → infra
lands → **vimcode-side adoption issue (#7) deletes the old per-backend code.** The
recurring failure mode this doc exists to fix: **infra lands in quadraui but the #7
adoption issue never gets picked up**, so the bespoke code lingers as tech debt.

## Critical path — #7 Platform-Neutral adoption

**Ready now** — the quadraui infra already exists; these are pure vimcode-side
deletions waiting to be picked up:

| Issue | Migration | Note |
|---|---|---|
| **#512** | `:vert diffsplit` → `quadraui::DiffView` | 🟢 **quadraui#294 LANDED** — the flagship "lifted but not adopted" case. Best first pick. |
| **#448** | GTK event dispatch → `ShellApp::handle(UiEvent)` | shell runner already forwards `UiEvent` |
| **#449** | GTK click dispatch → `FrameHitMap` | |
| **#454** | toast render + click dispatch in GTK | |
| **#459** | context-menu dispatch → `ModalStack` | deletes the 4 TUI panel-intercept gates |
| **#477** | TUI tab-bar drag slots → cached `TabBar` hit regions | |
| **#478** | TUI sidebar-item hover popup → shared tooltip builder | |
| **#133** | unified sidebar rendering → `ScreenLayout` | (was tracked under Crate Extraction) |
| **#146** | Lua plugin API → expose quadraui primitives | weakest fit — re-triage if it drifts |
| **#479** | TUI Settings inline-edit → `FormController` | 🟢 quadraui#221/#157/#222 LANDED |
| **#481** | TUI window-sep scrollbar + tab-drop overlay → primitives | 🟢 quadraui#226 + #121 LANDED |
| **#493** | Full GTK `run_with_shell` migration (collapse 14 DAs → 1, strip Relm4) | 🟢 quadraui#217 LANDED |
| **#508** | TUI text-selection + OSC52 clipboard; delete bespoke plumbing | 🟢 quadraui#269 + #283 LANDED |

**Ready once a quadraui fix lands** (feature exists, but with an open correctness bug):

| Issue | Migration | Waiting on |
|---|---|---|
| **#515** | editor-group drag-and-drop → `TabGroupController` | quadraui#349 LANDED, but **quadraui#375** (TUI drag-start panics) must be fixed before adoption is safe |

**Blocked on quadraui** — genuinely waiting on unbuilt infra (the *only* one left):

| Issue | Migration | Blocked on (quadraui milestone #9) |
|---|---|---|
| **#480** | TUI Source Control panel → shared primitives | **#223** ButtonBar · **#224** dual-mode Palette · **#225** Dialog rich content (TextInput #222 already landed) |

## Status (2026-06-26)

- ✅ **Milestone #7 "Platform-Neutral" created** and seeded with 15 adoption issues
  (11 pulled out of #5, #133 out of Crate Extraction, + #146/#512/#515 from
  no-milestone). #5 is now scoped to pure quadraui-build. All 15 sent to the coord
  Pipeline:New (`coord` + `status:ready`).
- 🔎 **Supply audit (2026-06-26): the supply side is ~80% already built.** Of the 6
  issues first thought "blocked on quadraui", **4 are actually unblocked** (#479/#481/
  #493/#508 — infra closed, `~/src/quadraui` current), **#515 is unblocked but unsafe**
  until quadraui#375's drag panic is fixed, and **only #480 truly waits** on unbuilt
  quadraui infra (#223/#224/#225).
- ✅ **quadraui milestone #9 "vimcode Platform-Neutral blockers" created** — the
  supply-side counterpart, scoped to exactly #223/#224/#225 (gate #480) + #375
  (gate #515). When those close, the whole #7 milestone is unblocked.
- 📋 **Next pick:** #512 — quadraui#294 (`DiffView`) has landed; highest-value first
  deletion. Then sweep the rest of "Ready now" (#479/#481/#493/#508 are also free now).

## How to use this doc

- **Agents:** treat this as the standing objective behind all planning and triage.
  Bias toward clearing the "Ready now" block; never write new per-backend code
  (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui API, **delete**
  the old backend code in the same PR — a migration that leaves both paths is not done.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status. When
  quadraui infra lands, move the corresponding #7 issue from "Blocked" to "Ready now".
- **Milestone discipline:** new "delete vimcode's bespoke X for shared Y" work →
  **#7 Platform-Neutral**. A quadraui gap that *blocks* a #7 issue → file it on
  `JDonaghy/quadraui` and add it to **quadraui milestone #9 "vimcode Platform-Neutral
  blockers"** (general quadraui-build that isn't a #7 blocker → vimcode #5 / the
  quadraui repo's own milestones).
