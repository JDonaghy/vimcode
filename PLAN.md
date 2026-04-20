# VimCode — Current Plan

> **Purpose of this file:** Session-level coordination doc for in-flight
> multi-stage features, so work can be picked up on a different machine
> without reconstructing state from scratch. GitHub issues remain the
> source of truth for individual tasks — this file points at the current
> wave and explains how to resume.
>
> **Last updated:** 2026-04-19 (Session 303 — A.2b split into two sub-phases; A.2b-1 scaffolding ready to merge)

---

## Active wave — `quadraui` cross-platform UI crate extraction

Extracting a reusable UI crate from vimcode per
[`docs/UI_CRATE_DESIGN.md`](docs/UI_CRATE_DESIGN.md). vimcode is the
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
| **Phase A.1c** — Win-GUI `draw_tree` + Win-GUI SC panel | 🟡 Next | — | `quadraui-phase-a1c-*` | Windows |
| **Phase A.2a** — `TreeView` explorer (TUI) + `Decoration::Header` | ✅ Done | `1c4bbd7` | `quadraui-phase-a2a-*` | any (TUI) |
| **Phase A.2b-1** — GTK explorer scaffolding (data model + draw function, inert) | 🟡 Current | — | `quadraui-phase-a2b-*` | any (compiles on all platforms) |
| **Phase A.2b-2** — GTK explorer atomic switchover (native `gtk4::TreeView` → `DrawingArea`) | ⬜ Queued | — | `quadraui-phase-a2b2-*` | Linux / macOS with GTK4 |
| **Phase A.2c** — Win-GUI explorer | ⬜ Queued | — | `quadraui-phase-a2c-*` | Windows |
| **Phase A.3a** — `Form` primitive + TUI `draw_form` | ✅ Done | `4a4b456` | `quadraui-phase-a3a-*` | any |
| **Phase A.3b** — TUI settings panel uses `Form` | ✅ Done | `e708e43` | `quadraui-phase-a3b-*` | any |
| **Phase A.3c** — GTK `draw_form` primitive (migration deferred) | ✅ Done | `3f34a03` | `quadraui-phase-a3c-*` | any |
| **Phase A.3c-2** — GTK settings panel uses `draw_form` (native → DrawingArea) | ✅ Done | `fa44a51` | `quadraui-phase-a3c2-*` | Linux / macOS with GTK4 |
| **Phase A.3d** — `TextInput` cursor + selection in `Form` | ✅ Done | `f7f3a51` | `quadraui-phase-a3d-*` | any |
| **Phase A.4** — `Palette` primitive + TUI command palette | ✅ Done | `534c386` | `quadraui-phase-a4-*` | any |
| **Phase A.4b** — GTK `draw_palette` + GTK command palette | ✅ Done | `c8f2d91` | `quadraui-phase-a4b-*` | Linux / macOS with GTK4 |
| **Phase A.5** — `ListView` primitive + TUI quickfix | ✅ Done | `63d1b29` | `quadraui-phase-a5-*` | any |
| **Phase A.5b** — GTK `draw_list` + GTK quickfix | ✅ Done | `e1ea5ea` | `quadraui-phase-a5b-*` | Linux / macOS with GTK4 |
| Phase A.6 — `StatusBar` / `TabBar` / `ActivityBar` finish | ⬜ Queued | — | `quadraui-phase-a6-*` | any |
| Phase A.7 — `Terminal` primitive | ⬜ Queued | — | `quadraui-phase-a7-*` | any |
| Phase A.8 — `TextDisplay` | ⬜ Queued | — | `quadraui-phase-a8-*` | any |
| Phase A.9 — `TextEditor` + `BufferView` adapter | ⬜ Queued | — | `quadraui-phase-a9-*` | any — biggest stage |
| Phase B — extract & stabilise API | ⬜ Later | — | — | any |
| Phase C — macOS backend | ⬜ v1.x | — | — | macOS |
| Phase D — polish + k8s validation app | ⬜ Later | — | — | any |

A.1c and A.2c need a Windows machine. A.2b is the only remaining Linux
GTK migration — explorer native `gtk4::TreeView` → `DrawingArea` +
`quadraui_gtk::draw_tree`. It has been **split into two sub-phases**
because the atomic switchover is a ~1500-line diff across the view!
macro, the App struct, ~50 scattered `Msg` handlers, and a context-menu
rewrite; landing that as a single commit makes smoke-test regressions
hard to bisect. Sub-phase 1 (scaffolding, inert) establishes a
known-good foundation; sub-phase 2 flips the wiring and deletes the
dead `gtk4::TreeView` code. The lower-risk GTK stages (A.1b, A.3c-2,
A.4b, A.5b) are all done.

Design decisions covering primitive-distinctness (why `ListView` is
separate from `TreeView`, and how `DataTable` #140 should be scoped)
are documented in [`docs/DECISIONS_quadraui_primitives.md`](docs/DECISIONS_quadraui_primitives.md).

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

- **Branches are not automatically headers.** Early `draw_tree`
  implementations (TUI + GTK) applied section-header background styling
  to every branch row (any row with `is_expanded = Some(_)`). That was
  correct for SC (branches are section titles) but wrong for the
  explorer (branches are just directories and should look like sibling
  files). Fix (absorbed into `1c4bbd7`): added `Decoration::Header`.
  Apps tag header rows explicitly; backends style them distinctly.
  `is_expanded`-ness is now purely about chevron rendering. Rule:
  **tree hierarchy and visual emphasis are orthogonal.**

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

**Status:** 🟡 In progress on this branch.

**Branch:** `quadraui-phase-a2b-treeview-explorer-gtk` off `develop`.

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

**Status:** ⬜ Queued (next session).

**Branch:** `quadraui-phase-a2b2-switchover-gtk` off `develop` (after A.2b-1 lands).

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

**Already landed in sub-phase 1** (commit pending as of this edit):

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

## Design invariants that must hold across all stages

From [`docs/UI_CRATE_DESIGN.md`](docs/UI_CRATE_DESIGN.md) §10
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
| [`docs/UI_CRATE_DESIGN.md`](docs/UI_CRATE_DESIGN.md) | Authoritative design. All 13 §7 decisions are resolved. Start here. |
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
