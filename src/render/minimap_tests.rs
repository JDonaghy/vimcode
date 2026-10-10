use super::*;

// ─── Minimap (#35, #722) ────────────────────────────────────────────

/// A file with a distinctive indentation shape — deep on the inside,
/// flush at the edges — so a transposed dot grid would be obvious.
pub(crate) fn minimap_engine() -> Engine {
    let mut text = String::new();
    for i in 0..200 {
        let depth = if (40..160).contains(&i) { 3 } else { 0 };
        text.push_str(&"    ".repeat(depth));
        text.push_str(&format!("line {i} content\n"));
    }
    let mut e = test_engine(&text);
    // #1858: the minimap is experimental and off by default on every
    // backend now — this fixture exists specifically to exercise it, so
    // turn it on explicitly rather than lean on the (now-off) default.
    e.settings.minimap = true;
    e
}

/// A large, syntax-free buffer used to force the strip into #1093's
/// genuine sliding-window regime even under #1186's compression: `n` is
/// picked per call site to comfortably exceed
/// `MINIMAP_MAX_COMPRESSION * target_lines` for that call's own strip
/// geometry, so `build_minimap_data` cannot show the whole file no
/// matter how generous `K_max` is. Tests using this fixture are
/// specifically about the windowed case (file too large to ever fully
/// fit), as distinct from the compressed-but-whole-file case #1186
/// introduced — see `minimap_scale_grows_to_show_the_whole_file_when_it_fits_the_compression_ceiling`
/// for that one.
pub(crate) fn windowed_minimap_engine(n: usize) -> Engine {
    let mut text = String::with_capacity(n * 14);
    for i in 0..n {
        text.push_str(&format!("line {i} content\n"));
    }
    let mut e = test_engine(&text);
    // #1858: the minimap is experimental and off by default on every
    // backend now — this fixture exists specifically to exercise it, so
    // turn it on explicitly rather than lean on the (now-off) default.
    e.settings.minimap = true;
    e
}

/// A synthetic file large enough to make an O(buffer) per-frame cost
/// visible: `n_lines` lines, none of them trivially short (so a full
/// buffer-wide `String` materialisation actually does real allocation
/// work, not just touch thousands of empty strings).
///
/// #1096 (repairing the #728 guard's blind spot 2): installs a real
/// `Syntax` and forces a full reparse, so `buffer_state.highlights` is
/// actually populated — `test_engine` alone leaves `syntax: None` (no
/// filetype), which is exactly how the #1096 regression (an unbounded
/// per-highlighted-line `String` cache in `build_minimap_data`'s
/// highlight-mapping loop) went unexercised by every test using this
/// fixture: with `highlights` empty, that loop's body never ran at all.
/// The text itself (`fn line_N() { do_something(N); }`) was already
/// valid enough tree-sitter-Rust input to produce dense, realistic
/// highlight spans once a language is actually attached.
pub(crate) fn large_minimap_engine(n_lines: usize) -> Engine {
    let mut text = String::with_capacity(n_lines * 24);
    for i in 0..n_lines {
        text.push_str(&format!("fn line_{i}() {{ do_something({i}); }}\n"));
    }
    let mut e = test_engine(&text);
    let state = e.active_buffer_state_mut();
    state.syntax = Some(crate::core::syntax::Syntax::new_for_language(
        crate::core::syntax::SyntaxLanguage::Rust,
    ));
    // Explicit, generous limit rather than `update_syntax()`'s
    // process-wide `SYNTAX_MAX_LINES` atomic — other tests write that
    // atomic (see `test_syntax_max_lines_gate`'s own doc comment), so
    // reading it here would make this fixture's highlight population
    // racy under `cargo test`'s default parallelism.
    state.update_syntax_with_limit(n_lines.saturating_add(1));
    assert!(
        !state.highlights.is_empty(),
        "fixture must produce real highlights or the #1096 regression \
             class (unbounded per-line allocation in the highlight-mapping \
             loop) goes unexercised again"
    );
    e
}

/// Time `frames` simulated wheel-scroll frames of `build_minimap_data`
/// over `e`, advancing `scroll_top` each frame so the sampled window
/// keeps moving — exactly what sustained wheel scroll does. Returns the
/// accumulated time spent *inside* `build_minimap_data` only (engine
/// setup and the `scroll_top` write are outside the timed region).
fn time_minimap_frames(e: &mut Engine, n_lines: usize, frames: usize) -> std::time::Duration {
    let wid = e.active_window_id();
    let theme = Theme::onedark();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
    let mut total = std::time::Duration::ZERO;
    for i in 0..frames {
        if let Some(w) = e.windows.get_mut(&wid) {
            w.view.scroll_top = i % n_lines;
        }
        let t0 = std::time::Instant::now();
        let mm = build_minimap_data(e, &theme, wid, rect, 1.0, 40);
        total += t0.elapsed();
        assert!(mm.is_some(), "minimap must build for every simulated frame");
    }
    total
}

/// #728 performance acceptance: wheel-scrolling a big file must not cost
/// O(buffer) per frame. `build_minimap_data` used to allocate a `String`
/// for *every* line in the buffer on every call regardless of how many
/// it actually samples (`quadraui::sample_lines` only keeps
/// ~`target_lines`, but the old code built the whole buffer as
/// candidates first).
///
/// **This asserts on a *ratio*, not a wall-clock ceiling.** The same
/// scroll workload is run over a 1,000-line buffer and a 10,000-line
/// one — 10x the buffer, identical strip geometry, so identical
/// `target_lines` — and the per-frame costs are compared. The two
/// measurements are *interleaved* frame-by-frame so a burst of CPU
/// contention (the full suite runs these tests in parallel with ~2,300
/// others) inflates both sides equally and cancels out of the ratio.
///
/// The earlier version of this test pinned a 500ms absolute budget for
/// 300 frames; the fixed cost on an idle box is ~405ms, so the margin
/// was ~20% and it flaked at 517–586ms under full-suite contention
/// while passing in isolation. Absolute timings cannot be made
/// contention-proof; the ratio can, and it is what the test name
/// actually claims.
///
/// Measured on this machine (debug `cargo test --no-default-features
/// --bin vcd`), per-frame cost over a 10,000-line file:
///   - before (whole-buffer `owned: Vec<String>` every frame):
///     **~80.8ms/frame** — unusable under sustained wheel scroll,
///     matching the issue's report, and ~10x the 1,000-line cost
///     because the work is linear in buffer size.
///   - after (index-first sampling, only ~`target_lines` lines fetched
///     from the rope per frame): **~1.34ms/frame**, i.e. ~1x the
///     1,000-line cost — the cost tracks `target_lines` (the strip's
///     own display rows), not the buffer.
///
/// So the discriminator is ~1x (fixed) vs ~10x (linear); the 4x
/// threshold below sits between them with generous room on both sides.
///
/// #1096 repaired this guard's own blind spot: `large_minimap_engine`
/// now installs real Rust highlights (see that fixture's doc comment),
/// so this same measurement now also covers the highlight-mapping loop
/// — which is exactly where #1096's regression lived, and which the
/// unhighlighted fixture used to skip entirely. Re-measured after that
/// fix, same machine/build, same 1,000- vs 10,000-line/150-frame setup:
///   - #1096 regression, allocation only removed (highlight loop still
///     scans every entry in `buffer_state.highlights`, itself O(buffer)):
///     up to **~3,167ms/frame** at 10,000 lines, ratio up to ~9.9x —
///     failed this guard outright.
///   - #1096 fix (binary-search each sampled line directly into the
///     sorted `highlights` vec, `partition_point`, instead of scanning
///     it): **~34-44ms/frame**, ratio ~1.3x. The residual ~22ms/frame
///     floor (measured with `highlights` forced empty, same fixture) is
///     #1085's own block-aggregation cost — inherent to that issue's
///     correctness fix, unrelated to highlighting, and out of this
///     issue's scope (see #1093, which replaces this whole sampling
///     strategy, and #1097, the separate non-minimap CPU cost).
#[test]
fn minimap_scroll_does_not_scale_with_buffer_size() {
    const SMALL_LINES: usize = 1_000;
    const LARGE_LINES: usize = 10_000;
    const FRAMES: usize = 150;

    let mut small = large_minimap_engine(SMALL_LINES);
    small.settings.minimap = true;
    let mut large = large_minimap_engine(LARGE_LINES);
    large.settings.minimap = true;

    // Warm both sides (first-touch page faults, allocator growth) so the
    // ratio measures steady-state work rather than one-time setup.
    time_minimap_frames(&mut small, SMALL_LINES, 5);
    time_minimap_frames(&mut large, LARGE_LINES, 5);

    // Interleaved: alternate one small frame and one large frame so both
    // series see the same scheduling weather.
    let mut small_total = std::time::Duration::ZERO;
    let mut large_total = std::time::Duration::ZERO;
    for _ in 0..FRAMES {
        small_total += time_minimap_frames(&mut small, SMALL_LINES, 1);
        large_total += time_minimap_frames(&mut large, LARGE_LINES, 1);
    }

    let small_ms = small_total.as_secs_f64() * 1000.0 / FRAMES as f64;
    let large_ms = large_total.as_secs_f64() * 1000.0 / FRAMES as f64;
    let ratio = large_ms / small_ms.max(f64::MIN_POSITIVE);
    eprintln!(
        "minimap_scroll_does_not_scale_with_buffer_size: {FRAMES} frames \
             each — {SMALL_LINES} lines {small_ms:.4}ms/frame, {LARGE_LINES} \
             lines {large_ms:.4}ms/frame, ratio {ratio:.2}x"
    );

    assert!(
        ratio < 4.0,
        "a 10x bigger buffer cost {ratio:.2}x more per minimap frame \
             ({SMALL_LINES} lines: {small_ms:.4}ms/frame, {LARGE_LINES} \
             lines: {large_ms:.4}ms/frame) — the per-frame cost must track \
             the strip's display rows, not the buffer; this smells like a \
             return of the whole-buffer materialisation"
    );
}

/// #1096 regression guard, repairing both of the #728 guard's blind
/// spots at once:
///
/// - **Blind spot 1** (`minimap_scroll_does_not_scale_with_buffer_size`
///   asserts a *ratio*, not a ceiling): a same-geometry constant-factor
///   regression multiplies both sides of that ratio equally and cancels
///   out exactly — which is what let #1096 land invisibly (8x more line
///   reads, ~20x more surviving highlight spans, at unchanged
///   `target_lines`). This test instead counts real work
///   (`MINIMAP_LINE_FETCH_COUNT`, incremented once per
///   `minimap_line_text` call) and checks it against a formula derived
///   from `target_lines`, not against a sibling run's own — possibly
///   also regressed — cost.
/// - **Blind spot 2** (that test's fixture never set a filetype, so
///   `highlights` stayed empty and the loop #1096 regressed never ran):
///   `large_minimap_engine` now installs a real `Syntax` and asserts
///   its own highlights are non-empty (see that fixture's doc comment).
///
/// Both buffers use blocks large enough to saturate
/// `quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP` (`n_lines /
/// target_lines` well over the cap in both cases), so both should hit
/// the exact same fetch
/// ceiling regardless of the 10x difference in buffer size — the #728
/// invariant this test exists to pin, stated as a number instead of a
/// ratio.
///
/// **RED against the unfixed highlight-mapping loop:** confirmed by
/// hand — reinstating the per-call `line_text_cache: HashMap<usize,
/// String>` the loop used before this fix makes `large_fetches` grow
/// with the highlighted line count (~buffer size) instead of staying
/// pinned to the block-sampling ceiling, so `large_fetches >
/// small_fetches` (and both blow past `ceiling`) once `n_lines` is
/// large enough that most highlighted lines aren't themselves block
/// starts. Reverted before landing this test.
#[test]
fn minimap_line_fetch_count_tracks_target_lines_not_buffer_size() {
    const SMALL_LINES: usize = 2_000;
    const LARGE_LINES: usize = 20_000;

    let small = large_minimap_engine(SMALL_LINES);
    let large = large_minimap_engine(LARGE_LINES);
    let theme = Theme::onedark();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
    let wid_small = small.active_window_id();
    let wid_large = large.active_window_id();

    let small_fetches = count_minimap_line_fetches(|| {
        let mm = build_minimap_data(&small, &theme, wid_small, rect, 1.0, 40);
        assert!(mm.is_some(), "minimap must build for the small fixture");
    });
    let large_fetches = count_minimap_line_fetches(|| {
        let mm = build_minimap_data(&large, &theme, wid_large, rect, 1.0, 40);
        assert!(mm.is_some(), "minimap must build for the large fixture");
    });

    assert_eq!(
        small_fetches, large_fetches,
        "line fetches must depend only on strip geometry, not buffer \
             length ({SMALL_LINES}-line buffer: {small_fetches} fetches, \
             {LARGE_LINES}-line buffer: {large_fetches} fetches) — a \
             mismatch means some path (most likely the highlight-mapping \
             loop) is scanning proportionally to the buffer again"
    );

    // The ceiling itself, computed the same way `build_minimap_data`
    // derives `target_lines` at this rect/line_height, so this stays in
    // lockstep with that formula rather than hardcoding a number that
    // could silently drift from it.
    let display_rows = 40usize;
    let gtk_row_capacity =
        (rect.height / quadraui::primitives::minimap::ROW_PITCH_PX).floor() as usize;
    let target_lines = display_rows
        .saturating_mul(MINIMAP_LINES_PER_ROW)
        .max(gtk_row_capacity)
        .max(1);
    let cap = quadraui::primitives::minimap::BLOCK_LINE_SAMPLE_CAP;
    let ceiling = target_lines * cap;
    assert!(
        small_fetches <= ceiling,
        "expected at most {ceiling} line fetches (target_lines=\
             {target_lines} * BLOCK_LINE_SAMPLE_CAP={cap}), got \
             {small_fetches} — the #728 invariant (bounded by strip size, \
             not buffer length) no longer holds"
    );
}

/// Acceptance: `:set nominimap` must widen the editor text area by
/// *exactly* the reserved width, and `:set minimap` must give it back.
/// Asserted on the rendered `text_viewport_cols`, not on the setting.
/// The expected delta is computed through `minimap_reserved_width`
/// itself (the single source of truth `build_screen_layout` also
/// reads) rather than a hardcoded column count, since #722 made that
/// width a function of pane width instead of a fixed constant.
#[test]
fn nominimap_widens_the_editor_by_exactly_the_reserved_width() {
    let mut e = minimap_engine();

    e.settings.minimap = true;
    let with = render_engine(&e, 120.0, 30.0);
    let cols_with = with.windows[0].text_viewport_cols;
    assert_eq!(
        with.minimap.len(),
        1,
        "the minimap must be present when the setting is on"
    );
    // #1094: `render_engine` is TUI-shaped (`scrollbar_reserve == 0.0`,
    // `char_width == 1.0`), so the strip now also claims the one-column
    // gutter that keeps its own boundary clear of the pane's outermost
    // (scroll) column — see `scroll_gutter_width`'s doc comment. Turning
    // the minimap off reclaims that sliver along with the strip itself.
    //
    // `.ceil()`, not a bare `as usize` truncation (#1326 review): a
    // 120-wide pane's `minimap_reserved_width` happens to be exactly
    // `18.0` here (`120 * MINIMAP_WIDTH_FRACTION`), so truncating vs.
    // ceiling this specific fixture's width was never distinguishable —
    // but `build_screen_layout`'s own `render_viewport_cols` computation
    // (`floor(rect.width - minimap_w)`, `minimap_w` a continuous
    // `f64`) reclaims `ceil(minimap_w)` columns whenever `minimap_w`
    // isn't already a whole number, not `floor(minimap_w)`. See the
    // split-pane test below, whose 79-column pane is the first fixture
    // in this file to actually exercise a fractional `minimap_w`.
    let expected_cols = (minimap_reserved_width(&e, 120.0, 1.0, TUI_MINIMAP_SIZING, 0.0)
        + scroll_gutter_width(0.0, 1.0))
    .ceil() as usize;

    e.settings.minimap = false;
    let without = render_engine(&e, 120.0, 30.0);
    let cols_without = without.windows[0].text_viewport_cols;
    assert!(
        without.minimap.is_empty(),
        "`:set nominimap` must remove the minimap from the layout"
    );

    assert_eq!(
        cols_without - cols_with,
        expected_cols,
        "turning the minimap off must hand the editor back exactly the \
             reserved width, strip plus its scroll gutter (with={cols_with}, \
             without={cols_without}, expected={expected_cols})"
    );
}

/// #722 acceptance: `:set nominimap` reclaims exactly the reserved width
/// in *every* pane of a split, not just a single active one — extending
/// the test above to `:vsplit`.
#[test]
fn nominimap_widens_every_pane_in_a_split_by_exactly_its_reserved_width() {
    let mut e = minimap_engine();
    e.split_window(SplitDirection::Vertical, None);

    e.settings.minimap = true;
    let with = render_engine(&e, 160.0, 30.0);
    assert_eq!(with.windows.len(), 2, "vsplit must produce two windows");
    assert_eq!(
        with.minimap.len(),
        2,
        "both panes must carry their own minimap when the setting is on"
    );

    e.settings.minimap = false;
    let without = render_engine(&e, 160.0, 30.0);
    assert!(
        without.minimap.is_empty(),
        "`:set nominimap` must remove every pane's minimap"
    );
    // `minimap_reserved_width` reads `engine.settings.minimap` itself —
    // put it back on before using `e` to recompute what each pane
    // *would* reserve, or every expectation below collapses to 0.
    e.settings.minimap = true;

    for w_with in &with.windows {
        let w_without = without
            .windows
            .iter()
            .find(|w| w.window_id == w_with.window_id)
            .expect("window set must be identical with/without the minimap");
        // #1094: `w_with.rect.width`/`w_without.rect.width` are now the
        // pane's own (un-narrowed either way) width — the strip no
        // longer narrows `rect` itself, only the text-column count — so
        // either carries the same pane width `minimap_reserved_width`
        // expects. The expected reclaim adds `scroll_gutter_width`'s
        // one-column TUI gutter to the strip's own raw width, same as
        // the single-window test above — `.ceil()`'d for the same
        // reason (#1326 review, see that test's doc comment): a
        // `:vsplit`'s two panes need not split evenly (#1326 reserves
        // one column for the divider bar itself, so a fresh 160-wide
        // `<C-w>v` here is 80/79, not 80/80), and an odd pane width
        // like 79 gives `minimap_reserved_width` a genuinely
        // fractional result (`79 * MINIMAP_WIDTH_FRACTION = 11.85`) —
        // this is the fixture that first exercises that fractional
        // case, where truncating instead of ceiling disagreed with
        // `build_screen_layout`'s real reclaimed width by exactly one
        // column.
        let expected_cols =
            (minimap_reserved_width(&e, w_without.rect.width, 1.0, TUI_MINIMAP_SIZING, 0.0)
                + scroll_gutter_width(0.0, 1.0))
            .ceil() as usize;
        assert_eq!(
            w_without.text_viewport_cols - w_with.text_viewport_cols,
            expected_cols,
            "pane {:?} must regain exactly its own reserved width \
                 (with={}, without={}, expected={expected_cols})",
            w_with.window_id,
            w_with.text_viewport_cols,
            w_without.text_viewport_cols
        );
    }
}

/// #722 review follow-up (non-blocking finding): clicking a
/// **background** pane's minimap must focus that pane, not just
/// scroll/reposition its cursor while leaving focus (and keyboard
/// input) on whichever pane was already active. Before #722 this could
/// never come up — only the active pane had a minimap at all — so
/// there was no "click a strip that isn't the focused pane's" case to
/// get wrong until every pane got its own strip.
#[test]
fn minimap_click_on_a_background_pane_focuses_that_pane() {
    let mut e = minimap_engine();
    e.split_window(SplitDirection::Vertical, None);
    let screen = render_engine(&e, 160.0, 30.0);
    assert_eq!(
        screen.minimap.len(),
        2,
        "vsplit must give each pane a minimap"
    );

    let active_before = e.active_window_id();
    let background_mm = screen
        .minimap
        .iter()
        .find(|m| m.window_id != active_before)
        .expect("the split must have a non-active pane with its own minimap");
    let mid_x = background_mm.rect.x + background_mm.rect.width / 2.0;
    let mid_y = background_mm.rect.y + background_mm.rect.height / 2.0;

    let (hit_win, _line) =
        apply_minimap_click(&mut e, &screen, mid_x, mid_y).expect("click must be handled");
    assert_eq!(
        hit_win, background_mm.window_id,
        "test setup sanity: the click must resolve to the background pane"
    );
    assert_ne!(
        hit_win, active_before,
        "test setup sanity: the clicked pane must actually have been \
             the non-active one"
    );
    assert_eq!(
        e.active_window_id(),
        background_mm.window_id,
        "clicking a background pane's minimap must focus that pane, \
             not just scroll it while leaving focus on {active_before:?}"
    );
}

/// #1292 review (blocking finding): `apply_gutter_action`'s `DiffPeek`,
/// `DiagnosticHover` and `CodeAction` arms used to assign
/// `engine.active_tab_mut().active_window = window_id` directly instead
/// of routing through `Tab::focus_window`, so clicking a gutter icon
/// (diagnostic squiggle, code-action lightbulb, or diff-peek marker) on
/// a *background* split pane moved focus there but left
/// `Tab::prev_window` stale — a following `CTRL-W p` would then recall
/// whatever was previously active before the click, not the pane the
/// click itself came from, diverging from Neovim's `prevwin`.
///
/// **Verified RED against the pre-fix shape:** reverting the three
/// `apply_gutter_action` arms back to a direct `active_window =`
/// assignment makes this fail — `prev_window` stays `None` (or
/// whatever it held before the click) instead of recording
/// `active_before`.
#[test]
fn gutter_diagnostic_hover_on_a_background_pane_focuses_it_and_records_prev_window() {
    let mut e = Engine::new_for_test();
    e.split_window(SplitDirection::Vertical, None);
    let active_before = e.active_window_id();
    let background = e
        .active_tab()
        .window_ids()
        .into_iter()
        .find(|&w| w != active_before)
        .expect("split must produce a second, non-active window");

    let mut rw = fixture_window(WindowRect::new(0.0, 0.0, 40.0, 10.0), 4, 10, 0, 30, 0.0);
    rw.window_id = background;
    rw.diagnostic_gutter
        .insert(2, crate::core::lsp::DiagnosticSeverity::Error);

    apply_gutter_action(&mut e, &rw, background, 2, 0, "");

    assert_eq!(
        e.active_window_id(),
        background,
        "test setup sanity: the gutter click must focus the background pane"
    );
    assert_eq!(
        e.active_tab().prev_window,
        Some(active_before),
        "a gutter click that moves focus to a background pane must \
             record the previously-active window as Tab::prev_window, \
             exactly like every other focus-changing call site, so a \
             following CTRL-W p recalls it"
    );
}

/// `reserved_width` is the single source of truth for that reclaim —
/// both backends and `build_screen_layout` route through it.
#[test]
fn minimap_reserved_width_is_zero_when_the_setting_is_off() {
    let mut e = minimap_engine();
    e.settings.minimap = true;
    assert!(minimap_reserved_width(&e, 120.0, 1.0, TUI_MINIMAP_SIZING, 0.0) > 0.0);
    assert!(minimap_reserved_width(&e, 1200.0, 8.0, gtk_minimap_sizing(), 0.0) > 0.0);
    e.settings.minimap = false;
    assert_eq!(
        minimap_reserved_width(&e, 120.0, 1.0, TUI_MINIMAP_SIZING, 0.0),
        0.0
    );
    assert_eq!(
        minimap_reserved_width(&e, 1200.0, 8.0, gtk_minimap_sizing(), 0.0),
        0.0
    );
}

/// #828 acceptance (updated for #1869's real formula, not deleted):
/// `sizing` is an explicit parameter, honored exactly as given — not
/// re-derived from `char_width` via the old `if char_width > 1.0 { ... }
/// else { ... }` convention this file used to hardcode inside
/// `minimap_reserved_width` itself.
///
/// RED against that pre-#828 shape: there was no `sizing` parameter to
/// pass at all — the function *always* picked its constant set from
/// `char_width`, so a caller could never say "use GTK's own policy"
/// while passing a TUI-shaped `char_width`. This pins that passing
/// `gtk_minimap_sizing()` alongside `char_width == 1.0` (TUI's own real
/// metric) still resolves via [`vs_code_minimap_width_px`] — the real
/// VS Code pixel formula — rather than TUI's `resolve_width`-based
/// column policy, by checking the result against the formula computed
/// by hand, not merely against *some* non-TUI-shaped number.
#[test]
fn minimap_reserved_width_uses_the_explicit_sizing_not_char_width() {
    let e = minimap_engine();
    // Wide enough that the `char_width == 1.0` passed alongside it
    // (TUI's own MINIMAP_MIN_TEXT_COLS suppression check) doesn't
    // itself suppress the strip.
    let pane_width = 100.0;
    let char_width = 1.0;
    let got = minimap_reserved_width(&e, pane_width, char_width, gtk_minimap_sizing(), 0.0);
    // Hand-computed VS Code formula, independent of
    // `vs_code_minimap_width_px`'s own implementation: floor((100 - 14
    // - 2) / (1.0 + 1.0)) + 8 = floor(84 / 2) + 8 = 42 + 8 = 50.
    let expected = 50.0;
    assert_eq!(
        got, expected,
        "an explicit gtk_minimap_sizing() must resolve via the real VS \
             Code pixel formula even at char_width == 1.0 (TUI's own \
             metric), not TUI's column-based resolve_width: got {got}, \
             expected {expected}"
    );
}

/// A window too narrow to spare the strip suppresses the minimap rather
/// than squeezing the text into a sliver.
#[test]
fn minimap_suppresses_itself_in_a_narrow_window() {
    let e = minimap_engine();
    assert_eq!(
        minimap_reserved_width(&e, 20.0, 1.0, TUI_MINIMAP_SIZING, 0.0),
        0.0,
        "a 20-column window cannot spare the minimap's floor width plus \
             MINIMAP_MIN_TEXT_COLS of surviving text"
    );
}

/// #1094 acceptance ("A narrow pane still resolves to a sane layout
/// rather than squeezing the text out"): `minimap_reserved_width`'s own
/// affordability check only knows about the *pane's* width — it has no
/// notion of the scroll gutter that now sits beyond the strip too (see
/// `scroll_gutter_width`), so a pane that can afford the strip *alone*
/// can still be too narrow to afford the strip *and* the gutter without
/// squeezing the text below `MINIMAP_MIN_TEXT_COLS`. `build_screen_
/// layout` has to re-check with the gutter folded in and self-suppress
/// the same way a pane too narrow for the strip alone already does —
/// this is that interaction, driven through the real entry point
/// rather than `minimap_reserved_width` in isolation (which cannot see
/// `scrollbar_reserve` at all).
///
/// RED against a `build_screen_layout` that never re-checks
/// (equivalently, against reverting this fix): at `scrollbar_reserve =
/// 10.0`, the strip still shows and squeezes the pane's 40 columns down
/// to `40 - 6 (strip) - 10 (reserve) = 24` — below `MINIMAP_MIN_TEXT_
/// COLS` (30) — instead of self-suppressing. Confirmed by hand against
/// the pre-fix `render.rs`.
#[test]
fn minimap_suppresses_itself_when_the_pane_cannot_afford_both_the_strip_and_the_scroll_gutter() {
    let mut engine = test_engine(
        "a line of text long enough to fill the whole pane width and then some more text",
    );
    // #1858: minimap is experimental and off by default — this test is
    // specifically about the minimap's self-suppression, so turn it on
    // explicitly.
    engine.settings.minimap = true;
    let theme = Theme::onedark();
    // Same 40-column pane `build_screen_layout_honors_an_explicit_
    // scrollbar_reserve_even_at_char_width_one` uses: wide enough to
    // afford the strip (`minimap_reserved_width` alone returns non-zero
    // here) but not wide enough to *also* clear `MINIMAP_MIN_TEXT_COLS`
    // once a real scroll gutter is reserved beyond it.
    let bounds = WindowRect::new(0.0, 0.0, 40.0, 10.0);
    let (rects, _) = engine.calculate_group_window_rects(bounds, 1.0);

    let no_reserve = build_screen_layout(
        &engine,
        &theme,
        &rects,
        1.0,
        1.0,
        false,
        0.0,
        TUI_MINIMAP_SIZING,
    );
    assert!(
        !no_reserve.minimap.is_empty(),
        "fixture precondition: this 40-column pane must afford the strip \
             on its own (no scroll gutter competing for the same width), or \
             this test isn't exercising the interaction at all"
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
    assert!(
        with_reserve.minimap.is_empty(),
        "a pane that can only afford the strip *or* the scroll gutter, \
             not both without squeezing text below MINIMAP_MIN_TEXT_COLS, \
             must self-suppress the strip rather than paint a squeezed \
             layout; got minimap: {:?}",
        with_reserve.minimap
    );
    // And the editor still gets a sane (non-zero, no-panic) viewport —
    // "resolves to a sane layout" means real text columns survive, not
    // just "doesn't crash".
    assert!(
        with_reserve.windows[0].text_viewport_cols > 0,
        "self-suppressing the strip must not leave the pane with zero \
             text columns either"
    );
}

// ── #1085/#1098: point-sample → block-aggregation ───────────────────
//
// The white-box test that used to live here
// (`a_stride_skipped_distinctive_line_still_shows_up`) drove
// `minimap_block_bounds`/`minimap_block_sample_indices`/
// `minimap_block_text` directly — pure block-partitioning and
// dither-aggregation arithmetic, with no highlight/vimcode-specific
// logic in it at all. #1098 lifted that arithmetic into quadraui
// (quadraui#1012), which already carries the equivalent case
// (`sample_blocks_no_line_in_a_block_is_ever_fully_discarded`, same
// "a rare long line surrounded by short ones must not be discarded"
// property) in its own suite, so keeping a second copy of the same
// expectations here would just be a duplicate to keep in sync. The
// sibling test below stays: it drives the full `build_minimap_data`
// pipeline, which is where vimcode's own half (window/highlight
// mapping) still lives.

/// #1085 acceptance criterion 2: the viewport highlight band's own
/// *content* must differ between a blank run and a dense block of the
/// same file — not just which rows are highlighted (already covered
/// by `minimap_click_at_the_middle_seeks_to_half_the_file` and
/// friends), but what the aggregation actually painted into those
/// rows. A fix that only stops saturating (quadraui#1007's rasteriser
/// half) without also making vimcode's own sampling represent every
/// line would still show a band that doesn't track *where* on screen
/// the editor actually is.
///
/// The dense region's every 5th line (`j % 5 == 2`, never `== 0`) is
/// the only non-blank content, deliberately never a block boundary
/// (`HALF` is itself a multiple of the stride, so a block boundary is
/// always `≡ 0 (mod 5)`, absolute or region-relative) — a fixture
/// where the dense region's content sat *on* the sampled point would
/// pass under the pre-#1085 point-sampler too and prove nothing about
/// this issue.
///
/// **RED against unfixed `develop`:** confirmed by hand with the same
/// `block_sample_indices` → `vec![start]` revert #1085's own
/// (now-removed) white-box test used — the point-sampler only ever
/// reads each block's first line, which is blank in *both* regions by
/// construction here, so `dense_dots` collapses to `0` too and
/// `dense_dots > blank_dots * 4` (`0 > 0`) fails. Reverted before
/// landing this test.
#[test]
fn viewport_band_content_differs_between_a_blank_run_and_a_dense_block() {
    const HALF: usize = 400;
    let mut text = String::with_capacity(HALF * 22);
    for _ in 0..HALF {
        text.push('\n'); // blank run
    }
    for j in 0..HALF {
        if j % 5 == 2 {
            text.push_str(&"x".repeat(20));
        }
        text.push('\n');
    }
    let mut e = test_engine(&text);
    let theme = Theme::onedark();
    let wid = e.active_window_id();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);

    let count_band_dots = |e: &mut Engine, scroll_top: usize| -> usize {
        if let Some(w) = e.windows.get_mut(&wid) {
            w.view.scroll_top = scroll_top;
        }
        let mm = build_minimap_data(e, &theme, wid, rect, 1.0, 40)
            .expect("minimap must build")
            .minimap;
        let end = (mm.visible_row_start + mm.visible_row_count).min(mm.lines.len());
        mm.lines[mm.visible_row_start..end]
            .iter()
            .map(|l| l.text.chars().filter(|c| !c.is_whitespace()).count())
            .sum()
    };

    let blank_dots = count_band_dots(&mut e, 10);
    let dense_dots = count_band_dots(&mut e, HALF + 10);

    assert_eq!(
        blank_dots, 0,
        "the viewport band over a blank run (scroll_top=10) must show \
             zero set columns"
    );
    assert!(
        dense_dots > 0,
        "the viewport band over a dense block (scroll_top={}) must \
             show at least one set column",
        HALF + 10
    );
    assert!(
        dense_dots > blank_dots * 4,
        "the viewport band's own content must differ measurably \
             between a blank run ({blank_dots} set columns) and a dense \
             block ({dense_dots} set columns) of the same file"
    );
}

/// #1186 acceptance: the viewport-highlight band (`Minimap::visible_row_start`/
/// `visible_row_count`, painted as a background band by both backends'
/// rasterisers — see `quadraui::tui::minimap`'s module doc) must shrink
/// and reposition correctly once blocks start covering more than one
/// real buffer line, not stay pinned to the pre-#1186 per-line shape.
///
/// A `viewport_lines`-tall editor viewport spans `viewport_lines` real
/// buffer lines, which now maps to `ceil(viewport_lines / block_width)`
/// **blocks** (clamped to at least 1) rather than `viewport_lines`
/// blocks — since each block covers `block_width` real lines. This
/// checks that relationship directly against a compressed window
/// (`block_width > 1`), and that `visible_row_start` lands on the block
/// that actually contains `scroll_top`, not the next one after it (the
/// off-by-one the old `position(|l| l.line_idx >= scroll_top)` formula
/// would hit once a block's own range no longer starts exactly at
/// `scroll_top`).
///
/// **RED against unfixed `develop`:** confirmed by hand — reverting
/// `visible_row_start`'s `partition_point` formula back to
/// `position(|l| l.line_idx >= scroll_top)` (the pre-#1186 formula) at
/// this test's compression skips the block that actually contains
/// `scroll_top`, landing one block later than expected and failing the
/// `visible_row_start` assertion below. Reverted before landing this
/// test.
#[test]
fn viewport_band_scales_down_and_repositions_under_compression() {
    const N_LINES: usize = 20_000;
    const EDITOR_VISIBLE_ROWS: usize = 40;
    let mut e = large_minimap_engine(N_LINES);
    let theme = Theme::onedark();
    let wid = e.active_window_id();
    let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);

    // Scroll well into the file (but not to the very bottom, so the
    // scrolled-to line lands squarely inside some block's range rather
    // than coincidentally on a window boundary).
    let scroll_top = N_LINES / 3;
    if let Some(w) = e.windows.get_mut(&wid) {
        w.view.scroll_top = scroll_top;
        w.view.cursor.line = scroll_top;
    }

    let mm = build_minimap_data(&e, &theme, wid, rect, 1.0, EDITOR_VISIBLE_ROWS)
        .expect("minimap must build")
        .minimap;

    let block_width = mm.lines[1].line_idx - mm.lines[0].line_idx;
    assert!(
        block_width > 1,
        "test setup sanity: this fixture must be far enough past the \
             compression ceiling to produce multi-line blocks, or this \
             test cannot distinguish compressed from uncompressed band math \
             (block_width={block_width})"
    );

    // Height: a `EDITOR_VISIBLE_ROWS`-line viewport must cover roughly
    // `EDITOR_VISIBLE_ROWS / block_width` blocks (clamped to >= 1) —
    // never the pre-#1186 `EDITOR_VISIBLE_ROWS` blocks a compressed
    // window would wildly overshoot to.
    let expected_rows = (EDITOR_VISIBLE_ROWS / block_width).max(1);
    assert!(
        mm.visible_row_count <= expected_rows + 1 && mm.visible_row_count >= 1,
        "a {EDITOR_VISIBLE_ROWS}-line viewport at block_width \
             {block_width} must show roughly {expected_rows} band block(s) \
             (clamped to >= 1), got {} — the band must shrink under \
             compression, not stay pinned to the uncompressed \
             {EDITOR_VISIBLE_ROWS}",
        mm.visible_row_count
    );

    // Position: `visible_row_start` must be the block that actually
    // *contains* `scroll_top`, i.e. the last block whose own start is
    // `<= scroll_top`.
    let expected_start = mm
        .lines
        .iter()
        .rposition(|l| l.line_idx <= scroll_top)
        .expect("some block must start at or before scroll_top");
    assert_eq!(
        mm.visible_row_start, expected_start,
        "visible_row_start must be the block containing scroll_top \
             ({scroll_top}), not the next block after it"
    );
}

/// #722 acceptance: in a `:vsplit`, both panes show their own minimap
/// over their own buffer — not a single strip pinned to the active
/// pane. Each buffer gets distinct content so a transposed or
/// cross-wired sample (pane A showing pane B's file) would show up as a
/// mismatched `total_buffer_lines`.
#[test]
fn split_gives_every_pane_its_own_minimap_over_its_own_buffer() {
    let mut e = test_engine("");
    // #1858: minimap is experimental and off by default — this test is
    // specifically about the minimap, so turn it on explicitly.
    e.settings.minimap = true;
    e.buffer_mut().insert(0, &"left\n".repeat(50));
    e.split_window(SplitDirection::Vertical, None);
    // The split's new window starts on the same buffer; give it its own
    // so the two minimaps are provably over different files.
    let new_buf = e.buffer_manager.create();
    e.buffer_manager
        .get_mut(new_buf)
        .unwrap()
        .buffer
        .insert(0, &"right\n".repeat(120));
    e.active_window_mut().buffer_id = new_buf;

    let screen = render_engine(&e, 160.0, 30.0);
    assert_eq!(screen.windows.len(), 2, "vsplit must produce two windows");
    assert_eq!(
        screen.minimap.len(),
        2,
        "both panes must carry a minimap, not just the active one"
    );

    let totals: Vec<usize> = screen
        .windows
        .iter()
        .map(|w| {
            screen
                .minimap
                .iter()
                .find(|m| m.window_id == w.window_id)
                .unwrap_or_else(|| panic!("pane {:?} must have its own minimap", w.window_id))
                .minimap
                .total_buffer_lines
        })
        .collect();
    assert_ne!(
        totals[0], totals[1],
        "the two panes' minimaps must reflect their own distinct buffers, \
             got matching totals {totals:?}"
    );
}

/// #722 acceptance: switching focus between panes must not change
/// either pane's text width. Before the fix, the width reclaim was
/// gated on `is_active`, so *both* panes reflowed every time focus
/// moved — assert the window rects (hence `text_viewport_cols`) are
/// bit-identical across a focus change with the minimap on.
#[test]
fn focus_change_does_not_move_either_panes_text_width() {
    let mut e = minimap_engine();
    e.split_window(SplitDirection::Vertical, None);
    e.settings.minimap = true;

    let before = render_engine(&e, 160.0, 30.0);
    assert_eq!(before.minimap.len(), 2, "both panes must have a minimap");
    let widths_before: std::collections::HashMap<WindowId, f64> = before
        .windows
        .iter()
        .map(|w| (w.window_id, w.rect.width))
        .collect();
    let cols_before: std::collections::HashMap<WindowId, usize> = before
        .windows
        .iter()
        .map(|w| (w.window_id, w.text_viewport_cols))
        .collect();

    let focus_before = e.active_window_id();
    e.focus_next_window();
    assert_ne!(
        e.active_window_id(),
        focus_before,
        "test setup sanity: focus must actually have moved"
    );

    let after = render_engine(&e, 160.0, 30.0);
    assert_eq!(
        after.minimap.len(),
        2,
        "both panes must still have a minimap"
    );
    for w in &after.windows {
        assert_eq!(
            w.rect.width, widths_before[&w.window_id],
            "pane {:?}'s width must not change when focus moves elsewhere",
            w.window_id
        );
        assert_eq!(
            w.text_viewport_cols, cols_before[&w.window_id],
            "pane {:?}'s text_viewport_cols must not change when focus \
                 moves elsewhere",
            w.window_id
        );
    }
}

#[test]
fn test_settings_to_form_read_only_by_default() {
    let e = test_engine("");
    let idx = crate::core::settings::SETTING_DEFS
        .iter()
        .position(|d| d.key == "font_family")
        .expect("font_family setting exists");
    let form = settings_to_form(&e);
    let field = form
        .fields
        .iter()
        .find(|f| f.id == quadraui::WidgetId::new(format!("setting-{idx}")))
        .expect("font_family field present");
    assert!(
        matches!(field.kind, quadraui::FieldKind::ReadOnly { .. }),
        "non-edited StringVal setting should render ReadOnly, got {:?}",
        field.kind
    );
}

#[test]
fn test_settings_to_form_inline_edit_emits_text_input_with_cursor() {
    let mut e = test_engine("");
    let idx = crate::core::settings::SETTING_DEFS
        .iter()
        .position(|d| d.key == "font_family")
        .expect("font_family setting exists");
    e.settings_editing = Some(idx);
    e.settings_edit_buf = "Fira Code".to_string();

    let form = settings_to_form(&e);
    let field = form
        .fields
        .iter()
        .find(|f| f.id == quadraui::WidgetId::new(format!("setting-{idx}")))
        .expect("font_family field present");
    match &field.kind {
        quadraui::FieldKind::TextInput { value, cursor, .. } => {
            assert_eq!(value, "Fira Code");
            assert_eq!(*cursor, Some("Fira Code".len()));
        }
        other => panic!("expected TextInput while editing, got {other:?}"),
    }
}

#[test]
fn test_settings_to_form_ext_setting_inline_edit_emits_text_input() {
    use crate::core::extensions::{ExtSettingDef, ExtensionManifest};
    use crate::core::session::InstalledExtension;

    let mut e = test_engine("");
    e.ext_registry = Some(vec![ExtensionManifest {
        name: "myext".to_string(),
        display_name: "My Ext".to_string(),
        settings: vec![ExtSettingDef {
            key: "greeting".to_string(),
            label: "Greeting".to_string(),
            r#type: "string".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    }]);
    e.extension_state.installed.push(InstalledExtension {
        name: "myext".to_string(),
        version: String::new(),
    });
    e.ext_settings_editing = Some(("myext".to_string(), "greeting".to_string()));
    e.settings_edit_buf = "hi".to_string();

    let form = settings_to_form(&e);
    let field = form
        .fields
        .iter()
        .find(|f| f.id == quadraui::WidgetId::new("ext-setting-myext-greeting"))
        .expect("ext setting field present");
    match &field.kind {
        quadraui::FieldKind::TextInput { value, cursor, .. } => {
            assert_eq!(value, "hi");
            assert_eq!(*cursor, Some("hi".len()));
        }
        other => panic!("expected TextInput while editing ext setting, got {other:?}"),
    }
}

#[test]
fn test_screen_layout_basic_structure() {
    let e = test_engine("Hello, world!\nSecond line\nThird line\n");
    let layout = render_engine(&e, 80.0, 24.0);

    // Should have exactly one window
    assert_eq!(layout.windows.len(), 1, "single buffer = single window");

    // Window should contain rendered lines
    let win = &layout.windows[0];
    assert!(
        win.lines.len() >= 3,
        "should render at least 3 content lines"
    );
    assert!(win.is_active);
    assert!(win.cursor.is_some(), "cursor should be visible");

    // First line content
    assert_eq!(win.lines[0].raw_text.trim_end(), "Hello, world!");

    // Tab bar should have one tab
    assert!(!layout.tab_bar.is_empty());
    assert!(layout.tab_bar[0].active);
}

#[test]
fn test_screen_layout_cursor_position() {
    let mut e = test_engine("abcdef\nghijkl\n");
    // Move cursor to line 1, col 3
    e.handle_key("j", Some('j'), false);
    e.handle_key("l", Some('l'), false);
    e.handle_key("l", Some('l'), false);
    e.handle_key("l", Some('l'), false);
    let layout = render_engine(&e, 80.0, 24.0);

    let win = &layout.windows[0];
    let (cursor_pos, _shape) = win.cursor.unwrap();
    assert_eq!(cursor_pos.view_line, 1, "cursor on second line");
    assert_eq!(cursor_pos.col, 3, "cursor at col 3");
}

#[test]
fn test_screen_layout_split_windows() {
    let mut e = test_engine("file one\n");
    // This test is about general split geometry, not the minimap — turn
    // it off so its per-pane width reservation (#722: every pane reserves
    // its own strip now, not just the active one) doesn't confound the
    // "windows divide the available width" assertion below.
    e.settings.minimap = false;
    // Open a vertical split
    e.open_editor_group(SplitDirection::Vertical);

    let layout = render_engine(&e, 80.0, 24.0);
    assert_eq!(layout.windows.len(), 2, "vsplit should produce two windows");

    // Windows should divide the horizontal space
    let w0 = &layout.windows[0];
    let w1 = &layout.windows[1];
    assert!(w0.rect.width > 0.0);
    assert!(w1.rect.width > 0.0);
    assert!(
        (w0.rect.width + w1.rect.width - 80.0).abs() < 2.0,
        "widths should approximately sum to terminal width"
    );
}

#[test]
fn test_screen_layout_terminal_open() {
    let mut e = test_engine("content\n");
    e.terminal_open = true;
    e.session.terminal_panel_rows = 10;

    let layout = render_engine(&e, 80.0, 24.0);

    // Bottom panel active tab should reflect terminal
    assert_eq!(
        layout.bottom_tabs.active,
        BottomPanelKind::Terminal,
        "bottom panel should show terminal tab"
    );

    // Editor window height should be reduced (less than full 24 rows)
    let win = &layout.windows[0];
    assert!(
        win.rect.height < 24.0,
        "editor should be shorter when terminal is open"
    );
}

#[test]
fn test_screen_layout_visual_selection() {
    let mut e = test_engine("select this text\n");
    // Enter visual mode and select 5 chars
    e.handle_key("v", Some('v'), false);
    for _ in 0..4 {
        e.handle_key("l", Some('l'), false);
    }

    let layout = render_engine(&e, 80.0, 24.0);
    let win = &layout.windows[0];
    assert!(
        win.selection.is_some(),
        "visual mode should produce a selection range"
    );
}

#[test]
fn test_screen_layout_command_line() {
    let mut e = test_engine("hello\n");
    // Enter command mode
    e.handle_key(":", Some(':'), false);
    e.handle_key("w", Some('w'), false);

    let layout = render_engine(&e, 80.0, 24.0);
    assert!(
        layout.command.text.contains(":w"),
        "command line should show ':w', got: {:?}",
        layout.command.text
    );
    assert!(layout.command.show_cursor);
}
