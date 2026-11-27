//! GTK-only click-context construction. Everything else that used to live
//! in this file moved to the backend-neutral `crate::click` — none of it
//! named a `gtk4`/`pango` type. Re-exported below so this module's own tests
//! (and the rest of `crate::gtk`) keep resolving the names unchanged.
#[cfg(test)]
use gtk4::pango;
#[cfg(test)]
use pangocairo::functions as pangocairo;

// Only this file's own `#[cfg(test)]` modules below reach these through
// `use super::*` (they exercise the moved functions against real headless
// Pango layouts), so a non-test build sees the re-export as unused.
#[allow(unused_imports)]
pub(crate) use crate::click::*;

// This module's tests exercise the re-exported `crate::click` functions
// against real headless Pango layouts, which is why this GTK-only file
// (rather than the neutral `crate::click`) still hosts them. `use super::*`
// inside each `mod ... tests` pulls these in, mirroring what `use super::*`
// (of `crate::gtk`) used to provide before the production code moved out.
#[cfg(test)]
use crate::core;
#[cfg(test)]
use crate::core::engine::EngineAction;
#[cfg(test)]
use crate::core::window::GroupId;
#[cfg(test)]
use crate::core::Engine;
#[cfg(test)]
use crate::gtk::backend;
#[cfg(test)]
use crate::render;
#[cfg(test)]
use crate::render::{self as render_mod, ScreenZone, Theme};
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::rc::Rc;

#[cfg(test)]
mod emoji_click_column_tests {
    //! Click-to-column resolution with emoji (✅ 🟡 ❌ ⏭️) on a markdown
    //! line, through the production path (`render::editor_text_layout` +
    //! `EditorLayout::col_at_x`). quadraui resolves editor click columns
    //! monospace, so every glyph wider than one cell drifts the columns
    //! after it; the gated test below fails until quadraui offers an exact,
    //! shaped column inverse for pixel backends.
    use super::*;
    use crate::core::WindowRect;
    use crate::render::build_screen_layout;
    use ::pangocairo::cairo::{Context, Format, ImageSurface};

    /// `crate::harness::KNOWN_BUGS` label for the test below.
    const EMOJI_CLICK_COLUMN_BUG: &str = "gtk_click_column_after_wide_glyph";

    fn headless_pango_layout() -> pango::Layout {
        let surface = ImageSurface::create(Format::ARgb32, 900, 60).expect("create ImageSurface");
        let cr = Context::new(&surface).expect("Context::new");
        let ctx = pangocairo::create_context(&cr);
        ctx.set_font_description(&pango::FontDescription::from_string("Monospace 12"));
        pango::Layout::new(&ctx)
    }

    #[test]
    fn click_resolves_exact_column_on_emoji_markdown_line() {
        crate::harness::known_bug_gate(EMOJI_CLICK_COLUMN_BUG, || {
            // Concurrent Pango/Cairo text work from two test threads segfaults
            // inside FreeType — see `src/test_paint.rs`.
            let _paint = crate::test_paint::PaintGuard::acquire();
            let text = "Total: **58 commands**  \u{b7}  \u{2705} 36  \u{b7}  \u{1f7e1} 2  \u{b7}  \u{274c} 14  \u{b7}  \u{23ed}\u{fe0f} 6";
            let mut engine = Engine::new();
            engine.buffer_mut().insert(0, text);
            let buf_id = engine.active_buffer_id();
            engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
                Some(std::path::PathBuf::from("notes.md"));

            // Lay the editor out at the measuring font's own advance, so only a
            // glyph wider than one cell can move a column.
            let measure_layout = headless_pango_layout();
            measure_layout.set_text("0");
            let char_width = measure_layout.pixel_size().0 as f64;
            let line_height = 18.0;
            let theme = Theme::onedark();
            let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
            let (rects, _) = engine.calculate_group_window_rects(bounds, 24.0);
            let layout = build_screen_layout(
                &engine,
                &theme,
                &rects,
                line_height,
                char_width,
                true,
                8.0,
                crate::render::gtk_minimap_sizing(),
            );
            let rw = &layout.windows[0];
            assert_eq!(
                rw.lines[0].raw_text, text,
                "line should not wrap in an 800px window"
            );

            let (editor, editor_layout) = render::editor_text_layout(rw, char_width, line_height);
            measure_layout.set_text(text);

            let char_count = text.chars().count();
            let mut prev_pos: Option<(i32, i32)> = None;
            for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
                let pos = measure_layout.index_to_pos(byte_idx as i32);
                // A zero-width combining mark (e.g. the U+FE0F variation
                // selector on "⏭️") shares its base character's glyph cluster
                // — Pango reports the *identical* (x, width) rect for both
                // byte offsets, since there is no distinct on-screen pixel
                // region for the combining mark alone. A click can only ever
                // land on the cluster as a whole, so such chars have no
                // resolvable column of their own to assert against — skip
                // them rather than asserting an unreachable identity.
                if prev_pos == Some((pos.x(), pos.width())) {
                    continue;
                }
                prev_pos = Some((pos.x(), pos.width()));

                let glyph_left_x =
                    editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
                let glyph_width = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
                let click_x = glyph_left_x + glyph_width * 0.25;

                let resolved = editor_layout.col_at_x(&editor, 0, click_x as f32);
                assert_eq!(
                    resolved, char_idx,
                    "clicking char {char_idx} ({ch:?}) resolved to col {resolved}, not {char_idx} \
                     — the paint↔click column inverse has drifted for this glyph"
                );
            }
            assert_eq!(char_count, text.chars().count());
        });
    }
}

#[cfg(test)]
mod cross_split_drag_focus_tests {
    //! Dragging a text selection in one editor group (a GTK split pane)
    //! must not steal focus to a neighboring group's window merely because
    //! the mouse passes over it while the button is held. `Engine::mouse_drag`
    //! keeps the selection data pinned to the originating window. But a
    //! drag-continuation hit-test must leave `active_group`/`active_window_id()`
    //! untouched, while a genuine click still focuses the pane it lands in.
    use super::*;
    use crate::render::build_screen_layout;

    fn empty_tab_pixel_hits() -> GroupTabBarLayoutMap {
        HashMap::new()
    }

    #[test]
    fn drag_continuation_does_not_steal_focus_to_neighboring_group() {
        let mut engine = Engine::new();
        // Pin the no-gutter geometry this test's `char_width * 2.0` pixel math
        // assumes, since gutter width isn't what this test is about.
        engine.settings.line_numbers = crate::core::settings::LineNumberMode::None;
        engine.buffer_mut().insert(0, "hello world");
        let wid_a = engine.active_window_id();
        let group_a = engine.active_group;

        // Open a second editor group side-by-side — GTK's split-pane
        // feature (bound to the tab bar's split button). `open_editor_group`
        // makes the new group/window active.
        engine.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let group_b = engine.active_group;
        let wid_b = engine.active_window_id();
        assert_ne!(group_a, group_b);
        assert_ne!(wid_a, wid_b);

        // Simulate the user clicking back into the left pane to start the drag.
        engine.mouse_click(wid_a, 0, 1);
        assert_eq!(engine.active_group, group_a);
        assert_eq!(engine.active_window_id(), wid_a);
        engine.mouse_drag(wid_a, 0, 4);
        assert!(engine.mouse_drag_active);
        assert_eq!(engine.mouse_drag_origin_window, Some(wid_a));

        // Lay out both panes side by side and locate each window's rect.
        let theme = Theme::onedark();
        let bounds = core::WindowRect::new(0.0, 0.0, 1600.0, 400.0);
        let line_height: f64 = 18.0;
        let char_width: f64 = 9.0;
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );
        let rw_b = screen
            .windows
            .iter()
            .find(|w| w.window_id == wid_b)
            .expect("window B should be laid out");
        // A pixel comfortably inside window B's text area (below its tab bar).
        let x_in_b = rw_b.rect.x + char_width * 2.0;
        let y_in_b = rw_b.rect.y + line_height * 2.0;

        let backend = Rc::new(RefCell::new(super::super::backend::GtkBackend::new()));
        let tab_pixel_hits = empty_tab_pixel_hits();

        // Drag continuation: the mouse is over window B's pixels, but this
        // must resolve as a pure hit-test — no focus/group side effects.
        let target = pixel_to_click_target(
            &mut engine,
            x_in_b,
            y_in_b,
            line_height,
            char_width,
            &screen,
            &tab_pixel_hits,
            None, // no cached band hits in this test — exercises the
            // `screen_zone_hit_test` fallback path
            false, // mutate_focus: drag continuation
            &mut quadraui::DragState::default(),
            false, // no Alt context in this test
        );
        assert_eq!(
            engine.active_group, group_a,
            "a held drag sweeping over the neighboring group must not steal active_group"
        );
        assert_eq!(
            engine.active_window_id(),
            wid_a,
            "a held drag sweeping over the neighboring group must not steal active_window_id \
             (render::build_selection's is_active gate keys off this)"
        );
        match target {
            ClickTarget::BufferPos(wid, _, _) => {
                assert_eq!(
                    wid, wid_b,
                    "the hit-test should still resolve the real window under the cursor"
                )
            }
            other => panic!("expected a BufferPos hit in window B's text area, got {other:?}"),
        }
        // The engine-level origin lock (already covered by
        // `test_mouse_drag_locked_to_origin_window`) rejects this mismatched
        // window_id, so the selection itself stays anchored to window A.
        engine.mouse_drag(wid_b, 0, 8);
        assert_eq!(engine.mouse_drag_origin_window, Some(wid_a));

        // Contrast: a genuine click landing in window B (mutate_focus: true)
        // — as a real MouseClick/DoubleClick event would — SHOULD focus it.
        // This proves the flag actually gates behavior rather than being a
        // no-op, and that real clicks keep working as before.
        let click_target = pixel_to_click_target(
            &mut engine,
            x_in_b,
            y_in_b,
            line_height,
            char_width,
            &screen,
            &tab_pixel_hits,
            None,
            true, // mutate_focus: genuine click
            &mut quadraui::DragState::default(),
            false, // no Alt context in this test
        );
        assert_eq!(
            engine.active_group, group_b,
            "a genuine click must still focus the pane it lands in"
        );
        assert!(matches!(click_target, ClickTarget::BufferPos(wid, _, _) if wid == wid_b));
    }
}

#[cfg(test)]
mod editor_band_hits_tests {
    //! `EditorBandHits::zone_at` and the `band_hits` branch of
    //! `pixel_to_click_target` map the editor band's tab-bar `FrameTree`
    //! nodes and pane rects back to `render::ScreenZone`. These tests build
    //! the hits from the same `to_q_editor` / `tab_bar_draw_targets`
    //! construction `App::compose_editor_band_rungs` records, instead of
    //! exercising only the `screen_zone_hit_test` fallback.
    use super::*;
    use crate::core::WindowRect;
    use crate::render::build_screen_layout;

    /// Lay out `engine`'s groups and record the band hits the way
    /// `compose_editor_band_rungs` does: every pane in `screen.windows`
    /// order, then every tab bar. Also returns each tab bar's group and rect.
    fn build_band_hits(
        engine: &Engine,
        theme: &Theme,
        line_height: f64,
        char_width: f64,
    ) -> (
        render::ScreenLayout,
        EditorBandHits,
        Vec<(GroupId, quadraui::Rect)>,
    ) {
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(
            engine,
            theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );

        let mut hits = EditorBandHits::new();
        for rw in &screen.windows {
            hits.push_editor(render_mod::to_q_editor(rw).rect);
        }

        let tab_row_h = render_mod::tab_row_height_px(line_height);
        let tab_bar_h = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
        let mut bars = Vec::new();
        for target in render_mod::tab_bar_draw_targets(engine, &screen, tab_row_h, tab_bar_h) {
            hits.push_tab_bar(target.group_id, target.rect, target.bar.clone());
            bars.push((target.group_id, target.rect));
        }
        (screen, hits, bars)
    }

    #[test]
    fn zone_at_resolves_editor_point() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid = engine.active_window_id();
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hits, _) = build_band_hits(&engine, &theme, line_height, char_width);
        let rw = screen
            .windows
            .first()
            .expect("single window should be laid out");
        let x = rw.rect.x + char_width * 2.0;
        let y = rw.rect.y + line_height * 2.0;

        match hits.zone_at(&screen, x, y) {
            ScreenZone::Window {
                window_id,
                window_idx,
                ..
            } => {
                assert_eq!(window_id, wid);
                assert_eq!(window_idx, 0);
            }
            other => panic!("expected ScreenZone::Window from the band hits, got {other:?}"),
        }
    }

    #[test]
    fn zone_at_resolves_tab_bar_point() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let group_id = engine.active_group;
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hits, bars) = build_band_hits(&engine, &theme, line_height, char_width);
        let &(zone_group, rect) = bars
            .first()
            .expect("single-group mode should still record one tab bar");
        assert_eq!(zone_group, group_id);
        let x = rect.x as f64 + 2.0;
        let y = rect.y as f64 + 2.0;

        match hits.zone_at(&screen, x, y) {
            ScreenZone::TabBar {
                group_id: resolved,
                local_x,
                bar_width,
            } => {
                assert_eq!(resolved, group_id);
                assert_eq!(local_x, 2.0);
                assert_eq!(bar_width, rect.width as f64);
            }
            other => panic!("expected ScreenZone::TabBar from the band hits, got {other:?}"),
        }
    }

    /// Every editor tab bar paints under one `WidgetId`, so a click on the
    /// second group's bar must resolve by node position to that group, not
    /// to whichever group shares the id first.
    #[test]
    fn zone_at_resolves_each_split_groups_own_tab_bar() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        engine.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hits, bars) = build_band_hits(&engine, &theme, line_height, char_width);
        assert_eq!(bars.len(), 2, "a vertical group split records two tab bars");
        assert_ne!(bars[0].0, bars[1].0);
        for &(group_id, rect) in &bars {
            let x = rect.x as f64 + 3.0;
            let y = rect.y as f64 + 2.0;
            match hits.zone_at(&screen, x, y) {
                ScreenZone::TabBar {
                    group_id: resolved,
                    local_x,
                    ..
                } => {
                    assert_eq!(resolved, group_id);
                    assert_eq!(local_x, 3.0);
                }
                other => panic!("expected group {group_id:?}'s tab bar, got {other:?}"),
            }
        }
    }

    #[test]
    fn zone_at_misses_outside_every_surface() {
        let engine = Engine::new();
        let theme = Theme::onedark();
        let (screen, hits, _) = build_band_hits(&engine, &theme, 18.0, 9.0);
        assert!(matches!(
            hits.zone_at(&screen, 5000.0, 5000.0),
            ScreenZone::None
        ));
    }

    #[test]
    fn pixel_to_click_target_consults_the_cached_band_hits_for_editor_clicks() {
        // Proves `pixel_to_click_target` actually takes the `band_hits`
        // branch (not silently falling through to `screen_zone_hit_test`)
        // when real hits are supplied — the production `Some` path, as
        // opposed to the `None` fallback path covered by
        // `cross_split_drag_focus_tests`.
        let mut engine = Engine::new();
        // Pin the no-gutter geometry this test's `char_width * 2.0` pixel math
        // assumes, since gutter width isn't what this test is about.
        engine.settings.line_numbers = crate::core::settings::LineNumberMode::None;
        engine.buffer_mut().insert(0, "hello world");
        let wid = engine.active_window_id();
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hits, _) = build_band_hits(&engine, &theme, line_height, char_width);
        let rw = screen
            .windows
            .first()
            .expect("single window should be laid out");
        let x = rw.rect.x + char_width * 2.0;
        let y = rw.rect.y + line_height * 2.0;

        let backend = Rc::new(RefCell::new(super::super::backend::GtkBackend::new()));
        let empty_pixel_hits: GroupTabBarLayoutMap = HashMap::new();

        let target = pixel_to_click_target(
            &mut engine,
            x,
            y,
            line_height,
            char_width,
            &screen,
            &empty_pixel_hits,
            Some(&hits),
            true,
            &mut quadraui::DragState::default(),
            false,
        );
        match target {
            ClickTarget::BufferPos(id, _, _) => assert_eq!(id, wid),
            other => panic!("expected a BufferPos hit resolved via the band hits, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod single_group_tab_click_dispatch_tests {
    //! Clicking a non-active tab in a single-group layout, or a tab's ×,
    //! must resolve correctly. The defect lived in `screen_zone_hit_test`'s
    //! single-group arm, which hardcoded the tab row's top at the origin
    //! instead of deriving it from the window rects the way the split arm did.
    //! Once the chrome included a menu/title-bar band, the content origin
    //! moved down, so the single-group band pointed at chrome pixels no tab
    //! was drawn on — and every single-group tab click resolved to `ScreenZone::None`.
    //!
    //! Two things make these tests discriminate where the black-box
    //! `gtk::testing` pair does not:
    //!
    //! 1. `band_hits: None` — forcing the `screen_zone_hit_test` fallback
    //!    branch. GTK's production routing prefers the cached `EditorBandHits`
    //!    and only falls back here on a miss / before the first paint.
    //! 2. A **synthetic 100px content offset**, matching the test in `render::tests`.
    //!    The headless harness's default title-bar chrome only shifts ~23px,
    //!    which is small enough that the painted click y falls inside both the
    //!    correct band and the buggy hardcoded one — the offset has to exceed
    //!    the bar height to separate them.
    use super::*;
    use crate::render::build_screen_layout;

    /// Chrome-shifted editor content origin — large enough to separate the
    /// correct band from the buggy one.
    const CONTENT_X: f64 = 50.0;
    const CONTENT_Y: f64 = 100.0;
    const CONTENT_W: f64 = 800.0;
    const CONTENT_H: f64 = 600.0;
    /// Synthetic per-tab pixel width, bar-relative (see [`synthetic_tab_bar_layout`]).
    const TAB_W: f64 = 120.0;

    /// The `TabBarLayout` the rasteriser would have cached for a single group
    /// with `tabs` tabs: contiguous `TAB_W`-wide slots from the bar's left
    /// edge, each with a 15px close (`×`) zone inset near its right edge.
    ///
    /// Bar-relative, exactly like the `TabBarLayout` a real `draw_dyn`
    /// tab-bar paint returns — which is what `pixel_to_click_target` matches
    /// `ScreenZone::TabBar { local_x }` against. Synthetic rather than
    /// rasterised so the test states its own geometry instead of depending on
    /// Pango font metrics. Close regions are pushed before tab-body regions
    /// so `TabBarLayout::hit_test` resolves them first, matching a real
    /// paint's more-specific-first `hit_regions` ordering.
    fn synthetic_tab_bar_layout(tabs: usize) -> quadraui::TabBarLayout {
        let mut hit_regions = Vec::new();
        let mut visible_tabs = Vec::new();
        for i in 0..tabs {
            let tab_x = i as f64 * TAB_W;
            let tab_rect = quadraui::Rect::new(tab_x as f32, 0.0, TAB_W as f32, 24.0);
            let close_rect = quadraui::Rect::new((tab_x + 100.0) as f32, 0.0, 15.0, 24.0);
            hit_regions.push((close_rect, quadraui::TabBarHit::TabClose(i)));
            hit_regions.push((tab_rect, quadraui::TabBarHit::Tab(i)));
            visible_tabs.push(quadraui::VisibleTab {
                tab_idx: i,
                bounds: tab_rect,
                close_bounds: Some(close_rect),
            });
        }
        quadraui::TabBarLayout {
            bar_width: (tabs as f64 * TAB_W) as f32,
            bar_height: 24.0,
            visible_tabs,
            visible_segments: Vec::new(),
            scroll_left: None,
            scroll_right: None,
            hit_regions,
            resolved_scroll_offset: 0,
        }
    }

    /// Bar-relative x of a point inside tab `idx`'s body, clear of its × zone.
    fn tab_body_local_x(idx: usize) -> f64 {
        idx as f64 * TAB_W + 20.0
    }

    /// Bar-relative x of a point inside tab `idx`'s × zone.
    fn tab_close_local_x(idx: usize) -> f64 {
        idx as f64 * TAB_W + 107.0
    }

    /// Three tabs in a single editor group (the exact shape the regression
    /// reports), laid out at the chrome-shifted content origin, plus the cached
    /// rasteriser geometry and the empty legacy maps `pixel_to_click_target`
    /// still takes. No buffer edits anywhere, so a close click isn't diverted
    /// into the dirty-buffer confirm dialog.
    struct Fixture {
        engine: Engine,
        group: GroupId,
        screen: render::ScreenLayout,
        tab_pixel_hits: GroupTabBarLayoutMap,
        backend: Rc<RefCell<super::super::backend::GtkBackend>>,
        line_height: f64,
        char_width: f64,
    }

    impl Fixture {
        fn new() -> Self {
            let mut engine = Engine::new();
            engine.new_tab(None);
            engine.new_tab(None);
            let group = engine.active_group;
            assert_eq!(engine.editor_groups[&group].tabs.len(), 3);
            assert_eq!(
                engine.editor_groups[&group].active_tab, 2,
                "`new_tab` activates the tab it creates"
            );

            let theme = Theme::onedark();
            let line_height: f64 = 20.0;
            let char_width: f64 = 8.0;
            let tab_bar_height =
                render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
            let content = core::WindowRect::new(CONTENT_X, CONTENT_Y, CONTENT_W, CONTENT_H);
            let (rects, _) = engine.calculate_group_window_rects(content, tab_bar_height);
            let screen = build_screen_layout(
                &engine,
                &theme,
                &rects,
                line_height,
                char_width,
                false,
                8.0,
                crate::render::gtk_minimap_sizing(),
            );
            assert!(
                screen.editor_group_split.is_none(),
                "these tests must exercise the single-group arm; a split layout would \
                 take the branch that never regressed"
            );

            let mut tab_pixel_hits: GroupTabBarLayoutMap = HashMap::new();
            tab_pixel_hits.insert(
                group.0,
                (
                    quadraui::Rect::new(0.0, 0.0, (3.0 * TAB_W) as f32, 24.0),
                    synthetic_tab_bar_layout(3),
                ),
            );

            Self {
                engine,
                group,
                screen,
                tab_pixel_hits,
                backend: Rc::new(RefCell::new(super::super::backend::GtkBackend::new())),
                line_height,
                char_width,
            }
        }

        /// The tab row sits immediately ABOVE the window content, i.e. in
        /// `[CONTENT_Y, CONTENT_Y + tab_bar_height)`. The pre-fix code looked
        /// for it in `[0, tab_bar_height)`, which at this offset holds no tab
        /// pixels at all.
        const CLICK_Y: f64 = CONTENT_Y + 2.0;

        /// Resolve a tab-bar click through the production GTK dispatch entry
        /// point, with `band_hits: None` to force the `screen_zone_hit_test`
        /// fallback branch (see this module's doc comment).
        fn click(&mut self, local_x: f64) -> ClickTarget {
            pixel_to_click_target(
                &mut self.engine,
                CONTENT_X + local_x,
                Self::CLICK_Y,
                self.line_height,
                self.char_width,
                &self.screen,
                &self.tab_pixel_hits,
                None,
                true, // a genuine click
                &mut quadraui::DragState::default(),
                false, // no Alt context in this test
            )
        }

        /// The same pixel driven through the real click handler —
        /// `handle_mouse_click` is the production caller that turns
        /// `ClickTarget::CloseTab` into `Engine::close_tab`.
        fn full_click(&mut self, local_x: f64) -> (Option<bool>, Option<EngineAction>) {
            handle_mouse_click(
                &mut self.engine,
                CONTENT_X + local_x,
                Self::CLICK_Y,
                false, // alt
                self.line_height,
                self.char_width,
                &self.screen,
                &self.tab_pixel_hits,
                None,
                &mut quadraui::DragState::default(),
            )
        }
    }

    /// Clicking a non-active tab in a single-group layout must resolve as a
    /// tab-bar hit and activate that tab.
    #[test]
    fn single_group_tab_click_activates_that_tab_via_click_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;

        let target = f.click(tab_body_local_x(0));
        assert!(
            matches!(target, ClickTarget::TabBar),
            "a single-group click on tab 0's body must resolve as a tab-bar hit, got {target:?} \
             (pre-fix this was ClickTarget::None — the click missed the bar entirely)"
        );
        assert_eq!(
            f.engine.editor_groups[&group].active_tab, 0,
            "clicking tab 0 in a single-group layout must activate it (#553)"
        );
    }

    /// Clicking a tab's × in a single-group layout must resolve to `CloseTab`
    /// for that tab and actually close it.
    #[test]
    fn single_group_tab_close_click_targets_and_closes_that_tab_via_click_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;

        let target = f.click(tab_close_local_x(1));
        assert_eq!(
            target,
            ClickTarget::CloseTab(group, 1),
            "a single-group click on tab 1's × must resolve to CloseTab for tab 1 \
             (pre-fix: ClickTarget::None, so nothing ever closed)"
        );

        let before = f.engine.editor_groups[&group].tabs.len();
        let (dirty_confirm, _) = f.full_click(tab_close_local_x(1));
        assert_eq!(
            dirty_confirm, None,
            "fixture buffers are unmodified, so no dirty-buffer confirm should intercept the close"
        );
        assert_eq!(
            f.engine.editor_groups[&group].tabs.len(),
            before - 1,
            "clicking a tab's × in a single-group layout must close it (#553)"
        );
    }

    /// The split-right button routes through the full production
    /// `handle_mouse_click` entry point against the shared engine call, so a
    /// future arm added to `handle_tab_bar_click` reaches GTK without anyone
    /// having to hand-port it here.
    #[test]
    fn tab_bar_split_right_button_opens_new_group_via_shared_engine_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;
        let groups_before = f.engine.editor_groups.len();

        // Place a synthetic SplitRight button segment just past the three
        // tabs, mirroring the disjoint right-side button zones the
        // rasteriser lays out in production.
        let seg_start = TAB_W * 3.0;
        let seg_end = seg_start + 24.0;
        f.tab_pixel_hits
            .get_mut(&group.0)
            .unwrap()
            .1
            .hit_regions
            .push((
                quadraui::Rect::new(seg_start as f32, 0.0, (seg_end - seg_start) as f32, 24.0),
                quadraui::TabBarHit::RightSegment(quadraui::WidgetId::new("tab:split_right")),
            ));

        let (click_result, engine_action) = f.full_click(seg_start + 5.0);
        assert_eq!(
            click_result, None,
            "a split-button click is not a buffer click"
        );
        assert!(engine_action.is_none());
        assert_eq!(
            f.engine.editor_groups.len(),
            groups_before + 1,
            "clicking the tab bar's split-right button must open a new editor group, \
             routed through Engine::handle_tab_bar_click (#814)"
        );
    }
}
