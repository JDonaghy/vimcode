# VimCode — Current Plan

> **Purpose of this file:** Session-level coordination doc for in-flight
> multi-stage features, so work can be picked up on a different machine
> without reconstructing state from scratch. GitHub issues remain the
> source of truth for individual tasks — this file points at the current
> wave and explains how to resume.
>
> **Last updated:** 2026-04-20 (Session 315 — A.2c shipped: Win-GUI explorer migration)

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
| **Optional Win-GUI parity** — see "Win-GUI parity scope" section below | ⬜ Optional | — | `quadraui-phase-a*-win` | Windows |
| **Phase B.1** — `UiEvent` + `Accelerator` + `Backend` trait scaffolding | ✅ Done | _tbd_ | `quadraui-phase-b1-backend-trait` | any |
| Phase B.2 — pilot migration: terminal maximize to `Accelerator::Global` | ⬜ **Next — sketch first** (see below) | — | `quadraui-phase-b2-maximize-pilot` | any |
| Phase B.3 — layout primitives (`Panel`, `Split`, `Tabs`, `MenuBar`, `Modal`) | ⬜ After B.2 | — | `quadraui-phase-b3-layout` | any |
| Phase B.4+ — migrate remaining vimcode subsystems to UiEvent | ⬜ After B.3 | — | `quadraui-phase-b4-*` | any |
| Phase B.5 — Postman-class validation app (#169) | ⬜ After B.3/B.4 | — | _new workspace member_ | any |
| Phase C — macOS backend | ⬜ v1.x | — | — | macOS |
| Phase D — polish + k8s validation app (#145) | ⬜ Later | — | — | any |

**All required platform-specific stages are now done.** A.1b/A.2b/
A.3c-2/A.4b/A.5b shipped on Linux GTK; A.1c (Session 314) and A.2c
(Session 315) shipped on Windows. A.2b was split into two sub-phases
because the atomic switchover was a ~1500-line diff across the view!
macro, the App struct, ~50 scattered `Msg` handlers, and a context-
menu rewrite; the split kept smoke-test regressions bisectable. The
remaining open quadraui stages (A.6–A.9) are platform-neutral, and
the Win-GUI parity stages (A.6*–A.7 for Windows) are explicitly
optional — see "Win-GUI parity scope" below.

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

## Phase B.2 starting notes — terminal-maximize pilot migration

**Status:** Ready to start, but sketch design questions first before any
code. Branch: `quadraui-phase-b2-maximize-pilot` off develop.

### What gets migrated in B.2

**Just the keybinding path for Ctrl+Shift+T.** Everything else stays
bespoke for now and migrates later:

- Toolbar button (click hit-test + dispatch) → stays; migrates in B.3
  when `Panel` primitive owns its click regions.
- `:TerminalMaximize` ex command → stays on current `EngineAction`
  path.
- `PanelChromeDesc` chrome-rows math → stays; that's layout, not
  dispatch.
- Window-resize handler → stays; that's layout lifecycle, not
  keybinding.
- Per-window status suppression when maximized → stays.

### What B.2 actually requires

Three things, in order:

1. **Real `Backend` impls** — the B.1 scaffolding has types but no
   working impls yet. Each of `TuiBackend`, `GtkBackend`, `WinBackend`
   needs:
   - Struct holding the accelerator registry, event queue, and
     whatever drawing-context reference is required (ratatui `Frame`,
     Cairo `Context`, Direct2D `RenderTarget`).
   - `register_accelerator()` storing registrations for
     `poll_events()` to match against.
   - `poll_events()` that drains native events (crossterm / GTK
     signals / Win32 `WndProc`), compares key events to registered
     accelerators, emits `UiEvent::Accelerator` on match or
     `UiEvent::KeyPressed` / other variants for unmatched input.
   - `draw_*` methods — trivial thin wrappers around the existing
     free functions (`quadraui_tui::draw_tree`, etc.). No behaviour
     change; apps that want to keep calling the free functions
     directly still can.
2. **Vimcode engine grows a `handle_ui_event(&mut self, UiEvent)`
   entry point.** Dispatches `UiEvent::Accelerator("terminal.toggle_maximize", _)`
   to the existing `toggle_terminal_maximize()` method. For B.2 this
   has exactly one accelerator arm; B.4 adds more.
3. **Delete** the old per-backend `pk.toggle_terminal_maximize`
   key-matcher checks + their handlers in TUI `event_loop`, GTK key
   controller, Win-GUI `on_key_down`. Plus the
   `EngineAction::ToggleTerminalMaximize` action-dispatch branches
   in each backend's action handler.

### Realistic LOC delta

The proposal's **aspirational -60 LOC net** is wrong for B.2 specifically.
First real backend impl is ~150 new lines (struct + registration +
event translation); old deletions are ~25 lines per backend. Net
~**+250 / -75** across the three backends + engine dispatch.

Payoff arrives in **Phase B.4** when each subsequent accelerator
migration is +1 line (a new registration entry) and -20 lines
(native-key plumbing deleted per backend). That's when the
per-feature wins from the proposal actually materialise.

### Open design questions — sketch before coding

Spend 15-30 min writing answers into
`quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` as a new §11 "Phase B.2
implementation notes" **before** touching any code:

1. **TuiBackend struct shape.** Does it own the ratatui `Terminal`
   end-to-end, or does the app pass a `&mut Frame<'_>` into
   `begin_frame` via a backend-specific extension trait (violating
   the clean `Backend` trait shape)? Resolution probably: struct
   owns the `Terminal`; `begin_frame` gets a frame internally and
   `draw_*` uses it. Has implications for how apps call `backend.run()`
   or similar top-level method.
2. **Native-event → UiEvent translation.** First-match-wins against
   registered accelerators, else raw `UiEvent::KeyPressed`? What about
   key events that partially match a binding's scope (widget-scoped
   accelerator with wrong widget focused)? Translation happens in
   `poll_events`; spell out the algorithm.
3. **Main-loop integration.** Does vimcode's `event_loop` in
   `tui_main/mod.rs` keep its current structure and call
   `backend.poll_events()` once per tick, or does it invert to
   `for ev in backend.poll_events() { engine.handle_ui_event(ev) }`?
   Cleanest cutover vs. minimum-diff. Same question for GTK's Relm4
   message loop and Win-GUI's message pump.
4. **GTK event ownership.** GTK4 signals fire into Relm4 message
   handlers on the main thread. Where does the Relm4 handler push an
   event onto `GtkBackend`'s queue — is the backend a Relm4
   component, or does it live as a side-channel the Relm4 handlers
   write into?
5. **Win-GUI message-pump integration.** `WndProc` callbacks are
   synchronous; they can push directly onto the backend's queue.
   Decide: does `WndProc` call through to the backend synchronously
   (like `backend.push_native_event(...)`), or does it buffer raw
   native events and translate later in `poll_events`?

Answers close the only parts of the B.1 design that were sketched
but not proven. Without them, the first `impl Backend for TuiBackend`
will reveal the problems midway through, and the fix might involve
changing the `Backend` trait signature — which blocks GTK/Win-GUI
impls.

### Do this in a new session

This conversation ran the design through all 5 decisions and landed
B.1. Fresh session is appropriate for the sketch + code work; all
the load-bearing artifacts are on develop:

- `quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` §1–§10 (all 5 decisions
  resolved)
- `quadraui/docs/APP_ARCHITECTURE.md` (app-developer patterns)
- `docs/NATIVE_GUI_LESSONS.md` (backend-implementer pitfalls)
- `PLAN.md` § "Lessons learned" (maximize-era rules)
- This §"Phase B.2 starting notes" (what you're reading)
- Open issues: #167, #168, #169, #170 merged at `06dec4a`

### Workflow reminders

Per `CLAUDE.md` "Development Workflow":

- Branch off `develop`, not `main`.
- **Do NOT push until user approves.** Offer smoke tests / ask "ready
  to push?" before any `git push`.
- **Ask user which path** (A = local ff-merge + push develop, B = push
  branch + PR targeting `develop`) — don't infer.
- When opening a PR, base is `develop`. `gh pr create` defaults to
  `main`; always pass `--base develop` or `gh pr edit --base develop`
  after creation.

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

## Phase A.3 — `Form` primitive + settings panel

**Branch:** `quadraui-phase-a3-form-settings` off `develop`.

**Platform:** any — settings panel exists in TUI and GTK.

**Why this is next (reordered ahead of A.2b):** A.2b (GTK explorer) needs
a text-input primitive for inline rename / new-entry. Building A.2b
before Form/TextInput exists would mean dialog fallbacks and re-work
when the primitive later lands, so we let the catalog catch up first.

**Scope — new primitives in `quadraui`:**

- `Form` — container primitive holding labeled field rows.
- Field types for v1 baseline: `Toggle` (bool), `TextInput` (string),
  `Button`. (Richer fields — `Dropdown`, `Slider`, `ColorPicker` —
  tracked in #143 and defer to a follow-up.)
- `FormEvent` variants: `ToggleChanged { id, value }`,
  `TextInputChanged { id, value }`, `ButtonClicked { id }`, plus
  `KeyPressed { key, modifiers }` for app-level routing.
- All types owned + serde-compatible per plugin invariants (§10).

**Scope — migration:**

1. Define `quadraui::primitives::form` with the types above.
2. Add `draw_form()` in each TUI + GTK backend (Win-GUI deferred).
3. New adapter `settings_to_form()` in `src/render.rs` — converts
   `Engine.settings` state into a `quadraui::Form` description.
4. TUI `render_settings_panel()` in `src/tui_main/panels.rs` — replace
   with `draw_form()` call when no special state is active.
5. GTK settings panel (it exists imperatively, not via a native widget) —
   replace with `draw_form()` call.
6. Keyboard navigation between fields: Tab / arrows / typing for
   `TextInput` focus; Space to toggle; Enter to activate buttons.
7. Scroll for long settings lists (primitive-owned? or app-owned as in A.1/A.2).

**Out of scope for A.3:**

- `Dropdown`, `Slider`, `ColorPicker` fields (tracked in #143). Enum-valued
  settings keep using text-input + validation until #143 lands.
- Settings search / filter input (can reuse the `TextInput` primitive,
  though).
- Win-GUI port (follow-up stage A.3c).

**Reference implementations:** None yet — Form is a brand-new primitive.
The `TreeView` primitive (`quadraui/src/primitives/tree.rs`) is the
template for shape (data struct + event enum + backend draw function).

**Smoke test after implementing:**

```bash
cargo run --bin vcd    # TUI
cargo run              # GUI
```

- Settings panel renders with current values
- Tab / arrow keys move between fields
- Toggle settings flip with Space
- Text input fields accept typing, Backspace, Enter commits, Escape cancels
- Button rows dispatch the expected engine action

**Rough size estimate:** Larger than A.1a/A.1b (~600–900 lines) because
Form is a new primitive with more event surface than TreeView.

---

## Phase A.2b — GTK explorer migration (two sub-phases)

**Split rationale:** the full migration touches the `view!` macro,
the App struct, ~50 scattered `Msg` handlers that reference
`file_tree_view` / `tree_store` / `name_cell`, plus a 310-line
right-click context-menu rewrite. Landing that atomically makes any
smoke-test regression hard to bisect. Instead we ship the scaffolding
dead-code-first so the draw pipeline is known-good before flipping
the wiring.

### Sub-phase A.2b-1 — scaffolding (inert)

**Status:** ✅ Done (`e34a72f`).

**Branch:** `quadraui-phase-a2b-treeview-explorer-gtk` (merged, deleted).

**Platform:** any (no GTK-specific runtime changes; the new code is not
yet called).

**What lands:**

1. `src/gtk/explorer.rs` — new module with `ExplorerRow`,
   `ExplorerState { rows, expanded, selected, scroll_top }`,
   `build_explorer_rows`, and `explorer_to_tree_view` adapter. Mirrors
   the TUI's `ExplorerRow` / `collect_rows` shape — intentionally
   duplicated for sub-phase 1, to be unified into `src/render.rs` in a
   later session once both backends have stabilised on
   `quadraui::TreeView`.
2. `draw_explorer_panel` in `src/gtk/draw.rs` — calls
   `quadraui_gtk::draw_tree` and overlays a scrollbar using the same
   pattern as `draw_settings_panel` (A.3c-2).
3. Both pieces are `#[allow(dead_code)]`; the file tree still renders
   via the native `gtk4::TreeView`.

**Validation:** `cargo fmt`, `cargo clippy` (default + no-default-features),
`cargo test --no-default-features` — all pass. No behavioural change.

### Sub-phase A.2b-2 — atomic switchover

**Status:** ⬜ Queued. Tracked as [#152](https://github.com/JDonaghy/vimcode/issues/152).

**Branch:** `issue-152-a2b2-switchover-gtk` off `develop`.

**Platform:** Linux or macOS with GTK4 (4.10+).

**Platform:** Linux or macOS with GTK4 (4.10+).

**This is the biggest architectural migration in all of Phase A.** Unlike
the SC panel (A.1b), which was already rendered into a `DrawingArea`,
the GTK explorer today uses a **native `gtk4::TreeView` widget with a
`TreeStore` model**. Migrating means tearing out the native widget
entirely and rendering the explorer into a `DrawingArea` via
`quadraui_gtk::draw_tree`.

**What the native widget provides today (that we lose by default):**

- Built-in vertical scrolling with kinetic inertia
- Native keyboard navigation (Up/Down/Left/Right/Page-Up/Down/Home/End)
- Right-click context menu integration
- Accessibility tree exposed to screen readers / AT-SPI
- Native drag-and-drop handles
- Row focus outline, hover states

A.2b reimplements **only what's needed right now** on top of the
primitive. The rest defers to later quadraui stages (context menus,
a11y, drag-drop).

**Scope (sub-phase 2):**

**Already landed in sub-phase 1** (`e34a72f`):

- `src/gtk/explorer.rs` with `ExplorerRow`, `ExplorerState`,
  `build_explorer_rows`, `explorer_to_tree_view` adapter.
- `draw_explorer_panel` in `src/gtk/draw.rs` (calls
  `quadraui_gtk::draw_tree` + scrollbar overlay).

**Remaining work for sub-phase 2:**

1. Find the GTK explorer widget setup in `src/gtk/mod.rs` (search for
   `TreeView::new()` or similar) and the associated `TreeStore` /
   `ListStore` construction. Remove them.
2. Replace with a `DrawingArea` sized and placed the same way as the
   SC sidebar panel.
3. (done in sub-phase 1 — `explorer_to_tree_view` adapter already
   exists in `src/gtk/explorer.rs`.)
4. Wire the DrawingArea's draw callback to call
   `draw_explorer_panel()` with the adapted tree.
5. Re-wire click handling: the old `TreeView` widget dispatched
   `row-activated` / `cursor-changed` signals. Now clicks land on the
   DrawingArea; compute `row = (click_y / item_height)` and update
   `sidebar.selected`, then call existing engine methods to
   open/toggle/etc. Use `src/tui_main/mouse.rs` explorer click handling
   as the reference.
6. Re-wire keyboard handling: capture Key controller events on the
   DrawingArea, dispatch `j/k/l/h/Enter/Escape` to the same engine
   methods the TUI uses. Use `src/tui_main/mod.rs` lines 2640-2760
   as the reference.
7. Add a scrollbar overlay (mirror what the TUI does in
   `render_explorer_scrollbar` — or use a Cairo version of the same
   thumb-and-track pattern).

**Special-mode handling (rename / new-entry):** same pattern as TUI's
A.2a: when `engine.explorer_rename.is_some()` or
`engine.explorer_new_entry.is_some()`, fall through to a legacy path.
For the GTK migration, the "legacy path" will need to be written
because the old native-widget code won't exist any more. Options:
(a) render the edit input as a GTK `Entry` widget overlaid on the
relevant row, or (b) defer rename/new-entry to a stage after Form
lands. **Recommendation:** option (b) — keep A.2b focused on baseline
rendering. Mark rename/new-entry as unavailable in GTK during A.2b
(the TUI keeps working). Restore them after `Form` / `TextInput`
primitive lands.

**Reference implementations:**
- `src/tui_main/panels.rs::explorer_to_tree_view` (adapter)
- `src/tui_main/panels.rs::render_sidebar` (rendering dispatch, special-mode branch)
- `src/tui_main/quadraui_tui.rs::draw_tree` (rendering template, TUI)
- `src/gtk/quadraui_gtk.rs::draw_tree` (rendering template, GTK — already exists for SC)

**Pre-flight reading:**
- [`docs/NATIVE_GUI_LESSONS.md`](docs/NATIVE_GUI_LESSONS.md) — lessons
  from the Win-GUI build. Click geometry vs. draw geometry mismatches
  (§5) are the most likely class of bug when wiring the DrawingArea.

**Smoke test after implementing:**

```bash
cargo run   # default GUI
```

1. Explorer panel renders on launch — tree of files and dirs, icons,
   indent, chevrons.
2. `j`/`k` moves selection through all visible rows.
3. `l`/Enter on a file opens it in the editor.
4. `l`/Enter on a dir toggles expand/collapse.
5. `h` on an expanded dir collapses it; `h` at root unfocuses (matches
   TUI behaviour).
6. Scrollbar updates as selection / content changes.
7. Git indicators (M/A/D) appear right-aligned on modified files.
8. Diagnostics: errors/warnings badge on files with LSP diagnostics.
9. Mouse click on any row selects it.
10. **Known regressions** vs. old native widget (document clearly if
    they affect users):
    - Inline rename (deferred)
    - Drag-and-drop (deferred — wasn't in TUI either)
    - Context menus (deferred to A.x)
    - Accessibility tree (deferred — v1.1 per design §7.6)

**Out of scope for A.2b:**

- `TreeEvent` routing (still direct-to-engine for Phase A)
- Primitive-owned scroll state
- Context menus
- Inline rename (falls under Form primitive)
- Native drag-and-drop

---

## Phase A.1c — Win-GUI `draw_tree`

**Branch:** `quadraui-phase-a1c-treeview-win-gui` off `develop`.

**Platform:** Windows with MSVC build tools + Rust stable. Needed because
Direct2D/DirectWrite bindings only build under `target_os = "windows"`.

**Setup on Windows:**

```powershell
# Install Rust via rustup.rs (default toolchain = stable-msvc)
# Install Git for Windows
git clone git@github.com:JDonaghy/vimcode.git
cd vimcode
git checkout develop
cargo build --bin vimcode-win --features win-gui --no-default-features
```

Running: `.\target\debug\vimcode-win.exe` (or use `cargo run --bin vimcode-win --features win-gui --no-default-features`).

**Scope:**

1. Create `src/win_gui/quadraui_win.rs` with a `draw_tree` function that
   takes a Direct2D render target, area rect, the `TreeView`, and theme.
2. Port the TUI reference to Direct2D + DirectWrite: row background fill
   (`FillRectangle`), chevron (`DrawText`), icon, styled spans, badge.
3. In `src/win_gui/mod.rs` (wherever the SC panel is drawn — search for
   `draw_source_control_panel` or similar), replace the section loop with
   a call to `render::source_control_to_tree_view()` + `quadraui_win::draw_tree`.
4. Click handling stays on the existing path (event routing is later).

**Pre-flight reading (MANDATORY):**

- [`docs/NATIVE_GUI_LESSONS.md`](docs/NATIVE_GUI_LESSONS.md) — every lesson
  from the initial Win-GUI build. The tab-bar breadcrumb offset bug (§1)
  and the draw/click geometry mismatch (§5) are classes of bugs likely to
  recur in TreeView rasterisation.
- [`src/tui_main/quadraui_tui.rs`](src/tui_main/quadraui_tui.rs) — reference
  implementation.

**Smoke test after implementing:**

- Launch `vimcode-win.exe`.
- Open the git panel.
- Verify sections, chevrons, icons, selection highlight.
- Verify click-to-open, keyboard nav, Tab expand/collapse, `s` to stage.
- Multi-group layouts don't break (§2 of NATIVE_GUI_LESSONS).

**Win-GUI-specific constraints:**

- NEVER run `cargo test` with `--features win-gui` (spawns real windows).
- Clippy: `cargo clippy --features win-gui --no-default-features`.
- Build the binary: `cargo build --bin vimcode-win --features win-gui --no-default-features`.

---

## Phase A.2c — Win-GUI explorer

**Branch:** `quadraui-phase-a2c-explorer-win-gui` off `develop`.

**Platform:** Windows (same toolchain as A.1c).

**Scope:**

1. After A.1c lands `quadraui_win::draw_tree`, the same primitive renders
   the explorer panel. Pattern mirrors A.2b-1 + A.2b-2 (Linux GTK):
   - Build an `ExplorerState` (rows + expanded set + selection + scroll
     offset) on the Win-GUI App. The Linux side put this in
     `src/gtk/explorer.rs`; the Win-GUI equivalent should live in
     `src/win_gui/explorer.rs` (or be inlined if the App is small).
   - Adapter `explorer_to_tree_view(state, has_focus, engine)` — port
     from `src/gtk/explorer.rs`. The function is platform-agnostic
     except for the `quadraui::TreeView` it returns; can be largely
     copied.
   - Replace whatever the Win-GUI explorer currently renders with a
     `quadraui_win::draw_tree(...)` call into the shared explorer rect.
2. Click handling: hit-test by row index (fixed row height per the
   primitive). Wire to engine's `open_file` / `toggle_dir` etc. Mirror
   the Linux pattern in `src/gtk/mod.rs::handle_explorer_da_click`.
3. Keyboard: `j/k/h/l/Enter` → engine. Mirror
   `src/gtk/mod.rs::handle_explorer_da_key`.
4. Scroll: mouse wheel → fractional accumulator → row scroll. Mirror
   Linux's `explorer_scroll_accum`.
5. Per-row tooltip / right-click context menu: **deferred** (same as
   the Linux A.2b-2 deferral). Restore later when needed.

**Pre-flight reading (MANDATORY):**

- [`src/gtk/explorer.rs`](src/gtk/explorer.rs) — Linux reference for the
  state model + adapter. The shape ports near-verbatim.
- [`src/gtk/draw.rs::draw_explorer_panel`](src/gtk/draw.rs) — Linux
  reference for the draw-callback structure (build primitive, call
  `quadraui_gtk::draw_tree`, overlay scrollbar).
- [`src/gtk/mod.rs`](src/gtk/mod.rs) — search for
  `handle_explorer_da_click` / `handle_explorer_da_key` /
  `handle_explorer_da_right_click` for click + key + menu wiring.
- [`docs/NATIVE_GUI_LESSONS.md`](docs/NATIVE_GUI_LESSONS.md) — §5
  (click/draw geometry mismatch) is the most likely class of bug.

**Smoke test:**

- Launch `vimcode-win.exe` with the explorer panel open.
- Tree of files / dirs renders with chevrons + icons + indent.
- `j`/`k` navigates rows; `l`/Enter opens / toggles.
- Mouse click selects + opens.
- Scroll wheel scrolls.
- Git indicators / diagnostics badges (if Win-GUI has them) render.
- Multi-group editor layouts don't break.

---

## Win-GUI parity scope (optional, post-A.1c / A.2c)

A.6 and A.7 added Linux-side StatusBar / TabBar / ActivityBar / Terminal
primitives + migrations through quadraui. **Win-GUI was not migrated
through any of those stages** — its bespoke renderers are unaffected
and continue to work as before. Win-GUI is the "newest backend" per
[`CLAUDE.md`](CLAUDE.md) and historically lags features.

**You don't have to do these to "finish" the wave.** A.1c + A.2c are
the only Windows stages tracked as required. Everything below is
optional polish — landing them brings Win-GUI up to feature parity
with the Linux GTK side and demonstrates that the quadraui primitives
work across all three rendering backends (Direct2D, Cairo, ratatui).

| Optional stage | Adds | Linux reference | Estimated size |
|----------------|------|-----------------|----------------|
| A.6b-win | Win-GUI `quadraui_win::draw_status_bar` | `src/gtk/quadraui_gtk.rs::draw_status_bar` (~120 lines) + `src/gtk/draw.rs::draw_window_status_bar` wrapper (~30 lines) | ~200 lines |
| A.6d-win | Win-GUI `quadraui_win::draw_tab_bar` | `src/gtk/quadraui_gtk.rs::draw_tab_bar` (~340 lines) | ~400 lines |
| A.6f-win | Win-GUI `quadraui_win::draw_activity_bar` + native→DA atomic switchover | `src/gtk/quadraui_gtk.rs::draw_activity_bar` + `src/gtk/mod.rs` adapter / wiring (~500 lines total) | ~500 lines |
| A.7-win | Win-GUI `quadraui_win::draw_terminal_cells` | `src/gtk/quadraui_gtk.rs::draw_terminal_cells` (~95 lines) + `src/gtk/draw.rs` wrapper (~25 lines) | ~150 lines |

**Scope each as its own branch** following the established pattern
(`quadraui-phase-a6b-status-bar-win`, etc.), one stage per commit,
smoke test before merge.

**Adapters are already shared.** All the `render::*_to_quadraui` builders
(`window_status_line_to_status_bar`, `build_tab_bar_primitive`,
`terminal_cells_to_quadraui`) live in `src/render.rs` and are platform-
agnostic. Win-GUI just needs the `quadraui_win::draw_*` rasterisation
functions and to call the existing adapters from its own draw paths.

**Lessons learned in A.6 / A.7 that apply to Win-GUI:**

- **Wide-glyph allowlist, not range check.** `is_nerd_wide` started as
  a PUA range test and broke 6 snapshot tests. Specific glyphs are
  rendered wide; others aren't. Hardcode the allowlist (currently
  4 chars: F0932 SPLIT_RIGHT, F0143 DIFF_PREV, F0140 DIFF_NEXT,
  F0233 DIFF_FOLD). Direct2D's text layout will need the same care —
  measure each PUA glyph empirically before deciding.
- **WidgetId-based action dispatch (A.6a precedent).** For StatusBar +
  TabBar, the engine-side action enums (`StatusAction`,
  `TabBarClickTarget`) are encoded as opaque `WidgetId` strings in
  the primitive (e.g. `"status:goto_line"`, `"tab:diff_prev"`).
  Decoder helpers (`render::status_action_from_id`,
  `activity_id_to_panel`) live next to the encoders. Follow the same
  pattern in Win-GUI's click handlers.
- **Rendering vs interaction state split.** Hover / drag / focus are
  per-frame backend state, not primitive state. Pass them as extra
  parameters to the `draw_*` function alongside the primitive (e.g.
  `hovered_close_tab: Option<usize>`, `hovered_idx: Option<usize>`).
  Keeps the primitive plugin-friendly without bloating it.
- **Build the primitive once per frame, not per row.** A.7's first
  draft built the `quadraui::Terminal` per-row in a loop (huge waste).
  Build once at the top of the render call, dispatch row-by-row from
  the owned data. Same applies to `draw_tab_bar` etc.

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
