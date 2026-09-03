# VimCode Project State

**Last updated:** September 3, 2026 — **the platform-neutrality chain drained, and the audit it mandated is now run.** Milestone #7 is **0 open**: everything the 2026-09-01 audit filed landed, including 16 slices it never named (#751–#766). The post-#735 sizing audit — which the previous revision explicitly warned not to skip — is below, and it **missed its projection by roughly 60%**. Nothing is in flight and nothing is queued for vimcode or quadraui. The most actionable thing on this page is that **#47 was closed having shipped zero code, with its blocker filed nowhere.**

## Active milestone: #7 Platform-Neutral — **complete (0 open)**

**The north star is [`GOALS.md`](GOALS.md): eliminate all platform-specific code from
vimcode and lift it into quadraui.** Milestone **#7 Platform-Neutral** is the consume
side (vimcode adopts a shipped quadraui API and *deletes* its bespoke per-backend code);
milestone **#5 Cross-Platform UI Crate** is the supply side (building quadraui itself).
Don't conflate them.

### What landed

The 2026-09-01 audit filed ten issues and queued them in two parallel chains. All ten
closed, along with the slice chains that #733/#734/#735 turned out to need:

| Convergence | Parent | Slices that did the work |
|---|---|---|
| Mouse routing — one precedence ladder, was written twice | #733 | #751 → #756 |
| Keyboard dispatch — incl. the 19 stale `mirrors mod.rs:NNNN` pointers | #734 | #757 → #762 |
| Frame composition — `FrameOp` / `compose_frame`, one walk per backend | #735 | #763 → #766 |

Also closed: **#730** (`ai_panel` paint, closing epic #592), **#593** (GTK `Ctrl+V`),
**#731** (22 permanently-`None` Relm4 handles + ~103 unreachable arms), **#732** (the GTK
`Msg` bus — 124 variants, 301 sites, a 684-line `dispatch`), **#658** (preview tier),
**#480**, **#550**, **#551**. **#146** moved out to **#4 Editor Features** as
recommended — it is an addition, not a deletion, and it was making the burndown mean two
things.

Two structural landmarks fell with them:

- **#657 shipped `[lib] vimcode_core`** (`eb745e2`). `render`, `tui_main` and `gtk` are
  promoted out of the `vimcode` / `vcd` binaries into the library, and
  `tests/acceptance/` is sealed. The oracle loop is available to this repo for the
  first time — see `tests/acceptance.rs` and `docs/ARCHITECTURE.md`.
- **#766 deleted `draw_frame`** (`eedebf8`), the last raw-`ratatui::Frame` path. The
  #735 staging question ("enumerate the raw-`Buffer` residue first") resolved exactly as
  the previous revision predicted: it was `#[cfg(test)]`-gated and dead in production,
  and the three test-only helpers went with it.

Still true from earlier in the arc: `fn event_loop` does not exist in `src/`;
`src/gtk/draw.rs` is deleted; both `ShellApp` migrations (#448, #595) are closed.

### The post-#735 sizing audit — run on `develop @ eedebf8`

Production lines, `#[cfg(test)]` excluded. **All four columns measured with the same
script** (`scripts/prod_lines.py`, added for this audit) so they are comparable:

| | 2026-05-01 | 2026-07-01 | pre-chain 2026-08-31 | **now 2026-09-03** |
|---|---|---|---|---|
| `src/gtk/` | 18,969 | 13,675 | 12,526 | **9,650** |
| `src/tui_main/` | 14,649 | 10,358 | 11,125 | **10,345** |
| **both backends** | 33,618 | 24,033 | 23,651 | **19,995** |
| `src/render.rs` (shared) | 10,574 | 12,807 | 15,009 | **21,405** |

**Projected vs. actual over the chain (08-31 → 09-03):**

| | projected | actual |
|---|---|---|
| Backends | −8,700 … −9,500, landing near 14,000–15,000 | **−3,656, landing at 19,995** |
| `render.rs` | +4,000 … +5,000 | **+6,396** |
| Net across the three files | ≈ −4,000 | **+2,740** |

Where the reduction came from:

| File | pre-chain | now | Δ |
|---|---|---|---|
| `src/gtk/mod.rs` | 10,518 | 7,684 | **−2,834** |
| `src/tui_main/panels.rs` | 1,554 | 1,208 | −346 |
| `src/tui_main/mouse.rs` | 3,211 | 2,895 | −316 |
| `src/tui_main/shell_app.rs` | 4,109 | 3,989 | −120 |
| `src/gtk/click.rs` | 751 | 696 | −55 |
| `src/gtk/util.rs` | 303 | 250 | −53 |
| `src/gtk/css.rs` | 507 | 507 | 0 |

`gtk/mod.rs` is 78% of the entire cut. `tui_main/mouse.rs` — the file #733 was sized
against at −3,000…−3,500 — lost **316 lines**.

> **Correcting the record.** The `src/gtk/` figure this file previously carried as
> "12,588 at 2026-09-01" was measured *before* #727/#728/#730 landed; it matches the
> pre-chain 08-31 column, not the 09-01 tree. The 05-01 and 07-01 figures also differ
> from the previously recorded ones (by 10–290 lines) for the same reason. That is the
> whole argument for `scripts/prod_lines.py`: **regenerate, don't re-type.**

### What the chain bought, stated honestly

Every *decision* — which surface was hit, which handler owns a key, what order a frame is
composed in — is now stated once in `render.rs`, and both backends walk it. Delegation
density is high: `src/gtk/mod.rs` makes 424 `render::` calls. That is a durable
correctness win, and it is also *why* the net line count went up — the shared
op-sequence machinery (`FrameOp`/`compose_frame`, the routers) costs more lines than the
duplicate pair it replaced.

**It is not "thin event-to-engine wiring."** 19,995 production lines across two backends
is a long way from the north star, and the remaining gap should not be planned as small.

### What remains — four items, none of them queued

1. **The irreducible surface is recorded but never aggregated.** The slices did the
   honest thing and recorded verdicts in code where convergence was rejected on the
   merits. **Nine anchors** carry a *"one-sided" / "do not converge" / "intrinsic
   difference"* verdict — four in `src/render.rs`, two in `src/tui_main/mouse.rs`
   (#751 and #752), one each in `src/gtk/mod.rs`, `src/gtk/testing.rs` and
   `src/tui_main/shell_app.rs`. Find them with:
   `grep -rn -iE "do not converge|not converged|one-sided|intrinsic difference" src/`
   Nobody has turned those into one "this is the per-backend surface that stays"
   statement, which is what would let anyone judge how far 19,995 is from done.
2. **The duplication moved down into quadraui and is unqueued.** **quadraui#481**
   (shared runtime core — 1,671 non-trivial lines byte-identical between `gtk/*.rs` and
   `macos/*.rs`, `EventOutcome` declared twice verbatim, the resize debounce written
   twice and absent on macOS) and **quadraui#482** (Backend API integrity — trait
   asymmetry, unit leaks, a UTF-8 boundary fix living as 7 private copies). Both open,
   both un-milestoned, neither in any queue.
3. **#47's blocker is filed nowhere** — see below.
4. **The divergence bug class is still ~44 issues deep** (#206, #420, #264, #194, #233
   and friends), plus milestone #5's cross-backend residue (#149, #167, #168, #233,
   #294). `GOALS.md`'s thesis is that each is a symptom of a duplicated surface; if the
   convergence had reached far enough this list would be shrinking. It is the only
   outcome measure this goal has that isn't a line count — watch it.

### ⚠️ #47 was closed having shipped no code

**#47 (native macOS GUI) is closed and its diff is documentation only.** Commit
`44882e9` — *"re-audit at pickup, no code — Backend-trait Rc-handle gap blocks Stage 1"* —
recorded the real blocker: `App` calls `GtkBackend::modal_stack_handle()` /
`drag_state_handle()` at 44 call sites in the drag and modal dispatch paths. Those are
**inherent methods on the concrete struct, not on the generic `quadraui::Backend`
trait**, and `MacBackend`'s trait equivalents (`modal_stack_mut`, `drag_and_modal_mut`)
return short-lived `&mut` borrows that cannot be stashed and reused the way `App` does.
Full findings and two candidate API shapes are in [`PLAN.md`](PLAN.md).

That commit's own recommendation — *"file this as a quadraui issue before any
vimcode-side Stage 1 code is written"* — **was never carried out.** No open quadraui
issue mentions either method name. The finding now lives only in `PLAN.md`, attached to a
**closed** issue, which is precisely where the next triage pass will not look.

**This is `GOALS.md`'s supply-side trap in a new shape.** The documented failure mode was
"infra lands in quadraui but the #7 adoption issue never gets picked up." This is the
inverse: the consume-side issue was *closed* while its supply-side blocker went
unrecorded. The rule that would have caught it is now written down in `GOALS.md`: **a #7
issue that turns out to be supply-blocked stays open behind its blocker; it does not get
closed.**

**Action:** file the gap on `JDonaghy/quadraui` (or fold it into quadraui#482, its
natural home), re-open quadraui milestone #9 "vimcode Platform-Neutral blockers", and
re-open vimcode#47 behind it.

### Milestone hygiene

- **#7 is 0 open.** #146 moved to #4 Editor Features; #47 sits in #5 Cross-Platform UI
  Crate, which `GOALS.md` defines as covering the macOS/Windows backends.
- **quadraui milestone #9** ("vimcode Platform-Neutral blockers") is closed out and
  should be re-opened to hold the #47 blocker.
- **Stale Win-GUI issues.** Roughly a dozen open `Win-GUI:` issues (#160–#178, #61,
  #172, #176) describe a backend **deleted from this repo on 2026-05-11** (`3e4bcff`).
  Their live counterparts are quadraui#19–#31 / quadraui#580. They should be migrated or
  closed rather than left to imply `src/win_gui/` still exists.

### A note on line numbers in this file

There are none, deliberately. Locate code by **symbol**, not coordinate:
`grep -n "impl quadraui::ShellApp for App" src/gtk/mod.rs` and friends. Where a *count*
appears it is evidence measured on a named revision — regenerate it
(`python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs`) rather than trusting
it. #734 existed in the first place because `src/tui_main/` carried 19
`mirrors mod.rs:NNNN` comments whose targets had all drifted.

---

> Feature documentation lives in **README.md**. Sessions 389 and earlier in
> **SESSION_HISTORY.md**. No multi-stage wave is in flight — **PLAN.md** holds the #47
> re-audit findings and is otherwise history.

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

**Status (2026-09-03):** **Paint duplication is done for every
surface in the table below** — all ✅ on both backends. The
GTK-side regression that #540 introduced (surfaces painted only
by the since-deleted `draw.rs`) was swept by #669–#672, and the
last holdout, `ai_panel`, was painted on GTK by #730.

No bespoke section-walk paint code remains (debug sidebar moved to
`MultiSectionView` in #296 — both paint and click consume one cached
layout per frame). The mouse-routing, keyboard-dispatch and
frame-composition duplication that this note used to point at as
"untracked residual" was converged by #751–#766: both backends now
walk one `FrameOp` sequence built by `render::compose_frame`, and
`draw_frame` — the last raw-`ratatui::Frame` path — is deleted (#766).

What remains cross-backend is the set of rungs the slices
**deliberately declined to converge**, each with its verdict recorded
at the call site (`grep -rn -iE "do not converge|one-sided|intrinsic difference" src/`),
plus intrinsic-to-surface divergences (Cairo painter order vs ratatui
cell coalescence, px vs cell units). See "What remains" above — that
set has never been aggregated into one statement, and doing so is the
next piece of the north star's own work.

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
| Editor hover popup (markdown + code-hl + selection + scroll + links) | `RichTextPopup` | ✅ | ✅ | #214 shipped (`c8a23e9`); rasterisers lifted via #266 (`779f6e8`); paint migrated to `Surface::RichTextPopup` via `frame.draw()` in #469 / PR #487 (`1912cd3`). Both backends consume `quadraui::{tui,gtk}::draw_rich_text_popup` through the trait. |
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
- `render::compute_editor_layout(engine, total_height, line_height, menu_in_viewport) -> EditorLayout` — one-shot layout computation for all chrome heights (#386). GTK passes pixel units, TUI passes `line_height=1.0` for row units. Replaces `gtk_editor_bottom`, `gtk_terminal_target_maximize_rows`, TUI `terminal_target_maximize_rows_tui`, and the unused `editor_bottom_px`.
- `Engine::handle_completion_click(CompletionsHit) -> bool` — click-to-pick on completion popup (#288). Both backends cache `CompletionsLayout` from render, call `hit_test()` at click time. `Item(idx)` → apply + dismiss, `Inert` → dismiss, `Empty` → dismiss + fall through.
- `Engine::context_menu_hit_to_idx()` + cached `ContextMenuLayout` — context menu click/hover via `hit_test()` (#210). Both backends cache layout from render. GTK motion handler + click handler + TUI click + motion handlers all replaced with shared `hit_test()`. `resolve_context_menu_click()` gated to `#[cfg(test)]`.
- `Engine::resolve_bottom_panel_zone()` + `BottomPanelGeometry` — cached vertical geometry for bottom panel zone detection (#418). Explicit `toolbar_y`/`content_y`/`content_row_h` offsets (not uniform `row_h`) so GTK's taller tab bar gets correct zones. Both backends cache at paint time.
- `Engine::handle_terminal_split_click(TerminalSplitHit) -> bool` + cached `TerminalSplitLayout` — terminal split divider detection, pane focus, and selection via quadraui `hit_test()` (#430, quadraui#196). Both backends cache split layout from `build_terminal_draw_data()`. Zero per-backend divider math.
- `quadraui::AppShell` + `engine::sidebar` — sidebar visibility and active panel owned by the engine (#385). TUI reads all state from `engine.app_shell`; panel switching, focus flags, and session persistence handled by engine methods (`toggle_sidebar_panel`, `focus_sidebar_panel`, `handle_nav_overflow`). GTK `sync_sidebar_from_engine()` reads engine state; `sync_sidebar_widgets()` updates GTK widget visibility via `active_panel_id: String` + lookup-table arrays (#408/#409 removed `SidebarPanel` enum). ExtPanel panels bypass AppShell — `sync_sidebar_from_engine()` checks `ext_panel_active` (#413).

**North-star ("developer doesn't need to know the backend") status after B.5:**

- ✅ True for picker / status-bar / tree / dialog / context-menu / tooltip-shaped surfaces — adding a new instance means writing data + handlers, never touching Pango/cells.
- ✅ True for **rich-document** popups since #214 shipped + #266 lifted both rasterisers — adding new rich popups means writing a `RichTextDocument` and handlers, never touching Pango/cells.
- ⚠️ **Hit-test glue partially shared** (#210/#344) — screen-level zone detection (tab bar, window, divider, breadcrumb) and window sub-zone detection (gutter, status bar, scrollbar, text area) now shared via `render::screen_zone_hit_test` + `window_zone_hit_test`. GTK caches ScreenLayout from paint (#344). Remaining per-backend: motion-handler → `selected_idx` wiring for primitive surfaces (#210), tab bar inner slot resolution (Pango vs char-cell).
- ❌ No `Backend::watch_file(path) -> Stream<FileEvent>` trait method — every backend rolls its own watcher (TUI poll, GTK GIO). Suppress decision is shared (#201) but not the watcher invocation.
- ✅ **Editor viewport lifted** (Phase C Stage 1 / #276). Both backends paint through `quadraui::{tui,gtk}::draw_editor`. The vim-motion-suite vision (PLAN.md) is now unblocked at the paint layer; engine-slice extraction (Phase 2 — `editor_core` crate carving out `keys.rs` + buffer + LSP) remains as a separate multi-month wave.
- ⏭️ Win-GUI removed from this repo on 2026-05-11 (`3e4bcff`). Will be re-added as a thin wrapper when quadraui ships its Win backend (quadraui#19–#31, quadraui#580). The `Win-GUI:` issues still open on *this* tracker describe that deleted backend — migrate or close them (see Milestone hygiene above).

---

## Recent Work

> Sessions 389 and earlier in **SESSION_HISTORY.md**.

**2026-09-03 — the chain drained; #7 closed out; the audit run.** #751–#756 converged
mouse routing, #757–#762 keyboard dispatch, #763–#766 frame composition (`FrameOp` /
`compose_frame`, then the deletion of `draw_frame`). #657 promoted `render`/`tui_main`/
`gtk` into `[lib] vimcode_core` and sealed `tests/acceptance/`. #730/#593/#731/#732/#658/
#480/#550/#551 all closed; #146 moved to #4. Milestone #7 reached **0 open**. Ran the
post-#735 sizing audit the previous revision mandated and added `scripts/prod_lines.py`
so it is reproducible: backends **−3,656** against a −8,700…−9,500 projection, `render.rs`
**+6,396**, net **+2,740**. #47 closed having shipped **no code** (`44882e9`) with its
`Backend`-trait Rc-handle blocker filed nowhere — the top open action.

**2026-09-01 — platform-neutrality audit, and everything it found is now queued.**
Filed #730 (`ai_panel`), #731 (orphan handles), #732 (`Msg` bus), #733 (mouse routers),
#734 (keyboard), #735 (frame composition). Re-scoped #593 (unblocked, `GtkDriver`
supersedes its smoke plan), #657 (audit run and recorded, fixture list corrected, freeze
contradiction flagged) and #47 (macOS: thin wrapper, not Core Graphics). Moved #146 out
of #7. Queued all of it plus quadraui#596/#597 — 16 entries, two parallel chains. #592
given an audit comment and deliberately **left open** on `ai_panel`. Docs: PRs #729
(PROJECT_STATE + PLAN) and #736 (GOALS).

**2026-08-26 → 09-01 — the #592 epic and the dedup sweep cleared.** #669/#670/#671/#672
(GTK live-path paint + `draw.rs` deletion), #676 (Command Center), #673/#674/#677 (tab
MRU, jump-list pane identity, vacuous-test rewrites), #621/#659/#660/#536 (dedup),
#691 (quadraui pinned as a git rev instead of a sibling path dep), #693/#694/#695
(menu-bar paint + hamburger), #699–#705 (VS Code chrome-metrics parity), #35 (minimap
primitive, both backends), #710/#712 (omnibar + dropdown fonts), #715/#716/#719/#720
(WM identity, titlebar glyphs, app icon), #722/#723 (per-pane minimap, scroll thumb).

**2026-08-26 — both `ShellApp` migrations closed.** #448 (GTK) and #595 (TUI).
`fn event_loop` deleted from `src/` (#634).
