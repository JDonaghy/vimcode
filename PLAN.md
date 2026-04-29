# VimCode — Current Plan

> **Purpose of this file:** Session-level coordination doc for in-flight
> multi-stage features, so work can be picked up on a different machine
> without reconstructing state from scratch. GitHub issues remain the
> source of truth for individual tasks — this file points at the current
> wave and explains how to resume.
>
> **Last updated:** 2026-04-29 (Session 342 — **Phase C Stage 1 ([#276](https://github.com/JDonaghy/vimcode/pull/284)) shipped end-to-end**: `quadraui::Editor` primitive + dual TUI/GTK rasterisers landed in 4 sub-commits + fmt fixup on `issue-276-editor-primitive`. Net **−1456 LOC** of vimcode-private paint code; **+1972 LOC** of shared paint in quadraui ready for B.6 Win-GUI consumption. The editor viewport is now lifted; the only TUI/GTK duplication left is in three sidebar surfaces (#280 / #281 / #282), the rich hover popup (#214), and GTK's not-yet-migrated completion popup. See "🎯 NEXT FOCUS" below.)

---

## 🎯 NEXT FOCUS — Eliminate remaining TUI/GTK duplication

After Stage 1, **the editor viewport itself is no longer duplicated**.
What's left is three sidebar surfaces still hand-rolled per backend,
one rich-document popup that needs a new primitive, and one popup
that's lifted on TUI but not yet on GTK. Each is a discrete lift,
none blocks the others.

### The remaining duplication, ranked by ease × payoff

| # | Surface | Issue | Effort | Why next |
|---|---|---|---|---|
| 1 | **GTK `Completions` popup** | (no issue yet) | small (~1 day) | TUI already on `quadraui::Completions`; GTK still bespoke. Smallest remaining lift, eliminates one full duplication. |
| 2 | **Extension panel** | [#280](https://github.com/JDonaghy/vimcode/issues/280) | ~12 hrs | Extends `TreeView` with section headers; data shape is straightforward. Mechanical port once the primitive accepts headers. |
| 3 | **Editor hover popup (RichTextPopup)** | [#214](https://github.com/JDonaghy/vimcode/issues/214) | medium (new primitive) | Needs **new primitive** that handles markdown + code-hl + selection + scroll + links. Higher design cost than the chrome lifts but eliminates the most LOC of bespoke duplication still left after Stage 1. |
| 4 | **Debug sidebar** | [#281](https://github.com/JDonaghy/vimcode/issues/281) | ~16 hrs | 4 sections + per-section scrollbars; hand-rolled hit-test (#210/#211 baggage). Best done after #214 since the section-shape may inform a `MultiSectionView` primitive. |
| 5 | **Source control panel** | [#282](https://github.com/JDonaghy/vimcode/issues/282) | ~24 hrs | Complex due to inline commit-message editing. Likely consumes the same `MultiSectionView` shape that #281 designs. |

**Recommended order:** GTK `Completions` (warm-up) → #280 (mechanical
TreeView extension) → #214 (design pass on RichTextPopup) → #281 → #282.
The sequencing is "smallest lifts first to bank quick wins; hardest
last when patterns are established."

### Strategic complement — B.6 Win-GUI rebuild

[B.6](#phase-b6--win-gui-rebuild) is **unblocked** now that the editor
primitive is lifted. Win-GUI rebuild on quadraui is the natural
multi-week project once the TUI/GTK duplication is fully drained.
**B.6 doesn't eliminate TUI/GTK duplication** — it adds a third
backend that consumes the same primitives — so it sits orthogonal
to the focus above. Pick it up after the chrome lifts, or in
parallel if a Win-GUI session opens up.

### Independent quality work (can land any time)

Pre-existing or smoke-test follow-ups, none blocking the duplication-
elimination arc:

- [#262](https://github.com/JDonaghy/vimcode/issues/262) — Breadcrumb dropdown: parent symbols expandable but not jumpable.
- [#263](https://github.com/JDonaghy/vimcode/issues/263) — TUI breakpoint dot missing in gutter.
- [#264](https://github.com/JDonaghy/vimcode/issues/264) — Settings panel renders broken when sidebar narrow.
- [#265](https://github.com/JDonaghy/vimcode/issues/265) — TUI nerd-font wide-glyph predicate disagrees with terminal.
- [#272](https://github.com/JDonaghy/vimcode/issues/272) — GTK go-to-definition link click in focused hover does nothing.
- [#273](https://github.com/JDonaghy/vimcode/issues/273) — GTK cairo dialog spawns without keyboard focus until → pressed.
- [#274](https://github.com/JDonaghy/vimcode/issues/274) — Inventory + replace remaining native gtk4::Dialog widgets.
- [#283](https://github.com/JDonaghy/vimcode/issues/283) — TUI: LSP diagnostic dot overwrites breakpoint marker (gutter column collision; verbatim-port behaviour from #276 surfaced during smoke).

### Pickup checklist for any of the above

1. **Read `quadraui/docs/DECISIONS.md`** and
   **`quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` §9** — primitive-
   distinctness principles + resolved decisions log. Required per
   `CLAUDE.md` for any work that touches `quadraui/`.
2. **Look at the most-recent primitive lift as a template:**
   - `quadraui/src/primitives/editor.rs` (#276) for primitive shape +
     unit-test pattern + serde-friendly types.
   - `quadraui/src/{tui,gtk}/editor.rs` (#276) for verbatim-port
     rasterisers with private helpers.
   - `src/render.rs::to_q_editor` (#276) for boundary-adapter shape.
3. **Verbatim port first; redesign later.** Stage 1 of #276 hardened
   this rule: lift the data shape from the engine-side IR
   field-for-field, then iterate. The Stage 2 scrollbar caveat (TUI
   and GTK had subtly different math) applies — diff each paint
   category before assuming the two backends draw the same shape.

## ✅ Phase C — quadraui duplication cleanup (closed)

Phase C umbrella: [#275](https://github.com/JDonaghy/vimcode/issues/275).
All four numbered stages shipped:

- **Stage 1** ([#276](https://github.com/JDonaghy/vimcode/pull/284), Session 342, `3fcc7fb`/`ef45610`/`c985d58`/`5b23718`/`8c8cd24`) — `quadraui::Editor` primitive + dual rasterisers. ~470 LOC TUI + ~720 LOC GTK paint sites collapsed to ~25-line delegators. ~29 new `Theme` fields. `q_theme()` adapter split into `q_theme_chrome` + `q_theme_editor`. 290 quadraui tests pass (was 287, +3 editor).
- **Stage 2** ([#277](https://github.com/JDonaghy/vimcode/issues/277), `fbbc85f`/`b952c6a`/`d3abb17`/`2cc2ad9`) — `quadraui::Scrollbar` primitive + dual rasterisers + visible-track q_theme mapping + page-jump on track click. GTK fixes for native v-scrollbar trough visibility, viewport-sized page step, and h-scrollbar position above per-window status line.
- **Stage 3** ([#278](https://github.com/JDonaghy/vimcode/issues/278), `fd08db0`) — `quadraui::{tui,gtk}::draw_settings_chrome` helpers. Settings panel header + search row paint through quadraui; form body already did via `Form`.
- **Stage 4** ([#279](https://github.com/JDonaghy/vimcode/issues/279), `8e55720`) — `quadraui::MessageList` primitive + dual rasterisers. AI sidebar message-history paint loop lifted; header / separator / input area / focus border stay panel-specific.

**Lessons captured** (apply to subsequent lifts):

- **Selection paint ordering is intrinsic-to-surface.** GTK paints
  selections before text (Cairo painter order); TUI paints after
  (cells coalesce fg+bg+char). The data primitive is shared; the
  paint approach is not — don't try to consolidate.
- **Cursor side-effects** that don't fit `Buffer`-only paint
  (e.g. `Frame::set_cursor_position` for TUI Bar/Underline shapes)
  are returned via a `*PaintResult` struct; the host applies them.
- **Theme growth.** Adding ~25+ fields to `quadraui::Theme` for one
  primitive is fine, but split the `q_theme()` adapter into chrome +
  primitive halves so each section stays comprehensible.
- **Backend-specific math.** "The math is identical" is rarely true
  on first inspection. Stage 2 (scrollbars) and Stage 1 (cursor inversion
  vs alpha-rect) both hit small math/mechanism divergences. The data
  primitive can be shared while each rasteriser keeps its native
  drawing approach.
- **Glyphs on the primitive.** Icon characters (lightbulb, find/replace
  glyphs) live as fields on the primitive itself, populated by the
  host's icon registry per frame. Keeps the rasteriser independent
  of the host's icon system.

## Phase B history (everything below shipped — kept for reference)

### Phase #270 Stage A — GtkBackend lift (✅ shipped)

`b256993` shipped:
- `quadraui::gtk::backend::GtkBackend` (lifted ~920 lines).
- `quadraui::gtk::services::GtkPlatformServices` (lifted ~96 lines).
- `quadraui::gtk::events::*` GDK→UiEvent translation (~334 lines).

Vimcode-private deps replaced with backend-stored fields:
`nerd_fonts_enabled: bool` + `set_nerd_fonts(bool)` setter
(replaces `crate::icons::nerd_fonts_enabled()` global atomic);
`ui_font: String` + `set_ui_font(impl Into<String>)` setter
(replaces `super::draw::UI_FONT()` macro). Both are wired once at
`GtkBackend` construction and re-synced per frame in vimcode's
`CacheFontMetrics` handler so runtime toggles propagate.

Vimcode-side `src/gtk/{backend,events,services}.rs` collapsed to
thin re-exports — call sites unchanged.

### Phase #270 Stage B — GTK runner (✅ shipped)

`f6b5a17` shipped:
- `quadraui::gtk::run<A: AppLogic>(app)` runner — single-DA shell
  with cairo + Pango setup, theme-bg clear at frame start, key /
  mouse / scroll → UiEvent translation, Reaction dispatch
  (Continue / Redraw / Exit). Returns `std::process::ExitCode`.
- `AppLogic::AreaId` associated type (option B target-routed
  render) — same trait shape works on both backends. Single-area
  apps use `type AreaId = ()`.
- `quadraui/examples/gtk_app.rs` end-to-end + all four examples
  (tui_app, gtk_app, tui_demo, gtk_demo) consolidated through
  `examples/common/{mini_app,demo}.rs`. Backend example files
  collapsed to ~22-line `main()` trampolines; example LOC dropped
  1274 → 526 (-59%).

Multi-DrawingArea support (vimcode-style ~20 DAs) is the
trait-level shape ready (`AreaId` is the plumbing) but the runner
ships single-DA only. A multi-area runner shape will land when
there's an actual consumer asking for it.

### Phase #266 — Lift remaining 3 rasterisers (✅ partially shipped)

`779f6e8` shipped the rich_text_popup (TUI + GTK) and completions
(TUI) lifts. `quadraui::Theme` grew 5 fields (`link_fg`,
`completion_bg/fg/border/selected_bg`). GTK rich_text_popup now takes
`ui_font_desc` as an explicit parameter so the rasteriser is
self-contained; the scrollbar geometry constants
(`RICH_TEXT_POPUP_SB_WIDTH/_INSET`) re-export from quadraui to keep
paint and click hit-test single-sourced.

find_replace was deferred to **#271** because it requires a primitive
migration (see Phase #271 below) — moving it through `q_theme()` is
not enough.

### Phase #271 — find_replace primitive migration (✅ shipped)

`91e89e9` shipped the lift:
- New `quadraui::primitives::find_replace` module with
  `FindReplaceClickTarget`, `FrHitRegion`, `FR_PANEL_WIDTH`,
  `compute_hit_regions`, and `FindReplacePanel`.
- Glyph fields (`replace_one_glyph: String`,
  `replace_all_glyph: String`) carried on the panel so the
  rasteriser doesn't depend on any host app's icon registry.
- `quadraui::tui::draw_find_replace` + `quadraui::gtk::draw_find_replace`
  rasterisers lifted from the vimcode shims.
- `quadraui::Theme.accent_bg` added for toggle highlight backgrounds.
- Vimcode side: `core::engine::*` re-exports the lifted types;
  `render::FindReplacePanel` is now a type alias for
  `quadraui::FindReplacePanel`. Both shims (TUI + GTK) collapsed
  to thin delegators.

Net diff -681/+64 lines. Closes the last "trait-less primitive"
gap on the find/replace overlay.

### Phase #267 — Dialog dual-Pango (✅ shipped)

`8b64217` shipped the single-Pango-layout refactor: `quadraui::gtk::draw_dialog`
takes `pango_layout: &pango::Layout` + `ui_font_desc: &pango::FontDescription`
and swaps fonts on the layout per-region (UI font for title +
buttons, restored body font for body + input, restored at end).
`Backend::draw_dialog(&Dialog, &DialogLayout) -> Vec<Rect>` trait
method added with `TuiBackend` + `GtkBackend` impls, mirroring
`draw_context_menu`'s shape. Closes the last "trait-less primitive"
gap on `dialog`.

### Phase B.6 — Win-GUI rebuild

**Goal:** rewrite `src/win_gui/` as a clean `Backend` impl
consuming `quadraui::win_gui::draw_*` rasterisers, similar to how
GTK and TUI work post-B.5b/B.5c. The current Win-GUI is bespoke
(see `BUGS.md` for known gaps).

**Pickup:** read `docs/NATIVE_GUI_LESSONS.md` first — it documents
pitfalls from the original Win-GUI build. The rebuild benefits from
quadraui's primitive layouts so it doesn't re-derive what TUI/GTK
already nailed down.

**Tracking issues:**
- [#271](https://github.com/JDonaghy/vimcode/issues/271) — find_replace primitive lift (deferred from #266).
- [#262](https://github.com/JDonaghy/vimcode/issues/262) — breadcrumb dropdown.
- [#263](https://github.com/JDonaghy/vimcode/issues/263) — TUI breakpoint dot.
- [#264](https://github.com/JDonaghy/vimcode/issues/264) — settings narrow.
- [#265](https://github.com/JDonaghy/vimcode/issues/265) — TUI nerd-font widths.
- [#272](https://github.com/JDonaghy/vimcode/issues/272) — GTK hover go-to-definition link click.
- [#273](https://github.com/JDonaghy/vimcode/issues/273) — GTK cairo dialog focus-on-spawn.
- [#274](https://github.com/JDonaghy/vimcode/issues/274) — Inventory + replace remaining native gtk4::Dialog widgets.

## ✅ Phase B.5c → B.5e (TUI side) — trait coverage + audit + TUI runner (shipped)

**Closed master issues: [#259](https://github.com/JDonaghy/vimcode/issues/259) (B.5c), [#260](https://github.com/JDonaghy/vimcode/issues/260) (B.5d), [#261](https://github.com/JDonaghy/vimcode/issues/261) (B.5e), [#268](https://github.com/JDonaghy/vimcode/issues/268) (TuiBackend lift), [#269](https://github.com/JDonaghy/vimcode/issues/269) (TUI runner).**

The runner-crate vision sketched in B.5d is now real on the TUI
side: `cargo run --example tui_app --features tui` shows the
end-to-end pattern.

### B.5c stages (#259) — trait coverage

| Stage | Commit | Scope |
|---|---|---|
| B5c.1 | `985b087` | `draw_status_bar` returns `Vec<StatusBarHitRegion>`; drops `&StatusBarLayout`. |
| B5c.2 | `b3eeadf` … `e32cc8a` | `draw_tab_bar` returns `TabBarHits` (lifted to primitives, includes `close_bounds`); drops `&TabBarLayout`. Vimcode's hand-rolled tab/diff/split TUI hit-tests migrated to `bar.layout(...).hit_test()`. Icon glyphs in `build_tab_bar_primitive` now respect `nerdfonts` setting via `Icon::c()`. |
| B5c.3 | `92722cc` | `draw_text_display` drops `&TextDisplayLayout`. |
| B5c.4 | `57f3d21` | TUI explorer / settings / source-control panels route tree/form draws through `Backend::draw_*` via `enter_frame_scope`. |
| B5c.5 | `7558220` | Lifted `quadraui_gtk::draw_activity_bar` + `draw_terminal_cells` into `quadraui::gtk::*` with `quadraui::Theme`. New `ActivityBarRowHit` primitive. Theme grows `inactive_fg` + `selection_bg`. Color grows `lighten()`. |
| B5c.6 | `a4e6c9f` | `Backend::draw_tooltip` + `Backend::draw_context_menu` (3/6 trait-less primitives covered). |
| B5c.7 | `40efa99` | Parity sweep + docs. |

### B.5d (#260) — Setup audit

`docs/BACKEND_SETUP_AUDIT.md` (commit `76b0a51`) compares TUI vs
GTK init/event-loop code, identifies which parts are genuinely
backend-specific vs. boilerplate, and sketches the runner-crate
API the TUI runner subsequently shipped.

### B.5e (#261, #268, #269) — TUI runner

| Stage | Commit | Scope |
|---|---|---|
| Stage A | `2aee735` | `AppLogic` + `Reaction` trait in `quadraui::runner`. |
| Lift | `c74dcff` + `79fe1dd` | `TuiBackend` + `TuiPlatformServices` + crossterm event translation lifted to `quadraui::tui::*`. ~1750 lines moved. Vimcode-side modules collapse to thin re-exports. |
| Runner | `aa60de8` | `quadraui::tui::run<A: AppLogic>(app)` + `examples/tui_app.rs`. ~150 + 100 lines. |

### Smoke followups filed (still open)

#262 (breadcrumb dropdown), #263 (TUI breakpoint dot), #264
(settings narrow), #265 (TUI nerd-font widths). All pre-existing
bugs surfaced during smoke tests — none introduced by the arc.

### Trait coverage state (post-arc)

| Primitive | TUI | GTK |
|---|---|---|
| `tree`, `list`, `form`, `palette` | ✅ | ✅ |
| `status_bar`, `tab_bar`, `text_display` | ✅ | ✅ |
| `activity_bar`, `terminal` | ⚠️ stub (TUI inline) | ✅ |
| `tooltip`, `context_menu` | ✅ | ✅ |
| `dialog`, `rich_text_popup`, `completions`, `find_replace` | ❌ (#266 / #267) | ❌ (#266 / #267) |

## ✅ Phase B.5b — GTK runtime migration onto Backend trait

**Master tracking issue: [#249](https://github.com/JDonaghy/vimcode/issues/249) (ready to close).** Stages 1–7 shipped. Stages 8–12 deferred to #254.

### Why this exists separately from B.5

B.5's 9 stages built **the trait surface** on GTK — the struct,
the trait impl, translation helpers, the accelerator registry, the
event queue. **All compiled, none consumed at runtime.** The GTK
app today still routes:

- Mouse / key dispatch through Relm4 `Msg::*` flow (NOT `wait_events`)
- 22 inline `engine.<modal>.is_some()` modal gates (NOT `ModalStack`)
- 16 inline `matches_gtk_key` arms in the key handler (NOT `UiEvent::Accelerator`)
- 24 direct `quadraui_gtk::draw_*` shim calls (NOT `Backend::draw_*` — only the 1 quickfix call goes through the trait)
- Clipboard via engine-level closures (NOT `PlatformServices`)

`src/gtk/events.rs` carries `#![allow(dead_code)]`. The accelerator
registry is populated but inert. The trait is real but largely
unused at runtime.

B.5b is the work that gets the GTK app *onto* the trait. Each
sub-stage merges to develop independently; `vimcode` keeps booting
and rendering throughout. See #249 for the stage map and
dependencies.

### B.5b stage summary (full detail in #249)

| # | Goal | Status |
|---|---|---|
| **B5b.1** | Wire signal callbacks to push `UiEvent`s into `events_handle()`; add `glib::timeout_add_local` drain (16 ms). Foundation for everything else. **Scope:** editor DA's key + mouse-down + drag-update + drag-end + scroll callbacks. Sidebar / panel callbacks land alongside their respective click migrations in later stages. | ✅ |
| **B5b.2** | `dispatch_gtk_panel_accelerator` helper + single `match_keypress` lookup replace 13 inline `matches_gtk_key` arms in the editor key handler. `util::matches_gtk_key` deleted. Synchronous dispatch (called from the GTK signal handler), not yet via the queue drain — keeps zero-latency for shortcuts. | ✅ |
| **B5b.3** | Dialog modal click hit-test routes through `ModalStack::push` + `quadraui::dispatch_mouse_down` (picker-pattern mirror). Push on every frame the dialog is open (idempotent), pop on close. Inner button hit-test still uses GTK pixel rects from the last draw. **Out of scope:** the 9 inline `engine.dialog.is_some()` gates in sidebar key handlers (lines 5464, 5472, 5484, 5490, 5501, 5507, 5518, 5533, 5548) — those gate key routing, not click hit-testing, and are a separate concern from `ModalStack`. | ✅ |
| **B5b.4** | Context menu click hit-test routes through `ModalStack` + `dispatch_mouse_down`. Inner row hit-test migrated to `quadraui::ContextMenuLayout::hit_test` (matches the renderer; closes #251 off-by-one). | ✅ |
| **B5b.5** | Completion popup registered on `ModalStack`. Bounds piped from `draw_completion_popup` (returns `Option<Rect>`) into `App.completion_popup_rect`; click handler pushes them. Click inside the popup dismisses + consumes (cursor doesn't move); click outside dismisses + propagates. | ✅ |
| **B5b.6** | Hover popup is already on `ModalStack` via `reconcile_editor_hover_modal` (#216). Stage 6 adds a `blocking_modal_open` gate on the GTK hover trigger so mousing over LSP-hoverable text under an open palette / dialog / context menu / completion / find-replace / tab-switcher doesn't pop the hover popup behind the modal (closes #247). Click-handler refactor (`dispatch_mouse_down` replacing the inline `on_popup` rect check) deferred — current inline path works correctly. | ✅ |
| **B5b.7** | Tab-switcher modal click via `ModalStack` + `dispatch_mouse_down`; `Engine::is_blocking_modal_open()` becomes the single source of truth for the hover-trigger gate + scrollbar-hide list (replaces 3 inline enumerations). | ✅ |
| **B5b.8** | Migrate the 4 trait-callable `quadraui_gtk::draw_*` sites onto `Backend::draw_*` via `enter_frame_scope`: picker (palette), source-control + explorer panels (tree), settings panel (form). | ✅ |
| **B5b.9** | Trait extended with `&Layout` parameters for the 5 layout-passthrough primitives (status_bar / tab_bar / activity_bar / terminal / text_display) per `BACKEND_TRAIT_PROPOSAL.md` §6.2. TUI + GTK trait impls updated. | ✅ |
| **B5b.10** | 3 of 5 layout-passthrough trait impls (status_bar / tab_bar / text_display) route through `quadraui::gtk::*`. The other two (activity_bar / terminal) stay as forward-compat stubs because their rasterisers live in `crate::gtk::quadraui_gtk::*` and take legacy `render::Theme`; lifting them into quadraui itself is the #223 lift task. GTK call sites continue to use the legacy shims directly because the trait method returns `()` while sites need hit-region info. | ✅ (within scope) |
| **B5b.11** | Dropped `App.modal_stack` and `App.drag_state` alias fields; ~30 call sites now reach state via `self.backend.borrow().modal_stack_handle()` / `drag_state_handle()`. | ✅ |
| **B5b.12** | Deleted dead `quadraui_gtk::draw_{tree, form, list, palette}` shims; removed `#![allow(dead_code)]` from `events.rs`. | ✅ |
| **B5b.13** | Smoke-test parity confirmed during each stage land (Path A verification). | ✅ |

### Stage 1 ship notes

What changed in B5b.1:
- `let backend_events = gtk_backend.events_handle();` exposed in `init()` scope before `view_output!()` so closures inside the `view!` macro can capture it.
- Editor DA's `EventControllerKey::connect_key_pressed` pushes `UiEvent::KeyPressed` via `events::gdk_key_to_uievent()` at the top of the closure (every keypress translated, even those that early-return through `matches_gtk_key`/`match_accelerator` paths).
- `GestureClick::connect_pressed` pushes `UiEvent::MouseDown`.
- `GestureDrag::connect_drag_update` pushes `UiEvent::MouseMoved` with a left-button-held mask.
- `GestureDrag::connect_drag_end` pushes `UiEvent::MouseUp` (reconstructs release coords from start+delta — the existing `Msg::MouseUp` discards them).
- `EventControllerScroll::connect_scroll` pushes `UiEvent::Scroll`, using `last_editor_pointer` for the position so the consumer can route to the window under the cursor.
- `glib::timeout_add_local(16ms)` drains via `Backend::poll_events()` and discards.
- `#[allow(dead_code)]` removed from `GtkBackend::events_handle()` and `App.backend` — both now live.

Out of scope for B5b.1, deferred:
- Sidebar callbacks (explorer, debug, git, ext, AI panels) — wired alongside their click migration in B5b.3–B5b.7.
- Settings panel callbacks (around lines 2261–2291).
- Window resize → `UiEvent::WindowResized` — the trait already updates viewport via `begin_frame`, so this is redundant in GTK.
- Motion events (~100–200 Hz) — not pushed; the drain doesn't need them today and they'd dominate the queue. Add when a consumer needs them.

### Stage 2 ship notes

What changed in B5b.2:
- `GtkBackend::match_keypress` flipped to `pub` so the GTK key callback can synchronously query the registry.
- `fn dispatch_gtk_panel_accelerator(id, &ComponentSender<App>, &Rc<RefCell<Engine>>) -> bool` added at module scope. Returns `true` if the id was handled. Mirrors `tui_main::dispatch_panel_accelerator`. Dispatch site: 14 ids (the 12 late panel keys plus `ACC_OPEN_TERMINAL` and `ACC_TERMINAL_TOGGLE_MAX` for completeness — the latter is unreachable today because the engine's accelerator block matches first).
- The editor key handler does ONE `match_keypress` lookup per keypress and stashes the `Option<AcceleratorId>`:
  - **Early dispatch**, before the terminal-focus block, only fires for `ACC_OPEN_TERMINAL` so Ctrl+T keeps working when the terminal has focus (matches pre-migration semantics).
  - **Late dispatch**, after the terminal-focus block, routes any other id through `dispatch_gtk_panel_accelerator`.
- 13 inline `if matches_gtk_key(&pk.X, key, modifier) { ... }` arms removed from the editor key handler.
- `util::matches_gtk_key` deleted (only call site was the editor handler; one stale comment in `App::handle_panel_key` now references `render::matches_key_binding` instead).
- `App.backend` field is now cloned into the model rather than moved, so the `view!` macro can also clone it into the key callback's capture list.

Why this is *not* yet driven by the queue drain:
- Synchronous dispatch from the GTK signal handler keeps zero-latency on shortcut keys. The drain (16 ms tick) would add up to that interval of latency to "open command palette" — perceptible.
- The producers from Stage 1 *also* push `UiEvent::KeyPressed` for the same key; the drain rewrites it to `UiEvent::Accelerator(id, mods)` via `apply_accelerators` and discards. So the queue stays consistent with the synchronous path; subsequent stages can move dispatch fully onto the drain when the latency tradeoff is OK.

### Issues this resolves

- **#192** GTK palette mouse leak → B5b.3
- **#229** GTK editor hover scrollbar leak → B5b.6
- **#236** GTK ContextMenu border → folds into B5b.4
- **#247** GTK modal hover + Pango font swap → B5b.6
- **#243** GTK debug sidebar GestureDrag → standalone, fits B5b.1 pattern

---

## ✅ Phase B.5 — GTK Backend trait plumbing (shipped)

Bring vimcode's GTK binary onto `quadraui::Backend`. **What B.5
actually delivered:** the trait surface, the infrastructure, one
pilot site (quickfix panel through `Backend::draw_list`). Runtime
migration is B.5b above.

Each stage merges to develop on its own — `vimcode` keeps working
at every commit. Refactor in place; no parallel tree.

### Pinned decisions (locked before Stage 1)

1. **Event-loop shape: option A — event queue.** GtkBackend holds
   `Rc<RefCell<VecDeque<UiEvent>>>`; GTK signal callbacks push into
   it; `wait_events(timeout)` drains the queue (using
   `glib::MainContext::iteration(false)` between checks if empty).
   This keeps the trait shape uniform with TUI and forward-compatible
   with all callback-driven backends (Win-GUI, macOS, Android, iOS,
   web). TUI is **not** inverted — its synchronous loop stays
   greppable end-to-end.
2. **Editor primitive stays inherent**, like TUI — `draw.rs::draw_editor`
   doesn't go through the trait. Per `BACKEND_TRAIT_PROPOSAL.md` §6.2,
   the editor primitive is deferred until all 4 backends ship.
3. **Settings panel stays GTK-native** for inputs that are better as
   native widgets (font picker, color picker, file picker). The Form
   primitive carries the rest.
4. **Stage workflow: Path A.** Each stage merges to develop independently.
   `vimcode` keeps booting and rendering at every commit.

### Stage map

| Stage | Goal | Status | Commit |
|---|---|---|---|
| **0** | Plan + decisions locked | ✅ | `2c8fe7f` |
| **1** | `GtkBackend` skeleton + frame-scope (`&cairo::Context`). Owns `modal_stack`, `drag_state`, accelerators, viewport, services. Stub `draw_*` delegates to existing `quadraui_gtk::draw_*` shims. App holds it as `Rc<RefCell<GtkBackend>>`. | ✅ | `76be71d` |
| **2** | Trait `draw_*` methods route through `GtkBackend` for the 4 clean primitives (palette / list / tree / form). Other 5 deferred (need `&Layout` parameter on the trait, same as TUI). | ✅ | `0b209d5` |
| **3** | Pilot: quickfix panel routes through `Backend::draw_list` via `enter_frame_scope`. Mirrors TUI Stage 3a. | ✅ | `44f4291` |
| **4** | `src/gtk/events.rs` — GDK → `UiEvent` translation helpers + 10 unit tests. `events_handle()` + `push_event` API on GtkBackend. Producer wiring deferred (issue #248). | ✅ | `a4637b9` |
| **5** | Reduced scope. Shipped: `is_modal_open()` helper. Bulk migration (dialog / context-menu / completion → `ModalStack` + dispatch through `dispatch_mouse_*`) filed as #248 for iterative work. | ✅ | `3a68a88` |
| **6** | Panel-key accelerators registered on `GtkBackend` at App init. Dispatch swap (replacing inline `matches_gtk_key` arms) deferred until producers are wired. | ✅ | `c90c85b` |
| **7** | `GtkPlatformServices` — `write_text` + `open_url` wired through GTK APIs. Async-API methods (clipboard read, file dialogs, notifications) stay stubbed pending a future async-aware trait shape. | ✅ | `73d37d5` |
| **8** | Cleanup + parity verification: GTK + workspace builds clean, BACKEND.md updated. | ✅ | (this commit) |

**Total: 9 stages shipped over 1 session** (faster than the original ~10–12 estimate; many stages were smaller than feared because the chrome work in Phase A had already moved most primitive rendering onto `quadraui_gtk::draw_*` shims).

### B.5 follow-up issues

These were filed during B.5 stages and represent iterative work that lands incrementally:

- **#247** — GTK: LSP hover fires through open modal; modal font swaps. Resolves once Stage 5+ work in #248 puts dialog/completion modals on `ModalStack` and adds an `is_modal_open()` gate to the LSP hover trigger.
- **#248** — Stage 5+ follow-up: migrate dialog / context-menu / completion modals to `ModalStack`. Each modal is its own focused PR. Closes #192 (palette mouse leak) + partially #229 (hover scrollbar leak) along the way.

### Forward-compatibility notes (for backends after B.5)

The queue pattern from Stage 4 generalises to every callback-driven
backend. Concrete plans for later phases (not B.5 scope):

- **B.6 Win-GUI** — `wait_events` calls `MsgWaitForMultipleObjects`
  with the timeout, then `PeekMessage`/`DispatchMessage`. WindowProc
  translates messages to `UiEvent` and pushes onto the queue.
  Win32 is hybrid (poll + callbacks); fits naturally.
- **B.7 macOS native** — `NSApplication.run()` runs on the main
  thread; delegate methods push to a `Mutex<VecDeque<UiEvent>>`.
  `wait_events` polls the queue; if empty, runs one iteration of
  `NSRunLoop` with a brief timeout.
- **Future Android (NDK + ALooper)** — `ALooper_pollAll(timeout)` is
  the natural `wait_events` shape. Touch events would require new
  `UiEvent::Touch*` variants (additive, doesn't break the trait).
- **Future iOS (UIKit via objc2)** — same queue pattern as macOS;
  also needs touch variants.
- **Future WASM/web** — JS event listeners push to a queue;
  `requestAnimationFrame` callback drives paint. Single-threaded
  but doesn't conflict with the queue.

The hard work for mobile/web is **engine-layer** (subprocess
spawning for LSP / git / terminal panes; sandboxed filesystems; soft
keyboard / IME; touch primitives), not the Backend trait.

### Critical risks for B.5

1. **Stage 4 — event-queue + Relm4 interplay.** GTK signal callbacks
   live inside Relm4 widget templates; the queue handle needs to be
   reachable from every callback. Likely solution: `Rc<RefCell<VecDeque>>`
   field on `GtkBackend`; each callback closure captures a clone.
   Risk: if Relm4's actor model fights the shared-state pattern,
   we may need a different IPC shape (mpsc channel from callbacks
   to the wait_events drainer).
2. **Refactoring the 11k-line `mod.rs`.** Some App component fields
   move to `GtkBackend`; others stay. Stage 1 establishes the
   boundary; later stages may surface fields that need to move.
3. **Editor draw stays inherent** but must continue to share data
   with the trait-driven primitives (e.g. font metrics, theme).
   Need to thread those through cleanly.

### Open issues B.5 may resolve

- **#229** GTK editor hover: scrollbar leak (right-edge specific) — falls out of dispatch consolidation
- **#192** GTK palette mouse leak — same
- **#236** GTK ContextMenu border barely visible — touched in chrome migration but separate
- **#243** GTK debug sidebar: no GestureDrag (filed during Stage 5c) — Stage 5 directly addresses

---

## ✅ Phase B.4 — TUI Backend trait (shipped)

The quadraui readiness gate cleared (all primitives shipped with
D6 `layout()` + `hit_test()`; cross-backend dispatch infrastructure
proven). Phase B.4 was the multi-session rewrite of vimcode's TUI
backend on top of `quadraui::Backend` + `UiEvent` + the dispatch
infrastructure.

### Stage map

| Stage | Goal | Status | Commit |
|---|---|---|---|
| **0** | PLAN.md trim for B.4 focus | ✅ | `b97635d` |
| **1** | `TuiBackend` skeleton + frame ownership | ✅ | `3a10bdb` |
| **2** | `palette` / `list` / `tree` / `form` draws via trait + frame-scope mechanism | ✅ | `9c3a681` |
| **3a** | Quickfix panel via `Backend::draw_list` | ✅ | `3ae3360` |
| **3b** | `MockBackend` cross-backend test fixture | ✅ | `12fef4a` |
| **4** | `crossterm → UiEvent` translation + `poll_events`/`wait_events` impls | ✅ | `d5a5159` |
| **5a** | `wait_events` semantics + inverse `UiEvent → crossterm::Event` helpers | ✅ | `6b7a2e4` |
| **5b** | Event loop flipped to `Backend::wait_events` — **trait now load-bearing** | ✅ | `4016ecc` |
| **6** | Accelerator registry consolidation for cross-mode global keybindings | ✅ | `b400ce0` |
| **5c** | Drag-state consolidation: 5 scrollbars (search/settings/debug-sidebar/terminal/debug-output) → `quadraui::DragState`; `grab_offset` field added to `DragTarget::ScrollbarY` | ✅ | `f8d394f` |
| **5d** | Editor-window scrollbar (`ScrollDragState`) migration — `DragTarget::ScrollbarX` variant added for horizontal axis | ✅ | `801ad84` |
| **7** | Cleanup + GTK + Win-GUI compile verified + `BACKEND.md` worked example | ✅ | (this commit) |

**Architectural milestones:**
- **5b** — every native event reaches the existing dispatch through
  `Backend::wait_events` + inverse synth helpers. Trait load-bearing.
- **6** — 14 cross-mode keybindings + `terminal.toggle_maximize` route
  through `UiEvent::Accelerator` dispatch instead of inline
  `matches_tui_key` arms. Per-feature wiring collapses to "register
  one accelerator + emit one `UiEvent`" for these bindings.
- **5c** — 5 of 6 TUI scrollbar drags share `quadraui::DragState`;
  `DragTarget::ScrollbarY` gained `grab_offset` so cursor preserves
  its relative position on the thumb during drag.

After Stage 5d + 7, the per-feature wiring story collapses fully —
remaining work is the editor-window scrollbar (needs horizontal axis
in DragTarget) and cleanup of any cross-backend compile lag.

### How the open risks resolved

The original plan flagged three discoveries that could reshape the work.
**First risk** (Terminal/Frame borrow-checker pain) was avoided —
TuiBackend doesn't own the Terminal (the event loop does). A
type-erased `current_frame_ptr: Cell<*mut ()>` set inside
`enter_frame_scope` lets trait `draw_*` methods reach the Frame
without lifetime parameters on the struct. **Second risk** (missing
UiEvent variants) didn't fire — the existing variants covered every
crossterm event we actually surface today. **Third risk** (drag-state
shapes) split: standard vertical scrollbars unified cleanly into
`DragTarget::ScrollbarY` with widget-id encoding (Stage 5c shipped).
The editor scrollbar's horizontal-axis support and per-window-id
plumbing didn't fit; deferred to Stage 5d (issue #244).

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

**Stage 5c smoke-test follow-ups** (filed alongside 5c ship):

- **#200** — TUI ext panel: scrollbar not drawn even when content overflows (pre-existing missing render path)
- **#242** — TUI debug sidebar: scrollbar click doesn't reach `handle_mouse` (needs investigation — log shows clicks not landing at the rendered scrollbar column)
- **#243** — GTK debug sidebar: no `GestureDrag` for thumb-grab (mirror the explorer pattern)
- **#244** — Stage 5d — editor-window scrollbar (`ScrollDragState`) migration (needs `ScrollbarX` variant)
- **#245** — Inverted scrollbars (terminal scrollback, debug output): thumb-grab not preserved
- **#246** — TUI explorer scrollbar thumb brighter color than other panels (theme consistency)

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
