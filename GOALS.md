# Current Goal — North Star

> **The living, primary objective for vimcode and every agent that works on it.**
> This is *meta-level*: above any single issue or session. Both humans and agents
> may edit it as priorities evolve — keep it short, current, and re-date the Status
> line. The `Platform-Neutrality Rule` at the top of `CLAUDE.md` is the *operational
> rule*; **this file is the source of truth for *intent* and *sequencing*.**
>
> _Last updated: 2026-08-26_

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
| **#448** GTK event dispatch → `ShellApp::handle(UiEvent)` | #8 | ✅ Closed. Relm4 stripped (#540); regressions D–J all fixed. `impl ShellApp for App` at `src/gtk/mod.rs:8565`. |
| **#595** TUI → `ShellApp` + `run_with_shell` | #9 | ✅ Closed. All ten stages landed; **`fn event_loop` no longer exists in `src/`** (#634). `TuiShellApp` at `src/tui_main/shell_app.rs:1128`. |

Every issue in the old "Ready now" table — #512, #449, #454, #459, #477, #478,
#133, #479, #481, #493, #508, #515, #480 — is **closed**. #7 Platform-Neutral now
stands at 21 closed.

## Critical path — what is actually left

**The remaining work is debt left behind by those migrations, not more migration.**
Three groups, in priority order.

### 1. The orphaned GTK paint path — the biggest single item (#592, epic)

`src/gtk/draw.rs` is **3,733 lines under a file-level `#![allow(dead_code)]`**. Of
its 29 exported `pub(super) fn`s, **zero have a live caller** — it was orphaned by
#540 and nothing noticed, because that file-level allow muted the only warning that
would have said so. The user-visible cost: **13 populated `ScreenLayout` fields the
GTK live path silently drops**, each a feature that works on TUI and paints nothing
on GTK. #587 burned roughly five agent sessions on *one* of them (the command
palette) because the symptom points at input while the fault is paint.

Staged 2026-08-26 into four dispatchable children, chained **A → B → C → D**
(all three of A/B/C declare the same three files, so they cannot run concurrently):

| Issue | Scope |
|---|---|
| **#669** (A) | overlay popups: `completion`, `hover`, `editor_hover`, `diff_peek`, `signature_help` |
| **#670** (B) | panels: `quickfix`, `bottom_tabs`, `debug_toolbar`, `panel_hover`, `ai_panel` |
| **#671** (C) | chrome: `find_replace`, `tab_switcher`, `separated_status_line`, `tab_tooltip` |
| **#672** (D) | register scroll surfaces + hit-test caches on the live path, **then delete `draw.rs`** |

**#593** (`Ctrl+V` no-ops on GTK) is the same root cause on the *event* side rather
than the paint side — quadraui emits `UiEvent::ClipboardPaste` and vimcode has no
arm for it. Hold it until #672 lands; it also needs live smoke on a display-capable
machine (no GTK acceptance driver existed when it was filed — #646 has since
shipped one, so re-scope it rather than assuming manual smoke).

### 2. The dedup sweep — infra landed, adoption never happened

This is precisely the failure mode this doc exists to catch. All four are **ready
now**; the supply side is already closed.

| Issue | Migration | Supply gate |
|---|---|---|
| **#621** | delete local `fuzzy_score` / `fuzzy_score_with_positions` → `quadraui::text_util` | 🟢 quadraui#474 LANDED |
| **#659** | driver tab geometry + `SidebarSystem::reveal` | 🟢 quadraui#594 + #595 CLOSED |
| **#660** | retire 4 duplicates: tab scroll, tab measure, hit shim, split tree → `SplitTree` | 🟢 quadraui already ships all four |
| **#536** | activity-bar keyboard nav → `AppShell` cursor | 🟢 quadraui#386 CLOSED (the issue body's "blocked by" was stale until 2026-08-26) |

⚠ **#660 carries a live trap:** `SplitDirection` is *inverted* between the two
crates — vimcode's `Horizontal` means top/bottom, quadraui's means side-by-side. A
mechanical swap rotates every split 90° **and compiles**. Assert orientation on a
rendered grid, never on the enum name.

**Blocked on quadraui — the only one:**

| Issue | Migration | Waiting on |
|---|---|---|
| **#658** | preview-tab tier → `WorkspaceController` | quadraui#597 (which needs quadraui#596) — both OPEN |

Until #658 lands there are **two implementations of vimcode's own preview policy** —
the original here and the port in quadraui — which defeats the reason the port
happened.

### 3. Residual per-backend surface — not yet issue-shaped

Worth knowing when scoping; file issues as these become concrete.

- **GTK still routes through a private `enum Msg` bus** (`src/gtk/mod.rs:990-1319`,
  302 `Msg::` sites) that `ShellApp::handle` translates into before calling
  `fn dispatch` (`:1670`). The Relm4-era bus outlived its framework. TUI has no
  equivalent. #592-D's "Out of scope" flags it deliberately.
- **Raw size.** On `origin/develop` 2026-08-26: `src/gtk/` = 17,136 lines,
  `src/tui_main/` = 16,842, against `src/render.rs` = 17,041 shared. Roughly a
  quarter of each backend is in-crate tests, and `draw.rs`'s 3,733 dead lines come
  out with #672 — but "thin event-to-engine wiring" is still a long way off.
- **#146** (Lua plugin API → quadraui primitives) is the last survivor of the
  original #7 seeding and remains the weakest fit. Re-triage or drop it.

## Architecture milestones — beyond primitive-by-primitive adoption

| Issue | What | Status |
|---|---|---|
| **quadraui#465** | macOS backend: `ShellApp` + `run_with_shell` composition support. All macOS chrome *primitives* already exist — this is purely the composition/runner wiring, the macOS analogue of what #595 did for TUI. **This is the actual gate on "the macOS port is a thin wrapper."** | 📋 OPEN, supply-side (#5) |
| **#657** | Put vimcode on the oracle loop: promote `gtk`/`render`/`tui_main` into `vimcode_core`, then seal a `tests/acceptance/` suite | 📋 OPEN, `tier:large` |

**#657 is the trust gate under everything above.** vimcode's tests today are written
by the same worker that writes the fix, and #553 is the in-repo proof: its first
attempt shipped GtkDriver tests that **stayed green with the bug reinstated**, and
only the adversarial review caught it. A self-authored oracle caught by review is
luck, not a gate. Note the false blocker recorded in #657's body — "vimcode needs a
GTK acceptance driver first" is **wrong**, and chasing it costs a large piece of
work that isn't needed.

## Status (2026-08-26)

- ✅ **#448 and #595 both closed.** The two `ShellApp` migrations are done; both
  backends run the quadraui-owned loop and `event_loop()` is deleted.
- ✅ **quadraui milestone #9 cleared** (0 open / 5 closed) — no supply-side work
  gates #7 except quadraui#596/#597 for #658.
- 🔨 **#592 staged into #669/#670/#671/#672** and queued A→B→C→D. This is the
  highest-value remaining item: it deletes ~3.7k dead lines *and* fixes 13
  user-visible GTK gaps.
- 🔨 **Dedup sweep queued:** #621, #659, #660, #536 — all ready, all pure deletion.
- 📋 **Next after that:** #593 (after #672), then #658 once quadraui#596/#597 land.
- ⚠️ **#657 remains the trust gate.** Everything above is being verified by
  worker-authored tests until it lands.

## How to use this doc

- **Agents:** treat this as the standing objective behind all planning and triage.
  Bias toward clearing the "what is actually left" block; never write new
  per-backend code (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui
  API, **delete** the old backend code in the same PR — a migration that leaves both
  paths is not done.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status. When
  quadraui infra lands, move the corresponding #7 issue out of "Blocked on quadraui".
- **Milestone discipline:** new "delete vimcode's bespoke X for shared Y" work →
  **#7 Platform-Neutral**. A quadraui gap that *blocks* a #7 issue → file it on
  `JDonaghy/quadraui` and re-open **quadraui milestone #9 "vimcode Platform-Neutral
  blockers"** (general quadraui-build that isn't a #7 blocker → vimcode #5 / the
  quadraui repo's own milestones).
