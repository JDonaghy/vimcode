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

**Blocked on quadraui** — the #7 adoption can't start until the named quadraui infra
lands and `~/src/quadraui` is pulled:

| Issue | Migration | Blocked on |
|---|---|---|
| **#479** | TUI Settings inline-edit → `FormController` | quadraui `Form` text-cursor support |
| **#480** | TUI Source Control panel → shared primitives | quadraui TextInput / ButtonBar / dual-mode Palette / Dialog rich content |
| **#481** | TUI window-separator scrollbar + tab-drop overlay → primitives | quadraui `Backend::draw_scrollbar()` + drag-drop visual |
| **#493** | Full GTK `run_with_shell` migration (collapse 14 DAs → 1, strip Relm4) | quadraui#217 |
| **#508** | TUI text-selection + OSC52 clipboard; delete bespoke plumbing | quadraui#269 |
| **#515** | editor-group drag-and-drop → `TabGroupController` | quadraui#349 |

## Status (2026-06-26)

- ✅ **Milestone #7 "Platform-Neutral" created** and seeded with 15 adoption issues
  (11 pulled out of #5, #133 out of Crate Extraction, + #146/#512/#515 from
  no-milestone). #5 is now scoped to pure quadraui-build.
- 📋 **Next pick:** #512 — quadraui#294 (`DiffView`) has landed, so this is the
  highest-value unblocked deletion. Then sweep the rest of the "Ready now" block.
- 🚧 **Six issues are blocked on quadraui infra** (#479/#480/#481/#493/#508/#515) —
  each names the gap; those are the priority quadraui-repo asks.

## How to use this doc

- **Agents:** treat this as the standing objective behind all planning and triage.
  Bias toward clearing the "Ready now" block; never write new per-backend code
  (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui API, **delete**
  the old backend code in the same PR — a migration that leaves both paths is not done.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status. When
  quadraui infra lands, move the corresponding #7 issue from "Blocked" to "Ready now".
- **Milestone discipline:** new quadraui-build gaps → #5 (or the quadraui repo);
  new "delete vimcode's bespoke X for shared Y" work → **#7 Platform-Neutral**.
