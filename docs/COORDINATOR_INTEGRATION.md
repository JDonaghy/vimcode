# Agent Cockpit — the Coordinator Client and the ACP Agent Host

> **Status:** Design / pre-implementation — _2026-06-12, restructured 2026-09-13_
> **Repos:** `JDonaghy/vimcode` (host), `JDonaghy/quadraui` (shared components), `JDonaghy/claude-coordinator` (data + pipeline brain)
> **Roadmap:** milestone [`vimcode-coordinator`](https://github.com/JDonaghy/vimcode/milestone/6) · epic [#531](https://github.com/JDonaghy/vimcode/issues/531) (live tracker + dependency graph)
>
> **This doc covers two independent tracks.**
> **Track A — coordinator client (§1–§9):** host the coord board in vimcode and review
> remote fleet work as real code. **Track B — ACP agent host (§10):** run a live
> [ACP](https://agentclientprotocol.com) agent session *inside* vimcode.
> They share two components (§10.5) and nothing else. **Neither blocks the other.**
>
> **Both of Track A's external pre-reqs are now CLOSED** — see §13. This doc read as
> "blocked on quadraui#362 and coord#550" for months after both had landed.
>
> **Standing constraint (owner decision, 2026-09-13): no coordinator dependency in vimcode
> core.** A vimcode *extension* that requires `coord` is fine; the editor requiring it is
> not. `src/core/` and `src/render.rs` carry **no coord subcommand names, no coord schema,
> no coord lifecycle vocabulary** — see §6. Track B (§10) has no coordinator dependency of
> any kind.

## 1. Goal — client parity (Track A)

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

Track B (§10) applies that same pivot at a different distance from the work: rather than
reviewing a *remote* agent's finished branch, it hosts a *local* agent session live, with
tool calls surfaced and permission prompts answered as they happen. Track A is the fleet;
Track B is this checkout.

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
- **vimcode** hosts it in the Board panel (§7).

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

Two rules govern placement, and they compose.

**The platform-neutrality rule** (`CLAUDE.md`): shared logic in `render.rs`/engine, **1–3
lines of wiring per backend**, no bespoke GTK/TUI board code.

**The no-coord-in-core rule** (owner decision, 2026-09-13): vimcode is not a coordinator
client — it is an editor that can *host* one. `src/core/` and `src/render.rs` contain **no
`coord` subcommand names, no coord JSON schema, no coord lifecycle or gate vocabulary**.
vimcode must build, run and pass its full suite on a machine with no `coord` installed and
no coordinator config, **with no feature flag needed to achieve that**. Every coord-specific
fact lives in the coordinator extension bundle.

The consequence: what was designed as a coord-aware `coord_client.rs` compiled into the
binary becomes a **generic external-tool seam** (#522), and the board panel becomes a
**generic host** (#521).

- **`src/render.rs`** — `BoardData` built from **vimcode's own board contract** (§6.1),
  handed to `quadraui::Board`. New `ScreenLayout.board` slot, like `ext_sidebar`.
- **`src/core/tool_client.rs`** — a `ToolClient` trait: spawn a configured argv, capture
  stdout, parse JSON, map failure modes to typed errors. **Knows nothing about
  coordinator.** Mockable, so every consumer is testable with no provider installed.
- **`src/core/`** — engine fields for the Board panel (selection, focus, last fetched
  model, poll receiver), fed through `ToolClient`.
- **`src/gtk/` + `src/tui_main/`** — register the **Board** activity entry and draw
  `quadraui::Board`. Click/key → `BoardAction` → engine → the configured provider command.
  (The panel is *Board*, not *Coordinator*: a generic host is not named after one provider.)
- **Coordinator extension bundle** — the *packaging and glue*, and **the only place `coord`
  is named**: declares the board-provider command and poll interval in its manifest,
  registers its activity entry and the `:Coord*` commands (`:CoordRefine N`,
  `:CoordReview <id>`, `:CoordDispatch …`). It does **not** render the board (shared
  component) and holds **no** pipeline logic.

### 6.1 The board contract, and who adapts to it

vimcode defines the JSON shape it renders — `BoardModel` / `BoardColumn` / `BoardCard` plus
stage badges, matching quadraui's `Board` (quadraui#638). **The contract is vimcode's, not
coordinator's.** Any provider emitting that shape gets a board.

Coordinator's daemon emits *coord's* schema (`GET /board`, port 7435), so something must
adapt one to the other — and it must not be vimcode core. The design decision is open in
#522, with three candidates: a **shim shipped in the extension bundle** (recommended — the
manifest's command is whatever the bundle wants, so all coord knowledge stays there); a
**declarative mapping** in the manifest; or coordinator emitting vimcode's shape natively
(**rejected** — that is the same coupling pointing the other way).

Note that the Lua API has **no subprocess execution** today (`EXTENSIONS.md` §Lua Plugin
API), which is why the adapter is a script-plus-manifest rather than a Lua function.
Widening the Lua API so extensions can spawn processes and host panels is a real
alternative — it would let provider integrations be *pure* extensions — but it is
deliberately out of scope for #522, and it is a security surface worth designing on its own.

## 7. The Board panel

A new activity-bar entry — **Board** — alongside Explorer / Search / Source Control / Run /
Extensions. Selecting it shows whatever board its configured provider supplies (the shared
`quadraui::Board` component) in the sidebar or a full editor-area surface. Reuses the
activity-bar + panel machinery vimcode already has (the SC and Extensions panels are the
template — `TuiPanel`, `ext_sidebar`, `PanelRegistration`).

Named **Board** rather than *Issues* or *Coordinator* per §6: the host is generic, and with
no provider configured it says so rather than implying a missing coordinator. The
coordinator extension may label its own entry however it likes.

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

## 10. Track B — the ACP agent host

> Added 2026-09-13. Everything above (§1–§9) is **Track A**: vimcode as a *coordinator
> client*, driving remote fleet work. This section is **Track B**: vimcode as an *agent
> host*, running a live agent in this checkout. The two tracks share two components and
> nothing else (§10.5). **Track B does not depend on Track A.**

### 10.1 Goal

Speak [ACP](https://agentclientprotocol.com) — the Agent Client Protocol, Zed's
editor↔agent JSON-RPC protocol, also adopted by JetBrains (beta since 2025.3) — so that
**any** ACP-compatible agent runs in a live session inside vimcode: streamed output, tool
calls surfaced as they happen, permission prompts answered by a human, and proposed edits
landing in real buffers with LSP and diagnostics live.

Track A's thesis is *review the agent's work after the fact, as real code*. Track B's is
*watch and steer it while it happens, in the same editor*. Both are the same pivot — less
agentic, more hands-on — applied at different distances from the work.

### 10.2 The panel already exists

The scoping surprise, and the reason this track is cheaper than it looks: vimcode already
ships an AI chat panel, on the wrong transport.

`src/core/ai.rs` is a blocking `curl` client for Anthropic / OpenAI / Ollama behind an API
key. Everything layered above it is already backend-neutral and is exactly what an ACP
client needs:

| Piece | Where |
|---|---|
| `quadraui::ChatController` (`push_turn_markdown`, `set_busy`, `set_spinner_frame`) | `quadraui/src/compose/chat_controller.rs:168` |
| held as `Engine::ai_chat` | `src/core/engine/mod.rs:3513` |
| `PANEL_AI` activity-bar entry | `src/core/engine/sidebar.rs:17` |
| `ai_send_message` / `poll_ai` / `dispatch_ai_chat_event` | `src/core/engine/ext_panel.rs:2870-2965` |
| `render::route_ai_chat_event` | `src/render.rs:6691` |
| backend wiring, 1–3 lines each | `src/app.rs:3105-3107`, `src/tui_main/panels.rs:1095-1098` |

So Track B is largely a **transport swap behind an existing panel**, not a new UI surface.
`ai.rs` and the API-key requirement documented at `README.md:731` are retired once
agent-neutrality is proven.

### 10.3 Why `lsp.rs` is the model but not the reuse

vimcode already hand-rolls JSON-RPC-over-stdio twice — `src/core/lsp.rs` and
`src/core/dap.rs`. ACP is a third instance of the same shape, but two specifics do not
carry over, and both matter:

1. **Framing differs.** ACP is newline-delimited JSON — one UTF-8 message per line on
   stdio, stderr free for logs. LSP's `Content-Length` framing (`encode_message`
   `src/core/lsp.rs:510`, `parse_content_length` `:518`) is precisely the part that cannot
   be reused.
2. **There is no agent→client request path.** `src/core/lsp.rs:1519-1532` blanket-answers
   every server→client request with `result: null` without reading the method — which is
   why `workspace/applyEdit` is silently dropped today. An ACP client is *dominated* by
   agent→client requests (`session/request_permission`, `fs/read_text_file`,
   `fs/write_text_file`), and several must **park** until a human answers. That dispatch
   table plus an async reply path is the one genuinely new transport component.

What does carry over is the *pattern*: spawn + `setsid` + stderr ring
(`src/core/lsp.rs:916-990`), reader thread generic over `impl IoRead` (`:1434`),
`next_request_id` + `pending_requests` correlation (`:1105-1137`), and — load-bearing here
in a way it is not for LSP — stdin as `Arc<Mutex<Box<dyn Write + Send>>>` **shared with the
reader thread** (`:897-911`), which is how a parked request gets its reply written later.
(`src/core/dap.rs:104`,`:323` uses a `BufWriter` *not* shared with its reader, which is
exactly why the DAP client cannot answer adapter requests. Do not repeat that.)

**Decision: do not extract a shared `JsonRpcTransport` first.** Refactoring LSP's
heavily-tested transport to serve a third consumer is a large change with its own risk.
Build a new generic module for ACP now; retrofit LSP and DAP onto it later if it earns its
keep.

### 10.4 Standing commitments

- **Pin `protocolVersion: 1`.** Spec 1.7.0 is stable; a draft v2 restructures capabilities
  and drops `fs/*` and `terminal/*` in favour of MCP-over-ACP. Keep the client-*served*
  methods behind a trait so that migration is one impl swap.
- **Do not take the `agent-client-protocol` Rust SDK.** It is async/closure-shaped (2.x,
  edition 2024) and broke its whole API once already; vimcode has zero tokio and drives
  everything off a ≤250ms sync tick. Hand-roll NDJSON in the `lsp.rs` idiom.
- **Skip `terminal/*`.** Optional in v1, removed in the v2 draft, and a useful client
  (Neovim's CodeCompanion) ships without it. Note that `auth.terminal` (§10.6) is a
  different capability that shares the word.
- **Platform-neutrality.** All of it in `src/core/acp.rs`, an `Engine::poll_acp()` lane in
  `poll_idle()` (`src/core/engine/mod.rs:4415-4452`), and `render.rs` slots on existing
  quadraui primitives. Adding the poll lane is one field, one function, one call site —
  GTK (`src/app.rs:2512`) and TUI (`src/tui_main/shell_app.rs:2973`) already drive `tick`.
  **No new `src/gtk/` or `src/tui_main/` code.** Cap events per tick the way
  `lsp_manager.rs:994-1017` caps LSP at 50; chunk streams are high-rate.
- **A fake NDJSON agent is the test fixture.** CI has no Node and no agent login, so no
  slice may depend on a real adapter to prove itself. Every slice ships TUI `TuiDriver` +
  GTK `GtkDriver` black-box tests against the fake.

### 10.5 Where the two tracks touch

Two shared **components** — not shared issues. #522's generic `tool_client.rs` seam is unrelated:
it is a one-shot `coord … --json` subprocess, not a persistent session, and is not a fourth
JSON-RPC copy.

| Surface | Track A | Track B | Resolution |
|---|---|---|---|
| **Change review** | #525 — a *git branch* diff, already committed | #955 — a *proposed* `diff{path,oldText,newText}` tool call, revocable | **One** surface on `quadraui::DiffView` (`quadraui/src/primitives/diff_view.rs:413` — present and **unused by vimcode today**), fed by a **source-agnostic** change list. vimcode's current two-real-windows diff model (`src/core/engine/buffers.rs:1023-1154`) is right for comparing checked-out files and wrong for accept/reject of a proposal. Whichever issue lands first builds it; the second consumes it. Neither subsumes the other. |
| **Streaming panel** | #529 — coord's worker log + plan preview over SSE | #952 / #956 — `session/update` chunks and `plan` | Same primitive (`ChatController` / `quadraui::MessageList`), different feeders. Keep the plan model source-agnostic. |

Two corrections to §11/§12 that fall out of this: #529's stated "adopts the quadraui
terminal primitive" dependency is **already satisfied** (`terminal` is in vimcode's feature
set, `Cargo.toml:175`), and high-rate chunk streaming may need the live-append gap tracked
as vimcode#144 — a **quadraui** issue to file, never a `render.rs` workaround.

### 10.6 Auth, and the billing question

The Claude path is `@agentclientprotocol/claude-agent-acp` (Node ≥ 22, wrapping the Agent
SDK). It exposes a `type: "terminal"` auth method `claude-ai-login` — **Claude
subscription, not an API key** — but only if the client advertises
`clientCapabilities.auth.terminal: true`. Terminal auth stabilized in spec 1.7.0
(2026-08-20). Its `--hide-claude-auth` flag forces API-key-only; vimcode should not use it.

Terminal auth is *not* the `authenticate` RPC: the client runs the adapter as an
interactive process for the user to log in, then re-initializes.
`quadraui::terminal_engine::TerminalSession` (`quadraui/src/terminal_engine.rs:456`) is
already available to vimcode and sufficient — note its `spawn` takes a shell path rather
than an argv, which vimcode already works around (`src/core/engine/terminal_ops.rs:129-159`).

Treat "subscription works via ACP" as **currently true, not guaranteed** — Anthropic's
metering policy moved twice in 2026. Keep the API-key path working as a fallback.

**Claude Code itself has no native ACP** (no `--acp` flag as of 2.1.270). It does expose
`--input-format stream-json` with `--permission-prompts host`, a bidirectional non-ACP
protocol — a Node-free fallback **for Claude only**, at the cost of the agent-neutrality
this track exists for. A contingency, not a plan.

### 10.7 Phased plan — Track B

Rooted at #951, independent of Track A. Epic [#531](https://github.com/JDonaghy/vimcode/issues/531) is the live tracker.

- **ACP-0 — `acp.rs`**: NDJSON transport, bidirectional dispatch with async reply,
  `initialize` / `session/new` / `session/prompt` / `session/cancel`, `poll_acp` lane, and
  the fake-agent fixture. **[#951]** *(blocks all of Track B)*
- **ACP-1 — live session behind the AI panel**; `agent_message_chunk` /
  `agent_thought_chunk` → `ChatController`. **[#952]**
- **ACP-2 — `session/request_permission`** → `Engine::show_dialog`, with cancel-on-dismiss
  and `session/cancel` wired from day one. **[#953]** *The safety-critical slice.*
- **ACP-3 — serve `fs/read_text_file` / `fs/write_text_file`** through buffers (reads must
  serve **unsaved** content), fixing the no-undo closed-file `fs::write` hole at
  `src/core/engine/panels.rs:1981-2013`. **[#954]** *Ship with #953, never before it.*
- **ACP-4 — tool calls + the proposed-change review surface** on `DiffView`. **[#955]**
- **ACP-5 — plan / slash commands / modes / usage**. `plan` updates are **full
  replacement**, not deltas. **[#956]**
- **ACP-6 — `auth.terminal`** subscription login. **[#957]**
- **ACP-7 — a second agent with zero new Rust** — the neutrality proof. **[#958]**

```
#951 ─┬─ #952 ─┬─ #953 ─┬─ #955 ─┐
      │        ├─ #954 ─┘        ├─ #958
      │        └─ #956           │
      └─ #957 ──────────────────-┘
```

**Shortest line to daily use:** #951 → #952 → #953 + #954 — a working, supervised,
agent-neutral in-editor session.

**ACP-8, not yet filed — the coordinator seat.** Where both tracks converge: a
coord-dispatched human-attended session hosted *as an ACP session* in vimcode, verdict via
`coord report-result`. This is the strongest form of coordinator's own `GOAL.md` north star
("make human-attended interactive sessions drivable end-to-end"). Scope it only once #955
and #526 exist.

### 10.8 Relationship to claude-coordinator#1226

Coordinator has its own ACP epic, and the two are **complementary rather than
overlapping**. coord#1226 makes `coord` a *headless* ACP client where
`session/request_permission` becomes the chokepoint **coord's deny-list vetoes at** —
a capability it does not otherwise have at any price. It lists "ACP as an IDE/editor
integration surface" as an explicit non-goal, and its bridge is Python.

Track B is the editor side, where that same method routes to **a human**. Same protocol,
deliberately opposite policy, no shared code. **vimcode must not grow a deny-list or a
policy engine.**

One correction worth carrying back to #1226: its 2026-07-16 finding that the Claude ACP
adapter requires `ANTHROPIC_API_KEY` is now stale (§10.6).

## 11. Divergence & risks

- **Triple-render divergence** — mitigated by the shared component (one renderer).
- **Triple-projection divergence** — mitigated by `coord board --json` (one projection).
  Interim: if coord-tui keeps its Rust projection while vimcode uses JSON, the *component*
  is still shared; converge the projection later.
- **Pipeline freeze** — no daemon means `coord notify` must run; vimcode can drive it on a
  timer, but only opt-in (a passive viewer shouldn't silently dispatch metered work).
- **Worktree locality** — review-where-the-code-is (ssh) vs pull-local (`coord pull`).
  Support both; the issue→branch→files mapping is the core data the extension needs.
- **vimcode ↔ coord coupling** — bounded by the no-coord-in-core rule (§6). The
  *extension* requires a `coord` install on PATH; **the editor does not**. `core` sees only
  the generic `ToolClient` seam and vimcode's own board contract, and the constraint is
  enforced mechanically (#522 asserts that `src/core/` and `src/render.rs` carry no
  coordinator vocabulary) rather than by reviewer vigilance. Earlier drafts of this doc
  called `core` "pure" while placing a coord-aware client inside it — that is the drift the
  rule exists to prevent.

## 12. Phased plan — Track A

_Issue numbers in brackets; epic [#531](https://github.com/JDonaghy/vimcode/issues/531) is the live tracker.
Track B's plan is §10.7._

- **Foundation** — generic external-tool JSON seam (`tool_client.rs`), no coord in core
  (§6). **[#522]** _(blocks all of Track A)_
- **Phase 0 — read-only board panel** — generic **Board** activity entry renders the shared
  component from whatever provider an extension declares. **[#521]** _(needs #522;
  quadraui#362 and coord#550 are both closed — see §13)_
- **Phase 0b — board actions** — wire `BoardAction` → `coord` (dispatch/test/review/merge).
  **[#523]**
- **Phase 1 — issue authoring as buffers** — `:CoordRefine`, `:w` pushes. **[#524]**
- **Phase 2 — in-editor diff review** — `OpenReview` → multi-file diff tab, hunk nav. **[#525]**
  → verdict via `coord report-result`. **[#526]** *The differentiator.*
  _(#525 shares its change-review surface with #955 — see §10.5.)_
- **Phase 3 — hands-on** — inline comments → findings **[#527]**; human edits pushed back
  to the branch **[#528]**.
- **Phase 4 — observability** — live worker-log stream, plan preview, growing diff. **[#529]**
  _(the "adopts the quadraui terminal primitive" dependency is already satisfied: `terminal`
  is in vimcode's feature set, `Cargo.toml:175`. Shares its streaming panel with §10.5.)_
- **Phase 5 — remote / terminal** — vimcode-over-ssh as the fleet's review seat. **[#530]**

## 13. Pre-req issues — both external ones are CLOSED

Track A was filed behind two external blockers. **Both have landed. Track A is startable
today**; the only remaining foundation is in-repo (#522).

- **[quadraui#362](https://github.com/JDonaghy/quadraui/issues/362)** — reusable `Board`
  component. ✅ **Closed** — shipped as quadraui#638.
- **[claude-coordinator#550](https://github.com/JDonaghy/claude-coordinator/issues/550)** —
  machine-readable board projection. ✅ **Closed / rescoped.** The literal `coord board
  --json` ask is moot: the board daemon's `GET /board` (port 7435) already ships
  OpenAPI-documented JSON (coord#757). #550 was rescoped to moving coord-tui's *Rust gate
  projection* server-side, which is a coord-tui concern, **not a vimcode dependency**.
  Wherever this doc says `coord board --json` (§5, §8, §11), read `GET /board`.
- **[vimcode#521](https://github.com/JDonaghy/vimcode/issues/521)** — generic **Board**
  activity-bar panel hosting the quadraui Board, with the coordinator extension as one
  provider. Open; in-repo; needs #522.

## 14. Open questions

- Sidebar vs full editor-area board in vimcode (or both, like a maximizable panel)?
- Does coord-tui migrate its projection to `coord board --json` now, or keep Rust reads and
  only share the component first?
- Should vimcode ever run `coord notify` itself, or stay a pure viewer and require the
  operator's existing cron/`watch`?
- Multi-repo board scoping inside vimcode (you're usually in one repo's checkout, but the
  board spans all coordinator repos).

**Track B:**

- Does `src/core/ai.rs` survive as a direct-provider escape hatch, or is it deleted once
  #958 proves agent-neutrality? (Deferred decision, recorded in #952.)
- Which agent does #958 use for the neutrality proof — Gemini CLI or OpenCode? Both are
  native ACP servers, so neither confounds the proof with an adapter.
- Does ACP's transport eventually absorb LSP and DAP (§10.3 defers this deliberately), or
  do all three stay separate?
- When ACP v2 lands and `fs/*` moves to MCP-over-ACP, does vimcode follow, or pin v1 until
  the ecosystem forces the move?
