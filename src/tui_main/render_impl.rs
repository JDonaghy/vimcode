use super::*;

// ─── Bottom band row accounting ────────────────────────────────────────────────

/// The height, in rows, of each vimcode-specific chrome band that stacks
/// below the editor column: quickfix/location-list, the terminal/debug
/// bottom panel, the debug toolbar, the wildmenu, and the status line(s).
/// `AppShellLayout` has no concept of any of these — see
/// [`bottom_chrome_rects_for_shell_content`]'s doc comment for why they are
/// carved out by hand.
///
/// Single source of truth for this arithmetic (#1164). Before this type
/// existed, `build_screen_for_tui`, `build_screen_for_shell_content`,
/// `bottom_chrome_rects_for_shell_content` and `TuiShellApp::tick` each
/// hand-summed the same five gates independently — `tick` in particular ran
/// ahead of any paint (no `ScreenLayout` yet to read a rect back from), so
/// it re-derived every gate from `Engine` flags on its own rather than
/// composing bands, which is the exact "second geometry model" #1164
/// flagged: it duplicated `bottom_chrome_rects_for_shell_content`'s
/// per-band heights, drifting from them under nobody's notice (menu-bar
/// height folded into the wrong side of the sum, `window_status_line`
/// ignored in favour of a hardcoded status row) until this convergence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct BottomBandRowHeights {
    pub(super) quickfix: u16,
    pub(super) terminal: u16,
    pub(super) debug_toolbar: u16,
    pub(super) wildmenu: u16,
    pub(super) global_status: u16,
    pub(super) separated_status: u16,
}

impl BottomBandRowHeights {
    /// Total rows this band stack reserves below the editor, **including**
    /// the always-present 1-row command line but **excluding** the
    /// menu-bar row — callers that see the raw terminal viewport (`tick`,
    /// `build_screen_for_tui`) carve that off separately, matching
    /// `bottom_chrome_rects_for_shell_content`'s `area`, which never
    /// carries it in the first place (AppShell already carved its own
    /// title-bar row off before handing out `main_content_bounds`).
    pub(super) fn total(&self) -> u16 {
        1 + self.quickfix
            + self.terminal
            + self.debug_toolbar
            + self.wildmenu
            + self.global_status
            + self.separated_status
    }
}

/// Computes [`BottomBandRowHeights`] from live `Engine` state alone — no
/// `ScreenLayout` required, so this is cheap enough to call every `tick`,
/// not just once per paint.
///
/// `content_height` bounds the terminal panel's "maximize" target
/// (`terminal_target_maximize_rows_tui`): pass the editor-column height the
/// panel will actually be measured against — the AppShell-carved
/// `area.height` from a paint call, or the menu-row-adjusted raw viewport
/// height from `tick`.
pub(super) fn bottom_band_row_heights(
    engine: &Engine,
    content_height: u16,
) -> BottomBandRowHeights {
    // `quickfix_panel_rows` also accounts for the active window's open
    // location list (#1155) — the two share one bottom "list rung".
    let quickfix = render::quickfix_panel_rows(engine);
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    let terminal = if bottom_panel_open {
        let target = super::terminal_target_maximize_rows_tui(engine, content_height);
        engine.effective_terminal_panel_rows(target) + 2 // tab bar + header + content
    } else {
        0
    };
    let debug_toolbar = if engine.debug_toolbar_visible { 1 } else { 0 };
    let wildmenu: u16 = if !engine.wildmenu_items.is_empty() {
        1
    } else {
        0
    };
    let per_window_status = engine.settings.window_status_line;
    let global_status = if per_window_status { 0 } else { 1 };
    let separate_status =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open;
    let separated_status = if separate_status { 1 } else { 0 };
    BottomBandRowHeights {
        quickfix,
        terminal,
        debug_toolbar,
        wildmenu,
        global_status,
        separated_status,
    }
}

// ─── Screen layout bridging ───────────────────────────────────────────────────

#[cfg(test)]
pub(super) fn build_screen_for_tui(
    engine: &Engine,
    theme: &Theme,
    area: Rect,
    _sidebar: &TuiSidebar,
    sidebar_width: u16,
) -> render::ScreenLayout {
    // Global bottom rows: status(1) + cmd(1).  The tab bar row is included in
    // content_bounds and handled by calculate_group_window_rects (tab_bar_height=1).
    // Must match draw_frame's vertical layout exactly.
    let menu_height: u16 = if engine.menu_bar_visible { 1 } else { 0 };
    let content_height = area.height.saturating_sub(menu_height);
    let bands = bottom_band_row_heights(engine, content_height);
    let content_rows = content_height.saturating_sub(bands.total()); // cmd(1) + optional status(1) + panels + separated status
    let sv = engine.app_shell.sidebar_visible();
    let sidebar_cols = if sv { sidebar_width + 1 } else { 0 }; // +1 sep
    let ab_width = if engine.settings.autohide_panels && !sv {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let content_cols = area.width.saturating_sub(ab_width + sidebar_cols);
    // #550: window rects are absolute terminal-screen coordinates, matching
    // GTK's convention, rather than relative to the editor content area's own
    // top-left. `editor_area`'s origin here must match the `Layout` split
    // `draw_frame` performs on the same `area` (menu bar row, then activity
    // bar + sidebar columns) — see the mirrored computation there.
    let editor_origin_x = area.x as f64 + ab_width as f64 + sidebar_cols as f64;
    let editor_origin_y = area.y as f64 + menu_height as f64;
    let content_bounds = WindowRect::new(
        editor_origin_x,
        editor_origin_y,
        content_cols as f64,
        content_rows as f64,
    );
    let tui_tab_bar_height = if engine.settings.breadcrumbs && !engine.terminal_maximized {
        2.0
    } else {
        1.0
    };
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(content_bounds, tui_tab_bar_height);
    debug_log!(
        "build_screen: content_rows={} content_cols={} groups={} window_rects={}",
        content_rows,
        content_cols,
        engine.group_layout.leaf_count(),
        window_rects.len()
    );
    for (wid, r) in &window_rects {
        debug_log!(
            "  window {:?}: x={:.1} y={:.1} w={:.1} h={:.1}",
            wid,
            r.x,
            r.y,
            r.width,
            r.height
        );
    }
    let bsl_t0 = std::time::Instant::now();
    // TUI's `quadraui::Backend::scrollbar_reserve()` is always `0.0` (the
    // trait default — TUI has no overlay scrollbar chrome to dodge), so
    // this test-only helper states it directly rather than threading a
    // `&dyn Backend` through just to ask a question with one known answer.
    let result = build_screen_layout(
        engine,
        theme,
        &window_rects,
        1.0,
        1.0,
        true,
        0.0,
        render::TUI_MINIMAP_SIZING,
    );
    let bsl_elapsed = bsl_t0.elapsed();
    if bsl_elapsed.as_millis() > 10 {
        debug_log!(
            "PERF build_screen_layout: {:.1}ms",
            bsl_elapsed.as_secs_f64() * 1000.0
        );
    }
    result
}

/// `TuiShellApp::render_content`-side counterpart to [`build_screen_for_tui`]
/// (#601). `render_content` receives `layout.main_content_bounds` from
/// quadraui's `AppShell::render`, which has *already* painted the activity
/// bar + sidebar chrome and carved their width out of `area` — unlike
/// `build_screen_for_tui`, called from the live `event_loop()` path with the
/// *full* terminal rect, this must not subtract activity-bar/sidebar width a
/// second time (that would double-count it and shrink the editor area).
///
/// Still applies vimcode's own row accounting — quickfix/terminal/
/// debug-toolbar/wildmenu/status rows — since `AppShellLayout` has no
/// concept of any of those; this mirrors `build_screen_for_tui`'s tail (from
/// its `content_bounds` computation onward) so the two paths share the same
/// formula and can't silently drift. `area` is treated directly as the
/// editor column.
///
/// Deliberately does *not* reserve a menu-bar row, unlike
/// [`build_screen_for_tui`]'s otherwise-identical `content_rows` formula
/// (#635, Stage 6b item A): `AppShell::compute_layout` (quadraui,
/// `compose/app_shell.rs`) already carves its title-bar row off the top of
/// the window (`band_y += h; band_h -= h;`) *before* deriving
/// `main_content_bounds`, and `TuiShellApp::handle` keeps that reservation
/// synced to `engine.menu_bar_visible` through
/// `ShellContext::shell_mut().set_title_bar_visible`. So by the time `area`
/// (== `layout.main_content_bounds`) reaches here the row is already gone —
/// subtracting a second, vimcode-local `menu_height` on top of it would
/// consume 2 rows for a 1-row menu bar and push the editor content down one
/// row further than the live `draw_frame` path puts it. The row is painted
/// (menu bar + command centre) by `TuiShellApp::render_content` from
/// `layout.title_bar_bounds`, not from anything computed here. This also
/// keeps `content_rows` consistent with
/// [`bottom_chrome_rects_for_shell_content`]'s `Constraint::Min(0)` editor
/// chunk over the same `area`, which likewise has no menu term.
///
pub(super) fn build_screen_for_shell_content(
    engine: &Engine,
    theme: &Theme,
    area: Rect,
    backend: &dyn quadraui::Backend,
) -> render::ScreenLayout {
    // No `menu_height` term here — `area` already excludes AppShell's own
    // title-bar row. See this function's doc comment.
    let bands = bottom_band_row_heights(engine, area.height);
    let content_rows = area.height.saturating_sub(bands.total());
    let editor_origin_x = area.x as f64;
    let editor_origin_y = area.y as f64;
    let content_bounds = WindowRect::new(
        editor_origin_x,
        editor_origin_y,
        area.width as f64,
        content_rows as f64,
    );
    let tui_tab_bar_height = if engine.settings.breadcrumbs && !engine.terminal_maximized {
        2.0
    } else {
        1.0
    };
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(content_bounds, tui_tab_bar_height);
    // #828/quadraui#776: ask the real backend instead of sniffing
    // `char_width` inside `render.rs` — always `0.0` for TUI today (the
    // `Backend` trait's default, since TUI has no overlay scrollbar chrome
    // to dodge), but this is the one call site whose value should track
    // the trait, not restate the constant.
    //
    // #1097 (residual minimap-off click-cost profiling): this is the
    // *live* per-frame path — `TuiShellApp::render_content` calls this once
    // per redraw, and `handle_mouse_event` returns `Reaction::Redraw`
    // unconditionally for every dispatched mouse event, including every
    // coalesced `MouseMoved` fired while the mouse merely moves over the
    // editor (mouse capture reports motion, not just clicks). The
    // pre-existing `PERF build_screen_layout` hook on `build_screen_for_tui`
    // above only instruments that `#[cfg(test)]`-only helper — it can never
    // fire on a live run, since nothing outside tests calls it (see that
    // function's own gating). This is the timer that actually sees what a
    // live click/hover session costs. Threshold is 1ms, not that helper's
    // 10ms: the residual cost #1097 chases is *many small frames*, not one
    // slow one, and a 10ms floor would hide exactly that shape.
    let bsl_t0 = std::time::Instant::now();
    let result = build_screen_layout(
        engine,
        theme,
        &window_rects,
        1.0,
        1.0,
        true,
        backend.scrollbar_reserve() as f64,
        render::TUI_MINIMAP_SIZING,
    );
    let bsl_elapsed = bsl_t0.elapsed();
    if bsl_elapsed.as_millis() >= 1 {
        debug_log!(
            "PERF build_screen_layout(live): {:.2}ms",
            bsl_elapsed.as_secs_f64() * 1000.0
        );
    }
    result
}

/// Quickfix panel + bottom panel (terminal/debug output) rects for
/// [`super::shell_app::TuiShellApp::render_content`] (#608).
///
/// quadraui's `AppShellLayout` has no concept of quickfix/terminal/
/// debug-toolbar/wildmenu rows — vimcode-specific chrome that `draw_frame`'s
/// own vertical `Layout::split` (its `v_chunks` block) computes today from
/// the *live* editor column. This mirrors that split over `area` (==
/// `layout.main_content_bounds` from `render_content`, already carved of
/// activity-bar/sidebar width by quadraui's `AppShell::render`, exactly the
/// same "editor column" `draw_frame`'s `right_col` is) so the shell-content
/// path's quickfix/bottom-panel rects land at the same coordinates the live
/// path would use, and the space `build_screen_for_shell_content` reserved
/// above (subtracted from `content_rows`) lines up with what's actually
/// carved out here — same reserved-but-unpainted treatment `render_content`
/// already gives the menu-bar row applies to the remaining rows here too.
///
/// #605 (Stage 6 parity sweep): every one of `draw_frame`'s eight vertical
/// chunks is now returned, not just quickfix + bottom panel — the
/// separated-status / debug-toolbar / wildmenu / global-status / command-line
/// rows are all painted by `TuiShellApp::render_content` through
/// `Backend::draw_*` calls, so their rects have real consumers.
///
/// Investigated per this issue's scope note: quadraui's own generic
/// `ShellConfig::with_bottom_panel` / `BottomPanelController`
/// (`shell_adapter.rs`, `compose/app_shell.rs`) model a single resizable
/// drawer, not vimcode's stack of independently-toggleable
/// quickfix/terminal/debug-toolbar/wildmenu rows, and `TuiShellApp`'s own
/// `ShellConfig` (this module's `#[cfg(test)] config()`) doesn't call
/// `with_bottom_panel` — so `layout.bottom_panel_bounds` is always `None`
/// today. Carving a private sub-region out of `main_content_bounds`, matching
/// `draw_frame`, is therefore the only option that doesn't require a much
/// larger `ShellConfig`/layout-model change.
///
pub(super) struct BottomChromeRects {
    pub(super) quickfix: Rect,
    pub(super) bottom_panel: Rect,
    pub(super) debug_toolbar: Rect,
    pub(super) separated_status: Rect,
    pub(super) wildmenu: Rect,
    pub(super) status: Rect,
    pub(super) cmd: Rect,
}

pub(super) fn bottom_chrome_rects_for_shell_content(
    engine: &Engine,
    area: Rect,
) -> BottomChromeRects {
    // #1164: sourced from the same `bottom_band_row_heights` every other
    // row-accounting call site now shares, rather than re-deriving each
    // gate from `screen.<field>.is_some()` — those were always exactly
    // equivalent to the `Engine`-flag gates below (each `ScreenLayout`
    // field is populated from precisely one of these flags at `screen`
    // build time), so this drops a redundant, `ScreenLayout`-shaped
    // restatement of the same five gates instead of keeping two encodings
    // of one fact in sync by hand.
    let bands = bottom_band_row_heights(engine, area.height);

    // Mirrors `draw_frame`'s `v_chunks` layout exactly (see its own comment)
    // so `content_rows`'s reservation in `build_screen_for_shell_content`
    // and the rects carved here can't drift apart.
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),                         // 0: editor
            Constraint::Length(bands.quickfix),         // 1: quickfix
            Constraint::Length(bands.terminal),         // 2: terminal/debug bottom panel
            Constraint::Length(bands.debug_toolbar),    // 3: debug toolbar
            Constraint::Length(bands.separated_status), // 4: separated status
            Constraint::Length(bands.wildmenu),         // 5: wildmenu
            Constraint::Length(bands.global_status),    // 6: global status
            Constraint::Length(1),                      // 7: cmd
        ])
        .split(area);

    BottomChromeRects {
        quickfix: v_chunks[1],
        bottom_panel: v_chunks[2],
        debug_toolbar: v_chunks[3],
        separated_status: v_chunks[4],
        wildmenu: v_chunks[5],
        status: v_chunks[6],
        cmd: v_chunks[7],
    }
}

// ─── Frame rendering ──────────────────────────────────────────────────────────

// #766: the legacy full-frame raw-`ratatui::Frame` rasteriser (`draw_frame`)
// that used to live here is deleted. It was already `#[cfg(test)]`-only —
// #634 deleted its one production caller (`event_loop`) — so every rung it
// painted has had a `Backend::draw_*` route on the live `TuiShellApp::
// render_content` / GTK `render_content` paths since #601-#765; nothing here
// was "populated but never composed" (the residue check #766 asks for).
//
// What replaces it, for the handful of tests below that still need painted
// pixels rather than just a computed `ScreenLayout`
// (`build_screen_for_tui`/`build_screen_for_shell_content` alone), is
// `tests::render_tui_buffer_impl` — a thinner, test-only walk over the exact
// same shared `render::compose_editor_band` / `render::compose_bottom_band`
// artefacts both live backends already run (see `TuiShellApp::
// paint_editor_band` / `compose_bottom_band_rungs` and `gtk::App`'s twins).
// It deliberately does not paint chrome (activity bar / sidebar body / menu
// row / overlays): no test in this module asserts on any of it, and painting
// it needs an *owned* `TuiShellApp` + a real `AppShellLayout` (the
// `driver_with_shell` path used in `shell_app.rs`), which these tests' bare
// `&Engine` fixtures — several of which mutate `engine` again immediately
// after rendering — cannot hand over without moving `engine` out from under
// its own caller.

/// Paint the editor-anchored popups: completion menu, LSP hover, the rich
/// "editor hover" markdown popup, diff-peek, and signature-help.
///
/// Extracted out of `draw_frame` (#601) because every one of these is
/// already trait-only — `backend.draw_completions`/`draw_tooltip` plus
/// `render_editor_hover_popup` (also widened to `&mut dyn Backend` in
/// #601) — so the exact same code is callable from
/// `TuiShellApp::render_content`, which never has a raw `ratatui::Frame`
/// to pass. `draw_frame` now calls this too, so the two paint paths can't
/// drift on this logic. `area` stands in for each block's original
/// `frame.area()` call (all four computed the identical value from the
/// same frame, just redundantly per-block).
///
/// #1167: the build-adapter → `.layout()` → `backend.draw_*` → cache-
/// output part (identical to GTK's `App::paint_editor_popups_rung`, modulo
/// coordinate units) now lives once in `render::paint_editor_popups`. This
/// function's own job shrank to exactly what stays genuinely per-backend:
/// finding the active window and resolving each popup's on-screen anchor
/// point from it (gutter width, scroll offsets, tab-aware column math) —
/// cell math here, pixel math in `App::paint_editor_popups_rung`.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_editor_popups(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    area: Rect,
    theme: &Theme,
    completion_layout_out: &mut Option<quadraui::CompletionsLayout>,
    editor_hover_link_rects_out: &mut Vec<(quadraui::Rect, String)>,
    editor_hover_popup_rect_out: &mut Option<quadraui::Rect>,
    editor_hover_scrollbar_out: &mut Option<render::PopupScrollbarHit>,
) {
    let viewport = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    let active_win = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id);

    // ── Completion popup anchor ─────────────────────────────────────────────
    let completion = active_win.and_then(|active_win| {
        let menu = screen.completion.as_ref()?;
        let (cursor_pos, _) = active_win.cursor.as_ref()?;
        let gutter_w = active_win.gutter_char_width as u16;
        let win_x = active_win.rect.x as u16;
        let win_y = active_win.rect.y as u16;
        let raw = active_win
            .lines
            .get(cursor_pos.view_line)
            .map(|l| l.raw_text.as_str())
            .unwrap_or("");
        let vis_col = char_col_to_visual(raw, cursor_pos.col, active_win.tabstop)
            .saturating_sub(active_win.scroll_left) as u16;
        let popup_x = win_x + gutter_w + vis_col;
        let popup_y = win_y + cursor_pos.view_line as u16;
        // #420: clamp the popup into the *active window's own* rect, not
        // the shared `viewport` above (which spans every split in the
        // editor band, plus the gap the sidebar already leaves for the
        // first split). Feeding the wider shared viewport into
        // `Completions::layout` let its "shift left on right-overflow"
        // clamp push the popup past the active window's own left edge —
        // into a neighbouring split, or, with the leftmost split narrow,
        // visibly against the sidebar boundary. GTK's
        // `paint_editor_popups_rung` equivalent in `app.rs` already scopes
        // to `win_viewport` this way; this brings TUI in line with it.
        //
        // Built from `tui_window_paint_rect(&active_win.rect)` rather than
        // the raw (possibly fractional) `active_win.rect` directly:
        // `RenderedWindow` rects come from continuous float split math and
        // are not integer-valued in general (see the doc comment on
        // `tui_window_paint_rect` in `render.rs`, #1040), and every TUI
        // call site that resolves geometry against window bounds must snap
        // to the same whole-cell grid the paint path already truncated to,
        // or it silently clamps against geometry that was never painted.
        let win_rect = render::tui_window_paint_rect(&active_win.rect);
        let win_viewport = quadraui::Rect::from(win_rect);
        // #420: `Completions::layout` clamps the popup's *position* into
        // the viewport (`x.max(viewport.x)`), which is what fixes the
        // "bleeds into the sidebar/neighbouring split" symptom above — but
        // it never clamps `popup_width` itself, so an over-wide popup (a
        // long candidate label in a narrow split) can still render past
        // the viewport's right edge even once correctly positioned as far
        // left as it can go. That's a gap in the shared
        // `quadraui::Completions::layout` primitive (it already does the
        // symmetric clamp for height, `clipped_h` below `desired_height`),
        // not something to patch around per-backend here — see the
        // Platform-Neutrality Rule. Left as a tracked follow-up pending a
        // quadraui-side fix; do not re-add a `.min(win_viewport...)` cap
        // here without one.
        let popup_width = (menu.max_width as f32 + 4.0).max(12.0);
        let max_popup_height = 10.0;
        Some((
            render::PopupAnchor {
                x: popup_x as f32,
                y: popup_y as f32,
                viewport: win_viewport,
            },
            popup_width,
            max_popup_height,
        ))
    });

    // ── Hover popup anchor ───────────────────────────────────────────────────
    let hover = active_win.and_then(|active_win| {
        let hover = screen.hover.as_ref()?;
        let gutter_w = active_win.gutter_char_width as u16;
        let win_x = active_win.rect.x as u16;
        let win_y = active_win.rect.y as u16;
        let anchor_view = hover.anchor_line.saturating_sub(active_win.scroll_top) as u16;
        let vis_col = hover.anchor_col.saturating_sub(active_win.scroll_left) as u16;
        Some(render::PopupAnchor {
            x: (win_x + gutter_w + vis_col) as f32,
            y: (win_y + anchor_view) as f32,
            viewport,
        })
    });

    // ── Editor hover popup anchor (rich markdown, gh key or mouse dwell) ────
    // Frozen scroll offsets so the popup stays fixed on screen.
    let editor_hover = active_win.and_then(|active_win| {
        let eh = screen.editor_hover.as_ref()?;
        let gutter_w = active_win.gutter_char_width as u16;
        let win_x = active_win.rect.x as u16;
        let win_y = active_win.rect.y as u16;
        let anchor_view = eh.anchor_line.saturating_sub(eh.frozen_scroll_top) as u16;
        let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left) as u16;
        Some(render::PopupAnchor {
            x: (win_x + gutter_w + vis_col) as f32,
            y: (win_y + anchor_view) as f32,
            viewport,
        })
    });

    // ── Diff peek popup anchor (inline git hunk preview) ────────────────────
    let diff_peek = active_win.and_then(|active_win| {
        let peek = screen.diff_peek.as_ref()?;
        let gutter_w = active_win.gutter_char_width as u16;
        let win_x = active_win.rect.x as u16;
        let win_y = active_win.rect.y as u16;
        let anchor_view = peek.anchor_line.saturating_sub(active_win.scroll_top) as u16;
        // Anchor at the cursor's own row, left edge (no column offset);
        // placement=Bottom (with primitive fallback to Top) puts the
        // popup just below it.
        Some(render::PopupAnchor {
            x: (win_x + gutter_w) as f32,
            y: (win_y + anchor_view) as f32,
            viewport,
        })
    });

    // ── Signature-help popup anchor (insert mode, cursor inside a call) ─────
    let signature_help = active_win.and_then(|active_win| {
        let sig = screen.signature_help.as_ref()?;
        let gutter_w = active_win.gutter_char_width as u16;
        let win_x = active_win.rect.x as u16;
        let win_y = active_win.rect.y as u16;
        let anchor_view = sig.anchor_line.saturating_sub(active_win.scroll_top) as u16;
        let vis_col = sig.anchor_col.saturating_sub(active_win.scroll_left) as u16;
        Some(render::PopupAnchor {
            x: (win_x + gutter_w + vis_col) as f32,
            y: (win_y + anchor_view) as f32,
            viewport,
        })
    });

    // The deleted per-popup `render_editor_hover_popup` wrapper used to
    // re-sync the theme right before painting the rich-markdown editor-hover
    // popup specifically; both frame-level call sites (`draw_frame` /
    // `TuiShellApp::render_content`) already sync it earlier in the same
    // frame, so this is redundant in practice — kept anyway since it's a
    // one-line no-op when already in sync, and removing it isn't part of
    // this convergence.
    backend.set_theme(super::quadraui_tui::q_theme(theme));

    // Per D6: unit scale is 1.0/1.0 — TUI coordinates are already
    // cell-native (#669 widened these adapters to also serve GTK's pixel
    // space).
    render::paint_editor_popups(
        backend,
        screen,
        theme,
        1.0,
        1.0,
        completion,
        hover,
        editor_hover,
        diff_peek,
        signature_help,
        completion_layout_out,
        editor_hover_link_rects_out,
        editor_hover_popup_rect_out,
        editor_hover_scrollbar_out,
    );
}

// ─── Tab bar hit testing ─────────────────────────────────────────────────────

/// Given a column within a group's tab bar, return the shortened file path of
/// the tab at that column, or `None` if the column doesn't hit a tab with a file.
///
/// `hit_regions` is the `TabBarLayout` cached on `ScreenLayout` /
/// `GroupTabBar` by `render::build_screen_layout` — the same layout
/// `render::resolve_tab_bar_click` routes real clicks through.
///
/// #654: this used to walk the tabs itself, hand-rolling
/// `name.chars().count() + TAB_CLOSE_COLS` per tab *plus* a `+2` fudge for a
/// "scroll indicator" that the quadraui TUI tab-bar rasteriser never reserves
/// space for (see `tab_drag_slots_from_hit_regions`' #477 note — the same
/// duplicate had already drifted there). The result was a two-column drift
/// between tooltip and click hit-testing on any tab bar scrolled past tab 0.
/// Sharing the layout removes the whole class: tooltip, click, and drag now
/// read the same geometry.
pub(super) fn tab_tooltip_at_col(
    engine: &Engine,
    group_id: GroupId,
    local_col: u16,
    hit_regions: &quadraui::TabBarLayout,
) -> Option<String> {
    use crate::core::engine::TabBarClickTarget;
    let i = match render::resolve_tab_bar_click(hit_regions, local_col)? {
        TabBarClickTarget::Tab(i) | TabBarClickTarget::CloseTab(i) => i,
        _ => return None,
    };
    let group = engine.editor_groups.get(&group_id)?;
    let tab_data = group.tabs.get(i)?;
    let window = engine.windows.get(&tab_data.active_window)?;
    let state = engine.buffer_manager.get(window.buffer_id)?;
    let raw_path = state.file_path.as_ref()?;
    let path = crate::core::paths::strip_unc_prefix(raw_path);
    let home = crate::core::paths::home_dir();
    if let Ok(rest) = path.strip_prefix(&home) {
        return Some(format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()));
    }
    Some(path.display().to_string())
}

/// Extract per-tab drag-and-drop slot bounds — `(x_start, x_end)` pairs in
/// absolute screen-column units, ordered by tab index — from a tab bar's
/// layout. `base_x` is the absolute left edge the region columns are
/// relative to (bar left edge = column 0).
///
/// #477: `visible_tabs` is the single source of truth already used for mouse
/// click routing (`render::resolve_tab_bar_click`'s `hit_test`); this just
/// re-slices it into the `(f32, f32)` shape the drag overlay / drop-zone
/// geometry expects, instead of hand-rolling `name.chars().count() +
/// TAB_CLOSE_COLS` per tab (which had drifted from the real per-tab
/// close-button width and from an obsolete "+2 for the scroll indicator"
/// adjustment that the quadraui TUI tab bar rasteriser doesn't actually
/// reserve space for).
fn tab_drag_slots_from_hit_regions(
    hit_regions: &quadraui::TabBarLayout,
    base_x: f32,
) -> Vec<(f32, f32)> {
    let mut tabs: Vec<(usize, f32, f32)> = hit_regions
        .visible_tabs
        .iter()
        .map(|vt| {
            (
                vt.tab_idx,
                base_x + vt.bounds.x,
                base_x + vt.bounds.x + vt.bounds.width,
            )
        })
        .collect();
    tabs.sort_unstable_by_key(|(idx, ..)| *idx);
    tabs.into_iter().map(|(_, s, e)| (s, e)).collect()
}

/// Build the per-group tab-drag slot map consumed by the drag overlay and
/// drop-zone hit testing. Reuses the layout already cached on each
/// `GroupTabBar` by `render::build_screen_layout()` (from
/// `compute_tab_bar_layout()`) instead of recomputing tab positions
/// (#515).
///
/// #551: this used to branch, reading `ScreenLayout::tab_bar_hit_regions` at
/// `editor_x` for the single-group case. `group_tab_bars` now carries one
/// entry in that case too, built from the same tabs/scroll-offset/bar-width
/// inputs as the single-group field and with `bounds.x == editor_x`, so the
/// generic arm reproduces it exactly and the special case is gone.
fn build_tui_tab_slots(
    screen: &render::ScreenLayout,
) -> std::collections::HashMap<usize, Vec<(f32, f32)>> {
    let mut map = std::collections::HashMap::new();
    for gtb in &screen.group_tab_bars {
        // #550: `gtb.bounds` is already absolute terminal-screen space
        // (same convention as GTK), so no `editor_x` offset addition.
        let abs_x = gtb.bounds.x as f32;
        map.insert(
            gtb.group_id.0,
            tab_drag_slots_from_hit_regions(&gtb.hit_regions, abs_x),
        );
    }
    map
}

/// Render the tab drag overlay for the TUI path.
///
/// `tab_drag_source` is the (GroupId, tab_index) captured when the drag started.
/// `tab_drag_cursor` is the current cursor position during the drag.
/// `tab_drop_zone` is the most recently computed drop zone.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tab_drag_overlay(
    backend: &mut dyn quadraui::Backend,
    engine: &Engine,
    screen: &render::ScreenLayout,
    theme: &render::Theme,
    tab_drag_source: Option<(crate::core::window::GroupId, usize)>,
    tab_drag_cursor: Option<(f64, f64)>,
    tab_drop_zone: &crate::core::window::DropZone,
) {
    let tab_slots = build_tui_tab_slots(screen);
    let tbh_f = if engine.settings.breadcrumbs {
        2.0f32
    } else {
        1.0
    };
    // #550/#515/#551: `gtb.bounds` is always absolute (built from absolute
    // window rects, matching GTK) for every group count, so
    // `screen_to_drop_group_bounds` needs no origin argument and this call
    // site no longer has to pick one based on split-vs-single.
    let bounds = render::screen_to_drop_group_bounds(screen);
    let (groups, tbh) = render::build_tab_drop_groups(&bounds, engine, tbh_f, &tab_slots);
    let cursor = tab_drag_cursor
        .map(|(mx, my)| (mx as f32, my as f32))
        .unwrap_or((0.0, 0.0));
    let overlay =
        match render::compute_tab_drop_overlay(tab_drop_zone, &groups, cursor, tbh, 1.0, 2.0) {
            Some(o) => o,
            None => return,
        };

    {
        let q_overlay = quadraui::DropOverlay {
            highlight: overlay.highlight,
            insertion_bar: overlay.insertion_bar,
            ghost_position: Some(overlay.ghost_position),
        };
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_drop_overlay(&q_overlay);
    }

    // Look up the tab label from engine using the captured drag source.
    let drag_label: String = if let Some((src_gid, src_tab_idx)) = tab_drag_source {
        engine
            .editor_groups
            .get(&src_gid)
            .and_then(|g| g.tabs.get(src_tab_idx))
            .and_then(|t| {
                let win = engine.windows.get(&t.active_window)?;
                let state = engine.buffer_manager.get(win.buffer_id)?;
                Some(state.display_name().to_string())
            })
            .unwrap_or_default()
    } else {
        String::new()
    };

    if tab_drag_cursor.is_some() && !drag_label.is_empty() {
        let label = &drag_label;
        let gx = overlay.ghost_position.0 as u16;
        let gy = overlay.ghost_position.1 as u16;
        // #609: was a raw `Buffer` write (`frame.buffer_mut()`); routed
        // through `draw_rule_row` (the `Backend::draw_status_bar` trick —
        // see `draw_rule_cell_themed`'s doc comment) so this reaches the screen
        // from `&mut dyn Backend`, which `TuiShellApp::render_content` has
        // but no `Frame`/`Buffer` for. `RColor::White` /
        // `RColor::Indexed(238)` (an xterm-256 palette index with no
        // meaningful `quadraui::Color` RGB equivalent) become plain
        // truecolor RGB — `Color::from_rgb(255, 255, 255)` and the
        // `Indexed(238)` grayscale-ramp equivalent `Color::from_rgb(68, 68,
        // 68)` (xterm 256-color formula: `8 + (238 - 232) * 10`) — losing
        // exact palette parity but matching every other `draw_status_bar`
        // call site here, which are all already truecolor.
        let ghost_fg = Color::from_rgb(255, 255, 255);
        let ghost_bg = Color::from_rgb(68, 68, 68);
        draw_rule_row(backend, gx, gy, label, ghost_fg, ghost_bg, theme);
    }
}

/// Paint the tab-hover tooltip — the small popup shown when the mouse
/// hovers a tab and lingers, naming the buffer under the cursor. `screen
/// .tab_tooltip` (pre-computed by `render::build_screen_layout`) is the
/// only input; painting is a single-row `Backend::draw_status_bar` call
/// (the `draw_rule_row`/[`draw_rule_cell_themed`] trick, #609) instead of the raw
/// `Buffer` write this replaces, so both `draw_frame` and
/// `TuiShellApp::render_content` can call it — the two callers differ only
/// in `(x, y)`, since `render_content`'s `area` doesn't start at the
/// terminal's row 0 the way `draw_frame`'s `editor_area` implicitly did
/// (see call sites for the position math each uses).
pub(super) fn render_tab_hover_tooltip(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    max_width: u16,
    tooltip_text: &str,
    theme: &Theme,
) {
    // #671: geometry now lives in `render::tab_hover_tooltip_paint`, shared
    // with GTK's `render_content` — TUI passes `unit_w`/`unit_h` = 1.0/1.0
    // (cell-native), matching this function's pre-#671 behavior exactly.
    render::tab_hover_tooltip_paint(
        backend,
        x as f32,
        y as f32,
        max_width as f32,
        tooltip_text,
        theme,
        1.0,
        1.0,
    );
}

/// Compute the drop zone for a tab drag in TUI based on cursor cell position.
pub(super) fn compute_tui_tab_drop_zone(
    engine: &Engine,
    col: u16,
    row: u16,
    editor_left: u16,
    last_layout: Option<&render::ScreenLayout>,
    terminal_size: Option<Size>,
) -> crate::core::window::DropZone {
    let layout = match last_layout {
        Some(l) => l,
        None => return crate::core::window::DropZone::None,
    };
    if col < editor_left {
        return crate::core::window::DropZone::None;
    }
    // Group bounds now come entirely from `layout.group_tab_bars` (#551), so
    // the terminal size is no longer an input — but a frame with no known
    // size still means "nothing has been laid out yet", so keep rejecting it.
    if terminal_size.is_none() {
        return crate::core::window::DropZone::None;
    }
    let tab_slots = build_tui_tab_slots(layout);
    let tbh_f = if engine.settings.breadcrumbs {
        2.0f32
    } else {
        1.0
    };
    // #550/#515/#551: `gtb.bounds` is always absolute for every group count,
    // so no origin argument and no split-vs-single choice here.
    let bounds = render::screen_to_drop_group_bounds(layout);
    let (groups, tbh) = render::build_tab_drop_groups(&bounds, engine, tbh_f, &tab_slots);
    render::compute_tab_drop_zone(col as f32, row as f32, &groups, tbh)
}

// `render_tab_bar` and `draw_breadcrumb_bar` used to sit here — two
// `Rect`-taking wrappers whose whole body was "convert to `quadraui::Rect`,
// set the theme, forward to one `Backend::draw_*` call". #764 folded both into
// the shared `EditorOp::TabBars` / `EditorOp::Breadcrumbs` rungs
// (`render::paint_tab_bars` / `render::paint_breadcrumb_bars`), which GTK now
// calls too, so the per-target loop around them exists once instead of three
// times (here, `TuiShellApp::render_content`, and `gtk::App::render_content`).

// ─── Editor windows ───────────────────────────────────────────────────────────

/// `frame: None` (from `TuiShellApp::render_content`, #601) skips cursor
/// placement only (see `render_window`'s doc comment) — cursor placement
/// needs `Frame::set_cursor_position`, which `render_content` still can't
/// reach directly, but #604 closed that gap a layer up (`tui/run.rs
/// ::render_frame` applies `TuiBackend`'s cached `cursor_position` after
/// `render_content` returns). `render_separators`'s window-divider lines
/// used to be a second, unrelated casualty of `frame: None` (raw `Buffer`
/// writes with no `Backend::draw_*` trait equivalent) — #609 ported it to
/// `Backend::draw_status_bar` (see that function's doc comment), so it now
/// runs unconditionally here regardless of `frame`.
pub(super) fn render_all_windows(
    backend: &mut dyn quadraui::Backend,
    mut frame: Option<&mut ratatui::Frame>,
    windows: &[RenderedWindow],
    group_dividers: &[GroupDivider],
    theme: &Theme,
) {
    // #1039: paint the active window *last*.
    //
    // `render_window`'s own `frame: Some(_)` branch below already gates
    // correctly (it only calls `Frame::set_cursor_position` when
    // `cursor_position_native` is `Some`, and `render::build_rendered_window`
    // already only ever gives a non-active `RenderedWindow` a `None` cursor)
    // — but that branch is dead in the live app today: both call sites of
    // `render_all_windows` (this module's own live path and
    // `TuiShellApp::render_content`) pass `frame: None`, so real cursor
    // placement happens entirely through quadraui's `TuiBackend`, which
    // caches the *last* `Backend::draw_editor` call's `cursor_position` on
    // itself (`last_cursor_position`) and applies it to the real `Frame`
    // once `render_content` returns (see this fn's own doc comment for the
    // #604 handoff). That cache is overwritten unconditionally on *every*
    // `draw_editor` call — including ones for inactive windows, which
    // always report `cursor_position: None` — so whichever window happens
    // to paint last decides the whole frame's caret, active or not. Since
    // exactly one window is ever active, and only the active one ever
    // reports a `Bar`/`Underline` position, painting it last guarantees
    // its position is the one still standing when the frame's done — no
    // new state, just reordering against data (`is_active`) already on
    // `RenderedWindow`. The real fix belongs in quadraui (the cache should
    // not let a `None` clobber a `Some` within one frame); this is the
    // vimcode-side workaround pending that. That gap is drafted, ready to
    // file, in `docs/PENDING_QUADRAUI_ISSUES.md` ("`TuiBackend` lets a
    // `None` cursor_position clobber a `Some` within one frame", blocks
    // vimcode#1039); it is not filed yet because filing GitHub issues is a
    // coordinator/human action this worker session cannot perform
    // (`git`-only). Once that lands upstream, this partition-and-reorder
    // becomes redundant (order stops mattering) and can be deleted as a
    // deliberate follow-up — don't assume it's still needed without
    // rechecking.
    let (active, inactive): (Vec<&RenderedWindow>, Vec<&RenderedWindow>) =
        windows.iter().partition(|w| w.is_active);
    for window in inactive.into_iter().chain(active) {
        // #550: `window.rect` is already absolute terminal-screen
        // coordinates. #1040: this truncation to whole cells is the one
        // `render::tui_window_paint_rect`/`render::tui_editor_text_layout`
        // must reproduce exactly for click resolution to agree with what
        // gets painted — route through the shared helper rather than
        // repeating the `as u16` formula a second time.
        let paint_rect = render::tui_window_paint_rect(&window.rect);
        let win_rect = Rect {
            x: paint_rect.x as u16,
            y: paint_rect.y as u16,
            width: paint_rect.width as u16,
            height: paint_rect.height as u16,
        };
        render_window(backend, frame.as_deref_mut(), win_rect, window, theme);
    }
    render_separators(backend, windows, group_dividers, theme);
}

/// Render one editor window (pane) into `frame`.
///
/// Phase C Stage 1C (#276) collapsed the body of this function to a
/// thin delegator. The actual paint code lives in
/// `quadraui::tui::draw_editor`, fed by `render::to_q_editor` (the
/// boundary adapter that converts the engine-side `RenderedWindow`
/// IR into the cross-backend `quadraui::Editor` primitive). This
/// function handles only the bits the rasteriser deliberately
/// excludes: per-window status-line row reservation + paint, and
/// applying the rasteriser's returned cursor position when the shape
/// is `Bar` / `Underline` (which sets `Frame`-level cursor state and
/// can't live inside a `Buffer`-only rasteriser).
///
/// `frame` is `Option` (#601): `TuiShellApp::render_content` only ever gets
/// `&mut dyn quadraui::Backend`, never a raw `ratatui::Frame` (confirmed —
/// `TuiBackend`'s frame pointer is private with no public accessor), so it
/// calls this with `None` and simply doesn't get cursor placement — the
/// same already-tracked gap as quadraui#466 (vimcode#604). The live
/// `draw_frame` path keeps passing `Some(frame)`, unchanged behavior.
pub(super) fn render_window(
    backend: &mut dyn quadraui::Backend,
    frame: Option<&mut ratatui::Frame>,
    area: Rect,
    window: &RenderedWindow,
    theme: &Theme,
) {
    // Reserve the bottom row for the per-window status line when present.
    let status_bar_row = if window.status_line.is_some() && area.height > 1 {
        Some(area.y + area.height - 1)
    } else {
        None
    };
    let editor_area = if status_bar_row.is_some() {
        Rect {
            height: area.height - 1,
            ..area
        }
    } else {
        area
    };

    let editor = render::to_q_editor(window);
    let editor_q_rect = quadraui::Rect::new(
        editor_area.x as f32,
        editor_area.y as f32,
        editor_area.width as f32,
        editor_area.height as f32,
    );
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let result = backend.draw_editor(editor_q_rect, &editor);

    // quadraui#504: `cursor_position` (`(u16, u16)`) is deprecated in favor
    // of `cursor_position_native` (`Point`, native units — whole cells,
    // already rounded, for TUI). `Frame::set_cursor_position` only accepts
    // `(u16, u16)` (`impl From<(u16, u16)> for Position`, no `From<Point>`
    // quadraui could add without violating the orphan rule), so this is the
    // vimcode-side migration the quadraui deprecation note asks for.
    if let (Some(frame), Some(pos)) = (frame, result.cursor_position_native) {
        frame.set_cursor_position((pos.x as u16, pos.y as u16));
    }

    if let (Some(status), Some(sy)) = (&window.status_line, status_bar_row) {
        render_window_status_line(backend, editor_area.x, sy, editor_area.width, status, theme);
    }
}

/// Draw a per-window status line into the given row.
///
/// B5c.1: routes through `Backend::draw_status_bar`. The trait impl
/// computes layout internally with `MIN_GAP_CELLS = 2.0` so right
/// segments priority-drop on narrow bars (#159).
///
/// `StatusBar` adapter encodes engine-side `StatusAction` values as
/// opaque `WidgetId` strings; `status_segment_hit_test` (in mouse.rs)
/// decodes them back to `StatusAction` via `status_action_from_id`
/// after the layout's hit_test() resolves a click — TUI doesn't
/// consume the hit regions returned by `draw_status_bar` because the
/// click handler runs the layout on demand against current bar width.
/// See `render_tab_bar`'s doc comment for why `backend` is the trait object
/// rather than the concrete `TuiBackend` (#601).
pub(super) fn render_window_status_line(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    width: u16,
    status: &crate::render::WindowStatusLine,
    theme: &crate::render::Theme,
) {
    let bar = crate::render::window_status_line_to_status_bar(
        status,
        quadraui::WidgetId::new("status:window"),
    );
    let q_rect = quadraui::Rect::new(x as f32, y as f32, width as f32, 1.0);
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let _ = backend.draw_status_bar(q_rect, &bar, None, None);
}

/// Convert a character-index column to a visual column, expanding tabs.
/// Used by mouse hit-tests outside the editor paint path; the
/// in-rasteriser callers were lifted to `quadraui::tui::editor` in
/// Stage 1C of #276.
pub(super) fn char_col_to_visual(raw_text: &str, char_col: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut vis = 0usize;
    for (i, ch) in raw_text.chars().enumerate() {
        if ch == '\n' || ch == '\r' {
            break;
        }
        if i >= char_col {
            break;
        }
        if ch == '\t' {
            vis = ((vis / tabstop) + 1) * tabstop;
        } else {
            vis += 1;
        }
    }
    vis
}

/// True when `w`'s own `Backend::draw_editor` scrollbar occupies its last
/// column (i.e. its buffer content overflows the visible text rows).
/// Extracted from `render_separators`' vertical-separator branch (#481
/// iter4's fix) so both `render_separators` and [`group_divider_cells`]
/// (#609) can apply the exact same "the scrollbar already doubles as the
/// separator" rule without duplicating the row-accounting math.
fn window_overflows_vertically(w: &RenderedWindow) -> bool {
    let text_rows = (w.rect.height as usize).saturating_sub(
        if w.status_line.is_some() && w.rect.height > 1.0 {
            1
        } else {
            0
        },
    );
    w.total_lines > text_rows
}

/// Terminal column one past a window's right edge — i.e. the column the
/// *next* pane starts in, and therefore the column its group divider paints
/// in.
///
/// The sum is deliberately taken in **f32**, then truncated with quadraui's
/// own cell convention (`as u16`, per `SplitTreeDivider::cell_position`).
/// `RenderedWindow::rect` is an f64 *widening* of the f32 rect
/// `quadraui::SplitTree::layout` produced, and the sibling divider's
/// `position` is that same layout's `bounds.x + first_w` added in f32 — so
/// adding the two f64 fields back together here is not the same arithmetic
/// and can land a whole cell away.
///
/// Concretely (the drag this fixes, #753's `group_divider_drag_moves_the_
/// painted_divider_via_shell_app`): dragging a group divider to column 48 of
/// a 46-wide editor area stores `ratio = 14/46`, which f32 resolves to a
/// left pane of `13.999999046…` cells. quadraui's f32 `34.0 + 13.999999046`
/// rounds back up to exactly `48.0`, so the divider paints at cell 48; the
/// f64 sum stays `47.999999046…` and truncates to **47**, putting the left
/// pane's own separator at 46 instead of 47. `group_divider_cells`' "the
/// left pane already separates these two groups" guard then stops matching
/// and both lines paint — the #481 phantom double divider, with a blank
/// column wedged between them.
fn window_right_edge_cell(rect: &WindowRect) -> u16 {
    (rect.x as f32 + rect.width as f32).max(0.0) as u16
}

/// Absolute `(x, y)` terminal cells where [`render_separators`] paints a
/// vertical `'│'` window-divider glyph — the same geometry its own
/// painting loop below walks, factored out as a pure data computation (no
/// `Buffer`/`Backend` access) so [`group_divider_cells`] (#609) can ask
/// "would `render_separators` already put a divider-like glyph in this
/// cell?" without needing to read back whatever was actually painted.
///
/// `group_dividers` (#1094): the authoritative list of *editor-group*
/// boundaries, consulted only to exclude them from the geometric adjacency
/// test below. Before #1094, `RenderedWindow.rect` stopped short of a
/// pane's true right edge by its own minimap strip's width whenever the
/// strip was on — which, for two windows meeting at a *group* boundary
/// (the ordinary case for side-by-side groups, each with one full-height
/// window), coincidentally kept `a.rect`'s right edge more than a cell
/// short of `b.rect.x`, so this adjacency test could only ever fire for a
/// genuine `:vsplit` sibling *within* one group. #1094 widened
/// `RenderedWindow.rect` back out to the pane's true edge (needed so the
/// pane's own scroll column lands past the strip, not before it — see
/// `render.rs`'s `build_screen_layout_with_breadcrumb_row` doc comment),
/// which closed that gap and exposed a blind spot: nothing here previously
/// distinguished "two windows that are `:vsplit` siblings" from "two
/// windows that just happen to tile edge-to-edge because they're in
/// adjacent groups" — without this exclusion, a plain two-group split with
/// neither window scrolled painted its divider one column early (`sep_x -
/// 1`, this pass's own cell) with `group_divider_cells` correctly (but
/// now wrongly) yielding to it, and that painted column drifted out of
/// sync with `GroupDivider::position` as soon as float rounding gave the
/// two computations different answers (`separator_column_tracks_the_
/// divider_after_a_fractional_drag` pins exactly that drift) — the #753
/// group-divider-drag driver test caught the live version of this as the
/// divider under-tracking the drag by more than a cell.
///
/// Compared in `f64`, against `b.rect.x` directly — not the `u16`-truncated
/// `sep_x` computed below — because that comparison is `window_right_edge_
/// cell`'s own f32-rounding-prone one; using it here would just move the
/// drift into the exclusion test instead of removing it.
fn vertical_separator_cells(
    windows: &[RenderedWindow],
    group_dividers: &[GroupDivider],
) -> std::collections::HashSet<(u16, u16)> {
    let mut cells = std::collections::HashSet::new();
    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            let a = &windows[i];
            let b = &windows[j];

            // Window a is the left pane, b is the right pane. The boundary
            // sits in the last column of a (`sep_x - 1`). Also require
            // vertical overlap — windows from different groups may share an
            // x edge but not overlap in y (e.g. 2×2 grid).
            let v_overlap =
                a.rect.y.max(b.rect.y) < (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height);
            if (a.rect.x + a.rect.width - b.rect.x).abs() < 1.0 && v_overlap {
                // This boundary is an editor-group boundary, not a
                // `:vsplit` one — leave it entirely to `group_divider_cells`
                // (see this function's own doc comment).
                let is_group_boundary = group_dividers.iter().any(|d| {
                    d.direction == SplitDirection::Vertical && (d.position - b.rect.x).abs() < 1.0
                });
                if is_group_boundary {
                    continue;
                }
                // #550: `a.rect`/`b.rect` are already absolute terminal-screen
                // coordinates, so no `editor_area` offset addition needed.
                // See [`window_right_edge_cell`] for why the boundary column
                // is not `(a.rect.x + a.rect.width) as u16`.
                let sep_x = window_right_edge_cell(&a.rect);
                let y_start = a.rect.y.max(b.rect.y) as u16;
                let y_end = (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height) as u16;

                // #481 (iter4): `quadraui::tui::draw_editor` already paints
                // window `a`'s own vertical scrollbar in this exact column
                // (its last column) whenever it overflows — see
                // `render_window` → `draw_editor`, which runs for every window
                // *before* this pass. Re-drawing a second scrollbar here was
                // pure redundancy AND buggy: this pass computed the track from
                // `a.rect.height` (which includes the per-window status-line
                // row) whereas `draw_editor` reserves that row, so the repaint
                // came out one row taller and bled a stray track glyph onto the
                // status bar — reading as a slightly-longer "duplicate"
                // scrollbar jammed against the real one at tab-group
                // boundaries. Let `draw_editor`'s scrollbar own the column; it
                // doubles as the visual separator. Only when the left window
                // has NO scrollbar do we draw a plain divider line.
                let has_scroll = window_overflows_vertically(a) && y_end > y_start;

                if !has_scroll {
                    for y in y_start..y_end {
                        cells.insert((sep_x.saturating_sub(1), y));
                    }
                }
            }
        }
    }
    cells
}

/// Same trick as [`draw_rule_row`], for a horizontal run of `text` in one
/// row — used both for horizontal window separators (`'─'` repeated) and
/// the tab-drag ghost label / tab-hover tooltip (#609), which paint
/// multi-character text rather than a single rule glyph.
///
/// Sets the backend theme on every call, which is correct (if slightly
/// redundant with the caller's own up-front `set_theme`) for the
/// single-shot call sites (drag ghost, hover tooltip) but wasteful for
/// per-cell loops — see [`draw_rule_cell_themed`]/[`draw_rule_row_themed`],
/// which [`render_separators`] and [`render_group_dividers`] use instead so
/// the ~50-field `quadraui::Theme` is rebuilt once per frame, not once per
/// divider cell.
fn draw_rule_row(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    text: &str,
    fg: Color,
    bg: Color,
    theme: &Theme,
) {
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    draw_rule_row_themed(backend, x, y, text, fg, bg);
}

/// Paint a single divider/rule glyph at `(x, y)` through
/// `Backend::draw_status_bar` — the same "a solid-colour `StatusBar`
/// segment stands in for a plain rule line" trick `AppShell::render`'s own
/// generic divider (quadraui `compose/app_shell.rs::render`'s
/// `divider_bounds` block) uses for the sidebar-resize divider, so no new
/// quadraui primitive is needed (#609). `tui/status_bar.rs::draw_status_bar`
/// paints segment text verbatim, one character per cell, in the segment's
/// `fg`/`bg` — a 1-cell-wide, 1-row `StatusBar` therefore renders exactly
/// like a raw `set_cell` write would, but reaches the screen through
/// `&mut dyn Backend`, which `set_cell`/`Buffer` writes cannot.
///
/// Assumes the caller has already applied `backend.set_theme(...)` — see
/// [`draw_rule_row_themed`]'s doc comment for why callers that loop over
/// many cells use this instead of setting the theme per call.
fn draw_rule_cell_themed(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    ch: char,
    fg: Color,
    bg: Color,
) {
    draw_rule_row_themed(backend, x, y, &ch.to_string(), fg, bg);
}

/// Theme-less core of [`draw_rule_row`] — paints without touching the
/// backend's current theme. Callers that loop over many cells/rows in one
/// frame (`render_separators`, `render_group_dividers`) call
/// `backend.set_theme(...)` once up front and then use this (via
/// [`draw_rule_cell_themed`]) for every cell, instead of reconstructing the
/// theme on each of the dozens of divider cells a tall terminal can have.
pub(super) fn draw_rule_row_themed(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    text: &str,
    fg: Color,
    bg: Color,
) {
    draw_rule_row_q(backend, x, y, text, fg, bg);
}

/// [`draw_rule_row_themed`] over already-converted `quadraui::Color`s.
///
/// Panels that reproduce a quadraui rasteriser's own chrome (formerly #605's
/// `panels::draw_settings_chrome_via_backend`, a trait-only stand-in for
/// `quadraui::tui::draw_settings_chrome` that #635, Stage 6b, retired in
/// favour of the real `Backend::draw_settings_chrome` trait method once
/// `quadraui#531` landed) source their colours straight from `q_theme(theme)`
/// so they can't drift from the rasteriser they're standing in for —
/// round-tripping those back through vimcode's `Color` would be lossy
/// busywork.
pub(super) fn draw_rule_row_q(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    text: &str,
    fg: quadraui::Color,
    bg: quadraui::Color,
) {
    if text.is_empty() {
        return;
    }
    let bar = quadraui::StatusBar {
        // Every divider/rule/ghost/tooltip draw shares this literal ID.
        // That's intentionally inert today — `draw_status_bar`'s returned
        // hit-region layout is discarded (`let _ = ...`) by every caller
        // here, and TUI's `draw_status_bar` impl does no ID-keyed caching —
        // but if a future caller wires hover/press state or click handling
        // through this helper, every rule line sharing one ID will collide.
        id: quadraui::WidgetId::new("tui:rule"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: text.to_string(),
            fg,
            bg,
            bold: false,
            action_id: None,
        }],
        right_segments: vec![],
    };
    let width = text.chars().count() as f32;
    let q_rect = quadraui::Rect::new(x as f32, y as f32, width, 1.0);
    let _ = backend.draw_status_bar(q_rect, &bar, None, None);
}

/// Window/editor-group divider lines through `Backend::draw_status_bar`
/// (#609) — see [`draw_rule_cell_themed`]'s doc comment for the underlying trick.
/// Draws both the vertical dividers *between windows within a split group*
/// (e.g. `:vsplit`) and the horizontal ones (`:split`); the group-level
/// dividers *between* editor groups (`split.dividers`, drawn only when
/// `screen.editor_group_split.is_some()`) are a separate pass —
/// [`group_divider_cells`], called by both `draw_frame` and
/// `TuiShellApp::render_content`.
pub(super) fn render_separators(
    backend: &mut dyn quadraui::Backend,
    windows: &[RenderedWindow],
    group_dividers: &[GroupDivider],
    theme: &Theme,
) {
    if windows.len() <= 1 {
        return;
    }

    // Set the backend theme once up front rather than per divider cell/row
    // (see `draw_rule_row_themed`'s doc comment) — a tall terminal with
    // several vertical dividers would otherwise rebuild the ~50-field
    // `quadraui::Theme` dozens of times per frame.
    backend.set_theme(super::quadraui_tui::q_theme(theme));

    for (x, y) in vertical_separator_cells(windows, group_dividers) {
        draw_rule_cell_themed(backend, x, y, '│', theme.separator, theme.background);
    }

    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            let a = &windows[i];
            let b = &windows[j];

            // Horizontal separator — also require horizontal overlap.
            // Skip when the upper window has a per-window status bar (it replaces the separator).
            let h_overlap =
                a.rect.x.max(b.rect.x) < (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width);
            let upper_has_status = if (a.rect.y + a.rect.height - b.rect.y).abs() < 1.0 {
                a.status_line.is_some()
            } else if (b.rect.y + b.rect.height - a.rect.y).abs() < 1.0 {
                b.status_line.is_some()
            } else {
                false
            };
            if (a.rect.y + a.rect.height - b.rect.y).abs() < 1.0 && h_overlap && !upper_has_status {
                let sep_y = (a.rect.y + a.rect.height) as u16;
                let x_start = a.rect.x.max(b.rect.x) as u16;
                let x_end = (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width) as u16;
                if x_end > x_start {
                    let row: String = "─".repeat((x_end - x_start) as usize);
                    draw_rule_row_themed(
                        backend,
                        x_start,
                        sep_y.saturating_sub(1),
                        &row,
                        theme.separator,
                        theme.background,
                    );
                }
            }
        }
    }
}

/// Absolute `(x, y)` cells where the *group-level* divider (`split
/// .dividers` — the boundary between editor groups, e.g. `Ctrl+W v`, as
/// opposed to `render_separators`' within-group `:vsplit`/`:split`
/// dividers) should paint a vertical `'│'` glyph (#609). Filters out cells
/// where the window immediately to the left already shows a visual
/// separator of its own in that exact column — its own overflow scrollbar,
/// or a `render_separators` divider landing on the same cell (#481: two
/// adjacent divider-like columns read as a phantom "duplicate scrollbar").
///
/// Pure data computation over `windows`, no `Buffer` read: the pre-#609
/// `draw_frame` loop this replaces read back
/// `frame.buffer_mut()[(div_x - 1, y)]`'s already-painted symbol to detect
/// the same condition, which only worked because it ran after
/// `render_all_windows`/`render_separators` had already painted into that
/// `Buffer`. `TuiShellApp::render_content` has no `Buffer` to read at all
/// (see this module's own doc comment), so this recomputes "does the left
/// window already separate the two groups here" directly from window
/// geometry (`window_overflows_vertically` for the scrollbar case,
/// `vertical_separator_cells` for the `render_separators` case) instead —
/// both `draw_frame` and `render_content` can now share one answer.
pub(super) fn group_divider_cells(
    dividers: &[GroupDivider],
    windows: &[RenderedWindow],
    editor_area: Rect,
) -> Vec<(u16, u16)> {
    let already_separated = vertical_separator_cells(windows, dividers);
    let mut out = Vec::new();
    for div in dividers {
        if div.direction != SplitDirection::Vertical {
            continue; // horizontal splits use the tab bar as divider.
        }
        let div_x = div.position as u16;
        if div_x >= editor_area.x + editor_area.width {
            continue;
        }
        let y_start = div.cross_start as u16;
        let y_end = y_start + div.cross_size as u16;
        for y in y_start..y_end {
            if div_x > editor_area.x {
                let left_col = div_x - 1;
                let left_has_scrollbar = windows.iter().any(|w| {
                    // Same f32-precision boundary the separator pass uses —
                    // see [`window_right_edge_cell`].
                    let last_col = window_right_edge_cell(&w.rect);
                    // Bound the row range the same way `window_overflows_vertically`
                    // bounds `text_rows`: `draw_editor` only paints the scrollbar
                    // into the window's *text* rows, never the last row when that
                    // row is reserved for the per-window status line (see
                    // `render_window`). Without this the status-line row was
                    // wrongly treated as scrollbar-covered, leaving a 1-row gap in
                    // the group divider right at the neighbor's status bar.
                    let text_row_end = (w.rect.y + w.rect.height) as u16
                        - if w.status_line.is_some() && w.rect.height > 1.0 {
                            1
                        } else {
                            0
                        };
                    last_col.saturating_sub(1) == left_col
                        && (w.rect.y as u16..text_row_end).contains(&y)
                        && window_overflows_vertically(w)
                });
                if left_has_scrollbar || already_separated.contains(&(left_col, y)) {
                    continue;
                }
            }
            out.push((div_x, y));
        }
    }
    out
}

/// Paint the group-level divider lines computed by [`group_divider_cells`]
/// through `Backend::draw_status_bar` (see [`draw_rule_cell_themed`]'s doc
/// comment for the underlying trick). Shared by `draw_frame` (the live
/// path) and `TuiShellApp::render_content` (#609) — see the latter's call
/// site for why it's the same call for both.
pub(super) fn render_group_dividers(
    backend: &mut dyn quadraui::Backend,
    dividers: &[GroupDivider],
    windows: &[RenderedWindow],
    editor_area: Rect,
    theme: &Theme,
) {
    let cells = group_divider_cells(dividers, windows, editor_area);
    if cells.is_empty() {
        return;
    }
    // Set the theme once for the whole batch — see `draw_rule_row_themed`'s
    // doc comment for why the per-cell `draw_rule_cell_themed` (which
    // assumes the theme is already set) is used here instead of
    // `draw_rule_row` (which sets the theme on every call).
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    for (x, y) in cells {
        draw_rule_cell_themed(backend, x, y, '│', theme.separator, theme.background);
    }
}

// ─── Activity bar ─────────────────────────────────────────────────────────────

// ─── Menu bar rendering ─────────────────────────────────────────────────────
// (Now handled by MenuSystem::render() — see draw_frame menu dropdown block.)

// ─── Context menu popup rendering ───────────────────────────────────────────────────────

// ─── Debug toolbar rendering ────────────────────────────────────────────────────────────

// ─── Find/replace overlay ────────────────────────────────────────────────────

// ─── TUI rendering regression tests ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::window::WindowId;
    use crate::render::WindowStatusLine;
    use ratatui::backend::TestBackend;

    /// Create a hermetic engine for rendering tests.
    fn test_engine(text: &str) -> Engine {
        crate::core::session::suppress_disk_saves();
        // `Engine::new_for_test()` builds settings/session/history/git_branch
        // from in-memory defaults instead of loading ambient disk/git state
        // (#615, #439) — see its doc comment for why call-then-overwrite on
        // `Engine::new()` doesn't reliably undo `app_shell.hide_sidebar()`.
        let mut e = Engine::new_for_test();
        e.extension_state = crate::core::session::ExtensionState::default();
        e.ext_registry = None;
        e.mode = crate::core::Mode::Normal;
        e.rebuild_user_keymaps();
        // The committed snapshots assume the sidebar starts visible, which
        // isn't the default; show it through app_shell's own API.
        e.session.explorer_visible = true;
        if !e.app_shell.sidebar_visible() {
            e.app_shell.toggle_sidebar();
        }
        if !text.is_empty() {
            e.buffer_mut().insert(0, text);
        }
        e
    }

    /// Paint `engine`'s editor + bottom bands into a fresh `width`×`height`
    /// grid and return the painted buffer — the shared implementation behind
    /// [`render_tui`] / [`render_tui_row_cells`] / [`render_tui_buffer`].
    ///
    /// #766: replaces the deleted `draw_frame`. Deliberately paints no chrome
    /// (activity bar / sidebar body / menu row / overlays): no test in this
    /// module asserts on any of it, and painting it needs an *owned*
    /// `TuiShellApp` + a real `AppShellLayout` (the `driver_with_shell` path
    /// `shell_app.rs` uses), which this helper's bare `&Engine` — several
    /// callers mutate `engine` again immediately after rendering (see
    /// `dispatch_tab_bar_left_click`) — cannot hand over without moving
    /// `engine` out from under its own caller.
    ///
    /// What *is* painted is composed from the exact same shared walks both
    /// live `render_content`s run — `render::compose_editor_band` /
    /// `render::compose_bottom_band` — via `build_screen_for_shell_content`
    /// and `bottom_chrome_rects_for_shell_content`, the same two functions
    /// `TuiShellApp::render_content` calls. The one hand-rolled piece of
    /// geometry left is the activity-bar/sidebar-separator offset on the x
    /// axis (`TEST_EDITOR_LEFT`): every fixture in this module renders with
    /// the sidebar visible-but-0-wide (see `test_engine`'s doc comment), so
    /// that offset is always exactly `ACTIVITY_BAR_WIDTH + 1`.
    fn render_tui_buffer_impl(engine: &Engine, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::render::Theme::onedark();

        let menu_h: u16 = if engine.menu_bar_visible { 1 } else { 0 };
        let editor_x = ACTIVITY_BAR_WIDTH + 1;
        let area = Rect {
            x: editor_x,
            y: menu_h,
            width: width.saturating_sub(editor_x),
            height: height.saturating_sub(menu_h),
        };
        let mut tui_backend = super::backend::TuiBackend::new();
        let screen = build_screen_for_shell_content(engine, &theme, area, &tui_backend);
        let chrome = bottom_chrome_rects_for_shell_content(engine, area);
        let tui_tbh: f64 = if engine.settings.breadcrumbs && !engine.terminal_maximized {
            2.0
        } else {
            1.0
        };
        terminal
            .draw(|frame| {
                super::with_frame_scope(&mut tui_backend, frame, |backend, _frame| {
                    // ══ Editor band (#764, #735 slice 3) ═════════════════
                    for op in render::compose_editor_band(
                        engine,
                        &screen,
                        false,
                        engine.terminal_maximized,
                    ) {
                        match op {
                            render::EditorOp::Windows => render_all_windows(
                                backend,
                                None,
                                &screen.windows,
                                &screen.group_dividers,
                                &theme,
                            ),
                            render::EditorOp::Minimap => {
                                render::draw_minimap_strip(backend, &screen);
                            }
                            render::EditorOp::TabBars => {
                                backend.set_theme(super::quadraui_tui::q_theme(&theme));
                                let _ = render::paint_tab_bars(
                                    backend, engine, &screen, 1.0, tui_tbh, None,
                                );
                            }
                            render::EditorOp::Breadcrumbs => {
                                backend.set_theme(super::quadraui_tui::q_theme(&theme));
                                render::paint_breadcrumb_bars(
                                    backend,
                                    &screen,
                                    engine.terminal_maximized,
                                );
                            }
                            render::EditorOp::GroupDividers => render_group_dividers(
                                backend,
                                &screen.group_dividers,
                                &screen.windows,
                                area,
                                &theme,
                            ),
                            render::EditorOp::TabDragOverlay => render_tab_drag_overlay(
                                backend,
                                engine,
                                &screen,
                                &theme,
                                None,
                                None,
                                &crate::core::window::DropZone::None,
                            ),
                            render::EditorOp::TabTooltip => {
                                if let Some(ref tooltip_text) = screen.tab_tooltip {
                                    render_tab_hover_tooltip(
                                        backend,
                                        area.x,
                                        area.y + 1,
                                        area.width,
                                        tooltip_text,
                                        &theme,
                                    );
                                }
                            }
                        }
                    }

                    // ── Editor-anchored popups — same code both live paths
                    // call (`paint_editor_popups`, #601).
                    paint_editor_popups(
                        backend,
                        &screen,
                        area,
                        &theme,
                        &mut None,
                        &mut Vec::new(),
                        &mut None,
                        &mut None,
                    );

                    // ══ Bottom band (#765, #735 slice 4) ═════════════════
                    for op in render::compose_bottom_band(engine, &screen, false) {
                        match op {
                            render::BottomOp::Quickfix => {
                                if let Some(ref qf) = screen.quickfix {
                                    backend.set_theme(super::quadraui_tui::q_theme(&theme));
                                    render::paint_quickfix_rung(
                                        backend,
                                        qf,
                                        super::shell_app::to_q_rect(chrome.quickfix),
                                        0,
                                    );
                                }
                            }
                            render::BottomOp::BottomPanel => {
                                backend.set_theme(super::quadraui_tui::q_theme(&theme));
                                render::paint_bottom_panel_rung(
                                    backend,
                                    engine,
                                    &screen,
                                    &theme,
                                    super::shell_app::to_q_rect(chrome.bottom_panel),
                                    render::BottomPanelUnits::CELL,
                                );
                            }
                            render::BottomOp::DebugToolbar => {
                                render::draw_debug_toolbar(
                                    backend,
                                    engine,
                                    super::shell_app::to_q_rect(chrome.debug_toolbar),
                                );
                            }
                            render::BottomOp::SeparatedStatus => {
                                if let Some(ref status) = screen.separated_status_line {
                                    backend.set_theme(super::quadraui_tui::q_theme(&theme));
                                    let _ = render::paint_separated_status_rung(
                                        backend,
                                        status,
                                        super::shell_app::to_q_rect(chrome.separated_status),
                                    );
                                }
                            }
                            // Never composed: `compose_bottom_band` is called
                            // with `sidebar_open = false` above (this module
                            // never renders sidebar chrome).
                            render::BottomOp::PanelHover => {}
                        }
                    }

                    // ── Wildmenu / status / command line ──────────────────
                    if let Some(ref wm) = screen.wildmenu {
                        let bar = render::wildmenu_to_status_bar(wm, &theme);
                        backend.draw_status_bar(
                            super::shell_app::to_q_rect(chrome.wildmenu),
                            &bar,
                            None,
                            None,
                        );
                    }
                    if let Some(ref bar) = screen.global_status_bar {
                        backend.draw_status_bar(
                            super::shell_app::to_q_rect(chrome.status),
                            bar,
                            None,
                            None,
                        );
                    }
                    render_command_line(backend, chrome.cmd, &screen.command, &theme, None);
                });
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// Render the TUI and return the character buffer as a Vec of lines.
    fn render_tui(engine: &Engine, width: u16, height: u16) -> Vec<String> {
        let buf = render_tui_buffer_impl(engine, width, height);
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                let cell = &buf[(x, y)];
                line.push_str(cell.symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    /// Assert that a specific row in the rendered output contains a substring.
    fn assert_row_contains(lines: &[String], row: usize, substr: &str) {
        assert!(
            row < lines.len(),
            "row {row} out of bounds (have {} lines)",
            lines.len()
        );
        assert!(
            lines[row].contains(substr),
            "row {row}: expected {substr:?} in {:?}",
            lines[row]
        );
    }

    // ── #654: wide (CJK/emoji) tab names ──────────────────────────────────
    //
    // These four tests pin the invariant #654 is really about: *every* TUI
    // tab-bar hit-test — tooltip, left click, right click, drag slots —
    // resolves to the tab that is actually painted at that column, including
    // when a tab name contains double-width characters.
    //
    // They are deliberately written against the **rendered buffer** rather
    // than against a recomputed width, so they stay honest if the rasteriser's
    // measurement ever changes. See `render::tab_hit_width` for why that
    // measurement is still `chars().count()` today and what has to change in
    // quadraui before it can become `display_width`.

    /// A tab bar whose first tab has a CJK name and whose second is ASCII.
    /// Returns the engine with tab 0 active.
    fn engine_with_wide_named_tab() -> Engine {
        let mut e = test_engine("content\n");
        let set_path = |e: &mut Engine, p: &str| {
            let wid = e.active_window_id();
            let bid = e.windows.get(&wid).unwrap().buffer_id;
            e.buffer_manager.get_mut(bid).unwrap().file_path = Some(std::path::PathBuf::from(p));
        };
        set_path(&mut e, "/tmp/日本語.rs");
        e.new_tab(None);
        set_path(&mut e, "/tmp/second.rs");
        e.goto_tab(0);
        assert!(
            !e.menu_bar_visible,
            "these tests assume the tab bar is the top row"
        );
        e
    }

    /// `editor_left` as `mouse::handle_mouse` computes it for a visible
    /// sidebar of width 0 — the geometry `render_tui` renders with.
    const TEST_EDITOR_LEFT: u16 = ACTIVITY_BAR_WIDTH + 1;

    /// Per-cell symbols of one rendered row. Unlike `render_tui`'s joined
    /// strings this keeps column indices exact even when a cell holds a
    /// wide glyph or ratatui's wide-glyph continuation marker.
    fn render_tui_row_cells(engine: &Engine, width: u16, height: u16, row: u16) -> Vec<String> {
        let buf = render_tui_buffer_impl(engine, width, height);
        (0..width)
            .map(|x| buf[(x, row)].symbol().to_string())
            .collect()
    }

    /// First column at which `needle` appears as consecutive cells in `cells`.
    fn painted_col_of(cells: &[String], needle: &str) -> u16 {
        let joined: String = cells.concat();
        assert_eq!(
            joined.chars().count(),
            cells.len(),
            "test helper assumes one char per cell"
        );
        let byte_idx = joined
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} not painted in row {joined:?}"));
        joined[..byte_idx].chars().count() as u16
    }

    /// #654 acceptance: with a double-width tab name in front of it, the
    /// second tab's painted column must resolve to `Tab(1)` — not to the
    /// first tab and not to nothing.
    ///
    /// The expected column is read out of the rendered buffer, so this fails
    /// for *any* measurement that disagrees with the rasteriser (it catches
    /// both the pre-#654 hand-rolled walks and a naive `display_width` swap
    /// made without the matching quadraui change).
    #[test]
    fn tab_hit_regions_match_painted_columns_for_wide_names() {
        use crate::core::engine::TabBarClickTarget;
        let e = engine_with_wide_named_tab();
        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let screen = build_screen_for_tui(&e, &theme, area, &sidebar, 0);
        assert!(
            quadraui::tui::display_width(&screen.tab_bar[0].name)
                > screen.tab_bar[0].name.chars().count(),
            "tab 0 must actually contain double-width characters"
        );

        let cells = render_tui_row_cells(&e, 80, 24, 0);
        // #700: tab labels no longer carry an ordinal prefix ("1:"/"2:"), so
        // locate each tab by a substring of its own (distinct) filename.
        let tab0_col = painted_col_of(&cells, "日");
        let tab1_col = painted_col_of(&cells, "second");
        assert!(tab1_col > tab0_col);

        let resolve = |abs_col: u16| {
            render::resolve_tab_bar_click(&screen.tab_bar_hit_regions, abs_col - TEST_EDITOR_LEFT)
        };
        assert_eq!(
            resolve(tab1_col),
            Some(TabBarClickTarget::Tab(1)),
            "column {tab1_col} paints tab 1 but hit-tests as {:?}",
            resolve(tab1_col)
        );
        assert_eq!(resolve(tab0_col), Some(TabBarClickTarget::Tab(0)));
        // The close glyph painted between the two tabs belongs to tab 0.
        let close_col = painted_col_of(&cells, "×");
        assert!(close_col > tab0_col && close_col < tab1_col);
        assert_eq!(resolve(close_col), Some(TabBarClickTarget::CloseTab(0)));
    }

    /// End-to-end counterpart: a real left click at the second tab's painted
    /// column, dispatched through `handle_mouse`, activates the second tab.
    #[test]
    fn left_click_after_wide_named_tab_activates_the_painted_tab() {
        let mut e = engine_with_wide_named_tab();
        let cells = render_tui_row_cells(&e, 80, 24, 0);
        let tab1_col = painted_col_of(&cells, "second");
        let tab0_col = painted_col_of(&cells, "日");

        assert_eq!(e.active_group().active_tab, 0);
        dispatch_tab_bar_left_click(&mut e, tab1_col);
        assert_eq!(
            e.active_group().active_tab,
            1,
            "click at painted column {tab1_col} should activate tab 1"
        );
        dispatch_tab_bar_left_click(&mut e, tab0_col);
        assert_eq!(e.active_group().active_tab, 0);
    }

    /// Pure-ASCII tab names must be completely unaffected by #654.
    ///
    /// #700 removed the ordinal prefix that used to distinguish otherwise-
    /// identical `[No Name]` tabs by column, so each tab here is given a
    /// distinct file name instead — a closer match for real usage anyway.
    #[test]
    fn ascii_tab_names_still_hit_test_at_their_painted_columns() {
        let mut e = test_engine("content\n");
        let set_path = |e: &mut Engine, p: &str| {
            let wid = e.active_window_id();
            let bid = e.windows.get(&wid).unwrap().buffer_id;
            e.buffer_manager.get_mut(bid).unwrap().file_path = Some(std::path::PathBuf::from(p));
        };
        set_path(&mut e, "/tmp/one.rs");
        e.new_tab(None);
        set_path(&mut e, "/tmp/two.rs");
        e.new_tab(None);
        set_path(&mut e, "/tmp/three.rs");
        e.goto_tab(0);
        let cells = render_tui_row_cells(&e, 80, 24, 0);
        let cols: Vec<u16> = ["one.rs", "two.rs", "three.rs"]
            .iter()
            .map(|n| painted_col_of(&cells, n))
            .collect();
        for (i, col) in cols.iter().enumerate() {
            dispatch_tab_bar_left_click(&mut e, *col);
            assert_eq!(
                e.active_group().active_tab,
                i,
                "ASCII tab {i} painted at column {col} must still activate on click"
            );
        }
    }

    /// The hover tooltip and the click router must name the same tab.
    ///
    /// Before #654 `tab_tooltip_at_col` walked the tabs itself and added a
    /// `+2` "scroll indicator" offset whenever the bar was scrolled — space
    /// the rasteriser never reserves (the same stale adjustment #477 had
    /// already removed from the drag-slot map). That put the tooltip two
    /// columns out of step with the click hit-boxes on any scrolled tab bar.
    #[test]
    fn tab_tooltip_agrees_with_click_target_across_the_bar() {
        use crate::core::engine::TabBarClickTarget;
        let mut e = test_engine("content\n");
        // Enough tabs (with wide names) that the bar has to scroll.
        for i in 0..12 {
            if i > 0 {
                e.new_tab(None);
            }
            let wid = e.active_window_id();
            let bid = e.windows.get(&wid).unwrap().buffer_id;
            e.buffer_manager.get_mut(bid).unwrap().file_path =
                Some(std::path::PathBuf::from(format!("/tmp/日本語_{i}.rs")));
        }
        // Scroll the bar: this is the state the old `+2` "scroll indicator"
        // fudge in `tab_tooltip_at_col` keyed off.
        let gid = e.active_group;
        assert!(e.set_tab_scroll_offset(gid, 4));
        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let screen = build_screen_for_tui(&e, &theme, area, &sidebar, 0);
        // #764: reads the per-group offset the paint actually uses
        // (`GroupTabBar::tab_scroll_offset`), not the deleted single-group
        // `ScreenLayout::tab_scroll_offset` mirror — which was this field's
        // last reader anywhere, and the reason slice 1 could only mark it
        // "superseded" rather than delete it.
        assert!(
            screen.group_tab_bars[0].tab_scroll_offset > 0,
            "test needs a scrolled tab bar to exercise the stale +2 offset"
        );

        let mut checked = 0;
        for local_col in 0..40u16 {
            let target = render::resolve_tab_bar_click(&screen.tab_bar_hit_regions, local_col);
            let idx = match target {
                Some(TabBarClickTarget::Tab(i) | TabBarClickTarget::CloseTab(i)) => i,
                _ => continue,
            };
            let tooltip =
                tab_tooltip_at_col(&e, e.active_group, local_col, &screen.tab_bar_hit_regions);
            assert_eq!(
                tooltip.as_deref(),
                Some(format!("/tmp/日本語_{idx}.rs").as_str()),
                "column {local_col} clicks tab {idx} but its tooltip disagrees"
            );
            checked += 1;
        }
        assert!(checked > 0, "no tab columns were exercised");
    }

    /// Dispatch a left click on the single-group tab bar row through the real
    /// `handle_mouse` entry point, with the same layout `render_tui_row_cells`
    /// rendered from.
    fn dispatch_tab_bar_left_click(engine: &mut Engine, col: u16) {
        let theme = crate::render::Theme::onedark();
        let sidebar_for_layout = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let screen = build_screen_for_tui(engine, &theme, area, &sidebar_for_layout, 0);
        let ev = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        let mut sidebar = TuiSidebar::new();
        let mut drag_state = quadraui::DragState::default();
        let mut modal_stack = quadraui::ModalStack::new();
        let mut should_quit = false;
        handle_mouse(
            ev,
            &mut sidebar,
            engine,
            &Some(Size {
                width: 80,
                height: 24,
            }),
            0,
            &mut false,
            &mut false,
            &mut false,
            &mut None,
            &mut drag_state,
            &mut modal_stack,
            Some(&screen),
            false,
            &mut None,
            &mut should_quit,
            &mut None,
            &mut None,
            &mut crate::render::TabDragState::default(),
            &[],
            None,
            None,
            &[],
            None,
            &mut false,
            &mut false,
            None,
            None,
            None,
            None,
        );
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[test]
    fn test_tui_renders_file_content() {
        let e = test_engine("Hello, world!\nSecond line\n");
        let lines = render_tui(&e, 80, 24);

        // Content should appear somewhere in the rendered output
        let has_hello = lines.iter().any(|l| l.contains("Hello, world!"));
        assert!(has_hello, "rendered output should contain file content");

        let has_second = lines.iter().any(|l| l.contains("Second line"));
        assert!(has_second, "rendered output should contain second line");
    }

    /// #477 fix iteration 1 regression test: dragging a tab within a single
    /// (unsplit) tab group and dropping it over the tab bar must resolve to
    /// a same-group `TabReorder`, never a `Split`.
    ///
    /// Root cause was in `render::screen_to_drop_group_bounds`'s no-split
    /// branch: it passed the whole-editor origin/size (top-left at the
    /// global tab bar's row, per the "tab bar at row 0 of editor_area"
    /// convention) straight through as `DropGroupBounds` content bounds,
    /// which `build_tab_drop_groups` then shifted *up* by `tab_bar_height`
    /// again to reconstruct the full rect. That double-shift made the
    /// computed tab-bar band sit one row above the screen (`bounds.y`
    /// negative), so a cursor sitting on the real tab-bar row (row 0)
    /// tested as being *above* the bar — landing in the `Split(Top)`
    /// branch of `quadraui::compute_drop_zone` instead of `TabReorder`.
    #[test]
    fn test_tui_single_group_tab_drag_reorder_not_split_477() {
        let mut e = test_engine("content\n");
        e.new_tab(None);
        e.new_tab(None);
        e.new_tab(None);
        assert_eq!(e.active_group().tabs.len(), 4);
        assert_eq!(
            e.editor_groups.len(),
            1,
            "test setup must stay a single tab group"
        );

        let theme = crate::render::Theme::onedark();
        let sidebar = TuiSidebar::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let screen = build_screen_for_tui(&e, &theme, area, &sidebar, 0);
        assert!(
            screen.editor_group_split.is_none(),
            "test setup must stay a single tab group"
        );

        let slots = build_tui_tab_slots(&screen);
        let group_slots = slots
            .get(&e.active_group.0)
            .expect("single-group tab slots must be keyed by the active group id");
        assert_eq!(group_slots.len(), 4, "expected 4 visible tab slots");

        // Cursor over the middle of tab index 2 (3rd tab), row 0 — the tab
        // bar's own row. A drop here reorders within the group; it must
        // never be resolved as a split.
        let (s2, e2) = group_slots[2];
        let cursor_col = ((s2 + e2) / 2.0).round() as u16;
        let zone = compute_tui_tab_drop_zone(
            &e,
            cursor_col,
            0,
            0,
            Some(&screen),
            Some(Size {
                width: 80,
                height: 24,
            }),
        );
        match zone {
            crate::core::window::DropZone::TabReorder(gid, _) => {
                assert_eq!(gid, e.active_group, "reorder must target the source group");
            }
            other => {
                panic!("expected DropZone::TabReorder for a drop on the tab bar, got {other:?}")
            }
        }
    }

    #[test]
    fn test_tui_renders_tab_bar() {
        let e = test_engine("content\n");
        let lines = render_tui(&e, 80, 24);

        // Tab bar is the first line; should show "[No Name]" for unsaved buffer
        assert_row_contains(&lines, 0, "No Name");
    }

    #[test]
    fn test_tui_renders_command_line() {
        let e = test_engine("content\n");
        let lines = render_tui(&e, 80, 24);

        // Last line is the command line — should not contain normal text content.
        // Activity bar icons (nerd font glyphs) may appear in the leftmost columns.
        let last = &lines[23];
        assert!(
            !last.contains("content") && !last.contains("NORMAL"),
            "command line should not contain editor content or status, got: {last:?}"
        );
    }

    #[test]
    fn test_tui_renders_status_bar() {
        let e = test_engine("content\n");
        let lines = render_tui(&e, 80, 24);

        // Per-window status bar should show NORMAL mode
        let has_normal = lines
            .iter()
            .any(|l| l.contains("NORMAL") || l.contains("NOR"));
        assert!(has_normal, "status bar should show normal mode");
    }

    /// #481 iteration 4 regression: two vertically-split tab groups whose
    /// windows both overflow (so each shows a scrollbar) must render exactly
    /// ONE vertical bar at the group boundary — the left window's own
    /// `draw_editor` scrollbar. The pre-fix `render_separators` redundantly
    /// repainted a second scrollbar in the same column using `a.rect.height`
    /// (which includes the per-window status-line row) as the track height,
    /// so the repaint ran one row taller and bled a stray track glyph onto
    /// the status bar — the operator saw this as a slightly-longer "duplicate"
    /// scrollbar jammed against the real one.
    #[test]
    fn test_tui_two_groups_single_boundary_scrollbar_481() {
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("line {i}\n"));
        }
        let mut e = test_engine(&text);
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        // Operator flow: scroll the LEFT group down, the RIGHT group to a
        // different position, so both windows overflow and show scrollbars.
        e.focus_window_direction(crate::core::window::SplitDirection::Vertical, false);
        e.handle_key("G", Some('G'), false);
        e.focus_window_direction(crate::core::window::SplitDirection::Vertical, true);
        e.handle_key("g", Some('g'), false);
        e.handle_key("g", Some('g'), false);

        let width = 80u16;
        let height = 24u16;
        let buf = render_tui_buffer_impl(&e, width, height);

        // Recover the boundary column: the left window's last column, where
        // `draw_editor` paints its scrollbar. Locate the single boundary by
        // scanning for a column that is entirely scrollbar glyphs across the
        // editor body (rows 2..21 in this fixture) and is NOT the far-right
        // scrollbar of the right pane.
        let scroll_glyph = |s: &str| s == "█" || s == "░";
        let mut boundary_cols: Vec<u16> = Vec::new();
        for x in 0..width {
            let mut scroll_rows = 0;
            for y in 2..22u16 {
                if scroll_glyph(buf[(x, y)].symbol()) {
                    scroll_rows += 1;
                }
            }
            // A scrollbar column is (nearly) all scroll glyphs down the body.
            if scroll_rows >= 18 {
                boundary_cols.push(x);
            }
        }
        // Exactly two scrollbar columns overall: the left pane's (at the group
        // boundary) and the right pane's (far right edge). Crucially they must
        // not be adjacent — no "two jammed together" at the boundary.
        assert_eq!(
            boundary_cols.len(),
            2,
            "expected exactly 2 scrollbar columns (one per pane), got {boundary_cols:?}"
        );
        assert!(
            boundary_cols[1] - boundary_cols[0] > 2,
            "the two panes' scrollbars must be far apart, not jammed together: {boundary_cols:?}"
        );

        // The group-boundary scrollbar column must not have an adjacent
        // second vertical bar (scrollbar glyph or '│') immediately to its
        // right — that was the duplicate the operator reported.
        let sep_col = boundary_cols[0];
        for y in 2..22u16 {
            let right = buf[(sep_col + 1, y)].symbol();
            assert!(
                !(scroll_glyph(right) || right == "│"),
                "row {y}: found a duplicate separator glyph {right:?} at col {} right beside the boundary scrollbar",
                sep_col + 1
            );
        }

        // The boundary scrollbar must NOT bleed onto the per-window status
        // row (row 22): `draw_editor` reserves that row, and the old
        // `render_separators` repaint (one row too tall) painted a stray
        // track glyph there.
        let status_row = 22u16;
        assert!(
            !scroll_glyph(buf[(sep_col, status_row)].symbol()),
            "boundary scrollbar bled a glyph onto the status row at ({sep_col}, {status_row})"
        );
    }

    #[test]
    fn test_tui_split_renders_two_panes() {
        let mut e = test_engine("left pane\n");
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let lines = render_tui(&e, 80, 24);

        // Both panes should have a tab bar with "[No Name]"
        // Count occurrences of "No Name" across all lines
        let tab_count: usize = lines.iter().filter(|l| l.contains("No Name")).count();
        assert!(
            tab_count >= 2,
            "split should produce two tab bars, found {tab_count} 'No Name' occurrences"
        );
    }

    #[test]
    fn test_tui_dirty_indicator() {
        let mut e = test_engine("clean\n");
        e.handle_key("i", Some('i'), false);
        e.handle_key("x", Some('x'), false);
        e.handle_key("Escape", None, false);
        let lines = render_tui(&e, 80, 24);

        // Dirty buffer shows a dot indicator in the tab bar
        let has_dot = lines[0].contains('●') || lines[0].contains('•') || lines[0].contains('+');
        assert!(
            has_dot,
            "dirty buffer should show indicator in tab bar: {:?}",
            lines[0]
        );
    }

    #[test]
    fn test_tui_insert_mode_status() {
        let mut e = test_engine("hello\n");
        e.handle_key("i", Some('i'), false);
        let lines = render_tui(&e, 80, 24);

        let has_insert = lines
            .iter()
            .any(|l| l.contains("INSERT") || l.contains("INS"));
        assert!(has_insert, "insert mode should show in status bar");
    }

    #[test]
    fn test_tui_visual_mode_status() {
        let mut e = test_engine("hello\n");
        e.handle_key("v", Some('v'), false);
        let lines = render_tui(&e, 80, 24);

        let has_visual = lines
            .iter()
            .any(|l| l.contains("VISUAL") || l.contains("VIS"));
        assert!(has_visual, "visual mode should show in status bar");
    }

    #[test]
    fn test_tui_dimensions_respected() {
        let e = test_engine("content\n");
        // Small terminal
        let lines = render_tui(&e, 40, 10);
        assert_eq!(lines.len(), 10, "should render exactly 10 rows");

        // All lines should fit in 40 display columns.
        // Note: multi-byte nerd font glyphs may make .len() > 40 but the
        // ratatui buffer guarantees 40 cell columns. Check cell count instead.
        // (The render_tui helper already indexes by cell coordinates.)
    }

    #[test]
    fn test_tui_long_file_scroll() {
        // Create a file longer than the viewport
        let content: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        let e = test_engine(&content);
        let lines = render_tui(&e, 80, 15);

        // Should show "line 1" at the top (we're at scroll position 0)
        let has_line1 = lines.iter().any(|l| l.contains("line 1"));
        assert!(has_line1, "scrolled-to-top should show line 1");

        // Should NOT show "line 50" (too far down)
        let has_line50 = lines.iter().any(|l| l.contains("line 50"));
        assert!(!has_line50, "should not show line 50 in 15-row viewport");
    }

    /// #481 regression: in multi-tab-group vertical layouts, a group whose
    /// left window overflows renders that window's scrollbar in the column
    /// immediately before the group divider. The divider glyph must NOT be
    /// painted next to that scrollbar — doing so produced a phantom
    /// "duplicate scrollbar" bar. The scrollbar column doubles as the group
    /// separator, so no `│` may sit immediately to the right of a scrollbar
    /// glyph anywhere in the grid.
    #[test]
    fn test_tui_no_phantom_divider_beside_scrollbar_481() {
        let content: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        let mut e = test_engine(&content);
        // Two tab groups side by side; scroll the right one so it overflows.
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        e.active_window_mut().view.scroll_top = 40;
        e.active_window_mut().view.cursor.line = 45;
        // A third group, scrolled to yet another position.
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        e.active_window_mut().view.scroll_top = 80;
        e.active_window_mut().view.cursor.line = 85;

        let lines = render_tui(&e, 80, 24);
        for (y, l) in lines.iter().enumerate() {
            let chars: Vec<char> = l.chars().collect();
            for x in 1..chars.len() {
                let left = chars[x - 1];
                let cur = chars[x];
                if (left == '█' || left == '░') && cur == '│' {
                    panic!(
                        "phantom divider '│' at row {y}, col {x} sits immediately \
                         right of scrollbar glyph '{left}' — duplicate-scrollbar bug (#481)\n\
                         row: {l}"
                    );
                }
            }
        }
    }

    /// #481 guard: a group divider is still drawn between groups when the
    /// left window does NOT overflow (no scrollbar to double as separator).
    #[test]
    fn test_tui_divider_present_without_scrollbar_481() {
        // Short file: no overflow, so no per-window scrollbar.
        let content = "a\nb\nc\n";
        let mut e = test_engine(content);
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let lines = render_tui(&e, 80, 24);
        let has_divider = lines.iter().any(|l| l.contains('│'));
        assert!(
            has_divider,
            "vertical group divider '│' must be drawn when the left window has no scrollbar"
        );
    }

    // ── Snapshot tests (golden reference) ────────────────────────────────
    //
    // These capture the full rendered grid. Any visual change causes a
    // test failure until the snapshot is reviewed and accepted with:
    //   cargo insta review
    //
    // First run creates the snapshot files automatically.
    //
    // The `prepend_module_path(false)` setting ensures both the `vimcode`
    // and `vcd` binaries share the same snapshot files.

    fn snap_settings() -> insta::Settings {
        let mut s = insta::Settings::clone_current();
        s.set_prepend_module_to_snapshot(false);
        s.set_snapshot_path("snapshots");
        s
    }

    /// #1175 updated the golden file: the minimap strip's braille changed
    /// from `⠍⠈⠛⠛⠓⠓⠒⠒` to `⠅⠛⠓⠒⠒⠂⠀⠀`. That is quadraui#1032 (in the pin
    /// this issue bumps to) making the TUI dot scale adapt to the buffer's
    /// own widest sampled line — here the 22-column `println!` line — where
    /// it was previously a fixed 2-source-columns-per-cell. The same three
    /// lines are therefore drawn across a wider per-cell bucket, so the
    /// marks compress leftwards and the strip's last two cells fall empty.
    ///
    /// Verified to be an upstream-only change, not a side effect of this
    /// issue's own aggregation refactor: checking out `develop`'s
    /// `src/render.rs` and `src/tui_main/shell_app.rs` on top of the new
    /// pin reproduces byte-identical new output for both this snapshot and
    /// [`snapshot_split_panes`]. The colour-aggregation move (#1175) cannot
    /// affect these goldens at all — they capture `lines.join("\n")`, i.e.
    /// glyphs, and `syntax_spans` only carries colour.
    #[test]
    fn snapshot_normal_mode() {
        let e = test_engine("fn main() {\n    println!(\"hello\");\n}\n");
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("normal_mode", lines.join("\n")));
    }

    #[test]
    fn snapshot_insert_mode() {
        let mut e = test_engine("hello world\n");
        e.handle_key("i", Some('i'), false);
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("insert_mode", lines.join("\n")));
    }

    #[test]
    fn snapshot_visual_selection() {
        let mut e = test_engine("select this text\nand this too\n");
        e.handle_key("v", Some('v'), false);
        for _ in 0..10 {
            e.handle_key("l", Some('l'), false);
        }
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("visual_selection", lines.join("\n")));
    }

    #[test]
    fn snapshot_command_line() {
        let mut e = test_engine("buffer content\n");
        e.handle_key(":", Some(':'), false);
        e.handle_key("s", Some('s'), false);
        e.handle_key("e", Some('e'), false);
        e.handle_key("t", Some('t'), false);
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("command_line", lines.join("\n")));
    }

    /// #1175 updated the golden file, for the same reason as
    /// [`snapshot_normal_mode`] — quadraui#1032's adaptive TUI dot scale.
    /// Both panes show the same buffer, so both strips move identically
    /// (`⠉⠉⠈⠉⠁⠉` → `⠉⠉⠉⠉⠁⠀`): the 17-column `left pane content` now drives
    /// a wider per-cell bucket than the old fixed scale, filling the first
    /// four cells solid and emptying the last.
    #[test]
    fn snapshot_split_panes() {
        let mut e = test_engine("left pane content\n");
        e.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let lines = render_tui(&e, 80, 16);
        snap_settings().bind(|| insta::assert_snapshot!("split_panes", lines.join("\n")));
    }

    /// #35: the TUI minimap paints braille (`U+2800` block) dot cells — 2 dots
    /// wide × 4 buffer lines tall per terminal cell. The fixture's
    /// indentation shape is deliberately lopsided (a deeply-indented middle
    /// band between flush top and bottom bands) so the snapshot pins the dot
    /// grid's *orientation*: a transposed grid would paint the indent step at
    /// the wrong end of the strip and this golden file would change.
    ///
    /// RED-first: commenting out `render_content`'s `draw_minimap_strip`
    /// call makes the non-blank-braille assertion below fail — confirmed by
    /// hand before restoring the fix.
    ///
    /// #1093 updated the golden file: the strip now holds a fixed-scale
    /// *window* onto the buffer (121 lines, well over this geometry's
    /// ~52-line window) rather than the whole file squeezed to fit, so the
    /// indent step — real buffer lines 40..80 — now shows only the
    /// `40..52` slice that falls inside the window, one row earlier than
    /// before (the pre-#1093 compression happened to land it at row 4;
    /// windowing, with one buffer line per `lines` entry, lands it at row
    /// 10 — `40 / MINIMAP_LINES_PER_ROW`). Confirmed by hand: this is the
    /// same fixture, same terminal size, only the sampling scale changed.
    #[test]
    fn snapshot_minimap_braille() {
        let text: String = (0..120)
            .map(|i| {
                let depth = if (40..80).contains(&i) { 3 } else { 0 };
                format!("{}line {i}\n", "    ".repeat(depth))
            })
            .collect();
        let e = test_engine(&text);
        assert!(
            e.settings.minimap,
            "fixture must exercise the default-on minimap"
        );
        let lines = render_tui(&e, 100, 16);
        assert!(
            lines.iter().any(|l| l
                .chars()
                .any(|c| ('\u{2800}'..='\u{28FF}').contains(&c) && c != '\u{2800}')),
            "the minimap must paint non-blank braille glyphs; got:\n{}",
            lines.join("\n")
        );
        snap_settings().bind(|| insta::assert_snapshot!("minimap_braille", lines.join("\n")));
    }

    /// The `:set nominimap` half of the same picture — no braille anywhere,
    /// and the editor text reaches further right.
    ///
    /// RED-first: forcing `minimap_reserved_width`'s `has` to ignore
    /// `engine.settings.minimap` (always reserve the strip) makes braille
    /// paint even with the setting off, failing the assertion below —
    /// confirmed by hand before restoring the fix.
    #[test]
    fn nominimap_paints_no_braille() {
        let text: String = (0..120).map(|i| format!("line {i}\n")).collect();
        let mut e = test_engine(&text);
        e.settings.minimap = false;
        let lines = render_tui(&e, 100, 16);
        assert!(
            !lines
                .iter()
                .any(|l| l.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c))),
            "`:set nominimap` must paint no braille at all; got:\n{}",
            lines.join("\n")
        );
    }

    #[test]
    fn snapshot_line_numbers() {
        let mut e = test_engine("alpha\nbeta\ngamma\ndelta\nepsilon\n");
        e.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
        let lines = render_tui(&e, 60, 12);
        snap_settings().bind(|| insta::assert_snapshot!("line_numbers", lines.join("\n")));
    }

    // ── :help render regression tests (#596) ─────────────────────────────────

    /// Drive `:help` through the full event→handle→render path and assert that
    /// (a) the engine does not panic, and (b) the help content appears in the
    /// rendered output.
    ///
    /// Also exercises the case where the cursor is at a non-zero line before
    /// the split (the new help window inherits the view, but the help buffer
    /// is shorter — verifying that the render handles an out-of-range cursor).
    #[test]
    fn test_tui_help_no_panic_and_renders_content() {
        // Use a 50-line buffer and move the cursor down so the view is at a
        // non-zero position before :help is invoked.
        let long_text: String = (0..50).map(|i| format!("line {}\n", i)).collect();
        let mut e = test_engine(&long_text);
        // Move cursor to line ~30 so view.scroll_top and cursor.line are non-zero.
        for _ in 0..30 {
            e.handle_key("j", Some('j'), false);
        }
        // Enter command mode and type 'help', then submit.
        e.handle_key(":", Some(':'), false);
        e.handle_key("h", Some('h'), false);
        e.handle_key("e", Some('e'), false);
        e.handle_key("l", Some('l'), false);
        e.handle_key("p", Some('p'), false);
        e.handle_key("Return", None, false);

        // After :help, there should be a split (2 windows in the layout).
        let win_count = e.active_tab().layout.window_ids().len();
        assert_eq!(win_count, 2, ":help should open a vsplit (2 windows)");

        // Rendering must not panic.
        let lines = render_tui(&e, 80, 24);

        // The help content should appear somewhere in the rendered output.
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("VimCode Help") || rendered.contains("topics"),
            "rendered output should contain help content, got:\n{rendered}"
        );
    }

    /// `:help topics` — named topic must open a split and render content.
    #[test]
    fn test_tui_help_topics_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help topics".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("VimCode Help") || rendered.contains("topics"),
            "help topics should render, got:\n{rendered}"
        );
    }

    /// `:help keys` — must not panic, and must render key reference content.
    #[test]
    fn test_tui_help_keys_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help keys".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Normal Mode") || rendered.contains("Motion"),
            "help keys should render key reference, got:\n{rendered}"
        );
    }

    /// `:help commands` — must not panic, and must render command reference.
    #[test]
    fn test_tui_help_commands_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help commands".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Command Mode") || rendered.contains(":w"),
            "help commands should render command reference, got:\n{rendered}"
        );
    }

    /// `:help explorer` — must not panic, and must render explorer keys.
    #[test]
    fn test_tui_help_explorer_no_panic() {
        let mut e = test_engine("");
        e.handle_key(":", Some(':'), false);
        for ch in "help explorer".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);
        let lines = render_tui(&e, 80, 24);
        let rendered = lines.join("\n");
        assert!(
            rendered.contains("Explorer") || rendered.contains("sidebar"),
            "help explorer should render explorer reference, got:\n{rendered}"
        );
    }

    /// `:help bogus` (unknown topic) — must NOT open a split and must show
    /// the "No help for..." message without crashing.
    #[test]
    fn test_tui_help_unknown_topic_no_panic() {
        let mut e = test_engine("");
        let win_count_before = e.active_tab().layout.window_ids().len();
        e.handle_key(":", Some(':'), false);
        for ch in "help bogus".chars() {
            e.handle_key(&ch.to_string(), Some(ch), false);
        }
        e.handle_key("Return", None, false);

        // Unknown topic must NOT create a new split.
        let win_count_after = e.active_tab().layout.window_ids().len();
        assert_eq!(
            win_count_after, win_count_before,
            "unknown :help topic must not open a split"
        );
        assert!(
            e.message.contains("No help for"),
            "engine.message should say 'No help for', got: {:?}",
            e.message
        );

        // Render must not panic either.
        let _lines = render_tui(&e, 80, 24);
    }

    /// Minimal `RenderedWindow` fixture with every field defaulted except
    /// what the caller overrides — mirrors `render.rs::build_rendered_window`'s
    /// own `empty` closure, since `RenderedWindow` has no `Default` impl.
    fn fixture_window(
        window_id: WindowId,
        rect: WindowRect,
        total_lines: usize,
        status_line: Option<WindowStatusLine>,
    ) -> RenderedWindow {
        RenderedWindow {
            window_id,
            rect,
            lines: vec![],
            cursor: None,
            extra_cursors: vec![],
            selection: None,
            extra_selections: vec![],
            yank_highlight: None,
            scroll_top: 0,
            scroll_left: 0,
            total_lines,
            gutter_char_width: 0,
            text_viewport_cols: 0,
            minimap_reserved_w: 0.0,
            is_active: true,
            show_active_bg: false,
            has_git_diff: false,
            has_breakpoints: false,
            max_col: 0,
            diagnostic_gutter: std::collections::HashMap::new(),
            code_action_lines: std::collections::HashSet::new(),
            bracket_match_positions: Vec::new(),
            active_indent_col: None,
            tabstop: 4,
            cursorline: false,
            status_line,
        }
    }

    /// #609 review fix: [`group_divider_cells`]'s `left_has_scrollbar` check
    /// must exclude the neighbor window's per-window status-line row, the
    /// same way `window_overflows_vertically` excludes it from `text_rows`.
    /// Before the fix, the check spanned the window's *full* rect height,
    /// so an overflowing left window with a status line wrongly looked
    /// scrollbar-covered on its status-line row too — leaving a 1-row gap
    /// in the group divider exactly at that row. Regresses the scenario the
    /// higher-level `render_content_paints_group_divider_via_shell_app`
    /// test deliberately sidesteps (per its own doc comment: short,
    /// non-overflowing content).
    #[test]
    fn group_divider_cells_covers_neighbor_status_line_row() {
        // Left window: overflows vertically (total_lines=20 >> the 9 text
        // rows available after reserving 1 row for the status line out of
        // height=10), and has a per-window status line on its last row (y=9).
        let left = fixture_window(
            WindowId(0),
            WindowRect::new(0.0, 0.0, 10.0, 10.0),
            20,
            Some(WindowStatusLine {
                left_segments: vec![],
                right_segments: vec![],
            }),
        );
        let divider = GroupDivider {
            split_index: 0,
            direction: SplitDirection::Vertical,
            position: 10.0,
            axis_start: 0.0,
            axis_size: 20.0,
            cross_start: 0.0,
            cross_size: 10.0,
        };
        let editor_area = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        };

        let cells = group_divider_cells(&[divider], &[left], editor_area);

        // The status-line row (y=9) is NOT one of the window's text rows —
        // `draw_editor` never paints a scrollbar glyph there — so the group
        // divider must still cover it.
        assert!(
            cells.contains(&(10, 9)),
            "group divider should cover the neighbor's status-line row (y=9), \
             leaving no gap; got cells: {cells:?}"
        );
        // Sanity check the other half of the rule: every actual text row
        // (y=0..9) IS covered by the left window's own overflow scrollbar,
        // so the group divider correctly stays out of those rows (letting
        // the scrollbar double as the divider, per #481).
        for y in 0..9u16 {
            assert!(
                !cells.contains(&(10, y)),
                "row {y} is a text row with an overflow scrollbar; the group \
                 divider should not double up there. got cells: {cells:?}"
            );
        }
    }

    /// Regression pin for [`window_right_edge_cell`]: the exact geometry a
    /// group-divider drag produces (the `#753`
    /// `group_divider_drag_moves_the_painted_divider_via_shell_app` driver
    /// test's, reduced to pure data).
    ///
    /// Dragging the divider of a 46-cell-wide editor area to column 48
    /// stores `ratio = 14/46`, and `quadraui::SplitTree::layout` resolves
    /// that to a left pane exactly `13.999999046325684` cells wide — while
    /// the divider `position` it returns from the *same* `first_w`, summed
    /// in f32, is exactly `48.0`. Summing the pane's f64 `x + width` here
    /// instead yields `47.999999046…`, truncating to 47 — `window_right_edge_
    /// cell`'s own doc comment has the full #481 history of the phantom
    /// double divider that rounding drift used to cause here.
    ///
    /// #1094 closed that gap a different way: [`vertical_separator_cells`]
    /// no longer claims an *editor-group* boundary at all (it now excludes
    /// any pair whose meeting point matches a `GroupDivider`, comparing the
    /// same f64 `rect.x`/`position` units the group layout itself produced,
    /// not the `u16`-truncated `sep_x` this fixture's rounding drift was
    /// about) — group boundaries are [`group_divider_cells`]'s alone,
    /// unconditionally, at the divider's own authoritative `position`. That
    /// sidesteps the #481 rounding drift entirely for this case: nothing
    /// downstream of `group_divider_cells` ever re-derives the boundary
    /// column from `window_right_edge_cell`'s f32 sum. This fixture — a
    /// two-group boundary whose panes carry exactly the #481 rounding drift
    /// — now pins the *new* invariant: `vertical_separator_cells` produces
    /// nothing for it, and `group_divider_cells` alone paints the divider,
    /// consistently at its own `position`, drift or not.
    #[test]
    fn separator_column_tracks_the_divider_after_a_fractional_drag() {
        // `13.999999046325684` is not a typo — it is `(14f32 / 46f32 * 46f32)`
        // widened to f64, i.e. what quadraui actually hands back.
        let left_width = (14.0f32 / 46.0f32 * 46.0f32) as f64;
        assert!(
            left_width < 14.0,
            "fixture precondition: the f32 round-trip must land just *under* \
             a whole cell (got {left_width})"
        );
        let left = fixture_window(
            WindowId(0),
            WindowRect::new(34.0, 2.0, left_width, 21.0),
            1,
            None,
        );
        let right = fixture_window(WindowId(1), WindowRect::new(48.0, 2.0, 32.0, 21.0), 1, None);
        let windows = [left, right];
        let divider = GroupDivider {
            split_index: 0,
            direction: SplitDirection::Vertical,
            position: 48.0,
            axis_start: 34.0,
            axis_size: 46.0,
            cross_start: 0.0,
            cross_size: 24.0,
        };

        // #1094: this boundary is a *group* boundary (`right.rect.x == 48.0
        // == divider.position`, within the same f64 precision) — excluded
        // from `vertical_separator_cells` regardless of the f32 rounding
        // drift `window_right_edge_cell`'s own `sep_x` would otherwise carry.
        let cells = vertical_separator_cells(&windows, std::slice::from_ref(&divider));
        assert!(
            cells.is_empty(),
            "a pair of windows meeting exactly at a known group-divider \
             position must be left entirely to `group_divider_cells` — got \
             {cells:?}"
        );

        // ...so the group divider paints on every row, unconditionally, at
        // its own `position` (48) — not a rect-derived, rounding-prone
        // column beside it (#481's original failure mode).
        let editor_area = Rect {
            x: 34,
            y: 0,
            width: 46,
            height: 24,
        };
        let div_cells = group_divider_cells(&[divider], &windows, editor_area);
        for y in 1..23u16 {
            assert!(
                div_cells.contains(&(48, y)),
                "row {y}: with `vertical_separator_cells` no longer claiming \
                 this boundary, the group divider must paint every row at \
                 its own position (48); got {div_cells:?}"
            );
        }
    }

    // ── #1097: residual (minimap-off) click-cost profile ────────────────────
    //
    // Manual perf probe, not a regression gate — `#[ignore]`d so it never
    // runs in the normal suite. Opens this repo's own `src/app.rs`
    // (8,800+ lines, real tree-sitter Rust highlighting), forces `:set
    // nominimap`, and times the two things #1097 asked to have measured
    // apart from each other:
    //
    //  * the per-frame layout cost — `build_screen_for_shell_content`, the
    //    actual *live* render path (`TuiShellApp::render_content` calls this
    //    once per redraw; see that function's own doc comment, added by
    //    this issue, for why the pre-existing `build_screen_for_tui` PERF
    //    hook above can never fire on a live run — nothing outside
    //    `#[cfg(test)]` calls that helper at all);
    //  * the syntax/buffer-layer cost — `BufferState::update_syntax_with_limit`'s
    //    two whole-buffer passes, the issue's "unverified lead".
    //
    // Run with:
    //   cargo test --release --lib \
    //     tui_main::render_impl::tests::profile_minimap_off_click_cost \
    //     -- --ignored --nocapture
    #[test]
    #[ignore = "manual perf probe for #1097 — run with --ignored --nocapture"]
    fn profile_minimap_off_click_cost() {
        crate::core::session::suppress_disk_saves();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs");
        let mut e = Engine::new_for_test();
        e.startup_without_session_restore(Some(path.as_path()));
        e.settings.minimap = false;

        let line_count = e.active_buffer_state().buffer.content.len_lines();
        assert!(
            line_count > 8_000,
            "fixture precondition: src/app.rs must still be an 8,000+ line \
             file for this probe to reproduce #1097's repro shape (got \
             {line_count} lines)"
        );
        assert_eq!(
            render::minimap_reserved_width(&e, 200.0, 1.0, render::TUI_MINIMAP_SIZING),
            0.0,
            "scope guard: this probe measures the *minimap-off* residual \
             cost only — if this fails, `:set nominimap` didn't take, and \
             any numbers below are measuring #1096 (already root-caused), \
             not #1097"
        );

        let theme = crate::render::Theme::onedark();
        let tui_backend = super::backend::TuiBackend::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 50,
        };

        // Warm-up: pays any one-time allocation/cache cost outside the
        // timed loop.
        let _ = build_screen_for_shell_content(&e, &theme, area, &tui_backend);

        const N: u32 = 100;
        let t0 = Instant::now();
        for _ in 0..N {
            let _ = std::hint::black_box(build_screen_for_shell_content(
                &e,
                &theme,
                area,
                &tui_backend,
            ));
        }
        let layout_total = t0.elapsed();

        let bid = {
            let wid = e.active_window_id();
            e.windows.get(&wid).unwrap().buffer_id
        };
        let max_lines = crate::core::buffer_manager::syntax_max_lines();
        let t1 = Instant::now();
        for _ in 0..N {
            e.buffer_manager
                .get_mut(bid)
                .unwrap()
                .update_syntax_with_limit(max_lines);
        }
        let syntax_total = t1.elapsed();

        // Third number: the *full* per-frame paint (compose + style-every-
        // visible-cell + write into the ratatui buffer), not just the
        // layout step above — `render_tui_buffer_impl` runs the same
        // `build_screen_for_shell_content` plus the paint walk
        // `TuiShellApp::render_content` performs on every live redraw
        // (`render_all_windows` et al.). Layout alone turned out cheap
        // above; this checks whether the *paint* half of the frame — which
        // the layout-only number can't see — is where a click-driven
        // redraw storm would actually spend its time.
        let _ = render_tui_buffer_impl(&e, area.width, area.height);
        let t2 = Instant::now();
        for _ in 0..N {
            let _ = std::hint::black_box(render_tui_buffer_impl(&e, area.width, area.height));
        }
        let paint_total = t2.elapsed();

        println!(
            "#1097 profile ({line_count}-line file, minimap off, N={N}):\n  \
             build_screen_for_shell_content (layout only):   {:.3}ms/call ({:.1}ms total)\n  \
             render_tui_buffer_impl (layout + full paint):   {:.3}ms/call ({:.1}ms total)\n  \
             update_syntax_with_limit (syntax, full-buffer): {:.3}ms/call ({:.1}ms total)",
            layout_total.as_secs_f64() * 1000.0 / N as f64,
            layout_total.as_secs_f64() * 1000.0,
            paint_total.as_secs_f64() * 1000.0 / N as f64,
            paint_total.as_secs_f64() * 1000.0,
            syntax_total.as_secs_f64() * 1000.0 / N as f64,
            syntax_total.as_secs_f64() * 1000.0,
        );
    }
}
