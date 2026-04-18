# VimCode — Current Plan

> **Purpose of this file:** Session-level coordination doc for in-flight
> multi-stage features, so work can be picked up on a different machine
> without reconstructing state from scratch. GitHub issues remain the
> source of truth for individual tasks — this file points at the current
> wave and explains how to resume.
>
> **Last updated:** 2026-04-18 (end of Session 297)

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
| **Phase A.1b** — GTK `draw_tree` + GTK SC panel | 🟡 Next | — | `quadraui-phase-a1b-*` | Linux / macOS with GTK4 |
| **Phase A.1c** — Win-GUI `draw_tree` + Win-GUI SC panel | 🟡 Next | — | `quadraui-phase-a1c-*` | Windows |
| Phase A.2 — `TreeView` for explorer | ⬜ Queued | — | `quadraui-phase-a2-*` | any (GTK replaces native TreeView) |
| Phase A.3 — `Form` + settings panel | ⬜ Queued | — | `quadraui-phase-a3-*` | any |
| Phase A.4 — `Palette` (command palette) | ⬜ Queued | — | `quadraui-phase-a4-*` | any |
| Phase A.5 — `ListView` (quickfix, git status list) | ⬜ Queued | — | `quadraui-phase-a5-*` | any |
| Phase A.6 — `StatusBar` / `TabBar` / `ActivityBar` finish | ⬜ Queued | — | `quadraui-phase-a6-*` | any |
| Phase A.7 — `Terminal` primitive | ⬜ Queued | — | `quadraui-phase-a7-*` | any |
| Phase A.8 — `TextDisplay` | ⬜ Queued | — | `quadraui-phase-a8-*` | any |
| Phase A.9 — `TextEditor` + `BufferView` adapter | ⬜ Queued | — | `quadraui-phase-a9-*` | any — biggest stage |
| Phase B — extract & stabilise API | ⬜ Later | — | — | any |
| Phase C — macOS backend | ⬜ v1.x | — | — | macOS |
| Phase D — polish + k8s validation app | ⬜ Later | — | — | any |

A.1b and A.1c are independent — they can be done on different machines in
either order or in parallel.

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

## Phase A.1b — GTK `draw_tree`

**Branch:** `quadraui-phase-a1b-treeview-gtk` off `develop`.

**Platform:** Linux or macOS with GTK4 (4.10+). Cannot be done on Windows
without a GTK4 install, which is unsupported.

**Scope:**

1. Create `src/gtk/quadraui_gtk.rs` with `draw_tree(ctx: &cairo::Context, area: Rect, tree: &quadraui::TreeView, theme: &Theme)`.
2. Port the TUI reference (`src/tui_main/quadraui_tui.rs`) to Cairo/Pango:
   row background fill, chevron, icon (nerd-font-aware via
   `crate::icons::nerd_fonts_enabled()`), styled text, badge.
3. In `src/gtk/draw.rs::draw_source_control_panel()`, replace the
   section-rendering loop with a call to `render::source_control_to_tree_view()`
   followed by `quadraui_gtk::draw_tree(ctx, section_area, &tree, theme)`.
4. Keep the header / commit input / button row / popups untouched — same
   scope boundary as A.1a.

**Reference implementation:** `src/tui_main/quadraui_tui.rs` is the template.
Function-for-function translation to Cairo; only the rasterisation calls change.

**Smoke test after implementing:**

```bash
cargo run   # default GUI
```

- Click the git icon in the activity bar.
- Verify sections render identically to the TUI (chevrons, icons, badges).
- Keyboard nav / Tab / click behaviour unchanged from before.
- Window resize doesn't break the layout.

**Out of scope for A.1b (defer to later stages):**

- `TreeEvent` routing (mouse/key events still flow through existing engine
  methods for A.1; event-based routing is its own later sub-stage).
- Primitive-owned scroll state.
- Context menus.

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
