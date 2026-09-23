# TUI decomposition audit, round 2 — `panels.rs` + `render_impl.rs` (#1108)

> **This document introduces no production code.** Same rule as
> `docs/SHELLAPP_CONVERGENCE.md` (#950), `docs/BACKEND_SETUP_AUDIT.md` (#260) and
> `docs/SHELLAPP_AUDIT_R3.md` (#1192): it is the audit, not the convergence.
> `scripts/prod_lines.py src/gtk src/tui_main src/render.rs` delta for this PR is
> **0** — this is the "some issues legitimately will not shrink it" case
> `GOALS.md`'s #1044 section asks every #1169 PR to declare.

## 0. Scope

#1044 (2026-09-16) decomposed `TuiShellApp` + `mouse.rs` into 98 rungs and
explicitly declined to audit `backend.rs`, `panels.rs`, `render_impl.rs`,
`quadraui_tui.rs` and `services.rs` — see `GOALS.md`'s "Which files survive"
paragraph. This is that follow-up pass, re-scoped per the issue body against
measured sizes (`scripts/prod_lines.py`, this worktree @ `94204ac`,
2026-09-20 — 13 lines above the issue's own `a1c11c9` snapshot from churn in
the interim, immaterial to the verdicts below):

| File | Prod lines | Worth a pass? |
|---|---:|---|
| `panels.rs` | **1,122** | yes — audited in full below (§2) |
| `render_impl.rs` | **1,378** | yes — audited in full below (§3) |
| `quadraui_tui.rs` | 38 | no — thin `q_theme`/`draw_activity_bar` wrappers, nothing to decompose |
| `backend.rs` | 7 | no — a bare `TuiBackend::new()` re-export, nothing to decompose |
| `services.rs` | 3 | no — a single `pub(super) use` line, nothing to decompose |

`quadraui_tui.rs`/`backend.rs`/`services.rs` total **48** production lines
between them — a rung-by-rung pass on that would be exactly the
motion-without-progress this epic exists to catch. Disposed of in this one
paragraph, per the issue's own instruction.

## 1. Method

Same as #1044: read every production function/struct in each file (not
`#[cfg(test)]`-gated — those don't count toward `prod_lines.py` and are not
"rungs" in this audit's sense), verdict it against the corresponding GTK code
in `src/app.rs` where one exists, and cite the concrete evidence (doc comment,
diff, or a direct read of both sides) rather than assume from a function
name alone. Four verdicts, per #1044:

- **already-shared** — the decision (and usually the geometry) already routes
  through one `render::`/`core::` function both backends call; the TUI-local
  code left is thin per-backend wiring (unit conversion, `&mut dyn Backend`
  plumbing), the same shape #1044 counted 40 GTK-side rungs this way.
- **convergeable** — a genuine second implementation of logic that already has
  a shared home; no upstream blocker, just needs the TUI call site rewired.
- **irreducible** — a real, documented architectural fact (cell grid vs. pixel
  canvas, or a `ratatui`-only API) that has no shared answer to converge onto.
- **quadraui-gap** — convergeable only behind new quadraui infrastructure that
  does not exist yet. Per the issue's instruction: described, not designed,
  and not campaigned into a vimcode PR ahead of the upstream primitive
  (#1169's own ruling on exactly this file — see §2.9).

## 2. `panels.rs` — 1,122 production lines, 14 rungs

`panels.rs` is TUI's rasteriser for the sidebar body (7 built-in panels + the
plugin `ext:` panel), the panel-hover popup, and the `:`-command line. Its GTK
twin is `App::paint_sidebar_panel_rung` (`src/app.rs:3352`), read in full for
this audit.

| # | Rung | Lines | Verdict | One-line justification |
|---|---|---:|---|---|
| 1 | `render_activity_bar` | — | *(not a production rung)* | `#[cfg(test)]`-gated — dead in production since the activity bar moved to quadraui's `AppShell` (#536); excluded from `prod_lines.py` and from every count below. |
| 2 | `render_explorer_sidebar_content` | 51 | quadraui-gap | Same decision as `paint_sidebar_panel_rung`'s `PANEL_EXPLORER` arm (populate tree controller, set rect, `explorer_tree.render()`), but TUI additionally hand-fills the background via a `Backend::draw_status_bar` trick and pushes a `ScrollSurface` — composition-level TUI-only chrome with no shared primitive to fold into (see §2.9). |
| 3 | `render_sidebar_content` (dispatcher) | 31 | already-shared | Dispatches on `engine.app_shell.active_panel_id()`, the same `AppShell` state `paint_sidebar_panel_rung`'s `render::sidebar_owner(engine)` resolves — one `AppShell` cursor, two thin `match` shapes over it. |
| 4 | `fill_row_q` / `fill_row` / `fill_rect` | 40 | already-shared | A `Backend::draw_status_bar`-as-solid-fill trick — explicitly the same stand-in quadraui's own `AppShell::render` uses for its resize divider (own doc comment cites it); not a TUI-only invention, and GTK's Cairo canvas needs no fill-trait detour to begin with (native fill call), so there is nothing to converge on that side. |
| 5 | `render_settings_panel` | 54 | quadraui-gap | Same `render::populate_settings_form_controller` + `FormController::render_and_cache` GTK's `PANEL_SETTINGS` arm calls, but TUI additionally paints its own header/search chrome via `Backend::draw_settings_chrome` — GTK's arm calls neither `draw_settings_chrome` nor any header equivalent (see §2.9, and the open question in §5 item 3). |
| 6 | `render_search_panel` | 34 | already-shared | Doc comment: "already trait-pure... #607 widened the parameter... letting `render_content` call it via `render_sidebar_content` without a concrete backend" — same `populate_search_sidebar_system` + `SidebarSystem::render` GTK's `PANEL_SEARCH` arm calls; the only TUI-local code is two caret-clamp guards, unrelated to painting. |
| 7 | `render_command_line` | 15 | already-shared | Doc comment names the whole body as quadraui#1001's shared primitive: `render::command_line_view` + `render::command_line_selection_bytes` + `Backend::draw_command_line_selection` — the exact rung `GOALS.md`'s own table (item "`FrameOp::CommandLine` selection-highlight paint") already tracks as converged. |
| 8 | `render_source_control` | 181 | quadraui-gap | Same `render::sc_*` adapters (`sc_header_text`, `sc_commit_message_to_text_input`, `draw_sc_sidebar_panel`, `populate_sc_sidebar_system`, `sc_branch_picker_to_palette`, `sc_help_dialog_layout`) `paint_sidebar_panel_rung`'s `PANEL_GIT` arm calls — same decisions, same adapters, but the *band geometry* is derived twice: TUI from `sc_commit_input_box_height`/manual row arithmetic, GTK from `render::sc_sidebar_bands`. This is the largest single rung in the file and the biggest concrete instance of the D3-10 lead (see §2.9 and §4). |
| 9 | `render_ext_panel` | 190 | quadraui-gap | Same `render::ext_panel_to_tree_view` + `Backend::draw_tree` + `backend.tree_layout()` cache GTK's `id if id.starts_with("ext:")` arm uses; TUI additionally hand-rolls its own scrollbar (`draw_tree` "doesn't render scrollbars yet", own doc comment) and its own help-popup `TooltipLayout` construction — real per-panel composition GTK's arm doesn't need at all (that arm has neither). Second-largest rung; see §2.9. |
| 10 | `render_panel_hover_popup` | 134 | already-shared | Doc comment: "the same shared `render::ext_panel_hover_screen_row`/`ext_panel_chrome_rows` derivation `panel_hover_anchor_y` (GTK's twin of this function) now uses" — anchor math only, cell vs. pixel; the popup layout/paint itself is one call to the shared `RichTextPopup::layout()` + `Backend::draw_rich_text_popup`. |
| 11 | `render_ext_sidebar` | 84 | quadraui-gap | Same `render::ext_sidebar_to_multi_section_view` + `MultiSectionView::render` GTK's `PANEL_EXTENSIONS` arm calls, but TUI paints two additional header/search chrome rows by hand (`fill_row`, not even the shared `draw_settings_chrome`) that GTK's arm has no equivalent of at all — see the open question in §5 item 3. |
| 12 | `render_ai_sidebar` | 21 | already-shared | Doc comment: "delegates its entire paint to the shared `engine.ai_chat` (`quadraui::ChatController`), the same controller GTK's `render_content` `PANEL_AI` arm now also renders — one implementation instead of two" (#819). Confirmed: `paint_sidebar_panel_rung`'s `PANEL_AI` arm is the same three calls (`populate_ai_chat_controller`, set rect, `.render()`), modulo `Cell`-vs-field caching of backend metrics. |
| 13 | `render_debug_sidebar` | 56 | quadraui-gap | Same `render::debug_sidebar_chrome_to_status_bars` + four `quadraui::TreeView`s (`populate_dap_sidebar_system`) GTK's `PANEL_DEBUG` arm calls; TUI's body is a superset only in how it slices the chrome rows into `Rect`s (cell arithmetic) vs. GTK's `f32` pixel arithmetic for the same two bars — same composition shape as the rest of §2.9's bucket, smaller because the chrome here really is just two `StatusBar`s, no extra TUI-only chrome layer. |
| 14 | Doc-comment/module overhead (headers, the deleted-`draw_frame` note at line 1103, section dividers) | ~231 | irreducible | Prose, not logic — accounts for the gap between the sum of the rows above (891 = 616 quadraui-gap + 275 already-shared) and the measured 1,122; not a rung. |

### 2.9. The quadraui-gap bucket, named once

Six of the eight sidebar-panel-body rungs (2, 5, 8, 9, 11, 13 — **616
production lines**, ~55% of the file) share one shape: the *decision* and the
*data adapter* (`render::sc_*`, `render::ext_panel_to_tree_view`,
`render::populate_*`) are already fully shared with `paint_sidebar_panel_rung`;
what's independently written on each side is the panel's **chrome
composition** — which rows get a background fill, a header, a search box, a
scrollbar overlay, and in what order, before the shared widget body paints.
`paint_sidebar_panel_rung`'s arms are correspondingly smaller (its whole
match, all 8 panels, is **272 lines** — see `src/app.rs:3352-3623`) because
GTK either omits this chrome layer entirely (Settings, Extensions — see §5
item 3) or gets it from the pixel-space primitive directly where TUI has to
hand-roll a cell-space equivalent (Git's band geometry, ext-panel's
scrollbar).

**This confirms #1169's own ruling on this exact pairing, independently
re-derived**: converging `panels.rs`'s sidebar assembly into
`paint_sidebar_panel_rung` "moves mass without deleting it" absent a new
quadraui primitive. Per the issue's instruction, the primitive is described
here and not designed or campaigned:

**The gap:** quadraui has no *sidebar-panel-body composition* primitive — a
shared function/struct that takes "background fill colour, optional header
text, optional search-input state, a body widget (`TreeView`/`FormController`/
`SidebarSystem`/etc.), optional scrollbar" and a `unit_w`/`unit_h` pair (the
existing `FrameMetrics` convention), and produces the same ordered
`Backend::draw_*` call sequence for both a pixel canvas and a character grid.
Today each of the six panels above independently decides "clear background
first with `fill_rect`, then header row, then search row, then body" (TUI) or
"just call the body widget's `render()`, no chrome" (GTK) — two different
answers to the same layout question, not two implementations of one answer.
Until that primitive exists upstream, per `CLAUDE.md`'s Platform-Neutrality
Rule, **the fix belongs in quadraui, not in a vimcode PR that duplicates the
composition decision down into `render.rs` instead of up into a shared
`Backend`-trait-level widget.**

## 3. `render_impl.rs` — 1,378 production lines, ~13 rung groups

`render_impl.rs` is TUI's editor-band rasteriser: row/height accounting,
`ScreenLayout` construction for the two entry points (`build_screen_for_tui`,
test-only; `build_screen_for_shell_content`, live), editor popups, tab-bar
hit-testing/drag/tooltip, window painting, and window/group divider lines.

| # | Rung (group) | Lines | Verdict | One-line justification |
|---|---|---:|---|---|
| 1 | `BottomBandRowHeights` + `bottom_band_row_heights` | 71 | **convergeable — and it has a live bug** | Independently re-derives the exact five-band row accounting `render::compute_editor_layout` already computes generically (pixels for GTK via `line_height`, rows for TUI via `line_height = 1.0`) — but unlike `compute_editor_layout` (`per_window = effective_window_status_line(engine)`, `src/render.rs:22786`), this function reads the raw `engine.settings.window_status_line` field directly (`src/tui_main/render_impl.rs:80`), bypassing the `'laststatus'` narrowing #1206 added. See §5 item 1 — this is a live, unfixed TUI-only bug, not just duplicated code. |
| 2 | `build_screen_for_shell_content` | 64 | already-shared | Thin wiring around the fully-shared `render::build_screen_layout` (own doc comment: "mirrors `build_screen_for_tui`'s tail... so the two paths share the same formula"); the only TUI-local content is whole-cell window-rect origin math, the standing `FrameMetrics` unit fact. |
| 3 | `BottomChromeRects` + `bottom_chrome_rects_for_shell_content` | 51 | convergeable | Downstream of rung 1's bug/duplication — once `bottom_band_row_heights` calls the shared accounting, this function's `ratatui::Layout::split` over the same bands still needs no separate fix (the `Layout::split` call itself is a `ratatui`-only irreducible detail, but its *inputs* are the duplicate). |
| 4 | `paint_editor_popups` | 197 | already-shared | Doc comment (#1167): "the build-adapter → `.layout()` → `backend.draw_*` → cache-output part... now lives once in `render::paint_editor_popups`. This function's own job shrank to exactly what stays genuinely per-backend: finding the active window and resolving each popup's on-screen anchor point" — cell math, mirroring GTK's `paint_editor_popups_rung` (`app.rs:3673`) doing the pixel equivalent over the same shared function. |
| 5 | Tab-bar hit-test/drag/tooltip/drop-zone family (`tab_tooltip_at_col`, `tab_drag_slots_from_hit_regions`, `build_tui_tab_slots`, `render_tab_drag_overlay`, `render_tab_hover_tooltip`, `compute_tui_tab_drop_zone`) — 6 rungs | 251 | already-shared | Every one of the six delegates its actual decision to a `render::` function already shared with GTK's tab-drag code: `render::resolve_tab_bar_click`, `render::screen_to_drop_group_bounds`, `render::build_tab_drop_groups`, `render::compute_tab_drop_overlay`, `render::tab_hover_tooltip_paint`, `render::compute_tab_drop_zone`. TUI-local code is exclusively re-slicing the already-cached `TabBarLayout`/`ScreenLayout` into cell-space `(f32,f32)` pairs — own doc comments on `tab_drag_slots_from_hit_regions`/`build_tui_tab_slots` name this explicitly as "the single source of truth already used for mouse click routing," not a second geometry model. |
| 6 | Window paint family (`render_all_windows`, `render_window`, `render_window_status_line`, `char_col_to_visual`) — 4 rungs | 190 | already-shared | `render_window`'s own doc comment (Phase C Stage 1C, #276): "the actual paint code lives in `quadraui::tui::draw_editor`, fed by `render::to_q_editor`... This function handles only the bits the rasteriser deliberately excludes" (per-window status-line row carve-out, `Frame`-level cursor placement — the quadraui#504 migration). `char_col_to_visual` is a pure cell-space tab-expansion utility with no GTK counterpart to converge onto (GTK's `to_q_editor`/`draw_editor` do the equivalent expansion in pixels inside quadraui itself, not in vimcode code on either side). |
| 7 | Window/group divider + rule-row family (`window_overflows_vertically`, `window_right_edge_cell`, `vertical_separator_cells`, `draw_rule_row`/`draw_rule_cell_themed`/`draw_rule_row_themed`/`draw_rule_row_q`, `render_separators`, `group_divider_cells`, `render_group_dividers`) — 10 rungs | 410 | irreducible | **Already adjudicated in code, not re-litigated here**: `render::draw_dividers_as_splits`'s own doc comment (`src/render.rs:8283`) states the reason directly — "TUI paints its dividers cell by cell instead..., because it carries the #481 guard that suppresses a divider column immediately beside a neighbouring window's scrollbar — a coalescence problem that exists only in a character grid... what this slice shares is *which* dividers are painted and when." GTK consumes the shared `ScreenLayout::window_dividers`/`group_dividers` fields directly through that one shared painter; TUI cannot, because avoiding a doubled divider next to an overflow scrollbar is a per-cell adjacency problem with no pixel-space equivalent. |
| 8 | Doc-comment/module overhead (section headers, the #766 `draw_frame`-deletion note at line 367, the P0-style module-level comments) | ~144 | irreducible | Prose — accounts for the gap between the sum above (~1,234) and the measured 1,378; not a rung. `build_screen_for_tui` (lines 97-180, `#[cfg(test)]`) is excluded entirely, per `prod_lines.py`'s method. |

## 4. The ~2,000 ± 500 line estimate — confirmed, not refuted, and localised

`IRREDUCIBLE_SURFACE.md` §5 cites "a function-level audit puts the
genuinely-duplicated part of [the TUI/GTK residue] at only ~2,000 ± 500 code
lines" without naming which files carry it. This audit measures, rather than
asserts, the two files' share of that:

| Bucket | `panels.rs` | `render_impl.rs` | Total |
|---|---:|---:|---:|
| quadraui-gap (blocked on upstream primitive) | 616 | 0 | **616** |
| convergeable (no blocker, needs a PR) | 0 | 122 | **122** |
| already-shared (decision shared; thin per-backend wiring only) | 275 | 702 | 977 |
| irreducible (documented architectural fact) | 0 | 410 | 410 |
| doc-comment/module overhead (not logic) | ~231 | ~144 | ~375 |
| **Measured total** | **1,122** | **1,378** | **2,500** |

**Confirms the ~2,000 ± 500 figure — these two files alone account for a
material fraction of it, concentrated almost entirely in `panels.rs`'s
quadraui-gap bucket.** 616 + 122 = **738 lines** of these two files are
"genuinely duplicated implementation" by #1044's own definition (a second
hand-written answer to a question the other backend already has a shared
answer to, or would have one if a quadraui primitive existed) — roughly a
third of the citywide estimate, from 2 of the TUI's 5 largest files. The
remaining ~1,250 ± 500 lines of the estimate most plausibly sit in
`mouse.rs`/`shell_app.rs`'s own click-routing halves of these same eight
panels (out of this audit's scope; #1108's companion `mouse.rs` coverage was
already done by #1044 itself, verdicted mostly already-shared there — see
`GOALS.md`'s rung table, `mouse.rs` row 1) and in doc-comment/structural
overhead the same way `IRREDUCIBLE_SURFACE.md` §5 already flags ("39% of the
surrounding mass being comments" — this audit's own ~375-line overhead
bucket, 15% of the 2,500 measured here, is consistent with that being an
overall-corpus average pulled up by files denser in comments than these two).

**What this does not confirm:** that 2,000 lines are mechanically
convergeable today. 616 of the 738 "genuinely duplicated" lines are
**quadraui-gap**, not convergeable — per the issue's own instruction and
#1169's prior ruling, that bucket does not get a vimcode PR until the
upstream primitive in §2.9 exists.

## 5. Sequenced work order

Each item is sized as its own small, independently-mergeable issue, cheapest
and highest-divergence-risk first. None are filed yet — this worker session
has no `gh` access; coordinator/human: file each into milestone #7,
referencing #1108.

**Wave 1 — bug fix + investigation, zero design risk:**

1. **Fix `bottom_band_row_heights`'s stale `laststatus` read.** It computes
   `per_window_status`/`global_status`/`separated_status` from the raw
   `engine.settings.window_status_line` field (`render_impl.rs:80`) instead of
   `render::effective_window_status_line(engine)` — the accessor
   `compute_editor_layout` (GTK's equivalent accounting, `render.rs:22786`)
   and `build_screen_layout` itself (`render.rs:14498`) both already use. Since
   #1206 (`'laststatus'` support), setting `laststatus=0` or `laststatus=1`
   with multiple windows open likely reserves the wrong number of
   status/command rows on TUI while GTK reserves the correct amount — a
   live, user-visible row-count mismatch. Needs a driver test (TUI,
   `laststatus` 0 and 1 with 2+ windows, assert the rendered row count / cmd
   line's y-position) RED-verified against unfixed `develop` before landing
   the one-line fix.
2. **Investigate the Settings/Extensions header-chrome asymmetry** flagged in
   §2 rungs 5 and 11: `paint_sidebar_panel_rung`'s `PANEL_SETTINGS` and
   `PANEL_EXTENSIONS` arms call neither `Backend::draw_settings_chrome` nor
   any hand-rolled header/search row, while `panels.rs`'s
   `render_settings_panel`/`render_ext_sidebar` explicitly paint one. Confirm
   whether GTK's sidebar body gets this chrome from a different, generic
   place (a title bar `AppShell` composes above `sidebar_content_bounds`
   itself) or is genuinely missing it. Either outcome is a one-line
   correction to this document's §2 rows 5/11 or a real GTK bug fix — file
   whichever it turns out to be.

**Wave 2 — convergeable, no design decision needed:**

3. **Converge `BottomBandRowHeights`/`bottom_band_row_heights` onto
   `render::compute_editor_layout`.** Call
   `compute_editor_layout(engine, total_height, 1.0, menu_in_viewport)` and
   cast its `f64` `quickfix_h`/`terminal_h`/`debug_toolbar_h`/`wildmenu_h`/
   `status_bar_h`/`separated_status_h` fields to `u16` instead of
   re-deriving each independently. Fixes item 1 as a byproduct (the shared
   function already reads the narrowed accessor) rather than patching the
   symptom twice. Needs the two live call sites' differing
   `total_height`/`menu_in_viewport` conventions (documented on
   `build_screen_for_tui` vs. `build_screen_for_shell_content`) reconciled as
   part of the same PR — do this after item 1 lands as a safety net, or land
   both together.

**Wave 3 — quadraui-gap, upstream work required before any vimcode PR:**

4. **File the quadraui gap named in §2.9**: a shared sidebar-panel-body
   composition primitive (background fill, optional header/search chrome,
   body widget, optional scrollbar, parameterized by `unit_w`/`unit_h` per
   the existing `FrameMetrics` convention) that both `panels.rs`'s six
   quadraui-gap-verdicted renderers and `app.rs`'s `paint_sidebar_panel_rung`
   arms could delegate their entire body to — collapsing 616 TUI lines + a
   comparable share of GTK's 272-line match into one shared implementation
   plus two thin per-backend `Host` impls. **Do not implement the vimcode
   side of this before the primitive ships upstream** — per
   `CLAUDE.md`'s Platform-Neutrality Rule and #1169's prior ruling on this
   exact pairing.

## 6. What this audit does not claim

- It does not re-verdict anything `#1044`/`GOALS.md` already settled for
  `shell_app.rs`/`mouse.rs` — that is `#1192`'s territory, not this one's.
- Item 2 above is an open question, not a confirmed bug — stated as such,
  not asserted, per this file's own evidentiary standard.
- The quadraui primitive in §2.9/item 4 is described, not designed. A
  concrete API shape (trait method signature, struct fields) is upstream's
  call, consistent with `CLAUDE.md`'s instruction to describe the gap and
  stop.
