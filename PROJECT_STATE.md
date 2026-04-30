# VimCode Project State

**Last updated:** Apr 29, 2026 (Session 343 — **GTK `Completions` lift ([#285](https://github.com/JDonaghy/vimcode/issues/285)) shipped on `issue-285-gtk-completions-lift`**: `quadraui::gtk::draw_completions` rasteriser added (verbatim port of vimcode's `draw_completion_popup` body — full bg fill, full 4-side border, per-item selected-row highlight, " {label}" via Pango); `quadraui_gtk::draw_completions` shim added (mirror of TUI shim); `src/gtk/draw::draw_completion_popup` body collapsed to a delegator (build `quadraui::Completions` via the existing `render::completion_menu_to_quadraui_completions` adapter, call `Completions::layout(...)`, delegate, return `layout.bounds` for the existing modal-stack click integration). Both backends now paint completions through `quadraui::Completions` — closes the smallest remaining duplication slot from PLAN.md "🎯 NEXT FOCUS" item #1. Net **+99 LOC** of shared paint in quadraui; vimcode-private paint approximately neutral. Quality gate clean: 1950 lib tests + integration tests pass on `--no-default-features`; GTK build + clippy clean. Smoke test pending. Prior session 342 below.

Prior session (342 — Apr 29): **Phase C Stage 1 ([#276](https://github.com/JDonaghy/vimcode/pull/284), merged) shipped end-to-end**: `quadraui::Editor` primitive + dual TUI/GTK rasterisers landed in 5 commits on `issue-276-editor-primitive`. **Stage 1A** (`3fcc7fb`) lifted the supporting types (`DiagnosticSeverity` / `GitLineStatus` / `DiffLine` / `CursorShape` / `SelectionKind` / `CursorPos` / `EditorCursor` / `EditorSelection` / `Style` / `StyledSpan` / `DiagnosticMark` / `SpellMark`) + 29 new `Theme` fields + `q_theme()` chrome+editor split. **Stage 1B** (`ef45610`) added the `Editor` + `EditorLine` data structs (3 unit tests). **Stage 1C** (`c985d58`) lifted the TUI rasteriser via `to_q_editor` boundary adapter; `render_window` collapsed ~470 → ~25 LOC. **Stage 1D** (`5b23718`) lifted the GTK rasteriser; `draw_window` collapsed ~720 → ~25 LOC. **fmt fixup** (`8c8cd24`). Net **−1456 LOC** of vimcode-private paint code; **+1972 LOC** of shared paint in quadraui. quadraui: 290 tests pass (was 287, +3 editor); vimcode `--no-default-features` + clippy clean (1950 lib tests + integration); GTK build + clippy clean; kubeui + kubeui-gtk consumers build clean. Smoke-test follow-up filed: [#283](https://github.com/JDonaghy/vimcode/issues/283) — TUI LSP-diagnostic-dot column collision with breakpoint marker (verbatim-port behaviour). Issue [#276](https://github.com/JDonaghy/vimcode/issues/276) closed.

Prior session (341 — Apr 29): Phase C stages 2–4 shipped end-to-end. [#277](https://github.com/JDonaghy/vimcode/issues/277) (`fbbc85f`/`b952c6a`/`d3abb17`/`2cc2ad9`) lifted the `Scrollbar` primitive + dual rasterisers, fixed visible-track q_theme mapping, page-jump on track click, GTK native v-scrollbar trough visibility, viewport-sized page step, and h-scrollbar position above per-window status line. [#278](https://github.com/JDonaghy/vimcode/issues/278) (`fd08db0`) lifted `quadraui::{tui,gtk}::draw_settings_chrome` helpers. [#279](https://github.com/JDonaghy/vimcode/issues/279) (`8e55720`) lifted the `MessageList` primitive + dual rasterisers. Three deferred chrome lifts filed: [#280](https://github.com/JDonaghy/vimcode/issues/280), [#281](https://github.com/JDonaghy/vimcode/issues/281), [#282](https://github.com/JDonaghy/vimcode/issues/282).

Prior session (340 — Apr 29): #266 + #267 + #270 + #271 all shipped end-to-end. Runner-crate vision real on TUI + GTK; last find_replace primitive gap closed. #266 (`779f6e8`): rich_text_popup + completions lifted. #267 (`8b64217`): `Backend::draw_dialog` + single-Pango-layout refactor. #270 (`b256993` + `f6b5a17`): `GtkBackend` lifted + `quadraui::gtk::run<A: AppLogic>(app)` runner. #271 (`91e89e9`): `FindReplacePanel` lifted; `Theme.accent_bg` added.

**Next priority** (PLAN.md "🎯 NEXT FOCUS"): **Eliminate remaining TUI/GTK duplication.** With Stage 1 landed, the editor viewport is no longer duplicated; the rich editor hover popup is also already lifted (#214 + #266). What's left: GTK `Completions` lift (smallest), [#280](https://github.com/JDonaghy/vimcode/issues/280) extension panel, [#281](https://github.com/JDonaghy/vimcode/issues/281) debug sidebar, [#282](https://github.com/JDonaghy/vimcode/issues/282) source control panel. **B.6 (Win-GUI rebuild) is unblocked** by Stage 1 and sits orthogonal to the duplication-elimination arc — pick it up after the chrome lifts or in parallel.

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 339 are in **SESSION_HISTORY.md**.
> **Active multi-stage wave:** `quadraui` cross-platform UI crate extraction — see **PLAN.md** for pickup-on-another-machine instructions.

---

## Testing Policy

**Every new Vim feature and every bug fix MUST have comprehensive integration tests before the work is considered done.** Subtle bugs (register content, cursor position, newline handling, linewise vs. char-mode paste) are only reliably caught by tests. The process is:

1. Write failing tests that document the expected Vim behavior
2. Implement/fix the feature until all tests pass
3. Run the full suite (`cargo test`) — no regressions allowed

When implementing a new key/command, add tests covering:
- Basic happy path
- Edge cases: start/middle/end of line, start/end of file, empty buffer, count prefix
- Register content (text and `is_linewise` flag)
- Cursor position after the operation
- Interaction with paste (`p`/`P`) to verify the yanked/deleted content behaves correctly

---

## Cross-backend coverage

Snapshot of where each surface stands on its quadraui primitive.
TUI was the reference implementation through Phase C; GTK caught
up. Numbers update with each Path-A landing — read this to find
the next slice.

**Status (post #281, 2026-04-29):** **TUI/GTK paint duplication is
done.** Every entry in the cross-backend coverage table below is ✅
on both backends. Editor viewport lifted (#276), GTK `Completions`
(#285), editor hover popup (#214 + #266 — `RichTextPopup`),
extension panel (#280), source control panel
(`render::source_control_to_tree_view` + `Backend::draw_tree`),
debug sidebar (#281 — four `TreeView` instances, one per section).
TUI chrome 100% on quadraui; GTK chrome 100%. **No paint
duplication remains.** Smaller residual convergence work
(#210/#211/#288-style hit-test/click items, ~hundreds of LOC) plus
intrinsic-to-surface divergences (Cairo painter order vs ratatui
cell coalescence) remain but are tracked separately.

| Surface | Primitive | TUI | GTK | Notes |
|---|---|---|---|---|
| Status bar (per-window + global) | `StatusBar` | ✅ | ✅ | layout via `StatusBarLayout` |
| Tab bar | `TabBar` | ✅ | ✅ | |
| Activity bar | `ActivityBar` | ✅ | ✅ | |
| Tree view (explorer + SC) | `TreeView` | ✅ | ✅ | layout via `TreeViewLayout` |
| List view (quickfix + tab switcher) | `ListView` | ✅ | ✅ | layout via `ListViewLayout` |
| Form (settings) | `Form` | ✅ | ✅ | hint field exists but unrendered (#202) |
| Palette (cmd palette + folder picker) | `Palette` | ✅ | ✅ | layout via `PaletteLayout` |
| Find/replace overlay | shared hit-regions | ✅ | ✅ | engine-side `compute_find_replace_hit_regions` |
| Terminal cells | `Terminal` | ✅ | ✅ | |
| LSP hover popup (simple) | `Tooltip` | ✅ | ✅ | slice 1, `e1e76cd` |
| Signature help popup | `Tooltip{styled_lines}` | ✅ | ✅ | slice 2, `aaa9a3c` |
| Diff peek popup | `Tooltip{styled_lines}` | ✅ | ✅ | slice 3, `e6650fa` |
| Dialog (quit/close confirm) | `Dialog` | ✅ | ✅ | slice 5, `7768a25` |
| Context menu (right-click) | `ContextMenu` | ✅ | ✅ | slice 6, `7ce0f5d` |
| Menu dropdown (top menu bar) | `ContextMenu` | ✅ | ✅ | slice 6 (closed #181) |
| Debug toolbar | `StatusBar` | ✅ | ✅ | slice 8, `caf62a8` |
| Breadcrumb bar | `StatusBar` | ✅ | ✅ | slice 8 |
| Editor hover popup (markdown + code-hl + selection + scroll + links) | `RichTextPopup` | ✅ | ✅ | #214 shipped (`c8a23e9`); rasterisers lifted via #266 (`779f6e8`). Both backends consume `quadraui::{tui,gtk}::draw_rich_text_popup`. |
| Completion popup | `Completions` | ✅ | ✅ | #285 — GTK lifted to `quadraui::gtk::draw_completions` |
| Editor scrollbar (v + h paint) | `Scrollbar` | ✅ | ✅ | #277, `fbbc85f`+ |
| Settings panel chrome (header + search row) | `draw_settings_chrome` | ✅ | ✅ | #278, `fd08db0` |
| AI sidebar message history | `MessageList` | ✅ | ✅ | #279, `8e55720` |
| Editor viewport (text + gutter + cursor + selection + diagnostics) | `Editor` | ✅ | ✅ | #276, `5b23718`+ (Phase C Stage 1) |
| Extension panel | `TreeView` (with `Decoration::Header`) | ✅ | ✅ | #280, `d29d1b4`. Adapter `render::ext_sidebar_to_tree_view`. Click via `TreeViewLayout::hit_test()` on both backends. |
| Debug sidebar (variables tree, breakpoints, watch) | `TreeView` × 4 (one per section) | ✅ | ✅ | #281, `f3d78d6`. Adapter `render::debug_sidebar_section_to_tree_view` builds one `TreeView` per section. Click via `TreeViewLayout::hit_test()` on both backends. |
| Source control panel | `TreeView` (with `Decoration::Header`) | ✅ | ✅ | #282 already shipped — `render::source_control_to_tree_view` adapter + `Backend::draw_tree` on both backends. Table previously claimed bespoke; reconciled here. |

**Cross-backend logic-sharing** (where one implementation drives both backends):

- All primitive `Layout` algorithms (`StatusBarLayout`, `PaletteLayout`, etc.) — single implementation, both backends consume.
- `quadraui::dispatch_mouse_down/drag/up` + `ModalStack` + `DragState` — drives palette drag, picker drag, TUI sidebar scrollbar drag, and GTK explorer scrollbar drag (as of `3e5d7d3`).
- Engine-side hit-region builders (`compute_find_replace_hit_regions`) and cell-unit fit algorithms (`StatusBar::fit_right_start`, `TabBar::fit_active_scroll_offset`) — parameterised over a measurement closure so each backend supplies its native unit.
- `core::settings::SAVE_REVISION` — one source of truth both file watchers consult (#201).
- All `*_to_form` / `*_to_tree_view` / `lsp_status_for_buffer` adapters in `render.rs` and `core/engine/`.

**North-star ("developer doesn't need to know the backend") status after B.5:**

- ✅ True for picker / status-bar / tree / dialog / context-menu / tooltip-shaped surfaces — adding a new instance means writing data + handlers, never touching Pango/cells.
- ✅ True for **rich-document** popups since #214 shipped + #266 lifted both rasterisers — adding new rich popups means writing a `RichTextDocument` and handlers, never touching Pango/cells.
- ⚠️ **Hit-test glue still per-backend** (#210) — primitive layouts and `hit_test` methods are shared, but the wires from "mouse moved" → "selected_idx changed" are still hand-rolled in each backend's motion handler. Several bugs across the B.5 wave traced back to this (slice 6 row-height drift, slice 8 hand-rolled char-width math). Structural fix: motion handlers should call `layout.hit_test()` directly. The same shape exists in #211 (debug sidebar) and likely a few other surfaces.
- ❌ No `Backend::watch_file(path) -> Stream<FileEvent>` trait method — every backend rolls its own watcher (TUI poll, GTK GIO, future Win-GUI `ReadDirectoryChangesW`). Suppress decision is shared (#201) but not the watcher invocation.
- ✅ **Editor viewport lifted** (Phase C Stage 1 / #276). Both backends paint through `quadraui::{tui,gtk}::draw_editor`. The vim-motion-suite vision (PLAN.md) is now unblocked at the paint layer; engine-slice extraction (Phase 2 — `editor_core` crate carving out `keys.rs` + buffer + LSP) remains as a separate multi-month wave.
- ⏭️ Win-GUI has TreeView / Explorer / StatusBar / TabBar but most of B.3+ hasn't reached Windows. "Cross-platform" currently means ~1.5 platforms.

---

## Recent Work

**Session 342 — #276 Phase C Stage 1 shipped end-to-end (editor primitive):**

The editor viewport — last big duplication between TUI and GTK
paint paths — is now lifted into `quadraui::Editor`. Five commits
on `issue-276-editor-primitive`, merged via [PR #284](https://github.com/JDonaghy/vimcode/pull/284) to develop.

| Stage | Commit | Scope |
|---|---|---|
| 1A | `3fcc7fb` | Supporting types lift (`DiagnosticSeverity`, `GitLineStatus`, `DiffLine`, `CursorShape`, `SelectionKind`, `CursorPos`, `EditorCursor`, `EditorSelection`, `Style`, byte-range `StyledSpan`, `DiagnosticMark`, `SpellMark`) into `quadraui::primitives::editor`. ~29 new `Theme` fields under "Editor lift" banner. Theme drops `Eq` (f32 alpha fields). `q_theme()` adapter splits into `q_theme_chrome` + `q_theme_editor` halves in both backend adapters. Behavioural no-op. |
| 1B | `ef45610` | `Editor` + `EditorLine` data structs added to the same module. Field-for-field mirror of `vimcode::render::RenderedWindow` / `RenderedLine`. `Editor.lightbulb_glyph: char` is the only intentional addition (host populates from icon registry per frame). 3 unit tests cover serde round-trip + DiagnosticSeverity ordering. |
| 1C | `c985d58` | `quadraui::tui::draw_editor` (725 LOC) — verbatim port of `render_impl::render_window` body. `EditorPaintResult { cursor_position: Option<(u16,u16)> }` returned for Bar/Underline shapes (host calls `Frame::set_cursor_position`). `render_text_line` + `render_selection` private to the lifted module. `render::to_q_editor` boundary adapter (165 LOC) added. `render_impl::render_window` collapsed ~470 → ~25 LOC. |
| 1D | `5b23718` | `quadraui::gtk::draw_editor` (776 LOC) — verbatim port of `gtk/draw::draw_window` body. Selection paints **before** text (Cairo painter order); TUI paints after — divergence is intrinsic to the surfaces. `build_pango_attrs` + `draw_visual_selection` private to the lifted module. `gtk/draw::draw_window` collapsed ~720 → ~25 LOC. |
| fmt | `8c8cd24` | Trailing rustfmt cleanup. |

**Net code change**: vimcode-private paint code shrank by **−1456 LOC**;
quadraui gained **+1972 LOC** of shared paint that the upcoming Win-GUI
rebuild (B.6) will consume directly. Total diff: +2427 / −1455.

**Confirmed architectural decisions** (clarified during planning):
- **Theme**: ~29 editor fields lifted into `quadraui::Theme` (single rasteriser arg). `q_theme()` adapter splits internally.
- **Scrollbars**: TUI rasteriser keeps painting them internally via `quadraui::tui::draw_scrollbar` (Stage 2). GTK paints them outside the rasteriser, preserved by the delegator.
- **Module layout**: flat. `quadraui::Editor` / `EditorLine` / `EditorCursor` at the crate root; byte-range `StyledSpan` stays inside `primitives::editor` to disambiguate from existing owned-text `quadraui::StyledSpan`.

**Sharp edges that materialised during the lift**:
- **`StyledSpan` impedance** — owned-text `quadraui::StyledSpan` (plugin/serde) and byte-range `quadraui::primitives::editor::StyledSpan` (paint) coexist by design.
- **`DiagnosticSeverity` lift** — quadraui mirror of `core::lsp::DiagnosticSeverity`; `to_q_severity()` adapter at the boundary.
- **`active_background`** — lifted to `quadraui::Theme::editor_active_background`.
- **Cursor side-effect (TUI Bar/Underline)** — rasteriser returns `EditorPaintResult::cursor_position`; host calls `Frame::set_cursor_position`.
- **`Style.font_scale`** narrowed `f64 → f32` to unblock `Eq`/`Serialize` derives. Pango call site upcasts.
- **Selection paint ordering** — GTK paints before text, TUI paints after. Documented as intrinsic-to-surface; not consolidated.

**Smoke-test follow-up filed**: [#283](https://github.com/JDonaghy/vimcode/issues/283) — TUI LSP-diagnostic dot overwrites breakpoint marker (gutter column collision). Pre-existing behaviour, predates this PR — surfaced during smoke testing because GTK paints both visibly while TUI doesn't.

**What's next:** PLAN.md "🎯 NEXT FOCUS" — eliminate remaining
TUI/GTK duplication via the chrome-lift queue (GTK `Completions`
→ #280 → #281 → #282). B.6 Win-GUI rebuild is unblocked
and orthogonal — pick it up in parallel or after the lifts.

---

**Session 341 — Phase C stages 2–4 shipped end-to-end:**

[#277](https://github.com/JDonaghy/vimcode/issues/277) (`fbbc85f`/`b952c6a`/`d3abb17`/`2cc2ad9`) lifted the `Scrollbar` primitive + dual rasterisers, fixed visible-track q_theme mapping, page-jump on track click, GTK native v-scrollbar trough visibility, viewport-sized page step, and h-scrollbar position above the per-window status line. [#278](https://github.com/JDonaghy/vimcode/issues/278) (`fd08db0`) lifted `quadraui::{tui,gtk}::draw_settings_chrome` helpers — settings panel header + search row paint through quadraui; form body already did via `Form`. [#279](https://github.com/JDonaghy/vimcode/issues/279) (`8e55720`) lifted the `MessageList` primitive + dual rasterisers — AI sidebar message-history paint loop lifted; panel header / separator / input area / focus border stay panel-specific. Three deferred chrome lifts filed: [#280](https://github.com/JDonaghy/vimcode/issues/280), [#281](https://github.com/JDonaghy/vimcode/issues/281), [#282](https://github.com/JDonaghy/vimcode/issues/282). Phase C umbrella [#275](https://github.com/JDonaghy/vimcode/issues/275). quadraui: 287 tests pass (was 278, +9 fit_thumb tests); vimcode `--no-default-features` + clippy clean (5263 tests); GTK build + clippy clean; kubeui + kubeui-gtk consumers build clean.

---

> Session 339 and earlier in **SESSION_HISTORY.md**.
