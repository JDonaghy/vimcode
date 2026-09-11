//! Backend-neutral shell support functions used by `crate::app::App` (#862).
//!
//! Moved out of `src/gtk/mod.rs` (`gui`-gated) and `src/gtk/{css,util}.rs`:
//! every item here is pure computation over `Engine`/`quadraui` geometry and
//! string data — none of it names a `gtk4`/`pango`/`gio` type — so nesting it
//! inside `crate::gtk` only meant `crate::app` (and any future backend reusing
//! it) could not resolve it without the `gui` feature. `src/gtk/mod.rs`,
//! `src/gtk/css.rs` and `src/gtk/util.rs` re-export everything below so the
//! rest of `crate::gtk` keeps resolving these names unchanged. The genuinely
//! GTK-only siblings (`css::load_css`, `util::app_icon_image`'s PNG
//! rasterisation, `util::install_icon_and_desktop`, ...) stayed behind.
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

/// Absolute per-group close-glyph hit rects captured during `render_content`.
/// Keyed by `group_id.0` → `(bar_y_top, bar_y_bottom, per-tab Some((x0, x1)))`.
/// All coordinates are in **absolute surface pixels** (same space as the raw
/// mouse position), so hover hit-testing needs no geometry re-derivation. The
/// x-ranges are the *tight* close-glyph zone (see `crate::click`'s
/// `tighten_close_bounds`), matching the × highlight the rasteriser draws —
/// so a hover shows the exact box that a click would close. (#515)
pub(crate) type TabCloseAbsMap = HashMap<usize, (f64, f64, Vec<Option<(f64, f64)>>)>;

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

/// Calculate gutter width in pixels based on line number mode and buffer size
#[allow(dead_code)]
fn calculate_gutter_width(
    mode: core::settings::LineNumberMode,
    total_lines: usize,
    char_width: f64,
) -> f64 {
    use core::settings::LineNumberMode;
    match mode {
        LineNumberMode::None => 0.0,
        LineNumberMode::Absolute => {
            // Width = number of digits + 2 chars padding (1 on each side)
            let digits = total_lines.to_string().len().max(1);
            (digits + 2) as f64 * char_width
        }
        LineNumberMode::Relative | LineNumberMode::Hybrid => {
            // Relative numbers can be large for long files, use at least 3 digits + 2 padding
            let max_relative = total_lines.saturating_sub(1);
            let digits = max_relative.to_string().len().max(3);
            (digits + 2) as f64 * char_width
        }
    }
}

/// Compute the editor area bottom Y coordinate.  Must match draw_editor (draw.rs)
/// so that group rects and divider positions are consistent across draw and click.
/// Compute the target `terminal_panel_rows` when maximizing the GTK panel.
///
/// The rendered terminal panel takes `(terminal_panel_rows + 2) * lh` pixels
/// (2 chrome rows = bottom-panel tab bar + terminal toolbar). Editor tab bar
/// stays visible (1 row reserved); breadcrumbs are suppressed elsewhere so
/// we don't reserve a row for them here. Called every frame from `draw_frame`
fn gtk_editor_bottom(engine: &Engine, _da_width: f64, da_height: f64, line_height: f64) -> f64 {
    render::compute_editor_layout(engine, da_height, line_height, false).editor_bottom
}

/// Compute editor window rects with the same formula `render_content` uses
/// (previously shared with the now-deleted `sync_scrollbar`, #731), so event
/// handlers can do hit-testing without duplicating the layout logic.
pub(crate) fn compute_editor_window_rects(
    engine: &Engine,
    da_width: f64,
    da_height: f64,
    line_height: f64,
) -> Vec<(core::WindowId, core::WindowRect)> {
    let tab_bar_height = render::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let editor_bounds = core::WindowRect::new(
        0.0,
        0.0,
        da_width,
        gtk_editor_bottom(engine, da_width, da_height, line_height),
    );
    let (rects, _dividers) = engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
    rects
}

/// Compute the thumb geometry for one window's h scrollbar.
/// Returns `(track_x, track_y, track_w, sb_height, thumb_x, thumb_w, scroll_range, px_per_col)`.
/// Returns `None` when no scrollbar is needed (content fits).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn h_scrollbar_geometry(
    engine: &Engine,
    window_id: core::WindowId,
    rect: &core::WindowRect,
    char_width: f64,
    line_height: f64,
) -> Option<(f64, f64, f64, f64, f64, f64, f64, f64)> {
    let window = engine.windows.get(&window_id)?;
    let buffer_state = engine.buffer_manager.get(window.buffer_id)?;

    // max_col is pre-computed and cached in BufferState on every edit — O(1) vs O(N_lines).
    let max_line_length = buffer_state.max_col as f64;

    let v_scrollbar_px = 8.0_f64;
    let track_w = (rect.width - v_scrollbar_px).max(1.0);
    let visible_cols = (track_w / char_width).floor().max(1.0);

    if max_line_length <= visible_cols {
        return None;
    }

    let sb_height = (line_height * 0.35).round().max(4.0);
    let track_x = rect.x;
    // Per-window status line lives at `rect.y + rect.height -
    // line_height` and paints after the scrollbars, so anchor the
    // h-scrollbar above it when the status line is on. Otherwise the
    // status bar overdraws the entire scrollbar (it's `line_height`
    // tall vs the scrollbar's ~5px). `render::window_status_row_reserved`
    // is the single source of truth for whether that row is actually
    // painted (#728) — this used to check `window_status_line &&
    // !terminal_maximized` directly, which (unlike the shared helper)
    // never accounted for `status_line_above_terminal`/bottom-panel state
    // pulling the status line out into a separated bar instead, and so
    // could disagree with `build_screen_layout` about whether this row is
    // free.
    let status_offset = if render::window_status_row_reserved(engine) {
        line_height
    } else {
        0.0
    };
    let track_y = rect.y + rect.height - sb_height - status_offset;
    let scroll_range = (max_line_length - visible_cols).max(1.0);
    let thumb_frac = visible_cols / max_line_length;
    let thumb_w = (thumb_frac * track_w).max(20.0).min(track_w);
    let px_per_col = (track_w - thumb_w) / scroll_range;
    let scroll_left = window.view.scroll_left as f64;
    let thumb_x = track_x + (scroll_left / scroll_range) * (track_w - thumb_w);

    Some((
        track_x,
        track_y,
        track_w,
        sb_height,
        thumb_x,
        thumb_w,
        scroll_range,
        px_per_col,
    ))
}

/// Hit-test a point against all h scrollbars. Returns `(window_id,
/// scroll_left_at_click)` when the point is on any h scrollbar track (not only
/// the thumb), so the caller can decide between thumb-drag and track-click.
pub(crate) fn h_scrollbar_hit_test(
    engine: &Engine,
    x: f64,
    y: f64,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
) -> Option<(core::WindowId, usize)> {
    for (window_id, rect) in window_rects {
        if let Some((track_x, track_y, track_w, sb_height, _, _, _, _)) =
            h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        {
            if x >= track_x && x <= track_x + track_w && y >= track_y && y <= track_y + sb_height {
                let scroll_left = engine
                    .windows
                    .get(window_id)
                    .map(|w| w.view.scroll_left)
                    .unwrap_or(0);
                return Some((*window_id, scroll_left));
            }
        }
    }
    None
}

/// Open a URL in the default browser (only https/http).
pub(crate) fn open_url(url: &str) {
    crate::core::engine::open_url_in_browser(url);
}

/// Install the bundled Nerd Font icon subset to `~/.local/share/fonts/` so
/// GTK/Pango can resolve the Nerd Font glyphs without a user-installed Nerd Font.
/// The font file is embedded in the binary via `include_bytes!` and only written
/// to disk if it's missing or has the wrong size.
///
/// Only `App::new` (`gui`-gated) calls this today, so a `--no-default-features`
/// build sees it as dead code — allowed rather than `#[cfg]`-gating the
/// function itself, since it names no toolkit type and a future non-GTK
/// backend that also ships a bundled icon font could reuse it as-is.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub(crate) fn install_bundled_icon_font() {
    static FONT_BYTES: &[u8] = include_bytes!("../data/fonts/vimcode-icons.ttf");

    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return;
    };
    let fonts_dir = home.join(".local/share/fonts");
    let _ = std::fs::create_dir_all(&fonts_dir);
    let dest = fonts_dir.join("vimcode-icons.ttf");

    // Skip write if the file already exists with the correct size.
    if dest.exists() {
        if let Ok(meta) = std::fs::metadata(&dest) {
            if meta.len() == FONT_BYTES.len() as u64 {
                return;
            }
        }
    }

    if std::fs::write(&dest, FONT_BYTES).is_ok() {
        // Trigger fontconfig cache rebuild so the font is available immediately.
        let _ = std::process::Command::new("fc-cache")
            .arg("-f")
            .arg(&fonts_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}
