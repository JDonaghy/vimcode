# VimCode — Current Plan

> **Purpose of this file:** Session-level coordination doc for in-flight
> multi-stage features, so work can be picked up on a different machine
> without reconstructing state from scratch. GitHub issues remain the
> source of truth for individual tasks — this file points at the current
> wave and explains how to resume.
>
> **Last updated:** 2026-04-26 (Phase B.4 — TUI Backend trait implementation — kicks off. #223 arc and quadraui readiness gate are both complete; everything below shifts to executing the multi-stage TUI rewrite.)

---

## 🎯 CURRENT FOCUS — Phase B.4: TUI Backend trait implementation

The quadraui readiness gate is **clear** (all primitives shipped with
D6 `layout()` + `hit_test()`; cross-backend dispatch infrastructure
proven). Phase B.4 is the multi-session rewrite of vimcode's TUI
backend on top of `quadraui::Backend` + `UiEvent` + the dispatch
infrastructure. Each stage merges to develop on its own — `vcd`
keeps working at every commit. **No `tui_main_v2/` parallel tree.**
Refactor in place.

**Detailed stage-by-stage plan:** `~/.claude/plans/federated-hugging-hinton.md`
(Claude's planning workspace; copy to repo if needed for off-machine
pickup).

### Stage map

| Stage | Goal | Sessions |
|---|---|---|
| **1** | `TuiBackend` skeleton + frame ownership; stub `draw_*` delegating to existing free functions | 1–2 |
| **2** | Drawing methods route through trait; `quadraui_tui.rs` shims fold into backend | 1–2 |
| **3** | `paint<B: Backend>` extraction; cross-backend draws via trait, TUI helpers for editor / scrollbars | 1–2 |
| **4** | `poll_events()` / `wait_events()` translating crossterm → `UiEvent` | 2 |
| **5a** | Modal dispatch through `dispatch_mouse_down/up` (replaces inline modal hit-tests) | 1 |
| **5b** | Drag-state consolidation into `quadraui::DragState` (extends `DragTarget` enum) | 1 |
| **6** | Accelerator registry consolidation for cross-mode global keybindings | 1 |
| **7** | Cleanup + restore GTK / Win-GUI compile + `BACKEND.md` worked example | 0.5–1 |

**Realistic total:** 9–12 sessions. After Stage 7, the per-feature
wiring story collapses from "edit one site per backend" to "register
one accelerator + emit one `UiEvent`."

### What could re-shape the plan

Three discoveries would force re-scoping. **First**, if Stage 1 hits
borrow-checker pain holding `Terminal<CrosstermBackend<Stdout>>` across
trait method calls (real risk — `terminal.draw(|frame| { … })` only
yields `&mut Frame` inside the closure), `Backend` may need a
callback-style `with_frame` instead of `begin_frame` / `end_frame` —
that's a quadraui PR first. **Second**, if Stage 4 reveals missing
`UiEvent` variants (kitty-protocol focus, mouse-move-without-button,
etc.), each gap is a quadraui PR. **Third**, if Stage 5b's drag
shapes don't fit `DragTarget::ScrollbarY` (terminal split divider is
horizontal-axis, group divider is both axes), `DragTarget` needs
axis-aware variants — split 5b into 5b/5c.

If any fire: pause the stage, file the quadraui PR, land it, resume.
Don't paper over in TUI code — gaps get worse if patched there.

### Parallel cleanup work (not blockers)

Smoke-test follow-ups filed during the #223 arc — pick up alongside
B.4 stages when context aligns:

- **#225** — GTK tab switcher: rounded corners + bordered ListView support
- **#226** — Right-click "Open to the Side" v-splits current tab
- **#228** — GTK editor hover: heading bg incomplete
- **#229** — GTK editor hover: scrollbar leak (right-edge specific)
- **#230** — LSP "rust-analyzer..." indicator stuck
- **#231** — TUI rename: tree row stale tinting
- **#232** — Tab-click no longer highlights tree row (TUI + GTK regression)
- **#233** — GTK Dialog square corners (cross-backend visual divergence)
- **#234** — TUI menu-bar dropdown: hover routing gap
- **#236** — GTK ContextMenu border barely visible
- **#238** — TUI ContextMenu chrome overlaps trigger row in Above/Below placement
- **#239** — Anchor vimcode's status-bar pickers to their trigger segment
- **#241** — `quadraui_<backend>::run()` per-backend runner (post-B.4)

---

## Architectural focus

**North-star goal:** Get quadraui complete enough to **rewrite
vimcode on top of it**, then do that rewrite backend-by-backend:
**TUI first** (reference implementation — lowest-cost iteration, no
native deps), then **GTK**, then **Win-GUI**, then a fresh
**macOS native** backend that drops in clean because the contract
will be tight by the time it starts. The "coexistence rule" from
`UI_CRATE_DESIGN.md` §7 (each Phase A stage shipped alongside
existing code so nothing was ever half-migrated) is **superseded** —
D6 makes half-migration loud at the type level rather than silent,
and **vimcode has no external users yet**, so extended per-backend
breakage during the rewrite is acceptable. Secondary goal: prove
quadraui out for reuse (Postman-class app #169, k8s dashboard #145,
SQL client #46) — follows naturally once vimcode is landed cleanly
on top of it.

**Longer-horizon vision: vim-motion tool suite.** Once quadraui is
proven out and vimcode is solidly on top of it, the natural extension
is a **family of developer tools that share vim-motion editing + LSP
integration**. The SQL client (#46) is the first concrete instance,
but the pattern generalises: any tool with a text-editable field
(query editor, config editor, API request body, k8s manifest editor,
terraform planner, etc.) should feel like vim to someone who wants
it to. Architecturally this means:

- `vimcode-core` (Buffer + View + Engine modes + motion handlers +
  LSP client) extracted as a reusable library dependency
- Each app (SQL client, API client, k8s dashboard, …) reuses
  vimcode-core for its editable text regions and quadraui for
  everything else
- Shared modal-editing identity across the suite — learn once, use
  everywhere

This is explicitly **not a priority before B.5 (GTK rewrite)**; it's
captured here so the longer-term "why" stays in front of anyone
making scope decisions during the rewrites. When trade-offs appear
between "optimise for vimcode-the-editor" and "keep vimcode-core
extractable as a library," lean toward extractability — that's
where the compounding value is.

**State of the backends going in:** TUI is the most complete and
usable today; GTK and Win-GUI are full of bugs accumulated from the
coexistence-era band-aid cycle. All three get rebuilt on quadraui
during the rewrite; the goal is that after the rewrite, all four
backends (TUI + GTK + Win-GUI + macOS) have the same feature surface
and the same bug floor.

The `vimcode -t` flag (TUI mode in the GTK binary) is being retired —
the TUI binary is `vcd` only. One less entry point to migrate during
B.4.

**Wave:** quadraui Phase B (Backend trait + UiEvent + layout-owning
primitives). Phase A complete except optional Win-GUI parity stages
(no longer worth chasing — those backends get rebuilt anyway).

**Resolved this cycle (`quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` §9):**
- **D1–D5** (2026-04-22 morning): event/dispatch axis settled —
  ship `UiEvent` + `Accelerator` + `Backend` together; per-method
  draw (no `AnyPrimitive` enum); B.1 = scaffolding alone; B.2 pilot
  = terminal maximize; accept both vim-style and plus-style
  keybinding formats.
- **D6** (2026-04-22 evening): render/layout axis settled —
  primitives return fully-resolved `Layout`; backends rasterise
  verbatim. `Backend::draw_*` methods take `&Layout`, not
  `&Primitive`. Closes the structural gap that produced #178 and
  #179. Unblocks Phase B.3.
- **D7** (2026-04-23): focus model settled (five sub-decisions).
  Click+Tab+programmatic transitions; destruction falls back to
  app-designated default; focusability is a property of the
  primitive type; modal interactions use a backend-owned focus
  stack; native focus stays at the top-level with in-app simulation
  below. Iteration expected on edge cases. **All design-axis
  blockers for B.3 (ready-state quadraui) are now clear.**

**Open architectural questions (non-blocking):**
- §6.3 multi-window — defer to v1.x (vimcode + Postman are
  single-window).
- §6.5 IME — defer to v1.1.
- §6.6 performance — profile after first backend rewrite (TUI).

*Previously-blocking §6.4 focus model was resolved as D7 on
2026-04-23.*

**Quadraui readiness gate — what unblocks the TUI rewrite:**

*Design axes:*
- ✅ D1–D7 all resolved (event/dispatch + render/layout + focus).
- §6.3 / §6.5 / §6.6 deferred — don't block.

*Existing primitives with `layout()` + `hit_test()`:*
- ✅ All 9 shipped: `TabBar`, `StatusBar`, `TreeView`, `ListView`,
  `ActivityBar`, `Form`, `Palette`, `TextDisplay`, `Terminal`.

*New B.3 container primitives:*
- ✅ `Panel` (chrome + content_bounds)
- ✅ `Split` (two-pane draggable divider)
- ✅ `Modal` (backdrop + centered content)
- ✅ `Dialog` (title + body + buttons)
- ✅ `MenuBar` (top-level menu strip with Alt-nav)
- ⬜ `Tabs` — **skipped** as redundant with `TabBar` + app-owned
  content. Apps use `TabBar` for navigation and swap their content
  region based on `active_idx`; no composition primitive needed.
- ⬜ `Stack` — **skipped** as redundant with app rendering order
  (render overlays last). Z-stacking is trivially expressible in
  render order without a dedicated primitive.

*New B.3 surface primitives:*
- ✅ `ContextMenu`
- ✅ `Completions` (LSP-style autocomplete)
- ✅ `Tooltip` (anchor-relative placement)
- ✅ `Toast` (#141)
- ✅ `Spinner` + `ProgressBar` (#142)
- ✅ Form field primitives (#143): `Slider`, `ColorPicker`, `Dropdown`
  as `FieldKind` variants.

*TUI consumer migration (vimcode consumes layout() in real render paths):*
- ✅ `TabBar` (commits `ebe0eec` hit-test + `713f071` draw)
- ✅ `StatusBar` (commit `f263765` draw + hit-test)
- ⬜ `TreeView`, `ListView`, `ActivityBar`, `Palette`, `Form`,
  `TextDisplay`, `Terminal` — still on pre-D6 draw paths; migrations
  land during the B.4 TUI rewrite.

*Backend trait final shape:*
- ⬜ `Backend::draw_*(&Layout)` throughout — mechanical rewrite that
  happens during B.4 when backends are touched anyway.

**🎯 Readiness gate status: CLEAR for Phase B.4 (TUI rewrite).** All
primitives shipped with D6 `layout()` + `hit_test()`. TabBar +
StatusBar already consume layouts in TUI as proof of the pattern.
The remaining TUI consumer migrations are mechanical and can happen
incrementally during the rewrite.

**🎯 Phase B.4 event-routing status: PICKER SURFACE PROVEN
(Session 329).** The first cross-platform event-routing
infrastructure ships. `quadraui::ModalStack` + `dispatch_mouse_down`
arbitrates modal-vs-backdrop clicks; `DragState` +
`dispatch_mouse_drag` + `dispatch_mouse_up` handle drag translations
to primitive-specific events (e.g. `PaletteEvent::ScrollOffsetChanged`).
Both GTK and TUI route the picker modal's events through this
single code path; adding a third backend means consuming the same
dispatcher, not reimplementing it. Closes #190 (GTK palette
scrollbar was painted but not draggable) and #192 (GTK palette
click-drag leaked to editor). Follow-up commits extend the pattern
off the picker onto tab switcher, sidebar scrollbars, dialogs — each
a per-surface commit with zero quadraui changes.

**🎯 GTK rendering status: FOUR PRIMITIVES ON D6 (Session 329).**
`draw_status_bar`, `draw_list`, `draw_tree`, `draw_palette` all
consume their respective `*Layout` structs. Proves the D6 contract
works across char-cell (TUI) and pixel + Pango (GTK) coordinate
systems.

**🎯 Phase B.4 chrome status: SUBSTANTIALLY DONE (Session 328).**
Every major TUI overlay now renders through a quadraui primitive or
through shared cross-backend hit-region data:

- **Tooltip-backed:** LSP hover, signature help, diff peek (multi-line
  styled extension landed)
- **Dialog-backed:** modal dialogs, quit-confirm, close-tab confirm
- **ContextMenu-backed:** tab action menu, menu dropdown
- **ListView-backed (bordered):** tab switcher (`bordered: bool`
  extension landed)
- **Palette-backed:** folder picker, command palette (and quickfix
  panel via the flat ListView path)
- **StatusBar-backed:** debug toolbar, breadcrumb bar, editor status
  line
- **Shared hit_regions:** find/replace overlay (no primitive — the
  data shape itself IS the cross-backend contract)

Deferred for B.4 chrome (out of scope, see Session 328 notes):
- Tab drag overlay (doesn't fit primitive model; backend-specific)
- Menu bar row (composite chrome — labels + nav arrows + search;
  MenuBar primitive only covers the labels strip)
- Picker preview pane / tree-indented variant (needs Palette
  preview-pane support added)

*TBD:* `TextEditor` / `BufferView` — Phase A.9 was marked deferred
because vimcode's existing engine-owned text rendering path is still
adequate. Decide at TUI-rewrite-start whether the rewrite needs a
quadraui editor primitive or can keep the engine-owned path. Leaning
toward keeping engine-owned for TUI; revisit when GTK rewrite starts.

**Backend rewrite order (after readiness gate clears):**
1. **TUI** — smallest surface, no native deps, fastest iteration
   cycle. Stress-tests the quadraui contract. Rewrite discovers
   whatever's wrong with the crate before any native-backend pain.
2. **GTK** — primary Linux backend. Rewrite uses the TUI-proven
   primitives; any gaps that show up here feed back into quadraui
   and then forward to TUI.
3. **Win-GUI** — third opinion on the contract. By this point, most
   quadraui gaps should already be fixed.
4. **macOS native** — the "easy" one. Core Graphics + Core Text;
   the contract is tight by now, lessons encoded. Historically
   blocked by the parallel complexity of maintaining three other
   backends; under this plan, it's the last thing written.

**If you're new here, read in order before touching `quadraui/`:**
1. This section (you're reading it).
2. [`quadraui/docs/DECISIONS.md`](quadraui/docs/DECISIONS.md) — primitive-distinctness principles (~140 lines).
3. [`quadraui/docs/UI_CRATE_DESIGN.md`](quadraui/docs/UI_CRATE_DESIGN.md) §6 + §7 — backend responsibilities + key decisions.
4. [`quadraui/docs/BACKEND_TRAIT_PROPOSAL.md`](quadraui/docs/BACKEND_TRAIT_PROPOSAL.md) §9 — resolved decisions D1–D7 with full rationale.
5. The PLAN.md stage table below — what's shipped, what's next.

Skip steps 2–4 only if the work is purely within an already-migrated
backend draw function (no primitive contract changes, no new
primitives, no cross-backend behaviour).

---

## Active wave — `quadraui` cross-platform UI crate extraction

Extracting a reusable UI crate from vimcode per
[`quadraui/docs/UI_CRATE_DESIGN.md`](quadraui/docs/UI_CRATE_DESIGN.md). vimcode is the
test app; target downstream apps include a cross-platform k8s dashboard
(see issue [#145](https://github.com/JDonaghy/vimcode/issues/145)).

**Release baseline:** `v0.10.0` (on `main`).

**Development branch:** `develop` — all new work starts here.

### Stage map

| Stage | Status | Commit | Branch pattern | Platform needed |
|-------|--------|--------|----------------|-----------------|
| **Phase A.0** — workspace scaffold | ✅ Done | `36ccad3` | `quadraui-phase-a0-*` | any |
| **Phase A.1a** — `TreeView` primitive + TUI SC panel | ✅ Done | `bac137e` | `quadraui-phase-a1a-*` | any (TUI) |
| **Phase A.1b** — GTK `draw_tree` + GTK SC panel | ✅ Done | `e12601e` | `quadraui-phase-a1b-*` | Linux / macOS with GTK4 |
| **Phase A.1c** — Win-GUI `draw_tree` + Win-GUI SC panel | ✅ Done | `25e94f8` | `quadraui-phase-a1c-treeview-win-gui` | Windows |
| **Phase A.2a** — `TreeView` explorer (TUI) + `Decoration::Header` | ✅ Done | `1c4bbd7` | `quadraui-phase-a2a-*` | any (TUI) |
| **Phase A.2b-1** — GTK explorer scaffolding (data model + draw function, inert) | ✅ Done | `e34a72f` | `quadraui-phase-a2b-*` | any (compiles on all platforms) |
| **Phase A.2b-2** — GTK explorer atomic switchover (native `gtk4::TreeView` → `DrawingArea`) | ✅ Done | `26ed4e9` | `issue-152-a2b2-switchover-gtk` | Linux / macOS with GTK4 |
| **Phase A.2c** — Win-GUI explorer | ✅ Done | `f74594a` | `quadraui-phase-a2c-explorer-win-gui` | Windows |
| **Phase A.3a** — `Form` primitive + TUI `draw_form` | ✅ Done | `4a4b456` | `quadraui-phase-a3a-*` | any |
| **Phase A.3b** — TUI settings panel uses `Form` | ✅ Done | `e708e43` | `quadraui-phase-a3b-*` | any |
| **Phase A.3c** — GTK `draw_form` primitive (migration deferred) | ✅ Done | `3f34a03` | `quadraui-phase-a3c-*` | any |
| **Phase A.3c-2** — GTK settings panel uses `draw_form` (native → DrawingArea) | ✅ Done | `fa44a51` | `quadraui-phase-a3c2-*` | Linux / macOS with GTK4 |
| **Phase A.3d** — `TextInput` cursor + selection in `Form` | ✅ Done | `f7f3a51` | `quadraui-phase-a3d-*` | any |
| **Phase A.4** — `Palette` primitive + TUI command palette | ✅ Done | `534c386` | `quadraui-phase-a4-*` | any |
| **Phase A.4b** — GTK `draw_palette` + GTK command palette | ✅ Done | `c8f2d91` | `quadraui-phase-a4b-*` | Linux / macOS with GTK4 |
| **Phase A.5** — `ListView` primitive + TUI quickfix | ✅ Done | `63d1b29` | `quadraui-phase-a5-*` | any |
| **Phase A.5b** — GTK `draw_list` + GTK quickfix | ✅ Done | `e1ea5ea` | `quadraui-phase-a5b-*` | Linux / macOS with GTK4 |
| **Phase A.6a** — `StatusBar` primitive + TUI per-window status line migration | ✅ Done | `3b020ef` | `quadraui-phase-a6a-status-bar-tui` | any (TUI) |
| **Phase A.6b** — GTK `draw_status_bar` migration | ✅ Done | `96c48bf` | `quadraui-phase-a6b-status-bar-gtk` | Linux / macOS with GTK4 |
| **Phase A.6c** — `TabBar` primitive + TUI migration | ✅ Done | `2196b27` | `quadraui-phase-a6c-tab-bar-tui` | any (TUI) |
| **Phase A.6d** — GTK `draw_tab_bar` migration | ✅ Done | `e93b857` | `quadraui-phase-a6d-tab-bar-gtk` | Linux / macOS with GTK4 |
| **Phase A.6e** — `ActivityBar` primitive + TUI migration | ✅ Done | `2c89dcf` | `quadraui-phase-a6e-activity-bar` | any (TUI) |
| **Phase A.6f** — GTK ActivityBar native→DrawingArea migration | ✅ Done | `4d494a1` | `quadraui-phase-a6f-activity-bar-gtk` | Linux / macOS with GTK4 |
| **Phase A.7** — `Terminal` primitive + TUI + GTK cell migration | ✅ Done | `aab8668` | `quadraui-phase-a7-terminal` | any |
| **Phase A.8** — `TextDisplay` primitive scaffolding (no migration) | ✅ Done | `ff6b13f` | `quadraui-phase-a8-text-display` | any |
| Phase A.9 — `TextEditor` + `BufferView` adapter | ⬜ Deferred (not needed for vimcode) | — | `quadraui-phase-a9-*` | any — biggest stage |
| Win-GUI parity (A.6*-win, A.7-win) | 🚫 Abandoned — backend rebuilt in B.6 | — | — | — |
| **Phase B.1** — `UiEvent` + `Accelerator` + `Backend` trait scaffolding | ✅ Done | _tbd_ | `quadraui-phase-b1-backend-trait` | any |
| **Phase B.2** — pilot migration: terminal maximize to `Accelerator::Global` | ✅ Done | _tbd_ | `quadraui-phase-b2-maximize-pilot` | any |
| **Phase B.3** — layout primitives (`Panel`, `Split`, `MenuBar`, `Modal`, `Dialog`, `ContextMenu`, `Completions`, `Tooltip`, `Toast`, `Spinner`, `ProgressBar`) + D6 retrofit on all 9 existing primitives | ✅ Done | (Session 327, ~25 commits) | `quadraui-phase-b3-*` | any |
| **Phase B.4 chrome (TUI)** — every major TUI overlay migrated to a quadraui primitive or shared hit-region data | ✅ Substantially done (Session 328) | `4eacaa0` | `quadraui-{popup}-*` | any (TUI) |
| **Phase B.4 trait** — `TuiBackend` impl + `paint<B: Backend>` + `poll_events` translation. **🎯 Active wave** — see "🎯 CURRENT FOCUS" at top for stage breakdown (1–7) | ⬜ Stage 1 next | — | `quadraui-phase-b4-stage-{N}` | any (TUI) |
| Phase B.4 editor viewport (TUI) | ⬜ Deferred — chrome-only scope chose to leave `render::build_rendered_window` in place | — | `quadraui-phase-b4-editor-*` | any (TUI) |
| Phase B.5 — GTK rewrite (chrome → editor) | ⬜ After B.4 | — | `quadraui-phase-b5-*` | Linux / macOS with GTK4 |
| Phase B.6 — Win-GUI rewrite | ⬜ After B.5 | — | `quadraui-phase-b6-*` | Windows |
| Phase B.7 — macOS native rewrite | ⬜ After B.6 | — | `quadraui-phase-b7-*` | macOS |
| Phase B.8 — Postman-class validation app (#169) | ⬜ After B.4 | — | _new workspace member_ | any |
| Phase C — macOS backend | ⬜ v1.x | — | — | macOS |
| Phase D — polish + k8s validation app (#145) | ⬜ Later | — | — | any |

**Phase A is complete.** Optional Win-GUI parity stages (A.6*–A.7
for Windows) are no longer worth chasing — those backends get
rebuilt during B.5/B.6 anyway. Phase B is the active focus.

Design decisions covering primitive-distinctness (why `ListView` is
separate from `TreeView`, and how `DataTable` #140 should be scoped)
are documented in [`quadraui/docs/DECISIONS.md`](quadraui/docs/DECISIONS.md).

---

## Lessons learned during this wave

- **Adapters must preserve the flat-row count the engine expects.** The
  first draft of `source_control_to_tree_view()` added a `(no changes)`
  placeholder row for empty + expanded sections. That single extra row
  shifted the `sc.selected` (flat index) → `selected_path` (TreePath)
  mapping off by one, and `sc_flat_to_section_idx()` disagreed with the
  visual layout. Symptom: `Tab` and `Enter` acted on the wrong section;
  staging worked only because the file rows were always in non-empty
  sections. Fix (absorbed into `e12601e`): drop the placeholder. Rule:
  **any adapter row the engine doesn't count is a bug.** Backends that
  want an empty-state hint should render it as a visual detail that
  doesn't occupy a selectable row.

- **Flat-index selection mapping is the single biggest regression risk**
  in every backend migration. Always smoke-test keyboard nav (`j`/`k`)
  after touching an adapter. If the highlight visually lands on a
  non-header row but key behaviour says otherwise, the adapter has
  added or dropped a row.

- **DrawingArea-based sidebars need explicit `grab_focus` after the
  panel becomes visible.** A.3c-2 first shipped without this and the
  symptom was: clicking the activity bar opened the panel and the
  visual selection moved on click, but `j`/`k`/`/` keyboard input
  silently went nowhere. The activity-bar `gtk4::Button` keeps focus
  after click, the editor DA's key controller (capture phase, attached
  to the editor DA) does not fire for keys destined elsewhere, and
  the sidebar DA's own controller can't fire until something gives it
  focus. Fix (absorbed into the A.3c-2 commit): add the new panel to
  the per-panel `grab_focus` block in `Msg::SwitchPanel` *and* call
  `da.grab_focus()` in the panel's click handler. The same pattern
  already existed for SC / Extensions / Debug / AI; missing it for a
  new panel is a silent regression. Rule: **every new sidebar DA must
  appear in both the SwitchPanel grab-focus match and the click
  handler**, otherwise its key controller is dead code.

- **Rendering state belongs in the primitive; per-frame interaction
  state does not.** A.6d split the GTK tab bar migration by keeping
  `quadraui::TabBar` purely declarative (tabs + their flags + accent
  + right segments) and letting the GTK backend's `draw_tab_bar`
  accept a separate `hovered_close_tab: Option<usize>` parameter for
  the mouse-hover rounded-bg affordance. Plugin-declared tab bars
  still work because plugins don't need hover-overlay control — the
  backend computes hover from its own event stream and overlays
  visually. Rule: **if something can only be known by the backend
  (cursor position, focus-within, scroll momentum), pass it alongside
  the primitive rather than bloating the primitive with backend-owned
  state.**

- **Not every PUA glyph is 2 cells in a terminal.** First draft of
  `quadraui_tui::draw_tab_bar` (A.6c) treated every Private Use Area
  character (`U+E000..U+F8FF` + supplementary PUA) as wide and used
  `set_cell_wide` for them. That broke 6 snapshot tests: `SPLIT_DOWN`
  at `\u{f0d7}` is PUA but renders as 1 cell in practice, so the old
  code used plain `set_cell` for it. The fix was to narrow the
  wide-glyph predicate to an explicit allowlist of the 4 Nerd Font
  icons vimcode actually uses as wide (`F0932 F0143 F0140 F0233`).
  Rule: **wide-glyph treatment is per-glyph, not per-range.** When
  adding a new Nerd Font icon to a primitive, test empirically whether
  the terminal renders it as 1 or 2 cells and update `is_nerd_wide` if
  it's 2.

- **Win-GUI row heights stay uniform; GTK leaves are taller.** A.1c
  could have copied GTK's `line_height` / `line_height * 1.4`
  header/leaf split directly. It intentionally doesn't — the pre-
  migration Win-GUI SC panel used uniform `lh` rows everywhere, so
  the click-hit math in `src/win_gui/mod.rs` (which divides a mouse
  y offset by `lh` to get a flat row index) worked without per-row
  adjustment. Introducing a 1.4× leaf height would have silently
  broken that. Rule: **when porting a primitive's draw function to
  a new backend, match the new backend's pre-migration row cadence,
  not the other backend's.** Different backends are allowed to make
  different pixel-level decisions; the primitive only constrains
  data, not layout.

- **Branches are not automatically headers.** Early `draw_tree`
  implementations (TUI + GTK) applied section-header background styling
  to every branch row (any row with `is_expanded = Some(_)`). That was
  correct for SC (branches are section titles) but wrong for the
  explorer (branches are just directories and should look like sibling
  files). Fix (absorbed into `1c4bbd7`): added `Decoration::Header`.
  Apps tag header rows explicitly; backends style them distinctly.
  `is_expanded`-ness is now purely about chevron rendering. Rule:
  **tree hierarchy and visual emphasis are orthogonal.**

- **Generic-over-measurer is the cross-backend pattern.** When a
  rendering algorithm needs a "width" or per-element measurement
  (`fit X within Y`, `where does Z scroll to`, `which slice fits`),
  put the algorithm in `quadraui` parameterised over a measurement
  closure. Each backend supplies its native unit (TUI: char count,
  GTK: Pango pixel, Win-GUI: DirectWrite, macOS: Core Text). Two
  examples now live in the codebase as the established template:
  `quadraui::StatusBar::fit_right_start<F>` (#159) and
  `quadraui::TabBar::fit_active_scroll_offset<F>` (#158). The
  alternative — putting the algorithm in `Engine` with hardcoded
  TUI units — silently breaks every non-TUI backend with off-by-N
  layout bugs that look like timing bugs but aren't. **Default to
  the generic pattern from day one for any new "fit/scroll/elide"
  primitive logic.** See `docs/NATIVE_GUI_LESSONS.md` §12 for the
  detailed analysis.

- **GTK's `idle_add_local_once` doesn't fire reliably during
  continuous events.** When you need a follow-up paint after the
  current draw (because the current draw measured something that
  changes engine state), do the second paint inline within the
  same `set_draw_func` callback (overdraw in the same Cairo
  context) rather than scheduling via `idle_add`. During window
  drag-resize, GTK's idle queue is starved by resize events and
  the idle callback never fires until the drag ends — the user
  sees a stale frame the entire time. The two-pass-paint pattern
  in `src/gtk/mod.rs::set_draw_func` is the GTK equivalent of
  TUI's loop-iteration redraw and Win-GUI's WM_PAINT cycle. See
  `docs/NATIVE_GUI_LESSONS.md` §13 + §14.

- **Render-time effective values beat mutation-at-toggle-time.** The
  first draft of terminal maximize (#34) mutated
  `session.terminal_panel_rows` at the moment the user pressed
  `Ctrl+Shift+T` — the panel size was captured once. Window
  resizes afterwards didn't re-derive anything, so the panel
  stayed frozen at its toggle-time height while the window grew.
  Fix (commit `5bcb1bd`): the flag-only toggle +
  `Engine::effective_terminal_panel_rows(target)` pattern. Each
  backend calls the accessor **every frame** during layout; the
  `target` comes from current viewport geometry. Window resize
  → new target → new effective → panel tracks the window
  automatically. Rule: **if state affects per-frame rendering AND
  can be invalidated by backend events (resize, reflow, focus),
  store a flag, not a captured dimension. Expose an `effective_*`
  accessor that the render code calls every frame.** See
  `quadraui/docs/APP_ARCHITECTURE.md` for the full worked example.

- **Mouse hit-tests mirror draw-time geometry.** With the
  maximize refactor (above), a second category of bug bit twice:
  every backend site that read `session.terminal_panel_rows`
  (stored) for *hit-testing* was off-by-N when the panel was
  maximized, because the draw code was using the *effective*
  value. Clicks on the maximize / close / split / add buttons
  landed in dead space. Fixes: `1d7141a` (GTK), `507d63a` (TUI).
  Rule: **every backend site that computes a rect for hit-testing
  must use the same effective value the draw code used.** Grep
  for the raw field after every feature that introduces a new
  "effective" accessor. The count-and-replace model is dumb but
  reliable; every backend hit-test is a separate opportunity to
  miss a substitution.

- **Chrome arithmetic belongs in the engine, not in each backend.**
  Three backends × one "how many rows does the terminal get when
  maximized?" formula = three implementations that drift. Extract
  a `*ChromeDesc`-style struct in `src/core/engine/` that backends
  **fill with measurements**, then call a shared
  `.max_panel_content_rows()` method. Backends provide units
  (TUI cell-count, GTK Pango px / `line_height`, Win-GUI DirectWrite
  / `line_height`); the engine owns the subtraction. See
  `PanelChromeDesc` (introduced alongside this lesson) as the
  reference pattern. Rule: **backends provide measurements, not
  formulas.**

---

## Picking this up on another machine

### 1. Initial clone / sync

```bash
git clone git@github.com:JDonaghy/vimcode.git
cd vimcode
git checkout develop
git pull origin develop
```

Confirm tip matches the `bac137e` (or newer) commit recorded in the table
above. If newer, scan recent commits for any completed stage and update
this file.

### 2. Workspace layout

```
vimcode/
├── Cargo.toml            ← workspace root, also the `vimcode` package
├── quadraui/             ← workspace member (the new crate)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── types.rs      ← Color, Icon, StyledText, WidgetId, Modifiers,
│       │                  TreePath, SelectionMode, Decoration, Badge, TreeStyle
│       └── primitives/
│           ├── mod.rs
│           └── tree.rs   ← TreeView, TreeRow, TreeEvent
├── src/
│   ├── render.rs         ← `source_control_to_tree_view()` adapter lives here
│   ├── tui_main/
│   │   ├── quadraui_tui.rs     ← TUI `draw_tree` (reference implementation)
│   │   └── panels.rs            ← SC panel calls `quadraui_tui::draw_tree`
│   ├── gtk/              ← GTK backend; A.1b adds `quadraui_gtk.rs` here
│   └── win_gui/          ← Win-GUI backend; A.1c adds `quadraui_win.rs` here
└── docs/
    ├── UI_CRATE_DESIGN.md       ← authoritative design
    └── NATIVE_GUI_LESSONS.md    ← cross-backend bug patterns (read before Win/Mac work)
```

### 3. Build and test commands

Platform-agnostic:

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test --no-default-features   # required pre-commit/pre-release gate
cargo build
```

Platform-specific builds:

| Platform | Build target | Command |
|----------|--------------|---------|
| Linux / macOS GUI | `vimcode` (GTK) | `cargo build` (default `gui` feature) |
| Any | `vcd` (TUI) | `cargo build --bin vcd --no-default-features` |
| Windows native | `vimcode-win` | `cargo build --bin vimcode-win --features win-gui --no-default-features` |
| Windows lint | `vimcode-win` | `cargo clippy --features win-gui --no-default-features` |

**🚫 CRITICAL — NEVER run `cargo test` with `--features win-gui`.** It
spawns hundreds of real Win32 windows and locks up the machine. See
[`CLAUDE.md`](CLAUDE.md) Testing section for details.

### 4. Development workflow

See [`CLAUDE.md`](CLAUDE.md) "Development Workflow" for the full rules.
Summary:

1. Always branch off `develop` (never commit directly to `develop`).
2. Commit locally; do **not** push until the user has smoke-tested or
   explicitly waived smoke testing.
3. Two landing paths: (A) fast-forward merge + push for small/trivial
   changes; (B) push branch + PR for normal work. Default to B when unsure.

---

## Design invariants that must hold across all stages

From [`quadraui/docs/UI_CRATE_DESIGN.md`](quadraui/docs/UI_CRATE_DESIGN.md) §10
(plugin-driven UI invariants). Breaking any of these would force a breaking
quadraui API change when Lua plugins start declaring UI (see issues
[#146](https://github.com/JDonaghy/vimcode/issues/146) and
[#147](https://github.com/JDonaghy/vimcode/issues/147)).

1. **`WidgetId` is owned** (`String` / `Cow<'static, str>`) — not `&'static str`.
2. **Events are plain data**, not Rust closures.
3. **Primitive structs implement `Serialize + Deserialize`** so Lua tables
   can map via JSON.
4. **WidgetId namespacing** for plugin IDs (e.g. `"plugin:my-ext:send"`).
5. **No global event handlers** — every event references a `WidgetId`.
6. **Primitives don't borrow app state** (owned data or explicit `'a`
   lifetimes).

If you write a new primitive or extend an existing one, verify all six.

---

## Reference documents

| Doc | Purpose |
|-----|---------|
| [`quadraui/docs/UI_CRATE_DESIGN.md`](quadraui/docs/UI_CRATE_DESIGN.md) | Authoritative design. All 13 §7 decisions are resolved. Start here. |
| [`docs/NATIVE_GUI_LESSONS.md`](docs/NATIVE_GUI_LESSONS.md) | Cross-backend bug patterns — read before A.1c. |
| [`CLAUDE.md`](CLAUDE.md) | Project-wide rules, quality gates, branching workflow. |
| [`PROJECT_STATE.md`](PROJECT_STATE.md) | Session-by-session progress (historical). |
| GitHub milestone [`Cross-Platform UI Crate`](https://github.com/JDonaghy/vimcode/milestone/5) | Tracking issues for backlog primitives and validation apps. |

## Relevant GitHub issues

- [#133](https://github.com/JDonaghy/vimcode/issues/133) — Unified sidebar rendering via ScreenLayout (subsumed by quadraui; may close when A.1 complete across all backends)
- [#139](https://github.com/JDonaghy/vimcode/issues/139) — `TreeTable` primitive (v1 must-have, needed by k8s app)
- [#140](https://github.com/JDonaghy/vimcode/issues/140) — `DataTable` (decide: standalone or TreeTable-depth-0)
- [#141](https://github.com/JDonaghy/vimcode/issues/141) — `Toast` primitive
- [#142](https://github.com/JDonaghy/vimcode/issues/142) — `Spinner` + `ProgressBar` (v1 must-have)
- [#143](https://github.com/JDonaghy/vimcode/issues/143) — Form fields: Slider, ColorPicker, Dropdown
- [#144](https://github.com/JDonaghy/vimcode/issues/144) — Live-append `TextDisplay` streaming (v1 must-have)
- [#145](https://github.com/JDonaghy/vimcode/issues/145) — k8s dashboard validation app (Phase D)
- [#146](https://github.com/JDonaghy/vimcode/issues/146) — Lua plugin API for quadraui primitives
- [#147](https://github.com/JDonaghy/vimcode/issues/147) — Postman-like bundled extension (depends on #146)

---

## Updating this file

Update `PLAN.md` at the end of any session that advances a stage:

1. Mark completed stages ✅ and fill in the commit SHA.
2. If a stage's scope changed during implementation, note it.
3. Update the "Last updated" date at the top.
4. If the active wave finishes, mark it so and move the whole section
   into a historical/completed list (or delete; git retains history).
