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
    // #1106: this used to silently invent `DISPLAY=:0` whenever neither
    // `DISPLAY` nor `WAYLAND_DISPLAY` was set. That's a guess, not a
    // fallback — `:0` is far from guaranteed to be the display anyone
    // actually wants (Xvfb commonly picks `:99`, a second X session `:1`,
    // etc.), and if it happens to be wrong `gtk4::init()` below still fails,
    // just later and against a display name nobody chose, which is exactly
    // the "surfaces later and less legibly" failure mode #979 hit for
    // `--help`. Fail loudly here instead, before touching GTK at all, and
    // name the actual problem: no display was configured. (Deliberately not
    // an opt-in env var either — there is no way to guess a *correct*
    // display, only a hardcoded one, so an opt-in would just move the same
    // wrong guess behind a flag.)
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        eprintln!(
            "vimcode: no display found (neither DISPLAY nor WAYLAND_DISPLAY is set).\n\
             The GTK backend needs a running X11 or Wayland session. Set one of\n\
             those environment variables, or run the TUI backend instead (`vcd`),\n\
             which needs neither."
        );
        std::process::exit(1);
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
mod editor_scrollbar_geometry_tests {
    //! #1128: replaces the old `h_scrollbar_status_offset_tests` (#728) —
    //! that module pinned a status-row offset in the pre-#1128 hand-rolled
    //! h-scrollbar geometry helper this issue deleted. quadraui#968 taught
    //! `quadraui::gtk::editor::draw_editor` to paint both editor scrollbars
    //! itself via `Editor::layout`, laid out against the window's raw,
    //! unshrunk rect — it never applies a status-row offset (see
    //! `quadraui::gtk::editor`'s module doc, "Scrollbars" section) — so a
    //! hit-test helper that still applied one would disagree with what's
    //! actually painted: precisely the "hover and paint can disagree by
    //! construction" bug #1128 fixed. These tests pin the replacement
    //! helpers (`editor_scrollbar_layout`/`h_scrollbar_thumb_geometry`)
    //! against that reality instead.
    use super::{editor_scrollbar_layout, h_scrollbar_thumb_geometry};
    use crate::core::{Engine, WindowRect};

    /// A window whose longest line overflows a narrow viewport and whose
    /// buffer is long enough to need both a v-scrollbar and a multi-digit
    /// (wide) line-number gutter — the exact combination the deleted
    /// pre-#1128 helper got wrong: it ignored the gutter offset entirely
    /// and hardcoded an 8px reserve for the v-scrollbar column instead of
    /// reading the real `char_width`-wide one quadraui#968 reserves.
    fn engine_needing_h_scrollbar_with_wide_gutter() -> Engine {
        let mut e = Engine::new_for_test();
        e.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
        let long_line = "x".repeat(500);
        let filler: String = "line\n".repeat(999);
        e.buffer_mut().insert(0, &format!("{long_line}\n{filler}"));
        // `max_col` (what the scrollbar probe reads) is a cache refreshed
        // by `update_syntax`, not by a raw `Buffer::insert` — force it so
        // the 500-char line above is actually reflected.
        let wid = e.active_window_id();
        let buffer_id = e.windows.get(&wid).unwrap().buffer_id;
        e.buffer_manager.get_mut(buffer_id).unwrap().update_syntax();
        e
    }

    /// #1128 regression — RED against the deleted pre-fix helper (verified
    /// while writing this fix: it returned `track_x` 0.0 — gutter ignored
    /// — `track_w` 992.0 and `sb_height` 7.0 — an independently guessed 8px
    /// v-scrollbar reserve and a 0.35×line_height bar height, neither of
    /// which is what quadraui actually paints). The h-scrollbar track must
    /// start after the gutter and stop exactly one `char_width` cell short
    /// of the pane's right edge — the v-scrollbar's own reserved column
    /// (quadraui#968) — not span the full window width minus a hardcoded
    /// guess.
    #[test]
    fn track_starts_after_the_gutter_and_reserves_exactly_one_char_width_for_the_v_scrollbar() {
        let e = engine_needing_h_scrollbar_with_wide_gutter();
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 1000.0, 200.0);
        let char_width = 20.0;
        let line_height = 20.0;

        let (editor, layout) = editor_scrollbar_layout(&e, wid, &rect, char_width, line_height)
            .expect("window and buffer must resolve");
        let h_track = layout
            .h_scrollbar_bounds
            .expect("a 500-char line must overflow a 1000px-wide pane");
        let v_track = layout
            .v_scrollbar_bounds
            .expect("1000 lines must overflow a 10-visible-row pane");

        let gutter_w = editor.gutter_char_width as f64 * char_width;
        assert!(
            gutter_w > char_width,
            "fixture sanity: line numbers up to ~1000 must need more than one gutter column"
        );
        assert_eq!(
            h_track.x as f64,
            rect.x + gutter_w,
            "track must start after the gutter, not at the window's left edge"
        );
        assert_eq!(
            h_track.width as f64,
            rect.width - gutter_w - v_track.width as f64,
            "track must stop short by exactly the v-scrollbar's own reserved \
             column, not a hardcoded 8px"
        );
        assert_eq!(
            v_track.width as f64, char_width,
            "the v-scrollbar's reserved column is one char_width cell \
             (quadraui#968), not a hardcoded pixel constant"
        );
        assert_eq!(
            h_track.height as f64, line_height,
            "the h-scrollbar's own row is one full line_height tall, \
             matching quadraui's paint"
        );

        let (tx, ty, tw, th, ..) =
            h_scrollbar_thumb_geometry(&e, wid, &rect, char_width, line_height)
                .expect("thumb geometry must resolve alongside the track");
        assert_eq!(tx, h_track.x as f64);
        assert_eq!(ty, h_track.y as f64);
        assert_eq!(tw, h_track.width as f64);
        assert_eq!(th, h_track.height as f64);
    }

    fn cfg_status_line(e: &mut Engine) {
        e.settings.window_status_line = true;
    }
    fn cfg_separated_status(e: &mut Engine) {
        e.settings.window_status_line = true;
        e.settings.status_line_above_terminal = false;
        e.terminal_open = true;
    }
    fn cfg_maximized(e: &mut Engine) {
        e.settings.window_status_line = true;
        e.terminal_maximized = true;
    }

    /// #1128 regression: quadraui's real paint
    /// (`quadraui::gtk::editor::draw_editor`) lays scrollbars out against
    /// the window's raw rect and never shrinks it for a per-window status
    /// line first — so the track must not move for any of
    /// `window_status_line`, the "separated status" bottom-panel case, or a
    /// maximized terminal. The deleted pre-#1128 helper offset the track by
    /// a full `line_height` whenever `window_status_line` was on,
    /// disagreeing with paint in every one of these configurations.
    #[test]
    fn status_line_settings_never_move_the_track() {
        let rect = WindowRect::new(0.0, 0.0, 1000.0, 200.0);
        let char_width = 20.0;
        let line_height = 20.0;

        let baseline_y = {
            let e = engine_needing_h_scrollbar_with_wide_gutter();
            let wid = e.active_window_id();
            let (_, layout) = editor_scrollbar_layout(&e, wid, &rect, char_width, line_height)
                .expect("an overflowing line needs an h-scrollbar");
            layout.h_scrollbar_bounds.unwrap().y
        };

        for configure in [
            cfg_status_line as fn(&mut Engine),
            cfg_separated_status,
            cfg_maximized,
        ] {
            let mut e = engine_needing_h_scrollbar_with_wide_gutter();
            configure(&mut e);
            let wid = e.active_window_id();
            let (_, layout) = editor_scrollbar_layout(&e, wid, &rect, char_width, line_height)
                .expect("still overflowing under this configuration");
            assert_eq!(
                layout.h_scrollbar_bounds.unwrap().y,
                baseline_y,
                "no per-window-status-line configuration may move the \
                 h-scrollbar track"
            );
        }
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
                //
                // #934: the floor was 40.0, tuned against freetype's
                // rasterisation. A Darwin/Quartz run measured 36.1 for
                // solarized-dark's minimize glyph — Core Text's
                // gamma-correct AA compositing produces measurably softer
                // (lower-peak-luminance) glyph edges than freetype's for the
                // same thin box-drawing-style pen, without the button being
                // any less visible to a human. 25.0 keeps ~5x headroom
                // above the #552 regression this test exists to catch
                // (literally near-zero delta — white-on-near-white) while
                // absorbing the observed rasteriser gap; it is not tuned
                // against a full Darwin run across every theme/button, only
                // the one reported data point, so treat it as a floor with
                // margin rather than a measured-exact value.
                assert!(
                    delta > 25.0,
                    "theme {name:?}: window-control button {action:?} has only \
                     {delta:.1} luminance contrast against tab_bar_bg — \
                     effectively invisible"
                );
            }
        }
    }
}
