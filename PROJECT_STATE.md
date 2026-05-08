# VimCode Project State

**Last updated:** May 7, 2026 (Session 357 — **#329 GTK exit coredump** fixed. Root cause was RefCell double-borrow in `reveal_path_in_explorer`, not `process::exit` as originally hypothesized. Three-part fix: `try_borrow_mut` for `reveal_path_in_explorer` + tab-switcher timer, deferred `process::exit` via GLib idle callback, `ShowQuitConfirm` fallthrough guard.)

## Active milestone: Cross-Platform UI Crate

**This is the current top priority.** All quadraui primitive migrations must complete before moving to other milestones. The goal is zero bespoke per-backend code — every UI surface paints, scrolls, and handles clicks through quadraui's shared API. Win-GUI is deferred until quadraui implements that backend.

**All bespoke paint surfaces are now eliminated.** Every UI surface in both TUI and GTK paints through quadraui primitives. **Scroll dispatch consolidation (#307) is complete** — all scrollable surfaces route through `dispatch_scroll`/`dispatch_click`.

**Remaining milestone work** is consolidation and infrastructure:

| # | Title | Category |
|---|-------|----------|
| [#328](https://github.com/JDonaghy/vimcode/issues/328) | Adopt quadraui#77/#78/#79 (scroll_by, double-click, scroll convention) | Cleanup (low priority) |
| [#314](https://github.com/JDonaghy/vimcode/issues/314) | Search panel MSV scrollbar click/drag | Polish |
| [#295](https://github.com/JDonaghy/vimcode/issues/295) | TUI MSV scrollbar drag-to-scroll | Polish |
| [#315](https://github.com/JDonaghy/vimcode/issues/315) | GTK MSV scrollbar drag-to-scroll | Polish |
| [#294](https://github.com/JDonaghy/vimcode/issues/294) | quadraui MSV horizontal axis rasterisers | Infrastructure |
| [#311](https://github.com/JDonaghy/vimcode/issues/311) | Search panel arrow key cursor movement | Polish |
| [#312](https://github.com/JDonaghy/vimcode/issues/312) | Ctrl-Shift-F visual selection prepopulate | Feature |
| [#331](https://github.com/JDonaghy/vimcode/issues/331) | GTK debug toolbar → StatusBarInteraction | Cleanup (blocked on GTK UiEvent migration) |

**Shipped this session (357):**
- **#329 GTK exit coredump** (Path A, `fddd573`) — Root cause: `reveal_path_in_explorer` called `borrow_mut()` while the engine RefCell was already borrowed, panicking through the extern "C" `clicked_trampoline` → abort. Three-part fix: (1) `try_borrow_mut()` for `reveal_path_in_explorer` and the 100ms tab-switcher timer callback, (2) `process::exit` deferred via `idle_add_local_once` so it never runs inside a signal emission chain, (3) `return` after `save_session_and_exit()` in `ShowQuitConfirm` to prevent fallthrough after `-> !` became `-> ()`.

---

**Previous sessions (356 and earlier):** in SESSION_HISTORY.md.

Vimcode at 1931 lib tests passing. All on develop — no active branches.

> Sessions 357 and earlier in **SESSION_HISTORY.md**.

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 348 are in **SESSION_HISTORY.md**.
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
| Palette (cmd palette + folder picker) | `Palette` | ✅ | ✅ | layout via `PaletteLayout` |
| Find/replace overlay | shared hit-regions | ✅ | ✅ | engine-side `compute_find_replace_hit_regions` |
| Terminal cells | `Terminal` | ✅ | ✅ | |
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
| Source control panel | `TreeView` (with `Decoration::Header`) | ✅ | ✅ | #282 already shipped — `render::source_control_to_tree_view` adapter + `Backend::draw_tree` on both backends. Table previously claimed bespoke; reconciled here. |
| Bottom panel tabs (Terminal / Debug Output) | `TabBar` | ✅ | ✅ | #304, `5d7fa09`. Adapter `render::build_bottom_panel_tab_bar`. Click via `Engine::handle_bottom_tab_bar_click`. `show_tab_close: false`, `compact: true`. |
| Terminal toolbar (find bar + tab strip) | `StatusBar` / `TabBar` | ✅ | ✅ | #305, `08dd916`. Adapter `render::build_terminal_toolbar`. Click via `Engine::resolve_terminal_toolbar_click`. Tab strip uses `compact: true`. |
| Menu bar labels | `MenuSystem` | ✅ | ✅ | #319. `quadraui::MenuSystem` owns all state + rendering. `MenuOverlay` helper for GTK overlay DA. |
| Command center (nav arrows + search box) | `CommandCenter` | ✅ | ✅ | #310, `b5fdd7d`. Adapter `render::build_command_center_view`. Click via `CommandCenterLayout::hit_test`. |
| Search panel (chrome + results) | `MSV` + `Form` + `TreeView` | ✅ | ✅ | #302, `de625bb`. Adapter `render::build_search_panel_msv`. Form: query/replace TextInput + ToggleGroup + ButtonRow. Tree: file-grouped results. Click via `backend.form_layout`/`backend.tree_layout`. Replace All has confirmation dialog. |

**Cross-backend logic-sharing** (where one implementation drives both backends):

- All primitive `Layout` algorithms (`StatusBarLayout`, `PaletteLayout`, etc.) — single implementation, both backends consume.
- `quadraui::dispatch_scroll/click/mouse_down/drag/up` + `ModalStack` + `DragState` — drives all scroll wheel routing, scrollbar thumb-drag + track-page, palette drag, picker drag. All scrollable surfaces registered as `ScrollSurface` at paint time (#307, completed Session 353).
- Engine-side hit-region builders (`compute_find_replace_hit_regions`) and cell-unit fit algorithms (`StatusBar::fit_right_start`, `TabBar::fit_active_scroll_offset`) — parameterised over a measurement closure so each backend supplies its native unit.
- `core::settings::SAVE_REVISION` — one source of truth both file watchers consult (#201).
- All `*_to_form` / `*_to_tree_view` / `lsp_status_for_buffer` adapters in `render.rs` and `core/engine/`.
- `quadraui::MenuSystem` — menu bar + dropdown lifecycle (open/close, keyboard nav, hover-to-switch, modal stack). Both backends call `render()` and `handle()` with zero per-backend menu logic. GTK uses `MenuOverlay` helper for the titlebar DA overlay wiring.
- `quadraui::TreeController` — explorer file tree: selection, scroll, keyboard nav, inline editing (rename + new-file/folder). Both backends call `render()` for drawing and `_via` methods for keyboard editing. All domain logic in `engine/explorer_ops.rs`.
- `quadraui::StatusBarInteraction` — debug toolbar hover/press state. TUI uses it via UiEvent intercept; GTK still manual (#331).

**North-star ("developer doesn't need to know the backend") status after B.5:**

- ✅ True for picker / status-bar / tree / dialog / context-menu / tooltip-shaped surfaces — adding a new instance means writing data + handlers, never touching Pango/cells.
- ✅ True for **rich-document** popups since #214 shipped + #266 lifted both rasterisers — adding new rich popups means writing a `RichTextDocument` and handlers, never touching Pango/cells.
- ⚠️ **Hit-test glue still per-backend** (#210) — primitive layouts and `hit_test` methods are shared, but the wires from "mouse moved" → "selected_idx changed" are still hand-rolled in each backend's motion handler. Several bugs across the B.5 wave traced back to this (slice 6 row-height drift, slice 8 hand-rolled char-width math). Structural fix: motion handlers should call `layout.hit_test()` directly. The same shape exists in #211 (debug sidebar) and likely a few other surfaces.
- ❌ No `Backend::watch_file(path) -> Stream<FileEvent>` trait method — every backend rolls its own watcher (TUI poll, GTK GIO, future Win-GUI `ReadDirectoryChangesW`). Suppress decision is shared (#201) but not the watcher invocation.
- ✅ **Editor viewport lifted** (Phase C Stage 1 / #276). Both backends paint through `quadraui::{tui,gtk}::draw_editor`. The vim-motion-suite vision (PLAN.md) is now unblocked at the paint layer; engine-slice extraction (Phase 2 — `editor_core` crate carving out `keys.rs` + buffer + LSP) remains as a separate multi-month wave.
- ⏭️ Win-GUI has TreeView / Explorer / StatusBar / TabBar but most of B.3+ hasn't reached Windows. "Cross-platform" currently means ~1.5 platforms.

---

## Recent Work

> Sessions 351 and earlier in **SESSION_HISTORY.md**.
