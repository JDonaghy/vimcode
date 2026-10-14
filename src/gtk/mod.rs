// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional
// TODO: Migrate to ListView/ColumnView in a future phase
#![allow(deprecated)]

use std::path::PathBuf;

pub(crate) mod backend;
pub(crate) mod click;
mod events;
mod explorer;
mod services;
// Compiled under `test-support` so the sealed acceptance suite in
// `tests/acceptance.rs` — a separate crate — can reach the testing harness.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub(crate) mod util;

use util::*;

// `App` now lives in `crate::app`. Re-exported here so the GTK
// backend's own submodules (`click`, `testing`, …) keep resolving it as
// `super::App`, exactly as they did while the type was defined in this file.
pub(crate) use crate::app::App;

// `is_ext_panel_id`, the UI-font helpers, the tab-bar pixel-geometry
// types/functions, `StatusSegmentMap` and the scrollbar geometry/hit-test
// functions all moved to the backend-neutral `crate::app_support` and
// `crate::click` — none of them named a `gtk4`/`pango`/`gio` type, so
// nesting them in `crate::gtk` (behind the `gui` feature) only blocked
// `crate::app` from resolving them without GTK. These re-exports keep every
// existing reference in this module (and its `click`/`testing` submodules)
// resolving unchanged.
#[allow(unused_imports)]
pub(crate) use crate::app_support::*;
#[allow(unused_imports)]
pub(crate) use crate::click::GroupTabBarLayoutMap;

/// Entry point for GTK mode.
///
/// `pub` rather than `pub(crate)` because the caller is `src/main.rs`,
/// which is now a separate crate from this module's.
pub fn run(file_path: Option<PathBuf>) {
    // This fails loudly if neither `DISPLAY` nor `WAYLAND_DISPLAY` is set,
    // before touching GTK at all. On platforms where GTK actually talks to
    // X11/Wayland, a configured display is required; on macOS (quartz) and
    // Windows (win32) there is no display variable to set, so this guard
    // would refuse to start a perfectly good GUI build.
    if no_display_configured(
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    ) {
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
    // Initialize GTK before add_icon_theme_search_path()/App::new_portable()
    // so the Display/IconTheme calls below find an initialized toolkit.
    // Under the old Relm4 path this happened inside RelmApp::create_and_run();
    // with the ShellApp runner it happens inside gapp.run() which is called
    // by run_with_shell() — too late for either of those.
    gtk4::init().expect("Failed to initialize GTK");
    // The one GTK-only step `App::new` (now folded into the shared
    // `App::new_portable` every other GUI backend already used — see that
    // constructor's doc) can't do itself — quadraui has no portable
    // icon-theme search-path surface. Must run after `gtk4::init()`, same as
    // `App::new`'s old inline version needed.
    util::add_icon_theme_search_path();
    // Create the App and run via the quadraui ShellApp runner.
    // The runner creates its own GTK Application + window; vimcode's engine
    // and event handling are wired in via impl ShellApp for App above.
    //
    // The concrete backend is chosen here, at the GTK entry point, and
    // handed to `App::new_portable` rather than `App` constructing one
    // itself — this is the seam a future non-GTK wrapper would
    // pass a different `quadraui::Backend` impl through.
    let concrete_backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> =
        std::rc::Rc::new(std::cell::RefCell::new(
            Box::new(backend::GtkBackend::new()),
        ));
    let vimcode_app = App::new_portable(
        file_path,
        concrete_backend,
        crate::render::UnitProfile::px(),
    );
    let config = build_shell_config(&vimcode_app);
    quadraui::gtk::shell_runner::run_with_shell(vimcode_app, config);
}

/// Derive the runner's [`quadraui::ShellConfig`] from an [`App`]'s engine state.
///
/// Split out of [`run`] so the headless test harness can hand `driver_with_shell`
/// the same config the live runner uses.
///
/// This is a thin wrapper adding only the two builders that are genuinely
/// X11/Wayland-WM concepts with no cross-platform meaning (a macOS/Windows app
/// identifies itself to its WM/shell through a different mechanism entirely —
/// `Info.plist` / the executable's embedded manifest, not a runtime string).
/// `pub(crate)` (not private) so `src/win/mod.rs::run` and a future
/// `src/win/testing.rs` harness can reach it without copying it, even though
/// today's Win-GUI/macOS entry points call `app.shell_config()` directly.
pub(crate) fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    app.shell_config()
        // Route the WM app id / icon name through the single `APP_ID` constant,
        // rather than a fresh string literal, so there's exactly one identity string.
        .with_app_id(util::APP_ID)
        .with_icon_name(util::APP_ID)
        // `with_app_icon` is a no-op on GTK today
        // (`ShellConfig::app_icon`'s own doc — GTK keeps using
        // `icon_name`/the desktop-theme icon `util::install_icon_and_desktop`
        // installs). Set anyway, alongside the other two identity builders
        // above, so the three GUI backends build their `ShellConfig`
        // identically and a future GTK consumer of this field needs no new
        // call site.
        .with_app_icon(quadraui::ImageSource::Bytes(
            crate::app_support::APP_ICON_PNG.to_vec(),
        ))
}

/// True when GTK needs an X11/Wayland display and none is configured.
/// Always false on macOS and Windows, whose GTK backends (quartz, win32)
/// don't use `DISPLAY` / `WAYLAND_DISPLAY` at all.
fn no_display_configured(has_wayland: bool, has_x11: bool) -> bool {
    cfg!(not(any(target_os = "macos", target_os = "windows"))) && !has_wayland && !has_x11
}

#[cfg(test)]
mod no_display_guard_tests {
    use super::no_display_configured;

    #[test]
    fn either_display_variable_satisfies_the_guard() {
        assert!(!no_display_configured(true, false));
        assert!(!no_display_configured(false, true));
        assert!(!no_display_configured(true, true));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn no_display_variable_trips_the_guard_on_x11_wayland_platforms() {
        assert!(no_display_configured(false, false));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn guard_never_trips_where_gtk_has_a_native_backend() {
        assert!(!no_display_configured(false, false));
    }
}

#[cfg(test)]
mod editor_scrollbar_geometry_tests {
    //! Tests for the scrollbar geometry helpers (`editor_scrollbar_layout` and
    //! `scrollbar_thumb_geometry`) against the actual layout and painting
    //! quadraui performs: both the hit-test and the painted result must agree.
    use super::{editor_scrollbar_layout, scrollbar_thumb_geometry, ScrollbarAxis};
    use crate::core::{Engine, WindowRect};

    /// A window whose longest line overflows a narrow viewport and whose
    /// buffer is long enough to need both a v-scrollbar and a multi-digit
    /// (wide) line-number gutter — the exact combination where a naive helper
    /// would ignore the gutter offset and fail to reserve space for the
    /// actual `char_width`-wide v-scrollbar column.
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

    /// RED against the deleted pre-fix helper (verified
    /// while writing this fix: it returned `track_x` 0.0 — gutter ignored
    /// — `track_w` 992.0 and `sb_height` 7.0 — an independently guessed 8px
    /// v-scrollbar reserve and a 0.35×line_height bar height, neither of
    /// which is what quadraui actually paints). The h-scrollbar track must
    /// start after the gutter and stop exactly one `char_width` cell short
    /// of the pane's right edge — the v-scrollbar's own reserved column
    /// — not span the full window width minus a hardcoded
    /// guess.
    #[test]
    fn track_starts_after_the_gutter_and_reserves_exactly_one_char_width_for_the_v_scrollbar() {
        let e = engine_needing_h_scrollbar_with_wide_gutter();
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 1000.0, 200.0);
        let char_width = 20.0;
        let line_height = 20.0;

        let (editor, layout) =
            editor_scrollbar_layout(&e, wid, &rect, char_width, line_height, None)
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

        let (tx, ty, tw, th, ..) = scrollbar_thumb_geometry(
            &e,
            wid,
            &rect,
            char_width,
            line_height,
            None,
            ScrollbarAxis::Horizontal,
        )
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

    /// Quadraui's real paint lays scrollbars out against the window's raw rect
    /// and never shrinks it for a per-window status line first — so the track
    /// must not move for any of `window_status_line`, the "separated status"
    /// bottom-panel case, or a maximized terminal.
    #[test]
    fn status_line_settings_never_move_the_track() {
        let rect = WindowRect::new(0.0, 0.0, 1000.0, 200.0);
        let char_width = 20.0;
        let line_height = 20.0;

        let baseline_y = {
            let e = engine_needing_h_scrollbar_with_wide_gutter();
            let wid = e.active_window_id();
            let (_, layout) =
                editor_scrollbar_layout(&e, wid, &rect, char_width, line_height, None)
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
            let (_, layout) =
                editor_scrollbar_layout(&e, wid, &rect, char_width, line_height, None)
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
    //! Tests that `build_shell_config` actually calls the WM identity builders
    //! (`with_app_id()`, `with_icon_name()`) — the values must reach the
    //! `ShellConfig` GTK's toplevel is built from.
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

    /// Same reasoning as the identity test above —
    /// GTK ignores `ShellConfig::app_icon` at paint time (there is no
    /// headless "Dock"/taskbar to render into and assert on; that's the
    /// SMOKE_TESTS item), but a headless build *can* assert the bytes
    /// actually reach the `ShellConfig` every GUI backend's runner is
    /// handed, and that they decode as a real image rather than empty or
    /// truncated bytes.
    #[test]
    fn app_icon_reaches_shell_config_as_a_decodable_image() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(engine);
        let config = build_shell_config(&app);
        let quadraui::ImageSource::Bytes(bytes) = config
            .app_icon
            .expect("build_shell_config sets an app icon")
        else {
            panic!("app icon should be embedded bytes, not a path");
        };
        assert!(!bytes.is_empty());
        // PNG decoding needs no `librsvg2-common` loader (unlike the SVG
        // probe `host_has_svg_loader` above) — core `gdk-pixbuf` decodes PNG
        // unconditionally, so this assertion runs on every host, CI included.
        let decoded = gtk4::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(bytes))
            .expect("app icon bytes must decode as an image");
        assert!(decoded.width() > 0 && decoded.height() > 0);
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
    //! window-control buttons. A round-2 smoke test reported the
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

    /// The sentinel fill the headless surface is primed with before the bar
    /// paints — a color no theme uses, so "is this pixel part of the painted
    /// bar?" is answerable per-pixel.
    const SENTINEL: (u8, u8, u8) = (255, 0, 255);

    /// Paint `render::window_controls_status_bar(theme, false)` into a fresh
    /// `W` × `ROW_H` headless surface, asking for a bar `LINE_H` tall, and
    /// return `(pixel bytes, stride, StatusBarLayout)`.
    ///
    /// The surface is primed with [`SENTINEL`] first, so every byte the bar
    /// did *not* touch is identifiable afterwards — which is what lets both
    /// callers below distinguish "painted the theme background" from "never
    /// painted at all".
    fn paint_window_controls_bar(theme: &Theme) -> (Vec<u8>, usize, quadraui::StatusBarLayout) {
        // Concurrent Pango/Cairo text work from two test threads segfaults
        // inside FreeType — see `src/test_paint.rs`.
        let _paint = crate::test_paint::PaintGuard::acquire();
        let bar = render::window_controls_status_bar(theme, false);

        let mut surface =
            ImageSurface::create(Format::ARgb32, W, ROW_H).expect("create ImageSurface");
        let layout = {
            let cr = Context::new(&surface).expect("Context::new");
            let (r, g, b) = SENTINEL;
            cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
            cr.paint().ok();

            let pango_layout = pangocairo::functions::create_layout(&cr);
            // Use a real `GtkBackend` the same way `click.rs`'s own headless
            // paint tests do, rather than calling the deprecated
            // `quadraui::gtk::draw_status_bar` free function directly.
            //
            // The bar's painted height comes from `rect.height` and `set_current_line_height`
            // is set to match, so the backend's per-frame line height agrees with
            // the rect.
            use quadraui::Backend as _;
            let mut backend = super::backend::GtkBackend::new();
            backend.set_current_theme(render::to_quadraui_theme(theme));
            backend.set_current_line_height(LINE_H);
            backend.enter_frame_scope(&cr, &pango_layout, |b| {
                b.draw_status_bar_interactive(
                    quadraui::Rect::new(0.0, 0.0, W as f32, LINE_H as f32),
                    &bar,
                    &quadraui::InteractionState::new(),
                )
            })
        };
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("surface data").to_vec();
        (data, stride, layout)
    }

    /// The painted bar must be exactly `LINE_H` tall.
    ///
    /// The migration off the deprecated `quadraui::gtk::draw_status_bar` free
    /// function (which took `line_height` as a positional argument) onto
    /// `Backend::draw_status_bar_interactive` (which takes it inside the
    /// `Rect`) moved *where* the line height is passed. This asserts the
    /// painted result is unchanged by that move: rows `0..LINE_H` are filled
    /// and rows `LINE_H..ROW_H` are still untouched [`SENTINEL`]. A bar
    /// painted at `GtkBackend::new`'s 16px default instead of the requested
    /// 20px fails here on both halves.
    #[test]
    fn paints_the_bar_at_the_requested_line_height() {
        let theme = Theme::from_name(&Theme::available_names()[0]);
        let (data, stride, _layout) = paint_window_controls_bar(&theme);

        let row_is_all_sentinel = |y: i32| (0..W).all(|x| pixel(&data, stride, x, y) == SENTINEL);

        for y in 0..LINE_H as i32 {
            assert!(
                !row_is_all_sentinel(y),
                "row {y} of the bar's requested {LINE_H}px height was never painted — \
                 the bar is shorter than the rect it was given"
            );
        }
        for y in LINE_H as i32..ROW_H {
            assert!(
                row_is_all_sentinel(y),
                "row {y} is below the bar's requested {LINE_H}px height but got painted — \
                 the bar is taller than the rect it was given"
            );
        }
    }

    /// Paint `render::window_controls_status_bar(theme, false)` into a fresh
    /// headless surface and return the max luminance delta, against the
    /// bar's own background fill, found **within each button's own
    /// hit-region x-range** — keyed by the button's `action_id`.
    ///
    /// The original version of this helper measured the max delta
    /// found *anywhere in the whole row*. Three buttons share one row, so
    /// two glyphs painting fine was enough to clear the floor even though
    /// the third (minimize, `U+2500` — a hairline box-drawing rule with no
    /// coverage in the resolved UI font) contributed zero pixels. Per-segment
    /// measurement is the only version of this check that can fail on a
    /// single invisible button rather than needing all three to break at
    /// once. Segment x-ranges come from the `StatusBarLayout`
    /// `Backend::draw_status_bar_interactive` returns — the same hit-region
    /// data the real click handler resolves against
    /// (`render::window_controls_status_bar`'s doc comment) — rather than
    /// hardcoded pixel columns, so a future layout change can't silently
    /// desync the test from what's actually painted.
    ///
    /// A glyph that paints but has near-zero contrast against its own
    /// background (e.g. white-on-near-white) is exactly as invisible to a
    /// user as a glyph that paints nothing at all — a plain "differs from
    /// background" check would pass in both cases, so this measures the
    /// actual perceptual gap instead.
    fn per_segment_contrast_deltas(theme: &Theme) -> Vec<(String, f64)> {
        let (data, stride, layout) = paint_window_controls_bar(theme);

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
                // mid-line scanline: a thin glyph like an em dash
                // sits at a specific baseline offset that a single sampled
                // row can miss even though the glyph paints fine — that
                // would be a false "invisible" failure caused by the test's
                // own sampling, not a real bug. Scanning the full height
                // means only a genuinely unpainted segment reports zero.
                for y in 0..ROW_H {
                    for x in x0.max(0)..x1.min(W) {
                        let px = pixel(&data, stride, x, y);
                        if px == SENTINEL {
                            continue; // untouched sentinel fill — not part of the bar.
                        }
                        max_delta = max_delta.max((luminance(px) - bg_lum).abs());
                    }
                }
                (id.as_str().to_string(), max_delta)
            })
            .collect()
    }

    /// Test that minimize/maximize/close glyphs are visible against the tab bar
    /// background in every built-in theme, across every theme variant.
    ///
    /// Checked per button, not row-max: if any one of the three buttons becomes
    /// invisible due to contrast, this test will catch it.
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
                // WCAG-ish floor: anything much below this reads as "same color"
                // at a glance. The floor of 25.0 keeps headroom above near-zero
                // contrast while absorbing observed rasteriser differences.
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
