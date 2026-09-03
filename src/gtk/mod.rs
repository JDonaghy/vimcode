// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional
// TODO: Migrate to ListView/ColumnView in a future phase
#![allow(deprecated)]

use gtk4::pango;
use pangocairo::functions as pangocairo;
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use crate::core;
use crate::render;

use core::engine::EngineAction;
use core::settings::LineNumberMode;
use core::Engine;
use render::Theme;

use std::collections::HashMap;

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

pub(crate) fn is_ext_panel_id(id: &str) -> bool {
    id.starts_with("ext:")
}

pub(crate) type TabSlotMap = HashMap<usize, Vec<(f64, f64)>>;

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
/// x-ranges are the *tight* close-glyph zone (see [`CLOSE_*` metrics] and
/// [`tighten_close_bounds`]), matching the × highlight the rasteriser draws —
/// so a hover shows the exact box that a click would close. (#515)
pub(crate) type TabCloseAbsMap = HashMap<usize, (f64, f64, Vec<Option<(f64, f64)>>)>;

/// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`).
/// See `ShellApp::cached_tab_slots_abs` for the full doc comment. (#515)
pub(crate) type TabSlotsAbsMap = HashMap<usize, Vec<(f32, f32)>>;

// ── GTK editor-group tab-bar close-glyph metrics ─────────────────────────────
// The quadraui GTK rasteriser lays each non-compact tab out as
// `tab_pad | label | tab_inner_gap | × | tab_pad | tab_outer_gap` and reports a
// *padded* close-button hit zone spanning `[label_end, tab_right_edge]`. That
// zone is far wider than the drawn × glyph, so a click well before the glyph
// used to close the tab with no warning (#515). We trim the padded zone back to
// the glyph the rasteriser actually painted — plus the same 2px hover halo it
// draws behind the ×, so the clickable box equals the highlighted box.
//
// These mirror the non-compact constants in quadraui's `gtk::backend`
// (`tab_pad = 14`, `tab_inner_gap = 10`, `tab_outer_gap = 1`) and the 2px hover
// pad in `gtk::tab_bar`. Editor-group bars are always built with
// `compact: false` (see `render::build_tab_bar_primitive`). This duplication is
// the interim until quadraui exposes the tight glyph rect directly
// (quadraui#395 tracks the API gap); `tighten_close_bounds` is the single place
// it lives.
const CLOSE_TAB_INNER_GAP: f64 = 10.0;
const CLOSE_TAB_PAD: f64 = 14.0;
const CLOSE_TAB_OUTER_GAP: f64 = 1.0;
const CLOSE_HOVER_PAD: f64 = 2.0;

/// What the git sidebar's commit-message `TextInput` border costs vertically in
/// GTK's native unit: 1px on top + 1px on bottom. TUI's whole-cell equivalent
/// is `render::sc_commit_input_box_height`'s `+ 2` rows. Fed to
/// `render::sc_sidebar_bands` by both the painter and the click router so the
/// two agree (#544).
pub(crate) const SC_COMMIT_BORDER_PX: f32 = 2.0;

/// Trim a *padded* close-button hit zone `(start, end)` — as reported by
/// `quadraui::Backend::tab_bar_layout` — down to the tight × glyph box the
/// rasteriser actually draws (including its 2px hover halo). Leading
/// `tab_inner_gap` and trailing `tab_pad + tab_outer_gap` are dead padding that
/// should select the tab, not close it. Returns `None` if the padded zone is
/// degenerate (too small to contain a glyph). (#515)
fn tighten_close_bounds(start: f64, end: f64) -> Option<(f64, f64)> {
    let tight_start = start + CLOSE_TAB_INNER_GAP - CLOSE_HOVER_PAD;
    let tight_end = end - CLOSE_TAB_PAD - CLOSE_TAB_OUTER_GAP + CLOSE_HOVER_PAD;
    if tight_end > tight_start {
        Some((tight_start, tight_end))
    } else {
        None
    }
}

/// Per-group pixel-accurate tab-bar hit geometry recovered from
/// [`quadraui::Backend::tab_bar_layout`] during the ShellApp `render_content`
/// pass. All x-ranges are **relative to the group's tab-bar left edge** — the
/// same space as `render::screen_zone_hit_test`'s `local_x`.
///
/// This replaces the char-cell `hit_regions` approximation for GTK tab clicks.
/// GTK draws tabs with proportional-font Pango widths + fixed pixel padding
/// (`tab_pad`, `inner_gap`, close-glyph width), so a `name.chars() * char_width`
/// estimate under-measures every tab — shifting the tab/close boundaries and
/// making mid-tab clicks land on the close button and right-edge clicks land on
/// the next tab (#515 regression). The rasteriser reports the exact drawn
/// geometry, so we hit-test against that. (`hit_regions` stays authoritative for
/// the monospace TUI backend, whose char-cell layout matches its draw.)
#[derive(Default, Clone)]
pub(super) struct TabBarPixelHits {
    /// `(start_x, end_x)` per tab index; `(0.0, 0.0)` for scrolled-off tabs.
    pub slots: Vec<(f64, f64)>,
    /// `Some((start_x, end_x))` close-button zone per tab, or `None`.
    pub close: Vec<Option<(f64, f64)>>,
    /// Right-segment hit zones (split / diff / action buttons) as
    /// `(start_x, end_x, target)`, disjoint from the tab slots.
    pub segments: Vec<(f64, f64, crate::core::engine::TabBarClickTarget)>,
}

/// Key = `group_id.0` (single-group mode keys under the active group's id, which
/// is what `screen_zone_hit_test` reports for it).
pub(crate) type TabPixelHitMap = HashMap<usize, TabBarPixelHits>;

/// Convert a rasteriser [`quadraui::TabBarHits`] (absolute pixel x, from
/// `Backend::tab_bar_layout`) plus its source [`quadraui::TabBar`] into a
/// [`TabBarPixelHits`] with every x-range shifted to be **relative to
/// `bar_left_x`** (the group tab bar's left edge). Right-segment ids are mapped
/// to their `TabBarClickTarget` using the same `"tab:*"` ids that
/// `build_tab_bar_primitive` emits (mirrors `draw::draw_tab_bar`).
pub(crate) fn tab_hits_to_pixel_hits(
    hits: &quadraui::TabBarHits,
    bar: &quadraui::TabBar,
    bar_left_x: f64,
) -> TabBarPixelHits {
    use crate::core::engine::TabBarClickTarget as T;
    let rel = |a: f64, b: f64| (a - bar_left_x, b - bar_left_x);
    let slots = hits
        .slot_positions
        .iter()
        .map(|&(a, b)| {
            if (a, b) == (0.0, 0.0) {
                (0.0, 0.0) // scrolled-off sentinel — leave as zero-width
            } else {
                rel(a, b)
            }
        })
        .collect();
    // Trim the padded close zone the rasteriser reports down to the tight ×
    // glyph box (relative to the bar's left edge), so clicks/hover only fire on
    // the drawn glyph — not the ~25px of surrounding tab padding. (#515)
    let close = hits
        .close_bounds
        .iter()
        .map(|c| c.and_then(|(a, b)| tighten_close_bounds(a, b).map(|(ta, tb)| rel(ta, tb))))
        .collect();
    let mut segments = Vec::new();
    for (i, seg) in bar.right_segments.iter().enumerate() {
        let Some((a, b)) = hits.right_segment_bounds.get(i).copied() else {
            continue;
        };
        let Some(ref id) = seg.id else { continue };
        let target = match id.as_str() {
            "tab:split_right" => Some(T::SplitRight),
            "tab:split_down" => Some(T::SplitDown),
            "tab:diff_prev" => Some(T::DiffPrev),
            "tab:diff_next" => Some(T::DiffNext),
            "tab:diff_toggle" => Some(T::DiffToggle),
            "tab:action_menu" => Some(T::ActionMenu),
            _ => None,
        };
        if let Some(t) = target {
            let (s, e) = rel(a, b);
            segments.push((s, e, t));
        }
    }
    TabBarPixelHits {
        slots,
        close,
        segments,
    }
}

/// Build the absolute close-glyph hit record for one tab bar from its
/// bar-relative (already-tightened) close bounds. `bar_left_x` is the bar's
/// absolute left edge; `y_top`/`y_bot` bracket the tab row. Consumed by
/// `tab_close_hit_test` for hover. (#515)
pub(crate) fn abs_close_record(
    ph_close: &[Option<(f64, f64)>],
    bar_left_x: f64,
    y_top: f64,
    y_bot: f64,
) -> (f64, f64, Vec<Option<(f64, f64)>>) {
    let xs = ph_close
        .iter()
        .map(|c| c.map(|(a, b)| (a + bar_left_x, b + bar_left_x)))
        .collect();
    (y_top, y_bot, xs)
}

/// Collect the visible tab slots (absolute x-ranges) from a `TabBarHits`,
/// dropping the `(0.0, 0.0)` sentinels for scrolled-off / non-fitting tabs.
/// The result is a contiguous run starting at the tab bar's `scroll_offset`,
/// which the drop-zone reorder logic offsets back to absolute tab indices.
/// (#515)
pub(crate) fn abs_visible_slots(hits: &quadraui::TabBarHits) -> Vec<(f32, f32)> {
    hits.slot_positions
        .iter()
        .filter(|&&(a, b)| (a, b) != (0.0, 0.0))
        .map(|&(a, b)| (a as f32, b as f32))
        .collect()
}

/// Cached diff toolbar button positions per group: group_id -> (prev_start, prev_end, next_start, next_end, fold_start, fold_end).
/// Populated during draw_tab_bar, used for click hit-testing.
pub(crate) type DiffBtnMap = HashMap<usize, (f64, f64, f64, f64, f64, f64)>;

/// Cached split button pixel widths per group: group_id -> (both_btns_px, btn_right_px).
/// Only populated when split buttons are visible (active group in multi-group, or single-group mode).
pub(crate) type SplitBtnMap = HashMap<usize, (f64, f64)>;

/// Cached action menu button pixel range per group: group_id -> (start_x, end_x).
pub(crate) type ActionBtnMap = HashMap<usize, (f64, f64)>;

/// Cached per-window status segment hit zones: window_id -> Vec<(start_x, end_x, action)>.
/// Populated in `render_content`'s per-window/separated status bar paint
/// (#672 — re-homed off the dead `draw.rs::draw_window_status_bar`),
/// consumed by click hit-testing.
pub(crate) type StatusSegmentMap =
    HashMap<usize, Vec<(f64, f64, crate::core::engine::StatusAction)>>;

// view_row_to_buf_line and view_row_to_buf_pos_wrap are now shared functions
// in render.rs — use render::view_row_to_buf_line / render::view_row_to_buf_pos_wrap.

/// Calculate gutter width in pixels based on line number mode and buffer size
#[allow(dead_code)]
fn calculate_gutter_width(mode: LineNumberMode, total_lines: usize, char_width: f64) -> f64 {
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

// #731: `tab_close_hit_test`, `tab_tooltip_hit_test`, and `shorten_path`
// used to live here. Their only caller was the ~135-line hover-polling
// block removed from `App::tick` by this issue (see that removal's doc
// comment) — dead since #540, so deleting them changes nothing on screen.
// `App::cached_tab_close_abs` (mentioned in `tab_close_hit_test`'s old doc)
// is still populated by `render_content` and read elsewhere (click
// handling), so that mechanism itself is unaffected.

/// Entry point for GTK mode.
///
/// `pub` rather than `pub(crate)` since #657: the caller is `src/main.rs`,
/// which is now a separate crate from this module's.
pub fn run(file_path: Option<PathBuf>) {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        std::env::set_var("DISPLAY", ":0");
    }

    // Install panic hook that flushes swap files + writes crash log.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Emergency: flush swap files for all dirty buffers.
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                eprintln!("VimCode crashed. Details written to {}", path.display());
                eprintln!("Unsaved buffers written to swap files for recovery.");
                eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
            }
            prev_hook(info);
        }));
    }

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
    let vimcode_app = App::new(file_path);
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
fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    // Mirror the engine's AppShell panel list into the ShellConfig so the
    // quadraui runner renders the activity bar icons.  The engine stores all
    // panels (including "bottom:settings") in a single `panels()` slice;
    // ShellConfig wants top-pinned panels in its first arg and bottom-pinned
    // items via `with_bottom_items()`, so split on the "bottom:" ID prefix.
    // Fill in activity-bar icons before building ShellConfig.  The engine's
    // AppShell initialises all PanelDefinition.icon fields to "" because the
    // engine itself is backend-agnostic; the GTK runner is responsible for
    // mapping each panel ID to the correct Nerd-Font / fallback glyph.
    let panels_with_icons: Vec<_> = app
        .engine
        .borrow()
        .app_shell
        .panels()
        .iter()
        .cloned()
        .map(|mut p| {
            p.icon = match p.id.as_str() {
                "panel:explorer" => crate::icons::EXPLORER.s().to_string(),
                "panel:search" => crate::icons::SEARCH_COD.s().to_string(),
                "panel:debug" => crate::icons::DEBUG.s().to_string(),
                "panel:git" => crate::icons::GIT_BRANCH.s().to_string(),
                "panel:extensions" => crate::icons::EXTENSIONS.s().to_string(),
                "panel:ai" => crate::icons::AI_CHAT.s().to_string(),
                "bottom:settings" => crate::icons::SETTINGS.s().to_string(),
                _ => p.icon,
            };
            p
        })
        .collect();
    let (mut top_panels, bottom_items): (Vec<_>, Vec<_>) = panels_with_icons
        .into_iter()
        .partition(|p| !p.id.as_str().starts_with("bottom:"));
    // #557: plugin-provided panels (e.g. the Git Insights extension) live in
    // `engine.ext_panels`, not in the engine's `AppShell` — nothing registers
    // them there — so they have to be appended explicitly or the runner's
    // activity bar renders no icon for them at all. `ext_activity_panels`
    // already carries each panel's resolved icon, so the id→glyph match above
    // deliberately doesn't need an arm for them.
    top_panels.extend(app.engine.borrow().ext_activity_panels());
    // (#552) Reserve a full-width title-bar band across the top of the shell
    // (above activity bar + sidebar + main content, not just main content) —
    // GTK draws its own client-side menu bar + inline window controls into
    // it since `run_with_shell` creates an undecorated-chrome-free window.
    // Always on: GTK's menu bar acts as its titlebar (matches pre-#540
    // behaviour), unlike TUI where it's optional.
    //
    // #710 item 2: `height_lh` is a line-height *multiple* of the editor's
    // `current_line_height` (`AppShell::compute_layout`, quadraui
    // `compose/app_shell.rs`) — there is no fixed-px reservation API yet, so
    // this band is unavoidably coupled to editor font metrics until one
    // exists (see the doc comment on `quadraui::ShellConfig::with_title_bar`
    // and file a quadraui px-based-reservation issue if this residual needs
    // closing — #710's PR should note whether that filing happened). 1.0
    // measured ~18px in the headless GTK test harness — one editor text
    // line, visibly squat next to VS Code's 35px title bar / ~26px command
    // centre pill (`quadraui::gtk::command_center::draw_command_center`
    // paints the pill at `band_height - 4`). 1.7 measures ~31px band / 27px
    // pill here — much closer to parity without needing the fixed-px API.
    let mut cfg = quadraui::ShellConfig::new("VimCode", top_panels)
        .with_bottom_items(bottom_items)
        .with_title_bar(1.7)
        // #719: quadraui#656 builders — route the WM app id / icon name
        // through the single `APP_ID` constant #716 introduced, rather than
        // a fresh string literal, so there's exactly one identity string.
        .with_app_id(util::APP_ID)
        .with_icon_name(util::APP_ID)
        // #719: quadraui#657 fixed-px form. The activity bar's row height is
        // already the fixed `ACTIVITY_ROW_PX = 48.0` (VS Code parity), so
        // sizing the bar's *width* from the editor font (the old default)
        // made it oblong; pin the width to the same 48px instead of using
        // the font-relative unit form.
        .with_activity_bar_width_px(48.0);
    // #759: the shared Alt rung's sidebar clamps, so Alt+Left/Right resolve
    // identically on both backends. TUI has set exactly this pair since #634
    // (with a comment naming the failure — "Alt+Right would silently stop at
    // 50"); GTK kept quadraui's generic 8/50 because it had no Alt rung to
    // stop short in the first place. `default_sidebar_width` is deliberately
    // left at quadraui's 20: GTK's unit is a line-height, not a column, so
    // TUI's 30-*column* default is not the same quantity.
    cfg.min_sidebar_width = render::ALT_SIDEBAR_WIDTH_MIN as f32;
    cfg.max_sidebar_width = render::ALT_SIDEBAR_WIDTH_MAX as f32;
    cfg
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
            let c = render::to_quadraui_color(theme.tab_bar_bg);
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
