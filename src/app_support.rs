//! Backend-neutral shell support functions used by `crate::app::App` (#862).
//!
//! Moved out of `src/gtk/mod.rs` (`gui`-gated) and `src/gtk/util.rs`: every
//! item here is pure computation over `Engine`/`quadraui` geometry and
//! string data — none of it names a `gtk4`/`pango`/`gio` type — so nesting it
//! inside `crate::gtk` only meant `crate::app` (and any future backend reusing
//! it) could not resolve it without the `gui` feature. `src/gtk/mod.rs` and
//! `src/gtk/util.rs` re-export everything below so the rest of `crate::gtk`
//! keeps resolving these names unchanged. The genuinely GTK-only siblings
//! (`util::install_icon_and_desktop`, `util::add_icon_theme_search_path`,
//! ...) stayed behind. `src/css.rs`/`src/gtk/css.rs` — this module's own
//! former sibling for `make_theme_css`/`STATIC_CSS`/`load_css` — is gone
//! entirely as of #1498, once JDonaghy/quadraui#1091 gave
//! `GtkPlatformServices` its own equivalent stylesheet for the native file
//! dialog's fallback widgets.
use crate::core;
use crate::core::Engine;
use crate::render;

use std::collections::HashMap;

pub(crate) fn is_ext_panel_id(id: &str) -> bool {
    id.starts_with("ext:")
}

/// Pango font family for UI panels (menu bar, sidebars, dropdown,
/// dialogs, hover popups). Size is appended at use via [`UI_FONT`]
/// from the configured `settings.ui_font_size` (#217).
///
/// Re-homed from the deleted `src/gtk/draw.rs` (#672) — draw.rs was
/// dead under `ShellApp`, but this const and its three siblings below
/// were still live (read by the raw-Pango chrome `render_content`
/// paints directly, e.g. the menu-bar font and the breadcrumb-heading
/// font), so they moved rather than being deleted with the rest of
/// the file.
///
/// #704 item 1: the old list (`"Segoe UI, Ubuntu, Droid Sans, Sans"`)
/// led with two names that never resolve on Linux — Segoe UI is
/// Windows-only, Droid Sans was retired from Android a decade ago —
/// and never listed Cantarell, the default UI font on GNOME (the most
/// common Linux desktop and the one this project targets). On a
/// GNOME box without the `Ubuntu` font package installed, fontconfig
/// fell through the whole list to the trailing generic `Sans`, which
/// resolves to DejaVu Sans: wider, with a taller x-height, than what
/// VS Code lands on at the same nominal point size (VS Code's own
/// `-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, Ubuntu,
/// "Droid Sans", sans-serif` stack has the identical problem, but
/// Electron's Chromium has extra fallback logic Pango/fontconfig does
/// not). Reordered so the two real Linux desktop UI fonts — Cantarell
/// (GNOME) and Ubuntu (Ubuntu/Unity) — are tried first, ahead of the
/// Windows/legacy names kept only for a hypothetical native-Windows
/// GTK build; `Sans` remains the final catch-all so a system with none
/// of the above still gets *a* font rather than a Pango parse failure.
/// This was blocked on quadraui#624 landing `Backend::set_ui_font`
/// reaching non-dialog chrome (tab bar, status bar, tree, menu bar) —
/// before that, changing this constant only affected `draw_dialog`/
/// `draw_rich_text_popup` and nothing else (see the issue's "Negative
/// example" reference to #700 item 1's no-op shape). `ui_font_size`
/// (`core::settings::default_ui_font_size`, 10pt ≈ 13.3px at 96dpi) is
/// left as-is: VS Code's 13px default is a ~2% difference, dwarfed by
/// the metric change from fixing the family, so nudging both at once
/// would make it impossible to tell which change did what.
///
/// #1069 added a `cfg!` branch here, keyed on `target_os == "macos"`, to
/// try real CoreText UI font names ahead of the Linux/Windows list — a
/// *backend* fact (which family name resolves on which OS) leaking into
/// otherwise shared code, and one that bought nothing:
/// `MacBackend::parse_ui_font_desc`
/// didn't split a comma list at the time, so the whole string always
/// degraded to the CoreText system UI font regardless of which names led
/// it (documented at length in the pre-#1129 revision of this comment).
///
/// #1129: removed once quadraui#1023 landed comma-list parsing plus
/// [`quadraui::GenericFamily`] resolution for `Backend::set_ui_font` on
/// every pixel backend. GTK/fontconfig already resolves this exact list
/// natively (`Cantarell`/`Ubuntu` first, matching real Linux desktop UI
/// fonts, then the Windows/legacy names, `Sans` as the final catch-all —
/// see #704's history above). On macOS, `MacBackend::set_ui_font` now
/// tries each comma-separated candidate in order via `make_font_exact`
/// and degrades to the CoreText system UI font only if none resolve —
/// still inert for *this* list (none of these are real CoreText family
/// names) but no longer needs a `cfg!` branch to say so: the fix is one
/// shared string plus quadraui resolving it per-backend, not two lists.
const UI_FONT_FAMILY: &str = "Cantarell, Ubuntu, Segoe UI, Droid Sans, Sans";

thread_local! {
    /// Per-thread UI font size (points). Synced from
    /// `settings.ui_font_size` at the start of each frame by
    /// [`sync_ui_font_size`]. Read everywhere a Pango font description
    /// is built — avoids threading `&Settings` through every draw
    /// function for what's effectively one shared knob (#217).
    ///
    /// **Thread-local, not a process-global `AtomicU8` (#766).** Production is
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

/// Update this thread's UI font size from `settings`. Called
/// once per frame at the top of [`App::render_content`] (#672 —
/// `draw.rs::draw_editor`'s only live caller before the delete).
pub(crate) fn sync_ui_font_size(settings: &core::settings::Settings) {
    UI_FONT_SIZE.with(|s| s.set(settings.ui_font_size.max(6)));
}

/// Pango font description string for UI chrome at the currently
/// configured size. Call sites do `FontDescription::from_string(&UI_FONT())`.
#[allow(non_snake_case)]
pub(crate) fn UI_FONT() -> String {
    format!("{} {}", UI_FONT_FAMILY, UI_FONT_SIZE.with(|s| s.get()))
}

/// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`).
/// See `ShellApp::cached_tab_slots_abs` for the full doc comment. (#515)
pub(crate) type TabSlotsAbsMap = HashMap<usize, Vec<(f32, f32)>>;

/// What the git sidebar's commit-message `TextInput` border costs vertically in
/// GTK's native unit: 1px on top + 1px on bottom. TUI's whole-cell equivalent
/// is `render::sc_commit_input_box_height`'s `+ 2` rows. Fed to
/// `render::sc_sidebar_bands` by both the painter and the click router so the
/// two agree (#544).
pub(crate) const SC_COMMIT_BORDER_PX: f32 = 2.0;

/// Cached per-window status segment hit zones: window_id -> Vec<(start_x, end_x, action)>.
/// Populated in `render_content`'s per-window/separated status bar paint
/// (#672 — re-homed off the dead `draw.rs::draw_window_status_bar`),
/// consumed by click hit-testing.
pub(crate) type StatusSegmentMap =
    HashMap<usize, Vec<(f64, f64, crate::core::engine::StatusAction)>>;

/// Build the layout-only [`quadraui::Editor`] needed to call
/// [`quadraui::Editor::layout`] for scrollbar geometry — the same
/// primitive GTK's real paint path builds from a full `RenderedWindow`
/// (`render::to_q_editor` + `.layout()`, wired through
/// `quadraui::gtk::editor::draw_editor_with_options` since quadraui#968
/// taught that rasteriser to paint both scrollbars itself) — so hit-testing
/// reads the identical formula paint does and can never independently
/// drift from it the way the pre-#968 hand-rolled h-scrollbar geometry
/// helper this replaced did (#1128).
///
/// `Editor::layout`/`layout_with_options` only ever read
/// `total_lines`/`max_col`/`gutter_char_width` off the struct — never
/// `.lines`, any paint-only cosmetic field, or even `.rect` itself (the
/// `viewport` argument passed to `.layout()` is authoritative; see
/// [`render::tui_editor_text_layout`]'s doc for the same fact stated on the
/// TUI side). Building the full `RenderedWindow` paint uses would mean
/// re-rendering the buffer's visible text on every mouse motion just to
/// throw it away — this builds the cheap subset instead. Every other field
/// below is a throwaway needed only to satisfy `Editor`'s exhaustive
/// struct-literal contract (`quadraui/tests/downstream_struct_literals.rs`).
fn scrollbar_probe_editor(engine: &Engine, window_id: core::WindowId) -> Option<quadraui::Editor> {
    let window = engine.windows.get(&window_id)?;
    let buffer_state = engine.buffer_manager.get(window.buffer_id)?;

    // Mirrors `render::build_rendered_window`'s `has_git`/`has_bp`/
    // `line_number_mode` → `gutter_char_width` computation, so a wide
    // gutter (line numbers, git column, breakpoint column) shifts this
    // probe's scrollbar geometry by exactly the amount it shifts paint's —
    // the pre-#1128 h-scrollbar geometry helper this replaced ignored the
    // gutter entirely.
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

    Some(quadraui::Editor {
        id: quadraui::WidgetId::new("scrollbar_probe"),
        rect: quadraui::Rect::new(0.0, 0.0, 0.0, 0.0),
        lines: Vec::new(),
        cursor: None,
        extra_cursors: Vec::new(),
        selection: None,
        extra_selections: Vec::new(),
        yank_highlight: None,
        scroll_top: window.view.scroll_top,
        scroll_left: window.view.scroll_left,
        total_lines,
        max_col: buffer_state.max_col,
        gutter_char_width,
        is_active: false,
        show_active_bg: false,
        has_git_diff: false,
        has_breakpoints: false,
        diagnostic_gutter: HashMap::new(),
        code_action_lines: std::collections::HashSet::new(),
        bracket_match_positions: Vec::new(),
        active_indent_col: None,
        tabstop: engine.settings.tabstop.max(1) as usize,
        cursorline: false,
        lightbulb_glyph: '\0',
    })
}

/// This window's [`quadraui::Editor`] + [`quadraui::EditorLayout`], laid
/// out against `rect` at `char_width`/`line_height` — the same
/// [`quadraui::Editor::layout`] call paint makes, so
/// `.h_scrollbar_bounds`/`.v_scrollbar_bounds` are never independently
/// re-derived (#1128).
///
/// `rect` is handed to `.layout()` unmodified, exactly as
/// `quadraui::gtk::editor::draw_editor_with_options` hands it `editor.rect`
/// unmodified — including for a window with its own per-window status
/// line, which current paint does **not** shrink `rect` for before laying
/// out scrollbars (see `quadraui::gtk::editor`'s module doc, "Scrollbars"
/// section). That means the painted scrollbar can currently run under
/// where the status line paints afterward; that overlap is real, but
/// pre-existing and out of this issue's scope — #723/#1094 are the
/// scrollbar-*placement* follow-ups this issue's ordering note defers it
/// to. Reintroducing a status-row offset here — as the pre-#1128 code did —
/// would just make hit-testing disagree with paint in the other direction.
pub(crate) fn editor_scrollbar_layout(
    engine: &Engine,
    window_id: core::WindowId,
    rect: &core::WindowRect,
    char_width: f64,
    line_height: f64,
) -> Option<(quadraui::Editor, quadraui::EditorLayout)> {
    let editor = scrollbar_probe_editor(engine, window_id)?;
    let viewport = quadraui::Rect::new(
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
    );
    let layout = editor.layout(viewport, char_width as f32, line_height as f32);
    Some((editor, layout))
}

/// Which editor scrollbar a geometry/hit-test call is about (#1493).
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
/// group-divider's own hit zone (#987). Reading both through
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
/// Replaces the pre-#1128 h-scrollbar geometry helper, whose independently
/// guessed `8.0`px v-scrollbar reserve (the real reserve is one
/// `char_width`-wide cell, quadraui#968) and gutter-blind track start meant
/// hover/drag could resolve against a rect paint never actually drew, and
/// the pre-#1493 h/v duplication of this same function.
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
    axis: ScrollbarAxis,
) -> Option<(f64, f64, f64, f64, f64, f64, f64, f64)> {
    let (editor, layout) =
        editor_scrollbar_layout(engine, window_id, rect, char_width, line_height)?;
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
/// re-deriving inclusive/exclusive track bounds by hand — the pre-#1493
/// horizontal copy of this function used an inclusive upper bound (`<=`)
/// on both `x` and `y` where the vertical copy had already been fixed to
/// half-open (`<`) so a click on a window's right edge lands on the
/// group-divider's own hit zone instead of being swallowed by the
/// scrollbar (#987's `drag_group_divider_resizes` regression). Reading
/// both axes through the same `hit_test` call means they can never
/// re-diverge on that convention again.
pub(crate) fn scrollbar_hit_test(
    engine: &Engine,
    x: f64,
    y: f64,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
    axis: ScrollbarAxis,
) -> Option<(core::WindowId, usize)> {
    let want = match axis {
        ScrollbarAxis::Horizontal => quadraui::EditorHit::HScrollbar,
        ScrollbarAxis::Vertical => quadraui::EditorHit::VScrollbar,
    };
    for (window_id, rect) in window_rects {
        let Some((_, layout)) =
            editor_scrollbar_layout(engine, *window_id, rect, char_width, line_height)
        else {
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
/// `render::register_nerd_font_fallback` (#937's `Backend::
/// register_font_from_memory` route). #1130 deleted this module's former
/// fontconfig filesystem-install route (`install_bundled_icon_font` + its
/// font-cache-refresh shell-out) now that quadraui#1013 gives GTK a real
/// `register_font_from_memory` override — see that function's doc for how
/// the in-memory registration now covers every backend, GTK included.
pub(crate) static ICON_FONT_BYTES: &[u8] = include_bytes!("../data/fonts/vimcode-icons.ttf");

/// The app icon (Dock/app-switcher on macOS, big+small titlebar/taskbar icon
/// on Win-GUI), rasterised at 512px from `render::APP_ICON_SVG` and embedded
/// in the binary — quadraui#1142 / vimcode#1531.
///
/// A raster PNG rather than the SVG source: `ShellConfig::with_app_icon`
/// decodes through each backend's *tray*-icon pipeline
/// (`macos::tray::decode_ns_image` / `win::tray::decode_hicon`), not the
/// general `Backend::draw_image` path `render::APP_ICON_SVG`'s other call
/// site uses — `NSImage::initWithData` and WIC's default
/// `IWICImagingFactory` both decode PNG/JPEG/BMP reliably but have no
/// guaranteed SVG decoder, so a raster asset is the only format both
/// backends' tray decoders are guaranteed to accept. quadraui's own
/// `full_chrome_demo` example makes the same choice for its `with_app_icon`
/// call, for the same reason.
///
/// Backend-neutral home, not `src/gtk/`: `crate::gtk::build_shell_config`
/// does set it too (for consistency with `crate::macos`/`crate::win`), but
/// `ShellConfig::app_icon` is a no-op on GTK/TUI per its own doc — this
/// constant's *only* backend with real behaviour to prove is macOS/Win-GUI.
/// The byte constant itself carries no GTK dependency and both `src/macos/`
/// and `src/win/` need to reach it, so it lives beside `ICON_FONT_BYTES`
/// rather than under a single backend's directory.
///
/// `cfg_attr`'d the same way `App::shell_config` is (see that method's own
/// `#[cfg_attr]`): its only readers are `crate::gtk`/`crate::macos`/
/// `crate::win`'s `build_shell_config` functions, so a TUI-only
/// `--no-default-features` build — which compiles none of those three —
/// would otherwise report this `dead_code`.
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
