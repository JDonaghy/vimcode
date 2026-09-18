# Current Goal — North Star

> **The living, primary objective for vimcode and every agent that works on it.**
> This is *meta-level*: above any single issue or session. Both humans and agents
> may edit it as priorities evolve — keep it short, current, and re-date the Status
> line. The `Platform-Neutrality Rule` at the top of `CLAUDE.md` is the *operational
> rule*; **this file is the source of truth for *intent* and *sequencing*.**
>
> _Last updated: 2026-09-16 (#1044 — the `TuiShellApp`/`mouse.rs` rung-decomposition
> audit: 98 rungs enumerated, 26 convergeable and sequenced into 16 child issues
> below, one new `IRREDUCIBLE_SURFACE.md` fact, one quadraui gap drafted. No
> production code changed — see "2026-09-16 audit" section below. Prior entry
> (2026-09-11, macOS native-menu audit — milestone #7 reopened with #901/#902) kept
> for its own history).
> Milestone #7 is **3 open** (#901, #902, #1044 — #1044 itself closes once its
> 16 drafted child issues are filed; they are not yet filed and are not counted
> here)._

## 🎯 North star

**Eliminate all platform-specific code from vimcode and lift it into quadraui.**
`src/gtk/` and `src/tui_main/` should shrink to thin event-to-engine wiring — every
layout, hit-test, paint, and dispatch decision lives in the shared engine
(`render.rs` / `src/core/`) or in a quadraui primitive that both backends call in
1–3 lines. The end state: adding a feature touches `render.rs` once, not each
backend; a new backend (macOS, Windows) is a thin wrapper with no feature logic.

This is the direct corollary of the **Platform-Neutrality Rule** in `CLAUDE.md`:
that rule stops *new* per-backend code from being written; this goal *deletes the
existing* per-backend code that predates quadraui.

## Why this matters

- **Correctness through one code path.** Most live bugs are cross-backend
  divergence — GTK does X, TUI does Y, they drift. One shared implementation kills
  the whole class (see the dozens of open `GTK:`/`TUI:` bug issues; each is a
  symptom of a duplicated surface).
- **Leverage.** Every line lifted into quadraui is reused by coord-tui, kubeui, the
  future macOS/Windows backends, and any other consumer — vimcode stops paying to
  maintain a private UI toolkit.
- **The macOS/Windows backends are gated on this.** A native backend can only be a
  "thin wrapper" if there is no feature logic left in the existing backends to
  re-implement.

## The two-sided model — build vs. adopt

The work splits cleanly across **two milestones**. Don't conflate them:

| Milestone | Repo | What it is |
|---|---|---|
| **#5 Cross-Platform UI Crate** | `JDonaghy/vimcode` (tracking) + `JDonaghy/quadraui` | **Build quadraui itself** — new primitives, validation consumers, the macOS/Windows backends. The *supply* side. |
| **#7 Platform-Neutral** | `JDonaghy/vimcode` | **vimcode adopts a shipped quadraui API and deletes its bespoke per-backend code.** The *consume* side. |

A typical feature flows: gap found → quadraui issue (#5 / quadraui repo) → infra
lands → **vimcode-side adoption issue (#7) deletes the old per-backend code.** The
recurring failure mode this doc exists to fix: **infra lands in quadraui but the #7
adoption issue never gets picked up**, so the bespoke code lingers as tech debt.
**It just happened again in a new shape — see "#47 was closed without its blocker"
below.**

## 🔬 2026-09-16 audit — #1044: TuiShellApp/mouse.rs decomposed into 98 rungs

**What #1044 asked for:** an inventory, not a rewrite. Enumerate every rung of
`impl ShellApp for TuiShellApp` and `mouse::handle_mouse`, against its
`crate::app::App` counterpart, classify each as already-shared / convergeable /
irreducible / quadraui-gap, and produce a sequenced work order. **No production
code changed in this pass** — see `PROJECT_STATE.md` if that ever needs
re-confirming.

### Current sizing (regenerated 2026-09-16, `scripts/prod_lines.py`)

| File | Production lines |
|---|---:|
| `src/gtk/` | **1,020** |
| `src/tui_main/` (all files) | **10,471** |
| — of which `src/tui_main/shell_app.rs` | 4,459 (18,442 incl. tests — the issue's "17,947" cited the whole-file count on an older commit) |
| — of which `src/tui_main/mouse.rs` | 2,881 (3,878 incl. tests) |
| `src/app.rs` | 8,313 |
| `src/render.rs` | 20,997 |
| `src/macos/` | 138 |
| `src/win/` | 178 |

Two things worth stating plainly before the inventory: **GTK is not "1,020 lines
of shell logic"** — it's 1,020 lines of thin wiring calling into `src/app.rs`
(8,313) and `render.rs` (20,997), which is exactly the north-star shape. And
**most of `shell_app.rs`/`mouse.rs`'s bulk is tests, not production** —
18,442 + 3,878 = 22,320 raw lines, of which only 7,340 (33%) is production; the
issue's headline "10,437 TUI production lines vs GTK's 1,020" is real but is a
whole-directory figure (`src/tui_main/`, 5 files), not these two files alone.

### The prerequisite: the conformance harness has no `TuiShellApp` arm yet

The issue's prerequisite — "the conformance harness must be able to see
production-TUI divergence first" — is not hypothetical, it's already the
measured state of `src/harness.rs`/`src/tui_main/mod.rs`. `crate::harness::
ConformanceHarness<TuiDriver<...>>` (`tui_main/mod.rs`'s `testing::
conformance_harness`) deliberately wraps **`crate::app::App`**, not
`TuiShellApp` — its own doc comment explains why: a scenario written once
should exercise "the same dispatch/paint code on every backend," and `App`
(unconditionally compiled, no `gui` gate on the impl block) is the one thing
GTK/macOS/Win/TUI's harnesses all share. That is exactly right for what #982
was building, and exactly why it **cannot** also prove a `TuiShellApp`/
`mouse.rs` rung converged: today, zero `crate::harness` scenarios ever run
against production TUI code. A "converged" rung and a merely *deleted* one look
identical to the harness as it stands.

**P0 (land before any convergence child issue below):** add a `::tui_prod` (or
similarly named) harness arm that wraps the *actual* `TuiShellApp` +
`mouse::handle_mouse` path — mirroring `tui_main::testing::conformance_harness`
but built the way `src/tui_main/shell_app.rs`'s own `#[cfg(test)]` suite drives
`TuiShellApp` directly via `quadraui::tui::testing::driver_with_shell`, not via
`App`. This can reuse that suite's existing fixtures rather than building new
ones. Small, self-contained, unblocks everything else in this section.

### The inventory: 98 rungs, four verdicts

Two independent passes enumerated every rung — one at the `ShellApp`
trait-method level (`setup`/`handle`/`on_shell_event_ctx`/`take_requested_panel`/
`on_bottom_panel_event`/`tick`/`render_content`'s `FrameOp` arms), one across
every branch of `mouse::handle_mouse`. Verdict counts, reconciled (the
command-line-selection rung was flagged by both passes as the same underlying
gap, counted once):

| Verdict | Count |
|---|---:|
| Already-shared | 40 |
| Convergeable (incl. 5 pending a product decision) | 26 |
| Irreducible | 31 — reduces to 3 facts, see `docs/IRREDUCIBLE_SURFACE.md` §1/§7 |
| quadraui-gap | 1 — `CommandLine::selection` paint field, drafted in `docs/PENDING_QUADRAUI_ISSUES.md` |
| **Total rungs** | **98** |

**The headline finding: the mouse-router "epic" #950 flagged is smaller than
feared.** #950 (`docs/SHELLAPP_CONVERGENCE.md`) called converging `mouse.rs`
onto `src/click.rs` "the worst finding" and explicitly scoped it as multi-PR
epic work, not a cheap win. This audit's rung-by-rung walk of all 60 `mouse.rs`
rungs found **30 of them already call the same shared `render::`/`click::`
function `app.rs` does** — the #733/#751–#756/#815/#817/#823/#987 chain already
converged almost all of the actual hit-test/apply *decisions*. What's left
un-shared is not one monolithic reimplementation; it's the 16 named items
below (waves 1–3), plus the architectural cleanup of retiring the
`UiEvent`→crossterm round-trip wrapper once nothing needs it (wave 4) — a
materially smaller and more tractable backlog than #950's framing implied.

#### ShellApp trait-method rungs (`setup`/`handle`/`on_shell_event_ctx`/`take_requested_panel`/`on_bottom_panel_event`/`tick`/`render_content`)

| Rung | Verdict | Note |
|---|---|---|
| `render_content`: 12 of 14 `FrameOp` arms | already-shared | #824. |
| `FrameOp::CommandLine` base paint | irreducible | px/cell paint substrate. |
| `FrameOp::CommandLine` click→offset hit-test | already-shared | `render::command_line_click_char_idx`, quadraui#705 — corrects `IRREDUCIBLE_SURFACE.md` §2a, see §2c. |
| `FrameOp::CommandLine` selection-highlight paint | **quadraui-gap** | `CommandLine::selection` field missing upstream; drafted in `PENDING_QUADRAUI_ISSUES.md`. |
| `FrameOp::TabSwitcher` | convergeable | `max_visible` vs `visible_rows` field mixup — looks like a live bug, not intentional (wave 1, item 4 below). |
| Menu system / Command Center click / sidebar hover / panel-key accelerators / `ClipboardPaste` | already-shared | Each already calls one `render::` router (#752/#754/#755). |
| Window-control buttons, CSD drag/resize, outer-edge resize cursor, native menu/context menu, `CharTyped` IME, `WindowClose`, native-menu `setup`, initial CSS load | irreducible | All one fact — "TUI has no OS window," new row in `IRREDUCIBLE_SURFACE.md` §1. |
| Alt-menu-letter reveal shim, hamburger-corner one-shot guard | irreducible | Flip side of the existing "GTK menu bar is the CSD titlebar" fact. |
| `KeyPressed` decode | convergeable | GTK keeps 4 keys (`BackTab`/`PageUp`/`PageDown`/`Insert`) GTK-spelled instead of routing through the shared `render::engine_key_from_ui` TUI already fully uses (wave 2, item 8). |
| Menu-action → `EngineAction` applier | convergeable | TUI's `dispatch_post_key_action`+`handle_action` vs GTK's `handle_menu_action` — both start from the same shared `Engine::dispatch_menu_action` then hand-roll separate appliers (wave 2, item 11). |
| `PanelChanged`/`SidebarHidden`/`SidebarResized` shadow-`AppShell` sync | convergeable | Same shape, independently written 3×  (wave 2, item 10). |
| `BottomItemClicked` (Settings) | convergeable | TUI toggles, GTK only shows — behavioral drift, no platform reason (wave 1, item 5). |
| `take_requested_panel` | convergeable | GTK doesn't override it (stays `None`) — a real gap *on GTK*, not TUI (wave 2, item 12). |
| `on_bottom_panel_event` | convergeable, low priority | Dead hook on both sides — neither backend sets `ShellConfig.bottom_panel` (blocked the same way `TabGroupController`/`BottomPanelController` adoption already is — see `PENDING_QUADRAUI_ISSUES.md`). |
| `tick` chore lists | irreducible | #950 already classified this; only the *comments* are a (low-priority) doc-consolidation follow-up. |

#### `mouse.rs` rungs

| Area | Verdict | Note |
|---|---|---|
| Modal overlay apply, drag-route apply, chrome click/hover, tab hover, editor hover popup, folder picker, divider grab, minimap, gutter, sidebar hover/resize | already-shared | 30 rungs total — one router each (`render::route_modal_overlay_click`, `route_mouse_drag`, `route_chrome_click`, `route_sidebar_hover`, `route_editor_hover_popup_click`, `route_folder_picker_click`, `route_divider_grab`, `apply_minimap_click`, `apply_gutter_action`), resolving #825's items 1/2/4 as fully converged already. |
| Dead activity-bar block (incl. hamburger `MenuToggle`) | convergeable — **delete** | ~55 lines, confirmed unreachable: `shell_config` registers the hamburger as a real `PanelDefinition`, so `AppShell::handle` intercepts it upstream (wave 1, item 1). |
| `h_sb_drag_cell` field (`app.rs`) | convergeable — **delete** | Write-only, never read anywhere (wave 1, item 2). |
| Dead-shadowed wheel-scroll arms (`explorer:sb`, likely `tui:search_results`) | convergeable — **delete** | Shadowed by earlier direct-dispatch blocks that already `return` first (wave 1, item 3). |
| GTK terminal-split finalize: hardcoded `da_w = 800.0` / `terminal_cols()` hardcoded `80` | **bug, not convergence — file separately** | Live correctness bug found incidentally; out of #1044's scope, needs its own fix issue. |
| Tab-bar post-hit-test dispatch (duplicated 2× in `mouse.rs`, `click::dispatch_tab_bar_target` exists but unused by TUI) | convergeable | Clearest concrete mechanical win (wave 2, item 7). |
| Editor v/h scrollbar click+drag geometry | convergeable | Both sides hand-roll the identical track/thumb math independently (wave 2, item 9). |
| Wheel scroll: GTK missing `PANEL_GIT`/`SEARCH`/`SETTINGS`/ext-panel scroll; TUI missing hover-window (unfocused-pane) scroll | convergeable-pending-design | Mechanical shape known for both; needs a product nod, not a design question (wave 3, items 13/14). |
| Hover-link click-to-copy (TUI-only) | ✅ converged (#1067) | Was a real asymmetry, not intentional — GTK had painted/cached the link rects since the #540 migration but never wired a click reader. Now shared via `render::route_panel_hover_popup_click` (wave 3, item 15). |
| Explorer drag-and-drop finalize | **feature gap, not irreducible-surface** | TUI-only capability, nothing on GTK to converge with — see `IRREDUCIBLE_SURFACE.md` §7's note on scope. |
| Command-line selection (full rung) | quadraui-gap (same as above, cited once) | See `PENDING_QUADRAUI_ISSUES.md`. |
| Tab-bar hit-test, right-click resolution, text-selection drag-origin encoding, chrome-band geometry caching | irreducible | Cell-vs-pixel geometry (`click.rs`'s own documented reason) — instances of the existing frame-metrics fact. |
| Remaining `MouseDown`/`Up`/`Move`/`Scroll` top-level round-trip (`uievent_to_crossterm`) | convergeable, epic, **last** | #950's original finding — now known to be much smaller in practice since the decisions it wraps are already shared (wave 4, item 16). |

### Sequenced work order — cheapest & highest-divergence-risk first

Each item below is sized to be its own small, independently mergeable issue.
None of them are filed yet — this worker session has no `gh` access
(`CLAUDE.md`); coordinator/human: file each as its own vimcode issue into
milestone #7, referencing #1044.

**P0 — prerequisite, land first:** the `::tui_prod` conformance-harness arm
(above). Blocks every item below that claims a "converged" verdict needs
proof, not just the diff looking smaller.

**Wave 1 — dead code + live bugs, zero design risk (do first):**
1. Delete the dead activity-bar block in `mouse.rs` (~55 lines, incl. hamburger
   arm) — add a black-box regression test first (all 8 activity-bar targets +
   hamburger still work via the `ShellApp` path) before deleting.
2. Delete the dead `h_sb_drag_cell` field in `app.rs` (~3 lines, pure refactor).
3. Delete the dead-shadowed wheel-scroll arms (`explorer:sb`, and
   `tui:search_results` pending a quick shadow-confirmation grep).
4. Fix the `FrameOp::TabSwitcher` `max_visible`/`visible_rows` field mixup —
   likely a live, user-visible bug; needs a driver test on both backends
   (many-tabs-than-fit scenario, assert the visible-row count matches the
   intended capped value), observed red against unfixed `develop` first.
5. Fix `BottomItemClicked` toggle-vs-show drift between `TuiShellApp` (toggles)
   and `App` (only shows) — decide the intended behavior, converge, driver
   test on both backends.
6. File separately (not a #1044 child, a plain bug): GTK's terminal-split
   finalize hardcodes `da_w = 800.0`/`terminal_cols() == 80` instead of real
   pixel→cell conversion.

**Wave 2 — mechanical convergence, no design decision needed:**
7. Route `mouse.rs`'s two tab-bar-dispatch matches through `click::
   dispatch_tab_bar_target` (already exists, already backend-neutral, only
   called by GTK today) instead of hand-rolling it twice.
8. Converge `KeyPressed` decode: point GTK's 4 kept-GTK-spelled keys through
   the shared `render::engine_key_from_ui`.
9. Extract `resolve_editor_scrollbar_click(...)` into `render.rs` (same
   `unit_w`/`unit_h`-convention shape as existing shared geometry helpers);
   call from both `mouse.rs` and `app.rs`; delete the two hand-rolled copies.
10. Converge the `PanelChanged`/`SidebarHidden`/`SidebarResized` shadow-
    `AppShell`-sync trio into one shared helper (same `Host`-trait shape as
    `dispatch_panel_accelerator`).
11. Converge the menu-action `EngineAction` applier into a shared
    `apply_engine_action` taking a `Host` trait — larger; needs a small `Host`
    design, not a product decision.
12. Adopt `take_requested_panel` on `App` (currently unoverridden, stays
    `None`) — this **adds** ~20 lines to GTK rather than removing TUI lines,
    closing a real cross-backend behavior gap (item 4's divergence-bug-class
    below), not a line-count win.

**Wave 3 — needs a product decision before converging:**
13. ✅ **Decided (#1065): yes** — every other GUI app scrolls a scrollable
    panel under the wheel. Turned out **already wired for Git and Settings**:
    GTK's generic `try_route_sidebar_mouse_event` has routed `UiEvent::Scroll`
    to `handle_sc_sidebar_ui_event`/the Settings `FormController` since
    #544/#754, well before this audit — the "GTK missing PANEL_GIT/SETTINGS
    wheel scroll" line above was simply wrong, never empirically checked.
    **Search was genuinely broken**: `paint_sidebar_panel_rung`'s
    `PANEL_SEARCH` arm never called `search_sidebar_system.set_backend_info`
    — the exact #971 gap (`SidebarSystem::handle_cached` returns `Ignored`
    unconditionally until it's called once) already fixed for the git and
    ext-sidebar systems but missed for search, so every wheel notch (and
    every content-row click) over the results tree silently no-op'd. One-line
    fix, RED-verified against unfixed `develop`, driver tests added for git
    and search (`src/gtk/testing.rs::sidebar_panel_clicks`; settings already
    had one). **Ext-panel (plugin `ext_panel_active` panels) is out of scope
    here, genuinely blocked**: GTK's `id if id.starts_with("ext:")` paint arm
    and `try_route_sidebar_mouse_event`'s `ExtPanel` click arm both operate on
    `ext_sidebar_system` (the built-in Extensions *marketplace* tree) instead
    of the plugin's own `build_ext_panel_data`/`ext_panel_scroll_top` state —
    a pre-existing content-routing bug independent of scroll (already flagged
    by `switching_to_a_plugin_panel_clears_stale_marketplace_focus`'s doc
    comment as "not this issue's to fix"). Needs its own vimcode issue before
    ext-panel wheel scroll can be tested/fixed for real; once that lands,
    scroll comes for free through the same rung Git/Search/Settings use.
14. ✅ **Decided (#1066): yes — converged.** Scroll-follows-pointer is the norm
    in GUI editors, but the decisive weight was terminal-native precedent: real
    Vim's own mouse behaviour already scrolls the window *under the pointer*
    on a `:split`, independent of focus — the exact thing a "vim-like" editor
    should match, not GTK parity for its own sake. TUI's editor-viewport wheel
    fallback in `mouse.rs` (the `#825` comment had flagged it as the one place
    this diverged) was rewired onto the same shared primitives GTK's
    `handle_mouse_scroll_msg` already uses — `render::find_window_at` +
    `Engine::scroll_viewport_with_cursor_for_window` — no new per-backend
    logic. Surfaced (and fixed) a latent `find_window_at` call-site bug along
    the way: an odd number of available rows splits a horizontal `:split`
    unevenly (e.g. 37 -> two 18.5-row panes), so a pane boundary can land on a
    half-row; querying the integer row itself (rather than its cell *center*,
    `+ 0.5`) lands just outside the pane that visually owns that row. Driver
    test added (`wheel_scrolls_the_hovered_pane_not_the_focused_one_via_shell_
    app`, `tui_main/shell_app.rs`), RED-verified against unfixed `develop`.
15. ✅ **Decided (#1067): GTK should get it — converged, not TUI-only.**
    TUI's panel-hover-popup (source-control / extension-panel item dwell
    tooltip) link click was a hand-rolled inline hit test in `mouse.rs`; GTK
    painted and cached the identical `panel_hover_link_rects` (`render::
    panel_hover_popup_paint`) but never read them back on click at all — a
    complete no-op, not an intentional design choice. `panel_hover_popup_
    paint`'s own doc traces the gap to the #540 Relm4->ShellApp migration
    retiring `Msg::PanelHoverClick` without a replacement — GTK *used to*
    have this. There is no "terminal has no rich clipboard" case for
    *painting a hit region and then ignoring the click on it*; that argument
    only supports TUI's *action* differing (copy vs. open), which is exactly
    the precedent the already-shipped editor-hover-popup rung
    (`render::route_editor_hover_popup_click` /
    `EditorHoverPopupEffect::open_url`, "opens (GTK) or copies (TUI) — the
    one genuinely per-backend step") already established for the sibling
    popup. #1067 extracted the identical shape —
    `render::route_panel_hover_popup_click` /
    `render::apply_panel_hover_popup_route` — wired both backends onto it
    (`App::route_and_apply_panel_hover_popup` on GTK,
    `mouse::handle_mouse` on TUI), and along the way aligned TUI's
    previously-dropped `is_native` flag with GTK's so the two caches share
    one element shape. Driver tests on both backends, RED-verified against
    unfixed `develop`.

**Wave 4 — the epic, last, gated on P0:**
16. Retire `events::uievent_to_crossterm` + `mouse.rs`'s remaining
    non-shared entry-point plumbing by routing through `src/click.rs` + a new
    shared dispatch layer, per #950's own stage-2 ordering — now scoped down
    to Git/Search/Settings panel intercepts (Debug/Explorer/ExtPanel are
    already ported ahead of `mouse.rs`, confirmed this pass) plus the general
    `MouseDown`/`Up`/`Move` top-level routing. Ends when `uievent_to_crossterm`
    has no TUI callers left and can be deleted from `src/tui_main/events.rs`.

**Not #1044's scope, filed for reference:** the `CommandLine::selection`
quadraui gap (item 3 of the trait-method table) is drafted in
`docs/PENDING_QUADRAUI_ISSUES.md`, coordinator/human action to file per that
doc's standing note.

### Target end state — numerically

**`src/tui_main/` cannot shrink to `src/macos/mod.rs`'s size (138 lines)** —
unlike macOS, TUI's `shell_app.rs`/`mouse.rs`/`render_impl.rs`/`panels.rs` carry
real, irreducible responsibility no other backend has: the entire
ratatui/ANSI paint substrate (the TUI-side equivalent of GTK's Cairo calls,
themselves already routed through `render.rs`), the raw-mode-safe panic hook,
per-frame cell-grid viewport recomputation, keyboard-enhancement probing, PTY
resize handling, and genuine TUI-only features (explorer drag-and-drop,
minimap braille rendering). Converging waves 1–4 above nets roughly **−400 to
−900 production lines**, mostly out of `mouse.rs` and `shell_app.rs` — landing
`src/tui_main/`'s total near **9,600–10,000**, not lower. This matches
`IRREDUCIBLE_SURFACE.md` §4's existing estimate ("~2,000 ± 500 duplicated code
lines, nets −300 to −1,000") — this audit itemizes that estimate into 16 named,
mostly-small issues instead of leaving it as an aggregate guess.

**Which files survive, and in what shape:** `shell_app.rs` (the `ShellApp`
impl + the irreducible `setup`/`tick`/window-absence `handle` arms — thinner,
not gone), `mouse.rs` (the irreducible cell-hit-test math + apply bodies for
geometry that genuinely differs per backend — thinner once waves 1–2 land),
`backend.rs`/`panels.rs`/`render_impl.rs`/`quadraui_tui.rs`/`services.rs`
(**not audited by #1044** — the issue scoped this pass to `ShellApp` +
`mouse.rs` only; a future round should cover these if the goal wants full
`src/tui_main/` coverage), `events.rs` (shrinks as wave 4 removes
`uievent_to_crossterm` callers — plausibly deletable entirely once wave 4
finishes, if no `UiEvent`-shape conversion is still needed for the Git/Search/
Settings migration). No file is targeted for outright deletion by this
audit — that is explicitly out of scope ("Deleting `TuiShellApp` is the last
child issue, not this one").

## ⚠️ Milestone #7 is NOT drained — 2 open (2026-09-11)

The 2026-09-11 macOS audit found the failure mode this file exists to catch, in
its purest form: **quadraui shipped an API and vimcode never adopted it.**

| Issue | Adoption gap |
|---|---|
| **#901** | `Backend::install_menu_bar` — the native macOS build paints its menu bar *inside the window*. quadraui's NSMenu installer, `MenuBarItem.submenu` and `BackendCaps::native_menu` are all shipped in the pinned rev; vimcode references none of them. |
| **#902** | `Backend::show_context_menu` — same capability flag, same suppression gap, for right-click menus. Scoped to mirror VS Code's `window.menuStyle` setting. |

quadraui#184 is still open, and that is precisely why this went unnoticed: **its
supply side had already landed**, so the tracking issue staying open made it look
like the consume side could not start yet. *An open quadraui issue is not evidence
that its API is unavailable — check the pinned rev, not the issue state.*

**The rule the audit produced — keep it.** vimcode's platform-service adoption is
otherwise healthy, because *additive* services are called unconditionally on
`backend` in `src/app.rs` and the trait's default no-op absorbs backends that lack
them (`show_message_dialog`, `show_file_open_dialog`, `set_cursor`,
`toggle_window_maximize`, `begin_window_drag`). That pattern needs no capability
check and should stay.

It breaks only for **substitutive** services — where the native call *replaces*
something vimcode paints itself. There the no-op default is not enough: vimcode
must suppress its own rasteriser, which requires `backend.capabilities()`. As of
2026-09-11 vimcode queries `BackendCaps` **nowhere**, and quadraui's surface has
exactly two substitutive services — both unadopted, both now filed.

**So audit by asking "which quadraui calls would *replace* something we draw?"** —
not "which methods are unreferenced?" Most unreferenced methods are correctly
unreferenced. Deliberately *not* filed: `send_notification` (macOS/Windows
advertise `notifications: true`), because VS Code uses in-window toasts on every
platform and never OS notifications — vimcode's `draw_toast_stack` already
matches. Rationale recorded in #902 so a future audit does not re-flag it.

## ✅ The 2026-09-01 audit's issues have all landed

Everything that audit filed has landed. `#730` (`ai_panel` paint),
`#593` (GTK `Ctrl+V`), `#731` (orphan Relm4 handles), `#732` (the GTK `Msg` bus),
`#733`/`#734`/`#735` (mouse, keyboard, frame composition — each split into the
slice chains below), `#657` (the oracle loop), `#658` (preview tier), plus `#480`,
`#550` and `#551`. `#146` moved out to **#4 Editor Features**, as this file
recommended — it is an addition, not a deletion.

| Convergence | Slices that actually did the work |
|---|---|
| Mouse routing (#733) | #751 → #756 |
| Keyboard dispatch (#734) | #757 → #762 |
| Frame composition (#735) | #763 → #766 |

Two structural landmarks fell with them:

- **`#657` shipped `[lib] vimcode_core`** (`eb745e2`) — `render`, `tui_main` and
  `gtk` are promoted out of the binaries, and `tests/acceptance/` is sealed. The
  oracle loop is available to this repo for the first time.
- **`#766` deleted `draw_frame`** (`eedebf8`) — the last raw-`ratatui::Frame`
  path. Both backends now compose one `FrameOp` sequence and walk it.

Also closed earlier in the arc and still true: `fn event_loop` does not exist in
`src/`; `src/gtk/draw.rs` is deleted; both `ShellApp` migrations (#448, #595) are
closed.

**`#815` closed the folder-picker gap §1b/§4 below found.** The TUI-local
`FolderPickerState` is deleted; both backends now share
`quadraui::FolderPickerController` through `FrameOp::FolderPicker` — GTK's
`open_folder_dialog` no longer opens a native `gtk4::FileDialog`. See
`docs/IRREDUCIBLE_SURFACE.md` §1b.

## 📏 The post-#735 audit — run, and it missed its projection

The previous revision of this file said: *"re-run the sizing audit when #735 lands
rather than assuming the chain finishes the job."* Done, on `develop @ eedebf8`.

Production lines, `#[cfg(test)]` excluded, all columns measured with the same
script (`scripts/prod_lines.py`) so they are comparable to each other:

| | 2026-05-01 | 2026-07-01 | 08-31 `f867817` | **pre-chain** `6875315` | pre-#785 09-03 | **post-#785 @ `ee26268`** |
|---|---|---|---|---|---|---|
| `src/gtk/` | 18,969 | 13,675 | 12,526 | 9,765 | 9,650 | **2,607** |
| `src/tui_main/` | 14,649 | 10,358 | 11,125 | 10,958 | 10,345 | **10,366** |
| `src/app.rs` (hoisted out of `src/gtk/` by #785) | — | — | — | — | — | **7,131** |
| **all three files** | 33,618 | 24,033 | 23,651 | 20,723 | 19,995 (2 files) | **20,104** |
| `src/render.rs` (shared) | 10,574 | 12,807 | 15,009 | 15,558 | 21,405 | **21,405** |

The **pre-chain** column is `6875315`, the last #732 commit — the true point before
#733/#734/#735 and slices #751–#766 began. Everything between the 08-31 and
pre-chain columns is **#722–#732**, which was dead-code deletion, not convergence.
Collapsing those two columns into one is what produced the −3,656 misattribution
corrected below (#827, refined by #792).

(All 2026-09 columns confirmed by re-running `prod_lines.py` against a
`git archive` of the revision named in the header — the pre-#785 column against
`ee26268`, and the `6875315` and `eedebf8` columns regenerated 2026-09-12 when
#792's correction was folded in: 9,765 / 10,958 / 15,558 and 9,650 / 10,345 /
21,405 respectively. None of these is a stale guess re-typed from prose; that
failure mode is why the script exists.)

**Projected vs. actual.** Measure the chain against *its own* range
(`6875315` → `eedebf8`), not 08-31 → 09-03 — the latter silently includes
#722–#732's dead-code deletion and is what made the chain look 5× better than it
was:

| | projected | actual over the chain | (08-31 → 09-03, for reference) |
|---|---|---|---|
| Backends | −8,700 … −9,500, landing near 14,000–15,000 | **−728, landing at 19,995** | −3,656 |
| `render.rs` | +4,000 … +5,000 | **+5,847** | +6,396 |
| Net across the three | ≈ −4,000 | **+5,119** | +2,740 |

**The chain missed its projection by roughly 12×, not 2.4×**, and the net across the
three files went the *wrong way* by over 5,000 lines. Where the 08-31 → 09-03
reduction actually came from:

| File | pre-chain | now | Δ |
|---|---|---|---|
| `src/gtk/mod.rs` | 10,518 | 7,684 | **−2,834** |
| `src/tui_main/panels.rs` | 1,554 | 1,208 | −346 |
| `src/tui_main/mouse.rs` | 3,211 | 2,895 | −316 |
| `src/tui_main/shell_app.rs` | 4,109 | 3,989 | −120 |
| everything else | | | ≈ −40 net |

`gtk/mod.rs` alone is 78% of the cut. Notably `tui_main/mouse.rs` — the file #733
was sized against at −3,000…−3,500 — lost **316 lines**.

**Attribution correction (#827, made exact by #792): the −3,656 is mostly
dead-code removal, not convergence.** The decomposition reconciles exactly against
the table above — **−2,928** of it is #722–#732 deleting code outright (#731 alone
`−1,432/+245`; the #732 tranches `−1,837/+83`, `−535/+461`, `−529/+485` in
`gtk/mod.rs`), leaving **−728** for convergence proper. That is almost all of the
−2,834 booked against `src/gtk/mod.rs` above. −2,928 + −728 = −3,656; an earlier
revision of this section put the split at "roughly −2,825" and "closer to 900",
which does not sum and understates the error. Deleting unreachable code and
converging duplicated code are different activities and must not be pooled.

**The mechanism, visible in the diff.** Moving a *decision* into `render.rs` leaves
every *apply* body in place at its original size, now preceded by a
`MouseDragState`/`ModalOverlayState` literal (30–60 lines per call site) and a
"#NNN moved this" comment. The `FrameOp`/`EditorOp`/`BottomOp` machinery added three
enums, three order constants, three composers, three validators and ~150 lines of
doc. A 12-variant `match` is not shorter than 12 `if` blocks. This is why the chain
moved logic without shrinking anything.

> **#785 (stage 1 of #47) moved the mass, it did not delete it.** `struct App`,
> its `impl` blocks and `impl quadraui::ShellApp for App` were hoisted verbatim
> out of `src/gtk/mod.rs` into a new top-level `src/app.rs`. `src/gtk/`
> reads **2,607** against the pre-#785 9,650 column above and `src/app.rs`
> reads **7,131** — both regenerated at `ee26268` in the table above — but the
> *total* is essentially unchanged (module doc and re-stated imports account
> for the difference). Nothing here got smaller; a ~6,900-line block that was
> filed under "GTK backend" is now filed under "shell application", where a
> second native backend can reach it. `src/app.rs` is still `gui`-gated: its
> module doc enumerates the four platform-typed fields, ~11 platform hook call
> sites and the `crate::gtk::{click, css, util}` dependency that have to go
> before the gate can.
>
> **#862 (2026-09-10) dropped that gate.** `pub mod app;` in `src/lib.rs` no
> longer carries `#[cfg(feature = "gui")]` — the three remaining
> platform-typed fields are type-erased behind small local traits / a
> `Box<dyn Any>` drop-guard, and the portable majority of
> `crate::gtk::{click, css, util}` moved to the new backend-neutral
> `src/click.rs`/`src/app_support.rs`/`src/css.rs` (`src/gtk/{click,mod,css}.rs`
> re-export so nothing else in `crate::gtk` had to change). This does **not**
> shrink `src/app.rs` or `src/gtk/` in the table above — it is a
> compile-boundary fix, not a line-count fix; re-run the script if you need a
> fresh column. Re-measure before trusting either bullet's line counts.

> **Correcting the record.** The figure this file previously carried as
> "`src/gtk/` = 12,588 at 2026-09-01" was measured *before* #727/#728/#730 landed;
> it matches the pre-chain 08-31 column above, not the 09-01 tree. The 05-01 and
> 07-01 figures differ from the previously recorded ones by 10–290 lines for the
> same reason — inconsistent measurement points. **Regenerate with the script, do
> not trust a number typed into prose.**

### What the chain *did* buy

Every *decision* — which surface was hit, which handler owns a key, what order a
frame is composed in — is now stated once, in `render.rs`, and both backends walk
it. Delegation density is high: `src/gtk/mod.rs` makes 424 `render::` calls. That
is a real and durable correctness win, and it is the reason the net line count went
up: the shared op-sequence machinery (`FrameOp`/`compose_frame`, the routers) costs
more lines than the duplicate pair it replaced.

**What it did not buy is the north star's stated end state.** 19,995 lines across
two backends is not "thin event-to-engine wiring", and nobody should plan as though
the remaining gap is small.

## 🔭 What actually remains

### 1. The irreducible surface — ✅ aggregated, and it is small

Done: **[`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md)** (2026-09-03,
corrected 2026-09-05 per #827 — the folder-picker verdict below was wrong).
The headline, because it changes how the rest of this goal should be planned:

- The **nine** recorded verdicts reduce to **three distinct facts**, of which **two are
  genuinely irreducible** — px-vs-cell frame metrics, and GTK's menu bar *being* its CSD
  titlebar (#552). A fourth candidate, the TUI-only folder picker, was recorded as
  irreducible on the theory that GTK's native `GtkFileChooser` has no shared
  counterpart — wrong: `quadraui::compose::FolderPickerController` has existed since
  2026-05-25 and its module doc explicitly tells vimcode to delete the local copy and
  rewire both backends through it. That verdict is struck, not counted.
- **Only 1.3% of the two backends names a native toolkit type** — 246 production lines
  out of 19,429 (`scripts/native_lines.py`; ~25% undercount on the GTK side for stored
  widget handles, so call it under 4% even pessimistically).
- **So platform-specificity is not what is keeping 19,995 lines in the backends.**
  `src/gtk/mod.rs` (7,684) and `src/tui_main/shell_app.rs` (3,989) are two
  implementations of the same four `ShellApp` entry points. The chain converged the
  *decisions* those implementations make; it did not converge the implementations.

**Plan accordingly: this is ordinary duplication, not a platform-porting problem.** A
plan that sizes it as the latter will keep missing its projection the way #751–#766 did.

**The third verdict was mislabelled** — `tui_main/mouse.rs:1620` (command-line text
selection) reads as a decision but its own text says the fix is a
`CommandLineLayout::hit_test` in quadraui. That is a *blocked* convergence whose blocker
was never filed; `CommandLineLayout` does not exist in quadraui and **#194** is the
open consumer-side symptom. Second instance in a week of the #47 failure mode — see the
milestone-discipline rule at the bottom of this file, which applies to in-code comments
too: **a comment naming a missing upstream API is an unfiled issue, and grep will not
find it for you.**

### 2. "The duplication moved down a level" is largely refuted (#827)

The previous revision of this file claimed quadraui#481/#482 held a large, unqueued
mass of cross-backend duplication one level down. Re-checked against quadraui's own
pinned rev (`42e0f8f`) on 2026-09-05: most of the individual claims do not hold up.

| Claim (previous revision) | Reality at pin `42e0f8f` |
|---|---|
| `EventOutcome` declared twice verbatim | Declared **once** — `quadraui/src/runtime.rs:95` (quadraui#496, closed 09-02) |
| `shell_runner` 45 identical lines ×2 | Four runners of 4–7 non-comment lines each, all delegating to shared `shell_adapter.rs::build_shell_adapter` |
| 1,671 byte-identical lines gtk↔macos | Function-level duplication is ~85 lines. quadraui#481's own correction comment (09-03 17:45Z) withdrew the headline number as "idiom coincidence" |
| UTF-8 fix: 7 private copies, 0 public | **Public since 2026-08-15** — `text_util.rs:51-107`, re-exported from `lib.rs` (quadraui#503) |
| `gtk_tree_layout`/`mac_tree_layout` twins | Both 1-line wrappers over `primitives/layout_metrics.rs:60 tree_layout` (quadraui#499, 09-02) |
| "no `desktop/`" | `quadraui/src/desktop.rs` exists (754 lines) since 09-02 (quadraui#498) — the original claim grepped for a directory that had been renamed |
| #482 "holds the mass" | **All eight children #503–#510 are closed.** #482 has zero comments and is a hollow epic |

**Still true:** macOS dispatches `WindowResized` undebounced (`macos/run.rs:544-561`)
while TUI/GTK use the shared `ResizeDebouncer`. That is a real, small, still-open gap —
just not the "65% duplicated" epic the previous revision described. quadraui#481/#482
remain open and un-milestoned, but do not plan against their headline numbers; re-audit
the specific claim you need before acting on it.

### 3. #47's blocker was filed and cleared — this section was stale for two days (#827)

**Corrected 2026-09-05.** The previous revision said *"No open quadraui issue
mentions `modal_stack_handle` or `drag_state_handle`"* and called filing one "the
single most actionable item on this page." That stopped being true within hours of
being written:

- **quadraui#699** was filed 2026-09-03 **16:38Z**, into quadraui milestone **#9**,
  and **closed 17:11Z** (PR#700 / `88345fb`). A follow-up, **#704**, closed 21:41Z.
- **vimcode#47 was reopened 16:38Z** and is **OPEN**, in milestone **#5**, right now
  — it was never re-closed.
- The commit that last touched this file (`5e2c7cc`) landed **18:32Z — 81 minutes
  after #699 had already closed** — and still said the blocker was unfiled. The
  finding sat in `PLAN.md` for less than a day before it was acted on; the doc that
  said otherwise just never got re-read against events.
- quadraui milestone #9 was never closed — it's **open** (0 open / 7 closed issues
  in it). The previous instruction to "re-open" it was acting on a wrong premise.

**What actually happened, and what's still open:** quadraui#699/#704 gave every
backend a symmetric Rc-handle API (`modal_stack_handle()` / `drag_state_handle()`),
removing the `Backend`-trait asymmetry that blocked Stage 1. vimcode has already
started consuming it — **#811** (this branch's own history) bumped the quadraui pin
to `4ff2a64` and ported the four TUI-side call sites off the now-removed
`drag_and_modal_mut`. **vimcode#47 itself is still open** (Stage 1 — moving `App`'s
remaining GTK-specific call sites onto the new API, see `PLAN.md`) and is the actual
next actionable item here, not a re-filing task.

**The "44 call sites" figure was also wrong**, independent of the above. It came
from `grep -n 'self\.backend\.' src/gtk/mod.rs` — every use of the `backend` field,
not just the two Rc-handle methods. The real count, measured at `ee26268`
(`grep -n 'modal_stack_handle\|drag_state_handle' src/app.rs`, minus the two
doc-comment mentions of the method names): **19** — `modal_stack_handle` ×12,
`drag_state_handle` ×7. (Also note the field moved: by `ee26268` this code lives in
`src/app.rs`, not `src/gtk/mod.rs` — #785 had already hoisted it. Fixed in `PLAN.md`
and `PROJECT_STATE.md` too, which is where the 44 figure originates.)

### 4. The divergence bug class is not dead

This file's own thesis is that each open `GTK:`/`TUI:` bug is "a symptom of a
duplicated surface". Roughly 44 are still open — #206 (tooltip borders differ),
#420 (completion popup overflow, different failure per backend), #264 (settings
panel at narrow widths), #194 (status-bar selection: GTK can't, TUI has an offset
bug), #233 (dialog border glyphs). Plus milestone #5's cross-backend residue:
#149, #167, #168, #233, #294.

If the convergence had reached far enough, this list would be shrinking. Track
whether it does — it is the only outcome measure this goal has that isn't a line
count.

## Architecture milestones

| Issue | What | Status |
|---|---|---|
| **quadraui#465** | macOS `ShellApp` + `run_with_shell` composition | ✅ Closed 2026-08-31. The supply-side gate is cleared. |
| **#657** | Put vimcode on the oracle loop | ✅ Closed. `[lib] vimcode_core` + sealed `tests/acceptance/`. |
| **#47** | Native macOS GUI, as a thin wrapper | 🔓 **Reopened 2026-09-03, OPEN in milestone #5.** Blocker resolved (quadraui#699/#704); #811 already ported the TUI side. Stage 1 (GTK side) is the actual next work — see `PLAN.md`. |
| **quadraui#481 / #482** | Duplication one level down — largely refuted, see §2 above | 🔓 Open, un-milestoned. Don't plan against their headline numbers. |
| **#1044** | TuiShellApp/mouse.rs rung-decomposition audit | 🔓 **Open, milestone #7.** Audit complete (98 rungs, this file's 2026-09-16 section); 16 sequenced child issues drafted, not yet filed — coordinator/human action. Closes once filed. |

### The two decisions this file was holding open — both now moot

**1. The #657 policy freeze.** #657's body declared that no vimcode bug-fix
dispatch happens until vimcode is on the oracle loop. It was never honoured, and
#657 has now landed anyway. The question is retired by events; delete the paragraph
from the issue if it is ever re-opened.

**2. Accepting worker-authored verification for the chain.** The trade was taken:
every fix ahead of #657 was verified by tests its own author wrote. It is now
*unwindable* rather than hypothetical — the sealed suite exists, so the honest
follow-up is to decide whether any of #751–#766 warrants a retro-fitted
oracle-authored test, rather than re-litigating the sequencing.

## Status (2026-09-16, #1044 rung-decomposition audit)

- 🔬 **Milestone #7 is 3 open** (#901, #902, #1044). #1044 enumerated all 98
  rungs of `TuiShellApp`/`mouse.rs` against `App`: 40 already-shared, 26
  convergeable (sequenced into 16 child issues, see the 2026-09-16 section
  above), 31 irreducible (all instances of 3 facts, one of them new —
  `docs/IRREDUCIBLE_SURFACE.md` §1/§7), 1 quadraui-gap (drafted in
  `docs/PENDING_QUADRAUI_ISSUES.md`). **No production code changed** — this was
  the audit, not the convergence. Target: `src/tui_main/` lands near
  9,600–10,000 production lines once the 16 child issues land, not lower —
  see that section's "Target end state" for why it can't reach `src/macos/`'s
  138.
- ⚠️ **#901/#902 still open** (the 09-01 critical path plus 16 slices all
  landed, but the 2026-09-11 audit reopened the milestone: quadraui shipped
  `install_menu_bar` / `show_context_menu` and vimcode adopted neither).
  See the milestone section above for the additive-vs-substitutive rule.
- ✅ **The oracle loop is live here** (#657) and `draw_frame` is gone (#766).
- 📉 **The audit is run and the chain missed by ~12×.** Measured over its own range
  (`6875315`→`eedebf8`), convergence took **−728** off the backends against a
  −8,700…−9,500 projection while `render.rs` grew **+5,847** — net **+5,119**, the
  wrong direction. The −3,656 this section used to headline pooled in **−2,928** of
  #722–#732 dead-code deletion; the two sum exactly (§ above).
- 🔎 **And most of what is left is not code.** A function-level audit found **39% of
  the two backends' 11,673 production lines are comments** — 6,958 are code, of which
  roughly **2,000 ± 500** are genuinely the same logic written twice. Converging all
  of it nets **−300 to −1,000** across the three files. **Do not plan a convergence
  campaign here; the returns are not there.**
- 🔓 **#47 is open again, in milestone #5.** Its blocker (quadraui#699/#704) closed
  2026-09-03; #811 already ported the TUI side onto the new API. Stage 1 (GTK side)
  is the actual next actionable item — not a re-filing task.
- 🔓 **quadraui#481 / #482 remain open and un-milestoned**, but most of their
  headline duplication claims were refuted 2026-09-05 (see §2) — the one confirmed
  live gap is macOS's undebounced `WindowResized`.
- ✅ **The irreducible surface is aggregated** — [`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md),
  corrected 2026-09-05 (the folder-picker verdict was wrong; struck). **Two** of the
  three remaining facts are genuinely irreducible; only **1.3%** of the backends
  names a native toolkit type — the rest is duplication, and the goal should be
  planned as such.

## How to use this doc

- **Line numbers:** this file cites none, on purpose — locate by symbol
  (`grep -n "impl quadraui::ShellApp for App" src/app.rs`). Counts here are
  evidence measured on a named revision; **regenerate them with
  `python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs src/app.rs`**
  rather than trusting the table.
- **Agents:** treat this as the standing objective behind all planning and triage.
  Item 3's blocker is cleared — the next move is **vimcode#47 Stage 1** (see
  `PLAN.md`), then items 1, 2 and 4 above. Never write new per-backend code
  (`CLAUDE.md` Platform-Neutrality Rule). When you adopt a quadraui API,
  **delete** the old backend code in the same PR.
- **Humans:** edit freely as priorities shift; keep it short, re-date Status.
- **Milestone discipline:** new "delete vimcode's bespoke X for shared Y" work →
  **#7 Platform-Neutral**. A quadraui gap that *blocks* a #7 issue → file it on
  `JDonaghy/quadraui` and re-open **quadraui milestone #9 "vimcode Platform-Neutral
  blockers"**. **A #7 issue that turns out to be supply-blocked must be left open
  behind that blocker, not closed** — #47 is the cautionary example.
