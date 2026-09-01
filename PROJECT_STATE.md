# VimCode Project State

**Last updated:** September 1, 2026 — **audit session, and everything it found is now queued.** Closed the platform-neutrality audit by filing the five pockets nobody had issue-shaped, re-scoping three stale issues (#593, #657, #47), moving #146 out of #7, and putting the whole thing on the drive queue as **16 entries in two parallel chains**. `vimcode#727` and `quadraui#596` are running. Milestone #7 is 10 open / 29 closed, and every one of the ten is queued.

## Active milestone: #7 Platform-Neutral

**The north star is [`GOALS.md`](GOALS.md): eliminate all platform-specific code from
vimcode and lift it into quadraui.** Milestone **#7 Platform-Neutral** is the consume
side (vimcode adopts a shipped quadraui API and *deletes* its bespoke per-backend code);
milestone **#5 Cross-Platform UI Crate** is the supply side (building quadraui itself).
Don't conflate them. A native Windows/macOS backend gets re-added as a thin wrapper once
there is no feature logic left in the existing backends to re-implement.

### Done

- **Both structural migrations closed 2026-08-26.** #448 (GTK → `ShellApp::handle`) and
  #595 (TUI → `ShellApp` + `run_with_shell`). `fn event_loop` no longer exists in `src/`.
  `impl ShellApp for App` at `src/gtk/mod.rs:8123`; `TuiShellApp` at
  `src/tui_main/shell_app.rs:1251`.
- **The orphaned GTK paint path is gone.** #669/#670/#671/#672 painted 13 of the 14
  dropped `ScreenLayout` fields on GTK's live path and deleted `src/gtk/draw.rs`
  (−2,327 production lines). #676 recovered the Command Center, the one sibling the
  epic's `draw.rs`-shaped method could not have found.
- **Dedup sweep landed:** #621 (`fuzzy_score` → `quadraui::text_util`), #659 (driver tab
  geometry + `SidebarSystem::reveal`), #660 (four duplicates retired, incl. `SplitTree`),
  #536 (activity-bar keyboard nav → `AppShell` cursor).
- Milestone #7 stands at **29 closed / 10 open** (4 at the time of the audit, plus the
  six issues the audit filed); quadraui milestone #9 ("vimcode
  Platform-Neutral blockers") is closed out.

### The queue — everything known, in two parallel chains

```
quadraui#666 ✅ ─┬─ vimcode#727 ▶ → #728 → #730 → #593 → #731 → #732 → #733
                 │                → #734 → #735 → #657 → #47
                 └─ quadraui#596 ▶ → #597 → vimcode#658
```

Strict chains: every vimcode entry declares `src/gtk/mod.rs`, so they cannot run
concurrently and `coord`'s #2247 overlap predictor enforces the order. The two chains are
in different repos, so they run in parallel and never stale each other's Test verdicts —
which also means the ">2 issues per repo" drive-queue caution does not apply here: at most
one vimcode entry is ever tested-but-unmerged, so there is no `coord merge --revalidate`
drain to do.

| Order | Issue | Scope |
|---|---|---|
| 1 | **#730** | `#592-E` — paint `screen.ai_panel` on GTK. The 14th and last field from #592's table; #670 deferred it to a follow-up nobody filed (`mod.rs:9364-9369`, click holdout `:7325`). **Closes #592.** |
| 2 | **#593** | `Ctrl+V` on GTK. Unblocked by #672; #646's `GtkDriver` supersedes its stale "needs live smoke" plan. Smallest user-visible fix in the chain. Given a `## Files` block so the overlap predictor can order it. |
| 3 | **#731** | 22 Relm4-era widget handles permanently `None`, guarding ~103 unreachable arms. **Also re-derives #723**, whose landed fix (`e02a824`) targets a `gtk4::Scrollbar` that is never constructed. |
| 4 | **#732** | Retire the GTK `Msg` bus — 124 variants, 301 sites, 684-line `dispatch`, 16 `handle_*_msg` methods. |
| 5 | **#733** | Converge the two mouse routers — ~4,800 lines, one precedence ladder written twice. |
| 6 | **#734** | Converge keyboard dispatch — ~2,000 lines. The TUI half carries 19 `mirrors mod.rs:NNNN` comments and **every one of those line refs is stale**. |
| 7 | **#735** | Converge frame composition — ~4,500 lines. The hard one: units differ (px vs cells), `draw_frame` has raw-`Buffer` residue `render_content` structurally cannot reach, and painter models differ intrinsically. |
| 8 | **#657** | The oracle loop. Last, deliberately — Stage 1 rewrites every `crate::` path in the three modules the chain is about to shrink by ~9,000 lines. |
| 9 | **#47** | Native macOS GUI, **re-scoped**: a thin wrapper over `quadraui::macos::shell_runner::run_with_shell`, not Core Graphics. |
| ∥ | **quadraui#596 → #597 → #658** | The preview tier. #596/#597 were open, unassigned and **in nobody's queue** while #658 sat blocked on them — the supply-side trap `GOALS.md` exists to catch. Queued 2026-09-01. |

### The five pockets — all now issue-shaped

The previous revision of this file listed four pockets as untracked. All are filed, plus a
fifth it missed:

| Pocket | Size | Issue |
|---|---|---|
| Mouse/click routing | ~4,800 lines | #733 |
| GTK `Msg` bus | 124 variants / 301 sites | #732 |
| Frame composition | ~4,500 lines | #735 |
| Orphaned Relm4 widget handles | 22 fields / ~103 arms | #731 |
| **Keyboard dispatch** | ~2,000 lines | **#734** — missed by the previous revision entirely |

### What draining the chain will and will not achieve

Production lines, `#[cfg(test)]` excluded:

| | 2026-05-01 | 2026-07-01 | 2026-09-01 |
|---|---|---|---|
| `src/gtk/` | 18,979 | 13,388 | **12,588** |
| `src/tui_main/` | 14,657 | 10,305 | **11,135** |
| `src/render.rs` (shared) | 10,547 | 12,690 | **15,110** |

The May→July drop was real; **since July 1 the backends have been flat** (23,693 →
23,723) while `render.rs` grew +2,420. New work goes shared — the Platform-Neutrality Rule
is holding — but the existing mass stopped coming down, and `draw.rs`'s −2,327 was
cancelled by ordinary feature growth.

Draining the chain should remove **~8,700–9,500 production lines** from the backends
(#731 ~1,000, #732 ~1,100, #733 ~3,000–3,500, #734 ~1,200, #735 ~2,500), landing near
**14,000–15,000**, with perhaps +4,000–5,000 added to `render.rs`.

**That is a 38% cut and it is not "thin event-to-engine wiring."** What it buys is that
every *decision* — which surface was hit, which handler owns a key, what order a frame is
composed in — is stated once. What remains has **not been enumerated**: rasteriser
adapters, `src/gtk/css.rs` (508 lines), window/CSD wiring, clipboard provider setup, font
metrics. Some of that is legitimately platform-specific and should stay. **Re-run the
sizing audit when #735 lands** rather than assuming the chain finishes the job.

### Trust gate — accepted as a deliberate trade

**#657** is queued *after* the whole chain, so every fix ahead of it is verified by tests
its own author wrote — precisely the failure mode #657 exists to close, with #553 as the
in-repo proof (it shipped `GtkDriver` tests that stayed green with the bug reinstated).

This is a trade, not an oversight: promoting `gtk`/`render`/`tui_main` into the lib first
means rewriting every `crate::` path across code that #731–#735 then delete, and
re-resolving those conflicts on every subsequent PR. **Reversible with one `drive-queue`
re-chain** if the risk is judged too high.

Note also that #657's body opens by declaring a 2026-08-10 freeze on vimcode bug-fix
dispatch until the oracle lands. That has not been honoured — 20+ bug-fix/feature issues
merged 08-26 → 09-01. It is retired, or #657 moves to the front. Recorded on the issue as
an operator decision.

### Milestone hygiene

**#146** (Lua plugin API → quadraui primitives) moved **out** of #7 to Editor Features: it
is an *addition*, and every other #7 issue is a deletion or a convergence, so leaving it
in made the burndown mean two things. **#47** was put in #5 Cross-Platform UI Crate for
the same reason (`GOALS.md` defines #5 as covering "the macOS/Windows backends").

Result: **#7 is 10 open / 29 closed, and all ten are queued.**

### Supply side: the macOS gate is cleared

**quadraui#465** — the `ShellApp` + `run_with_shell` composition support this file and
`GOALS.md` both named as "the actual gate on 'the macOS port is a thin wrapper'" — closed
**2026-08-31** (`bd92d6f` + `434e1d6`). It is present at vimcode's currently pinned rev
`69fd9cdd` (`quadraui/src/macos/shell_runner.rs:24`), so **no pin bump is needed** to
start #47. Nothing on the quadraui side now blocks a macOS backend; the remaining gate is
vimcode-side and it is the chain above.

---

> Feature documentation lives in **README.md**. Sessions 389 and earlier in
> **SESSION_HISTORY.md**. No multi-stage wave is in flight — **PLAN.md** is history until
> the next one opens.

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

**Status (2026-09-01):** **Paint duplication is done for every
surface in the table below** — all ✅ on both backends. The
GTK-side regression that #540 introduced (surfaces painted only
by the since-deleted `draw.rs`) was swept by #669–#672; the one
holdout is `ai_panel`, which has no row here because it was never
migrated to a primitive on GTK at all.

No bespoke section-walk paint code remains (debug sidebar moved to
`MultiSectionView` in #296 — both paint and click consume one cached
layout per frame). What remains cross-backend is **not paint**: it is
the mouse-routing and event-dispatch duplication listed under
"Untracked residual" above, plus intrinsic-to-surface divergences
(Cairo painter order vs ratatui cell coalescence).

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
- ⏭️ Win-GUI removed (Session 363). Will be re-added as a thin wrapper when quadraui ships its Win backend (quadraui#19–#31).

---

## Recent Work

> Sessions 389 and earlier in **SESSION_HISTORY.md**.

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
