# Coordinator Integration — Shared Board, Hosted Anywhere

> **Status:** Design / pre-implementation — _2026-06-12_
> **Repos:** `JDonaghy/vimcode` (host), `JDonaghy/quadraui` (shared component), `JDonaghy/claude-coordinator` (data + pipeline brain)
> **Pre-req issues:** [quadraui#362](https://github.com/JDonaghy/quadraui/issues/362) (Board component), [claude-coordinator#550](https://github.com/JDonaghy/claude-coordinator/issues/550) (`coord board --json`), [vimcode#521](https://github.com/JDonaghy/vimcode/issues/521) (Issues panel)
> **Roadmap:** milestone [`vimcode-coordinator`](https://github.com/JDonaghy/vimcode/milestone/6) · epic [#531](https://github.com/JDonaghy/vimcode/issues/531) (live tracker + dependency graph)

## 1. Goal — client parity

The coordinator pipeline (refine → plan → work → review → merge) can be driven from
**either** the standalone `coord-tui` **or** from inside **vimcode** — and a user who
only ever opens one of them can still do *everything*. Some operators live in the
board; some live in the editor. Neither should be a second-class citizen.

The lever that makes this affordable: **one shared quadraui Board component**, rendered
identically in both apps, fed by **one board projection** computed in coordinator. Both
clients become thin: data-in, actions-out.

The payoff beyond parity is the thing the board can't do today — **review the agent's
work as real code, in a real editor**. That's the "less agentic, more hands-on" pivot,
and it falls out naturally once the board lives next to vimcode's diff/LSP/git machinery
(see §9).

## 2. Background — two worlds, one seam

Coordinator owns the **verb**: the issue lifecycle, the agent fleet, the merge brain
(`coord/notify.py`, `coord/merge_queue.py`, `coord/auto_loop.py` — all Python). vimcode
owns the **noun**: the actual code, with vim, LSP, git, diffs.

Today they never share a surface. Coordinator shows a *verdict card*; vimcode shows
*files*; the diff that connects them only exists if a human manually pulls a branch. The
integration makes the diff a first-class surface and lets the board and the editor live in
the same window.

**Strategic fit:** coordinator's own North Star (`GOAL.md`) is *"make human-attended
interactive sessions drivable end-to-end, reporting verdicts via `coord report-result`."*
A vimcode review **is** a human-attended session. vimcode-as-reviewer is the most ergonomic
realization of coordinator's ToS-compliant escape hatch, not a side quest.

## 3. Architecture — three peer clients, one component

Coordinator already treats its clients as **peers, not nested layers** (`docs/ARCHITECTURE.md`,
"Divergence risk"): the CLI and coord-tui are independent clients of the same SQLite +
GitHub state. vimcode becomes the **third peer client**.

```
              ~/.coord/coord.db (SQLite)  +  GitHub (issues/PRs)  +  coordinator.yml
                                          ▲
                  ┌───────────────────────┼───────────────────────┐
                  │                       │                       │
              coord CLI              coord-tui                vimcode
              (Python)               (Rust)                   (Rust + Lua ext)
                                         │                       │
                                         └─────────┬─────────────┘
                                                   ▼
                                  quadraui::Board   (shared component)
                                  data-in: BoardModel
                                  actions-out: BoardAction
```

Three hard rules keep this from becoming a maintenance trap:

1. **The board *render + input* is shared** — one `quadraui::Board` component, not
   re-implemented per app. (Today coord-tui hand-rolls its board in `tui/src/app.rs` from
   lower-level quadraui primitives; a third hand-roll in vimcode is the wrong move.)
2. **The board *projection* is computed once, in coordinator** — `coord board --json`
   emits the `BoardModel`. Clients render it; they don't recompute lifecycle/gate logic.
   This directly attacks the divergence risk coordinator already documents (Python vs Rust
   re-implementing `has_approved_review`, `PipelineMergeState`, conflict classifiers).
3. **vimcode never re-implements the pipeline brain.** It *consumes* state and *invokes*
   existing `coord` subcommands for actions. The brain stays in Python.

### Why not embed coord-tui wholesale?

coord-tui is a quadraui app, so "just embed the app" is tempting. But its data layer is
Python (SQLite schema, gate logic) and its rendering is bespoke. Embedding it would drag
vimcode's single-crate, `core`-is-pure architecture into coordinator's Python world.
Extracting the *board view* as a component (the expensive, reusable part) and keeping the
*projection* behind a JSON seam is the clean line. Both apps get thinner, not fatter.

## 4. The shared Board component (quadraui)

A reusable, themeable kanban/pipeline widget. **Pure render + input. No data fetching, no
business logic.** Mirrors how vimcode's `render.rs` already works: data → layout →
backend draws it; backend hands semantic actions back to the host.

**Data in — `BoardModel`:**

- `columns: Vec<BoardColumn>` — e.g. Backlog, Refining, Ready, Pipeline, Done.
- `BoardColumn { id, title, cards: Vec<BoardCard> }`.
- `BoardCard { id, repo, issue_number, title, labels, stage_badges, assignee, machine,
  verdict_state, decision_hint }`. `stage_badges` encode Plan/Work/Test/Review/Merge state
  (pending / running / passed / request-changes / blocked) so the card can show the
  pipeline at a glance.
- Optional `decision_hint` so the brain (coordinator) can surface "needs a judgment call"
  with a one-line recommendation (the `GOAL.md` Horizon decision-queue idea, #517/#518).

**Actions out — `BoardAction`:**

The component emits *semantic* events; the host decides what they mean.
`SelectCard(id)`, `OpenIssue(id)`, `Refine(id)`, `Dispatch(id)`, `RecordTest(id, verdict)`,
`StartReview(id)`, `OpenReview(id)` (← the deep-link into an editor review, §9),
`Merge(id)`, `DropToBacklog(id)`, `ContextMenu(id, anchor)`, `MoveSelection(dir)`.

**Input:** keyboard (vim-style `j`/`k`/`h`/`l`, `Enter`, `gg`/`G`, single-key stage actions
like coord-tui's `P`/`S`/`F` Test verdicts) **and** mouse (click select, right-click menu,
wheel scroll). vimcode and coord-tui both already lean on quadraui's paint↔click cache
(`feedback_cache_paint_layout`); the Board exposes the same contract.

**Consumers:**
- **coord-tui** migrates its bespoke `tui/src/app.rs` board onto the component (reference
  implementation; proves parity).
- **vimcode** hosts it in the Issues panel (§7).

## 5. The data bridge

### Reads — `coord board --json`

Coordinator grows a machine-readable projection. One command computes the `BoardModel`
(in Python, where the gate/lifecycle logic already lives) and emits a stable JSON schema.
vimcode polls it; coord-tui can adopt it later to retire its Rust-side projection.

- `coord board --json` → the full `BoardModel`.
- `coord show-plan <id> --json` → structured plan for plan-only assignments (plan preview).
- `--json` on `coord status` for machine/assignment/cost detail.

Rationale: without this, vimcode either (a) reads `~/.coord/coord.db` directly in Rust —
re-implementing schema + gate logic, the exact divergence trap — or (b) screen-scrapes
text output. A JSON projection is the cheap, correct seam.

### Actions — `coord` subprocess

Every board action maps to an **existing** `coord` subcommand. vimcode shells out; no new
coordinator verbs needed for parity:

| BoardAction | coord invocation |
|---|---|
| Refine → Ready | `coord refine` / `coord ready` |
| Dispatch work | `coord assign …` |
| Record Test gate | `coord test <id> --passed\|--skipped\|--fail` |
| Start review | `coord pr <id>` / `coord assign --review-of …` |
| Report verdict | `coord report-result --assignment <id> --verdict <v> --body-file <f>` |
| Merge | `coord merge …` |
| Drop to backlog | `coord backlog <repo> <issue>` |

`report-result` is the ToS-compliant verdict-in channel coordinator already standardized
on — a vimcode review writes its findings to a temp file and calls it with `--body-file`
(the `--body-file` need is already tracked in coordinator's `GOAL.md`).

### Freshness — the poll model

Coordinator has **no daemon**; the pipeline only advances when `coord notify` runs. vimcode
already has the pattern for this: background poll loops like `poll_ext_registry` /
`poll_sc_diff` that `try_recv` on a timer. The coordinator extension:
- polls `coord board --json` on a timer to refresh the `BoardModel`;
- optionally runs `coord notify` on a (longer) timer so the pipeline doesn't freeze when
  vimcode is the only client open. (Configurable — a passive viewer shouldn't drive the
  loop unasked.)

## 6. Where the code lives in vimcode

Follows the platform-neutrality rule (`CLAUDE.md`): shared logic in `render.rs`/engine,
**1–3 lines of wiring per backend**, no bespoke GTK/TUI board code.

- **`src/render.rs`** — `BoardData` (the vimcode-side view model) built from the
  `coord board --json` payload; handed to `quadraui::Board`. New `ScreenLayout.board`
  slot, like `ext_sidebar`.
- **`src/core/`** — engine fields for the coordinator panel (selection, focus, last
  fetched model, poll receiver). Pure; no Python, no GTK. Subprocess calls go through a
  thin `coord_client.rs` (spawn `coord …`, parse JSON) — `core` stays testable in
  isolation by mocking the client.
- **`src/gtk/` + `src/tui_main/`** — register the *Issues* entry in the activity bar and
  draw `quadraui::Board` (the component does the work). Click/key → `BoardAction` →
  engine → `coord_client`.
- **Lua extension bundle** — the *packaging and glue*: registers the Issues activity entry
  + `:Coord*` commands (`:CoordRefine N`, `:CoordReview <id>`, `:CoordDispatch …`), and
  carries the manifest. It does **not** render the board (that's the shared component) and
  does **not** hold pipeline logic.

## 7. The Issues panel

A new activity-bar entry — **Issues** — alongside Explorer / Search / Source Control / Run /
Extensions. Selecting it shows the coordinator board (the shared component) in the sidebar
or a full editor-area surface. Reuses the activity-bar + panel machinery vimcode already
has (the SC and Extensions panels are the template — `TuiPanel`, `ext_sidebar`,
`PanelRegistration`).

## 8. Parity matrix (the acceptance bar)

Every row works from **both** clients. coord-tui is the reference; vimcode reaches parity
by sourcing reads from `coord board --json` and actions from `coord` subprocess.

| Capability | coord-tui (today) | vimcode (target) |
|---|---|---|
| See board / pipeline | ✅ | ✅ (shared component) |
| Add / refine / edit an issue | ✅ (TUI fields) | ✅ **+ as a markdown buffer** (§9) |
| Dispatch work / plan | ✅ | ✅ |
| Record Test gate (P/S/F) | ✅ | ✅ |
| Start review | ✅ | ✅ |
| **Read the diff / review the code** | ⚠️ verdict only | ✅ **in-editor review tab** (§9) |
| Report verdict (approve / request-changes) | ✅ | ✅ |
| Merge | ✅ | ✅ |
| Watch live worker log | ✅ (terminal tab) | ✅ (needs quadraui terminal primitive) |

The two rows where vimcode *exceeds* the board are the whole point: issue authoring in a
real editor, and review against real code.

## 9. The review cockpit (the differentiator)

Parity gets vimcode to "the board, but in my editor." The reason to bother is the next
step: **the board deep-links into a real review.**

- **Issue authoring as buffers.** `:CoordRefine 42` opens the issue body as a markdown
  buffer — file-path completion, LSP symbol references, paste code from open buffers,
  markdown preview — `:w` pushes it back via `coord` and flips `status:refining → ready`.
  A real editor beats a TUI text field for prose+code authoring. (Addresses coordinator
  `GOAL.md` #547 briefing readability, #359 refinement limbo.)
- **In-editor diff review.** `BoardAction::OpenReview` on a completed work card opens the
  branch as a genuine multi-file diff tab. vimcode already ships every piece: `]c`/`[c`
  hunk navigation, diff-peek popups, git line-status gutters, the async "git show
  HEAD:file" diff-open (Session 197), LSP diagnostics + blame on the changed files.
- **Inline comments → findings.** Line annotations / virtual text (Session 113) pin review
  comments to lines; they collect into a review body written to `--body-file` and sent via
  `coord report-result`.
- **Human edits, pushed back.** The worktree is a real checkout. Fix a one-liner yourself
  in vim; the edit becomes a commit on the branch, finalized/pushed deliberately (coord's
  remote-fix `finalize` is the template) so commits never live only in a soon-pruned
  worktree.
- **Terminal-native reach.** Because vimcode isn't Electron, the review can run *where the
  code is* — vimcode-over-ssh in a worker's worktree — which is exactly coordinator's
  ssh+tmux fleet model (`GOAL.md` Horizon). coord-tui can't edit files there; vimcode can.

These are **follow-on** to the board (they don't block parity), but they're the reason the
host is vimcode specifically rather than any board renderer.

## 10. Divergence & risks

- **Triple-render divergence** — mitigated by the shared component (one renderer).
- **Triple-projection divergence** — mitigated by `coord board --json` (one projection).
  Interim: if coord-tui keeps its Rust projection while vimcode uses JSON, the *component*
  is still shared; converge the projection later.
- **Pipeline freeze** — no daemon means `coord notify` must run; vimcode can drive it on a
  timer, but only opt-in (a passive viewer shouldn't silently dispatch metered work).
- **Worktree locality** — review-where-the-code-is (ssh) vs pull-local (`coord pull`).
  Support both; the issue→branch→files mapping is the core data the extension needs.
- **vimcode ↔ coord coupling** — the extension requires a `coord` install on PATH; `core`
  stays pure by isolating subprocess calls behind `coord_client.rs` (mockable in tests).

## 11. Phased plan

_Issue numbers in brackets; epic [#531](https://github.com/JDonaghy/vimcode/issues/531) is the live tracker._

- **Foundation** — `coord_client.rs` subprocess + JSON bridge. **[#522]** _(blocks all)_
- **Phase 0 — read-only board panel** — Issues activity entry renders the shared component
  from `coord board --json`. **[#521]** _(blocks: quadraui#362, coord#550, #522)_
- **Phase 0b — board actions** — wire `BoardAction` → `coord` (dispatch/test/review/merge).
  **[#523]**
- **Phase 1 — issue authoring as buffers** — `:CoordRefine`, `:w` pushes. **[#524]**
- **Phase 2 — in-editor diff review** — `OpenReview` → multi-file diff tab, hunk nav. **[#525]**
  → verdict via `coord report-result`. **[#526]** *The differentiator.*
- **Phase 3 — hands-on** — inline comments → findings **[#527]**; human edits pushed back
  to the branch **[#528]**.
- **Phase 4 — observability** — live worker-log stream, plan preview, growing diff (adopts
  the quadraui terminal primitive). **[#529]**
- **Phase 5 — remote / terminal** — vimcode-over-ssh as the fleet's review seat. **[#530]**

## 12. Pre-req issues

The three "obvious" foundations for Phase 0:

- **[quadraui#362](https://github.com/JDonaghy/quadraui/issues/362)** — Reusable `Board`
  component (render + input; `BoardModel` in / `BoardAction` out; no business logic).
  Consumed by coord-tui and vimcode.
- **[claude-coordinator#550](https://github.com/JDonaghy/claude-coordinator/issues/550)** —
  `coord board --json` (+ `--json` on `status` / `show-plan`): one machine-readable board
  projection so non-Python clients render the same model without re-implementing gate logic.
- **[vimcode#521](https://github.com/JDonaghy/vimcode/issues/521)** — *Issues* activity-bar
  panel + coordinator extension hosting the quadraui Board, sourced from `coord board --json`,
  actions via `coord` subprocess.

## 13. Open questions

- Sidebar vs full editor-area board in vimcode (or both, like a maximizable panel)?
- Does coord-tui migrate its projection to `coord board --json` now, or keep Rust reads and
  only share the component first?
- Should vimcode ever run `coord notify` itself, or stay a pure viewer and require the
  operator's existing cron/`watch`?
- Multi-repo board scoping inside vimcode (you're usually in one repo's checkout, but the
  board spans all coordinator repos).
