# VimCode Project State

**Last updated:** May 16, 2026 (Session 379 — **PR maintenance + clipboard dedup landing.** Rebased PR #414 (SidebarPanel enum removal) onto develop, resolved 3 merge conflicts in `src/gtk/mod.rs` preserving both the string-based panel ID refactor and develop's borrow-safety/ext-panel/focus-guard fixes. Merged. Rebased PR #419 (clipboard dedup), found and fixed RefCell double-borrow crash in `TerminalCopySelection` handler (Rust 2021 temporary lifetime). Merged. Closed #408, #409, #417.)

## Active milestone: Cross-Platform UI Crate

**This is the current top priority.** All quadraui primitive migrations must complete before moving to other milestones. The goal is zero bespoke per-backend code — every UI surface paints, scrolls, and handles clicks through quadraui's shared API. A native Windows backend will be re-added as a thin wrapper when the quadraui Win backend ships (quadraui#19–#31).

**All bespoke paint surfaces are now eliminated.** Every UI surface in both TUI and GTK paints through quadraui primitives. **Scroll dispatch consolidation (#307) is complete** — all scrollable surfaces route through `dispatch_scroll`/`dispatch_click`.

**All vimcode-side milestone work is complete.** Remaining open issues in the milestone are quadraui-side infrastructure (#294, #168, #167, #149, #145, #144, #140, #139, #169) — primitives, research, and validation apps that live in the quadraui repo.

---

Vimcode at 1963 lib tests passing, 5257 total (lib+integration).

> Sessions 378 and earlier in **SESSION_HISTORY.md**.

> Feature documentation lives in **README.md**.
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
| Palette (all pickers: file/symbol/cmd/branch) | `Palette` | ✅ | ✅ | #402: all pickers route through `picker_panel_to_palette()` → `quadraui::Palette`. Preview panes + tree items. `PaletteLayout` for hit-test. `PickerGeometry` for popup bounds. |
| Find/replace overlay | shared hit-regions | ✅ | ✅ | engine-side `compute_find_replace_hit_regions` |
| Terminal cells + scrollbar + split | `Terminal` + `TerminalSplitLayout` | ✅ | ✅ | #353. `build_terminal_draw_data()` shared; both call `Backend::draw_terminal`. Themed scrollbar via `TerminalScrollbar { inverted: true }`. |
| LSP hover popup (simple) | `Tooltip` | ✅ | ✅ | slice 1, `e1e76cd` |
| Signature help popup | `Tooltip{styled_lines}` | ✅ | ✅ | slice 2, `aaa9a3c` |
| Diff peek popup | `Tooltip{styled_lines}` | ✅ | ✅ | slice 3, `e6650fa` |
| Dialog (quit/close confirm) | `Dialog` | ✅ | ✅ | slice 5, `7768a25` |
| Context menu (right-click) | `ContextMenu` | ✅ | ✅ | slice 6, `7ce0f5d` |
| Menu dropdown (top menu bar) | `MenuSystem` | ✅ | ✅ | #319. Owned by `MenuSystem::render()` + `MenuOverlay`. |
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
| Source control panel | `SidebarSystem` (4 sections) | ✅ | ✅ | #321/#339/#340. `populate_sc_sidebar_system` + `SidebarSystem.render()`. Unified dispatch via `dispatch_sc_sidebar_key_unified`. Section badges + visibility (quadraui#103). |
| Bottom panel tabs (Terminal / Debug Output) | `TabBar` | ✅ | ✅ | #304, `5d7fa09`. Adapter `render::build_bottom_panel_tab_bar`. Click via `Engine::handle_bottom_tab_bar_click`. `show_tab_close: false`, `compact: true`. |
| Terminal toolbar (find bar + tab strip) | `StatusBar` / `TabBar` | ✅ | ✅ | #305, `08dd916`. Adapter `render::build_terminal_toolbar`. Click via `Engine::resolve_terminal_toolbar_click`. Tab strip uses `compact: true`. |
| Menu bar labels | `MenuSystem` | ✅ | ✅ | #319. `quadraui::MenuSystem` owns all state + rendering. `MenuOverlay` helper for GTK overlay DA. |
| Command center (nav arrows + search box) | `CommandCenter` | ✅ | ✅ | #310, `b5fdd7d`. Adapter `render::build_command_center_view`. Click via `CommandCenterLayout::hit_test`. |
| Search panel (chrome + results) | `SidebarSystem` (Form + Tree) | ✅ | ✅ | #323/#333/#334. `populate_search_sidebar_system` + `SidebarSystem.render()`. Unified dispatch via `dispatch_search_sidebar_key_unified`. Form: query/replace TextInput + ToggleGroup + ButtonRow. Tree: file-grouped results with collapse. |

**Cross-backend logic-sharing** (where one implementation drives both backends):

- All primitive `Layout` algorithms (`StatusBarLayout`, `PaletteLayout`, etc.) — single implementation, both backends consume.
- `quadraui::dispatch_scroll/click/mouse_down/drag/up` + `ModalStack` + `DragState` — drives all scroll wheel routing, scrollbar thumb-drag + track-page, palette drag, picker drag. All scrollable surfaces registered as `ScrollSurface` at paint time (#307, completed Session 353).
- Engine-side hit-region builders (`compute_find_replace_hit_regions`) and cell-unit fit algorithms (`StatusBar::fit_right_start`, `TabBar::fit_active_scroll_offset`) — parameterised over a measurement closure so each backend supplies its native unit.
- `core::settings::SAVE_REVISION` — one source of truth both file watchers consult (#201).
- All `*_to_form` / `*_to_tree_view` / `lsp_status_for_buffer` adapters in `render.rs` and `core/engine/`.
- `quadraui::MenuSystem` — menu bar + dropdown lifecycle (open/close, keyboard nav, hover-to-switch, modal stack). Both backends call `render()` and `handle()` with zero per-backend menu logic. GTK uses `MenuOverlay` helper for the titlebar DA overlay wiring.
- `quadraui::TreeController` — explorer file tree: selection, scroll, keyboard nav, inline editing (rename + new-file/folder), **scrollbar rendering + interaction** (#415, quadraui#193). Both backends call `render()` for drawing (including built-in 8px/1-cell scrollbar) and route mouse events through `handle()` for scrollbar thumb drag, track click, and row selection. `_via` methods for keyboard editing. All domain logic in `engine/explorer_ops.rs`.
- `quadraui::SidebarSystem` — extensions sidebar (#336/#337/#338), source control panel (#321/#339/#340), and search panel (#323/#333/#334): section selection, scroll, keyboard nav, mouse handling, collapse, badges, visibility. Search panel uses `SectionKind::Form` for the chrome section (quadraui#105). Both backends call `populate_*()` + `render()` and `dispatch_*_key_unified()`. Zero per-backend nav/click code.
- `quadraui::StatusBarInteraction` — debug toolbar hover/press state. TUI uses it via UiEvent intercept; GTK manual wiring produces identical results (#331 verified and closed).
- `render::build_terminal_draw_data()` + `Backend::draw_terminal` — terminal cell grid + themed scrollbar + split-pane layout. Both backends call one shared builder, then `draw_terminal`. Zero per-backend terminal rendering code (#353).
- `render::build_tab_drop_groups()` + `compute_tab_drop_zone()` + `compute_tab_drop_overlay()` — tab drag-and-drop drop-zone computation (delegates to `quadraui::compute_drop_zone()`) and overlay geometry (highlight rect, insertion bar, ghost position). Both backends build a `tab_slots_map` (backend-specific measurement) and `DropGroupBounds`, then call shared functions. Zero per-backend drop-zone algorithm code (#345).
- `render::screen_zone_hit_test()` + `window_zone_hit_test()` + `resolve_gutter_action()` — screen-level click zone detection (tab bar, window, breadcrumb, divider), window sub-zone detection (gutter, status bar, scrollbar, text area), and gutter action resolution. GTK caches `ScreenLayout` from paint; both backends call shared functions for zone detection. Tab bar inner slot resolution (Pango vs char-cell) stays per-backend (#344).
- `render::build_tab_bar_primitive()` + `breadcrumbs_to_quadraui_status_bar()` — tab bar and breadcrumb bar primitives pre-built in `ScreenLayout` (#347). Both backends draw directly from `GroupTabBar.bar` / `BreadcrumbBar.bar` / `ScreenLayout.tab_bar_primitive`. Zero per-backend adapter construction or `show_split` logic.
- `render::picker_panel_to_palette()` + `PickerGeometry` — ALL picker types (file/symbol/command/branch, with/without preview, flat/tree) route through one adapter to `quadraui::Palette`. `PickerGeometry::compute()` + `PickerSizing` constants give a single source of truth for popup bounds. Zero per-backend picker rendering code (#402).
- `Engine::needs_clipboard_for_paste()` + `prepare_paste_clipboard()` — paste-key detection and clipboard register loading (#381). Both backends call the same two engine methods before `handle_key()`. Zero per-backend paste detection logic.
- `Engine::clipboard_read` + `clipboard_write` callbacks — clipboard access routed through engine-owned closures (#417). GTK `setup_gtk_clipboard()` wires `gdk4::Display` clipboard once at startup; TUI wires `copypasta` provider. Six GTK call sites (yank sync, paste prep, hover-popup copy, terminal copy/paste, AI panel Ctrl-V) consolidated. Zero per-backend clipboard logic beyond the one-time provider setup.
- `Engine::handle_explorer_mouse_event()` — single-click row dispatch (toggle dir / preview file) for explorer TreeController events (#415). Both backends route mouse events through `TreeController.handle()` → `handle_explorer_mouse_event()`.
- `render::compute_editor_layout(engine, total_height, line_height, menu_in_viewport) -> EditorLayout` — one-shot layout computation for all chrome heights (#386). GTK passes pixel units, TUI passes `line_height=1.0` for row units. Replaces `gtk_editor_bottom`, `gtk_terminal_target_maximize_rows`, TUI `terminal_target_maximize_rows_tui`, and the unused `editor_bottom_px`. Terminal click handlers still use bespoke maximize-snap math (#418).
- `quadraui::AppShell` + `engine::sidebar` — sidebar visibility and active panel owned by the engine (#385). TUI reads all state from `engine.app_shell`; panel switching, focus flags, and session persistence handled by engine methods (`toggle_sidebar_panel`, `focus_sidebar_panel`, `handle_nav_overflow`). GTK `sync_sidebar_from_engine()` reads engine state; `sync_sidebar_widgets()` updates GTK widget visibility via `active_panel_id: String` + lookup-table arrays (#408/#409 removed `SidebarPanel` enum). ExtPanel panels bypass AppShell — `sync_sidebar_from_engine()` checks `ext_panel_active` (#413).

**North-star ("developer doesn't need to know the backend") status after B.5:**

- ✅ True for picker / status-bar / tree / dialog / context-menu / tooltip-shaped surfaces — adding a new instance means writing data + handlers, never touching Pango/cells.
- ✅ True for **rich-document** popups since #214 shipped + #266 lifted both rasterisers — adding new rich popups means writing a `RichTextDocument` and handlers, never touching Pango/cells.
- ⚠️ **Hit-test glue partially shared** (#210/#344) — screen-level zone detection (tab bar, window, divider, breadcrumb) and window sub-zone detection (gutter, status bar, scrollbar, text area) now shared via `render::screen_zone_hit_test` + `window_zone_hit_test`. GTK caches ScreenLayout from paint (#344). Remaining per-backend: motion-handler → `selected_idx` wiring for primitive surfaces (#210), tab bar inner slot resolution (Pango vs char-cell).
- ❌ No `Backend::watch_file(path) -> Stream<FileEvent>` trait method — every backend rolls its own watcher (TUI poll, GTK GIO). Suppress decision is shared (#201) but not the watcher invocation.
- ✅ **Editor viewport lifted** (Phase C Stage 1 / #276). Both backends paint through `quadraui::{tui,gtk}::draw_editor`. The vim-motion-suite vision (PLAN.md) is now unblocked at the paint layer; engine-slice extraction (Phase 2 — `editor_core` crate carving out `keys.rs` + buffer + LSP) remains as a separate multi-month wave.
- ⏭️ Win-GUI removed (Session 363). Will be re-added as a thin wrapper when quadraui ships its Win backend (quadraui#19–#31).

---

## Recent Work

> Sessions 378 and earlier in **SESSION_HISTORY.md**.
