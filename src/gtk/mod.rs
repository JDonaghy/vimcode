// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional
// TODO: Migrate to ListView/ColumnView in a future phase
#![allow(deprecated)]

use std::path::PathBuf;

pub(crate) mod backend;
pub(crate) mod click;
pub(crate) mod css;
mod events;
mod explorer;
mod services;
// #657: also compiled under `test-support` so the sealed acceptance suite in
// `tests/acceptance.rs` — a separate crate — can reach the #646 harness.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub(crate) mod util;

use util::*;

// #785: `App` now lives in `crate::app`. Re-exported here so the GTK
// backend's own submodules (`click`, `testing`, …) keep resolving it as
// `super::App`, exactly as they did while the type was defined in this file.
pub(crate) use crate::app::App;

// #862: `is_ext_panel_id`, the UI-font helpers, the tab-bar pixel-geometry
// types/functions, `StatusSegmentMap`, `compute_editor_window_rects` and the
// h-scrollbar geometry/hit-test functions all moved to the backend-neutral
// `crate::app_support` and `crate::click` — none of them named a `gtk4`/
// `pango`/`gio` type, so nesting them in `crate::gtk` (behind the `gui`
// feature) only blocked `crate::app` from resolving them without GTK. These
// re-exports keep every existing reference in this module (and its `click`/
// `testing` submodules) resolving unchanged. Only the `#[cfg(test)]`
// submodules below and in `click.rs`/`testing.rs` reach them through `super::`
// today, so a plain (non-test) build sees them as unused.
#[allow(unused_imports)]
pub(crate) use crate::app_support::*;
#[allow(unused_imports)]
pub(crate) use crate::click::{TabBarPixelHits, TabPixelHitMap};

/// Entry point for GTK mode.
///
/// `pub` rather than `pub(crate)` since #657: the caller is `src/main.rs`,
/// which is now a separate crate from this module's.
pub fn run(file_path: Option<PathBuf>) {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        std::env::set_var("DISPLAY", ":0");
    }

    // Install panic hook that flushes swap files + writes crash log.
    crate::core::swap::install_gui_crash_hook();

    install_icon_and_desktop();
    unsafe {
        gtk4::glib::ffi::g_log_set_writer_func(Some(gtk_log_writer), std::ptr::null_mut(), None);
    }
    // Initialize GTK before App::new() so that CssProvider, Display,
    // and Settings calls inside App::new() find an initialized toolkit.
    // Under the old Relm4 path this happened inside RelmApp::create_and_run();
    // with the ShellApp runner it happens inside gapp.run() which is called
    // by run_with_shell() — too late for App::new().
    gtk4::init().expect("Failed to initialize GTK");
    // Create the App and run via the quadraui ShellApp runner.
    // The runner creates its own GTK Application + window; vimcode's engine
    // and event handling are wired in via impl ShellApp for App above.
    //
    // The concrete backend is chosen here, at the GTK entry point, and
    // handed to `App::new` rather than `App` constructing one itself
    // (#861) — this is the seam a future non-GTK wrapper (#859) would pass
    // a different `TextMetricsBackend` impl through.
    let text_metrics_backend: std::rc::Rc<
        std::cell::RefCell<Box<dyn crate::app::TextMetricsBackend>>,
    > = std::rc::Rc::new(std::cell::RefCell::new(
        Box::new(backend::GtkBackend::new()),
    ));
    let vimcode_app = App::new(file_path, text_metrics_backend);
    let config = build_shell_config(&vimcode_app);
    quadraui::gtk::shell_runner::run_with_shell(vimcode_app, config);
}

/// Derive the runner's [`quadraui::ShellConfig`] from an [`App`]'s engine state.
///
/// Split out of [`run`] (#646) so the headless test harness
/// (`crate::gtk::testing`) can hand `driver_with_shell` the *same* config the
/// live runner uses, instead of a hand-written approximation that would drift
/// from it silently (the failure mode the TUI side's `config()` test helper
/// already has to work around).
///
/// #866: used to carry its own full copy of the panel-icon-mapping/
/// title-bar/sidebar-clamp logic — see [`App::shell_config`]'s doc comment
/// for why that duplication existed and why it's gone now. This is a thin
/// wrapper adding only the two builders that are genuinely X11/Wayland-WM
/// concepts with no cross-platform meaning (a macOS/Windows app identifies
/// itself to its WM/shell through a different mechanism entirely —
/// `Info.plist` / the executable's embedded manifest, not a runtime string).
/// `pub(crate)` (not private) so `src/win/mod.rs::run` and a future
/// `src/win/testing.rs` harness can reach it without copying it a third
/// time, even though today's Win-GUI/macOS entry points call
/// `app.shell_config()` directly and never need the WM-only extras this
/// adds.
pub(crate) fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    app.shell_config()
        // #719: quadraui#656 builders — route the WM app id / icon name
        // through the single `APP_ID` constant #716 introduced, rather than
        // a fresh string literal, so there's exactly one identity string.
        .with_app_id(util::APP_ID)
        .with_icon_name(util::APP_ID)
}

// #731: the `native_scrollbar_placement_tests` module that used to live
// here (#723) tested `native_scrollbar_margin_start`'s pure inset
// arithmetic — that function guarded the native `gtk4::Scrollbar` overlay
// path deleted by this issue (`sync_scrollbar`/`create_window_scrollbars`),
// which never ran under the ShellApp runner in the first place (nothing
// assigns `self.overlay`/`self.drawing_area`), so #723's fix was never
// live on screen. See the doc comment above the `Surface::Editor` push in
// `render_content` for where the minimap-inset decision needs to move
// (quadraui's `gtk::editor::draw_editor`, mirroring TUI's inline
// scrollbar column) and the quadraui issue that needs filing first.

#[cfg(test)]
mod h_scrollbar_status_offset_tests {
    //! #728: `h_scrollbar_geometry`'s status-row offset used to check
    //! `window_status_line && !terminal_maximized` directly, while
    //! `render::build_screen_layout`'s reservation of that same row used
    //! `per_window_status && !separate_status` — two independent answers to
    //! "is a per-window status row painted here", each covering an axis the
    //! other didn't (`terminal_maximized` vs. `separate_status`). Both now
    //! go through `render::window_status_row_reserved`; these pin that the
    //! scrollbar's track actually moves in lockstep with it rather than
    //! re-diverging.
    use super::h_scrollbar_geometry;
    use crate::core::{Engine, WindowRect};

    /// A window whose longest line overflows a narrow viewport, so
    /// `h_scrollbar_geometry` returns `Some` rather than `None` ("content
    /// fits" — nothing to offset).
    fn engine_needing_h_scrollbar() -> Engine {
        let mut e = Engine::new_for_test();
        e.buffer_mut().insert(0, &"x".repeat(500));
        // `max_col` (what `h_scrollbar_geometry` reads) is a cache
        // refreshed by `update_syntax`, not by a raw `Buffer::insert` —
        // force it so the 500-char line above is actually reflected.
        let wid = e.active_window_id();
        let buffer_id = e.windows.get(&wid).unwrap().buffer_id;
        e.buffer_manager.get_mut(buffer_id).unwrap().update_syntax();
        e
    }

    #[test]
    fn track_moves_up_by_exactly_one_row_when_the_status_row_is_reserved() {
        let mut e = engine_needing_h_scrollbar();
        e.settings.window_status_line = true;
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
        let line_height = 20.0;

        let (_, track_y_with, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("an overflowing line needs an h-scrollbar");

        e.settings.window_status_line = false;
        let (_, track_y_without, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("still overflowing with the status line off");

        assert_eq!(
            track_y_without - track_y_with,
            line_height,
            "the status row must shift the h-scrollbar up by exactly one line_height"
        );
    }

    /// #728 regression: with `status_line_above_terminal` OFF and the
    /// bottom panel open, the active window's status is pulled into a
    /// *separated* bar above the terminal instead of painting inside this
    /// window — `render::window_status_row_reserved` reports the row as
    /// free, and the h-scrollbar must agree. The old
    /// `window_status_line && !terminal_maximized` predicate never checked
    /// this axis and would have offset for a row nothing paints here.
    /// RED against that predicate (verified while writing this fix): 13.0
    /// vs. 33.0 — the old code offset the track by a full `line_height` for
    /// a status row that was actually painted as a separated bar elsewhere.
    #[test]
    fn track_does_not_move_when_status_is_separated_above_the_terminal() {
        let mut e = engine_needing_h_scrollbar();
        e.settings.window_status_line = true;
        e.settings.status_line_above_terminal = false;
        e.terminal_open = true;
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
        let line_height = 20.0;

        let (_, track_y_separated, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("an overflowing line needs an h-scrollbar");

        e.settings.window_status_line = false;
        let (_, track_y_no_status, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("still overflowing with the status line off");

        assert_eq!(
            track_y_separated, track_y_no_status,
            "a separated status bar must not offset the h-scrollbar — this \
             window's own bottom row is free"
        );
    }

    /// #728 regression: while the terminal panel is maximized, editor
    /// windows are not the visible surface, so nothing paints a per-window
    /// status row even with the setting on — the h-scrollbar must not
    /// offset for one. This is the axis `build_screen_layout`'s old
    /// predicate never checked (only GTK's did).
    #[test]
    fn track_does_not_move_when_the_terminal_is_maximized() {
        let mut e = engine_needing_h_scrollbar();
        e.settings.window_status_line = true;
        e.terminal_maximized = true;
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
        let line_height = 20.0;

        let (_, track_y_maximized, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("an overflowing line needs an h-scrollbar");

        e.settings.window_status_line = false;
        let (_, track_y_no_status, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("still overflowing with the status line off");

        assert_eq!(track_y_maximized, track_y_no_status);
    }
}

#[cfg(test)]
mod shell_config_identity_tests {
    //! #719: quadraui#656/#657 landed `ShellConfig::with_app_id()` /
    //! `with_icon_name()` / `with_activity_bar_width_px()`, but a pin bump
    //! alone doesn't prove `build_shell_config` actually calls them — a
    //! headless build can't assert the WM taskbar/alt-tab icon (that's the
    //! SMOKE_TESTS item), but it *can* assert the values reach the
    //! `ShellConfig` GTK's toplevel is built from, which is the only part
    //! of this fix source-level tests can reach.
    use super::{build_shell_config, util, App};
    use crate::core::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn app_id_and_icon_name_reach_shell_config() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(engine);
        let config = build_shell_config(&app);
        assert_eq!(config.app_id, util::APP_ID);
        assert_eq!(config.icon_name.as_deref(), Some(util::APP_ID));
    }

    #[test]
    fn activity_bar_pinned_to_48px_matching_its_own_row_height() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(engine);
        let config = build_shell_config(&app);
        assert_eq!(config.activity_bar_width_px, Some(48.0));
    }
}

#[cfg(test)]
mod chrome_paint_tests {
    //! Headless pixel-paint regression test for the CSD title bar's inline
    //! window-control buttons (#552). A round-2 smoke test reported the
    //! minimize/maximize/close glyphs as completely invisible even though
    //! their click hit-regions were live. Paints
    //! `render::window_controls_status_bar` into an in-memory Cairo
    //! `ImageSurface` (no display required — same pattern quadraui's own
    //! `gtk/tab_bar.rs` headless paint tests use) and reads back pixels to
    //! confirm the button glyphs actually paint non-background pixels.
    use crate::render::{self, Theme};
    use pangocairo::cairo::{Context, Format, ImageSurface};

    const W: i32 = 400;
    const ROW_H: i32 = 28;
    const LINE_H: f64 = 20.0;

    /// Read an RGB triple from an ARgb32 surface at pixel (x, y).
    fn pixel(data: &[u8], stride: usize, x: i32, y: i32) -> (u8, u8, u8) {
        let off = y as usize * stride + x as usize * 4;
        (data[off + 2], data[off + 1], data[off])
    }

    /// Perceptual (sRGB-weighted) luminance, 0..255.
    fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
        0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
    }

    /// Paint `render::window_controls_status_bar(theme, false)` into a fresh
    /// headless surface and return the max luminance delta, against the
    /// bar's own background fill, found **within each button's own
    /// hit-region x-range** — keyed by the button's `action_id`.
    ///
    /// #715: the original version of this helper measured the max delta
    /// found *anywhere in the whole row*. Three buttons share one row, so
    /// two glyphs painting fine was enough to clear the floor even though
    /// the third (minimize, `U+2500` — a hairline box-drawing rule with no
    /// coverage in the resolved UI font) contributed zero pixels. Per-segment
    /// measurement is the only version of this check that can fail on a
    /// single invisible button rather than needing all three to break at
    /// once. Segment x-ranges come from the `StatusBarLayout` `draw_status_bar`
    /// returns — the same hit-region data the real click handler resolves
    /// against (`render::window_controls_status_bar`'s doc comment) — rather
    /// than hardcoded pixel columns, so a future layout change can't
    /// silently desync the test from what's actually painted.
    ///
    /// A glyph that paints but has near-zero contrast against its own
    /// background (e.g. white-on-near-white) is exactly as invisible to a
    /// user as a glyph that paints nothing at all — a plain "differs from
    /// background" check would pass in both cases, so this measures the
    /// actual perceptual gap instead.
    fn per_segment_contrast_deltas(theme: &Theme) -> Vec<(String, f64)> {
        // Concurrent Pango/Cairo text work from two test threads segfaults
        // inside FreeType — see `src/test_paint.rs`.
        let _paint = crate::test_paint::PaintGuard::acquire();
        let bar = render::window_controls_status_bar(theme, false);

        let mut surface =
            ImageSurface::create(Format::ARgb32, W, ROW_H).expect("create ImageSurface");
        let layout = {
            let cr = Context::new(&surface).expect("Context::new");
            // Fill with a color that can't be confused with any themed fg/bg.
            cr.set_source_rgb(1.0, 0.0, 1.0);
            cr.paint().ok();

            let pango_layout = pangocairo::functions::create_layout(&cr);
            quadraui::gtk::draw_status_bar(
                &cr,
                &pango_layout,
                0.0,
                0.0,
                W as f64,
                LINE_H,
                &bar,
                &render::to_quadraui_theme(theme),
                None,
                None,
            )
        };
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("surface data");

        let bg = {
            let c = theme.tab_bar_bg;
            (c.r, c.g, c.b)
        };
        let bg_lum = luminance(bg);

        layout
            .hit_regions
            .iter()
            .filter_map(|(rect, hit)| match hit {
                quadraui::StatusBarHit::Segment(id) => Some((rect, id)),
                quadraui::StatusBarHit::Empty => None,
            })
            .map(|(rect, id)| {
                let x0 = rect.x.round() as i32;
                let x1 = (rect.x + rect.width).round() as i32;
                let mut max_delta = 0.0f64;
                // Scan every row of the painted surface, not just one
                // mid-line scanline (#715): a thin glyph like an em dash
                // sits at a specific baseline offset that a single sampled
                // row can miss even though the glyph paints fine — that
                // would be a false "invisible" failure caused by the test's
                // own sampling, not a real bug. Scanning the full height
                // means only a genuinely unpainted segment reports zero.
                for y in 0..ROW_H {
                    for x in x0.max(0)..x1.min(W) {
                        let px = pixel(&data, stride, x, y);
                        if px == (255, 0, 255) {
                            continue; // untouched sentinel fill — not part of the bar.
                        }
                        max_delta = max_delta.max((luminance(px) - bg_lum).abs());
                    }
                }
                (id.as_str().to_string(), max_delta)
            })
            .collect()
    }

    /// #552 round-2 smoke test: the minimize/maximize/close glyphs rendered
    /// with zero visible pixels. Root cause: `window_controls_status_bar`
    /// paired its glyph `fg` with `theme.status_fg` (designed to contrast
    /// against `status_bg`, the *bottom* status line's background) instead
    /// of a color actually paired with `tab_bar_bg` — the background this
    /// row uses. The `vs_light` theme (`tab_bar_bg` #ececec, old `status_fg`
    /// #ffffff) rendered white-on-near-white, which is as good as invisible
    /// even though pixels technically get painted. Runs across every
    /// built-in theme (not just the default) so a future contrast
    /// regression on any one theme fails loudly instead of only surfacing
    /// in a manual smoke test against a theme nobody happened to try.
    ///
    /// #715: checked **per button**, not row-max. On the reporter's real GTK
    /// desktop the old minimize glyph (`U+2500`, a box-drawing hairline)
    /// painted zero visible pixels while `□`/`✕` painted fine — a row-max
    /// check only needs *one* of the three buttons visible to pass, so it
    /// shipped anyway. (This headless Cairo/Pango environment happens to
    /// have box-drawing coverage in its fallback font, so it can't reproduce
    /// that exact zero-pixel case — verified instead by temporarily blanking
    /// a segment's glyph entirely, which *is* reproducible headlessly and
    /// exercises the identical "one button visible, one isn't" failure
    /// shape.) Asserting on each of the three `action_id`s independently is
    /// the only version of this check that can catch a single dead button.
    #[test]
    fn window_control_buttons_are_visible_against_their_background_in_every_theme() {
        let expected_actions = [
            render::WINDOW_MINIMIZE_ACTION,
            render::WINDOW_MAXIMIZE_ACTION,
            render::WINDOW_CLOSE_ACTION,
        ];
        for name in Theme::available_names() {
            let theme = Theme::from_name(&name);
            let deltas = per_segment_contrast_deltas(&theme);
            for action in expected_actions {
                let delta = deltas
                    .iter()
                    .find(|(id, _)| id == action)
                    .unwrap_or_else(|| {
                        panic!(
                            "theme {name:?}: no hit-region painted for window-control \
                             action {action:?} — button missing from the row entirely"
                        )
                    })
                    .1;
                // WCAG-ish floor: anything much below this reads as "same
                // color" at a glance, which is exactly the bug this test
                // guards against.
                assert!(
                    delta > 40.0,
                    "theme {name:?}: window-control button {action:?} has only \
                     {delta:.1} luminance contrast against tab_bar_bg — \
                     effectively invisible"
                );
            }
        }
    }
}
