# VimCode Project State

**Last updated:** May 2, 2026 (Session 347 — **#166 diff-pane drift fix + #296 Debug sidebar MSV migration shipped**).

Three items landed on develop via Path A:

1. **[#166](https://github.com/JDonaghy/vimcode/issues/166) diff-pane alignment** (`b29a218`). Side-by-side diff panes drifted out of alignment past the first hunk because `sync_scroll_binds` set `partner.scroll_top` to a buffer line that the partner's render resolved at a different aligned-row index. Fix: `view.aligned_top: Option<usize>` pins both panes to one shared aligned-row index; `clear_all_diff_alignment()` helper centralises the teardown. 2 regression tests.

2. **GTK rename catch-up** (`6d70dba`). `multi_section_view_layout` → `gtk_msv_layout` (quadraui renamed the helper during repo extraction; vimcode lagged behind, breaking GTK build).

3. **[#296](https://github.com/JDonaghy/vimcode/issues/296) Debug sidebar → MultiSectionView** (`285916b`). Adopted the validated consumer pattern from [quadraui#1](https://github.com/JDonaghy/quadraui/issues/1). Both TUI + GTK paint through `draw_multi_section_view`; click/scroll read the cached `MultiSectionViewLayout` verbatim (never re-derive). New `render::debug_sidebar_to_multi_section_view` adapter: 4 `EqualShare` sections, `PerSection` scroll. Also fixed pre-existing TUI bug: debug-output panel click handler missing `col >= editor_left` check, silently eating sidebar clicks in overlapping row ranges. Net −139 LOC.

**Key lesson codified** in CLAUDE.md "Paint↔click integration pattern": click NEVER re-derives layout; paint caches it, click reads verbatim. This governs all future quadraui migrations. Captured in memory (`feedback_cache_paint_layout.md`).

Vimcode at 1952 lib + integration tests passing on develop@285916b. Both TUI + GTK build + clippy clean.

**Next priority**: [#282](https://github.com/JDonaghy/vimcode/issues/282) SC→MSV (blocked label removed — quadraui#2 + #3 both closed). Then Win-GUI rebuild (B.6).

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 346 are in **SESSION_HISTORY.md**.
> **Active multi-stage wave:** `quadraui` cross-platform UI crate extraction — see **PLAN.md** for pickup-on-another-machine instructions.

Prior session (346 — Apr 30): Harness-first course correction shipped. Pivoted from failed #296 attempts to structural fix: #297 cell_quantum, #298/#299 harnesses, #300 quadraui extracted to own repo. 4 vimcode issues gained `blocked` label with cross-repo prereq links.

> Sessions 344 and earlier in **SESSION_HISTORY.md**.


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

**Status (post #296, 2026-05-02):** **TUI/GTK paint duplication is
done.** Every entry in the cross-backend coverage table below is ✅
on both backends. Debug sidebar migrated to `MultiSectionView`
(#296) — both paint and click consume one cached layout per frame.
**No bespoke section-walk paint code remains.** Residual convergence
work (#210/#211/#288-style hit-test/click items) plus
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
| Debug sidebar (variables tree, breakpoints, watch) | `MultiSectionView` (4 × `TreeView`) | ✅ | ✅ | #296, `285916b`. Adapter `render::debug_sidebar_to_multi_section_view`. Paint caches layout; click reads verbatim. |
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

**Session 343 — TUI/GTK paint duplication arc closed end-to-end (`develop@7f3498c`):**

Six items shipped via Path A on develop, plus eight follow-up issues filed.

| # | What | Commit |
|---|---|---|
| 283 | TUI BP/diagnostic gutter collision (smoke fallout from #276) | `f0b850f` |
| — | Doc reconciliation: #214 RichTextPopup already shipped | `b96c65d` |
| 285 | GTK Completions popup lift (`quadraui::gtk::draw_completions`) | `345d81f` |
| 286 | GTK Ctrl-alone-dismisses completion popup | `392e89c` |
| 280 | Extension panel TreeView lift (`render::ext_sidebar_to_tree_view`) | `d29d1b4` + `6982462` |
| 281 | Debug sidebar lift onto four `TreeView` instances + 7 smoke fixes | 8 commits, head `7f3498c` |

**Phase C umbrella ([#275](https://github.com/JDonaghy/vimcode/issues/275)) closed.** Cross-backend coverage table is fully ✅✅ on both backends.

**The #281 smoke wave was instructive.** Eight commits to ship one lift, with three dead-end fixes (`d06568c` `38c052e` `c0fdc8a`) that landed before the actual root causes were diagnosed. The two real fixes were `33cfd2b` (GTK paint uses `line_height × 1.4` per row but click hit-test used 1.0× — the root of the 3→4, 6→8 row drift) and `f15a490` (TUI paint computed section heights locally while click read engine state populated from a different base — root of the section-walk drift). Lesson captured: when paint and click share a multi-section panel, they must read from one source-of-truth in one unit. **Filed as architectural follow-up [#293](https://github.com/JDonaghy/vimcode/issues/293) — `MultiSectionView` primitive** that owns the entire layout (titles + scrollbars + per-section trees) so future panels and future backends (Win-GUI, macOS) cannot reintroduce the drift.

**Follow-ups filed (open, prioritised for next sessions):**

- **#293** `MultiSectionView` primitive — architectural; should land before B.6.
- #292 GTK F-keys not reaching debugger when sidebar focused (likely menu-bar interception).
- #287 GTK Ctrl-P palette collision in completion popup.
- #288 completion popup click divergence TUI vs GTK.
- #290 / #291 TUI extension search input issues.
- #289 xterm.js + TUI in-browser demo (parking lot, low priority).

---

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
