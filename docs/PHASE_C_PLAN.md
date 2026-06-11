# Phase C — editor paint primitive (Phase 1) + chrome cleanup

> **Status**: Stages 2–4 ✅ shipped (Session 341, 2026-04-29).
> Stage 1 (editor primitive) deferred to a dedicated session because
> it's ~2000 LOC of careful work on its own.
>
> **Predecessor**: B.5c → B.5e → #266 → #267 → #270 → #271 (closed).
> **Successor**: B.6 (Win-GUI rebuild) — still parked behind Stage 1
> so it inherits the editor paint shape automatically once
> `quadraui::win_gui::draw_editor` lands.
>
> **Shipped**:
> - ✅ Stage 2 ([#277](https://github.com/JDonaghy/vimcode/issues/277)) — `Scrollbar` primitive + dual rasterisers + visibility + page-jump fixes.
> - ✅ Stage 3 ([#278](https://github.com/JDonaghy/vimcode/issues/278)) — `draw_settings_chrome` helpers in both backends.
> - ✅ Stage 4 ([#279](https://github.com/JDonaghy/vimcode/issues/279)) — `MessageList` primitive + dual rasterisers.
>
> **Deferred follow-ups filed during this session**:
> [#280](https://github.com/JDonaghy/vimcode/issues/280) (extension panel),
> [#281](https://github.com/JDonaghy/vimcode/issues/281) (debug sidebar),
> [#282](https://github.com/JDonaghy/vimcode/issues/282) (source control).

## Context

The B.5c → #271 arc just shipped: every quadraui primitive has lifted
rasterisers and full `Backend` trait coverage; the runner-crate vision
works on both TUI and GTK; vimcode's chrome (status bar, tabs, dialogs,
palette, tree, etc.) consumes shared layout + paint code from
`quadraui::*` across both backends.

**What's still duplicated** between the two binaries (`vcd` = TUI,
`vimcode` = GTK) falls into two buckets:

1. **The editor "widget" itself** —
   `src/tui_main/render_impl.rs::render_window` (~600 lines) and
   `src/gtk/draw.rs::draw_window` (~800 lines) paint the same data
   (`RenderedWindow` / `RenderedLine`) into different surfaces. The
   decision logic (what to draw, in what order, with what colors) is
   100% identical; only cell-vs-pixel emission differs.
2. **Five remaining sidebar surfaces** — debug sidebar, source control
   panel, extension panel, AI sidebar, settings panel chrome — plus
   the scrollbar paint, all hand-rolled per-backend on top of
   already-shared `ScreenLayout` data.

**Why now**: The primitive surface is solid; the runner-crate vision
(`AppLogic` + `quadraui::{tui,gtk}::run`) is shipped end-to-end. The
editor is the biggest remaining chunk of duplicated paint code, and
the SQL-client framing ("query pane with vim keybindings + LSP")
makes the editor's data-shape a candidate for a published
`quadraui::Editor` primitive. **Phase 1 lifts the *paint* without
taking on the much harder *engine slice* extraction.**

**Intended outcome**:

- A `quadraui::Editor` primitive + dual rasterisers, with vimcode
  painting through them on both backends. -1500 to -2000 LOC
  vimcode-private paint code.
- Three "easy chrome" lifts (settings panel chrome, scrollbar, AI
  sidebar) closing the smallest remaining gaps. -300 to -400 LOC.
- Three remaining chrome surfaces (extension panel, debug sidebar,
  source control) tracked as follow-up issues.
- Phase 2 (engine-slice extraction for editor) explicitly deferred to
  a separate later wave.

## Recommended approach

Land as four independent Path-A merges, each fully tested + verified
before the next, mirroring the pattern from #266→#271:

### Stage 1 — `quadraui::primitives::editor::Editor` primitive + rasterisers (largest, riskiest)

**New module** `quadraui/src/primitives/editor.rs`:

- `pub struct Editor` mirroring `render::RenderedWindow`. Carries
  `lines: Vec<EditorLine>`, `cursor: Option<EditorCursor>`,
  `scroll_top: usize`, `scroll_left: usize`, `gutter_chars: usize`,
  `selection: Option<Selection>`, `find_matches: Vec<Match>`, optional
  `dap_current_line`, etc.
- `pub struct EditorLine` mirroring `render::RenderedLine`:
  `spans: Vec<StyledSpan>`, `gutter_text`, `git_status`,
  `is_cursorline`, `is_diff`, `is_dap_current`, `bp_state`,
  `diagnostic`, `fold_marker`, `virtual_text`,
  `indent_guides: Vec<u16>`, `color_columns: Vec<u16>`,
  `wrap_continuation: bool`, etc.
- `pub struct EditorCursor`, `Selection`, `BreakpointState`,
  `GitLineStatus`, `DiagnosticSeverity` — likely all already exist in
  `quadraui::types` or need lifting.
- No layout function needed (the engine pre-builds the line list +
  cursor position; rasteriser just walks).

**New rasterisers**:

- `quadraui/src/tui/editor.rs::draw_editor(buf, rect, editor, theme)`
  — port of `tui_main/render_impl.rs::render_window` body verbatim.
  Same paint categories in same order: bg fills (cursorline / diff /
  DAP-stopped) → gutter (BP, git, line numbers, diagnostic icon,
  lightbulb, fold marker) → text spans → indent guides → color
  columns → virtual text → diagnostic underlines → spell underlines
  → bracket-match highlights → selection overlay → yank-flash →
  scrollbars.
- `quadraui/src/gtk/editor.rs::draw_editor(cr, layout, rect, editor, theme, line_height, char_width)`
  — port of `gtk/draw.rs::draw_window`.

**Theme growth**: likely 2-4 new fields on `quadraui::Theme` for
editor-specific colors (cursorline_bg, diff line tints, breakpoint
marker colors). Mapped from vimcode's rich `render::Theme` via
`q_theme()` adapter. Pattern matches the +5 fields #266 added.

**Vimcode adoption**:

- `core::engine::Engine::build_editor_for_window(window_id) -> quadraui::Editor`
  (or equivalent in `render.rs`) — a builder that fills the primitive
  from existing engine state. `RenderedWindow` keeps existing for
  transition compatibility.
- TUI: `tui_main/render_impl.rs::render_window` body collapses to
  `quadraui::tui::draw_editor(buf, rect, &editor, &theme)` with the
  small per-backend chrome (e.g. tab bar, status bar) staying around
  it.
- GTK: `gtk/draw.rs::draw_window` body collapses similarly.
- Click handlers (`tui_main/mouse.rs`, `gtk/click.rs`) already operate
  on engine state directly — no changes there for Stage 1.

**Critical files to modify**:

- New: `quadraui/src/primitives/editor.rs`,
  `quadraui/src/tui/editor.rs`, `quadraui/src/gtk/editor.rs`
- `quadraui/src/primitives/mod.rs` — add `pub mod editor;`
- `quadraui/src/lib.rs` — add re-exports
- `quadraui/src/tui/mod.rs`, `quadraui/src/gtk/mod.rs` — add module +
  re-export
- `quadraui/src/theme.rs` — add new editor theme fields
- `src/tui_main/quadraui_tui.rs` (`q_theme`),
  `src/gtk/quadraui_gtk.rs` (`q_theme`) — populate new fields
- `src/render.rs` — add
  `build_editor_primitive(engine, window_id) -> quadraui::Editor`
  builder
- `src/tui_main/render_impl.rs::render_window` — collapse to
  delegator
- `src/gtk/draw.rs::draw_window` — collapse to delegator

**Reuse**: `RenderedWindow` / `RenderedLine` field shapes are the
source of truth. **Don't redesign — translate verbatim.** The TUI
tests in `core::engine::tests` exercise editor state through to
`RenderedWindow`; they continue to validate end-to-end via the new
primitive without modification.

### Stage 2 — Scrollbar primitive (easy chrome win)

**Why first of the chrome**: the math is already proven identical
between backends (`tui_main/mouse.rs::scrollbar_grab_offset` ↔ GTK
equivalent), and `quadraui::dispatch::DragState` already tracks
scrollbar drag on both sides. Just the paint is duplicated.

**New module** `quadraui/src/primitives/scrollbar.rs`:

- `pub struct Scrollbar { track: Rect, thumb: Rect, axis: ScrollAxis, has_focus: bool }`
- `pub fn fit_thumb(scroll_top, total, visible, track_len) -> (thumb_top, thumb_len)`
  — the existing math factored out.

**New rasterisers**: `quadraui::tui::draw_scrollbar`,
`quadraui::gtk::draw_scrollbar`. Both consume the precomputed thumb
geometry; no further math.

**Adoption**: `tui_main/render_impl.rs:2451-2580` (vertical +
horizontal) and `gtk/draw.rs:779-822` collapse to delegators.

**Critical files**:

- New: `quadraui/src/primitives/scrollbar.rs`,
  `quadraui/src/tui/scrollbar.rs`, `quadraui/src/gtk/scrollbar.rs`
- Module wiring: `quadraui/src/{primitives,tui,gtk}/mod.rs`, `lib.rs`
- `src/tui_main/render_impl.rs`, `src/gtk/draw.rs` — collapse paint
  sites

### Stage 3 — Settings panel chrome (smallest)

The form body already paints through `quadraui::Form`. Only the
header row (` SETTINGS`) and the search row (` / ` + query + cursor)
are still hand-painted in both backends.

**Approach**: extract the chrome as a small helper — not a new
primitive, just a wrapper function in each rasteriser file that paints
the chrome then delegates to `draw_form`. Keep `quadraui::Form` itself
unchanged.

**Critical files**:

- `quadraui/src/tui/form.rs` and `quadraui/src/gtk/form.rs` — gain
  `draw_settings_chrome` helpers that paint the header + search row,
  callable as a thin wrapper before / around `draw_form`.
- Or: a thin
  `draw_form_with_chrome(rect, form, header_text, search_query, search_cursor)`
  variant.
- `src/tui_main/panels.rs:790-1200`, `src/gtk/draw.rs:4372-4600` —
  collapse to delegator.

### Stage 4 — AI sidebar (small)

Message list with alternating user/assistant backgrounds. Currently
192 LOC TUI + 230 LOC GTK; structurally a `ListView` variant.

**Approach**: build the AI panel through `quadraui::ListView` with
custom row backgrounds. Either:

- Extend `ListView` with an optional
  `row_bg_override: Vec<Option<Color>>` field (most invasive but most
  reusable), OR
- Add a thin `quadraui::primitives::message_list::MessageList`
  wrapper that consumes a
  `Vec<MessageEntry { author_kind, text }>` and produces
  alternating-bg paint.

**Critical files**:

- New (option 2): `quadraui/src/primitives/message_list.rs`,
  `quadraui/src/tui/message_list.rs`,
  `quadraui/src/gtk/message_list.rs`
- `src/tui_main/panels.rs:3028-3220`, `src/gtk/draw.rs:5595-5825` —
  collapse.

### Defer list

File these as follow-up GitHub issues during the session, do **NOT**
attempt this round:

- **Extension panel** lift (~12 hrs, depends on extending `TreeView`
  with section headers).
- **Debug sidebar** lift (~16 hrs, four sections with per-section
  scrollbars; hit-test is hand-rolled per #210).
- **Source control panel** lift (~24 hrs, complex due to inline
  commit-message editing and split panes).
- **Phase 2: editor engine-slice extraction** (`editor_core` crate
  carving out `keys.rs` + `buffer_manager` + LSP + syntax + git from
  vimcode for true SQL-client embedding) — separate multi-month wave.

## Verification

For each stage independently (run after each lift, before merging):

```sh
# 1. Build both feature combos
cd /home/john/src/vimcode
cargo build --no-default-features
cargo build

# 2. Clippy clean
cargo clippy --no-default-features -- -D warnings
cargo clippy -- -D warnings

# 3. Test suites
cargo test --no-default-features
cd quadraui && cargo test --features tui --features gtk

# 4. Other consumers of quadraui still build
cd .. && cargo build -p kubeui && cargo build -p kubeui-gtk

# 5. Examples (Stage 1 only — verifies Editor primitive can be built)
cd quadraui
cargo build --example tui_app --features tui
cargo build --example gtk_app --features gtk
```

**Smoke targets per stage**:

- **Stage 1 (editor)**: open vimcode + vcd, navigate around (j/k/h/l,
  page up/down, scroll), verify cursor / line numbers / git diff
  markers / syntax colors / selection / diagnostic squiggles /
  cursorline / scrollbars / wrap continuation all render identically
  before vs after. Open a file with active LSP diagnostics + git
  changes + a fold to exercise the harder paint categories.
- **Stage 2 (scrollbar)**: drag the editor scrollbar in both
  backends; verify thumb position + size match before vs after. Drag
  mid-document, near top, near bottom.
- **Stage 3 (settings)**: open `:set` settings panel, scroll, search,
  edit a value. Header + search row should paint identically.
- **Stage 4 (AI sidebar)**: open AI panel (`:Ai` or activity bar
  icon), send a message, verify alternating user/assistant
  backgrounds.

**Cross-stage**: at end of session, run the full quality gate one
more time before updating `PLAN.md` / `PROJECT_STATE.md` and pushing
the docs sync commit.

## PLAN.md / PROJECT_STATE.md updates (last commit of session)

After all stages land, the project's `PLAN.md` should be updated to
reflect the new "Next priority" focus:

- **Phase C — duplication cleanup**: replaces "B.6 Win-GUI rebuild"
  as the immediate next focus. List Stages 1-4 as shipped. List the
  deferred chrome surfaces (extension / debug / source-control) and
  the deferred Phase 2 (editor engine slice) as the remaining tracked
  work for *after* C.
- **Phase B.6 (Win-GUI rebuild)** moves from "next" to "after C".
  Rationale: with the editor primitive lifted, B.6's Win-GUI rebuild
  benefits — Win-GUI inherits the editor paint shape automatically
  once `quadraui::win_gui::draw_editor` exists.

## Stage ordering rationale

Stage 1 (editor) is largest but lowest *integration* risk because the
data shape (`RenderedWindow`) is already battle-tested across both
backends — it's literally the existing source of truth. The risk is
*paint correctness* (lots of categories), and that's verified by
smoke + the existing engine test suite which exercises the data
shape.

Stages 2-4 are easy follow-ups; doing them in the same session keeps
the dedup story coherent and gives the user 3-4 separate Path-A
merges to land at their pace.

Stage 1 first means the editor lift has fresh attention. If
energy/time runs out mid-session, Stages 2-4 can be picked up next
session — they're independent.
