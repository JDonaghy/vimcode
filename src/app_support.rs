//! Backend-neutral shell support functions used by `crate::app::App`.
//!
//! Every item here is pure computation over `Engine`/`quadraui` geometry and
//! string data — none of it names a `gtk4`/`pango`/`gio` type. `src/gtk/mod.rs`
//! and `src/gtk/util.rs` re-export everything below so the rest of `crate::gtk`
//! resolves these names unchanged. The genuinely GTK-only siblings
//! (`util::install_icon_and_desktop`, `util::add_icon_theme_search_path`, ...)
//! are in `src/gtk/`.
use crate::core;
use crate::core::Engine;
use crate::render;

use std::collections::HashMap;

pub(crate) fn is_ext_panel_id(id: &str) -> bool {
    id.starts_with("ext:")
}

/// Pango font family for UI panels (menu bar, sidebars, dropdown,
/// dialogs, hover popups). Size is appended at use via [`UI_FONT`]
/// from the configured `settings.ui_font_size`.
///
/// The list (`"Cantarell, Ubuntu, Segoe UI, Droid Sans, Sans"`) places
/// the two real Linux desktop UI fonts — Cantarell (GNOME) and Ubuntu
/// (Ubuntu/Unity) — first, ahead of Windows/legacy names kept for
/// hypothetical native-Windows GTK builds; `Sans` is the final catch-all.
/// On systems without the preferred fonts, fontconfig falls through to
/// `Sans`, ensuring *a* font always resolves.
const UI_FONT_FAMILY: &str = "Cantarell, Ubuntu, Segoe UI, Droid Sans, Sans";

thread_local! {
    /// Per-thread UI font size (points). Synced from
    /// `settings.ui_font_size` at the start of each frame by
    /// [`sync_ui_font_size`]. Read everywhere a Pango font description
    /// is built — avoids threading `&Settings` through every draw
    /// function for what's effectively one shared knob.
    ///
    /// **Thread-local, not a process-global `AtomicU8`.** Production is
    /// unaffected: every writer and reader runs on the GTK main thread, so
    /// "the current frame's font size" is the same value either way. The test
    /// suite is not — `#[test]`s each get their own thread and `cargo test`
    /// runs them in parallel, so a `GtkDriver` test that sets
    /// `settings.ui_font_size = 8`/`28` (see `testing.rs`'s
    /// `ui_font_size_changes_the_painted_*` cases) could store its size
    /// *between* another test's `sync_ui_font_size` and that test's
    /// `backend.set_ui_font(&UI_FONT())` a few lines later. The victim's frame
    /// then measured its chrome at the wrong point size, its breadcrumb glyphs
    /// landed outside the segment rect the hit region reported, and
    /// `breadcrumb_path_paints_dimmer_than_editor_body_text` read editor body
    /// text instead — a ~1-in-3 flake that reproduced only under parallel
    /// `cargo test --lib`, never with `--test-threads=1`.
    static UI_FONT_SIZE: std::cell::Cell<u8> = const { std::cell::Cell::new(10) };
}

/// Update this thread's UI font size from `settings`, resolving it through
/// `backend`'s platform-native convention when the user has never
/// customized `ui_font_size` away from its compile-time default.
/// See [`core::settings::Settings::effective_ui_font_size`]'s doc for the
/// "still at its default means never customized" test. Called once per
/// frame at the top of [`App::render_content`].
///
/// `.max(6)` ensures no chrome paint call reads a sub-paintable size.
/// Every call site ultimately reads through [`UI_FONT`].
pub(crate) fn sync_ui_font_size(
    settings: &core::settings::Settings,
    backend: &dyn quadraui::Backend,
) {
    let size = resolve_ui_font_size(settings, backend);
    UI_FONT_SIZE.with(|s| s.set(size.max(6)));
}

/// Resolve this session's editor font `(family, size_pt)` — the user's
/// explicit `settings.font_family`/`font_size` if they have ever set one,
/// else `backend`'s own platform-native convention.
/// Thin wrapper over [`core::settings::Settings::effective_editor_font`];
/// exists so every call site (the `ShellConfig`-construction pre-seed in
/// `App::shell_config` and the per-frame sync in
/// `App::sync_per_frame_backend_state`) asks `backend` for its defaults
/// the same way, rather than each inlining its own `backend.default_fonts()`
/// call.
pub(crate) fn resolve_editor_font(
    settings: &core::settings::Settings,
    backend: &dyn quadraui::Backend,
) -> (String, f32) {
    let defaults = backend.default_fonts();
    settings.effective_editor_font(&defaults.editor_family, defaults.editor_size_pt)
}

/// Resolve this session's UI/chrome font size in points — [`resolve_editor_font`]'s
/// twin for `settings.ui_font_size`. Thin wrapper over
/// [`core::settings::Settings::effective_ui_font_size`].
pub(crate) fn resolve_ui_font_size(
    settings: &core::settings::Settings,
    backend: &dyn quadraui::Backend,
) -> u8 {
    settings.effective_ui_font_size(backend.default_fonts().ui_size_pt)
}

/// Effective editor row pitch in pixels for this frame, given the point
/// size [`resolve_editor_font`] just resolved — [`resolve_editor_font`]'s
/// line-height twin.
///
/// `None` means "leave `backend`'s own natural font-metric line height
/// alone" — every backend except native macOS; see
/// [`core::settings::Settings::effective_line_height_multiplier`]'s doc.
///
/// `Some(px)` callers overwrite the live backend's current line height with
/// [`quadraui::Backend::set_current_line_height`], mirroring how
/// [`resolve_editor_font`]'s caller pushes its result through
/// `Backend::set_editor_font`.
///
/// Despite the name, this multiplies a *point* size (`editor_size_pt`) with
/// no pt→px conversion — true today because `effective_line_height_multiplier`
/// returns `None` on every backend except native macOS, and `MacBackend`/
/// Core Text treat 1pt == 1px. A future GTK/Win-GUI caller would need to
/// revisit this if either backend's pt/px ratio ever differs from 1:1.
///
/// The result is floored at 1px: `settings.line_height`'s clamp
/// (`0.0..=5.0`) can yield values rounding to 0, and a 0px row pitch would
/// cause division-by-zero in `*_layout` helpers.
pub(crate) fn resolve_editor_line_height_px(
    settings: &core::settings::Settings,
    backend: &dyn quadraui::Backend,
    editor_size_pt: f32,
) -> Option<f32> {
    let multiplier =
        settings.effective_line_height_multiplier(backend.services().platform_name())?;
    Some(((editor_size_pt as f64 * multiplier).round() as f32).max(1.0))
}

/// Pango font description string for UI chrome at the currently
/// configured size. Call sites do `FontDescription::from_string(&UI_FONT())`.
#[allow(non_snake_case)]
pub(crate) fn UI_FONT() -> String {
    format!("{} {}", UI_FONT_FAMILY, UI_FONT_SIZE.with(|s| s.get()))
}

/// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`).
/// See `ShellApp::cached_tab_slots_abs` for the full doc comment.
pub(crate) type TabSlotsAbsMap = HashMap<usize, Vec<(f32, f32)>>;

/// What the git sidebar's commit-message `TextInput` border costs vertically in
/// GTK's native unit: 1px on top + 1px on bottom. TUI's whole-cell equivalent
/// is `render::sc_commit_input_box_height`'s `+ 2` rows. Fed to
/// `render::sc_sidebar_bands` by both the painter and the click router so the
/// two agree.
pub(crate) const SC_COMMIT_BORDER_PX: f32 = 2.0;

/// Cached per-window status segment hit zones: window_id -> Vec<(start_x, end_x, action)>.
/// Populated during `render_content`'s per-window/separated status bar paint,
/// consumed by click hit-testing.
pub(crate) type StatusSegmentMap =
    HashMap<usize, Vec<(f64, f64, crate::core::engine::StatusAction)>>;

/// Build the layout-only [`quadraui::Editor`] needed to call
/// [`quadraui::Editor::layout`] for scrollbar geometry.
///
/// `Editor::layout`/`layout_with_options` only ever read
/// `total_lines`/`max_col`/`gutter_char_width` off the struct — never
/// `.lines`, any paint-only cosmetic field, or even `.rect` itself (the
/// `viewport` argument passed to `.layout()` is authoritative). Building the
/// full `RenderedWindow` paint uses would mean re-rendering the buffer's
/// visible text on every mouse motion just to throw it away — this builds
/// the cheap subset instead. Every field not set below stays at
/// `Editor::new`'s empty default.
fn scrollbar_probe_editor(engine: &Engine, window_id: core::WindowId) -> Option<quadraui::Editor> {
    let window = engine.windows.get(&window_id)?;
    let buffer_state = engine.buffer_manager.get(window.buffer_id)?;

    // Mirrors `render::build_rendered_window`'s `has_git`/`has_bp`/
    // `line_number_mode` → `gutter_char_width` computation, so a wide
    // gutter (line numbers, git column, breakpoint column) shifts this
    // probe's scrollbar geometry by exactly the amount it shifts paint's.
    let has_git = !buffer_state.git_diff.is_empty();
    let bp_key = buffer_state
        .file_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let has_bp = engine
        .dap_breakpoints
        .get(&bp_key)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || engine.dap_session_active;
    let line_number_mode = if buffer_state.md_rendered.is_some() {
        core::settings::LineNumberMode::None
    } else {
        engine.settings.line_numbers
    };
    let total_lines = buffer_state.buffer.len_lines();
    let gutter_char_width =
        render::calculate_gutter_cols(line_number_mode, total_lines, 0.0, has_git, has_bp);

    Some(
        quadraui::Editor::new(
            quadraui::WidgetId::new("scrollbar_probe"),
            quadraui::Rect::new(0.0, 0.0, 0.0, 0.0),
        )
        .with_scroll_top(window.view.scroll_top)
        .with_scroll_left(window.view.scroll_left)
        .with_total_lines(total_lines)
        .with_max_col(buffer_state.max_col)
        .with_gutter_char_width(gutter_char_width)
        .with_tabstop(engine.settings.tabstop.max(1) as usize)
        .with_lightbulb_glyph('\0'),
    )
}

/// This window's [`quadraui::Editor`] + [`quadraui::EditorLayout`], laid
/// out against `rect` at `char_width`/`line_height` — the same
/// [`quadraui::Editor::layout_with_options_and_v_scrollbar_w`] call paint
/// makes (via `backend.set_editor_v_scrollbar_width`/
/// `backend.editor_v_scrollbar_width`), so `.h_scrollbar_bounds`/
/// `.v_scrollbar_bounds` are never independently re-derived.
///
/// `v_scrollbar_w` must be the same value the backend painted with.
/// Callers with a `&dyn quadraui::Backend` in scope pass
/// `backend.editor_v_scrollbar_width()` through unchanged. Passing `None`
/// when the real backend has an override set would reintroduce paint↔hit-test
/// divergence: quadraui defaults an unset `v_scrollbar_w` to `cell_width`
/// (~7-9px), narrower than the 14px column paint actually draws on GTK/
/// macOS/Win, so hit-testing would miss clicks on the outer pixels. The
/// horizontal scrollbar's track width also depends on `v_scrollbar_w`.
///
/// `rect` is handed to `.layout_with_options_and_v_scrollbar_w()`
/// unmodified, exactly as `quadraui::gtk::editor::draw_editor_with_options`
/// hands it `editor.rect` unmodified — including for a window with its own
/// per-window status line. Current paint does **not** shrink `rect`
/// before laying out scrollbars, so the painted scrollbar can currently
/// run under where the status line paints afterward; this overlap is
/// pre-existing.
pub(crate) fn editor_scrollbar_layout(
    engine: &Engine,
    window_id: core::WindowId,
    rect: &core::WindowRect,
    char_width: f64,
    line_height: f64,
    v_scrollbar_w: Option<f32>,
) -> Option<(quadraui::Editor, quadraui::EditorLayout)> {
    let editor = scrollbar_probe_editor(engine, window_id)?;
    let viewport = quadraui::Rect::new(
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
    );
    let layout = editor.layout_with_options_and_v_scrollbar_w(
        viewport,
        char_width as f32,
        line_height as f32,
        quadraui::EditorPaintOptions::default(),
        v_scrollbar_w,
    );
    Some((editor, layout))
}

/// Which editor scrollbar a geometry/hit-test call is about.
///
/// Horizontal and vertical scrollbars are laid out by the identical
/// [`quadraui::Editor::layout`] call and hit-tested by the identical
/// [`quadraui::EditorLayout::hit_test`] — the only per-axis facts are which
/// `EditorLayout` bounds field to read, which `Editor`/`window.view` scroll
/// field drives the thumb, and which `quadraui::DragTarget` variant a thumb
/// grab arms. [`scrollbar_thumb_geometry`] and [`scrollbar_hit_test`] take
/// this enum instead of existing twice (`h_scrollbar_thumb_geometry`/
/// `v_scrollbar_thumb_geometry`, `h_scrollbar_hit_test`/
/// `v_scrollbar_hit_test`) — the two copies had already drifted once: the
/// horizontal hit-test used inclusive track bounds (`<=`) while the
/// vertical one had been fixed to half-open (`<`) to avoid swallowing the
/// group-divider's own hit zone. Reading both through
/// `quadraui::EditorLayout::hit_test` below means that half-open convention
/// can never re-diverge per axis again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollbarAxis {
    Horizontal,
    Vertical,
}

/// Thumb geometry for one window's scrollbar on `axis`, derived from
/// [`editor_scrollbar_layout`]'s `h_scrollbar_bounds`/`v_scrollbar_bounds`
/// track and the same [`quadraui::fit_thumb`] call
/// `quadraui::gtk::editor::draw_editor` paints the thumb with.
///
/// Returns `(track_x, track_y, track_w, track_h, thumb_pos, thumb_len,
/// scroll_range, px_per_unit)` — `thumb_pos`/`thumb_len` run along the
/// track's own axis (x/width for horizontal, y/height for vertical).
/// `None` when no scrollbar on this axis is painted (content fits).
#[allow(clippy::type_complexity)]
pub(crate) fn scrollbar_thumb_geometry(
    engine: &Engine,
    window_id: core::WindowId,
    rect: &core::WindowRect,
    char_width: f64,
    line_height: f64,
    v_scrollbar_w: Option<f32>,
    axis: ScrollbarAxis,
) -> Option<(f64, f64, f64, f64, f64, f64, f64, f64)> {
    let (editor, layout) = editor_scrollbar_layout(
        engine,
        window_id,
        rect,
        char_width,
        line_height,
        v_scrollbar_w,
    )?;
    let (track, scroll_pos, extent, visible, track_len) = match axis {
        ScrollbarAxis::Horizontal => {
            let track = layout.h_scrollbar_bounds?;
            (
                track,
                editor.scroll_left as f32,
                editor.max_col as f32,
                layout.visible_cols as f32,
                track.width,
            )
        }
        ScrollbarAxis::Vertical => {
            let track = layout.v_scrollbar_bounds?;
            (
                track,
                editor.scroll_top as f32,
                editor.total_lines as f32,
                layout.visible_lines as f32,
                track.height,
            )
        }
    };
    let (thumb_start, thumb_len) =
        quadraui::fit_thumb(scroll_pos, extent, visible, track_len, line_height as f32);
    let scroll_range = (extent as f64 - visible as f64).max(1.0);
    let px_per_unit = if track_len as f64 > thumb_len as f64 {
        (track_len - thumb_len) as f64 / scroll_range
    } else {
        0.0
    };
    let thumb_pos = match axis {
        ScrollbarAxis::Horizontal => track.x as f64 + thumb_start as f64,
        ScrollbarAxis::Vertical => track.y as f64 + thumb_start as f64,
    };

    Some((
        track.x as f64,
        track.y as f64,
        track.width as f64,
        track.height as f64,
        thumb_pos,
        thumb_len as f64,
        scroll_range,
        px_per_unit,
    ))
}

/// Hit-test a point against all windows' scrollbars on `axis`. Returns
/// `(window_id, scroll_at_click)` — `scroll_at_click` is `scroll_left` for
/// [`ScrollbarAxis::Horizontal`], `scroll_top` for
/// [`ScrollbarAxis::Vertical`] — when the point is on that scrollbar's
/// track (not only the thumb), so the caller can decide between
/// thumb-drag and track-click.
///
/// Delegates to [`quadraui::EditorLayout::hit_test`] rather than
/// re-deriving track bounds by hand, ensuring hit-test bounds follow
/// paint's conventions exactly.
// `v_scrollbar_w` threads the backend's scrollbar-width override through
// hit-testing so it agrees with paint. Keeping this as a single shared
// helper avoids the duplication of per-axis copies.
#[allow(clippy::too_many_arguments)]
pub(crate) fn scrollbar_hit_test(
    engine: &Engine,
    x: f64,
    y: f64,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
    v_scrollbar_w: Option<f32>,
    axis: ScrollbarAxis,
) -> Option<(core::WindowId, usize)> {
    let want = match axis {
        ScrollbarAxis::Horizontal => quadraui::EditorHit::HScrollbar,
        ScrollbarAxis::Vertical => quadraui::EditorHit::VScrollbar,
    };
    for (window_id, rect) in window_rects {
        let Some((_, layout)) = editor_scrollbar_layout(
            engine,
            *window_id,
            rect,
            char_width,
            line_height,
            v_scrollbar_w,
        ) else {
            continue;
        };
        if layout.hit_test(x as f32, y as f32) == want {
            let scroll = engine.windows.get(window_id).map(|w| match axis {
                ScrollbarAxis::Horizontal => w.view.scroll_left,
                ScrollbarAxis::Vertical => w.view.scroll_top,
            });
            return Some((*window_id, scroll.unwrap_or(0)));
        }
    }
    None
}

/// The bundled Nerd Font icon subset (Symbols Nerd Font 3.5.1), embedded in
/// the binary. Registered in-process through
/// `render::register_nerd_font_fallback` via `Backend::
/// register_font_from_memory`.
pub(crate) static ICON_FONT_BYTES: &[u8] = include_bytes!("../data/fonts/vimcode-icons.ttf");

/// The app icon (Dock/app-switcher on macOS, big+small titlebar/taskbar icon
/// on Win-GUI), rasterised at 512px from `render::APP_ICON_SVG` and embedded
/// in the binary.
///
/// A raster PNG rather than the SVG source: `ShellConfig::with_app_icon`
/// decodes through each backend's *tray*-icon pipeline
/// (`macos::tray::decode_ns_image` / `win::tray::decode_hicon`), not the
/// general `Backend::draw_image` path. `NSImage::initWithData` and WIC's
/// default `IWICImagingFactory` both decode PNG/JPEG/BMP reliably but have
/// no guaranteed SVG decoder, so a raster asset is the only format both
/// backends' tray decoders are guaranteed to accept.
///
/// Backend-neutral home, not `src/gtk/`: `crate::gtk::build_shell_config`
/// sets it too for consistency, but `ShellConfig::app_icon` is a no-op on
/// GTK/TUI per its own doc. Both `src/macos/` and `src/win/` need to reach
/// it, so it lives beside `ICON_FONT_BYTES` rather than under a single
/// backend's directory.
///
/// `cfg_attr`'d to avoid `dead_code` warnings in TUI-only builds.
#[cfg_attr(
    not(any(
        feature = "gui",
        feature = "win",
        all(feature = "macos", target_os = "macos")
    )),
    allow(dead_code)
)]
pub(crate) static APP_ICON_PNG: &[u8] =
    include_bytes!("../data/icons/io.github.jdonaghy.VimCode.png");
