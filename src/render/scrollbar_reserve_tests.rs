use super::*;

// ─── scrollbar_reserve (#828) ──────────────────────────────────────────

/// #828 acceptance: `build_screen_layout`'s `scrollbar_reserve` is an
/// explicit parameter now, honored exactly as given — not re-derived
/// from `char_width` via the old `if char_width > 1.0 { 8.0 } else {
/// 0.0 }` convention this file used to hardcode at the call site inside
/// `build_rendered_window`.
///
/// RED against that pre-#828 shape: a `char_width == 1.0` call (the old
/// convention's "this is TUI" signal) always mapped to a hardcoded
/// `0.0` reserve, with no way for a caller to say otherwise — there was
/// no `scrollbar_reserve` parameter to pass a non-zero value through in
/// the first place. This pins that a real, non-default
/// `quadraui::Backend::scrollbar_reserve()` answer is now genuinely
/// subtracted regardless of `char_width`, by comparing the painted
/// `text_viewport_cols` with and without a non-zero reserve at
/// `char_width == 1.0`.
#[test]
fn build_screen_layout_honors_an_explicit_scrollbar_reserve_even_at_char_width_one() {
    let mut engine = test_engine(
        "a line of text long enough to fill the whole pane width and then some more text",
    );
    // #1094: minimap off, to isolate `scrollbar_reserve`'s own effect on
    // the viewport from the (new, intentional) interaction where a pane
    // too narrow to afford both the strip and the scroll gutter beside
    // it suppresses the strip instead — this fixture's 40-col pane is
    // narrow enough for a 10-unit reserve to trip exactly that
    // suppression, which would otherwise make `cols_with` reflect a
    // *lost strip*, not the reserve alone.
    engine.settings.minimap = false;
    let theme = Theme::onedark();
    let bounds = WindowRect::new(0.0, 0.0, 40.0, 10.0);
    let (rects, _) = engine.calculate_group_window_rects(bounds, 1.0);

    let without_reserve = build_screen_layout(
        &engine,
        &theme,
        &rects,
        1.0,
        1.0,
        false,
        0.0,
        TUI_MINIMAP_SIZING,
    );
    let with_reserve = build_screen_layout(
        &engine,
        &theme,
        &rects,
        1.0,
        1.0,
        false,
        10.0,
        TUI_MINIMAP_SIZING,
    );

    let cols_without = without_reserve.windows[0].text_viewport_cols;
    let cols_with = with_reserve.windows[0].text_viewport_cols;
    assert_eq!(
        cols_without.saturating_sub(cols_with),
        10,
        "a 10-unit scrollbar_reserve at char_width == 1.0 must shrink \
             the viewport by exactly 10 columns: without={cols_without}, \
             with={cols_with}"
    );
}

/// #1093 acceptance criteria 4 and 5: clicking the vertical middle of the
/// strip seeks to ~50% of the strip's *painted window*, not 50% of the
/// whole file, and the scroll it produces is centred on that line (VS
/// Code parity), not top-aligned. Backend-independent — both backends
/// call exactly this.
///
/// **RED against the pre-#1093 shape:** the old assertion here was
/// `(line as f64 / total).abs() < 0.1` — i.e. "lands near 50% of the
/// whole file" — which is exactly the bug this issue reports (the whole
/// buffer squeezed into the strip on every frame, so 50% of the strip
/// always meant 50% of the file regardless of scroll position).
/// Confirmed by hand: reverting `build_minimap_data`'s windowing (handing
/// `quadraui::primitives::minimap::block_bounds` the whole
/// `total_buffer_lines` again) makes `window_len < total` (this test's
/// own setup-sanity check) fail
/// outright, since the window would once again cover the entire file.
#[test]
fn minimap_click_at_the_middle_seeks_to_the_middle_of_the_painted_window() {
    // #1186: `minimap_engine()`'s 201 lines now fits *entirely* inside
    // this strip's window (compression covers whole small files, which
    // is the point of #1186) — `windowed_minimap_engine` is large enough
    // to stay in #1093's genuine sliding-window regime even after
    // #1186's compression, which is what this test is actually about.
    let mut e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let win_id = screen.windows[0].window_id;
    let mm = screen.minimap.first().expect("minimap present");
    let total = mm.minimap.total_buffer_lines;
    // The real span of buffer lines the window covers — `lines.len()`
    // (a row/block *count*) stopped being a reliable proxy for that the
    // moment #1186 let a block cover more than one buffer line.
    let window_len = mm.minimap.lines.last().unwrap().line_idx + 1 - mm.minimap.lines[0].line_idx;
    assert!(
        window_len < total,
        "test setup sanity: the file must be taller than the strip's \
             own window, or this test cannot distinguish window-relative \
             from whole-file semantics (window_len={window_len}, \
             total={total})"
    );

    let mid_x = mm.rect.x + mm.rect.width / 2.0;
    let mid_y = mm.rect.y + mm.rect.height / 2.0;
    let (hit_win, line) =
        minimap_click_line(&screen, mid_x, mid_y).expect("middle of the strip must hit");
    assert_eq!(
        hit_win, win_id,
        "the hit must resolve to the pane it was clicked in"
    );

    // The cursor starts at the top of the file, so the painted window
    // itself starts at line 0 — a middle click must land near half of
    // *that* window, not half of `total`.
    let window_frac = line as f64 / window_len as f64;
    assert!(
        (window_frac - 0.5).abs() < 0.15,
        "a click at the vertical middle must land near 50% of the \
             painted window ({window_len} lines starting at line 0), got \
             line {line} ({window_frac:.3})"
    );
    let file_frac = line as f64 / total as f64;
    assert!(
        file_frac < 0.4,
        "the click must NOT land near 50% of the whole file — that is \
             the pre-#1093 whole-buffer-squeeze bug: got line {line} of \
             {total} ({file_frac:.3})"
    );

    // …and it actually scrolls the window there, *centred* on the
    // clicked line rather than top-aligned.
    let (scrolled_win, scrolled) =
        apply_minimap_click(&mut e, &screen, mid_x, mid_y).expect("click must be handled");
    assert_eq!(scrolled_win, win_id);
    assert_eq!(scrolled, line);
    let viewport_lines = e
        .windows
        .get(&win_id)
        .map(|w| w.view.viewport_lines)
        .unwrap_or(0);
    assert_eq!(
        e.scroll_top(),
        line.saturating_sub(viewport_lines / 2),
        "the click must centre the viewport on the clicked line \
             (against the painted window's own line, not `total`), not \
             pin it to the very top of the viewport"
    );
}

/// #1093 acceptance criteria 1 and 4: at the top of the file, the
/// strip's bottom row must NOT jump to EOF — the exact repro in the
/// issue ("open a 647-line file at line 1, click the bottom of the
/// strip, the view jumps to EOF"). It must instead land near the end of
/// the painted window, i.e. advance by roughly one strip's worth of
/// file. A point outside the strip must still miss so the caller falls
/// through to normal editor clicks.
///
/// **RED against the pre-#1093 shape:** confirmed by hand — the old
/// assertion (`bottom >= total - total / 10`, "lands in the last tenth
/// of the file") passed on unfixed `develop` and would fail against
/// this fix (`bottom` lands far short of `total`); the new assertions
/// below fail against unfixed `develop` instead, since there `bottom`
/// really is in the file's last tenth regardless of scroll position.
#[test]
fn minimap_click_at_the_bottom_does_not_jump_to_eof() {
    // #1186: see the sibling middle-click test's doc comment — a
    // 201-line file now fits entirely inside this geometry's compressed
    // window, so a much larger fixture is needed to keep exercising
    // #1093's genuine sliding-window regime.
    let e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let win_id = screen.windows[0].window_id;
    let mm = screen.minimap.first().expect("minimap present");
    let total = mm.minimap.total_buffer_lines;
    // Real buffer-line span the window covers — see the sibling test's
    // comment on why `lines.len()` (a row count) is no longer a
    // reliable proxy for this under #1186's multi-line blocks.
    let window_len = mm.minimap.lines.last().unwrap().line_idx + 1 - mm.minimap.lines[0].line_idx;
    assert!(
        window_len < total,
        "test setup sanity: the file must be taller than the strip's \
             own window (window_len={window_len}, total={total})"
    );
    let x = mm.rect.x + 1.0;

    assert_eq!(minimap_click_line(&screen, x, mm.rect.y), Some((win_id, 0)));
    let (_, bottom) = minimap_click_line(&screen, x, mm.rect.y + mm.rect.height - 0.5)
        .expect("bottom of the track must hit");
    assert!(
        bottom < total - total / 10,
        "the bottom of the track must NOT land in the last tenth of \
             the file while the cursor is still at the top — that is the \
             issue's own repro (bottom-of-strip click jumps to EOF): got \
             {bottom} of {total}"
    );
    assert!(
        bottom as f64 >= window_len as f64 * 0.7,
        "the bottom of the track must land near the end of the \
             painted window ({window_len} lines), not far short of it: \
             got {bottom}"
    );

    // One cell to the left of the strip is editor text, not the minimap.
    assert_eq!(
        minimap_click_line(&screen, mm.rect.x - 1.0, mm.rect.y + 5.0),
        None,
        "a point outside the strip must not be treated as a minimap click"
    );
}

/// #1253: `minimap_click_line`/`minimap_press` must resolve against the
/// **paint-time** `MinimapLayout` a prior `draw_minimap_strip` call
/// cached on `RenderedMinimap::resolved_layout`, not re-derive their own
/// via `layout_with_sizing` every click — the cache is only proven wired
/// up if a cached layout whose `bounds` genuinely disagrees with what a
/// fresh `layout_with_sizing(minimap_strip_rect(mm), ...)` call would
/// produce wins the hit-test.
///
/// Stashes a `MinimapLayout` whose `bounds` is shifted 1000px away from
/// the strip's real `mm.rect` (impossible for any real paint to
/// produce — this is a synthetic probe, not a realistic backend
/// scale/pitch difference) directly into `resolved_layout`. A point at
/// the real strip's centre must then MISS (the cached, shifted bounds
/// don't cover it) while the same point offset into the shifted bounds
/// must HIT — the opposite of what a from-scratch `layout_with_sizing`
/// recompute against the real, unshifted `mm.rect` would ever report,
/// so this can only pass if the cached layout is what actually got
/// consulted.
///
/// **RED without the #1253 wiring:** confirmed by hand — reverting
/// `minimap_layout_for_click` to unconditionally call
/// `mm.minimap.layout_with_sizing(minimap_strip_rect(mm), ...)` (the
/// pre-#1253 body of both `minimap_click_line` and `minimap_press`,
/// ignoring `resolved_layout` entirely) makes the real-strip-centre
/// assertion below fail (`Some` where the shifted cache must report
/// `None`) and the shifted-point assertion fail the opposite way.
#[test]
fn minimap_click_and_press_resolve_against_the_cached_paint_time_layout() {
    let e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let win_id = screen.windows[0].window_id;
    let mm = screen.minimap.first().expect("minimap present");

    let real_bounds = minimap_strip_rect(mm);
    let shift = 1000.0_f32;
    let mut shifted_layout = mm.minimap.layout_with_sizing(
        quadraui::Rect::new(
            real_bounds.x + shift,
            real_bounds.y,
            real_bounds.width,
            real_bounds.height,
        ),
        MINIMAP_LINES_PER_ROW,
        quadraui::MinimapSizing::FixedPitch(1.0),
    );
    // Keep `visible_lines` non-empty (needed for a `Seek` hit to resolve
    // a line at all) but leave `bounds` shifted — that's the only field
    // `hit_test` reads.
    assert!(
        !shifted_layout.visible_lines.is_empty(),
        "test setup sanity: the shifted layout must still have rows to resolve a hit against"
    );
    shifted_layout.bounds = quadraui::Rect::new(
        real_bounds.x + shift,
        real_bounds.y,
        real_bounds.width,
        real_bounds.height,
    );
    *mm.resolved_layout.borrow_mut() = Some(shifted_layout);

    let real_mid_x = real_bounds.x as f64 + real_bounds.width as f64 / 2.0;
    let real_mid_y = real_bounds.y as f64 + real_bounds.height as f64 / 2.0;
    assert_eq!(
        minimap_click_line(&screen, real_mid_x, real_mid_y),
        None,
        "a click at the real strip's own centre must MISS once the cache \
             holds a layout whose bounds live 1000px away — proves the \
             recompute-against-mm.rect path is NOT what answered this"
    );

    let shifted_mid_x = real_mid_x + shift as f64;
    let (hit_win, _line) = minimap_click_line(&screen, shifted_mid_x, real_mid_y)
        .expect("a click inside the cached layout's shifted bounds must HIT");
    assert_eq!(hit_win, win_id);

    // `minimap_press` shares the same resolution path — same proof,
    // same shifted-bounds probe.
    assert!(
        minimap_press(&e, &screen, real_mid_x, real_mid_y, false).is_none(),
        "minimap_press must also miss at the real strip's centre once \
             the cache holds the shifted layout"
    );
    assert!(
        minimap_press(&e, &screen, shifted_mid_x, real_mid_y, false).is_some(),
        "minimap_press must hit inside the cached layout's shifted bounds"
    );
}

/// #1187: a press on the viewport-highlight band itself begins a drag
/// that preserves the grab offset — `jump` is unset, and `grab_offset`
/// is the press's own offset from the band's top edge, never `0.0`
/// (which would jump the band's top under the cursor, the "grab
/// anywhere snaps to centre" bug the issue reports).
#[test]
fn minimap_press_on_the_highlight_band_preserves_the_grab_offset() {
    let e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let win_id = screen.windows[0].window_id;
    let mm = screen.minimap.first().expect("minimap present");

    // The cursor starts at the top of the file, so the viewport
    // highlight's own top edge coincides with the strip's top row
    // (#1093). At this fixture's geometry the band is exactly one row
    // tall (`FixedPitch(1.0)`'s own row pitch), so a press has to land
    // within that single row — 0.4 units down is safely inside
    // `[rect.y, rect.y + 1.0)` without being able to round into the row
    // below.
    let x = mm.rect.x + 1.0;
    let y = mm.rect.y + 0.4;
    let press = minimap_press(&e, &screen, x, y, false).expect("the strip must hit");
    assert_eq!(press.window_id, win_id);
    assert!(
        !press.jump,
        "a press 0.4 units below the strip's (and so the band's) own \
             top edge must land inside the highlight band, not on the bare \
             track"
    );
    assert!(
        (press.grab_offset - 0.4).abs() < 0.05,
        "grab_offset must be the press's own offset from the band's top \
             edge (~0.4 here, matching how far below the strip's top the \
             press landed) — got {}",
        press.grab_offset
    );
}

/// #1187: a press on the track *outside* the band must jump-to-position
/// (today's #1093 centring, applied by the caller — see
/// `MinimapPress::jump`'s doc comment) and then continue as a normal
/// thumb drag with `grab_offset: 0.0`, mirroring
/// `resolve_editor_scrollbar_click`'s track-click convention.
#[test]
fn minimap_press_outside_the_band_sets_jump_and_zero_grab_offset() {
    let e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let mm = screen.minimap.first().expect("minimap present");

    // Cursor at the top of the file puts the highlight band at the very
    // top of the strip; the strip's bottom row is far outside it.
    let x = mm.rect.x + 1.0;
    let y = mm.rect.y + mm.rect.height - 1.0;
    let press = minimap_press(&e, &screen, x, y, false).expect("the strip must hit");
    assert!(
        press.jump,
        "a press at the strip's bottom row, with the band pinned to \
             the top, must land outside the band"
    );
    assert_eq!(
        press.grab_offset, 0.0,
        "a track press outside the band must arm the drag with \
             grab_offset 0.0, matching resolve_editor_scrollbar_click's \
             track-click convention"
    );
}

/// #1187's actual fix: `max_scroll` must be the whole file's scroll
/// ceiling (`total_buffer_lines - viewport_lines`, the same arithmetic
/// `View::ensure_cursor_visible` clamps against) — never the strip's own
/// painted window, which is only a fraction of the file once #1093's
/// sliding window is in play. Anchoring to the window instead is exactly
/// the root cause: the window re-slides under a drag in lockstep with
/// `scroll_top`, so the drag's motion cancels itself out and a
/// whole-strip drag reaches only about one window's worth of lines.
#[test]
fn minimap_press_max_scroll_is_the_whole_files_scroll_ceiling() {
    let e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let win_id = screen.windows[0].window_id;
    let mm = screen.minimap.first().expect("minimap present");
    let total = mm.minimap.total_buffer_lines;
    let window_len = mm.minimap.lines.last().unwrap().line_idx + 1 - mm.minimap.lines[0].line_idx;
    assert!(
        window_len < total,
        "test setup sanity: the file must be taller than the strip's \
             own window (window_len={window_len}, total={total})"
    );

    let x = mm.rect.x + 1.0;
    let y = mm.rect.y + 1.0;
    let press = minimap_press(&e, &screen, x, y, false).expect("the strip must hit");
    let viewport_lines = e
        .windows
        .get(&win_id)
        .map(|w| w.view.viewport_lines)
        .unwrap_or(0);
    assert_eq!(
        press.max_scroll,
        total - viewport_lines,
        "max_scroll must be the whole file's scroll ceiling, not the \
             painted window's own (much smaller) length ({window_len})"
    );
}

/// A point outside the strip must not resolve to a press at all, so the
/// caller falls through to normal editor click handling — mirrors
/// `minimap_click_line`'s own negative-space case.
#[test]
fn minimap_press_returns_none_outside_the_strip() {
    let e = windowed_minimap_engine(50_000);
    let screen = render_engine(&e, 120.0, 30.0);
    let mm = screen.minimap.first().expect("minimap present");
    assert_eq!(
        minimap_press(&e, &screen, mm.rect.x - 1.0, mm.rect.y + 5.0, false),
        None
    );
}

/// Shared fixture for the [`fine_seek_geometry`] unit tests below:
/// a synthetic painted window (`base` 1000, `span` 401 — deliberately
/// not a round multiple of the real strip's own height, so a formula
/// that silently degenerated to the coarse, file-wide one would produce
/// a visibly different (and wrong) result rather than an accidental
/// match) inside a `bounds`/`max_scroll` pair distinct from either.
///
/// The returned `thumb_length` is an arbitrary, non-zero stand-in for
/// the real, already-painted `viewport_highlight.height` a live caller
/// would pass (#1828 review: `fine_seek_geometry` no longer re-derives
/// this itself — see its doc comment) — neither test below depends on
/// its actual value, only that it is threaded straight through
/// unmodified into `track_length`.
fn fine_geometry_fixture() -> (quadraui::Rect, quadraui::Minimap, usize, f32) {
    let bounds = quadraui::Rect::new(0.0, 10.0, 5.0, 20.0); // S0 = 10, Sh = 20
    let minimap = quadraui::Minimap {
        id: quadraui::WidgetId::new("mm"),
        lines: vec![
            quadraui::MinimapLine {
                text: String::new(),
                line_idx: 1000, // base
            },
            quadraui::MinimapLine {
                text: String::new(),
                line_idx: 1400, // span = 1400 + 1 - 1000 = 401
            },
        ],
        syntax_spans: Vec::new(),
        visible_row_start: 0,
        visible_row_count: 0,
        total_buffer_lines: 50_000,
    };
    let max_scroll = 40_000; // M
    let thumb_length = 1.5; // arbitrary; see doc comment above
    (bounds, minimap, max_scroll, thumb_length)
}

/// Drive a [`fine_seek_geometry`] result through the real,
/// unmodified `quadraui::dispatch_mouse_drag` — the same call both
/// backends' drag-move handlers make — and read back the
/// `ScrollOffsetChanged` offset it derives at `y`. Proves the geometry
/// this module hands `DragTarget::ScrollbarY` actually produces the
/// offsets [`fine_seek_geometry`]'s own doc comment claims, rather than
/// asserting on the four numbers in isolation and trusting the
/// arithmetic they're fed into.
fn dispatch_offset_at(
    track_start: f32,
    track_length: f32,
    thumb_length: f32,
    max_scroll: usize,
    grab_offset: f32,
    y: f32,
) -> usize {
    let mut drag = quadraui::DragState::default();
    drag.begin(quadraui::DragTarget::ScrollbarY {
        widget: quadraui::WidgetId::new("mm"),
        track_start,
        track_length,
        thumb_length,
        max_scroll,
        grab_offset,
        inverted: false,
    });
    let events =
        quadraui::dispatch_mouse_drag(&drag, quadraui::Point::new(0.0, y), Default::default());
    events
        .into_iter()
        .find_map(|ev| match ev {
            quadraui::UiEvent::ScrollOffsetChanged { new_offset, .. } => Some(new_offset),
            _ => None,
        })
        .expect("a ScrollbarY drag with max_scroll > 0 must emit ScrollOffsetChanged")
}

/// #1271: `fine_seek_geometry`'s `grab_offset` is derived by requiring
/// the mapping to be an **identity at the press point** — dispatching a
/// drag-move at the exact pixel the press happened at, with no
/// movement, must reproduce the current `scroll_top` exactly, not just
/// "close to it". Picks a non-zero, non-round `scroll_top` so a formula
/// that dropped the `(scroll_top / max_scroll) * effective_track` term
/// (leaving `grab_offset` at, say, a hand-rolled `py - band.y`) would
/// visibly miss.
#[test]
fn fine_seek_geometry_grab_offset_is_an_identity_at_the_press_point() {
    let (bounds, minimap, max_scroll, fixture_thumb_length) = fine_geometry_fixture();
    let scroll_top = 12_345;
    let py = 15.0; // inside [S0, S0 + Sh) = [10.0, 30.0)

    let (track_start, track_length, thumb_length, grab_offset) = fine_seek_geometry(
        &bounds,
        &minimap,
        max_scroll,
        fixture_thumb_length,
        scroll_top,
        py,
        true, // in_band: the identity derivation only applies here
    )
    .expect("span and max_scroll are both > 0 in the fixture");

    let new_offset = dispatch_offset_at(
        track_start,
        track_length,
        thumb_length,
        max_scroll,
        grab_offset,
        py,
    );
    assert_eq!(
        new_offset, scroll_top,
        "dispatching at the exact press point (no movement) must \
             reproduce the current scroll_top exactly — that is the \
             identity `grab_offset` is solved for"
    );
}

/// #1271: the virtual track spans exactly the painted window — at the
/// real strip's top (`y = S0`) the derived offset must be `base` (the
/// painted window's first buffer line), and at the real strip's bottom
/// (`y = S0 + Sh`) it must be `base + span` (one past its last line).
/// A formula that fell back to (or leaked) the coarse, file-wide
/// geometry would instead land near `0` and `max_scroll` respectively —
/// visibly different from `base` (1000) and `base + span` (1401) here.
#[test]
fn fine_seek_geometry_endpoints_span_exactly_the_painted_window() {
    let (bounds, minimap, max_scroll, fixture_thumb_length) = fine_geometry_fixture();
    let base = minimap.lines.first().unwrap().line_idx;
    let span = minimap.lines.last().unwrap().line_idx + 1 - base;

    // `in_band: false` here only decides `grab_offset` (pinned to 0.0,
    // the track-click convention) — `track_start`/`track_length`/
    // `thumb_length` don't depend on it, so one call supplies the
    // geometry both endpoint checks below drive.
    let (track_start, track_length, thumb_length, grab_offset) = fine_seek_geometry(
        &bounds,
        &minimap,
        max_scroll,
        fixture_thumb_length,
        0,
        bounds.y,
        false,
    )
    .expect("span and max_scroll are both > 0 in the fixture");
    assert_eq!(grab_offset, 0.0, "test setup: track press, not a band grab");

    let top_offset = dispatch_offset_at(
        track_start,
        track_length,
        thumb_length,
        max_scroll,
        grab_offset,
        bounds.y,
    );
    assert_eq!(
        top_offset, base,
        "the virtual track's own top (y = S0) must resolve to the \
             painted window's first buffer line ({base})"
    );

    let bottom_offset = dispatch_offset_at(
        track_start,
        track_length,
        thumb_length,
        max_scroll,
        grab_offset,
        bounds.y + bounds.height,
    );
    assert_eq!(
        bottom_offset,
        base + span,
        "the virtual track's own bottom (y = S0 + Sh) must resolve to \
             one past the painted window's last buffer line ({base} + {span})"
    );
}

/// #1187/#722: `Engine::activate_window` — what the minimap press rung's
/// non-jump (in-band) branch calls instead of the jump branch's
/// `apply_minimap_click` — must switch which window is active without
/// moving that (or any) window's cursor or scroll position. This is the
/// engine-level contract both backends' click/mouse handlers rely on;
/// see the GTK driver-tier acceptance
/// (`gtk::testing::minimap::press_inside_a_background_panes_highlight_band_still_focuses_it_on_gtk`)
/// for the end-to-end proof through a real press.
#[test]
fn activate_window_switches_the_active_pane_without_moving_cursor_or_scroll() {
    let mut e = minimap_engine();
    e.split_window(crate::core::window::SplitDirection::Vertical, None);
    let active_before = e.active_window_id();
    let other = *e
        .windows
        .keys()
        .find(|&&w| w != active_before)
        .expect("a vsplit must produce a second window");

    let scroll_before = e.windows.get(&other).unwrap().view.scroll_top;
    let cursor_before = e.windows.get(&other).unwrap().view.cursor;

    e.activate_window(other);

    assert_eq!(
        e.active_window_id(),
        other,
        "activate_window must make the named window active"
    );
    assert_eq!(
        e.windows.get(&other).unwrap().view.scroll_top,
        scroll_before,
        "activate_window must not move the window's scroll position"
    );
    assert_eq!(
        e.windows.get(&other).unwrap().view.cursor,
        cursor_before,
        "activate_window must not move the window's cursor"
    );
}

/// With the setting off there is nothing to click — the editor keeps the
/// full width and clicks in that column resolve as normal text clicks.
#[test]
fn minimap_click_is_a_no_op_when_the_setting_is_off() {
    let mut e = minimap_engine();
    e.settings.minimap = false;
    let screen = render_engine(&e, 120.0, 30.0);
    assert_eq!(minimap_click_line(&screen, 115.0, 15.0), None);
    assert_eq!(apply_minimap_click(&mut e, &screen, 115.0, 15.0), None);
}

/// The sampled lines and aggregated spans are quadraui's output, keyed
/// back to real buffer lines — a transposed or empty sample would show up
/// here before it reaches a snapshot.
///
/// #1093: renamed from `minimap_samples_the_whole_buffer_in_order` — the
/// strip now holds a *window*, not the whole buffer, so the claim in the
/// old name is no longer true (the window merely happens to start at
/// line 0 here, since the cursor starts at the top of the file). The
/// in-order/strictly-increasing assertions below are unchanged; a
/// `window_len < total_buffer_lines` check is added so this stays
/// honest about no longer covering the whole file.
#[test]
fn minimap_window_samples_in_order_starting_at_the_top() {
    let e = minimap_engine();
    let screen = render_engine(&e, 120.0, 30.0);
    let mm = &screen.minimap.first().expect("minimap present").minimap;
    assert_eq!(
        mm.total_buffer_lines, 201,
        "200 lines plus the trailing one"
    );
    assert!(!mm.lines.is_empty());
    assert!(
        mm.lines.len() < mm.total_buffer_lines,
        "test setup sanity: the file must be taller than the strip's \
             own window, or this test can't distinguish a window from the \
             pre-#1093 whole-buffer sample (window={}, total={})",
        mm.lines.len(),
        mm.total_buffer_lines
    );
    assert!(
        mm.lines.windows(2).all(|w| w[0].line_idx < w[1].line_idx),
        "sampled buffer line indices must be strictly increasing"
    );
    assert_eq!(mm.lines[0].line_idx, 0, "sampling starts at the first line");
    // The indented middle of the file must survive sampling as indented
    // text — this is the shape the TUI braille snapshot pins.
    assert!(
        mm.lines
            .iter()
            .any(|l| l.line_idx >= 40 && l.line_idx < 160 && l.text.starts_with("            ")),
        "the deeply-indented middle band must appear in the sample"
    );
}

/// #1093 acceptance criterion 1: a file several times taller than the
/// strip, cursor at line 1 — the strip's last painted row's `line_idx`
/// must be well short of `total_buffer_lines`. This is the issue's own
/// repro (`src/app_support.rs`, 647 lines, TUI minimap, cursor at line
/// 1: "the strip's bottom row is line ~647").
///
/// **RED against unfixed `develop`:** the pre-#1093 shape squeezed the
/// whole buffer into the strip on every frame, so the last painted row
/// was always `total_buffer_lines - 1` regardless of scroll position —
/// this exact assertion (`last_line_idx < total - total / 4`) fails
/// against that shape at any scroll position, including the top.
#[test]
fn minimap_window_stays_short_of_eof_when_scrolled_to_the_top() {
    // #1211: `K` (hence the window's own length) is now a function of
    // this call's geometry alone (`editor_visible_rows`, `target_lines`
    // == 160 here) — never of the buffer's own length — so any file
    // longer than that geometry-only window still cannot cover the
    // whole file, and 20,000 lines keeps exercising #1093's genuine
    // sliding-window regime exactly as it always has.
    let e = large_minimap_engine(20_000);
    let theme = Theme::onedark();
    let wid = e.active_window_id();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
    let mm = build_minimap_data(&e, &theme, wid, rect, 1.0, 40)
        .expect("minimap must build")
        .minimap;

    assert_eq!(mm.lines[0].line_idx, 0, "cursor starts at the top");
    let last_line_idx = mm.lines.last().unwrap().line_idx;
    assert!(
        last_line_idx < mm.total_buffer_lines - mm.total_buffer_lines / 4,
        "the strip's last painted row (line {last_line_idx} of \
             {}) must be well short of the end of the file while the \
             cursor is at the top — a full-length map would paint the \
             file's last line here on every frame",
        mm.total_buffer_lines
    );
}

/// #1093 acceptance criterion 2: the same file scrolled to the bottom —
/// the strip's last painted row **is** the last line of the file, and
/// its first row is not line 0 (both ends of the file are reachable by
/// scrolling, VS Code's `minimap.size: proportional`).
#[test]
fn minimap_window_reaches_eof_when_scrolled_to_the_bottom() {
    // #1211: see the sibling "stays short of eof" test's comment — the
    // window's length is geometry-only, so a 20,000-line buffer stays
    // outside it at this rect's `target_lines` (160) and the window
    // still has to slide.
    let mut e = large_minimap_engine(20_000);
    let theme = Theme::onedark();
    let wid = e.active_window_id();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
    const EDITOR_VISIBLE_ROWS: usize = 40;

    let total_buffer_lines = {
        let state = e.active_buffer_state_mut();
        state.buffer.content.len_lines()
    };
    let max_scroll_top = total_buffer_lines - EDITOR_VISIBLE_ROWS;
    if let Some(w) = e.windows.get_mut(&wid) {
        w.view.scroll_top = max_scroll_top;
    }

    let mm = build_minimap_data(&e, &theme, wid, rect, 1.0, EDITOR_VISIBLE_ROWS)
        .expect("minimap must build")
        .minimap;

    // #1186: the last `lines` entry is now a *block's* starting line,
    // not necessarily the file's literal last line — a block can cover
    // several real lines (`MINIMAP_MAX_COMPRESSION` at most here, since
    // 20,000 lines sits well past the compression ceiling), so "reaches
    // EOF" means the last block's own range covers `total_buffer_lines
    // - 1`, i.e. its start is within one block-width of the end.
    let last_line_idx = mm.lines.last().unwrap().line_idx;
    assert!(
        total_buffer_lines - last_line_idx <= MINIMAP_MAX_COMPRESSION,
        "scrolled to the bottom, the strip's last painted block (starting \
             at line {last_line_idx} of {total_buffer_lines}) must reach the \
             file's actual last line, within one block's own width"
    );
    assert!(
        mm.lines[0].line_idx > 0,
        "scrolled to the bottom, the strip's first painted row must \
             not still be line 0 — the window must have slid"
    );
}

/// #1211 acceptance: the scale (buffer lines per painted row) is a
/// function of the strip's own geometry (`editor_visible_rows`,
/// `target_lines`) — **never** of `total_buffer_lines`. #1186 derived
/// `K` from `total_buffer_lines.div_ceil(target_lines)`, which squeezed
/// the *whole file* into the strip for every file shorter than
/// `MINIMAP_MAX_COMPRESSION * target_lines` (essentially every real
/// file), disabling #1093's slide — this test locks in the fix: two
/// files of wildly different lengths, same strip geometry, must land on
/// the *identical* scale, and neither shows through to EOF while
/// scrolled to the top (both must still slide to reach it).
///
/// **RED against unfixed `develop`:** confirmed by hand — restoring the
/// pre-#1211 `k = total_buffer_lines.div_ceil(target_lines)` makes
/// `step(&mm_medium)` (1,500 lines) come out to `10` and
/// `step(&mm_huge)` (50,000 lines) come out to `64` (the
/// `MINIMAP_MAX_COMPRESSION` clamp binding) — different from each other,
/// failing the `assert_eq!` below — and `mm_medium` reaches EOF from the
/// top (the whole 1,500-line file fit in one window), failing the
/// "must NOT reach EOF" assertion for the medium fixture.
#[test]
fn minimap_scale_is_constant_across_file_length_not_derived_from_it() {
    let theme = Theme::onedark();
    // `target_lines` at this rect geometry (pinned by the sibling
    // windowing tests' own comments): 160.
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);

    let short = large_minimap_engine(50);
    let medium = large_minimap_engine(1_500);
    let huge = large_minimap_engine(50_000);
    let wid_short = short.active_window_id();
    let wid_medium = medium.active_window_id();
    let wid_huge = huge.active_window_id();

    let mm_short = build_minimap_data(&short, &theme, wid_short, rect, 1.0, 40)
        .expect("minimap must build for the short fixture")
        .minimap;
    let mm_medium = build_minimap_data(&medium, &theme, wid_medium, rect, 1.0, 40)
        .expect("minimap must build for the medium fixture")
        .minimap;
    let mm_huge = build_minimap_data(&huge, &theme, wid_huge, rect, 1.0, 40)
        .expect("minimap must build for the huge fixture")
        .minimap;

    let step = |mm: &quadraui::Minimap| mm.lines[1].line_idx - mm.lines[0].line_idx;

    // A file shorter than `target_lines` needs no compression at all —
    // unaffected by this fix, kept as a sibling floor.
    assert_eq!(
        step(&mm_short),
        1,
        "a file shorter than target_lines must sample one buffer line \
             per painted row"
    );

    // The core #1211 property: 1,500 lines and 50,000 lines, same
    // geometry, must produce the exact same scale — `K` is a function
    // of the strip, not the file.
    assert_eq!(
        step(&mm_medium),
        step(&mm_huge),
        "the painted lines-per-row must be identical for a 1,500-line \
             and a 50,000-line file at the same strip geometry — got {} vs \
             {}; a scale that differs by file length means K is still \
             derived from total_buffer_lines",
        step(&mm_medium),
        step(&mm_huge)
    );
    assert!(
        step(&mm_medium) > 1 && step(&mm_medium) <= MINIMAP_MAX_COMPRESSION,
        "the shared scale must compress (TUI-shaped geometry: more than \
             one buffer line per row) but stay within the safety ceiling — \
             got {}",
        step(&mm_medium)
    );

    // Neither file's window may reach EOF while scrolled to the top —
    // both are longer than the (geometry-only) window, so #1093's slide
    // must still be required to reach the end, for the small file just
    // as much as the huge one.
    for (label, mm) in [("medium", &mm_medium), ("huge", &mm_huge)] {
        assert_eq!(
            mm.lines[0].line_idx, 0,
            "{label} fixture must start at the top"
        );
        assert!(
            mm.lines.last().unwrap().line_idx < mm.total_buffer_lines - 1,
            "{label} fixture must NOT reach EOF while scrolled to the \
                 top — the window still has to slide (last painted line \
                 {} of {})",
            mm.lines.last().unwrap().line_idx,
            mm.total_buffer_lines
        );
    }
}

/// #1093 acceptance criterion 6: a file that fits entirely within the
/// strip's own capacity must still paint top-to-bottom with no window —
/// no regression to the pre-#1093 behaviour for the common case where
/// the whole file already fits.
#[test]
fn minimap_window_is_the_whole_file_when_it_fits_the_strip() {
    let e = large_minimap_engine(10);
    let theme = Theme::onedark();
    let wid = e.active_window_id();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
    let mm = build_minimap_data(&e, &theme, wid, rect, 1.0, 40)
        .expect("minimap must build")
        .minimap;

    assert_eq!(
        mm.lines.len(),
        mm.total_buffer_lines,
        "a file shorter than the strip's own capacity must show every \
             line, not a partial window"
    );
    assert_eq!(mm.lines[0].line_idx, 0);
    assert_eq!(mm.lines.last().unwrap().line_idx, mm.total_buffer_lines - 1);
}
