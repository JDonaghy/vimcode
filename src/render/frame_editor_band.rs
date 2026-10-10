use super::*;

// ─── Frame composition: the editor band (#764, #735 slice 3) ────────────────
//
// The band *below* the chrome band (next section) and the overlay band (below
// that): the editor column itself — windows, minimap strips, tab bars,
// breadcrumb bars, group dividers, the tab-drag ghost and the tab-hover
// tooltip.
//
// The divergence this removes, measured on `develop` before #764 — both
// backends walked the same run of rungs, in two different orders, and one of
// them was missing a rung outright:
//
//   GTK  windows(+`:split` dividers) → minimap → tab bars → breadcrumbs →
//        tooltip → …editor popups… → **tab-drag ghost**
//        (no between-group divider paint at all)
//   TUI  windows(+`:split` dividers) → minimap → tab bars → breadcrumbs →
//        **group dividers** → tab-drag ghost → tooltip
//
// Two real defects fell out of that, not just a cosmetic ordering difference:
//
//   * **group dividers were never painted on GTK.** `ScreenLayout
//     ::group_dividers` was populated every frame and hit-tested for drags
//     (`gtk/click.rs`'s `screen_zone_hit_test`), so a `Ctrl+W v` boundary was
//     draggable but invisible — input and paint disagreeing, the #587/#592
//     failure shape verbatim, and exactly what #735's own body predicted this
//     slice would find. GTK discarded the field into `_group_dividers` and
//     painted only `window_dividers` (the `:split`/`:vsplit` boundaries
//     *within* a group).
//   * **the tab-drag ghost sat on the wrong side of the editor popups on
//     GTK.** A completion menu / hover popup left open when a drag starts
//     painted *over* the drop-zone highlight that owns the pointer. TUI put
//     the ghost inside the band, where it belongs. [`EDITOR_Z_ORDER`] takes
//     TUI's placement.
//
// [`compose_editor_band`] states the order and the gates once, and both
// backends walk it.
//
// **Why here and not in quadraui** (`CLAUDE.md`'s "check quadraui first"):
// the same verdict slices 1 and 2 recorded, re-run for this band. quadraui's
// `compose::app_shell::AppShell` hands both backends the *shell's* zones and
// stops at `main_content_bounds`; what vimcode stacks inside that rect —
// which of N editor groups gets a tab bar, where the breadcrumb row sits
// relative to it, whether a group boundary gets a line — is vimcode's own
// app-level composition. quadraui states no order for any of it, so the
// ordering is vimcode's to state and `render.rs` is where vimcode states
// cross-backend contracts. The *rasterisers* underneath every rung are
// already quadraui's (`draw_editor`, `draw_minimap`, `draw_tab_bar_icons`,
// `draw_status_bar`, `draw_split`, `draw_drop_overlay`) — there is no
// per-backend painting left to lift, only per-backend *sequencing*, which is
// what this slice deletes.

/// One rung of the shared **editor band** — the surfaces vimcode stacks
/// inside `AppShellLayout::main_content_bounds`.
///
/// Deliberately unit-agnostic, exactly like [`FrameOp`]:
/// the rung says *what* is painted and *in what order*, never where or how
/// big. GTK composes it in pixels and TUI in cells, from the same vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorOp {
    /// Every editor window's text, gutter, per-window status line and the
    /// `:split`/`:vsplit` divider lines *within* each group
    /// (`ScreenLayout::window_dividers`).
    ///
    /// First, and deliberately before the tab bars: in a horizontal group
    /// split an adjacent group's window content would otherwise overwrite
    /// the neighbouring tab row (the note `draw_frame` has carried since
    /// #551).
    Windows,
    /// The per-window minimap strips (`ScreenLayout::minimap`, one entry per
    /// `WindowId`, not just the active window's — #35/#722). Painted over the
    /// windows because the strip is inset into the window's own right edge.
    Minimap,
    /// One tab bar per editor group (`ScreenLayout::group_tab_bars` via
    /// [`tab_bar_draw_targets`]). A single group is a split of one (#551), so
    /// there is no separate "unsplit" rung.
    TabBars,
    /// One breadcrumb bar per group, below that group's tab bar
    /// (`ScreenLayout::breadcrumbs` via [`breadcrumb_draw_targets`]).
    Breadcrumbs,
    /// Divider lines *between* editor groups (`Ctrl+W v` / `Ctrl+W s`
    /// boundaries — `ScreenLayout::group_dividers`), as opposed to the
    /// within-group `:split` lines the [`Self::Windows`] rung draws.
    ///
    /// Composed *after* the tab bars on purpose: a side-by-side (`Ctrl+W v`)
    /// group boundary is the outermost structure in the editor column and
    /// must stay visible where it runs *alongside* a neighbouring group's
    /// tab row. A stacked (`Ctrl+W s`) boundary runs *through* one instead —
    /// this rung's body ([`painted_group_dividers`]) never paints that
    /// direction at all, relying on the lower group's own tab row to read as
    /// the separator (#1586) — so composition order is moot for it either
    /// way.
    GroupDividers,
    /// The tab-drag drop feedback: drop-zone highlight, insertion bar and the
    /// dragged tab's ghost.
    TabDragOverlay,
    /// The tab-hover tooltip naming the buffer under the pointer
    /// (`ScreenLayout::tab_tooltip`).
    TabTooltip,
}

/// The canonical editor-band order, **lowest z first** (index 0 is composed
/// first, and everything after it may cover it).
///
/// Both backends iterate this array and `match` each rung, so "which order do
/// we stack the editor column in" is one artefact rather than two
/// transcriptions. Adding a surface means adding a variant here — a compile
/// error in both backends' `match` until both handle it, which is what makes
/// "populated but never composed" (#587/#592) structurally harder to reach,
/// and is precisely the guard [`EditorOp::GroupDividers`] did not have.
///
/// The whole band sits *below* [`FRAME_Z_ORDER`]: every editor rung is
/// composed before the first chrome
/// rung, on both backends. The editor-anchored popups (completion / hover /
/// signature help / diff peek) are **not** in this band — they are anchored to
/// the *cursor*, not to the editor column's own structure, and both backends
/// already paint them from one shared set of adapters. They stay where they
/// are, immediately after the band.
pub const EDITOR_Z_ORDER: [EditorOp; 7] = [
    EditorOp::Windows,
    EditorOp::Minimap,
    EditorOp::TabBars,
    EditorOp::Breadcrumbs,
    EditorOp::GroupDividers,
    EditorOp::TabDragOverlay,
    EditorOp::TabTooltip,
];

/// Is this group's tab bar painted this frame?
///
/// The single predicate behind both [`tab_bar_draw_targets`]'s filter and
/// [`compose_editor_band`]'s `TabBars` gate, so the gate cannot claim a rung
/// the paint then skips (or vice versa) — the drift [`FrameOp`]'s two
/// hand-written sidebar/menu-row gates had actually accumulated before #763.
pub(crate) fn tab_bar_is_drawn(engine: &Engine, gtb: &GroupTabBar) -> bool {
    !engine.is_tab_bar_hidden(gtb.group_id) && gtb.bounds.width > 0.0
}

/// Is this group's breadcrumb bar painted this frame? The
/// [`breadcrumb_draw_targets`] twin of [`tab_bar_is_drawn`], modulo the
/// caller-level `terminal_maximized` veto that hides the whole row.
pub(crate) fn breadcrumb_is_drawn(bc: &BreadcrumbBar) -> bool {
    !bc.segments.is_empty() && bc.bounds.width > 0.0
}

/// Which editor rungs a frame in this state must compose, in canonical order.
///
/// Like [`compose_frame`], this returns
/// only the *live* rungs: no editor rung owns a hit-test cache that has to be
/// cleared from an absent branch (the two that come closest — GTK's tab pixel
/// hit maps and the breadcrumb draw layouts — are cleared wholesale at the top
/// of the frame, before the walk).
///
/// `drag_active` is the one input that is not derivable from `screen`: the
/// drag lives in each backend's own `render::TabDragState` (GTK's
/// `App::tab_drag`, TUI's the pre-#1434 TUI shell's `tab_drag`), not in `ScreenLayout`.
/// Both pass `tab_drag.source().is_some()`.
pub fn compose_editor_band(
    engine: &Engine,
    screen: &ScreenLayout,
    drag_active: bool,
    terminal_maximized: bool,
) -> Vec<EditorOp> {
    EDITOR_Z_ORDER
        .iter()
        .copied()
        .filter(|op| match op {
            EditorOp::Windows => !screen.windows.is_empty(),
            // Empty whenever `minimap_reserved_width` reserved nothing —
            // which is how `:set nominimap` reaches the paint path — so no
            // separate `settings.minimap` test here (that would be a second
            // copy of the gate, free to drift from the reservation).
            EditorOp::Minimap => !screen.minimap.is_empty(),
            EditorOp::TabBars => screen
                .group_tab_bars
                .iter()
                .any(|gtb| tab_bar_is_drawn(engine, gtb)),
            EditorOp::Breadcrumbs => {
                !terminal_maximized && screen.breadcrumbs.iter().any(breadcrumb_is_drawn)
            }
            // Naturally empty with a single group — `GroupLayout::Leaf
            // ::dividers()` returns `vec![]` — so this needs no
            // `editor_group_split.is_some()` gate (#551). Gated on the
            // *painted* set (#1586's `painted_group_dividers`), not the raw
            // one: a frame whose only group divider is a stacked one (never
            // painted, see that fn's doc) composes no line at all, so this
            // rung isn't live for it either — keeps this filter and
            // [`paint_editor_band_rungs`]'s body from being able to
            // disagree about what "live" means for this op, the same
            // invariant [`tab_bar_is_drawn`] states for `TabBars`.
            EditorOp::GroupDividers => !painted_group_dividers(&screen.group_dividers).is_empty(),
            EditorOp::TabDragOverlay => drag_active,
            EditorOp::TabTooltip => screen.tab_tooltip.is_some(),
        })
        .collect()
}

/// Assert a backend's *actually composed* editor sequence never runs backwards
/// against [`EDITOR_Z_ORDER`].
///
/// The editor-band twin of [`check_frame_order`], and the same weaker half of
/// the acceptance
/// test: it does not care which rungs were live, only that whatever *was*
/// composed came out in canonical order. A rung hoisted out of the shared walk
/// — which is exactly how GTK's tab-drag ghost drifted past the editor popups
/// — fails this even when the exact live set is awkward to pin.
pub fn check_editor_band_order(composed: &[EditorOp]) -> Result<(), String> {
    let mut cursor = 0usize;
    for op in composed {
        match EDITOR_Z_ORDER[cursor..].iter().position(|c| c == op) {
            Some(offset) => cursor += offset + 1,
            None => {
                return Err(format!(
                    "editor band composed out of order: {composed:?}\n\
                     {op:?} came after a rung that EDITOR_Z_ORDER puts above it.\n\
                     canonical order is {EDITOR_Z_ORDER:?}"
                ));
            }
        }
    }
    Ok(())
}

/// The expected editor band for the cross-backend fixture used by
/// `editor_band_composes_in_canonical_order_via_gtk_driver` and
/// `..._via_shell_app`: a two-group `Ctrl+W v` split with breadcrumbs and the
/// minimap on and a tab-hover tooltip up — every rung live except the
/// tab-drag ghost, which needs a live pointer drag neither harness can start
/// from engine state alone.
///
/// Both backend tests call this one function — a single `#[cfg(test)]` fn in
/// `render.rs`, compiled into both bin targets — rather than each transcribing
/// its own `Vec<EditorOp>` literal, so the compiler keeps the two expectations
/// in step. Taking `drag` as a parameter keeps the expectation
/// *discriminating*: it is not simply "whatever [`EDITOR_Z_ORDER`] contains".
#[cfg(test)]
pub(crate) fn editor_band_fixture(drag: bool) -> Vec<EditorOp> {
    EDITOR_Z_ORDER
        .iter()
        .copied()
        .filter(|op| drag || !matches!(op, EditorOp::TabDragOverlay))
        .collect()
}

/// The unit system one backend composes [`paint_editor_band_rungs`] in —
/// [`BottomPanelUnits`]'s sibling for the editor band. `metrics` answers the
/// same "at least one line tall / one column wide" question [`FrameMetrics`]
/// exists for; `tab_row_h` is a second, independent unit because the tab
/// strip's own row is *not* one text line on GTK (it runs ~1.6× a line
/// height there) while it is unconditionally exactly one cell on TUI
/// regardless of the breadcrumbs setting — see the [`EditorOp::TabTooltip`]
/// call site below for the one rung that reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorBandUnits {
    pub metrics: FrameMetrics,
    /// Row height of the tab strip itself, in the caller's units.
    pub tab_row_h: f32,
}

impl EditorBandUnits {
    /// One terminal cell — what the pre-#1434 TUI shell's `paint_editor_band` composes in.
    pub const CELL: Self = Self {
        metrics: FrameMetrics::CELL,
        tab_row_h: 1.0,
    };

    /// Real pixels — what `gtk::App::compose_editor_band_rungs` composes in.
    /// `tab_row_h` is the caller's own measured tab-strip row height, not
    /// derived from `line_height` (see the struct doc for why the two
    /// disagree on GTK).
    pub fn px(line_height: f64, char_width: f64, tab_row_h: f64) -> Self {
        Self {
            metrics: FrameMetrics::px(line_height, char_width),
            tab_row_h: tab_row_h as f32,
        }
    }
}

// #1499: [`paint_editor_band_rungs`]'s four "genuinely per-backend" rungs —
// `Windows`, `TabBars`, `GroupDividers`, `TabDragOverlay` — used to live
// behind a `host: &mut impl EditorBandHost<'screen>` trait, implemented once
// per backend (`GtkEditorBandHost` in `app.rs`, a TUI twin before #1434).
// With only `App` left, the function takes `app: &App` plus the
// `window_editors`/`hit_bars` accumulators directly instead — see its own
// doc below.
//
// `Minimap`, `Breadcrumbs` and `TabTooltip` need no `app` at all: their
// bodies were already byte-identical between GTK and TUI, which is exactly
// what made them safe to inline into the shared walk directly rather than
// leaving them as a fifth/sixth/seventh trivial forwarding method.
//
// [`EditorOp::Windows`] (every editor window's text, gutter, per-window
// status line and the `:split`/`:vsplit` divider lines within each group)
// accumulates each window's owned `quadraui::Editor` into `window_editors` so
// `App`'s own `FrameHitMap` (#449) hit-tests the exact objects just painted,
// never a second copy that could drift. [`EditorOp::TabBars`] (one tab bar
// per editor group) similarly recovers the rasteriser's exact *pixel* hit
// geometry (`cached_tab_pixel_hits`/`cached_tab_close_abs`/
// `cached_tab_slots_abs`) for pixel-accurate click/hover resolution (#515,
// #703, #764) into `hit_bars`. [`EditorOp::GroupDividers`] (the
// between-*group* `Ctrl+W v`/`Ctrl+W s` boundary lines) and
// [`EditorOp::TabDragOverlay`] (the tab-drag drop-zone highlight, insertion
// bar and dragged-tab ghost) need no accumulator at all.

/// The shared **editor band** walk (#1251): the single ordered loop both
/// the pre-#1434 TUI shell's `paint_editor_band` and `App::compose_editor_band_rungs` run
/// over [`compose_editor_band`], replacing what used to be two hand-written
/// copies of the same seven-armed `match`. `Minimap`, `Breadcrumbs` and
/// `TabTooltip` paint identically on both backends (modulo `units`) and are
/// inlined here directly.
///
/// `band` is the editor column's bounds in the caller's units (only `x`, `y`
/// and `width` are read, by the `TabTooltip` rung); `units` carries the
/// metrics and tab-row height the same rung needs, and `tab_bar_h` (not part
/// of `units` — only the `TabBars`/`TabDragOverlay` rungs need it) is the
/// tab strip's own painted row height. `window_editors`/`hit_bars` accumulate
/// across the `Windows`/`TabBars` calls — the caller destructures them back
/// out once this returns, to build its `FrameHitMap`. Returns the rungs
/// actually composed, in order, for the caller to stash and validate with
/// [`check_editor_band_order`] — callers do that themselves (rather than this
/// function doing it) so the assertion message keeps each backend's own
/// "TUI "/"GTK " prefix, unchanged from before this convergence.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_editor_band_rungs<'screen>(
    backend: &mut dyn quadraui::Backend,
    engine: &Engine,
    screen: &'screen ScreenLayout,
    theme: &Theme,
    band: quadraui::Rect,
    units: EditorBandUnits,
    tab_bar_h: f64,
    drag_active: bool,
    app: &crate::app::App,
    window_editors: &mut Vec<quadraui::Editor>,
    hit_bars: &mut Vec<(GroupId, quadraui::Rect, &'screen quadraui::TabBar)>,
) -> Vec<EditorOp> {
    let mut composed = Vec::new();
    for op in compose_editor_band(engine, screen, drag_active, engine.terminal_maximized) {
        match op {
            EditorOp::Windows => app.paint_editor_windows_rung(
                backend,
                screen,
                units.metrics.line_height as f64,
                window_editors,
            ),
            // #35/#722: minimap strips on every window's right edge (one
            // entry per `WindowId` in `screen.minimap`, not just the active
            // window's) — one call, the rasteriser is quadraui's.
            EditorOp::Minimap => draw_minimap_strip(backend, screen),
            EditorOp::TabBars => app.paint_tab_bars_rung(
                backend,
                engine,
                screen,
                units.tab_row_h as f64,
                tab_bar_h,
                hit_bars,
            ),
            EditorOp::Breadcrumbs => {
                paint_breadcrumb_bars(backend, screen, engine.terminal_maximized)
            }
            EditorOp::GroupDividers => {
                let painted = painted_group_dividers(&screen.group_dividers);
                draw_dividers_as_splits(backend, &painted, |div| {
                    quadraui::WidgetId::new(format!("gdiv:{}", div.split_index))
                })
            }
            EditorOp::TabDragOverlay => {
                app.cache_tab_drop_geometry(screen, engine, tab_bar_h);
                let ctx = app.cached_drop_ctx.borrow();
                let (mx, my) = app.mouse_pos_cell.get();
                paint_tab_drop_overlay(
                    backend,
                    &ctx,
                    (mx as f32, my as f32),
                    2.0,
                    units.metrics.line_height,
                );
            }
            // Positioned one *tab row* below the top of the editor column —
            // `units.tab_row_h`, not `units.metrics.line_height` — mirroring
            // GTK's `tab_row_h` / TUI's always-one-cell tab strip; see
            // `EditorBandUnits`'s doc for why the two units genuinely differ.
            EditorOp::TabTooltip => {
                if let Some(ref tooltip_text) = screen.tab_tooltip {
                    tab_hover_tooltip_paint(
                        backend,
                        band.x,
                        band.y + units.tab_row_h,
                        band.width,
                        tooltip_text,
                        theme,
                        units.metrics.char_width,
                        units.metrics.line_height,
                    );
                }
            }
        }
        composed.push(op);
    }
    composed
}

// ══════════════════════════════════════════════════════════════════════════
// Bottom band (#765, #735 slice 4)
// ══════════════════════════════════════════════════════════════════════════
//
// The fourth and last of #735's *ordered runs*. Below the editor column
// ([`EDITOR_Z_ORDER`]) and above the surrounding chrome ([`FRAME_Z_ORDER`])
// sits a stack of bands vimcode carves out of the main content area by hand:
// quickfix, the bottom panel (terminal / debug output), the debug toolbar, the
// separated status line, and the sidebar-item hover popup that overhangs them.
//
// Before this slice that run was transcribed **three** times — GTK's
// `render_content`, TUI's `render_content`, and TUI's `#[cfg(test)]`
// `draw_frame` parity twin — and the three transcriptions had drifted in four
// separate ways, three of them real defects rather than cosmetic ordering:
//
//   * **GTK's panel-hover popup could not clear its own click-routing cache.**
//     The rung lived *inside* the [`FrameOp::SidebarPanel`] arm, nested under
//     `if let Some(q_sb) = layout.sidebar_content_bounds`, and so were its two
//     cache resets. Collapse the sidebar while a source-control / extension
//     item tooltip was up and `App::panel_hover_popup_rect` kept the last
//     painted rect **forever**: `handle_mouse_press` went on arbitrating
//     clicks against a popup that was no longer on screen, swallowing them
//     before the editor ever saw them. Input and paint disagreeing about a
//     surface that is not painted — the #587/#592 failure shape verbatim, and
//     precisely what a band walk makes structurally hard to reach, since
//     [`BottomOp::PanelHover`] is now composed (or not) at the top level where
//     the absent branch is reachable.
//   * **TUI's debug toolbar had the mirror-image bug.** Its rung had no `else`
//     at all, so the pre-#1434 TUI shell's `debug_toolbar_rect` kept its last value when
//     the toolbar hid — while GTK's `else` zeroed the equivalent two caches.
//     Two backends, two different halves of the same cache-clearing rule.
//   * **the separated status line was composed in two different places.** TUI
//     painted it *second*, before the bottom panel; GTK painted it *fourth*,
//     after the debug toolbar. Both backends' *geometry* agrees it belongs
//     last (GTK's `separated_status_y` y-cursor chain, TUI's own
//     `bottom_chrome_rects_for_shell_content` constraint array, which orders
//     `[editor, quickfix, bottom, debug, separated_status]`), so TUI's paint
//     order contradicted TUI's own layout order. Benign only for as long as no
//     rung in the band ever overdraws its neighbour. [`BOTTOM_Z_ORDER`] takes
//     the geometry order, which both backends already reserved space in.
//   * the two gates for the bottom panel itself were spelled differently —
//     GTK tested `el.terminal_h > 0.0` (a *height*, downstream of the rule),
//     TUI `chrome.bottom_panel.height > 0`. [`bottom_panel_is_drawn`] is now
//     the rule itself, stated once.
//
// **Why here and not in quadraui** (`CLAUDE.md`'s "check quadraui first"): the
// same verdict slices 1–3 recorded, re-run for this band. quadraui *does* ship
// a `BottomPanelController` — and `AppShellLayout::bottom_panel_bounds` with
// it — but it models a single generic drawer, not vimcode's stack of five
// independently-gated bands with a hover popup overhanging them; it is `None`
// for the pre-#1434 TUI shell and unwired on GTK for exactly that reason (see
// `bottom_chrome_rects_for_shell_content`'s doc comment). What vimcode stacks
// below its editor column, and in what order, is vimcode's own app-level
// composition. The *rasterisers* underneath every rung are already quadraui's
// (`draw_list`, `draw_tab_bar`, `draw_terminal`, `draw_terminal_divider`,
// `draw_text_display`, `draw_status_bar`, `draw_rich_text_popup`) — there is
// no per-backend painting left to lift, only per-backend *sequencing*, which
// is what this slice deletes.
//
// **Re-examined for #820** ("adopt `BottomPanelController` on GTK"), which
// filed against a grep that read "0 uses in `src/app.rs`" as an unstarted
// adoption. It isn't: both backends' zero call sites are deliberate (this
// paragraph, `bottom_chrome_rects_for_shell_content`'s doc comment in
// `tui_main/render_impl.rs`, and the pre-#1434 TUI shell's `render_content`'s own note by
// `BottomChromeRects`'s construction all record the same verdict), and the
// "3 mentions in the TUI" #820 read as partial progress are those three
// rejection notes, not partial implementation. The blocker restated: `AppShell`
// positions `BottomPanelController` as the last band before the content
// area's bottom edge, so it cannot coexist with the debug toolbar and
// separated-status rows vimcode stacks *below* the terminal/debug-output
// panel — adopting it would need a `ShellConfig`/`AppShellLayout` capable of
// more than one independently-gated bottom band, which does not exist
// upstream today.
//
// **That upstream gap has since shipped.** quadraui#997 (`d1b1931`) added N
// independently-gated stacked bottom bands, and vimcode's pinned rev carries
// it (the "multi-band bottom chrome" draft in
// `docs/PENDING_QUADRAUI_ISSUES.md` was struck 2026-09-22, #1259). The
// "does not exist upstream" blocker above is therefore historical: moving this
// band onto the shipped API is now an adoption question for vimcode, not a
// missing quadraui capability.

/// One rung of the shared **bottom band** — the stack of chrome vimcode carves
/// out of the bottom of `AppShellLayout::main_content_bounds`.
///
/// Deliberately unit-agnostic, exactly like [`FrameOp`] and
/// [`EditorOp`]: the rung says *what* is painted and *in what order*, never
/// where or how big. GTK composes it in pixels and TUI in cells, from the same
/// vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BottomOp {
    /// The quickfix result list (`ScreenLayout::quickfix`), directly below the
    /// editor column.
    Quickfix,
    /// The bottom panel: its tab strip plus whichever of the terminal or the
    /// debug output is active (`ScreenLayout::bottom_tabs`).
    BottomPanel,
    /// The debug toolbar strip (`ScreenLayout::debug_toolbar`).
    DebugToolbar,
    /// The extracted per-window status line, shown when `window_status_line`
    /// is on but `status_line_above_terminal` is off
    /// (`ScreenLayout::separated_status_line`).
    ///
    /// Last of the four stacked bands, because that is where *both* backends
    /// already reserve its row — see the module note above for the paint-order
    /// contradiction this fixes on TUI.
    SeparatedStatus,
    /// The sidebar-item dwell tooltip (`ScreenLayout::panel_hover`).
    ///
    /// In the band rather than beside the sidebar body it describes, because
    /// it is clamped against the *whole* content viewport and deliberately
    /// overhangs rightward past the sidebar's own bounds — so it must be
    /// composed after everything it can overhang, which is the entire stack
    /// above. GTK used to nest it inside the sidebar rung instead; see the
    /// module note for the stale-cache bug that caused.
    PanelHover,
}

/// The canonical bottom-band order, **lowest z first** (index 0 is composed
/// first, and everything after it may cover it).
///
/// The first four rungs are also, and not coincidentally, in top-to-bottom
/// *geometric* order: each one's y-cursor is the previous one's bottom edge on
/// GTK, and its successor in `bottom_chrome_rects_for_shell_content`'s
/// constraint array on TUI. Only [`BottomOp::PanelHover`] genuinely floats.
///
/// Both backends iterate this array and `match` each rung, so "which order do
/// we stack the bottom chrome in" is one artefact rather than three
/// transcriptions. Adding a surface means adding a variant here — a compile
/// error in both backends' `match` until both handle it.
///
/// The whole band sits *below* [`FRAME_Z_ORDER`] and *above*
/// [`EDITOR_Z_ORDER`]: every bottom rung is composed after the last editor
/// rung and before the first chrome rung, on both backends.
pub const BOTTOM_Z_ORDER: [BottomOp; 5] = [
    BottomOp::Quickfix,
    BottomOp::BottomPanel,
    BottomOp::DebugToolbar,
    BottomOp::SeparatedStatus,
    BottomOp::PanelHover,
];

/// Is the bottom panel painted this frame?
///
/// The rule itself, rather than either backend's downstream restatement of it:
/// GTK gated on `compute_editor_layout`'s `el.terminal_h > 0.0` and TUI on
/// `bottom_chrome_rects_for_shell_content`'s `chrome.bottom_panel.height > 0`.
/// Both are *heights computed from this predicate*, so both were free to drift
/// from it — and from each other — the moment either layout function changed.
pub fn bottom_panel_is_drawn(engine: &Engine) -> bool {
    engine.terminal_open || engine.bottom_panel_open
}

/// Is the sidebar-item hover popup painted this frame?
///
/// `screen.panel_hover` is populated only while the pointer is dwelling on an
/// item of a sidebar panel that has tooltips, so it already implies the
/// "source control or extension panel" test TUI's call site used to spell out
/// a second time — `render_panel_hover_popup` and [`panel_hover_popup_paint`]
/// both return early on `None` regardless.
///
/// `sidebar_open` is the one input that is not derivable from `screen` — the
/// same shape as [`compose_editor_band`]'s `drag_active`. Both live backends
/// pass `layout.sidebar_content_bounds.is_some()`: the popup is anchored to
/// the sidebar's *right edge*, so an open sidebar is what gives it an anchor
/// at all. Note this is deliberately **not** `app_shell.sidebar_visible()`,
/// which the two backends disagreed about — TUI tested both, GTK only the
/// bounds — and which is `false` for a default engine whose sidebar band the
/// shell has nonetheless reserved.
pub fn panel_hover_is_drawn(screen: &ScreenLayout, sidebar_open: bool) -> bool {
    sidebar_open && screen.panel_hover.is_some()
}

/// Which bottom rungs a frame in this state must compose, in canonical order.
///
/// Like [`compose_frame`] / [`compose_editor_band`], this returns only the
/// *live* rungs. Two rungs in
/// this band own hit-test caches that must be cleared when they are absent —
/// the bottom panel's `Engine::bottom_panel_geometry` and the hover popup's
/// popup/link rects — and both backends now clear them **before** the walk
/// rather than from an `else` arm inside it. That is deliberate: an `else`
/// that only runs when the walk reaches the rung cannot run when the rung is
/// gated off upstream, which is exactly how GTK's popup rect went stale (see
/// the module note above).
pub fn compose_bottom_band(
    engine: &Engine,
    screen: &ScreenLayout,
    sidebar_open: bool,
) -> Vec<BottomOp> {
    BOTTOM_Z_ORDER
        .iter()
        .copied()
        .filter(|op| match op {
            // Already `None` unless `quickfix.open && !quickfix.items
            // .is_empty()` (`build_screen_layout`), the same rule
            // `quickfix_panel_rows` reserves height by — so no second copy of
            // that gate here.
            BottomOp::Quickfix => screen.quickfix.is_some(),
            BottomOp::BottomPanel => bottom_panel_is_drawn(engine),
            BottomOp::DebugToolbar => screen.debug_toolbar.is_some(),
            BottomOp::SeparatedStatus => screen.separated_status_line.is_some(),
            BottomOp::PanelHover => panel_hover_is_drawn(screen, sidebar_open),
        })
        .collect()
}

/// Assert a backend's *actually composed* bottom sequence never runs backwards
/// against [`BOTTOM_Z_ORDER`].
///
/// The bottom-band twin of [`check_frame_order`] /
/// [`check_editor_band_order`], and the same
/// weaker half of the acceptance test: it does not care which rungs were live,
/// only that whatever *was* composed came out in canonical order. A rung
/// hoisted out of the shared walk — which is exactly how TUI's separated
/// status line drifted two places up the ladder — fails this even when the
/// exact live set is awkward to pin.
pub fn check_bottom_band_order(composed: &[BottomOp]) -> Result<(), String> {
    let mut cursor = 0usize;
    for op in composed {
        match BOTTOM_Z_ORDER[cursor..].iter().position(|c| c == op) {
            Some(rel) => cursor += rel + 1,
            None => {
                return Err(format!(
                    "bottom band composed out of order: {op:?} came after \
                     {:?}, but BOTTOM_Z_ORDER puts it before",
                    &BOTTOM_Z_ORDER[..cursor]
                ));
            }
        }
    }
    Ok(())
}

/// The expected bottom band for the cross-backend fixture used by
/// `bottom_band_composes_in_canonical_order_via_gtk_driver` and
/// `..._via_shell_app`: quickfix open with items, the bottom panel up, the
/// debug toolbar visible and the separated status line extracted — every rung
/// live except the sidebar hover popup, which needs a live pointer dwell.
///
/// Both backend tests call this one function — a single `#[cfg(test)]` fn in
/// `render.rs`, compiled into both bin targets — rather than each transcribing
/// its own `Vec<BottomOp>` literal, so the compiler keeps the two expectations
/// in step. Taking `panel_hover` as a parameter keeps the expectation
/// *discriminating*: it is not simply "whatever [`BOTTOM_Z_ORDER`] contains".
#[cfg(test)]
pub(crate) fn bottom_band_fixture(panel_hover: bool) -> Vec<BottomOp> {
    BOTTOM_Z_ORDER
        .iter()
        .copied()
        .filter(|op| panel_hover || !matches!(op, BottomOp::PanelHover))
        .collect()
}

/// How far down the quickfix list must be scrolled to keep the selected item
/// on screen, given `visible_rows` rows of body (i.e. the panel height minus
/// its one header row).
///
/// GTK recomputes this statelessly every frame — it has no persistent
/// `quickfix_scroll_top` to advance from key events, the way the pre-#1434 TUI shell
/// does — so it lived inline in `render_content` as eight lines of arithmetic.
/// Shared here so the two backends cannot disagree about what "keep the
/// selection visible" means.
pub fn quickfix_scroll_top(qf: &QuickfixPanel, visible_rows: usize) -> usize {
    if visible_rows == 0 {
        0
    } else {
        (qf.selected_idx + 1).saturating_sub(visible_rows)
    }
}

/// The [`BottomOp::Quickfix`] rung's whole body on both backends.
///
/// One `quickfix_to_list_view` adapter call and one `Backend::draw_list`; the
/// only thing the two call sites ever differed on was where `scroll_offset`
/// came from, which stays the caller's (GTK recomputes it via
/// [`quickfix_scroll_top`], TUI carries the pre-#1434 TUI shell's `quickfix_scroll_top`
/// across frames so `:cnext` can advance it).
pub fn paint_quickfix_rung(
    b: &mut dyn quadraui::Backend,
    qf: &QuickfixPanel,
    rect: quadraui::Rect,
    scroll_top: usize,
) {
    if rect.height <= 0.0 {
        return;
    }
    let mut list = quickfix_to_list_view(qf);
    list.scroll_offset = scroll_top;
    b.draw_list(rect, &list);
}

/// The [`BottomOp::SeparatedStatus`] rung's whole body on both backends.
///
/// Returns the `StatusBarLayout` the paint resolved, so the caller can record
/// its segment hit zones (#672) without a second no-paint measurement of the
/// same bar — the #654/#703 desync shape [`PaintedTabBar`]'s doc comment
/// describes. GTK used to `draw` and then call `backend.status_bar_layout` on
/// the same rect, laying the row out twice per frame; that second call is what
/// this returns instead.
pub fn paint_separated_status_rung(
    b: &mut dyn quadraui::Backend,
    status: &WindowStatusLine,
    rect: quadraui::Rect,
) -> quadraui::StatusBarLayout {
    let bar = window_status_line_to_status_bar(status, quadraui::WidgetId::new("status:separated"));
    let _ = b.draw_status_bar_interactive(rect, &bar, &quadraui::InteractionState::new());
    b.status_bar_layout(rect, &bar)
}

/// Paint a plain, content-free fill behind the "real" status bar at
/// `real_bg` — #1690's edge-to-edge VS Code parity backdrop, shared by
/// `App::paint_editor_windows_rung`'s single-window case and
/// `App::compose_bottom_band_rungs`'s `SeparatedStatus` arm.
///
/// Deliberately a *separate* paint rather than widening the real bar's own
/// rect: AppShell's activity-bar/sidebar click hit-test runs before
/// `App::handle` ever sees the event (quadraui's `ShellAdapter::handle`),
/// so widening the real bar would move its segments' hit zones into screen
/// positions a click can never actually reach — see [`status_bands`]'s doc
/// for the longer version. This backdrop carries no segments of its own
/// (so it is never stored in a click-hit map and can't be clicked), just
/// the real bar's own fill colour, painted into `rect` — which callers
/// widen to the real window/terminal width (`Backend::viewport().width`)
/// while the real bar underneath stays window/main-content-bounded.
///
/// `real_bg` is `None` when the real bar has no segments at all (nothing
/// to colour-match), in which case this is a no-op — matching
/// `status_bar.rs::paint`'s own "no segments → `theme.background`" default
/// would risk painting a visibly different fill than whatever the real bar
/// ends up using once it *does* have segments.
pub fn paint_status_backdrop(
    b: &mut dyn quadraui::Backend,
    id: &str,
    rect: quadraui::Rect,
    real_bg: Option<quadraui::Color>,
) {
    let Some(bg) = real_bg else {
        return;
    };
    let backdrop = quadraui::StatusBar {
        id: quadraui::WidgetId::new(id),
        left_segments: vec![quadraui::StatusBarSegment {
            text: String::new(),
            fg: bg,
            bg,
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    };
    let _ = b.draw_status_bar_interactive(rect, &backdrop, &quadraui::InteractionState::new());
}

/// The unit system one backend composes [`paint_bottom_panel_rung`] in.
///
/// Everything here is a genuine rasteriser difference #735 exists to preserve,
/// not a behaviour knob: Cairo repaints the whole surface every frame so GTK
/// needs no background pass, while ratatui coalesces cells and must blank the
/// terminal body itself; and a scrollbar gutter is six *pixels* on GTK and
/// nothing at all in a cell grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BottomPanelUnits {
    /// One line / one column, in the caller's units.
    pub metrics: FrameMetrics,
    /// Width of the gutter `build_terminal_draw_data` reserves for the
    /// scrollbar, in the caller's units, or `None` for a cell grid that has no
    /// room for one.
    pub terminal_scrollbar: Option<u16>,
    /// Blank the terminal body before painting into it. `false` on GTK (Cairo
    /// already cleared the surface), `true` on TUI.
    pub clear_terminal_bg: bool,
}

impl BottomPanelUnits {
    /// One terminal cell — what the pre-#1434 TUI shell's `render_content` composes in.
    pub const CELL: Self = Self {
        metrics: FrameMetrics::CELL,
        terminal_scrollbar: None,
        clear_terminal_bg: true,
    };

    /// Real pixels — what `gtk::App::render_content` composes in.
    pub fn px(line_height: f64, char_width: f64) -> Self {
        Self {
            metrics: FrameMetrics::px(line_height, char_width),
            terminal_scrollbar: Some(6),
            clear_terminal_bg: false,
        }
    }
}

/// The [`BottomOp::BottomPanel`] rung's whole body on both backends: the tab
/// strip, then whichever of the terminal or the debug output is active,
/// including the toolbar row, the split divider and both scroll surfaces.
///
/// `rect` is the whole panel band in the caller's units, tab strip included.
/// The three transcriptions of this that existed before #765 ran to ~170 lines
/// on GTK and ~120 apiece in TUI's two composers, and the debug-output arm's
/// scrollbar mapping was duplicated character-for-character between them.
pub fn paint_bottom_panel_rung(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    screen: &ScreenLayout,
    theme: &Theme,
    rect: quadraui::Rect,
    units: BottomPanelUnits,
) {
    let lh = units.metrics.line_height;
    let cw = units.metrics.char_width;
    engine
        .bottom_panel_geometry
        .replace(Some(crate::core::engine::BottomPanelGeometry {
            top_y: rect.y as f64,
            height: rect.height as f64,
            toolbar_y: lh as f64,
            content_y: 2.0 * lh as f64,
            content_row_h: lh as f64,
        }));

    // Row 1: the Terminal / Debug Output tab strip.
    let tab_bar = build_bottom_panel_tab_bar(
        &screen.bottom_tabs.active,
        engine.terminal_open,
        !screen.bottom_tabs.output_lines.is_empty(),
    );
    let layout = b.draw_tab_bar_layout(
        quadraui::Rect::new(rect.x, rect.y, rect.width, lh),
        &tab_bar,
        None,
    );
    engine
        .bottom_tab_bar_hits
        .replace(Some(crate::core::engine::BottomTabStripHits {
            layout,
            origin_x: rect.x as f64,
        }));

    // Rows 2..: the active panel's own toolbar row and body.
    let content_y = rect.y + 2.0 * lh;
    let content_h = (rect.height - 2.0 * lh).max(0.0);
    match screen.bottom_tabs.active {
        BottomPanelKind::Terminal => {
            let Some(ref term) = screen.bottom_tabs.terminal else {
                return;
            };
            let toolbar_rect = quadraui::Rect::new(rect.x, rect.y + lh, rect.width, lh);
            let toolbar_hits = match build_terminal_toolbar(term, theme) {
                TerminalToolbar::FindBar(bar) => {
                    let _ = b.draw_status_bar_interactive(
                        toolbar_rect,
                        &bar,
                        &quadraui::InteractionState::new(),
                    );
                    // No raw `pango::Layout` is reachable from a `&mut dyn
                    // Backend`-only signature (the #669 gap), so segment widths
                    // are approximated by char count * `cw` rather than exact
                    // glyph measurement — affects hit-region precision only,
                    // not paint.
                    // The two call sites spelled this gap `16.0` (GTK px) and
                    // `2.0` (TUI cells); at GTK's ~8px char width those are the
                    // same two-character gap, so it is `2.0 * cw` in both.
                    let layout = bar.layout(rect.width, lh, 2.0 * cw, |seg| {
                        quadraui::StatusSegmentMeasure::new(seg.text.chars().count() as f32 * cw)
                    });
                    crate::core::engine::TerminalToolbarHits::FindBar {
                        layout,
                        origin_x: rect.x as f64,
                    }
                }
                TerminalToolbar::TabStrip(bar) => {
                    crate::core::engine::TerminalToolbarHits::TabStrip {
                        layout: b.draw_tab_bar_layout(toolbar_rect, &bar, None),
                        origin_x: toolbar_rect.x as f64,
                    }
                }
            };
            engine.terminal_toolbar_hits.replace(Some(toolbar_hits));
            if content_h <= 0.0 {
                return;
            }
            let body = quadraui::Rect::new(rect.x, content_y, rect.width, content_h);
            let visible_rows = (content_h / lh) as usize;

            // ratatui coalesces cells, so the body has to be blanked before
            // the grid lands on it; Cairo has already cleared the surface.
            // `draw_status_bar`'s TUI rasteriser fills the *entire* row with
            // the first segment's `bg` before painting text, so an empty-text
            // segment reproduces a solid fill exactly (the #607 trick).
            if units.clear_terminal_bg {
                let bg_bar = quadraui::StatusBar {
                    id: quadraui::WidgetId::new("terminal:bg"),
                    left_segments: vec![quadraui::StatusBarSegment {
                        text: String::new(),
                        fg: theme.status_fg,
                        bg: theme.terminal_bg,
                        bold: false,
                        action_id: None,
                    }],
                    right_segments: vec![],
                };
                for i in 0..visible_rows {
                    let row =
                        quadraui::Rect::new(rect.x, content_y + i as f32 * lh, rect.width, lh);
                    let _ = b.draw_status_bar_interactive(
                        row,
                        &bg_bar,
                        &quadraui::InteractionState::new(),
                    );
                }
            }

            let td = build_terminal_draw_data(
                term,
                body,
                cw,
                lh,
                visible_rows,
                units.terminal_scrollbar,
            );
            engine.terminal_split_layout.replace(td.split);
            if let Some(split) = &td.split {
                // #635 (Stage 6b): `Backend::draw_terminal_divider`
                // (JDonaghy/quadraui#533) closed the gap that used to keep the
                // trait-only signature from painting a split at all.
                let left = td.left.as_ref().unwrap();
                let right = td.right.as_ref().unwrap();
                b.draw_terminal(split.left, left);
                b.draw_terminal(split.right, right);
                // Width stays a literal `1.0` in *both* unit systems, as both
                // call sites had it: one hairline pixel on GTK, one cell on
                // TUI. Scaling it by `cw` would fatten GTK's divider to a full
                // character width.
                b.draw_terminal_divider(quadraui::Rect::new(
                    split.divider_x,
                    content_y,
                    1.0,
                    content_h,
                ));
            } else if let Some(ref single) = td.single {
                b.draw_terminal(body, single);
            }

            let scrollbar = units.terminal_scrollbar.and_then(|gutter| {
                let g = terminal_scrollbar_geometry(term, visible_rows)?;
                let sb_w = gutter as f32;
                let sb_x = rect.x + rect.width - sb_w;
                // `terminal_scrollbar_geometry` reports fractions in `f64`;
                // every quadraui rect is `f32`.
                let thumb_t = g.thumb_top_frac as f32 * content_h;
                let thumb_h = (g.thumb_height_frac as f32 * content_h).max(4.0);
                Some(quadraui::SurfaceScrollbar {
                    axis: quadraui::ScrollAxis::Vertical,
                    track_bounds: quadraui::Rect::new(sb_x, content_y, sb_w, content_h),
                    thumb_bounds: quadraui::Rect::new(
                        sb_x + 1.0,
                        content_y + thumb_t,
                        sb_w - 2.0,
                        thumb_h,
                    ),
                    total_items: g.total_items,
                    visible_items: g.visible_items,
                    scroll_offset: term.scroll_offset,
                    inverted: true,
                })
            });
            engine
                .scroll_surfaces
                .borrow_mut()
                .push(quadraui::ScrollSurface {
                    id: quadraui::WidgetId::new("terminal_scrollback"),
                    bounds: body,
                    scrollbar,
                });
        }
        BottomPanelKind::DebugOutput => {
            if content_h <= 0.0 {
                return;
            }
            // A **fifth** divergence, and the one with a visible symptom: the
            // debug output has no toolbar row of its own, so its body starts
            // one line below the tab strip, not two. TUI had this right
            // (`content_area`, `rect.y + 1`); GTK reused the terminal arm's
            // `content_y` (`rect.y + 2 * lh`) and so left a blank line under
            // the tab strip and clipped one line off the bottom of the output.
            // TUI's geometry is the correct one and is what both compose now.
            let body =
                quadraui::Rect::new(rect.x, rect.y + lh, rect.width, (rect.height - lh).max(0.0));
            let td = debug_output_to_text_display(
                &screen.bottom_tabs.output_lines,
                engine.debug_output_scroll,
                engine.debug_output_auto_scroll,
            );
            let layout = b.text_display_layout(body, &td);
            b.draw_text_display(body, &td);
            let scrollbar =
                layout
                    .scrollbar_bounds
                    .zip(layout.thumb_bounds)
                    .map(|(track, thumb)| quadraui::SurfaceScrollbar {
                        axis: quadraui::ScrollAxis::Vertical,
                        track_bounds: quadraui::Rect::new(
                            body.x + track.x,
                            body.y + track.y,
                            track.width,
                            track.height,
                        ),
                        thumb_bounds: quadraui::Rect::new(
                            body.x + thumb.x,
                            body.y + thumb.y,
                            thumb.width,
                            thumb.height,
                        ),
                        total_items: td.lines.len(),
                        visible_items: layout.visible_lines.len(),
                        scroll_offset: layout.resolved_scroll_offset,
                        inverted: false,
                    });
            engine
                .scroll_surfaces
                .borrow_mut()
                .push(quadraui::ScrollSurface {
                    id: quadraui::WidgetId::new("debug_output"),
                    bounds: body,
                    scrollbar,
                });
        }
    }
}

/// One tab bar as [`paint_tab_bars`] left it.
///
/// `layout` is the geometry the rasteriser *actually resolved while
/// painting*, not a second no-paint measurement of the same bar. That
/// distinction is the #654/#703 desync in structural form: GTK used to paint
/// with `draw_tab_bar_icons` and then re-measure with `tab_bar_layout_icons`,
/// two calls that agree only as long as nobody changes the font, the icon
/// sidecar or the chrome between them. `Backend::draw_tab_bar_icons_layout`
/// already returns the same `TabBarLayout` type, so the paint's own answer
/// is both cheaper and impossible to desync.
///
/// `layout`'s own geometry (`visible_tabs`/`visible_segments`/`hit_regions`)
/// is bar-**relative** (see that type's doc) — callers needing absolute
/// screen coordinates add `rect`'s own origin back in (#1491; see
/// `click::GroupTabBarLayoutMap`).
pub struct PaintedTabBar<'a> {
    pub group_id: GroupId,
    /// The rect the bar was painted into, in the caller's units.
    pub rect: quadraui::Rect,
    pub bar: &'a quadraui::TabBar,
    pub layout: quadraui::TabBarLayout,
}

/// Paint every editor group's tab bar — the [`EditorOp::TabBars`] rung's whole
/// body on both backends.
///
/// `tab_row_h` / `reserved_h` are [`tab_bar_draw_targets`]'s, in the caller's
/// units (GTK: `tab_row_height_px` / `tab_bar_height_px`; TUI: `1.0` and 1-or-2
/// rows). `hovered_close` is the `(group, tab)` whose close glyph the pointer
/// is over, so the rasteriser can tint it; TUI passes `None` (it has no
/// close-glyph hover state).
///
/// Returns one [`PaintedTabBar`] per bar, in paint order. Both backends cache
/// `rect`/`layout` per group (`click::GroupTabBarLayoutMap`) for hit-testing
/// and the tab-drop/visible-column engine feedback that used to read
/// `TabBarHits`; GTK also re-pushes `rect`/`bar` into the separate
/// `ScreenLayout` it builds a `FrameHitMap` from.
pub fn paint_tab_bars<'a>(
    backend: &mut dyn quadraui::Backend,
    engine: &Engine,
    screen: &'a ScreenLayout,
    tab_row_h: f64,
    reserved_h: f64,
    hovered_close: Option<(GroupId, usize)>,
) -> Vec<PaintedTabBar<'a>> {
    tab_bar_draw_targets(engine, screen, tab_row_h, reserved_h)
        .into_iter()
        .map(|target| {
            let hover = hovered_close.and_then(|(gid, i)| (gid == target.group_id).then_some(i));
            // #703: `draw_tab_bar_icons_layout` with an empty sidecar is
            // byte-identical to `draw_tab_bar_layout` (quadraui's
            // `draw_tab_bar_layout` literally forwards to it with `&[]`), so
            // the Nerd-Fonts-off path keeps today's geometry exactly.
            let layout =
                backend.draw_tab_bar_icons_layout(target.rect, target.bar, target.icons, hover);
            PaintedTabBar {
                group_id: target.group_id,
                rect: target.rect,
                bar: target.bar,
                layout,
            }
        })
        .collect()
}

/// Paint every group's breadcrumb bar and stash each bar's draw-time layout —
/// the [`EditorOp::Breadcrumbs`] rung's whole body on both backends.
///
/// The stashed `StatusBarLayout` is what `resolve_breadcrumb_click` hit-tests
/// against later, and it is the value `draw_status_bar` *returned from the
/// paint*, not a separate `status_bar_layout` re-measure of the same bar —
/// same reasoning as [`PaintedTabBar::hits`]. GTK previously did make that
/// second call; on a headless `GtkDriver` frame it can resolve differently
/// from the paint, which is the class of measure/paint desync #654 was.
pub fn paint_breadcrumb_bars(
    backend: &mut dyn quadraui::Backend,
    screen: &ScreenLayout,
    terminal_maximized: bool,
) {
    for t in breadcrumb_draw_targets(screen, terminal_maximized) {
        let layout =
            backend.draw_status_bar_interactive(t.rect, t.bar, &quadraui::InteractionState::new());
        *t.draw_layout.borrow_mut() = Some(layout);
    }
}

/// Paint a run of divider lines through quadraui's `Split` primitive.
///
/// Generic over [`DividerGeometry`] so the same call serves both
/// `ScreenLayout::window_dividers` (the `:split`/`:vsplit` boundaries inside a
/// group, part of the [`EditorOp::Windows`] rung) and
/// `ScreenLayout::group_dividers` (the [`EditorOp::GroupDividers`] rung) —
/// `divider_to_split` already accepts either.
///
/// This is GTK's rasterisation of both rungs. TUI paints its dividers cell by
/// cell instead (`render_impl::render_group_dividers`), because it carries the
/// #481 guard that suppresses a divider column immediately beside a
/// neighbouring window's scrollbar — a coalescence problem that exists only in
/// a character grid. Per [`FrameMetrics`]'s note, rect math and rasterisation
/// stay per backend by design; what this slice shares is *which* dividers are
/// painted and *when*, which is what had actually drifted (GTK painted the
/// group set not at all).
pub fn draw_dividers_as_splits<D: DividerGeometry>(
    backend: &mut dyn quadraui::Backend,
    dividers: &[D],
    id_for: impl Fn(&D) -> quadraui::WidgetId,
) {
    for div in dividers {
        let (split, rect) = divider_to_split(div, id_for(div));
        backend.draw_split(rect, &split);
    }
}

/// Which of `screen.group_dividers` [`EditorOp::GroupDividers`] actually
/// paints a line for (#1586).
///
/// A **stacked** (`SplitDirection::Horizontal` in vimcode's own naming —
/// top/bottom) group divider's `position` sits exactly where
/// `GroupLayout::calculate_group_rects` starts reserving `tab_bar_height`
/// for the *lower* group's own tab row (`GroupLayout::dividers`/
/// `calculate_group_rects` share one `quadraui::SplitTree::layout` pass, so
/// the two can never disagree about where that boundary is). Painting a real
/// divider line there — any thickness a backend chooses, TUI's 1-cell-thick
/// renderer or a GTK/macOS/Win rasteriser's own independently-derived pixel
/// width — necessarily overwrites part of that row: TUI's whole 1-cell-tall
/// bar (#1586's reported symptom: "no tab row at all, just a divider line,
/// then breadcrumbs"), or a GTK/macOS line through the tab labels.
///
/// The fix is not to reserve extra space for the divider (the "Wanted"
/// option this issue's body rejected): a backend's real divider-line
/// thickness is picked entirely inside that backend's own `Split::layout`
/// (e.g. quadraui's `pixel::DIVIDER` on GTK/macOS/Win), independent of
/// whatever thickness vimcode's own core layer might reserve — so a fixed
/// "reserve N logical units" number picked in `core::window` can never be
/// guaranteed to cover what each backend actually paints. Instead, a
/// stacked group divider is simply never painted: the lower group's own tab
/// row is *already* a full-width, differently-styled band immediately below
/// the boundary, so it already reads as the separator with nothing extra —
/// exactly the affordance TUI's own hit-test tolerance already assumed
/// (`DividerMetrics::group_horizontal`'s `(0.0, tab_bar_rows)` band reaches
/// across the whole tab-bar block precisely because "there is no separate
/// glyph to aim at" was already the intended design for this direction, see
/// `DividerState::on_tab_bar`'s doc).
///
/// Side-by-side (`Vertical`) group dividers are untouched — they run
/// *alongside* a tab row, not *through* one, so nothing here changes for
/// them (#1586's explicit "keep the side-by-side case exactly as it is
/// today").
///
/// `screen.group_dividers` itself is **not** filtered — only this paint-time
/// view is. Hit-testing/drag (`route_divider_grab`/`apply_divider_drag`/
/// `divider_ratio_from_pos`) still sees every divider, stacked or not, and
/// resizing a stacked split by dragging its (now invisible, tab-row-shaped)
/// boundary is unaffected.
///
/// ## #1586's macOS-GUI symptoms — which of them this fixes
///
/// The original report described *three* things wrong with the bottom
/// group's tab row on macOS/GTK, not just one. This filter's single root
/// cause — `draw_split` filling `theme.separator` as one *uniform* rect the
/// full width of the row (`quadraui::primitives::split::
/// native_surface_paint::paint`) — fully accounts for the first two:
///
/// - *"a horizontal line runs through the tab labels"* — the fill, wherever
///   it lands over a low-contrast (inactive) tab's own background.
/// - *"a dark block covers the first (active) tab"* — the exact same fill,
///   at the exact same y, reads as a solid block rather than a thin line
///   specifically where it happens to land over the *active* tab's own
///   (higher-contrast, highlighted) background — no second mechanism
///   needed, confirmed by reading `native_surface_paint::paint`: it issues
///   one `surface_fill_rect` call over `layout.divider_bounds`, not two, and
///   nothing there special-cases a tab's active state.
///
/// It does **not** explain the third: *"the top-left group's first tab...
/// looks double-painted, with its label drawn over itself"* — an unrelated
/// group, not adjacent to the stacked boundary at all. Traced and ruled
/// out as sharing this cause: `divider_to_split`'s emitted `Rect` is
/// confined to its own divider's `axis_start`/`cross_start` box (see its
/// own tests, `divider_to_split_horizontal_maps_direction_ratio_and_bounds`
/// et al. — the rect never reaches `y = 0`), so nothing this filter touches
/// can reach the top-left group's own tab row under any input. Left
/// unfixed and unexplained by this change; needs its own macOS-side
/// reproduction (a screenshot/screen-recording artifact is also plausible
/// and hasn't been ruled out) before a cause can be assigned.
pub fn painted_group_dividers(dividers: &[GroupDivider]) -> Vec<GroupDivider> {
    dividers
        .iter()
        .filter(|d| d.direction == SplitDirection::Vertical)
        .cloned()
        .collect()
}

/// Paint the tab-drag drop feedback — the [`EditorOp::TabDragOverlay`] rung's
/// shared body.
///
/// Returns the resolved geometry so a caller can decorate it further: TUI
/// paints the dragged buffer's name at `ghost_position` afterwards (a
/// `draw_status_bar` row — see `render_impl::render_tab_drag_overlay`), where
/// GTK leaves the ghost to quadraui's own rasteriser. `None` means the drop
/// zone resolved to nothing and no overlay was painted.
///
/// `bar_thickness` / `ghost_offset` are the insertion bar's width and the
/// ghost's horizontal nudge in the caller's units (GTK: `2.0` px and one line
/// height; TUI: `1.0` and `2.0` cells).
pub fn paint_tab_drop_overlay(
    backend: &mut dyn quadraui::Backend,
    ctx: &TabDropCtx,
    cursor: (f32, f32),
    bar_thickness: f32,
    ghost_offset: f32,
) -> Option<quadraui::DropOverlay> {
    let overlay = tab_drop_overlay(ctx, cursor.0, cursor.1, bar_thickness, ghost_offset)?;
    backend.draw_drop_overlay(&overlay);
    Some(overlay)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ─── Frame composition (#735 slices 1-6, folded by #766) ──────────────

    /// Pins the canonical z-order itself. Not a tautology: this is the *only*
    /// place the order is written down now, so the assertion is what a reviewer
    /// reads to see it, and reordering the constant to "fix" a backend fails
    /// here first with a diff that names both rungs.
    #[test]
    fn frame_z_order_is_the_single_shared_ladder() {
        assert_eq!(
            FRAME_Z_ORDER,
            [
                FrameOp::MenuRow,
                FrameOp::SidebarPanel,
                FrameOp::Wildmenu,
                FrameOp::StatusBar,
                FrameOp::CommandLine,
                FrameOp::FolderPicker,
                FrameOp::MenuDropdown,
                FrameOp::CommandCenter,
                FrameOp::FindReplace,
                FrameOp::UnifiedPicker,
                FrameOp::TabSwitcher,
                FrameOp::ContextMenu,
                FrameOp::ChangeReview,
                FrameOp::Dialog,
                FrameOp::ToastStack,
            ],
            "the shared frame z-order changed — both backends walk this array, \
             so a reorder here moves both. Confirm the new order against \
             `route_modal_overlay_click`'s arbitration before accepting."
        );

        let pos = |op: FrameOp| FRAME_Z_ORDER.iter().position(|c| *c == op).unwrap();

        // #766: the whole chrome half precedes the whole overlay half. This is
        // the invariant the fold has to preserve — before it, the two halves
        // were separate constants and "chrome below overlays" was only a
        // sentence in a doc comment.
        let first_overlay = FRAME_Z_ORDER
            .iter()
            .position(|op| op.is_overlay())
            .expect("the overlay tail exists");
        assert!(
            FRAME_Z_ORDER[..first_overlay]
                .iter()
                .all(|op| !op.is_overlay())
                && FRAME_Z_ORDER[first_overlay..]
                    .iter()
                    .all(|op| op.is_overlay()),
            "the overlay rungs must be a contiguous *tail* of FRAME_Z_ORDER — \
             a chrome rung composed after an overlay rung would be painted on \
             top of a modal"
        );

        // The two placements #735 had to *pick* between, called out so a future
        // edit that quietly reverts either one fails with the reason attached.
        assert!(
            pos(FrameOp::Dialog) > pos(FrameOp::ContextMenu),
            "a modal dialog takes every event ahead of the context menu \
             (`route_modal_key` / `route_modal_overlay_click`), so it must also \
             paint above it — GTK had this inverted before #735"
        );
        assert!(
            pos(FrameOp::Dialog) > pos(FrameOp::MenuDropdown),
            "title-bar chrome is chrome: a modal covers it — TUI had this \
             inverted before #735"
        );
        assert!(
            pos(FrameOp::CommandCenter) > pos(FrameOp::MenuDropdown),
            "`MenuSystem::render` repaints `draw_menu_bar` across the whole \
             band, erasing a command centre drawn first (#676 on GTK, #712 on \
             TUI)"
        );
        assert!(
            pos(FrameOp::MenuDropdown) > pos(FrameOp::MenuRow),
            "the dropdown arm paints the band the measure rung published, so it \
             must come after it (#712)"
        );
        assert!(
            pos(FrameOp::ToastStack) == FRAME_Z_ORDER.len() - 1,
            "toasts sit on top of everything, and are the first rung \
             `route_modal_overlay_click` arbitrates"
        );
    }

    /// #751 acceptance: the order the *mouse* router arbitrates in must be the
    /// exact inverse of the order the *paint* sequence composes in.
    ///
    /// They are the same fact stated twice — whatever paints on top is what the
    /// user is aiming at — and #587/#592 are what happens when the two
    /// disagree. Before #751 the two halves genuinely did: the paint order
    /// put the context menu above the picker and find/replace, while TUI's
    /// `handle_mouse` arbitrated the picker and find/replace ~1,100 lines
    /// *before* it. Nothing could catch that because the mouse ladder was
    /// straight-line control flow, not a value; `MOUSE_ARBITRATION_ORDER`
    /// makes it one.
    #[test]
    fn mouse_arbitration_is_the_inverse_of_the_paint_z_order() {
        let expected: Vec<FrameOp> = FRAME_Z_ORDER
            .iter()
            .rev()
            .copied()
            .filter(|op| MOUSE_ARBITRATION_ORDER.contains(op))
            .collect();
        assert_eq!(
            MOUSE_ARBITRATION_ORDER.to_vec(),
            expected,
            "`route_modal_overlay_click` arbitrates in a different order than \
             `FRAME_Z_ORDER` paints in — an overlay would be painted on top \
             of the one that actually owns the click underneath it"
        );

        // The rungs that carry *mouse* input must all be arbitrated: a painted
        // rung missing from the router falls through to the editor beneath it,
        // which is the tab-switcher bug #733 found. `MenuDropdown` /
        // `CommandCenter` are the deliberate exceptions — `MenuSystem::handle`
        // owns the title-bar band's own events before this router runs.
        for op in FRAME_Z_ORDER.into_iter().filter(|op| op.is_overlay()) {
            let arbitrated = MOUSE_ARBITRATION_ORDER.contains(&op);
            // `MenuDropdown` / `CommandCenter`: `MenuSystem::handle` owns the
            // title-bar band's own events before this router runs.
            // `FolderPicker`: shared on both backends since #815, but not
            // folded into this ladder — see `route_folder_picker_click`'s
            // doc comment for why it is checked directly instead.
            // `ChangeReview` (#955): same policy — see
            // `route_change_review_click`'s doc comment.
            let routed_elsewhere = matches!(
                op,
                FrameOp::MenuDropdown
                    | FrameOp::CommandCenter
                    | FrameOp::FolderPicker
                    | FrameOp::ChangeReview
            );
            assert_eq!(
                arbitrated, !routed_elsewhere,
                "{op:?} is painted but not arbitrated (or vice versa) — add it \
                 to `MOUSE_ARBITRATION_ORDER` and to \
                 `route_modal_overlay_click`, or state why it is chrome"
            );
        }

        // Chrome rungs are never arbitrated here: they route through
        // `route_chrome_click` / `MenuSystem::handle`, not the modal ladder.
        for op in FRAME_Z_ORDER.into_iter().filter(|op| !op.is_overlay()) {
            assert!(
                !MOUSE_ARBITRATION_ORDER.contains(&op),
                "{op:?} is chrome — it must not be in the modal-overlay ladder"
            );
        }
    }

    #[test]
    fn compose_frame_filters_to_live_rungs_in_canonical_order() {
        let none = FramePresence::default();
        assert!(compose_frame(&none).is_empty());

        // Deliberately set in an order that is *not* the paint order, to prove
        // the output order comes from `FRAME_Z_ORDER` and not from the
        // struct's field order or the caller's.
        let mut p = FramePresence::default();
        p.toast_stack = true;
        p.context_menu = true;
        p.menu_dropdown = true;
        p.dialog = true;
        p.command_line = true;
        p.menu_row = true;
        assert_eq!(
            compose_frame(&p),
            vec![
                FrameOp::MenuRow,
                FrameOp::CommandLine,
                FrameOp::MenuDropdown,
                FrameOp::ContextMenu,
                FrameOp::Dialog,
                FrameOp::ToastStack,
            ]
        );

        let all = FramePresence {
            menu_row: true,
            sidebar_panel: true,
            wildmenu: true,
            status_bar: true,
            command_line: true,
            folder_picker: true,
            menu_dropdown: true,
            command_center: true,
            find_replace: true,
            unified_picker: true,
            tab_switcher: true,
            context_menu: true,
            change_review: true,
            dialog: true,
            toast_stack: true,
        };
        assert_eq!(compose_frame(&all), FRAME_Z_ORDER.to_vec());
    }

    #[test]
    fn check_frame_order_accepts_any_canonical_subsequence() {
        assert!(check_frame_order(&[]).is_ok());
        assert!(check_frame_order(&FRAME_Z_ORDER).is_ok());
        assert!(check_frame_order(&[FrameOp::ContextMenu, FrameOp::Dialog]).is_ok());
        assert!(check_frame_order(&[
            FrameOp::MenuRow,
            FrameOp::MenuDropdown,
            FrameOp::UnifiedPicker,
            FrameOp::ToastStack
        ])
        .is_ok());
    }

    /// The exact pre-#735 GTK sequence, which is what this check exists to
    /// reject. Keeps the regression legible: if someone hoists a rung back out
    /// of the shared walk, this is the shape it produces.
    #[test]
    fn check_frame_order_rejects_the_gtk_dialog_context_menu_inversion() {
        let err = check_frame_order(&[FrameOp::Dialog, FrameOp::ContextMenu])
            .expect_err("dialog-then-context-menu runs backwards against FRAME_Z_ORDER");
        assert!(err.contains("ContextMenu"), "{err}");
        assert!(err.contains("out of order"), "{err}");

        // And the pre-#735 TUI one: title-bar chrome after the modal stack.
        assert!(check_frame_order(&[
            FrameOp::Dialog,
            FrameOp::MenuDropdown,
            FrameOp::CommandCenter,
            FrameOp::ToastStack,
        ])
        .is_err());

        // #766: chrome hoisted below the overlay tail is now the *same* check —
        // before the fold these were two independent orders and this sequence
        // was accepted by both of them.
        assert!(check_frame_order(&[FrameOp::Dialog, FrameOp::CommandLine]).is_err());

        // A repeated rung is a double-paint, not a valid sequence.
        assert!(check_frame_order(&[FrameOp::Dialog, FrameOp::Dialog]).is_err());
    }

    /// A `ScreenLayout` for an untouched engine: no wildmenu, per-window status
    /// lines on (so no global status bar), menu bar hidden.
    fn bare_screen_layout() -> ScreenLayout {
        let engine = crate::core::Engine::new();
        let theme = Theme::from_name(&engine.settings.colorscheme);
        build_screen_layout(
            &engine,
            &theme,
            &[],
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        )
    }

    /// An `AppShellLayout` with every optional band absent — the shape a shell
    /// that reserved nothing hands back.
    fn bare_shell_layout() -> quadraui::AppShellLayout {
        quadraui::AppShellLayout {
            window_bounds: quadraui::Rect::new(0.0, 0.0, 1400.0, 900.0),
            title_bar_bounds: None,
            activity_bar_bounds: quadraui::Rect::default(),
            sidebar_header_bounds: None,
            sidebar_content_bounds: None,
            divider_bounds: None,
            main_content_bounds: quadraui::Rect::new(0.0, 0.0, 1400.0, 900.0),
            bottom_panel_bounds: None,
            command_line_bounds: None,
            status_bar_bounds: None,
        }
    }

    /// `FramePresence::from_screen`'s gates, in both unit systems, on the
    /// degenerate shapes the driver-level tests cannot construct.
    ///
    /// The two divergences #763 closes are here as facts: before it, GTK's
    /// sidebar rung had **no** minimum-size check (it painted a whole panel
    /// into a zero-height rect) and its menu-row rung checked only
    /// `height > 0.0` with no width check at all. TUI checked both, in cells.
    #[test]
    fn compose_frame_gates_degenerate_bands_in_the_callers_units() {
        let compose = |screen: &ScreenLayout, layout: &quadraui::AppShellLayout, m| {
            compose_frame(&FramePresence::from_screen(screen, layout, m))
        };
        let mut screen = bare_screen_layout();
        screen.menu_bar_visible = true;

        let full_band = quadraui::Rect::new(0.0, 0.0, 1400.0, 17.0);
        let mut layout = bare_shell_layout();
        layout.title_bar_bounds = Some(full_band);
        layout.sidebar_content_bounds = Some(quadraui::Rect::new(0.0, 17.0, 240.0, 800.0));

        let px = FrameMetrics::px(17.0, 8.0);
        assert_eq!(
            compose(&screen, &layout, px),
            vec![
                FrameOp::MenuRow,
                FrameOp::SidebarPanel,
                FrameOp::CommandLine,
                FrameOp::MenuDropdown,
                FrameOp::CommandCenter,
            ],
            "no wildmenu and no global status bar in a default ScreenLayout; \
             the title bar drives all three of its rungs"
        );

        // A band one pixel shorter than a text line is not a band. GTK's own
        // `height > 0.0` gate accepted this. #766: this now also gates the
        // *dropdown*, whose GTK arm checked nothing but `menu_bar_visible`.
        layout.title_bar_bounds = Some(quadraui::Rect::new(0.0, 0.0, 1400.0, 16.0));
        for op in [
            FrameOp::MenuRow,
            FrameOp::MenuDropdown,
            FrameOp::CommandCenter,
        ] {
            assert!(!compose(&screen, &layout, px).contains(&op), "{op:?}");
        }

        // ... and neither is a zero-width one. GTK never checked the width.
        layout.title_bar_bounds = Some(quadraui::Rect::new(0.0, 0.0, 0.0, 17.0));
        assert!(!compose(&screen, &layout, px).contains(&FrameOp::MenuRow));

        layout.title_bar_bounds = Some(full_band);
        screen.menu_bar_visible = false;
        assert!(!compose(&screen, &layout, px).contains(&FrameOp::MenuRow));
        assert!(!compose(&screen, &layout, px).contains(&FrameOp::MenuDropdown));
        screen.menu_bar_visible = true;

        // A collapsed sidebar content rect: `Some`, but nothing fits in it.
        // This is the shape GTK painted a whole panel into before #763.
        layout.sidebar_content_bounds = Some(quadraui::Rect::new(0.0, 17.0, 240.0, 0.0));
        assert!(!compose(&screen, &layout, px).contains(&FrameOp::SidebarPanel));
        layout.sidebar_content_bounds = None;
        assert!(!compose(&screen, &layout, px).contains(&FrameOp::SidebarPanel));

        // The same rects are all fine in *cells*, which is the whole point of
        // `FrameMetrics` being a unit rather than a geometry: a 240x0 rect is
        // still degenerate, but the 1400x16 title bar is 16 rows tall.
        layout.title_bar_bounds = Some(quadraui::Rect::new(0.0, 0.0, 1400.0, 16.0));
        layout.sidebar_content_bounds = Some(quadraui::Rect::new(0.0, 17.0, 240.0, 800.0));
        assert_eq!(
            compose(&screen, &layout, FrameMetrics::CELL),
            vec![
                FrameOp::MenuRow,
                FrameOp::SidebarPanel,
                FrameOp::CommandLine,
                FrameOp::MenuDropdown,
                FrameOp::CommandCenter,
            ]
        );
    }

    /// #939: `FramePresence::from_screen` must split the Command Center's
    /// liveness gate from the drawn menu row's. Before this fix, a single
    /// `title_bar` bool (`screen.menu_bar_visible && title_bar_band_live`)
    /// drove `menu_row`, `menu_dropdown` **and** `command_center` — so a
    /// native-menu backend (macOS's `MacBackend`) setting
    /// `menu_bar_visible = false` to suppress the redundant in-window
    /// `File Edit View` row (#901) killed the Command Center along with it,
    /// and the omnibar never painted on macOS at all.
    ///
    /// `menu_row` / `menu_dropdown` must stay coupled to `menu_bar_visible`
    /// — no drawn menu means no drawn dropdown — but `command_center` must
    /// depend only on the title-bar band existing.
    ///
    /// RED-verified against this fix: with `command_center` reverted to
    /// `title_bar` (the pre-#939 expression) instead of
    /// `title_bar_band_live`, the `assert!(presence.command_center, ...)`
    /// below fails once `menu_bar_visible` is `false`.
    #[test]
    fn command_center_liveness_is_split_from_menu_bar_visible() {
        let mut screen = bare_screen_layout();
        let full_band = quadraui::Rect::new(0.0, 0.0, 1400.0, 17.0);
        let mut layout = bare_shell_layout();
        layout.title_bar_bounds = Some(full_band);
        let px = FrameMetrics::px(17.0, 8.0);

        screen.menu_bar_visible = true;
        let presence = FramePresence::from_screen(&screen, &layout, px);
        assert!(
            presence.menu_row && presence.menu_dropdown && presence.command_center,
            "with the band live and menu_bar_visible true, all three \
             title-bar rungs must be live: {presence:?}"
        );

        screen.menu_bar_visible = false;
        let presence = FramePresence::from_screen(&screen, &layout, px);
        assert!(
            !presence.menu_row,
            "menu_row must stay coupled to menu_bar_visible: {presence:?}"
        );
        assert!(
            !presence.menu_dropdown,
            "menu_dropdown must stay coupled to menu_bar_visible -- no drawn \
             menu means no drawn dropdown: {presence:?}"
        );
        assert!(
            presence.command_center,
            "command_center must NOT be coupled to menu_bar_visible -- a \
             native-menu backend suppresses the drawn row but the Command \
             Center still belongs in the (still-live) band: {presence:?}"
        );

        // And the band simply not existing still kills it, same as before --
        // the split is about `menu_bar_visible`, not about dropping the
        // band-liveness check entirely.
        layout.title_bar_bounds = None;
        let presence = FramePresence::from_screen(&screen, &layout, px);
        assert!(
            !presence.command_center,
            "command_center must still require the title-bar band to exist: {presence:?}"
        );
    }

    /// The command line is composed on every frame — the row is always
    /// reserved, empty or not — and the wildmenu / status rungs follow their
    /// `ScreenLayout` fields.
    #[test]
    fn compose_frame_always_composes_the_command_line() {
        let screen = bare_screen_layout();
        let layout = bare_shell_layout();
        assert_eq!(
            compose_frame(&FramePresence::from_screen(
                &screen,
                &layout,
                FrameMetrics::CELL
            )),
            vec![FrameOp::CommandLine],
            "nothing else is live in an empty layout, but the command line row \
             is always painted"
        );
        assert_eq!(
            chrome_band_fixture(true),
            FRAME_Z_ORDER
                .iter()
                .copied()
                .filter(|op| !op.is_overlay())
                .collect::<Vec<_>>()
        );
    }

    /// The pre-#763 GTK chrome order, which is what this check exists to
    /// reject. Keeps the regression legible: if someone hoists a rung back out
    /// of the shared walk, this is the shape it produces.
    #[test]
    fn check_frame_order_rejects_the_gtk_status_wildmenu_inversion() {
        assert_eq!(
            check_frame_order(&[FrameOp::MenuRow, FrameOp::CommandLine]),
            Ok(()),
            "any canonical subsequence is fine — the check is about order, not \
             about which rungs were live"
        );

        let err = check_frame_order(&[FrameOp::StatusBar, FrameOp::Wildmenu])
            .expect_err("status-then-wildmenu runs backwards against FRAME_Z_ORDER");
        assert!(err.contains("Wildmenu"), "{err}");
        assert!(err.contains("out of order"), "{err}");

        // The pre-#763 GTK tail: the sidebar body composed after the command
        // line.
        assert!(check_frame_order(&[FrameOp::CommandLine, FrameOp::SidebarPanel]).is_err());

        // A repeated rung is a double-composition, not a valid sequence.
        assert!(check_frame_order(&[FrameOp::CommandLine, FrameOp::CommandLine]).is_err());
    }

    pub(crate) fn empty_sc_data() -> SourceControlData {
        SourceControlData {
            branch: "main".into(),
            ahead: 0,
            behind: 0,
            merge: vec![],
            staged: vec![],
            unstaged: vec![],
            worktrees: vec![],
            log: vec![],
            sections_expanded: [true; crate::core::engine::SC_SECTION_COUNT],
            selected: 0,
            has_focus: false,
            commit_message: String::new(),
            commit_cursor: 0,
            commit_input_active: false,
            button_focused: None,
            button_hovered: None,
            branch_picker: None,
            help_open: false,
            sc_sections_start_y: None,
        }
    }

    #[test]
    fn test_sc_button_toolbar_ids_and_shape() {
        use crate::core::engine::SC_BUTTON_IDS;
        use quadraui::ToolbarButton;

        let sc = empty_sc_data();
        let bar = sc_button_toolbar(&sc);
        assert_eq!(bar.buttons.len(), 4);

        // Ids appear in button-index order and match the shared constant.
        for (i, btn) in bar.buttons.iter().enumerate() {
            match btn {
                ToolbarButton::Action { id, label, .. } => {
                    assert_eq!(id.as_str(), SC_BUTTON_IDS[i]);
                    // Commit carries a label; Push/Pull/Sync are icon-only.
                    if i == 0 {
                        assert_eq!(label, "Commit");
                    } else {
                        assert!(label.is_empty());
                    }
                }
                other => panic!("expected Action, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_sc_button_toolbar_commit_enabled_tracks_message() {
        use quadraui::ToolbarButton;

        let commit_enabled = |sc: &SourceControlData| match &sc_button_toolbar(sc).buttons[0] {
            ToolbarButton::Action { enabled, .. } => *enabled,
            _ => panic!("commit button missing"),
        };

        let mut sc = empty_sc_data();
        assert!(!commit_enabled(&sc), "empty message → Commit disabled");

        sc.commit_message = "   ".into();
        assert!(!commit_enabled(&sc), "whitespace-only message → disabled");

        sc.commit_message = "feat: x".into();
        assert!(commit_enabled(&sc), "non-empty message → Commit enabled");
    }

    #[test]
    fn test_sc_button_id_index_round_trip() {
        use crate::core::engine::Engine;
        for idx in 0..4 {
            let id = Engine::sc_button_id(idx).expect("id for valid index");
            assert_eq!(Engine::sc_button_index(&id), Some(idx));
        }
        assert!(Engine::sc_button_id(4).is_none());
    }

    #[test]
    fn test_sc_button_toolbar_hit_test_resolves_index() {
        use crate::core::engine::Engine;
        use quadraui::ToolbarHit;

        // Lay the toolbar out the way the TUI backend does, then prove a
        // click inside each button's bounds maps back to its index.
        let sc = empty_sc_data();
        let bar = sc_button_toolbar(&sc);
        let area = ratatui::layout::Rect::new(0, 5, 60, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);

        // Push is button index 1 and is enabled (icon-only).
        let push = &layout.visible_items[1];
        let hit = layout.hit_test(push.bounds.x + 0.5, push.bounds.y);
        match hit {
            ToolbarHit::Button(id) => assert_eq!(Engine::sc_button_index(&id), Some(1)),
            other => panic!("expected Button hit, got {other:?}"),
        }
    }

    #[test]
    fn test_sc_button_toolbar_disabled_commit_not_clickable() {
        use quadraui::ToolbarHit;

        // Empty message → Commit disabled → its slot hit-tests as Empty.
        let sc = empty_sc_data();
        let bar = sc_button_toolbar(&sc);
        let area = ratatui::layout::Rect::new(0, 0, 60, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);
        let commit = &layout.visible_items[0];
        assert!(!commit.clickable, "disabled Commit must not be clickable");
        assert_eq!(
            layout.hit_test(commit.bounds.x + 0.5, commit.bounds.y),
            ToolbarHit::Empty
        );
    }

    // ── #1207: 'linebreak' wrap-point selection ─────────────────────────
    //
    // RED against unfixed `develop`: before #1207,
    // `compute_word_wrap_segments` had no `linebreak` parameter at all and
    // *always* sought a word boundary — i.e. it always did what
    // `linebreak_off_hard_cuts_mid_word` asserts must NOT happen, so that
    // test would have failed (the segment boundary would have fallen back
    // to the space, not the viewport column).

    #[test]
    fn linebreak_off_hard_cuts_mid_word() {
        // "helloworld" is 10 chars with no word boundary anywhere; a 5-col
        // viewport with linebreak off must cut mid-word at column 5.
        let segs = compute_word_wrap_segments("helloworld", 5, false);
        assert_eq!(segs, vec![(0, 5), (5, 10)]);
    }

    #[test]
    fn linebreak_on_breaks_at_word_boundary() {
        // "hello world" (11 chars) with an 8-col viewport: a hard cut at
        // column 8 would split "wor|ld", but 'linebreak' must back up to
        // the space at index 5 instead.
        let segs = compute_word_wrap_segments("hello world", 8, true);
        assert_eq!(segs, vec![(0, 6), (6, 11)]);
    }

    #[test]
    fn linebreak_on_falls_back_to_hard_cut_with_no_boundary() {
        // No word boundary anywhere in "helloworld" — 'linebreak' can't do
        // anything but the same hard cut as linebreak-off.
        let segs = compute_word_wrap_segments("helloworld", 5, true);
        assert_eq!(segs, vec![(0, 5), (5, 10)]);
    }

    #[test]
    fn linebreak_is_a_no_op_when_the_line_fits() {
        assert_eq!(compute_word_wrap_segments("short", 80, false), vec![(0, 5)]);
        assert_eq!(compute_word_wrap_segments("short", 80, true), vec![(0, 5)]);
    }
}
