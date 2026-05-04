# VimCode Project State

**Last updated:** May 4, 2026 (Session 349 — #304 bottom panel tabs shipped; quadraui gained `show_tab_close`).

## Active milestone: Cross-Platform UI Crate

**This is the current top priority.** All quadraui primitive migrations must complete before moving to other milestones. The goal is zero bespoke per-backend code — every UI surface paints, scrolls, and handles clicks through quadraui's shared API. Win-GUI is deferred until quadraui implements that backend.

**Remaining bespoke paint surfaces** (3 issues):

| # | Surface | Primitive | Est. |
|---|---------|-----------|------|
| [#302](https://github.com/JDonaghy/vimcode/issues/302) | Search panel (~321 TUI + native GTK) | MSV+TreeView | 12–16 hrs |
| [#301](https://github.com/JDonaghy/vimcode/issues/301) | Menu bar (~119 TUI + ~172 GTK) | `MenuBar` | 6–8 hrs |
| [#305](https://github.com/JDonaghy/vimcode/issues/305) | Terminal toolbar (~73 TUI + ~66 GTK) | `StatusBar` + `TabBar` | 4–6 hrs |

**All quadraui prereqs are now resolved.** quadraui#6 (MenuBar rasterisers) and quadraui#7 (SearchPanel) are both CLOSED — #301 and #302 are ready to work on with no blockers.

**Scroll dispatch migration** ([#307](https://github.com/JDonaghy/vimcode/issues/307)):

#303 established the `ScrollSurface` + `dispatch_scroll` + `dispatch_click` pattern. [#307](https://github.com/JDonaghy/vimcode/issues/307) tracks migrating all remaining scrollable surfaces (editor viewport, terminal scrollback, sidebar panels, debug sidebar, hover popup) to the same shared dispatch — eliminating ~82 lines of bespoke per-backend scroll routing in TUI `mouse.rs` alone, plus equivalent GTK code.

**Shipped this session:**
- **#304** (`5d7fa09`) — Bottom panel tabs → `quadraui::TabBar`. Both backends paint through `build_bottom_panel_tab_bar()` → `Backend::draw_tab_bar()`. Click dispatch via shared `Engine::handle_bottom_tab_bar_click()`. quadraui gained `TabBar.show_tab_close: bool` (`b9d62cd`) to suppress per-tab close buttons.

---

**Previous session (348):** #306 debug sidebar chrome + #303 debug output shipped; platform-neutrality rule established. quadraui #46/#47/#48 built to unblock #303.

Vimcode at 1952 lib + 2040 integration tests passing on develop@`5d7fa09`. Both TUI + GTK build + clippy clean.

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 348 are in **SESSION_HISTORY.md**.
> **Active multi-stage wave:** `quadraui` cross-platform UI crate extraction — see **PLAN.md** for pickup-on-another-machine instructions.

> Sessions 347 and earlier in **SESSION_HISTORY.md**.


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
| Bottom panel tabs (Terminal / Debug Output) | `TabBar` | ✅ | ✅ | #304, `5d7fa09`. Adapter `render::build_bottom_panel_tab_bar`. Click via `Engine::handle_bottom_tab_bar_click`. `show_tab_close: false` suppresses per-tab ×. |

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

> Sessions 343 and earlier in **SESSION_HISTORY.md**.

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
