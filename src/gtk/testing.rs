//! In-crate headless GTK black-box test harness (#646).
//!
//! vimcode's GTK backend had no black-box harness at all, while the TUI side
//! has ~50 `TuiDriver` / `driver_with_shell` sites. This module is the GTK
//! twin: it wraps the real [`App`] — the same `quadraui::ShellApp` impl
//! `gtk::run()` hands to `run_with_shell` — in
//! [`quadraui::gtk::testing::driver_with_shell`], so a test can click, press
//! keys and scroll against production dispatch/paint code with no display.
//!
//! ```ignore
//! let mut h = harness(engine_with_a_split(), 1400, 900);
//! let (x, y) = h.window_center(unfocused).unwrap();
//! h.driver.dispatch(UiEvent::Scroll { position: Point::new(x, y), .. });
//! assert!(h.engine.borrow().windows[&unfocused].view.scroll_top > 0);
//! ```
//!
//! # What this harness actually covers
//!
//! [`harness`] builds the `App` via [`App::new_headless`] (in-memory `Engine`,
//! none of `App::new`'s display-dependent prologue — see that constructor's
//! doc for the exact list of skipped steps) and the *live*
//! [`super::build_shell_config`], the same function `run()` calls. From there
//! `GtkDriver` routes through `quadraui::gtk::run::render_frame` /
//! `dispatch_event` / `dispatch_click` — the same entry points the live GTK
//! runner uses — so `render_content`, `ShellApp::handle`, the ActivityBar
//! focus intercept, accelerators and text-selection drags all behave as they
//! do in production.
//!
//! # Limits — do NOT read a green run here as "the GTK app works"
//!
//! These are inherited from `GtkDriver` (see its module doc) plus vimcode's
//! own constructor gap, and are the reason this harness supplements rather
//! than replaces a live smoke test:
//!
//! - **No real GDK signal delivery.** The driver synthesises
//!   `quadraui::UiEvent`s directly. Raw keycode translation
//!   (`gdk_key_to_uievent`), IME/dead keys, `EventController` wiring,
//!   scroll-event coalescing and GTK's own gesture recognisers are *not*
//!   exercised. A bug that lives in the GDK→`UiEvent` translation layer is
//!   invisible here and can only be caught on a live display.
//! - **No character grid.** Unlike `TuiDriver::screen`, there is nothing to
//!   string-match a whole frame against. Assert with
//!   `painted_texts` / `screen_contains` / `find` / `find_bounds` / `pixel`.
//!   `find*` only sees text the GTK backend records via
//!   `record_painted_text`, which is not every widget — a `None` from `find`
//!   means "not recorded", not necessarily "not drawn".
//! - **No window.** `App::window` stays `None`
//!   (`capture_window_and_apply_csd` finds no mapped toplevel), so CSD
//!   minimise/maximise/close and anything else routed through
//!   `gtk4::Window` is inert under test.
//! - **No display-dependent init.** No CSS is attached to a `GdkDisplay`, no
//!   icon theme search path, no clipboard provider, no
//!   `Engine::startup`/session restore. Behaviour that depends on any of
//!   those is out of reach.
//! - **No main loop.** `tick()` is never pumped by the driver, so timer-driven
//!   work (LSP polling, toasts, the settings-file monitor) does not advance.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use quadraui::gtk::testing::{driver_with_shell, GtkDriver};
use quadraui::AppLogic;

use super::{App, StatusSegmentMap};
use crate::core::Engine;

/// A headless GTK driver plus the `Rc` handle to the engine it drives.
///
/// `GtkDriver::app()` only reaches quadraui's opaque `ShellAdapter`, which has
/// no accessor back to the concrete [`App`] — the same constraint the TUI
/// tests document. Keeping the engine `Rc` alongside the driver is how a test
/// asserts on engine state after an event, instead of being limited to painted
/// pixels.
pub struct Harness<A: AppLogic> {
    pub driver: GtkDriver<A>,
    pub engine: Rc<RefCell<Engine>>,
    /// The `render::ScreenLayout` the last `render_content` pass painted with
    /// — the source for per-editor-window pixel rects (see
    /// [`Self::window_center`]).
    pub screen_layout: Rc<RefCell<Option<crate::render::ScreenLayout>>>,
    /// Picker/command-palette popup rect `(x, y, w, h)` the last frame
    /// actually painted, or `None` if that frame drew no picker — see
    /// [`Self::picker_popup`] (#555).
    #[allow(clippy::type_complexity)]
    pub picker_popup_rect: Rc<std::cell::Cell<Option<(f64, f64, f64, f64)>>>,
    /// Line height the last frame actually painted with (#555).
    pub painted_line_height: Rc<std::cell::Cell<Option<f64>>>,
    /// Character-cell advance (pixels) the shell last reported to `App`.
    /// The horizontal twin of [`Self::painted_line_height`] — needed to turn
    /// `RenderedWindow::gutter_char_width` (char cells) into the pixel band
    /// the line-number gutter occupies (#701).
    pub painted_char_width: Rc<Cell<f64>>,
    /// Completion popup layout the last frame painted, or `None` if that
    /// frame drew no completion popup — the completion twin of
    /// [`Self::picker_popup_rect`] (#669).
    pub completion_layout: Rc<RefCell<Option<quadraui::CompletionsLayout>>>,
    /// Editor hover (rich markdown) popup bounds `(x, y, w, h)` the last
    /// frame painted, or `None` if that frame drew no editor hover popup
    /// (#669).
    #[allow(clippy::type_complexity)]
    pub editor_hover_popup_rect: Rc<std::cell::Cell<Option<(f64, f64, f64, f64)>>>,
    /// The editor hover popup's painted scrollbar track/thumb, or `None` when
    /// its content fits. Exposed for #755's scrollbar-rung coverage: tests
    /// aim at the *painted* thumb rather than hardcoding a pixel.
    pub editor_hover_scrollbar: Rc<std::cell::Cell<Option<crate::render::PopupScrollbarHit>>>,
    /// The editor hover popup's painted link rects `(x, y, w, h, uri)`.
    /// Exposed for #755's link-rung coverage, for the same reason as the
    /// scrollbar above: aim at what was painted, never at a hardcoded pixel.
    #[allow(clippy::type_complexity)]
    pub editor_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>>,
    /// Sidebar-item hover popup bounds `(x, y, w, h)` the last frame painted,
    /// or `None` if that frame drew no panel-hover popup (#670).
    #[allow(clippy::type_complexity)]
    pub panel_hover_popup_rect: Rc<std::cell::Cell<Option<(f64, f64, f64, f64)>>>,
    /// Tab-switcher popup bounds `(x, y, w, h)` the last frame painted, or
    /// `None` if that frame drew no tab switcher (#671). Same field
    /// `handle_mouse_press`'s "Tab switcher modal arbitration" block reads
    /// for click routing (`App::tab_switcher_popup_rect`).
    #[allow(clippy::type_complexity)]
    pub tab_switcher_popup_rect: Rc<std::cell::Cell<Option<(f64, f64, f64, f64)>>>,
    /// The frame rungs the last frame actually composed, in composition order
    /// (#735, folded into one sequence by #766). The GTK half of the
    /// cross-backend sequence-equality assertion — `TuiShellApp` carries the
    /// identical `composed_frame` and its `frame_sequence_*_via_shell_app`
    /// tests assert against the same expected `Vec<FrameOp>`
    /// (`render::frame_sequence_fixture`) for the same engine state.
    pub composed_frame: Rc<RefCell<Vec<crate::render::FrameOp>>>,
    /// The editor rungs the last frame actually composed, in composition order
    /// (#764). The GTK half of the cross-backend editor-band assertion —
    /// `TuiShellApp` carries the identical `composed_editor_band` and its
    /// `editor_band_*_via_shell_app` tests assert against the same expected
    /// `Vec<EditorOp>` for the same engine state.
    pub composed_editor_band: Rc<RefCell<Vec<crate::render::EditorOp>>>,
    /// The bottom rungs the last frame actually composed, in composition order
    /// (#765). The GTK half of the cross-backend bottom-band assertion —
    /// `TuiShellApp` carries the identical `composed_bottom_band` and its
    /// `bottom_band_*_via_shell_app` tests assert against the same expected
    /// `Vec<BottomOp>` for the same engine state.
    pub composed_bottom_band: Rc<RefCell<Vec<crate::render::BottomOp>>>,
    /// The sidebar content rect the last frame painted the active panel into,
    /// or `None` if the sidebar was hidden. The sidebar twin of
    /// [`Self::screen_layout`]'s window rects — aim panel clicks at this rather
    /// than at guessed offsets (#544).
    pub painted_sidebar_bounds: Rc<std::cell::Cell<Option<quadraui::Rect>>>,
    /// Cached per-window (and separated-line) status bar segment hit zones
    /// (#672) — `window_id.0 -> [(start_x, end_x, StatusAction)]`, local to
    /// each bar's own rect. Populated live by `render_content`; see
    /// [`Self::status_segment_center`].
    pub status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Painted rect of the separated status line's status bar, or `None` if
    /// the last frame drew no separated line (#672). See
    /// [`Self::separated_status_segment_center`].
    pub separated_status_bar_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Rect of the drawn inline window-control buttons (minimize/maximize/
    /// close) the last frame painted, or a default (zero) rect before the
    /// first frame / when the menu bar is hidden (#676). The Command Center
    /// (nav arrows + search box) must never paint past this rect's left
    /// edge — see `command_center_does_not_overlap_window_controls`.
    pub title_bar_rect: Rc<Cell<quadraui::Rect>>,
    /// The full menu-bar row band the last frame laid out (`layout
    /// .title_bar_bounds`), or a default (zero) rect before the first frame
    /// (#720). Aim app-icon pixel probes at
    /// `render::split_menu_row_for_app_icon(this).0` rather than at guessed
    /// chrome offsets — see `app_icon_paints_left_of_the_file_menu`.
    pub menu_row_rect: Rc<Cell<quadraui::Rect>>,
    /// Cached in-canvas `DialogLayout` from the last `render_content` paint,
    /// or `None` if that frame drew no in-canvas dialog — either no dialog
    /// was open, or an open one went the #727 native-dialog route instead
    /// (see [`Self::native_dialog_shown`] / [`Self::pending_native_dialog`]).
    pub dialog_layout: Rc<RefCell<Option<quadraui::DialogLayout>>>,
    /// #727: `true` once a native message-dialog present has been queued
    /// (or already shown) for the `engine.dialog` currently open.
    pub native_dialog_shown: Rc<Cell<bool>>,
    /// Cached `ContextMenuLayout` from the last `render_content` paint, or
    /// `None` if that frame drew no context menu (an items-less menu is not
    /// painted). The locate-target for menu-row pixel probes — never assert on
    /// this being `Some`, assert on the pixels it points at (#751).
    pub context_menu_layout: Rc<RefCell<Option<quadraui::ContextMenuLayout>>>,
    /// #727: a native message dialog queued by `render_content`'s
    /// edge-trigger check, awaiting `tick()` to drain it. Tests read this
    /// with `Cell::take` directly (never calling `tick()`, which would
    /// actually try to pop a real `gtk4::AlertDialog` and block forever
    /// with no display / no user to click it) to observe *that* a present
    /// was queued without running the blocking call itself.
    pub pending_native_dialog: Rc<Cell<Option<quadraui::MessageDialogOptions>>>,
    /// Shared claim on the process working directory, held for as long as
    /// this harness can paint (#785).
    ///
    /// Frames painted here are CWD-dependent whether the test knows it or
    /// not: `Engine` roots the file explorer at `std::env::current_dir()`,
    /// and `ext_panel` shows paths relative to it. Meanwhile
    /// `Engine::open_folder` chdirs the *process* by design, and `cargo
    /// test` runs every test in one process across many threads — so a
    /// workspace test on another thread can swap the sidebar's entire
    /// contents out from under a pixel probe here, mid-test. That is what
    /// took `tab_hover_tooltip_paints_below_tab_row_not_inside_it` red: two
    /// frames it compares for equality were painted either side of another
    /// test's `chdir`.
    ///
    /// This guard makes the two mutually exclusive. It is private and
    /// deliberately unnamed by any test — construct a harness through
    /// [`harness`] and the protection comes with it. See `src/test_cwd.rs`
    /// for the mechanism, including the "never take a `CwdGuard` on a thread
    /// holding a harness" rule.
    _cwd: crate::test_cwd::CwdReadGuard,
    /// Held for the harness's whole lifetime so no other thread can be
    /// inside Pango/Cairo text code while this one paints.
    ///
    /// `cargo test` runs the suite on ~20 threads and this harness paints
    /// for real (Cairo `ImageSurface` + `pango::Layout`); libcairo hands
    /// glyph work to a process-global FreeType layer that is not safe to use
    /// from two threads at once, and doing so segfaults inside
    /// `FT_Load_Glyph`. That showed up as an intermittent `SIGSEGV` in ~10%
    /// of full-suite runs of the `vimcode_core` lib test binary — never
    /// reproducible with the GTK tests run alone, because 123 of them rarely
    /// collide. See `src/test_paint.rs` for the coredump stacks and the
    /// mechanism.
    ///
    /// Private and deliberately unnamed by any test, same as [`Self::_cwd`]:
    /// construct a harness through [`harness`] and the protection comes with
    /// it.
    _paint: crate::test_paint::PaintGuard,
}

impl<A: AppLogic> Harness<A> {
    /// Centre point (absolute pixels) of the editor pane for `window_id`, as
    /// the last frame painted it. `None` if that window was not rendered.
    ///
    /// The GTK backend does not `record_painted_text` for editor text, so
    /// `GtkDriver::find` cannot locate a pane. This is the pane-level
    /// substitute, and it keeps the *locate targets, never hardcode coords*
    /// rule intact: aim events at the rect the renderer reported.
    pub fn window_center(&self, window_id: crate::core::WindowId) -> Option<(f32, f32)> {
        let layout = self.screen_layout.borrow();
        let rw = layout
            .as_ref()?
            .windows
            .iter()
            .find(|w| w.window_id == window_id)?;
        Some((
            (rw.rect.x + rw.rect.width / 2.0) as f32,
            (rw.rect.y + rw.rect.height / 2.0) as f32,
        ))
    }

    /// Centre point (absolute pixels) of the `action` segment in
    /// `window_id`'s **per-window** status bar, as the last frame painted it
    /// via `status_segment_map` (#672). `None` if that window has no status
    /// bar, wasn't painted, or doesn't currently show a segment for `action`
    /// (e.g. `ChangeLanguage` when the buffer's filetype is empty).
    ///
    /// `status_segment_map` stores `(start_x, end_x)` local to the bar's own
    /// rect (`window_zone_hit_test`'s `local_x` contract — see `click.rs`),
    /// so this resolves the bar's absolute origin from the same painted
    /// `screen_layout` window rect `window_center` uses, then re-derives the
    /// bar's y-band the identical way `render_content`/`window_zone_hit_test`
    /// do: the bottom `line_height` pixels of the window.
    pub fn status_segment_center(
        &self,
        window_id: crate::core::WindowId,
        action: crate::core::engine::StatusAction,
    ) -> Option<(f32, f32)> {
        let local_x = {
            let map = self.status_segment_map.borrow();
            let zones = map.get(&window_id.0)?;
            let (start, end) = zones
                .iter()
                .find(|(_, _, a)| *a == action)
                .map(|(s, e, _)| (*s, *e))?;
            (start + end) / 2.0
        };
        let layout = self.screen_layout.borrow();
        let rw = layout
            .as_ref()?
            .windows
            .iter()
            .find(|w| w.window_id == window_id)?;
        let lh = self.painted_line_height()?;
        Some((
            (rw.rect.x + local_x) as f32,
            (rw.rect.y + rw.rect.height - lh / 2.0) as f32,
        ))
    }

    /// Centre point (absolute pixels) of the `action` segment in the
    /// **separated status line** (#671/#672) — the full-width bar shown
    /// above the terminal/status band when `window_status_line` is on but
    /// `status_line_above_terminal` is off. `None` if that line wasn't
    /// painted this frame or shows no segment for `action`.
    ///
    /// Keyed by `active_window_id` (the separated line always shows the
    /// active window's status — see `render_content`'s insertion site), and
    /// located via `separated_status_bar_rect`, the painted rect cached
    /// purely so a test can find this bar without re-deriving
    /// `compute_editor_layout`'s panel-stacking arithmetic.
    pub fn separated_status_segment_center(
        &self,
        active_window_id: crate::core::WindowId,
        action: crate::core::engine::StatusAction,
    ) -> Option<(f32, f32)> {
        let local_x = {
            let map = self.status_segment_map.borrow();
            let zones = map.get(&active_window_id.0)?;
            let (start, end) = zones
                .iter()
                .find(|(_, _, a)| *a == action)
                .map(|(s, e, _)| (*s, *e))?;
            (start + end) / 2.0
        };
        let bar = self.separated_status_bar_rect.get()?;
        Some((bar.x + local_x as f32, bar.y + bar.height / 2.0))
    }

    /// Centre point (absolute pixels) of breadcrumb segment `seg_idx` in
    /// `group_id`'s breadcrumb bar, as the last frame painted it. `None` if
    /// that group drew no breadcrumb bar or the segment was clipped away.
    ///
    /// Same *locate targets, never hardcode coords* rule as
    /// `GtkDriver::tab_center`: the x-range comes from the `StatusBarLayout`
    /// the rasteriser cached during the breadcrumb draw pass (`bc:N` hit
    /// regions), offset by the bar's own absolute origin (#555).
    ///
    /// This one stays local (unlike the tab helpers #659 deleted in favour of
    /// quadraui#594's driver versions) because it resolves through vimcode's
    /// own `ScreenLayout::breadcrumbs` — a vimcode structure quadraui has no
    /// equivalent of — rather than through a cached quadraui primitive layout.
    pub fn breadcrumb_segment_center(
        &self,
        group_id: crate::core::window::GroupId,
        seg_idx: usize,
    ) -> Option<(f32, f32)> {
        let layout = self.screen_layout.borrow();
        let bc = layout
            .as_ref()?
            .breadcrumbs
            .iter()
            .find(|b| b.group_id == group_id)?;
        let want = quadraui::WidgetId::new(format!("bc:{seg_idx}"));
        let guard = bc.draw_layout.borrow();
        let sbl = guard.as_ref()?;
        let rect = sbl.hit_regions.iter().find_map(|(r, hit)| match hit {
            quadraui::StatusBarHit::Segment(id) if *id == want => Some(*r),
            _ => None,
        })?;
        Some((
            bc.bounds.x as f32 + rect.x + rect.width / 2.0,
            bc.bounds.y as f32 + rect.y + rect.height / 2.0,
        ))
    }

    /// The picker / command-palette popup rect `(x, y, w, h)` **the last frame
    /// actually painted**, or `None` if that frame painted no picker.
    ///
    /// This is the same cell `App::compute_picker_popup_bounds` hands the
    /// click path, and it is written *only* inside `render_content`'s
    /// `screen.picker` draw branch (and cleared on any frame without one), so
    /// `Some` here means the popup was drawn — not merely that
    /// `engine.picker_open` flipped (#555).
    #[allow(clippy::type_complexity)]
    pub fn picker_popup(&self) -> Option<(f64, f64, f64, f64)> {
        self.picker_popup_rect.get()
    }

    /// Line height the last frame painted with — the value every painted-
    /// geometry hit-test must measure against (#555).
    pub fn painted_line_height(&self) -> Option<f64> {
        self.painted_line_height.get()
    }

    /// Character-cell advance (pixels) the shell last reported. Multiply by
    /// [`crate::render::RenderedWindow::gutter_char_width`] to get the pixel
    /// width of the line-number gutter (#701).
    pub fn painted_char_width(&self) -> f64 {
        self.painted_char_width.get()
    }

    /// Geometry of the painted picker's result list: `(popup_x, list_w,
    /// rows_top, line_height)`. `None` before the picker has painted.
    ///
    /// Mirrors the row layout `quadraui::gtk::draw_palette` paints with — a
    /// title row and a query row (`show_query` is always `true` for vimcode's
    /// picker, see `render::picker_panel_to_palette`) then a 1px separator,
    /// after which each result row is exactly one line high. It is the same
    /// arithmetic `handle_mouse_click_msg`'s picker branch hit-tests with, so
    /// a test that aims here and a user who clicks the pixels resolve to the
    /// same row.
    fn picker_rows_geometry(&self) -> Option<(f64, f64, f64, f64)> {
        let (px, py, pw, _ph) = self.picker_popup()?;
        let lh = self.painted_line_height()?;
        let has_preview = self.engine.borrow().picker_preview.is_some();
        let list_w = if has_preview { (pw * 0.4).round() } else { pw };
        Some((px, list_w, py + lh * 2.0 + 1.0, lh))
    }

    /// Click target for on-screen result row `row` of the painted picker:
    /// a quarter of the way across the result list, vertically centred in the
    /// row (#555). `row` counts painted rows from the top of the list, so it
    /// equals the item index only while the list is scrolled to the top.
    pub fn picker_row_center(&self, row: usize) -> Option<(f32, f32)> {
        let (px, list_w, rows_top, lh) = self.picker_rows_geometry()?;
        Some((
            (px + list_w * 0.25) as f32,
            (rows_top + lh * (row as f64 + 0.5)) as f32,
        ))
    }

    /// Pixel-probe point for on-screen result row `row`: just inside the
    /// popup's left border, vertically centred in the row.
    ///
    /// `draw_palette` fills the selected row's background across the whole
    /// list column but starts its text at `x + 8`, so this point reads the
    /// row's *background* — selected vs not — with no glyph in the way (#555).
    pub fn picker_row_probe(&self, row: usize) -> Option<(i32, i32)> {
        let (px, _list_w, rows_top, lh) = self.picker_rows_geometry()?;
        Some((
            (px + 3.0).round() as i32,
            (rows_top + lh * (row as f64 + 0.5)).round() as i32,
        ))
    }
}

/// Wrap `engine` in the real GTK [`App`] + the live
/// [`super::build_shell_config`] and hand back a headless driver over a
/// `width`×`height` **pixel** surface.
///
/// The engine is supplied by the caller so each test states exactly the
/// buffers, tabs and groups it asserts on — no `Engine::startup`, hence no
/// dependence on the developer's real session (see [`App::new_headless`]).
pub fn harness(engine: Engine, width: i32, height: i32) -> Harness<impl AppLogic> {
    // Both taken *before* the first frame is painted (`driver_with_shell`
    // paints one immediately) and released only when the harness drops — see
    // `Harness::_cwd` (#785) and `Harness::_paint`.
    //
    // Order matters only in that it must be consistent: paint first, then
    // cwd. The `CwdGuard` writers never take the paint lock, so there is no
    // cycle either way — see `src/test_paint.rs`.
    let paint = crate::test_paint::PaintGuard::acquire();
    let cwd = crate::test_cwd::CwdReadGuard::acquire();
    let engine = Rc::new(RefCell::new(engine));
    let app = App::new_headless(Rc::clone(&engine));
    let config = super::build_shell_config(&app);
    let screen_layout = Rc::clone(&app.cached_screen_layout);
    let picker_popup_rect = Rc::clone(&app.picker_popup_rect);
    let painted_line_height = Rc::clone(&app.painted_line_height);
    let painted_char_width = Rc::clone(&app.char_width_cell);
    let painted_sidebar_bounds = Rc::clone(&app.painted_sidebar_bounds);
    let completion_layout = Rc::clone(&app.completion_layout);
    let editor_hover_popup_rect = Rc::clone(&app.editor_hover_popup_rect);
    let editor_hover_scrollbar = Rc::clone(&app.editor_hover_scrollbar);
    let editor_hover_link_rects = Rc::clone(&app.editor_hover_link_rects);
    let panel_hover_popup_rect = Rc::clone(&app.panel_hover_popup_rect);
    let tab_switcher_popup_rect = Rc::clone(&app.tab_switcher_popup_rect);
    let composed_frame = Rc::clone(&app.composed_frame);
    let composed_editor_band = Rc::clone(&app.composed_editor_band);
    let composed_bottom_band = Rc::clone(&app.composed_bottom_band);
    let status_segment_map = Rc::clone(&app.status_segment_map);
    let separated_status_bar_rect = Rc::clone(&app.separated_status_bar_rect);
    let title_bar_rect = Rc::clone(&app.title_bar_rect);
    let menu_row_rect = Rc::clone(&app.menu_row_rect);
    let dialog_layout = Rc::clone(&app.dialog_layout);
    let context_menu_layout = Rc::clone(&app.context_menu_layout);
    let native_dialog_shown = Rc::clone(&app.native_dialog_shown);
    let pending_native_dialog = Rc::clone(&app.pending_native_dialog);
    Harness {
        driver: driver_with_shell(app, config, width, height),
        engine,
        screen_layout,
        picker_popup_rect,
        painted_line_height,
        painted_char_width,
        painted_sidebar_bounds,
        completion_layout,
        editor_hover_popup_rect,
        editor_hover_scrollbar,
        editor_hover_link_rects,
        panel_hover_popup_rect,
        tab_switcher_popup_rect,
        composed_frame,
        composed_editor_band,
        composed_bottom_band,
        status_segment_map,
        separated_status_bar_rect,
        title_bar_rect,
        menu_row_rect,
        dialog_layout,
        context_menu_layout,
        native_dialog_shown,
        pending_native_dialog,
        _cwd: cwd,
        _paint: paint,
    }
}

#[cfg(test)]
mod tests {
    //! Per the #646 scope note: the harness above is the deliverable; tests are
    //! added **per behaviour-changing issue**, not as a suite. There is one
    //! harness smoke and one behaviour test here. Resist growing this into a
    //! catch-all.
    use super::*;
    use crate::core::window::SplitDirection;
    use quadraui::{Point, ScrollDelta, UiEvent};

    /// A live harness must hold the process-CWD claim for its whole
    /// lifetime, so no `chdir`-ing test on another thread can repaint its
    /// sidebar mid-probe (#785).
    ///
    /// Asserted on the lock rather than by racing threads — a timing test
    /// for a timing bug is just a second flake. RED-first: delete the `_cwd`
    /// field from `Harness` (the state this branch's Test-stage failure was
    /// reported against) and `try_write` succeeds here, failing the first
    /// assertion.
    #[test]
    fn harness_holds_the_cwd_claim_while_it_can_paint() {
        let h = harness(Engine::new(), 800, 600);
        assert!(
            crate::test_cwd::CWD_LOCK.try_write().is_err(),
            "a live harness must exclude CwdGuard — every frame it paints \
             roots the file explorer at the process CWD (#785)"
        );

        // Two at once is a normal pattern here (compare frame A with frame
        // B); the claim must be reentrant rather than deadlocking, and must
        // survive the inner one dropping.
        let inner = harness(Engine::new(), 800, 600);
        drop(inner);
        assert!(
            crate::test_cwd::CWD_LOCK.try_write().is_err(),
            "dropping one of two live harnesses must not release the claim"
        );
        drop(h);

        // Deliberately no "and now it is released" assertion: another
        // thread's harness may legitimately hold the shared claim at any
        // moment, so `try_write().is_ok()` here would itself be a flake. A
        // claim that were never released would instead hang every workspace
        // test in the suite — impossible to miss, and not something an
        // assertion has to catch.
    }

    /// Buffer long enough that a viewport scroll cannot be clamped away.
    fn engine_with_long_buffer() -> Engine {
        let mut engine = Engine::new();
        let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
        engine.buffer_mut().insert(0, &text);
        engine
    }

    /// The `WidgetId` the editor tab bar paints under — the key
    /// `GtkDriver::tab_center` / `tab_close_center` (quadraui#594) look their
    /// cached `TabBarLayout` up by.
    ///
    /// #659 deleted this harness's own `tab_center` / `tab_close_center` (the
    /// pair quadraui#594 was promoted *from*) so the geometry has exactly one
    /// implementation, in quadraui, shared with coord-tui. The only vimcode
    /// residue is this id, and it is read straight off the primitive builder
    /// rather than retyped.
    fn editor_tab_bar_id() -> quadraui::WidgetId {
        quadraui::WidgetId::new(crate::render::EDITOR_TAB_BAR_WIDGET_ID)
    }

    // ── #753 (mouse ladder slice 3): dividers + drag ────────────────────
    //
    // The GTK half of the rung this slice lifted into `render.rs`
    // (`route_divider_grab` / `apply_divider_drag` / `TabDragState`); the TUI
    // half lives in `tui_main::shell_app`'s
    // `group_divider_drag_moves_the_painted_divider_via_shell_app` and
    // `group_divider_click_without_move_leaves_the_divider_put_via_shell_app`.

    /// Scan one painted row for the x of the window-split divider line.
    ///
    /// `draw_split` paints no text, so `find`/`find_bounds` cannot see it —
    /// this is the #555 "probe pixels when the content is not a label" route.
    /// Searches for `colour` within `+/- span` of `near`, which keeps the test
    /// honest about *where* the line ended up without hardcoding either end of
    /// the drag.
    fn painted_divider_x<A: AppLogic>(
        h: &mut Harness<A>,
        near: i32,
        y: i32,
        span: i32,
        colour: (u8, u8, u8),
    ) -> Option<i32> {
        (near - span..=near + span).find(|x| h.driver.pixel(*x, y) == colour)
    }

    /// #753, GTK half: dragging a `:vsplit` window divider must repaint the
    /// divider line at the new position.
    ///
    /// Asserts on **rendered pixels** (`CLAUDE.md` rule 1), not on
    /// `App::divider_grab` becoming `Some` — a router that arms the grab and
    /// never applies it would pass that, and "arm" and "apply" are exactly the
    /// two halves this slice moved into shared code.
    ///
    /// The divider's own painted colour is read off the *first* frame rather
    /// than hardcoded, so a theme change cannot silently turn this test into a
    /// tautology.
    #[test]
    fn window_split_divider_drag_repaints_the_line_at_the_new_position() {
        let mut engine = engine_with_long_buffer();
        engine.split_window(SplitDirection::Vertical, None);
        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        // Locate the divider from the geometry the frame actually painted.
        let (start_x, mid_y) = {
            let layout = h.screen_layout.borrow();
            let layout = layout.as_ref().expect("a frame must have been painted");
            let div = layout
                .window_dividers
                .first()
                .expect("a `:vsplit` paints exactly one window divider");
            (
                div.position as i32,
                (div.cross_start + div.cross_size / 2.0) as i32,
            )
        };
        let line_colour = h.driver.pixel(start_x, mid_y);
        let background = h.driver.pixel(start_x - 40, mid_y);
        assert_ne!(
            line_colour, background,
            "the divider line must be visually distinct from the pane behind it, \
             or this test cannot tell whether it moved"
        );

        // Grab the painted line and drag it 200px left.
        let target_x = start_x - 200;
        h.driver.mouse_down(start_x as f32, mid_y as f32);
        h.driver.mouse_move(target_x as f32, mid_y as f32);
        h.driver.mouse_up(target_x as f32, mid_y as f32);
        h.driver.render();

        let repainted = painted_divider_x(&mut h, target_x, mid_y, 8, line_colour)
            .unwrap_or_else(|| panic!("no divider line found near the drag column {target_x}"));
        assert!(
            repainted.abs_diff(target_x) <= 2,
            "the divider line must repaint at the drag column ({target_x}), found it at {repainted}"
        );
        assert!(
            painted_divider_x(&mut h, start_x, mid_y, 4, line_colour).is_none(),
            "the divider line must no longer paint at its old column ({start_x})"
        );
    }

    /// #753, GTK half: a press on the divider followed by a release with no
    /// intervening move must leave the line exactly where it was.
    ///
    /// The arm-without-apply case that `route_divider_grab` and
    /// `apply_divider_drag` are deliberately split across — a router that
    /// applied a ratio on press would nudge the divider here.
    #[test]
    fn window_split_divider_click_without_move_leaves_the_line_put() {
        let mut engine = engine_with_long_buffer();
        engine.split_window(SplitDirection::Vertical, None);
        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        let (start_x, mid_y) = {
            let layout = h.screen_layout.borrow();
            let layout = layout.as_ref().expect("a frame must have been painted");
            let div = layout
                .window_dividers
                .first()
                .expect("a `:vsplit` paints exactly one window divider");
            (
                div.position as i32,
                (div.cross_start + div.cross_size / 2.0) as i32,
            )
        };
        let line_colour = h.driver.pixel(start_x, mid_y);

        h.driver.mouse_down(start_x as f32, mid_y as f32);
        h.driver.mouse_up(start_x as f32, mid_y as f32);
        h.driver.render();

        assert_eq!(
            painted_divider_x(&mut h, start_x, mid_y, 8, line_colour),
            Some(start_x),
            "a press-and-release on the divider with no drag must not move it"
        );
    }

    /// #818: two nested `:vsplit`s paint **two** window-divider lines whose
    /// `split_index` differs (0 for the outer split, 1 for the one nested
    /// inside its second window). This is the exact shape #582/#452 warned
    /// two independently hand-rolled recursive passes over `WindowLayout`
    /// (`calculate_rects` and `dividers`) could number or position
    /// inconsistently — #818 replaced both passes with the leaves/dividers
    /// `quadraui::SplitTree::layout` computes together in one pass. Dragging
    /// only the inner divider must move exactly that line and leave the
    /// outer, sibling divider's column untouched; a split_index/rect mixup
    /// reintroduced by a future change to `WindowLayout::to_split_tree`/
    /// `dividers` would move the wrong one (or both).
    #[test]
    fn nested_window_split_dividers_move_independently_when_dragged() {
        let mut engine = engine_with_long_buffer();
        engine.split_window(SplitDirection::Vertical, None);
        engine.split_window(SplitDirection::Vertical, None);
        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        // Select by `split_index` rather than by comparing painted x — the
        // nested (inner) divider's *position* can land on either side of the
        // outer divider's depending on which child the nesting happened in,
        // so magnitude is not a reliable way to tell them apart. `dividers()`
        // numbers the outer (top-level) split `0` in pre-order and the split
        // nested inside one of its children `1`.
        let (outer_x, inner_x, mid_y) = {
            let layout = h.screen_layout.borrow();
            let layout = layout.as_ref().expect("a frame must have been painted");
            assert_eq!(
                layout.window_dividers.len(),
                2,
                "two nested `:vsplit`s must paint two dividers, got {:?}",
                layout.window_dividers
            );
            let outer = layout
                .window_dividers
                .iter()
                .find(|d| d.split_index == 0)
                .expect("the outer split must be split_index 0");
            let inner = layout
                .window_dividers
                .iter()
                .find(|d| d.split_index == 1)
                .expect("the nested split must be split_index 1");
            (
                outer.position as i32,
                inner.position as i32,
                (inner.cross_start + inner.cross_size / 2.0) as i32,
            )
        };
        assert_ne!(
            outer_x, inner_x,
            "the outer and inner dividers must paint at different columns"
        );

        let line_colour = h.driver.pixel(outer_x, mid_y);
        let background = h.driver.pixel(outer_x - 40, mid_y);
        assert_ne!(
            line_colour, background,
            "the divider line must be visually distinct from the pane behind it, \
             or this test cannot tell whether it moved"
        );

        // Drag only the inner (rightmost) divider.
        let target_x = inner_x - 50;
        h.driver.mouse_down(inner_x as f32, mid_y as f32);
        h.driver.mouse_move(target_x as f32, mid_y as f32);
        h.driver.mouse_up(target_x as f32, mid_y as f32);
        h.driver.render();

        let moved_inner = painted_divider_x(&mut h, target_x, mid_y, 8, line_colour)
            .unwrap_or_else(|| panic!("no divider line found near the drag column {target_x}"));
        assert!(
            moved_inner.abs_diff(target_x) <= 2,
            "the dragged (inner) divider must repaint at {target_x}, found it at {moved_inner}"
        );
        assert!(
            painted_divider_x(&mut h, inner_x, mid_y, 4, line_colour).is_none(),
            "the dragged divider must no longer paint at its old column ({inner_x})"
        );

        // The OUTER divider — the one NOT dragged — must still repaint at (or
        // within a couple of AA/rounding pixels of) its original column. A
        // real split_index/rect mixup would move it by tens or hundreds of
        // pixels (or make it vanish entirely, like the dragged divider's old
        // column above), not by an AA rounding pixel or two — so a tight but
        // non-zero tolerance still catches the bug class this test targets.
        let moved_outer = painted_divider_x(&mut h, outer_x, mid_y, 4, line_colour)
            .unwrap_or_else(|| panic!("the outer divider must still be painted near {outer_x}"));
        assert!(
            moved_outer.abs_diff(outer_x) <= 2,
            "dragging the inner divider must not move the outer, sibling divider \
             (a split_index/rect mixup between the two would move both): outer \
             divider was at {outer_x}, now painted at {moved_outer}"
        );
    }

    /// #753, GTK half of the tab-drag rung: dragging one tab past another must
    /// reorder the **painted** tab bar.
    ///
    /// Drives the whole shared `render::TabDragState` machine through
    /// production dispatch — arm on the tab-bar press, cross the 8px threshold
    /// on the move, re-resolve the press through `pixel_to_click_target`
    /// (GTK's `TabDragMove::Crossed` confirmation), track the drop zone, and
    /// commit on release via `Engine::apply_tab_drop_zone`.
    ///
    /// Asserts on the x of the two painted tab labels, not on
    /// `editor_groups[..].tabs` order: `ScreenLayout` fields have been
    /// populated-but-unpainted before (#587/#592), and the point of a reorder
    /// is what the user sees.
    #[test]
    fn tab_drag_past_a_neighbour_reorders_the_painted_tab_bar() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_753_gtk_tab_drag_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("zqa753.txt");
        let b = dir.join("zqb753.txt");
        std::fs::write(&a, "a\n").unwrap();
        std::fs::write(&b, "b\n").unwrap();

        let mut engine = Engine::new();
        engine.new_tab(Some(&a));
        engine.new_tab(Some(&b));
        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        // The tab *label* is recorded with a trailing space ("zqa753.txt ");
        // the per-window status line and the breadcrumb row paint the same
        // file name without it. Match the trailing space so `find_bounds`
        // cannot resolve to the status bar 800px lower down.
        let tab_label = |name: &str| format!("{name}.txt ");
        let bounds = |h: &Harness<_>, name: &str| {
            let needle = tab_label(name);
            h.driver
                .find_bounds(&needle)
                .unwrap_or_else(|| panic!("tab label {needle:?} must be painted"))
        };
        // Which of the two paints first is `new_tab`'s business, not this
        // test's — take the painted order as given and drag the left one onto
        // the right one.
        let (left, right) = {
            let (a, b) = (bounds(&h, "zqa753"), bounds(&h, "zqb753"));
            if a.x < b.x {
                ("zqa753", "zqb753")
            } else {
                ("zqb753", "zqa753")
            }
        };
        let left_before = bounds(&h, left);
        let right_before = bounds(&h, right);

        let from = (
            left_before.x + left_before.width / 2.0,
            left_before.y + left_before.height / 2.0,
        );
        let to = (
            right_before.x + right_before.width / 2.0,
            right_before.y + right_before.height / 2.0,
        );
        // Two moves, not one: the first crosses the 8px threshold and *starts*
        // the drag (`TabDragMove::Crossed` -> `TabDragState::begin`), the
        // second is the first one to be tracked into a drop zone
        // (`TabDragMove::Tracking` -> `track`). A single move would release
        // with `DropZone::None` and drop nothing — the same sequencing the
        // live backend has always had.
        h.driver.mouse_down(from.0, from.1);
        h.driver.mouse_move(to.0, to.1);
        h.driver.mouse_move(to.0, to.1);
        h.driver.mouse_up(to.0, to.1);
        h.driver.render();

        let left_after = bounds(&h, left);
        let right_after = bounds(&h, right);
        assert!(
            left_after.x > right_after.x,
            "dragging {left} onto {right} must repaint it to the right of it \
             (was {} < {}, now {} vs {})",
            left_before.x,
            right_before.x,
            left_after.x,
            right_after.x
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Harness smoke: the real `App` + the *live* `ShellConfig` construct and
    /// paint a first frame with no display attached. The GTK twin of the TUI's
    /// `shell_app_constructs_via_driver_with_shell`.
    #[test]
    fn constructs_and_paints_a_first_frame_with_no_display() {
        let mut h = harness(engine_with_long_buffer(), 1400, 900);
        // Shell chrome really reached the surface — proves `render_content` ran
        // through the production paint path rather than bailing out early.
        assert!(
            h.driver.screen_contains("EXPLORER"),
            "expected the activity-bar/sidebar chrome to paint; got {:?}",
            h.driver.painted_texts()
        );
        let _ = h.driver.pixel(0, 0);
    }

    /// #646 / #240: a wheel event must scroll the pane **under the pointer**,
    /// not whichever pane happens to hold focus — the behaviour TUI has had
    /// since #240 (`tui_main::mouse` resolves the hovered window from the
    /// event's own row/col).
    ///
    /// This was dead on GTK after the #540 Relm4→ShellApp migration. Two
    /// independent causes, both fixed alongside this test:
    ///
    /// 1. `handle_mouse_scroll_msg` reads the pointer out of `App::last_editor_pointer`,
    ///    which nothing assigned once the `EventControllerMotion` that used to
    ///    write it was removed — so it was permanently `None`.
    /// 2. The hovered-window lookup was gated on `App::drawing_area`, which is
    ///    also never assigned under the ShellApp runner (the runner owns the
    ///    single DrawingArea), and re-derived pane rects at a `(0, 0)` origin
    ///    that ignores the activity-bar/sidebar and title-bar offsets.
    ///
    /// Before the fix this asserted red: **both** wheel events scrolled the
    /// focused pane and the unfocused pane stayed at `scroll_top == 0`.
    ///
    /// Note the assertion that focus does *not* move: hovering must scroll
    /// without stealing focus, which is what distinguishes this from a click.
    #[test]
    fn wheel_scrolls_the_pane_under_the_pointer_not_the_focused_one() {
        let mut engine = engine_with_long_buffer();
        engine.split_window(SplitDirection::Horizontal, None);
        let mut h = harness(engine, 1400, 900);

        let focused = h.engine.borrow().active_window_id();
        let unfocused = *h
            .engine
            .borrow()
            .windows
            .keys()
            .find(|id| **id != focused)
            .expect("`:split` must produce a second window");

        // Aim at the rects the frame actually painted, not at guessed offsets.
        let (ux, uy) = h
            .window_center(unfocused)
            .expect("the unfocused pane must have been painted");
        let (fx, fy) = h
            .window_center(focused)
            .expect("the focused pane must have been painted");

        // #554: a wheel-**down** notch is `delta.y == -1.0`, not `+1.0`.
        // `UiEvent::Scroll.delta` is in quadraui's convention (positive y = up
        // toward the top of the content) — see `gdk_scroll_to_uievent`, which
        // negates GTK's raw `dy` to produce it. This closure previously
        // dispatched `+1.0` and still asserted `scroll_top` *increases*, which
        // only held because the GTK `UiEvent::Scroll` arm was passing the
        // quadraui-convention delta straight through to `handle_mouse_scroll_msg`
        // (which wants GTK-raw polarity) — the very inversion #554 reports.
        let wheel_down_at = |h: &mut Harness<_>, x: f32, y: f32| {
            h.driver.dispatch(UiEvent::Scroll {
                widget: None,
                position: Point::new(x, y),
                delta: ScrollDelta::new(0.0, -1.0),
            });
        };

        wheel_down_at(&mut h, ux, uy);
        {
            let e = h.engine.borrow();
            assert!(
                e.windows[&unfocused].view.scroll_top > 0,
                "wheel over the unfocused pane must scroll it (scroll_top stayed 0)"
            );
            assert_eq!(
                e.windows[&focused].view.scroll_top, 0,
                "wheel over the unfocused pane must NOT scroll the focused pane"
            );
            assert_eq!(
                e.active_window_id(),
                focused,
                "hovering to scroll must not move focus"
            );
        }

        // ...and the focused pane still scrolls when the pointer is over it.
        let before = h.engine.borrow().windows[&unfocused].view.scroll_top;
        wheel_down_at(&mut h, fx, fy);
        {
            let e = h.engine.borrow();
            assert!(
                e.windows[&focused].view.scroll_top > 0,
                "wheel over the focused pane must scroll it"
            );
            assert_eq!(
                e.windows[&unfocused].view.scroll_top, before,
                "wheel over the focused pane must not disturb the other pane"
            );
        }
    }

    /// #554: scrolling the wheel **down** must move the viewport **down**.
    ///
    /// Drives the *real* GDK translator (`gdk_scroll_to_uievent`, re-exported
    /// by `super::events` from `quadraui::gtk::events`) rather than a
    /// hand-built `UiEvent`, so the whole polarity chain is under test in one
    /// place:
    ///
    /// ```text
    ///   GDK dy  ──gdk_scroll_to_uievent──▶  UiEvent::Scroll.delta.y
    ///   (+ = down)        (negates)          (+ = up, quadraui convention)
    ///           ──ShellApp::handle──▶  handle_mouse_scroll_msg delta_y
    ///                (negates back)      (+ = down, GTK-raw — what every
    ///                                     downstream consumer expects)
    /// ```
    ///
    /// The #540 Relm4→ShellApp migration deleted the `connect_scroll` closure
    /// that fed the scroll handler GTK's raw `dy` and left the runner's
    /// already-negated `UiEvent::Scroll` as the only source, dropping the
    /// second negation. Every wheel notch then reached the engine with the
    /// sign flipped: wheel-down scrolled the text up.
    ///
    /// Both halves matter. Asserting the translator alone would stay green
    /// with the bug (`gdk_scroll_to_uievent` was never wrong); asserting the
    /// engine alone off a hand-built `UiEvent` would go green again the moment
    /// someone "fixed" the inversion by flipping the *translator* and breaking
    /// TUI/macOS, which share it.
    #[test]
    fn gdk_wheel_down_scrolls_the_viewport_down_not_up() {
        use crate::gtk::events::gdk_scroll_to_uievent;

        let mut h = harness(engine_with_long_buffer(), 1400, 900);
        let win = h.engine.borrow().active_window_id();
        let (x, y) = h
            .window_center(win)
            .expect("the editor pane must have been painted");

        // Half 1 — the translation itself. GTK reports positive dy for a
        // wheel-down notch; `UiEvent::Scroll` carries the negated value.
        let down = gdk_scroll_to_uievent(0.0, 1.0, x as f64, y as f64);
        match &down {
            UiEvent::Scroll {
                delta, position, ..
            } => {
                assert_eq!(
                    delta.y, -1.0,
                    "GDK dy=+1 (wheel down) must translate to delta.y=-1 \
                     (quadraui: positive y = up)"
                );
                assert_eq!(delta.x, 0.0, "a pure vertical notch must not pan x");
                assert_eq!(*position, Point::new(x, y), "the wheel position is lost");
            }
            other => panic!("expected UiEvent::Scroll, got {other:?}"),
        }

        // Half 2 — what that event does to the engine, through production
        // dispatch. Wheel down ⇒ later lines come into view ⇒ scroll_top rises.
        h.driver.dispatch(down);
        let after_down = h.engine.borrow().windows[&win].view.scroll_top;
        assert!(
            after_down > 0,
            "wheel down must move the viewport DOWN (scroll_top 0 -> >0), \
             got {after_down} — direction is inverted (#554)"
        );

        // ...and the opposite notch walks it back, so this cannot pass by a
        // consumer that ignores the sign entirely.
        h.driver
            .dispatch(gdk_scroll_to_uievent(0.0, -1.0, x as f64, y as f64));
        let after_up = h.engine.borrow().windows[&win].view.scroll_top;
        assert!(
            after_up < after_down,
            "wheel up must move the viewport back UP ({after_down} -> {after_up})"
        );
    }

    /// #672: the debug-output bottom panel's scroll wheel routes through
    /// `quadraui::dispatch_scroll` against `engine.scroll_surfaces` — a list
    /// that, under `ShellApp`, only the dead `src/gtk/draw.rs` ever pushed
    /// to. `handle_mouse_scroll_msg` hit-tests it (mod.rs `"debug_output" =>` arm)
    /// but nothing populated it, so this scroll was a silent no-op with the
    /// panel visible and painted. This test fails red against an empty list
    /// (falls through to the generic active-window viewport scroll instead,
    /// leaving `debug_output_scroll` at 0) and only passes once
    /// `render_content` registers the surface every frame, the way TUI's
    /// `render_impl.rs` always has.
    #[test]
    fn wheel_scrolls_the_debug_output_panel_via_registered_scroll_surface() {
        let mut engine = Engine::new();
        engine.bottom_panel_open = true;
        engine.bottom_panel_kind = crate::core::engine::BottomPanelKind::DebugOutput;
        engine.dap_output_lines = (0..200).map(|i| format!("line {i}")).collect();
        let mut h = harness(engine, 1400, 900);

        let geometry = h
            .engine
            .borrow()
            .bottom_panel_geometry
            .borrow()
            .expect("the debug output panel must have painted geometry");
        let win_x = h
            .screen_layout
            .borrow()
            .as_ref()
            .and_then(|s| s.windows.first())
            .map(|w| w.rect.x)
            .expect("the editor window must have painted a rect");

        // Aim inside the panel's content band: one content row below its top
        // (skips the tab-bar + toolbar rows `content_y` already accounts for).
        let x = (win_x + 50.0) as f32;
        let y = (geometry.top_y + geometry.content_y + geometry.content_row_h) as f32;

        // Quadraui-convention delta: negative y = scroll down (see the
        // polarity test above). `handle_debug_output_scroll` interprets a
        // positive GTK-raw `delta.y` (what `ShellApp::handle` negates this
        // back into) as "scroll down, toward newer output".
        h.driver.dispatch(UiEvent::Scroll {
            widget: None,
            position: Point::new(x, y),
            delta: ScrollDelta::new(0.0, -1.0),
        });

        assert!(
            h.engine.borrow().debug_output_scroll > 0,
            "wheel over the debug output panel must scroll it via the \
             registered `scroll_surfaces` entry (scroll_top stayed 0 — \
             the surface list is empty or the scroll fell through to the \
             editor viewport instead)"
        );
    }

    /// #731 review: the test above only proves `debug_output_scroll`
    /// (engine *state*) advances — CLAUDE.md calls that exact pattern out
    /// by name as insufficient black-box coverage ("assert on rendered
    /// output — never on state being populated"). Before #731, this same
    /// `"debug_output" =>` arm called `if let Some(da) =
    /// self.drawing_area.borrow().as_ref() { da.queue_draw(); }` on a
    /// handle permanently `None` under the ShellApp runner, and nothing
    /// else in the arm set `draw_needed`. So `App::handle` fell through to
    /// `Reaction::Continue`, `GtkDriver::dispatch` never called `render()`,
    /// and the painted frame would have stayed byte-identical to the
    /// pre-scroll one even though engine state changed — the #587/#592 "state
    /// changes, screen doesn't" bug class.
    ///
    /// Scrolling *down* (as the test above does) doesn't actually move the
    /// painted content here: `dap_output_lines` starts with `auto_scroll:
    /// true` (pins the view to the tail), and `handle_debug_output_scroll`'s
    /// `delta_y > 0.0` branch only sets `auto_scroll = true` — it never turns
    /// it off — so the panel keeps rendering the same tail lines no matter
    /// how far `debug_output_scroll` climbs (verified empirically: a fresh
    /// harness built with `debug_output_scroll: 0` and one built with `: 3`
    /// paint byte-identical panels). Scrolling *up* is the direction that
    /// exercises the fix: `handle_debug_output_scroll`'s `else` branch
    /// unconditionally sets `auto_scroll = false`, switching the painted
    /// view from "pinned to the tail" to "pinned to `scroll_offset`" —
    /// `quadraui::TextDisplay`'s documented `auto_scroll` semantics
    /// (`primitives/text_display.rs`) — which repaints a visibly different
    /// set of lines even though `debug_output_scroll` itself stays 0. This
    /// test fails red against the pre-#731 code: it snapshots the panel,
    /// scrolls up, re-snapshots, and requires at least one pixel to differ.
    #[test]
    fn wheel_scroll_up_on_debug_output_panel_repaints_the_unpinned_view() {
        let mut engine = Engine::new();
        engine.bottom_panel_open = true;
        engine.bottom_panel_kind = crate::core::engine::BottomPanelKind::DebugOutput;
        engine.dap_output_lines = (0..200).map(|i| format!("line {i}")).collect();
        assert!(
            engine.debug_output_auto_scroll,
            "fixture sanity: auto-scroll must start on (pinned to the tail) \
             for the pre-#731 failure mode below to be reachable at all"
        );
        let mut h = harness(engine, 1400, 900);

        let geometry = h
            .engine
            .borrow()
            .bottom_panel_geometry
            .borrow()
            .expect("the debug output panel must have painted geometry");
        let win_x = h
            .screen_layout
            .borrow()
            .as_ref()
            .and_then(|s| s.windows.first())
            .map(|w| w.rect.x)
            .expect("the editor window must have painted a rect");

        let x = (win_x + 50.0) as f32;
        let y = (geometry.top_y + geometry.content_y + geometry.content_row_h) as f32;

        // Sample the panel's whole content band.
        let (bx0, by0) = (win_x as i32, geometry.top_y as i32);
        let (bx1, by1) = (
            (win_x + 400.0) as i32,
            (geometry.top_y + geometry.height) as i32,
        );
        let sample = |h: &mut Harness<_>| {
            let mut px = Vec::new();
            let mut sy = by0;
            while sy < by1 {
                let mut sx = bx0;
                while sx < bx1 {
                    px.push(h.driver.pixel(sx, sy));
                    sx += 2;
                }
                sy += 2;
            }
            px
        };

        let before = sample(&mut h);

        // Opposite sign from the "wheel down" test above — GTK-raw wheel-up,
        // routed the same way (`handle_mouse_scroll_msg` -> `dispatch_scroll` ->
        // this `"debug_output" =>` arm) but landing in
        // `handle_debug_output_scroll`'s `else` branch.
        h.driver.dispatch(UiEvent::Scroll {
            widget: None,
            position: Point::new(x, y),
            delta: ScrollDelta::new(0.0, 1.0),
        });

        assert!(
            !h.engine.borrow().debug_output_auto_scroll,
            "fixture sanity: wheel-up must turn auto-scroll off, or this \
             test can't distinguish a real repaint fix from coincidence"
        );

        let after = sample(&mut h);
        assert_ne!(
            before, after,
            "wheel-scrolling up on the debug output panel must repaint the \
             now-unpinned view, not just flip `auto_scroll` silently behind \
             an unqueued redraw — see this test's doc comment for the exact \
             pre-#731 failure mode this catches"
        );
    }

    /// #672: the per-window status bar's segment click hit-test
    /// (`click.rs::pixel_to_click_target`'s `WindowZone::StatusBar` arm)
    /// reads `status_segment_map`, but under `ShellApp` nothing ever
    /// populated it before this fix — the dead
    /// `draw.rs::draw_window_status_bar` was the map's only writer, so
    /// every segment click (goto-line, change-language, switch-branch, ...)
    /// silently resolved to `ClickTarget::None`. This drives a real click at
    /// the "Ln N, Col N" segment's painted `(start_x, end_x)` — recovered
    /// live from `status_segment_map`, not guessed — and asserts the picker
    /// it opens actually *paints* (not just an engine flag flip): this test
    /// fails red against an empty map, because the click then falls through
    /// to the editor's `TextArea` zone instead and never reaches
    /// `StatusAction::GoToLine`.
    #[test]
    fn status_bar_segment_click_opens_go_to_line_picker() {
        let mut h = harness(engine_with_long_buffer(), 1400, 900);
        let win = h.engine.borrow().active_window_id();
        assert!(
            h.engine.borrow().settings.window_status_line,
            "fixture assumes the per-window status bar is on by default"
        );

        assert!(
            !h.engine.borrow().picker_open,
            "no picker should be open before the click"
        );
        assert!(
            h.picker_popup().is_none(),
            "no picker popup should have painted before the click"
        );

        let (x, y) = h
            .status_segment_center(win, crate::core::engine::StatusAction::GoToLine)
            .expect(
                "the per-window status bar must have painted a GoToLine \
                 (\"Ln N, Col N\") segment into status_segment_map",
            );
        h.driver.click(x, y);

        assert!(
            h.engine.borrow().picker_open,
            "clicking the status bar's Ln/Col segment must open the \
             go-to-line picker (#672)"
        );
        assert_eq!(
            h.engine.borrow().picker_source,
            crate::core::engine::PickerSource::CommandCenter,
            "GoToLine routes through the CommandCenter picker with a `:` \
             query prefix (Engine::handle_status_action)"
        );
        // `picker_popup_rect` is written only inside `render_content`'s
        // picker draw branch, so this proves the popup actually painted —
        // not merely that engine state flipped (mirrors
        // `breadcrumb_segment_click_opens_the_dropdown_and_selection_dispatches`,
        // #555).
        let (_, _, pw, ph) = h
            .picker_popup()
            .expect("the go-to-line picker must actually paint, not just flip engine state");
        assert!(
            pw > 0.0 && ph > 0.0,
            "the painted picker popup must have a non-degenerate rect, got {pw}x{ph}"
        );
    }

    /// #672's separated-status-line twin of
    /// [`status_bar_segment_click_opens_go_to_line_picker`]: with
    /// `status_line_above_terminal` off and a bottom panel open, the active
    /// window's status is extracted into the full-width `separated_status_line`
    /// bar instead of living inside the window (`render::build_screen_layout`'s
    /// `separate_status` branch — the window itself paints no status bar in
    /// this mode). That bar's segment hit zones are inserted into the *same*
    /// `status_segment_map`, keyed by `active_window_id`, at the second call
    /// site the review flagged (mod.rs's "Draw separated status line" block) —
    /// exercising it here, separately from the per-window path above, is what
    /// proves *both* flagged insertion sites are wired live, not just one.
    #[test]
    fn separated_status_line_segment_click_opens_go_to_line_picker() {
        let mut engine = engine_with_long_buffer();
        engine.settings.status_line_above_terminal = false;
        engine.terminal_open = true;
        engine.session.terminal_panel_rows = 10;
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();

        assert!(
            !h.engine.borrow().picker_open,
            "no picker should be open before the click"
        );
        assert!(
            h.picker_popup().is_none(),
            "no picker popup should have painted before the click"
        );

        let (x, y) = h
            .separated_status_segment_center(win, crate::core::engine::StatusAction::GoToLine)
            .expect(
                "the separated status line must have painted a GoToLine \
                 (\"Ln N, Col N\") segment into status_segment_map",
            );
        h.driver.click(x, y);

        assert!(
            h.engine.borrow().picker_open,
            "clicking the separated status line's Ln/Col segment must open \
             the go-to-line picker (#672)"
        );
        let (_, _, pw, ph) = h
            .picker_popup()
            .expect("the go-to-line picker must actually paint, not just flip engine state");
        assert!(
            pw > 0.0 && ph > 0.0,
            "the painted picker popup must have a non-degenerate rect, got {pw}x{ph}"
        );
    }

    /// Three tabs in the default **single** editor group — the exact shape
    /// #553 reports as dead (tab clicks came back to life as soon as a second
    /// group existed).
    fn engine_with_three_tabs_one_group() -> Engine {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "alpha");
        engine.new_tab(None);
        engine.new_tab(None);
        engine
    }

    /// #553: with a single tab group, clicking a non-active tab must activate
    /// it. Regression from the #540 Relm4→ShellApp migration — the split-group
    /// path worked, the single-group path did not.
    ///
    /// # What this test does NOT prove
    ///
    /// This and [`single_group_tab_close_button_closes_that_tab`] are the
    /// **acceptance** tests #553 asks for (drive a real click through
    /// production `dispatch_click` and assert the engine's active tab / tab
    /// count changed). They are *not* regression guards for the specific
    /// pre-`8fbbf85` hit-band defect — they stay green with that bug
    /// reinstated, for two independent reasons:
    ///
    /// 1. GTK's `pixel_to_click_target` resolves clicks primarily via the
    ///    cached `quadraui::FrameHitMap` (#449) and only falls back to
    ///    `screen_zone_hit_test` / `render::tab_bar_hit_bands` on a miss, so
    ///    a driver-level click never reaches the regressed code at all.
    /// 2. Even with the fallback forced, this harness's default
    ///    `ShellConfig::with_title_bar(1.0)` chrome offsets the content origin
    ///    by only ~23px — less than the tab bar's own height — so the painted
    ///    click y lands inside *both* the correct band and the buggy
    ///    origin-anchored one. Separating them needs an offset larger than
    ///    `tab_bar_height`.
    ///
    /// The regression guard at the dispatch layer is
    /// `gtk::click::single_group_tab_click_dispatch_tests::
    /// single_group_tab_click_activates_and_close_button_targets_that_tab`,
    /// which passes `frame_hit_map: None` and a synthetic 100px content offset
    /// for exactly these reasons; the layout-level guards are
    /// `render::tests::test_tab_bar_hit_bands_single_and_split_share_one_derivation`
    /// and `test_single_group_tab_bar_hit_test_with_editor_offset`.
    #[test]
    fn single_group_tab_click_activates_that_tab() {
        let mut h = harness(engine_with_three_tabs_one_group(), 1400, 900);
        let group = h.engine.borrow().active_group;
        assert_eq!(
            h.engine.borrow().editor_groups[&group].tabs.len(),
            3,
            "fixture must open three tabs in one group"
        );
        assert_eq!(
            h.engine.borrow().editor_groups[&group].active_tab,
            2,
            "`new_tab` activates the tab it creates"
        );

        let (x, y) = h
            .driver
            .tab_center(&editor_tab_bar_id(), 0)
            .expect("the single-group tab bar must have painted tab 0");
        h.driver.click(x, y);

        assert_eq!(
            h.engine.borrow().editor_groups[&group].active_tab,
            0,
            "clicking tab 0 in a single-group layout must activate it"
        );
    }

    /// #659: the quadraui#594 driver helpers resolve against vimcode's editor
    /// tab bar at all — i.e. [`crate::render::EDITOR_TAB_BAR_WIDGET_ID`] is
    /// really the id `GtkBackend::draw_tab_bar` cached the layout under, and
    /// the bar paints during a plain harness frame.
    ///
    /// Also pins the `None` contract the deleted local helpers had (#553) and
    /// the promoted ones keep: an index past the last visible tab resolves to
    /// `None` rather than to a stale or clamped point, for the close button as
    /// well as the tab body. quadraui covers the *other* `None` case — a tab
    /// with `is_closable: false` — in
    /// `quadraui::gtk::testing::tests::tab_close_center_none_when_tab_not_closable`;
    /// it is not re-tested here because
    /// [`crate::render::build_tab_bar_primitive`] hardcodes `is_closable:
    /// true`, so vimcode cannot construct that state. Not duplicating it is
    /// the point of the promotion.
    #[test]
    fn driver_tab_geometry_resolves_for_the_editor_tab_bar() {
        let h = harness(engine_with_three_tabs_one_group(), 1400, 900);
        let bar = editor_tab_bar_id();

        let mut centers = Vec::new();
        for idx in 0..3 {
            let c = h
                .driver
                .tab_center(&bar, idx)
                .unwrap_or_else(|| panic!("tab {idx} must have painted"));
            assert!(
                h.driver.tab_close_center(&bar, idx).is_some(),
                "tab {idx} is closable, so it must have painted a × "
            );
            centers.push(c);
        }
        assert!(
            centers[0].0 < centers[1].0 && centers[1].0 < centers[2].0,
            "tab centres must increase left to right, got {centers:?}"
        );
        assert!(
            centers.iter().all(|c| c.1 == centers[0].1),
            "one bar means one y-centre, got {centers:?}"
        );

        assert_eq!(
            h.driver.tab_center(&bar, 3),
            None,
            "an index past the last tab must not resolve to a point"
        );
        assert_eq!(
            h.driver.tab_close_center(&bar, 3),
            None,
            "…and neither must its close button (#553)"
        );

        assert_eq!(
            h.driver
                .tab_center(&quadraui::WidgetId::new("tabs:not-a-real-bar"), 0),
            None,
            "an id no bar painted under must resolve to None, not to another \
             bar's geometry"
        );
    }

    /// #553: with a single tab group, clicking a tab's × must close it.
    ///
    /// # It found a real bug (#659), fixed upstream by quadraui#615 (#679)
    ///
    /// This test was `#[ignore]`d for the life of #659 because it exposed a
    /// genuine GTK paint-vs-hit-test divergence in quadraui. The `#[ignore]`
    /// was lifted by #679, which bumped the quadraui pin to `6a8a959`
    /// ("fix(quadraui#615): stop double-shifting `GtkBackend::tab_bar_layout`
    /// hits"). **The assertion below never changed** — it was correct all
    /// along, and the upstream fix is what made it pass. The diagnosis is kept
    /// here because it is the only written record of how the two coordinate
    /// spaces drifted, and re-reading it is cheaper than re-deriving it.
    ///
    /// This test was green before #659 and went red after it, and **the code
    /// under test did not change**. Only the source of the click coordinate
    /// did, from this module's own `tab_close_center` to quadraui#594's
    /// `GtkDriver::tab_close_center`. The two disagreed because they read
    /// different things:
    ///
    /// * the deleted local helper read `App::cached_tab_close_abs`, derived
    ///   from `Backend::tab_bar_layout()` — the *same* no-paint measurement
    ///   `click::pixel_to_click_target` resolves against, so the harness and
    ///   the click router shared one number and agreed with each other no
    ///   matter what was actually drawn;
    /// * `GtkDriver::tab_close_center` reads the `TabBarLayout` that
    ///   `GtkBackend::draw_tab_bar` cached **while painting**.
    ///
    /// So the old assertion was a closed loop: it could only ever fail if
    /// `tab_bar_layout()` disagreed with itself. Pointing it at the painted
    /// geometry opened the loop, and the loop turned out to be open by a whole
    /// tab's width. Measured in this harness at 1400×900, three tabs, one
    /// group, at the then-pinned `5a418ca`:
    ///
    /// * pixel-scanning row y=40 finds the active (third) tab's background
    ///   spanning x=766..939, and the tab strip starting at x=418 — i.e. the
    ///   painted tab pitch is ~174px and tab 0 is painted at 418..592;
    /// * `Backend::tab_bar_layout()` for that same bar (`rect.x == 418`)
    ///   returns `slot_positions == [(836, 1010), (1010, 1184), (1184,
    ///   1358)]` — the right *pitch* (174) but carrying `rect.x` twice
    ///   (836 == 418 + 418).
    ///
    /// # Exact quadraui defect (confirmed by instrumenting `render_content`)
    ///
    /// `GtkBackend::tab_bar_layout` (`quadraui/src/gtk/backend.rs`) added
    /// `rect.x` to every hit range **twice**, at the then-pinned rev
    /// `5a418ca`:
    ///
    /// 1. `crate::backend::shift_tab_bar_hits(&mut hits, rect.x as f64)`,
    ///    added by quadraui#552 right after `tab_bar_layout_to_hits` (whose
    ///    own doc comment says its spans are bar-relative and the caller owes
    ///    exactly one `shift_tab_bar_hits`);
    /// 2. the older hand-rolled `let x_off = rect.x as f64; …` loop at the
    ///    bottom of the same function, which #552 left in place — it walks
    ///    `slot_positions`, `close_bounds` and `right_segment_bounds` and adds
    ///    `x_off` to each a second time.
    ///
    /// Deleting either one fixes it, and quadraui#615 deleted the second.
    /// `gtk::draw_tab_bar` (the paint path, in `quadraui/src/gtk/tab_bar.rs`)
    /// always shifted exactly once, which is why the painted glyphs and
    /// `GtkDriver::tab_close_center` agreed with each other and only the
    /// no-paint query was wrong.
    ///
    /// The net effect on a user was that *whenever the tab bar did not start
    /// at x == 0* — i.e. any time the sidebar/activity bar is open — every
    /// entry in `App::cached_tab_pixel_hits` was one `rect.x` too far right, so
    /// `click::resolve_pixel_tab_click` matched nothing and GTK silently fell
    /// back to `resolve_charcell_tab_click`. That fallback is a monospace
    /// approximation of a proportional-font bar: it landed close enough to keep
    /// *selecting* a tab roughly right (which is why
    /// `single_group_tab_click_activates_that_tab` kept passing throughout) but
    /// it never resolved the narrow × zone, so clicking a tab's close button
    /// did nothing at all.
    ///
    /// It reproduced **identically on the pre-#659 quadraui pin** (`f6d27c2`):
    /// the same pixel scan gave the same 766..939 band, so the #659 pin bump
    /// did not cause it and reverting the pin would not have fixed it. It was a
    /// pre-existing GTK paint-vs-hit-test divergence that the #659 migration
    /// merely made visible, and per CLAUDE.md's Platform-Neutrality Rule the
    /// fix belonged in quadraui (`GtkBackend::tab_bar_layout` / `gtk::
    /// draw_tab_bar` must agree on the coordinate space), not in a vimcode
    /// backend file — which is exactly where it landed.
    #[test]
    fn single_group_tab_close_button_closes_that_tab() {
        let mut h = harness(engine_with_three_tabs_one_group(), 1400, 900);
        let group = h.engine.borrow().active_group;
        let before = h.engine.borrow().editor_groups[&group].tabs.len();
        assert_eq!(before, 3);

        let (x, y) = h
            .driver
            .tab_close_center(&editor_tab_bar_id(), 0)
            .expect("the single-group tab bar must have painted tab 0's close button");
        h.driver.click(x, y);

        assert_eq!(
            h.engine.borrow().editor_groups[&group].tabs.len(),
            before - 1,
            "clicking a tab's × in a single-group layout must close it"
        );
    }

    // ── #703: per-tab language icons (quadraui `draw_tab_bar_icons`) ────────

    /// Three tabs in one group, each backed by a distinguishable real file
    /// path so the tabs paint different labels *and* different language
    /// badges (`.rs` orange, `.py` blue, `.md` blue).
    fn engine_with_three_named_tabs() -> Engine {
        let mut engine = Engine::new();
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.cwd = cwd.clone();
        let names = ["alpha703.rs", "beta703.py", "gamma703.md"];
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                engine.new_tab(None);
            }
            let buf = engine.active_buffer_id();
            if let Some(state) = engine.buffer_manager.get_mut(buf) {
                state.file_path = Some(cwd.join(name));
            }
        }
        engine
    }

    /// Display names of a group's tabs, in slot order — read from the engine
    /// so the assertion says *which* tab survived, not just how many did.
    fn tab_names(h: &Harness<impl AppLogic>) -> Vec<String> {
        let engine = h.engine.borrow();
        let group = &engine.editor_groups[&engine.active_group];
        group
            .tabs
            .iter()
            .map(|tab| {
                engine
                    .windows
                    .get(&tab.active_window)
                    .and_then(|w| engine.buffer_manager.get(w.buffer_id))
                    .map(|s| s.display_name())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// True for a pixel painted in the Rust badge's identity colour,
    /// [`crate::icons::ICON_ORANGE`].
    ///
    /// A small tolerance rather than exact equality because Cairo antialiases
    /// the glyph against the tab background — but a *small* one (±25 per
    /// channel), because the glyph's core does land on the nominal value and
    /// a loose "reddish" predicate matches warm antialiasing fringes from the
    /// label text itself.
    fn is_icon_orange((r, g, b): (u8, u8, u8)) -> bool {
        let (want_r, want_g, want_b) = crate::icons::ICON_ORANGE;
        let near = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 25;
        near(r, want_r) && near(g, want_g) && near(b, want_b)
    }

    /// Two tabs whose labels are the same length and the same language, so
    /// their painted slots are (to within a pixel of Pango kerning) equal
    /// width — which is what lets [`tab_zero_left_half`] recover tab 0's left
    /// edge from the two tab centres alone.
    fn engine_with_two_rust_tabs() -> Engine {
        let mut engine = Engine::new();
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.cwd = cwd.clone();
        for (i, name) in ["aaa703.rs", "bbb703.rs"].iter().enumerate() {
            if i > 0 {
                engine.new_tab(None);
            }
            let buf = engine.active_buffer_id();
            if let Some(state) = engine.buffer_manager.get_mut(buf) {
                state.file_path = Some(cwd.join(name));
            }
        }
        engine
    }

    /// Every pixel in the **left half of tab 0's painted slot** — the strip
    /// that holds the tab's leading padding, its icon, and the start of its
    /// label — plus tab 0's centre x.
    ///
    /// The slot is recovered from the two tab centres rather than from
    /// `find_bounds`: `GtkDriver::find_bounds` returns the *first* painted
    /// label matching a needle, and the breadcrumb bar and the explorer tree
    /// both paint the same filename, so a needle-anchored probe silently
    /// measures the wrong widget (it did — the label "moved" from x=490 to
    /// x=490 because both readings were the breadcrumb's). `tab_center`
    /// resolves against the `TabBarLayout` the rasteriser actually cached
    /// while painting this bar, which is unambiguous.
    fn tab_zero_left_half(h: &mut Harness<impl AppLogic>) -> (Vec<(u8, u8, u8)>, f32) {
        let bar = editor_tab_bar_id();
        let c0 = h
            .driver
            .tab_center(&bar, 0)
            .expect("tab 0 must have painted");
        let c1 = h
            .driver
            .tab_center(&bar, 1)
            .expect("tab 1 must have painted");
        // Equal-width slots ⇒ the centre-to-centre distance is one slot, so
        // tab 0's left edge is half a slot left of its own centre.
        let left = c0.0 - (c1.0 - c0.0) / 2.0;
        let mut px = Vec::new();
        for x in (left.max(0.0) as i32)..(c0.0 as i32) {
            for y in (c0.1 as i32 - 6)..(c0.1 as i32 + 6) {
                px.push(h.driver.pixel(x, y));
            }
        }
        (px, c0.0)
    }

    /// #703 acceptance (GTK): a `.rs` tab paints its language badge — in the
    /// badge's own identity colour — inside its own slot, and that badge
    /// occupies real space: the tab is wider than the identical tab rendered
    /// with `&[]`.
    ///
    /// # Why this fails against unfixed `develop`
    ///
    /// `develop` paints through `Surface::TabBar` → `Backend::draw_tab_bar`,
    /// which has no icon sidecar at all: tab 0's slot is bare background plus
    /// a grey label, so `is_icon_orange` matches nothing, and tab 0's centre
    /// is identical with Nerd Fonts on and off.
    #[test]
    fn tab_paints_its_language_icon_and_widens_the_tab() {
        let prev_nf = crate::icons::nerd_fonts_enabled();

        // The flag has to be set *after* `Engine::new` (which applies the
        // developer's own settings and would otherwise clobber it) and before
        // the harness paints its first frame.
        let render_with_nerd_fonts = |on: bool| {
            let mut engine = engine_with_two_rust_tabs();
            engine.settings.use_nerd_fonts = on;
            crate::icons::set_nerd_fonts(on);
            let mut h = harness(engine, 1400, 900);
            tab_zero_left_half(&mut h)
        };

        let (on_px, on_center_x) = render_with_nerd_fonts(true);
        let (off_px, off_center_x) = render_with_nerd_fonts(false);

        crate::icons::set_nerd_fonts(prev_nf);

        let reddest = |px: &[(u8, u8, u8)]| {
            px.iter()
                .max_by_key(|(r, _, b)| *r as i32 - *b as i32)
                .copied()
        };
        assert!(
            on_px.iter().copied().any(is_icon_orange),
            "a .rs tab must paint its orange Rust badge inside its own slot; \
             sampled {} px, reddest was {:?}",
            on_px.len(),
            reddest(&on_px)
        );
        assert!(
            !off_px.iter().copied().any(is_icon_orange),
            "with Nerd Fonts off nothing may be painted there — `&[]`, not \
             an ASCII fallback; reddest was {:?}",
            reddest(&off_px)
        );
        assert!(
            on_center_x > off_center_x,
            "the icon reservation must widen the tab, pushing its centre \
             right: with icons {on_center_x}, without {off_center_x}"
        );
    }

    /// #703, **the regression that matters**: with icons painted, a click on
    /// the painted × must still close the tab it sits on.
    ///
    /// # Why this fails if `tab_bar_layout` is reinstated at `gtk/mod.rs`
    ///
    /// GTK caches click geometry from a *second*, no-paint measurement pass
    /// (`App::cached_tab_pixel_hits`). quadraui's own doc is explicit that a
    /// caller which paints with icons must measure with
    /// `tab_bar_layout_icons`: the icon reservation widens every decorated
    /// tab, so the icon-less twin reports every slot and close-button bound
    /// shifted left of the painted glyphs, by a cumulative one icon width per
    /// preceding tab.
    ///
    /// Verified by doing exactly that — swapping the `render_content` call
    /// back to `backend.tab_bar_layout(tb_rect, target.bar)` — and re-running:
    /// **no tab closes at all**. The × zone is narrow (`tighten_close_bounds`
    /// trims it to the glyph box), so a drift of one icon width moves it clean
    /// off every cached hit zone and `resolve_pixel_tab_click` matches
    /// nothing, exactly the dead-close-button symptom #659/quadraui#615
    /// produced. `single_group_tab_close_button_closes_that_tab` above goes
    /// red under the same swap; this test adds the *which* tab (tab 1 of 3,
    /// where the cumulative drift is larger than tab 0's).
    #[test]
    fn tab_close_button_closes_the_tab_under_the_cursor_with_icons_painted() {
        let prev_nf = crate::icons::nerd_fonts_enabled();
        // After `Engine::new` (which applies the developer's own settings),
        // before the harness paints — see `tab_paints_its_language_icon…`.
        let mut engine = engine_with_three_named_tabs();
        engine.settings.use_nerd_fonts = true;
        crate::icons::set_nerd_fonts(true);
        let mut h = harness(engine, 1400, 900);

        assert_eq!(
            tab_names(&h),
            vec!["alpha703.rs", "beta703.py", "gamma703.md"],
            "fixture must open three distinguishable tabs in one group"
        );

        let (x, y) = h
            .driver
            .tab_close_center(&editor_tab_bar_id(), 1)
            .expect("tab 1 must have painted a close button");
        h.driver.click(x, y);
        crate::icons::set_nerd_fonts(prev_nf);

        assert_eq!(
            tab_names(&h),
            vec!["alpha703.rs", "gamma703.md"],
            "clicking tab 1's painted × must close tab 1 — measuring with \
             the icon-less `tab_bar_layout` while painting with icons closes \
             the tab to its left"
        );
    }

    /// An engine whose active buffer has a real (multi-component) file path
    /// under `cwd`, so `build_breadcrumbs_for_group` produces one clickable
    /// segment per path component.
    fn engine_with_breadcrumb_path() -> Engine {
        let mut engine = Engine::new_for_test();
        // Use the crate root as cwd so the scoped file picker the dropdown
        // opens has real entries to list (`picker_populate_files` walks cwd).
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.cwd = cwd.clone();
        let buf = engine.active_buffer_id();
        if let Some(state) = engine.buffer_manager.get_mut(buf) {
            state.file_path = Some(cwd.join("src").join("main.rs"));
        }
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine
    }

    /// #555: clicking a breadcrumb segment must open its dropdown (the scoped
    /// picker) and a selection inside that dropdown must dispatch.
    ///
    /// Regression from the #540 Relm4->ShellApp migration: the breadcrumb bar
    /// went dead because the click handler hit-tests `bc.draw_layout`, which
    /// only the *paint* pass fills in.
    ///
    /// # Why this asserts on pixels rather than `screen_contains`
    ///
    /// The dropdown is a `quadraui::Palette`, and `GtkBackend::draw_palette`
    /// contributes nothing to the painted-text map at the quadraui rev
    /// `quadraui-pin.txt` pins (paint-time text recording — quadraui's
    /// `gtk/painted_text.rs` — landed after it). So `screen_contains("Find
    /// Files")` / `find("<row label>")` cannot see the popup here, exactly as
    /// this module's header warns: a `None` from `find` means "not recorded",
    /// not "not drawn". The pixel probes below are rev-independent and are in
    /// fact the stronger check — they prove the *painted* row the highlight
    /// moved to is the same row the click resolved to, which a text match
    /// would not.
    #[test]
    fn breadcrumb_segment_click_opens_the_dropdown_and_selection_dispatches() {
        let mut h = harness(engine_with_breadcrumb_path(), 1400, 900);
        assert!(
            h.engine.borrow().settings.breadcrumbs,
            "fixture assumes breadcrumbs are on by default"
        );
        let group = h.engine.borrow().active_group;

        let (x, y) = h
            .breadcrumb_segment_center(group, 0)
            .expect("the breadcrumb bar must have painted segment 0 with a hit region");

        assert!(
            !h.engine.borrow().picker_open,
            "no picker should be open before the click"
        );
        assert!(
            h.picker_popup().is_none(),
            "no picker popup should have painted before the click"
        );

        h.driver.click(x, y);

        assert!(
            h.engine.borrow().picker_open,
            "clicking a breadcrumb segment must open its dropdown (#555)"
        );
        // `picker_popup` is written only inside `render_content`'s picker draw
        // branch, so this is paint, not engine bookkeeping.
        let (px, py, pw, ph) = h
            .picker_popup()
            .expect("the dropdown must actually paint, not just flip engine state (#555)");
        assert!(
            pw > 0.0 && ph > 0.0,
            "the painted dropdown must have a non-degenerate rect, got {pw}x{ph}"
        );

        let lh = h
            .painted_line_height()
            .expect("the frame must publish the line height it painted with");
        let rows_on_screen = ((ph - lh * 2.0 - 1.0 - 4.0) / lh) as usize;
        let items = h.engine.borrow().picker_items.len();
        // Row 0 is selected on open; ROW is the row this test clicks. Both must
        // be painted, and far enough apart to probe independently.
        const ROW: usize = 3;
        assert!(
            items > ROW && rows_on_screen > ROW,
            "fixture must list more than {ROW} rows and paint them all \
             ({items} items, {rows_on_screen} rows visible)"
        );

        // Row 0 is the open-state selection, so its background is the palette's
        // selection colour and every other row's is the popup background. If
        // the popup had not painted, both probes would read the same editor
        // pixel.
        let (sel_x, sel_y) = h.picker_row_probe(0).expect("row 0 must be painted");
        let (un_x, un_y) = h.picker_row_probe(ROW).expect("row {ROW} must be painted");
        let selected_bg = h.driver.pixel(sel_x, sel_y);
        let unselected_bg = h.driver.pixel(un_x, un_y);
        assert_ne!(
            selected_bg, unselected_bg,
            "the painted dropdown must highlight its selected row (#555); \
             both probes read {selected_bg:?} at popup ({px}, {py}) {pw}x{ph}"
        );

        // ...and clicking a row inside the dropdown selects it. The target sits
        // in the popup's left column, which the centred popup overlays on top of
        // the sidebar — the exact band `try_route_sidebar_mouse_event` used to
        // swallow before the press could reach the picker.
        let (rx, ry) = h
            .picker_row_center(ROW)
            .expect("the dropdown's result rows must be locatable");
        h.driver.click(rx, ry);

        let picked = {
            let e = h.engine.borrow();
            assert_eq!(
                e.picker_selected, ROW,
                "clicking painted row {ROW} must select item {ROW}"
            );
            e.picker_items[ROW].display.clone()
        };

        // The highlight must have followed the click in the *painted* frame:
        // row ROW now reads the selection colour and row 0 the plain one. This
        // is what pins paint geometry and click geometry to each other — the
        // "cache at paint, hit-test at click" invariant #555 broke.
        assert_eq!(
            h.driver.pixel(un_x, un_y),
            selected_bg,
            "the clicked row must paint as selected"
        );
        assert_eq!(
            h.driver.pixel(sel_x, sel_y),
            unselected_bg,
            "the previously selected row must paint as unselected"
        );

        // Confirming that selection navigates: the picker closes and the
        // active buffer is the file the user picked.
        h.driver.press_named(quadraui::NamedKey::Enter);
        {
            let e = h.engine.borrow();
            assert!(
                !e.picker_open,
                "confirming a dropdown entry must close the dropdown"
            );
            let path = e
                .buffer_manager
                .get(e.active_buffer_id())
                .and_then(|b| b.file_path.clone())
                .expect("confirming must open a file into the active buffer");
            assert!(
                path.ends_with(&picked),
                "confirming `{picked}` must navigate to it; landed on {path:?}"
            );
        }
        assert!(
            h.picker_popup().is_none(),
            "the frame painted after confirming must draw no dropdown"
        );
    }

    // ── #700: VS Code chrome-metrics parity ─────────────────────────────────

    /// #700 item 4: a tab paints as its bare filename (`"main.rs"`), not the
    /// old `" 1: main.rs "` ordinal-prefixed label — and the close glyph
    /// still hit-tests, proving `quadraui::TabItem`'s "filename after the
    /// last `\": \"`" underline contract and `tighten_close_bounds`'s
    /// geometry both degrade correctly with no `": "` in the label at all
    /// (`rfind(": ").unwrap_or(0)` underlines/measures from byte 0, i.e. the
    /// whole label, when there's no separator — see `render.rs`'s
    /// `TabInfo::name` doc).
    ///
    /// RED-first: reinstating the old `format!(" {}: {} ", i + 1, name)`
    /// makes `screen_contains("main.rs")` still pass (it's a substring of
    /// `" 1: main.rs "`) but `!screen_contains(": main.rs")` goes red, and
    /// `find("1:")` starts resolving — this test's actual regression guard.
    #[test]
    fn tab_label_paints_without_ordinal_prefix_and_close_glyph_still_hit_tests() {
        let h = harness(engine_with_breadcrumb_path(), 1400, 900);
        assert!(
            h.driver.screen_contains("main.rs"),
            "the tab must paint its filename; painted texts: {:?}",
            h.driver.painted_texts()
        );
        assert!(
            !h.driver.screen_contains(": main.rs") && !h.driver.screen_contains("1:"),
            "#700: the tab label must not carry an ordinal prefix; painted \
             texts: {:?}",
            h.driver.painted_texts()
        );

        let bar = editor_tab_bar_id();
        assert!(
            h.driver.tab_center(&bar, 0).is_some(),
            "the (only) tab must have painted"
        );
        assert!(
            h.driver.tab_close_center(&bar, 0).is_some(),
            "the close glyph must still hit-test with no ordinal prefix in \
             the label"
        );
    }

    /// #700 items 2/3: the tab-bar row and the breadcrumb row are fixed-pixel
    /// chrome, not `ceil(line_height * 1.6)` / `+ line_height`. This harness
    /// cannot vary `settings.font_size` and observe a painted difference —
    /// vimcode's GTK runner paints the editor at a hardcoded "Monospace 11"
    /// regardless of `settings.font_size`/`font_family` (see the
    /// `build_editor_click_context` call site's doc comment in
    /// `App::render_content`), so `render::tests::
    /// test_tab_bar_height_px_independent_of_font_size` (varying the
    /// `line_height` parameter those helpers actually take) is the real
    /// font-size-independence proof; this test instead pins that the fixed
    /// pixel constants actually reach the live paint pipeline, and that
    /// breadcrumbs add exactly [`crate::render::BREADCRUMB_ROW_HEIGHT_PX`] —
    /// not a whole `line_height`-tall row — above the window content.
    ///
    /// RED-first: reinstating the old `tab_bar_height = tab_row_height +
    /// line_height` formula makes `with_breadcrumbs_y - without_breadcrumbs_y`
    /// equal the harness's painted `line_height` instead of `22.0`; the
    /// assertion below that those two differ is what proves the harness's
    /// `line_height` isn't coincidentally `22.0` already.
    #[test]
    fn breadcrumb_row_adds_a_fixed_22px_not_a_whole_line_height() {
        let h_on = harness(engine_with_breadcrumb_path(), 1400, 900);
        assert!(h_on.engine.borrow().settings.breadcrumbs);
        let win_on = h_on.engine.borrow().active_window_id();
        h_on.window_center(win_on)
            .expect("editor pane must paint with breadcrumbs on");
        let on_top = {
            let layout = h_on.screen_layout.borrow();
            layout
                .as_ref()
                .unwrap()
                .windows
                .iter()
                .find(|w| w.window_id == win_on)
                .unwrap()
                .rect
                .y
        };

        let mut engine_off = engine_with_breadcrumb_path();
        engine_off.settings.breadcrumbs = false;
        let h_off = harness(engine_off, 1400, 900);
        let win_off = h_off.engine.borrow().active_window_id();
        h_off
            .window_center(win_off)
            .expect("editor pane must paint with breadcrumbs off");
        let off_top = {
            let layout = h_off.screen_layout.borrow();
            layout
                .as_ref()
                .unwrap()
                .windows
                .iter()
                .find(|w| w.window_id == win_off)
                .unwrap()
                .rect
                .y
        };

        let delta = on_top - off_top;
        let lh = h_on
            .painted_line_height()
            .expect("frame must publish the line height it painted with");
        assert_ne!(
            lh, 22.0,
            "test setup sanity: the harness's painted line_height must not \
             coincidentally equal BREADCRUMB_ROW_HEIGHT_PX, or this test \
             cannot distinguish the fix from the old `+ line_height` bug"
        );
        assert!(
            (delta - crate::render::BREADCRUMB_ROW_HEIGHT_PX).abs() < 0.5,
            "breadcrumbs must reserve exactly BREADCRUMB_ROW_HEIGHT_PX \
             ({}) above the window content, not line_height ({lh}); got \
             delta {delta} (on_top={on_top}, off_top={off_top})",
            crate::render::BREADCRUMB_ROW_HEIGHT_PX
        );
    }

    /// #705 item 3 / quadraui#624: breadcrumb text must track
    /// `settings.ui_font_size` — the app's dedicated "chrome font size"
    /// knob (#217, `UI_FONT_FAMILY`/`UI_FONT_SIZE` in `gtk/mod.rs`) — not
    /// float at whatever font a previous draw call in the frame happened to
    /// leave on the shared Pango layout. Before `App::render_content` was
    /// wired to call `Backend::set_ui_font(&UI_FONT())` on the *paint*
    /// backend every frame (quadraui#624's mechanism), `ui_font` sat at
    /// quadraui's own hardcoded `"Sans 11"` default forever — two very
    /// different `ui_font_size` values would have painted breadcrumb text
    /// at the identical size. That is the RED case this test's first
    /// assertion catches: comment out the `backend.set_ui_font(&UI_FONT())`
    /// call and `big.width > small.width` goes false.
    ///
    /// Measures *width*, not height: `find_bounds`' recorded rect reports a
    /// fixed row-slot height for status-bar segments (matching
    /// `BREADCRUMB_ROW_HEIGHT_PX`) regardless of the font actually painted,
    /// but glyph width scales with point size same as it does — this
    /// mirrors quadraui#624's own GTK positive control
    /// (`gtk_backend_menu_bar_layout_ui_font_size_is_not_inert`: "changing
    /// `ui_font` alone must visibly widen the measured menu item").
    ///
    /// This does not (and, per the sibling
    /// `breadcrumb_row_adds_a_fixed_22px_not_a_whole_line_height` test's
    /// doc comment just above, cannot within this harness) vary
    /// `settings.font_size` — vimcode's GTK runner paints the editor at a
    /// hardcoded font regardless of that setting, so there is no live paint
    /// path from it to anything this test could observe change. The second
    /// assertion below still pins the *intended* independence contract as a
    /// regression guard: if `font_size` is ever wired into the editor paint
    /// path in a way that also (incorrectly) reaches chrome text, this
    /// starts failing.
    #[test]
    fn breadcrumb_text_width_tracks_ui_font_size_not_editor_font_size() {
        let mut engine_small = engine_with_breadcrumb_path();
        engine_small.settings.ui_font_size = 8;
        let h_small = harness(engine_small, 1400, 900);
        let small = h_small
            .driver
            .find_bounds("src")
            .expect("the breadcrumb's \"src\" path segment must paint");

        let mut engine_big = engine_with_breadcrumb_path();
        engine_big.settings.ui_font_size = 28;
        let h_big = harness(engine_big, 1400, 900);
        let big = h_big
            .driver
            .find_bounds("src")
            .expect("the breadcrumb's \"src\" path segment must paint");

        assert!(
            big.width > small.width * 1.5,
            "breadcrumb glyph width must track settings.ui_font_size (8 vs \
             28 pt): got small={:?} big={:?} — if these are close, \
             `Backend::set_ui_font` isn't reaching the paint backend and \
             breadcrumb text is stuck at quadraui's hardcoded chrome-font \
             default",
            small,
            big
        );

        // Regression guard (see doc comment above): the editor's own font
        // knob must not leak into chrome text, however that ever gets wired.
        let mut engine_a = engine_with_breadcrumb_path();
        engine_a.settings.font_size = 10;
        let h_a = harness(engine_a, 1400, 900);
        let a = h_a
            .driver
            .find_bounds("src")
            .expect("the breadcrumb's \"src\" path segment must paint");

        let mut engine_b = engine_with_breadcrumb_path();
        engine_b.settings.font_size = 60;
        let h_b = harness(engine_b, 1400, 900);
        let b = h_b
            .driver
            .find_bounds("src")
            .expect("the breadcrumb's \"src\" path segment must paint");

        assert!(
            (a.width - b.width).abs() < 0.5,
            "breadcrumb glyph width must NOT track settings.font_size (10 \
             vs 60): got a={a:?} b={b:?}"
        );
    }

    /// #704 item 1 / quadraui#624: tab labels must also honour
    /// `Backend::set_ui_font` — the other non-dialog surface #704's
    /// acceptance criterion names ("a tab label or status-bar segment";
    /// the sibling test just above already covers the status-bar/breadcrumb
    /// half). `draw_tab_bar_icons`'s paint call and its no-paint measurement
    /// twin both switched to `ui_font` under quadraui#624 — before that
    /// landed, every chrome surface except `draw_dialog`/
    /// `draw_rich_text_popup` painted with the shared *editor* Pango layout,
    /// so `set_ui_font` was silently a no-op here too (#704's "Do not start
    /// this before quadraui#624 lands" blocker, now cleared).
    ///
    /// #704's actual code change is widening `UI_FONT_FAMILY` (this module,
    /// `gtk/mod.rs`) to try real Linux desktop UI fonts (Cantarell, Ubuntu)
    /// ahead of the generic `Sans` fallback it used to collapse to. There is
    /// no user-facing `ui_font_family` setting to vary directly the way
    /// `ui_font_size` can be (see the sibling test), and family+size travel
    /// to the paint backend as ONE Pango font-description string
    /// (`UI_FONT()`, `App::render_content`'s `backend.set_ui_font(&UI_FONT())`
    /// call) with no separate code path for either half. So proving the
    /// *size* half reaches the tab bar — mirroring quadraui#624's own GTK
    /// positive control, `gtk_backend_menu_bar_layout_ui_font_size_is_not_inert`
    /// — is proof the *family* half reaches it too: this is the practical
    /// form of "chrome glyph extents change when `UI_FONT_FAMILY` changes,
    /// on a surface that is not a dialog" that a hardcoded `const` (rather
    /// than a runtime setting) admits.
    #[test]
    fn tab_label_width_tracks_ui_font_size_not_editor_font_size() {
        let mut engine_small = engine_with_three_named_tabs();
        engine_small.settings.ui_font_size = 8;
        let h_small = harness(engine_small, 1400, 900);
        let small = h_small
            .driver
            .find_bounds("alpha703")
            .expect("the tab label must paint");

        let mut engine_big = engine_with_three_named_tabs();
        engine_big.settings.ui_font_size = 28;
        let h_big = harness(engine_big, 1400, 900);
        let big = h_big
            .driver
            .find_bounds("alpha703")
            .expect("the tab label must paint");

        assert!(
            big.width > small.width * 1.5,
            "tab label glyph width must track settings.ui_font_size (8 vs \
             28 pt): got small={:?} big={:?} — if these are close, \
             `Backend::set_ui_font` isn't reaching the tab bar and labels \
             are stuck at quadraui's hardcoded chrome-font default",
            small,
            big
        );
    }

    /// #705 item 5 / quadraui#625: menu-bar mnemonic underlines must not
    /// paint unconditionally. Before quadraui#625, `alt_char_byte_range`
    /// fell back to underlining char 0 whenever a label carried no `&` at
    /// all — #700 stripped the `&` from every `MenuDef` label
    /// (`render::build_menu_defs`) hoping that would silence the underline,
    /// but the fallback ignored the missing marker and underlined "File"'s
    /// 'F' regardless (see that function's doc comment, now updated).
    /// quadraui#625 part (1) fixed the fallback to return `None` (no
    /// underline) instead of defaulting to char 0.
    ///
    /// Pixel-probing the underline stroke's own absolute color/position is
    /// deliberately avoided — quadraui's own test suite documents why
    /// (`alt_char_byte_range_indexes_display_text_not_label`'s doc comment:
    /// "the underline's own pixels are Pango-font-metric dependent and not
    /// stable across CI hosts"). Instead this paints the SAME "File" label
    /// twice, on the SAME host, in the SAME process — once through the
    /// real, `&`-free `build_menu_defs()` output, once with a synthetic
    /// `&File` substituted for just that one entry — and diffs the
    /// identical glyph region between the two frames. `display_text` strips
    /// the `&` before layout, so both frames paint literally "File" at the
    /// same position/size (asserted below); the ONLY possible pixel
    /// difference left between the two captures is the underline
    /// decoration itself.
    ///
    /// RED-first: if the char-0 fallback bug were reinstated, BOTH frames
    /// would underline 'F' (the fallback doesn't care whether `&` is
    /// present), the two captures would be pixel-identical, and `differs`
    /// would come back `false`. A test asserting only that the real,
    /// `&`-free render has *some* property (e.g. "no underline color at a
    /// hardcoded offset") could pass by coincidence against a wrong offset
    /// guess; this diffs against a positive control instead.
    #[test]
    fn menu_bar_underline_absent_without_ampersand_present_with_it() {
        // Both harnesses re-apply `set_menus` once, post-construction,
        // through the *identical* code path — differing only in the "File"
        // label's text — so neither picks up any stray hover/active-index
        // state difference between "however `setup()`'s one-time
        // `set_menus` call left things" and "a second `set_menus` call
        // plus an extra `render()`". An earlier version of this test
        // called `set_menus` only on the marked side and saw the whole
        // menu-item cell's background differ between the two captures
        // (an `is_active` highlight artifact of that asymmetry) — a false
        // signal that had nothing to do with the underline.
        let mut h_plain = harness(Engine::new_for_test(), 1200, 800);
        {
            let defs = crate::render::build_menu_defs(false);
            h_plain
                .engine
                .borrow()
                .menu_system
                .borrow_mut()
                .set_menus(defs);
        }
        h_plain.driver.render();
        let bounds_plain = h_plain
            .driver
            .find_bounds("File")
            .expect("the File menu-bar header must paint");
        assert!(
            bounds_plain.y < 40.0,
            "sanity: \"File\" should resolve to the menu-bar header near the \
             top of the window, not some other painted text; got {bounds_plain:?}"
        );

        let mut h_marked = harness(Engine::new_for_test(), 1200, 800);
        {
            let mut defs = crate::render::build_menu_defs(false);
            let file_def = defs
                .iter_mut()
                .find(|d| d.label == "File")
                .expect("MENU_STRUCTURE must carry a File entry");
            file_def.label = "&File".to_string();
            h_marked
                .engine
                .borrow()
                .menu_system
                .borrow_mut()
                .set_menus(defs);
        }
        h_marked.driver.render();
        let bounds_marked = h_marked
            .driver
            .find_bounds("File")
            .expect("the File menu-bar header must still paint with the marker");

        assert_eq!(
            (
                bounds_plain.x,
                bounds_plain.y,
                bounds_plain.width,
                bounds_plain.height
            ),
            (
                bounds_marked.x,
                bounds_marked.y,
                bounds_marked.width,
                bounds_marked.height
            ),
            "the `&` must be stripped before layout — both frames should \
             paint literally \"File\" at the identical position/size, \
             isolating the underline decoration as the only possible pixel \
             difference"
        );

        let x0 = bounds_plain.x.floor() as i32;
        let x1 = (bounds_plain.x + bounds_plain.width).ceil() as i32;
        let y0 = bounds_plain.y.floor() as i32;
        // A few px of slack below the ink rect — underlines paint just
        // under the baseline, which may sit slightly outside the recorded
        // ink extents.
        let y1 = (bounds_plain.y + bounds_plain.height).ceil() as i32 + 3;

        let mut differs = false;
        for y in y0..y1 {
            for x in x0..x1 {
                if h_plain.driver.pixel(x, y) != h_marked.driver.pixel(x, y) {
                    differs = true;
                }
            }
        }
        assert!(
            differs,
            "expected the synthetic \"&File\" render to paint an underline \
             stroke somewhere under 'F' that the real, `&`-free \"File\" \
             render does not — got pixel-identical regions in \
             x=[{x0},{x1}) y=[{y0},{y1}), meaning either underlines never \
             paint at all (masking a real regression elsewhere) or the \
             char-0 fallback bug is back and both labels underline \
             regardless of `&`"
        );
    }

    /// #700 item 6: indent guides must actually *paint* by default, not just
    /// leave `settings.indent_guides` set to `true` — the exact gap #587/#592
    /// (`ScreenLayout.picker`) burned ~5 sessions on: a state field flipped on
    /// for months with nothing painting it, and a test asserting the field
    /// alone would have stayed green throughout. `settings.rs`'s
    /// `test_settings_default` already pins the field; this pins the pixels.
    ///
    /// Renders the same indented buffer twice — once at the real default
    /// (`Engine::new()`, untouched), once with `indent_guides` forced off —
    /// and requires the two frames to differ somewhere in the indented
    /// region. `quadraui::gtk::editor`'s indent-guide rasteriser (pinned rev,
    /// `src/gtk/editor.rs:290-306`) strokes a 1px vertical line at each guide
    /// column, so any real diff here can only be that stroke.
    ///
    /// RED-first: forcing `indent_guides: false` on *both* harnesses (i.e.
    /// simulating the pre-#700 off-by-default state) collapses `differing`
    /// to 0 and this test fails — confirmed by hand before restoring the
    /// real default.
    #[test]
    fn indent_guides_paint_by_default() {
        // Two levels of leading-space indent (tabstop defaults to 4), so a
        // guide is expected at column 0 and column 4 on the third line.
        let indented_text = "fn main() {\n    if true {\n        let x = 1;\n    }\n}\n";

        let mut engine_on = Engine::new();
        engine_on.buffer_mut().insert(0, indented_text);
        assert!(
            engine_on.settings.indent_guides,
            "test setup sanity: Engine::new()'s real default must be on, or \
             this test isn't exercising the default at all"
        );
        let mut h_on = harness(engine_on, 1400, 900);
        let win_on = h_on.engine.borrow().active_window_id();
        h_on.window_center(win_on)
            .expect("editor pane must paint with the default settings");
        let rect_on = {
            let layout = h_on.screen_layout.borrow();
            layout
                .as_ref()
                .unwrap()
                .windows
                .iter()
                .find(|w| w.window_id == win_on)
                .unwrap()
                .rect
        };
        let lh = h_on
            .painted_line_height()
            .expect("frame must publish the line height it painted with");

        let mut engine_off = Engine::new();
        engine_off.buffer_mut().insert(0, indented_text);
        engine_off.settings.indent_guides = false;
        let mut h_off = harness(engine_off, 1400, 900);
        let win_off = h_off.engine.borrow().active_window_id();
        h_off
            .window_center(win_off)
            .expect("editor pane must paint with guides disabled");

        // Row 2 (0-indexed) is "        let x = 1;" — 8 columns of leading
        // whitespace, so both the col-0 and col-4 guides should be live.
        let y = (rect_on.y + lh * 2.5) as i32;
        let x0 = rect_on.x as i32;
        let x1 = (rect_on.x + 100.0) as i32;

        let mut differing = 0;
        for x in x0..x1 {
            if h_on.driver.pixel(x, y) != h_off.driver.pixel(x, y) {
                differing += 1;
            }
        }
        assert!(
            differing > 0,
            "indent guides must paint visibly different pixels on an \
             indented line by default (#700 item 6); sampled x in \
             {x0}..{x1} at y={y}, 0/{} differed",
            x1 - x0
        );
    }

    /// Two editor groups whose buffers have breadcrumb paths of *different*
    /// depths: the active group (A) shows `a.rs` (1 segment), the other group
    /// (B) shows `src/core/deep.rs` (3 segments). Returns `(engine, group_b)`.
    fn engine_with_two_groups_different_depths() -> (Engine, crate::core::window::GroupId) {
        use crate::core::window::SplitDirection;
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut engine = Engine::new_for_test();
        engine.cwd = cwd.clone();
        let buf_a = engine.active_buffer_id();
        if let Some(st) = engine.buffer_manager.get_mut(buf_a) {
            st.file_path = Some(cwd.join("a.rs"));
        }
        let group_a = engine.active_group;

        engine.open_editor_group(SplitDirection::Vertical);
        let group_b = engine.active_group;
        assert_ne!(group_a, group_b, "`open_editor_group` must create a group");

        let buf_b = engine.buffer_manager.create();
        if let Some(st) = engine.buffer_manager.get_mut(buf_b) {
            st.file_path = Some(cwd.join("src").join("core").join("deep.rs"));
        }
        let win_b = engine.active_window_id();
        if let Some(w) = engine.windows.get_mut(&win_b) {
            w.buffer_id = buf_b;
        }

        // Focus goes back to A — the shape the bug needs.
        engine.active_group = group_a;
        (engine, group_b)
    }

    /// #555: a breadcrumb click must act on the group whose bar was clicked.
    ///
    /// `resolve_breadcrumb_click` scans *every* group's bar and returned a
    /// bare segment index, which `Engine::handle_breadcrumb_click` then
    /// resolved against the **active** group's segments. Click the deeper
    /// group's third segment while a shallower group holds focus and the
    /// index is out of range, so `breadcrumb_open_scoped` bails and the click
    /// does nothing at all — the "clicks dead" half of the report.
    #[test]
    fn breadcrumb_click_acts_on_the_clicked_group_not_the_focused_one() {
        let (engine, group_b) = engine_with_two_groups_different_depths();
        let mut h = harness(engine, 1600, 900);
        let group_a = h.engine.borrow().active_group;

        // Segment 2 of B = `deep.rs`; group A's bar only has segment 0.
        let (x, y) = h
            .breadcrumb_segment_center(group_b, 2)
            .expect("group B's breadcrumb bar must have painted three segments");

        h.driver.click(x, y);

        assert!(
            h.engine.borrow().picker_open,
            "clicking group B's breadcrumb must open a dropdown even while \
             group A is focused (#555)"
        );
        assert_eq!(
            h.engine.borrow().active_group,
            group_b,
            "the clicked group must take focus, so the dropdown is scoped to \
             the file the user actually clicked (was {group_a:?})"
        );
    }

    /// #557: a plugin-registered ("extension") panel must contribute a
    /// **visible** activity-bar icon on the GTK `ShellApp` path.
    ///
    /// `build_shell_config` derives the runner's panel list from the engine's
    /// `AppShell`, which only ever holds the seven built-ins — extension
    /// panels live in `engine.ext_panels` and were dropped entirely, so e.g.
    /// the Git Insights extension registered a display name and an icon and
    /// nothing rendered for it.
    ///
    /// `use_nerd_fonts` is pinned off (and `ext_panels` cleared of whatever
    /// the developer has actually installed) so the glyph painted is the
    /// registration's plain-ASCII *fallback*, which any font can render —
    /// a Nerd Font codepoint would make this depend on the test machine
    /// having Symbols Nerd Font installed.
    ///
    /// Asserted **in pixels**, not painted text: quadraui's GTK activity-bar
    /// rasteriser draws its glyphs straight to the Cairo context
    /// (`quadraui/src/gtk/activity_bar.rs`) rather than through the recorded
    /// `draw_text` path, so `painted_texts()` cannot see the icon strip at
    /// all. Rather than hard-code the icon's row (fragile against `AppShell`'s
    /// own layout constants), this renders the *same* app twice — once with
    /// the extension registered, once without — and requires the activity-bar
    /// column to differ. Only the extra icon can account for that: everything
    /// else in the strip, including which panel is active, is identical.
    #[test]
    fn extension_panel_contributes_an_activity_bar_icon() {
        /// Plain ASCII so the assertion doesn't depend on an installed font.
        const EXT_ICON: char = 'X';
        /// Comfortably inside the activity bar (3 line-heights wide) and
        /// below the title-bar band, so only the icon strip is sampled.
        const STRIP_W: i32 = 40;
        const STRIP_Y: std::ops::Range<i32> = 100..800;

        fn activity_bar_strip(with_ext: bool) -> Vec<(u8, u8, u8)> {
            let mut engine = Engine::new();
            engine.settings.use_nerd_fonts = false;
            engine.ext_panels.clear();
            if with_ext {
                engine.ext_panels.insert(
                    "git-insights".to_string(),
                    crate::core::plugin::PanelRegistration {
                        name: "git-insights".to_string(),
                        title: "Git Insights".to_string(),
                        icon: '\u{f113}',
                        fallback_icon: Some(EXT_ICON),
                        sections: Vec::new(),
                    },
                );
            }
            let mut h = harness(engine, 1400, 900);
            let mut px = Vec::new();
            for y in STRIP_Y.step_by(2) {
                for x in (0..STRIP_W).step_by(2) {
                    px.push(h.driver.pixel(x, y));
                }
            }
            px
        }

        let without = activity_bar_strip(false);
        let with = activity_bar_strip(true);
        let differing = with
            .iter()
            .zip(without.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing > 0,
            "registering an extension panel must change what the activity bar \
             paints — the Git Insights icon was missing entirely (#557); \
             {}/{} sampled pixels differed",
            differing,
            with.len()
        );
    }

    /// #727: a natively-expressible dialog (no `DialogTable`, no text
    /// input — `quit_unsaved`, the "Unsaved Changes" confirm, is exactly
    /// this shape) must be presented via a real `PlatformServices::
    /// show_message_dialog` call *exactly once* per open, unlike the
    /// in-canvas `Dialog` primitive, which is happily repainted every
    /// frame.
    ///
    /// `GtkDriver` paints into an in-memory Cairo surface and never opens a
    /// live GTK window — quadraui's own #666 doc says as much:
    /// "`GtkDriver` paints Cairo — it never sees a native `AlertDialog`
    /// window at all" — so an automated test structurally cannot observe
    /// the real alert appearing (that gap is exactly what `SMOKE_TESTS`
    /// covers). This module's own doc adds the other half: "`tick()` is
    /// never pumped by the driver", so `render_content`'s queued
    /// `pending_native_dialog` is never drained by a real blocking
    /// `show_message_dialog` call here — safe to assert on without risking
    /// a test that hangs forever with no display and no user to click it.
    ///
    /// What *is* fully in reach headlessly: `render_content`'s edge-trigger
    /// decision itself — repaint several frames with the same dialog still
    /// open and confirm the native present was queued exactly once, with
    /// the in-canvas draw suppressed on every one of those frames.
    ///
    /// The bookkeeping assertions (`dialog_layout.borrow().is_none()`,
    /// `native_dialog_shown`, `pending_native_dialog`) alone would stay
    /// green even if `render_content`'s native branch *also* kept calling
    /// `frame.draw(backend)` underneath — the #587/#592 "cache says one
    /// thing, paint does another" shape — so every state check below is
    /// paired with a `screen_contains` read of the *painted* surface: the
    /// in-canvas dialog's own title ("Unsaved Changes") and its
    /// "Save All & Quit" button label must be absent from the Cairo surface
    /// on every frame the native path is taken.
    ///
    /// RED-verified: with the `!self.native_dialog_shown.get()` guard
    /// dropped from `render_content` (so it always re-queues whenever
    /// `native_dialog_options` is `Some`), `pending_native_dialog.take()`
    /// returns `Some` on every repaint below instead of only the first —
    /// this test fails at the "must not re-queue" assertion in the loop.
    /// Restored before committing.
    #[test]
    fn native_dialog_presented_exactly_once_across_repeated_frames() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "unsaved change");
        engine.show_quit_confirm();
        let mut h = harness(engine, 1400, 900);

        // `harness()` already painted one frame (inside `GtkDriver::new`)
        // with the dialog open — that first paint must have suppressed the
        // in-canvas draw and queued exactly one native present.
        assert!(
            h.dialog_layout.borrow().is_none(),
            "a natively-expressible dialog must not paint in-canvas"
        );
        assert!(
            !h.driver.screen_contains("Unsaved Changes"),
            "the in-canvas dialog title must not be painted on the surface \
             while the native alert is in flight — a cleared \
             `dialog_layout` alone doesn't prove nothing was drawn"
        );
        assert!(
            !h.driver.screen_contains("Save All & Quit"),
            "the in-canvas dialog's button labels must not be painted on \
             the surface while the native alert is in flight"
        );
        assert!(
            h.native_dialog_shown.get(),
            "the edge-trigger flag must flip on the first paint of a \
             natively-expressible dialog"
        );
        assert!(
            h.pending_native_dialog.take().is_some(),
            "the first paint of a natively-expressible dialog must queue \
             exactly one native present"
        );

        // Several more frames with the *same* dialog still open: must
        // never re-queue a second present, and must keep suppressing the
        // in-canvas draw — both the cache and the actual painted surface.
        for i in 0..5 {
            h.driver.render();
            assert!(
                h.dialog_layout.borrow().is_none(),
                "frame {i}: in-canvas draw must stay suppressed while the \
                 native dialog is in flight"
            );
            assert!(
                !h.driver.screen_contains("Unsaved Changes"),
                "frame {i}: the in-canvas dialog title must stay off the \
                 painted surface while the native alert is in flight"
            );
            assert!(
                h.pending_native_dialog.take().is_none(),
                "frame {i}: must not re-queue a native present while the \
                 same dialog stays open"
            );
        }
    }

    /// #823 item 4: `App::show_quit_confirm` used to restate
    /// `Engine::show_quit_confirm`'s dialog body inline (byte-identical
    /// `DialogButton` literals, `core/engine/panels.rs`) instead of calling
    /// it. Reached here through the real menu path — clicking File > Quit
    /// with unsaved changes — rather than calling either method directly,
    /// so a regression in the collapse (the App method silently not
    /// calling the engine one, or calling some other dialog) shows up the
    /// same way a user's click would, unlike the sibling test above (which
    /// opens the identical dialog via `engine.show_quit_confirm()` directly
    /// and would stay green even if `App::show_quit_confirm` called
    /// nothing at all).
    ///
    /// The dialog is natively-expressible (#727), so — same as the sibling
    /// test above — it never paints in-canvas and `screen_contains
    /// ("Unsaved Changes")` cannot be the proof; `native_dialog_shown` /
    /// `pending_native_dialog` are the same two signals reused here.
    ///
    /// RED-verified: with `App::show_quit_confirm`'s
    /// `self.engine.borrow_mut().show_quit_confirm()` call replaced by a
    /// no-op, this test fails (no native dialog is ever queued); restored
    /// before committing.
    #[test]
    fn menu_quit_with_unsaved_changes_opens_confirm_dialog() {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "unsaved change");
        // `buffer_mut().insert` edits the rope directly and does not flip
        // `BufferState::dirty` (that's set by the engine's own edit
        // commands) — `has_any_unsaved()` (what `handle_menu_action`'s
        // "quit_menu" arm gates on) reads `dirty` directly, so the fixture
        // must set it explicitly or the click below silently takes the
        // "nothing unsaved" branch instead.
        let id = engine.active_buffer_id();
        if let Some(buf) = engine.buffer_manager.get_mut(id) {
            buf.dirty = true;
        }
        let mut h = harness(engine, 1200, 800);
        h.driver.render();

        let file = h
            .driver
            .find_bounds("File")
            .expect("the File menu-bar header must paint");
        h.driver
            .click(file.x + file.width / 2.0, file.y + file.height / 2.0);
        h.driver.render();

        let quit = h
            .driver
            .find_bounds("Quit")
            .expect("clicking File must open its dropdown, showing Quit");
        h.driver
            .click(quit.x + quit.width / 2.0, quit.y + quit.height / 2.0);
        h.driver.render();

        assert!(
            h.native_dialog_shown.get(),
            "File > Quit with unsaved changes must open the quit-confirm \
             dialog (the edge-trigger flag must flip)"
        );
        assert!(
            h.pending_native_dialog.take().is_some(),
            "File > Quit with unsaved changes must queue the native \
             quit-confirm present"
        );
    }

    /// #823 item 4 (close-tab half): `App::show_close_tab_confirm` used to
    /// restate `Engine::show_close_tab_confirm`'s dialog body inline
    /// (byte-identical `DialogButton` literals, `core/engine/panels.rs`)
    /// instead of calling it. Reached through a real click on a dirty tab's
    /// × button — `handle_mouse_click`'s `Some(true)` ("close-tab on dirty
    /// buffer") result, `gtk/click.rs` — the same path
    /// `single_group_tab_close_button_closes_that_tab` above exercises for
    /// a *clean* tab (which closes immediately, no dialog); this is its
    /// dirty-tab sibling.
    ///
    /// Same reasoning as the quit-confirm test above applies to the
    /// assertions: `close_tab_confirm` is equally natively-expressible
    /// (#727: no `DialogTable`, no text input), so `native_dialog_shown` /
    /// `pending_native_dialog` are the proof, not `screen_contains`.
    ///
    /// RED-verified: with `App::show_close_tab_confirm`'s
    /// `self.engine.borrow_mut().show_close_tab_confirm()` call replaced by
    /// a no-op, this test fails (no native dialog is ever queued);
    /// restored before committing.
    #[test]
    fn close_dirty_tab_button_opens_confirm_dialog() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "alpha");
        // Captured before `new_tab` below switches the active buffer —
        // this is tab 0's buffer, the one whose × this test clicks.
        let buf0 = engine.active_buffer_id();
        engine.new_tab(None);
        engine.new_tab(None);
        if let Some(buf) = engine.buffer_manager.get_mut(buf0) {
            buf.dirty = true;
        }
        let mut h = harness(engine, 1400, 900);

        let (x, y) = h
            .driver
            .tab_close_center(&editor_tab_bar_id(), 0)
            .expect("the single-group tab bar must have painted tab 0's close button");
        h.driver.click(x, y);
        h.driver.render();

        assert!(
            h.native_dialog_shown.get(),
            "clicking \u{d7} on a dirty tab must open the close-tab-confirm \
             dialog (the edge-trigger flag must flip)"
        );
        assert!(
            h.pending_native_dialog.take().is_some(),
            "clicking \u{d7} on a dirty tab must queue the native \
             close-tab-confirm present"
        );
    }

    /// #727's native path only covers dialogs `quadraui::native_dialog_options`
    /// reports as natively expressible — a dialog carrying a text input
    /// (e.g. the move-file destination prompt) is not, and must keep
    /// rendering exactly as it did before this issue: in-canvas, with no
    /// native present ever queued.
    ///
    /// As with the sibling test above, the `dialog_layout.borrow().is_some()`
    /// bookkeeping check alone would stay green even if nothing had actually
    /// reached the Cairo surface, so this also reads the painted content
    /// directly: the dialog's title and its body prompt must both be
    /// on-screen. (The text-input *field* itself paints only the live value
    /// plus a cursor glyph — `DialogInputPanel::display`, `render.rs` — with
    /// no separate "Destination:" label text to assert on, so the body
    /// line `start_move_file_dialog` sets — "Enter destination path:" — is
    /// the stand-in proof that the input dialog reached the surface.)
    ///
    /// RED-verified: forcing this dialog down the native branch (making
    /// `native_dialog_options` return `Some` unconditionally, ignoring the
    /// text input) makes `dialog_layout.borrow().is_some()` false and both
    /// `screen_contains` calls below false — this test fails at the layout
    /// assertion first. Restored before committing.
    #[test]
    fn text_input_dialog_stays_in_canvas_not_native() {
        let mut engine = Engine::new();
        engine.start_move_file_dialog(
            std::path::Path::new("/tmp/project/foo.rs"),
            std::path::Path::new("/tmp/project"),
        );
        let h = harness(engine, 1400, 900);

        assert!(
            h.dialog_layout.borrow().is_some(),
            "a dialog carrying a text input must still paint in-canvas"
        );
        assert!(
            h.driver.screen_contains("Move 'foo.rs'"),
            "the in-canvas dialog's title must actually be painted on the \
             surface, not just recorded in the layout cache"
        );
        assert!(
            h.driver.screen_contains("Enter destination path:"),
            "the in-canvas dialog's body prompt must actually be painted \
             on the surface"
        );
        assert!(
            h.pending_native_dialog.take().is_none(),
            "a text-input dialog must never be queued for a native present"
        );
    }
}

#[cfg(test)]
mod sidebar_panel_clicks {
    //! #544 (#448-D): under the ShellApp runner every sidebar panel except the
    //! file explorer silently dropped mouse input.
    //!
    //! The Relm4 build gave each panel its own `DrawingArea` and GDK delivered
    //! clicks straight to it. The #540 migration collapsed all of them into the
    //! runner's single surface, so panels are now painted by
    //! `render_content` into `layout.sidebar_content_bounds` and every event
    //! arrives at `ShellApp::handle`. Only the explorer was re-wired there;
    //! search / git / debug / extensions / settings presses fell through to the
    //! editor click path, matched no editor zone, and were discarded — hence
    //! "settings panel clicks, search sidebar clicks, git sidebar clicks" all
    //! reported dead.
    //!
    //! These drive the real `App` through `GtkDriver`, so they exercise
    //! `ShellApp::handle`'s routing and the controllers that painted the panel,
    //! and every click is aimed at the rect the frame *actually* painted
    //! (`painted_sidebar_bounds`) rather than a guessed offset.
    use super::*;
    use crate::core::engine::sidebar::*;
    use quadraui::{Point, ScrollDelta, UiEvent};

    /// A harness showing `panel` in the sidebar. `show_panel` is used directly
    /// rather than `focus_sidebar_panel`/`toggle_sidebar_panel` because those
    /// persist `session.explorer_visible` to the developer's real session file.
    /// Nerd fonts are pinned off so nothing depends on the test machine having
    /// a Nerd Font installed.
    fn panel_harness(panel: &str) -> Harness<impl AppLogic> {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.app_shell.show_panel(&quadraui::WidgetId::new(panel));
        harness(engine, 1400, 900)
    }

    /// Settings: a click on a category row must expand/collapse it.
    ///
    /// The pre-fix path reached the since-retired `Msg::SettingsClick` arm, whose geometry was read
    /// off `settings_da_ref` — a `DrawingArea` that is `None` for the whole
    /// life of a ShellApp run, so panel width/height came back `0` and every
    /// row test failed even when the message was dispatched. Nothing dispatched
    /// it either. Now the press goes to the same `FormController` that painted
    /// the rows.
    #[test]
    fn settings_panel_click_toggles_the_clicked_category() {
        let mut h = panel_harness(PANEL_SETTINGS);
        let sb = h
            .painted_sidebar_bounds
            .get()
            .expect("the settings panel must have painted into a sidebar rect");
        assert!(
            matches!(
                h.engine.borrow().settings_flat_list().first(),
                Some(crate::core::engine::SettingsRow::CoreCategory(0))
            ),
            "this test aims at the first row expecting it to be category 0"
        );
        let before = h.engine.borrow().settings_collapsed[0];
        // #677 audit: label of the first setting row under category 0 --
        // `settings_collapsed[cat_idx]` gates whether `settings_flat_list`
        // includes any `CoreSetting` rows for that category
        // (`Engine::settings_flat_list`), so this row's label is painted iff
        // the category is expanded. Used below to prove the click changes
        // what's on screen, not just the `settings_collapsed` flag.
        let first_setting_label = match h.engine.borrow().settings_flat_list().get(1) {
            Some(crate::core::engine::SettingsRow::CoreSetting(idx)) => {
                crate::core::settings::SETTING_DEFS[*idx].label
            }
            other => panic!("expected category 0's first setting row, got {other:?}"),
        };
        assert_eq!(
            h.driver.screen_contains(first_setting_label),
            !before,
            "sanity: the first setting's label must be painted iff its category \
             starts expanded"
        );

        h.driver.click(sb.x + 20.0, sb.y + 4.0);

        assert_eq!(
            h.engine.borrow().settings_collapsed[0],
            !before,
            "clicking the first settings category must toggle it (#544)"
        );
        assert_eq!(
            h.engine.borrow().settings_selected,
            0,
            "the clicked row must also become the selection"
        );
        // #677 audit: the two asserts above were the whole test before this
        // audit -- pure `settings_collapsed`/`settings_selected` state
        // checks with no confirmation the panel actually repainted.
        // Verified vacuous by mutation: hardcoding `settings_flat_list`'s
        // `collapsed` local to `false` (so painting always shows every row
        // regardless of `settings_collapsed`) left both asserts above green
        // and only this one red.
        assert_eq!(
            h.driver.screen_contains(first_setting_label),
            before,
            "clicking the category must actually repaint the row list -- the \
             first setting's label must now be painted iff the category is \
             still expanded (i.e. the opposite of the pre-click state)"
        );
    }

    /// Settings: the wheel must scroll the panel, not the editor behind it.
    #[test]
    fn settings_panel_scrolls_under_the_wheel() {
        let mut h = panel_harness(PANEL_SETTINGS);
        let sb = h.painted_sidebar_bounds.get().unwrap();
        assert_eq!(h.engine.borrow().settings_scroll_top, 0);

        h.driver.dispatch(UiEvent::Scroll {
            widget: None,
            // Negative y = wheel down in quadraui's convention.
            delta: ScrollDelta::new(0.0, -1.0),
            position: Point::new(sb.x + 20.0, sb.y + 100.0),
        });

        assert!(
            h.engine.borrow().settings_scroll_top > 0,
            "a wheel notch over the settings panel must scroll it (#544)"
        );
    }

    /// Search: clicking the query box at the top of the panel must focus it.
    ///
    /// `search_panel_form_focus` is what the renderer reads to draw the caret
    /// and what keystrokes are routed by, so a `None` here is the "typing goes
    /// nowhere after clicking the search box" half of the report.
    #[test]
    fn search_panel_click_focuses_the_query_field() {
        let mut h = panel_harness(PANEL_SEARCH);
        let sb = h.painted_sidebar_bounds.get().unwrap();
        // Clear the panel's default focus so the assertion below can only pass
        // because the click put it back.
        h.engine.borrow().search_panel_form_focus.replace(None);
        h.engine.borrow_mut().search_set_focus(false);

        h.driver.click(sb.x + 20.0, sb.y + 4.0);

        assert_eq!(
            h.engine
                .borrow()
                .search_panel_form_focus
                .borrow()
                .as_deref(),
            Some("search:query"),
            "clicking the search panel's query field must focus it (#544)"
        );
        assert!(
            h.engine.borrow().search_has_focus,
            "and the panel itself must take focus"
        );
    }

    /// Git: the commit-message box must activate when clicked, and the header
    /// row above it must not.
    ///
    /// This is the band geometry (`render::sc_sidebar_bands`) the painter and
    /// the router now share. The pre-fix handler assumed `DrawingArea`-local
    /// coordinates with the panel top at `y == 0`, which the ShellApp painter
    /// never produces — under the sidebar's real origin every band test landed
    /// in the wrong band even if the event had reached it.
    ///
    /// Skipped when the checkout isn't a git repo: without a `SourceControl`
    /// screen the panel paints nothing at all and there are no bands to hit.
    #[test]
    fn git_panel_click_activates_the_commit_box_but_not_the_header() {
        let mut h = panel_harness(PANEL_GIT);
        if h.engine.borrow().sc_panel_layout.borrow().is_none() {
            return;
        }
        let sb = h.painted_sidebar_bounds.get().unwrap();
        let lh = h.painted_line_height.get().unwrap() as f32;
        let bands = crate::render::sc_sidebar_bands(
            &h.engine.borrow().sc_commit_message.clone(),
            sb,
            lh,
            super::super::SC_COMMIT_BORDER_PX,
        );

        h.driver.click(
            bands.commit_input.x + 20.0,
            bands.commit_input.y + bands.commit_input.height / 2.0,
        );
        assert!(
            h.engine.borrow().sc_commit_input_active,
            "clicking the commit-message box must put the caret in it (#544)"
        );

        h.driver.click(
            bands.header.x + 20.0,
            bands.header.y + bands.header.height / 2.0,
        );
        assert!(
            !h.engine.borrow().sc_commit_input_active,
            "clicking the header row above it must take the caret back out"
        );
    }

    // ── #754 (mouse ladder slice 4: panels) ────────────────────────────────

    /// GTK half of `bottom_panel_tab_strip_click_switches_the_painted_panel_
    /// via_shell_app`: the shared tab strip must switch which panel is
    /// **painted** here too, through `render::route_bottom_panel_click` ->
    /// `render::apply_bottom_panel_route`.
    ///
    /// The click is aimed at the geometry the frame actually painted — the
    /// `slot_positions` `draw_tab_bar` returned into
    /// `engine.bottom_tab_bar_hits`, and the tab-strip row from
    /// `engine.bottom_panel_geometry` — never a guessed offset, so a strip
    /// that paints somewhere else fails rather than passing by luck.
    #[test]
    fn bottom_panel_tab_strip_click_switches_the_painted_panel() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.terminal_new_tab(80, 10);
        engine
            .dap_output_lines
            .push("ZQXW754GTKDEBUGMARKER".to_string());

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        assert!(
            !h.driver.screen_contains("ZQXW754GTKDEBUGMARKER"),
            "precondition: the Terminal tab owns the panel body"
        );

        // Locate the *second* painted tab slot (Terminal, then Debug Output).
        let (slot_x, strip_y) = {
            let engine = h.engine.borrow();
            let hits = engine.bottom_tab_bar_hits.borrow();
            let hits = hits
                .as_ref()
                .expect("the bottom panel must have painted a tab strip");
            let &(sx, ex) = hits
                .slot_positions
                .get(1)
                .expect("Terminal + Debug Output are two painted slots");
            let geom = engine
                .bottom_panel_geometry
                .borrow()
                .expect("the bottom panel must have painted");
            ((sx + ex) / 2.0, geom.top_y + geom.toolbar_y / 2.0)
        };
        h.driver.click(slot_x as f32, strip_y as f32);
        h.driver.render();

        assert!(
            h.driver.screen_contains("ZQXW754GTKDEBUGMARKER"),
            "clicking the Debug Output tab must repaint the panel body with the \
             debug output (#754 `BottomPanelRoute::TabBar`)"
        );
    }

    // ── #758 / #734 slice 3: the shared terminal (PTY) keyboard rung ───────

    /// GTK half of `terminal_ctrl_f_opens_the_painted_find_bar_via_shell_app`
    /// (`tui_main/shell_app.rs`): with the terminal focused, Ctrl+F must open
    /// the *terminal's* find bar, and subsequent characters must land in that
    /// bar's query — through `App::handle` -> `handle_key_press` ->
    /// `render::route_terminal_key` -> `Engine::handle_terminal_key`.
    ///
    /// GTK had **no terminal keyboard rung at all** between #540 and #758:
    /// the `if engine.borrow().terminal_has_focus { … }` block lived in the
    /// Relm4 `view!`'s `EventControllerKey` closure and was deleted with it,
    /// so Ctrl+F opened the editor's find/replace overlay and every other key
    /// ran a vim command on the buffer while the user was looking at a shell
    /// prompt (#471).
    ///
    /// Asserts on rendered output (`CLAUDE.md` rule 1): the terminal
    /// toolbar's painted `" FIND: …"` text (`render::build_terminal_toolbar`,
    /// drawn by `draw_terminal_panel`), never `terminal_find_active`.
    ///
    /// **Verified RED against unfixed `develop`:** without the
    /// `render::route_terminal_key` call, Ctrl+F falls through to
    /// `Engine::handle_key` and no `"FIND:"` ever paints — the second
    /// assertion fires.
    #[test]
    fn terminal_ctrl_f_opens_the_painted_find_bar() {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = false;
        // `terminal_new_tab` opens the panel and focuses it.
        engine.terminal_new_tab(80, 10);

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        assert!(
            !h.driver.screen_contains("FIND:"),
            "precondition: the terminal toolbar starts as a tab strip; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.ctrl_char('f');
        h.driver.render();
        assert!(
            h.driver.screen_contains("FIND:"),
            "Ctrl+F with the terminal focused must open the terminal find bar, \
             not the editor find/replace overlay; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.type_char('z');
        h.driver.render();
        assert!(
            h.driver.screen_contains("FIND: z"),
            "characters typed after Ctrl+F must reach the terminal find query \
             through the shared router; painted: {:?}",
            h.driver.painted_texts()
        );
    }

    /// A focused terminal must swallow ordinary keys so they never reach the
    /// editor buffer. This is the user-visible shape of the missing GTK rung:
    /// typing `x` at a shell prompt deleted a character from the *file*.
    ///
    /// Asserts on the painted buffer text, with a positive control — the same
    /// key on the same fixture with the terminal unfocused must delete the
    /// character — so a fixture whose text could not change would fail.
    ///
    /// **Verified RED against unfixed `develop`:** without the router call
    /// the first `x` reaches `Engine::handle_key`, the painted line drops to
    /// `QXWTERMGTK758` immediately, and the second assertion fires.
    #[test]
    fn focused_terminal_swallows_editor_keys_on_gtk() {
        let build = |focused: bool| {
            let mut engine = Engine::new_for_test();
            engine.settings.use_nerd_fonts = false;
            engine.buffer_mut().insert(0, "ZQXWTERMGTK758\n");
            engine.terminal_new_tab(80, 6);
            engine.terminal_has_focus = focused;
            harness(engine, 1400, 900)
        };

        let mut h = build(true);
        h.driver.render();
        assert!(
            h.driver.screen_contains("ZQXWTERMGTK758"),
            "precondition: the buffer line must paint; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.type_char('x');
        h.driver.render();
        assert!(
            h.driver.screen_contains("ZQXWTERMGTK758"),
            "`x` with the terminal focused must go to the PTY, not delete a \
             character from the editor buffer; painted: {:?}",
            h.driver.painted_texts()
        );

        let mut control = build(false);
        control.driver.render();
        control.driver.type_char('x');
        control.driver.render();
        assert!(
            control.driver.screen_contains("QXWTERMGTK758")
                && !control.driver.screen_contains("ZQXWTERMGTK758"),
            "control: with the terminal unfocused `x` must delete the first \
             character; painted: {:?}",
            control.driver.painted_texts()
        );
    }

    /// The sidebar hover rung must exist **on this backend at all**.
    ///
    /// Before #754 the Source Control toolbar's hover highlight was driven by
    /// ~78 lines that ran only in `tui_main/mouse.rs`: GTK painted
    /// `SourceControlData::button_hovered` faithfully (`draw_sc_sidebar_panel`
    /// passes it to `Backend::draw_sidebar_panel` as `hovered_id`) but nothing
    /// on this side ever set it, so the highlight could not appear no matter
    /// where the pointer went. That paint-without-input asymmetry is the
    /// mechanism behind #499/#484.
    ///
    /// Asserts on **rendered pixels** (`CLAUDE.md` rule 1) — the button's own
    /// painted band before and after the pointer arrives. Asserting
    /// `sc_button_hovered == Some(_)` would pass against a backend that sets
    /// the field and never repaints, which is precisely the failure this rung
    /// is fixing in the other direction.
    ///
    /// Skipped when the checkout isn't a git repo: with no `SourceControl`
    /// screen the panel paints no toolbar and there is no button to hover.
    #[test]
    fn source_control_toolbar_button_highlights_on_hover() {
        let mut h = panel_harness(PANEL_GIT);
        h.driver.render();
        let sb = match h.painted_sidebar_bounds.get() {
            Some(sb) if h.engine.borrow().sc_panel_layout.borrow().is_some() => sb,
            _ => return,
        };

        // Locate a toolbar button from the layout the frame painted — its own
        // `bounds`, not a scan or a guess. The hover highlight is a rounded
        // rect inset 2px inside those bounds, so probe the centre.
        let button = {
            let engine = h.engine.borrow();
            let layout = engine.sc_panel_layout.borrow();
            layout
                .as_ref()
                .and_then(|l| l.toolbar_layout.as_ref())
                .and_then(|t| t.visible_items.iter().find(|i| i.clickable).cloned())
        };
        let Some(button) = button else {
            return; // no clickable toolbar buttons painted in this repo state
        };
        let (bx, by) = (
            button.bounds.x + button.bounds.width / 2.0,
            button.bounds.y + button.bounds.height / 2.0,
        );

        let sample = |h: &mut Harness<_>| -> Vec<(u8, u8, u8)> {
            let x0 = (button.bounds.x + 3.0) as i32;
            let x1 = (button.bounds.x + button.bounds.width - 3.0) as i32;
            (x0..x1).map(|x| h.driver.pixel(x, by as i32)).collect()
        };
        let before = sample(&mut h);

        h.driver.mouse_move(bx, by);
        h.driver.render();
        let after = sample(&mut h);

        assert_ne!(
            before, after,
            "moving the pointer onto a Source Control toolbar button must repaint \
             it hovered — GTK painted `button_hovered` but nothing set it before \
             #754 made `render::route_sidebar_hover` shared"
        );
    }

    /// #823 item 8: `Engine::clear_sidebar_focus` used to set
    /// `sc_has_focus = false` directly instead of calling
    /// `Engine::sc_set_focus(false)` — skipping that method's other two
    /// effects, one of which is clearing `sc_button_focused`. `sc_button_focused`
    /// drives `draw_sc_sidebar_panel`'s `pressed` highlight
    /// (`render.rs`: `let pressed = sc.button_focused.and_then(Engine::sc_button_id);`)
    /// independently of `sc_has_focus`, so the bug was real and visible: a
    /// Source Control toolbar button left "pressed" (e.g. via keyboard nav)
    /// stayed highlighted even after focus moved to the editor.
    ///
    /// Reuses `source_control_toolbar_button_highlights_on_hover`'s
    /// pixel-diff technique just above (same reasoning: asserting
    /// `sc_button_focused.is_none()` would pass even if nothing ever
    /// repainted).
    ///
    /// RED-verified: with `Engine::clear_sidebar_focus`'s `self.sc_set_focus
    /// (false)` reverted to a direct `self.sc_has_focus = false`, this test
    /// fails (the pressed highlight survives the editor click); restored
    /// before committing.
    #[test]
    fn clicking_editor_clears_a_pressed_sc_toolbar_button_highlight() {
        let mut h = panel_harness(PANEL_GIT);
        h.driver.render();
        match h.painted_sidebar_bounds.get() {
            Some(_) if h.engine.borrow().sc_panel_layout.borrow().is_some() => {}
            _ => return,
        };

        let button = {
            let engine = h.engine.borrow();
            let layout = engine.sc_panel_layout.borrow();
            layout
                .as_ref()
                .and_then(|l| l.toolbar_layout.as_ref())
                .and_then(|t| t.visible_items.iter().find(|i| i.clickable).cloned())
        };
        let Some(button) = button else {
            return; // no clickable toolbar buttons painted in this repo state
        };
        let by = button.bounds.y + button.bounds.height / 2.0;
        let sample = |h: &mut Harness<_>| -> Vec<(u8, u8, u8)> {
            let x0 = (button.bounds.x + 3.0) as i32;
            let x1 = (button.bounds.x + button.bounds.width - 3.0) as i32;
            (x0..x1).map(|x| h.driver.pixel(x, by as i32)).collect()
        };
        let baseline = sample(&mut h);

        // Arm: this button "pressed"/focused, as keyboard nav within the
        // panel would leave it (`source_control.rs`'s Tab handling sets
        // exactly this field).
        h.engine.borrow_mut().sc_has_focus = true;
        h.engine.borrow_mut().sc_button_focused = Some(button.item_idx);
        h.driver.render();
        let pressed = sample(&mut h);
        assert_ne!(
            baseline, pressed,
            "test setup sanity: sc_button_focused must actually repaint a \
             pressed highlight, or this test cannot tell a fixed bug from a \
             no-op one"
        );

        // Click into the editor pane — the same "clicking the editor clears
        // every sidebar's keyboard focus" rung `clear_sidebar_focus`'s own
        // doc comment describes.
        let win = h.engine.borrow().active_window_id();
        let (ex, ey) = h
            .window_center(win)
            .expect("the editor pane must have painted a window rect");
        h.driver.click(ex, ey);
        h.driver.render();
        let after = sample(&mut h);

        assert_eq!(
            after, baseline,
            "clicking the editor must clear the Source Control toolbar's \
             pressed/focused button highlight all the way back to baseline \
             (Engine::clear_sidebar_focus must fully clear sc_button_focused \
             via sc_set_focus, not just sc_has_focus)"
        );
    }

    /// #637/#754: switching to a plugin ("extension") panel from the
    /// activity bar must clear focus flags a previously-visited panel left
    /// set, on **this** backend too.
    ///
    /// `render::apply_activity_panel_switch`'s own doc comment: "#637's
    /// focus clear was TUI-only" — TUI's `App::switch_panel` twin
    /// (`mouse.rs`'s `ActivityBarTarget` match) called
    /// `engine.clear_sidebar_focus()` before showing a plugin panel; GTK's
    /// pre-#754 `switch_panel` never did, so a stale `ext_sidebar_has_focus`
    /// left by an earlier visit to the Extensions *marketplace* panel
    /// stayed stuck `true` after switching to an unrelated plugin panel.
    /// TUI's regression test for the mirror-image bug is
    /// `plugin_ext_panel_wins_focus_and_clicks_after_marketplace_visit`
    /// (`tui_main/shell_app.rs`) — this is the GTK half.
    ///
    /// Drives a **real activity-bar click** through `GtkDriver` —
    /// `AppShellEvent::PanelChanged` -> `App::switch_panel` ->
    /// `render::apply_activity_panel_switch` — aimed at the exact row
    /// `quadraui::gtk::ACTIVITY_ROW_PX` (the fixed per-icon height both the
    /// painter and the runner's own hit-test use) puts the newly-registered
    /// ext panel at. GTK's activity bar has no menu-toggle row (unlike
    /// TUI's optional menu bar, GTK's is always the CSD title bar), so the
    /// six fixed top-pinned items — explorer, search, debug, git,
    /// extensions, ai (`sidebar::FIXED_ACTIVITY_PANEL_IDS`, the same shared
    /// order both backends' shell config builds from) — occupy indices 0-5
    /// and the ext panel lands at index 6; found empirically with a probe
    /// harness clicking each row and checking `ext_panel_active`, since
    /// this backend has no cached hit-region equivalent to
    /// `bottom_tab_bar_hits` to read the geometry from directly.
    #[test]
    fn switching_to_a_plugin_panel_clears_stale_marketplace_focus() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.ext_panels.clear();
        engine.ext_panels.insert(
            "git-insights".to_string(),
            crate::core::plugin::PanelRegistration {
                name: "git-insights".to_string(),
                title: "Git Insights".to_string(),
                icon: '\u{f113}',
                fallback_icon: Some('X'),
                sections: Vec::new(),
            },
        );
        // Visit the Extensions marketplace panel first, as a user would
        // before ever opening a plugin panel this session.
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXTENSIONS));
        engine.ext_sidebar_has_focus = true;

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        let ab_top = (h.menu_row_rect.get().y + h.menu_row_rect.get().height) as f32;

        // Click the plugin panel's activity-bar icon (index 6 — see doc
        // comment above).
        let y = ab_top + (6.0 + 0.5) * quadraui::gtk::ACTIVITY_ROW_PX as f32;
        h.driver.click(20.0, y);

        assert_eq!(
            h.engine.borrow().ext_panel_active.as_deref(),
            Some("git-insights"),
            "precondition: the click must have actually switched to the \
             plugin panel — if this fails, the click missed the icon"
        );
        assert!(
            !h.engine.borrow().ext_sidebar_has_focus,
            "switching to a plugin panel must clear the Extensions \
             marketplace's stale ext_sidebar_has_focus (#637's GTK gap — \
             apply_activity_panel_switch now calls clear_sidebar_focus() \
             on both backends)"
        );
        assert!(
            h.engine.borrow().ext_panel_has_focus,
            "the plugin panel itself must take focus"
        );
    }

    /// Debug: a press in the panel body must reach the panel at all — before
    /// #544 it was swallowed by the editor click path, which left
    /// `dap_sidebar_has_focus` false so every subsequent keystroke went to the
    /// buffer instead of the debug tree.
    #[test]
    fn debug_panel_click_gives_the_panel_focus() {
        let mut h = panel_harness(PANEL_DEBUG);
        let body = h.engine.borrow().dap_sidebar_body_rect.get();
        assert!(body.width > 0.0, "the debug panel must have painted a body");
        assert!(!h.engine.borrow().dap_sidebar_has_focus);

        h.driver.click(body.x + 20.0, body.y + 4.0);

        assert!(
            h.engine.borrow().dap_sidebar_has_focus,
            "a click in the debug panel body must focus it (#544)"
        );
    }

    /// An editor text-selection drag that wanders over the sidebar must still
    /// finalise in the editor: only a press that a panel *claimed* captures the
    /// rest of the gesture. Guards the `sidebar_pointer_captured` follow-through
    /// added for panel scrollbar drags from swallowing unrelated releases.
    #[test]
    fn an_editor_drag_crossing_the_sidebar_is_not_stolen_by_a_panel() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.buffer_mut().insert(
            0,
            "alpha beta gamma
second line here
",
        );
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        let (wx, wy) = h.window_center(win).expect("editor pane must paint");
        let sb = h.painted_sidebar_bounds.get().unwrap();

        h.driver.mouse_down(wx, wy);
        h.driver.mouse_move(sb.x + 10.0, sb.y + 10.0);
        h.driver.mouse_up(sb.x + 10.0, sb.y + 10.0);

        assert!(
            !h.engine.borrow().explorer_has_focus,
            "the explorer must not claim a drag that started in the editor (#544)"
        );
    }

    /// #756 acceptance, GTK half of the drag rung: an editor text-selection
    /// drag must still paint a selection now that
    /// `render::route_mouse_drag` — not `handle_mouse_drag_msg`'s own
    /// hand-ordered ladder — decides that the point belongs to the editor.
    ///
    /// Asserted on painted pixels (`CLAUDE.md` testing rule 1): the cell the
    /// drag sweeps over must change background between before and after, which
    /// is only true if the `MouseDragRoute::EditorText` arm actually reached
    /// `handle_mouse_drag`. Confirmed RED by disabling that arm
    /// (`EditorText if false =>`, so the route falls to the no-op group): the
    /// two probes then both read `(39, 39, 43)`.
    #[test]
    fn an_editor_text_drag_paints_a_selection_through_the_shared_drag_router() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        let mut text = String::new();
        for _ in 0..60 {
            text.push_str("alpha beta gamma delta epsilon\n");
        }
        engine.buffer_mut().insert(0, &text);
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        h.window_center(win).expect("editor pane must paint");
        let cw = h.painted_char_width();
        let lh = h
            .painted_line_height()
            .expect("the frame must publish the line height it painted with");
        assert!(
            cw > 0.0,
            "the frame must publish the char width it painted with"
        );

        // Anchor inside the *text*, not at the pane's centre: every line in
        // the fixture is 30 characters, so a pane-centre press would land past
        // end-of-line on both ends of the sweep and select nothing.
        let (text_x, row_y) = {
            let layout = h.screen_layout.borrow();
            let rw = layout
                .as_ref()
                .expect("a frame must have painted")
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active pane must be in the layout");
            (
                rw.rect.x + rw.gutter_char_width as f64 * cw,
                rw.rect.y + lh * 2.5,
            )
        };
        let probe = ((text_x + cw * 3.5) as i32, row_y as i32);
        // Park the caret on the probe's row *first*, so the `before` sample
        // already includes the cursor-line highlight and the only thing left
        // for the gesture below to change is the selection itself.
        h.driver.click((text_x + cw * 0.5) as f32, row_y as f32);
        let before = h.driver.pixel(probe.0, probe.1);

        // Press to the left of the probe and sweep past it while held — the
        // press anchors the selection, the *move* is the rung under test.
        h.driver
            .mouse_down((text_x + cw * 0.5) as f32, row_y as f32);
        h.driver
            .mouse_move((text_x + cw * 8.5) as f32, row_y as f32);
        h.driver.mouse_up((text_x + cw * 8.5) as f32, row_y as f32);

        let after = h.driver.pixel(probe.0, probe.1);
        assert_ne!(
            before, after,
            "a held drag across the editor text must repaint the swept cell \
             with the selection background; both probes read {before:?} at \
             {probe:?}"
        );
    }

    /// The mirror image of the test above: a drag that *starts* inside the
    /// sidebar must keep its grab for the rest of the gesture even once the
    /// pointer wanders out over the editor — a panel scrollbar-thumb or tree
    /// row drag must not hand off to the editor's own drag handling the
    /// instant the cursor crosses the sidebar's right edge. Exercises the
    /// `dragging` branch of `try_route_sidebar_mouse_event` (the "captured but
    /// out of bounds" path the ordinary not-dragging fallthrough never
    /// reaches), unlike the test above where the press starts outside the
    /// sidebar and `sidebar_pointer_captured` is never set.
    ///
    /// Uses the debug panel rather than the explorer: `route_debug_sidebar_event`
    /// sets `dap_sidebar_has_focus` on *any* press inside the body rect
    /// unconditionally (see `debug_panel_click_gives_the_panel_focus` above),
    /// so the assertions here don't depend on a real DAP session or tree rows
    /// existing — only on the routing/capture plumbing under test.
    #[test]
    fn a_sidebar_drag_keeps_its_grab_once_it_crosses_into_the_editor() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.buffer_mut().insert(
            0,
            "alpha beta gamma
second line here
",
        );
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_DEBUG));
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        let (wx, wy) = h.window_center(win).expect("editor pane must paint");
        let body = h.engine.borrow().dap_sidebar_body_rect.get();
        assert!(body.width > 0.0, "the debug panel must have painted a body");
        let cursor_before = *h.engine.borrow().cursor();

        h.driver.mouse_down(body.x + 20.0, body.y + 4.0);
        assert!(
            h.engine.borrow().dap_sidebar_has_focus,
            "a press inside the sidebar must focus the debug panel"
        );

        // Follow the gesture out into the editor pane while the button is
        // still held — this is the same move/up pair the test above uses,
        // just starting from inside the sidebar instead of outside it.
        h.driver.mouse_move(wx, wy);
        h.driver.mouse_up(wx, wy);

        assert_eq!(
            *h.engine.borrow().cursor(),
            cursor_before,
            "a captured sidebar drag must never reach the editor's own click \
             path, so the buffer cursor must not move even though the \
             release lands on top of the editor pane (#544)"
        );
    }

    /// AI: selecting the panel must paint its content through the adopted
    /// `quadraui::ChatController` (#819, replacing the hand-painted
    /// `draw_ai_sidebar_panel`) — `ChatController::render`'s status strip is
    /// this panel's header row.
    ///
    /// Asserts on the *rendered* header text via `screen_contains`, never on
    /// engine state alone — `ScreenLayout.picker` sat populated on GTK for
    /// months while nothing painted it (CLAUDE.md rule 1 / #587).
    #[test]
    fn ai_panel_paints_its_header() {
        let h = panel_harness(PANEL_AI);
        assert!(
            h.driver.screen_contains("AI ASSISTANT"),
            "selecting the AI panel must paint its header (#730/#819)"
        );
    }

    /// AI: a press anywhere in the panel body must focus it. The
    /// `ChatController` input has no separate "not editing" mode to
    /// distinguish a message-history click from an input-box click (#819 —
    /// every keystroke lands in the input once focused), so unlike the git
    /// sidebar's commit box this is a single always-focused body, not a
    /// click-focuses/click-in-input-edits split.
    #[test]
    fn ai_panel_click_focuses_panel() {
        let mut h = panel_harness(PANEL_AI);
        assert!(!h.engine.borrow().ai_has_focus);

        let sb = h.painted_sidebar_bounds.get().unwrap();
        h.driver.click(sb.x + 20.0, sb.y + 20.0);
        assert!(
            h.engine.borrow().ai_has_focus,
            "a click in the panel body must focus it (#544)"
        );
    }

    /// AI: typing after a click must land in the `ChatController` input box
    /// and grow across multiple visual lines on `Enter` — the "multi-line
    /// growing input box with an inline cursor cell" this panel's own doc
    /// comment always described but the pre-#819 hand-rolled input never
    /// actually supported (plain `Enter` used to *submit*; there was no way
    /// to type a newline into the box at all).
    ///
    /// Verified red against the pre-#819 code: with the hand-rolled
    /// `handle_ai_panel_key`, `Enter` while editing called `ai_send_message`
    /// and closed the box instead of inserting `\n`, so "second" was never
    /// painted on its own row — this test asserts on the *rendered* input
    /// text via `screen_contains`, not on engine state, so it fails exactly
    /// where the old behavior diverges.
    #[test]
    fn ai_panel_typed_text_supports_multiline_input() {
        let mut h = panel_harness(PANEL_AI);
        let sb = h.painted_sidebar_bounds.get().unwrap();
        h.driver.click(sb.x + 20.0, sb.y + 20.0);
        assert!(h.engine.borrow().ai_has_focus);

        for c in "first".chars() {
            h.driver.type_char(c);
        }
        h.driver.press_named(quadraui::NamedKey::Enter);
        for c in "second".chars() {
            h.driver.type_char(c);
        }

        assert_eq!(
            h.engine.borrow().ai_chat.borrow().input_text(),
            "first\nsecond",
            "Enter must insert a newline into the input buffer, not submit"
        );
        assert!(
            h.driver.screen_contains("first") && h.driver.screen_contains("second"),
            "both input lines must be painted"
        );
    }

    /// AI: `Ctrl+S` submits the input as a user turn, which must then appear
    /// in the rendered transcript, and the input box must clear.
    #[test]
    fn ai_panel_submit_appends_transcript_turn() {
        let mut h = panel_harness(PANEL_AI);
        let sb = h.painted_sidebar_bounds.get().unwrap();
        h.driver.click(sb.x + 20.0, sb.y + 20.0);

        for c in "hello assistant".chars() {
            h.driver.type_char(c);
        }
        h.driver.ctrl_char('s');

        assert!(
            h.driver.screen_contains("hello assistant"),
            "a submitted message must appear in the rendered transcript (#819)"
        );
        assert_eq!(
            h.engine.borrow().ai_chat.borrow().input_text(),
            "",
            "submitting must clear the input box"
        );
    }

    /// AI: the transcript must scroll — messages painted while stuck to the
    /// tail must no longer include the earliest message once scrolled up,
    /// and scrolling must reveal it. `ChatController` follows the tail by
    /// default (#819), so a fresh long conversation shows only its most
    /// recent turns until the user scrolls.
    #[test]
    fn ai_panel_scrolls_transcript() {
        let mut h = panel_harness(PANEL_AI);
        {
            let mut engine = h.engine.borrow_mut();
            engine.ai_messages = (0..60)
                .map(|i| crate::core::ai::AiMessage {
                    role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                    content: format!("MSG_MARKER_{i}"),
                })
                .collect();
        }
        let sb = h.painted_sidebar_bounds.get().unwrap();
        // A neutral event to force a repaint against the freshly-seeded
        // transcript before sampling the screen.
        h.driver.click(sb.x + 20.0, sb.y + 20.0);

        assert!(
            h.driver.screen_contains("MSG_MARKER_59"),
            "stuck-to-bottom must show the most recent message"
        );
        assert!(
            !h.driver.screen_contains("MSG_MARKER_0"),
            "a 60-message conversation must not fit entirely in the viewport, \
             so the very first message must be scrolled out of view"
        );

        // Scroll up (positive delta.y, quadraui's convention) over the panel.
        for _ in 0..60 {
            h.driver.dispatch(quadraui::UiEvent::Scroll {
                widget: None,
                delta: quadraui::ScrollDelta::new(0.0, 1.0),
                position: quadraui::Point::new(sb.x + 20.0, sb.y + 20.0),
            });
        }

        assert_eq!(
            h.engine.borrow().ai_chat.borrow().transcript_scroll_top(),
            0,
            "60 wheel notches (180 rows) must be enough to reach the very \
             top of a 60-message transcript"
        );
        assert!(
            h.driver.screen_contains("MSG_MARKER_0"),
            "scrolling up over the panel must reveal earlier messages (#819)"
        );
    }
}

/// #669: the five editor-anchored popups (completion, LSP hover, editor
/// hover, diff peek, signature help) painted through the pre-#540 Relm4
/// `draw.rs` path and stopped painting once `render_content` became the
/// live GTK draw path — the `screen.*` fields these read were (and still
/// are) populated correctly by the engine the whole time, so nothing but
/// the paint call itself was missing (#592's root-cause finding). Each
/// test below builds two independently-constructed harnesses over
/// otherwise-identical engine state, differing only in the one
/// `screen.*`-feeding field that turns the popup on, and requires at
/// least one sampled pixel near the cursor to differ — deleting the
/// corresponding paint block in `render_content` turns every one of
/// these red.
///
/// Two *independent* harnesses, not one harness rendered twice: probed
/// (see prior investigation on #669) and confirmed that re-`render()`ing
/// the *same* `GtkDriver` a second time is not guaranteed pixel-stable
/// even with zero engine-state change, while two freshly-constructed
/// harnesses over identical state paint byte-identically. This mirrors
/// `extension_panel_contributes_an_activity_bar_icon`'s established
/// with/without-harness comparison above, for the same reason.
#[cfg(test)]
mod editor_popups {
    use super::*;

    /// A minimal buffer with an active editor window — these tests only
    /// need *some* text under the cursor, not a scrollable one.
    fn small_engine() -> Engine {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "fn main() {\n    println!(\"hi\");\n}\n");
        engine
    }

    /// Sample a pixel region big enough to catch any of the five popups
    /// regardless of Top/Bottom placement fallback: several rows above and
    /// many rows below the active window's top edge, spanning most of its
    /// width. `configure` mutates the engine to (or not to) activate a
    /// popup before the harness's first (and only) render.
    fn popup_region_pixels(configure: impl FnOnce(&mut Engine)) -> Vec<(u8, u8, u8)> {
        let mut engine = small_engine();
        configure(&mut engine);
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        let rect = {
            let layout = h.screen_layout.borrow();
            layout
                .as_ref()
                .expect("render_content must have painted a ScreenLayout")
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active window must have painted")
                .rect
        };
        let lh = h.painted_line_height.get().unwrap_or(20.0);
        let x0 = (rect.x + 4.0) as i32;
        let x1 = (rect.x + rect.width - 4.0).max(rect.x + 40.0) as i32;
        let y0 = rect.y as i32;
        let y1 = (rect.y + lh * 12.0).min(rect.y + rect.height) as i32;
        let mut px = Vec::new();
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                px.push(h.driver.pixel(x, y));
                x += 3;
            }
            y += 2;
        }
        px
    }

    /// Assert that activating a popup changed at least one sampled pixel
    /// near the cursor, relative to the same region with the popup off.
    fn assert_region_changed(without: &[(u8, u8, u8)], with: &[(u8, u8, u8)], what: &str) {
        let differing = with
            .iter()
            .zip(without.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing > 0,
            "{what} must paint new pixels near the cursor; \
             {differing}/{} sampled pixels differed",
            with.len()
        );
    }

    #[test]
    fn completion_popup_paints_and_caches_a_hit_testable_layout() {
        let without = popup_region_pixels(|_| {});
        let with = popup_region_pixels(|e| {
            e.completion_candidates = vec![
                "println".to_string(),
                "print".to_string(),
                "process".to_string(),
            ];
            e.completion_idx = Some(0);
            e.completion_start_col = 0;
        });
        assert_region_changed(&without, &with, "an active completion menu");

        // Also verify the layout is cached for the click handler's
        // hit-testing (B.5b Stage 5), separately from the paint proof above.
        let mut engine = small_engine();
        engine.completion_candidates = vec![
            "println".to_string(),
            "print".to_string(),
            "process".to_string(),
        ];
        engine.completion_idx = Some(0);
        engine.completion_start_col = 0;
        let h = harness(engine, 1400, 900);
        let layout_cell = h.completion_layout.borrow();
        let layout = layout_cell
            .as_ref()
            .expect("completion popup must cache its CompletionsLayout for click hit-testing");
        assert!(layout.bounds.width > 0.0 && layout.bounds.height > 0.0);
    }

    #[test]
    fn hover_popup_paints() {
        let without = popup_region_pixels(|_| {});
        let with = popup_region_pixels(|e| {
            e.lsp_hover_text = Some("fn foo() -> i32".to_string());
        });
        assert_region_changed(&without, &with, "an active LSP hover popup");
    }

    #[test]
    fn signature_help_popup_paints() {
        let without = popup_region_pixels(|_| {});
        let with = popup_region_pixels(|e| {
            e.lsp_signature_help = Some(crate::core::lsp::SignatureHelpData {
                label: "fn foo(x: i32, y: i32)".to_string(),
                params: vec![(7, 13)],
                active_param: Some(0),
            });
        });
        assert_region_changed(&without, &with, "active signature help");
    }

    #[test]
    fn diff_peek_popup_paints() {
        let without = popup_region_pixels(|_| {});
        let with = popup_region_pixels(|e| {
            e.diff_peek = Some(crate::core::engine::DiffPeekState {
                hunk_index: 0,
                anchor_line: 1,
                hunk_lines: vec!["-old line".to_string(), "+new line".to_string()],
                file_header: String::new(),
                hunk: crate::core::git::Hunk {
                    header: "@@ -2 +2,2 @@".to_string(),
                    lines: vec!["-old line".to_string(), "+new line".to_string()],
                },
            });
        });
        assert_region_changed(&without, &with, "an open diff-peek popup");
    }

    #[test]
    fn editor_hover_popup_paints_and_caches_bounds_for_click() {
        let without = popup_region_pixels(|_| {});
        let with = popup_region_pixels(|e| {
            e.show_editor_hover(
                1,
                4,
                "**println!** — entry point",
                crate::core::engine::EditorHoverSource::Lsp,
                false,
                false,
            );
        });
        assert_region_changed(&without, &with, "an open editor-hover popup");

        // Also verify bounds are cached for the click + drag handlers (#215).
        let mut engine = small_engine();
        engine.show_editor_hover(
            1,
            4,
            "**println!** — entry point",
            crate::core::engine::EditorHoverSource::Lsp,
            false,
            false,
        );
        let h = harness(engine, 1400, 900);
        let (_, _, pw, ph) = h.editor_hover_popup_rect.get().expect(
            "editor hover popup must cache its bounds for the click + drag handlers (#215)",
        );
        assert!(pw > 0.0 && ph > 0.0);
    }

    /// #821: hover popups adopt `quadraui::compose::markdown::render_markdown_to_styled`
    /// instead of vimcode's hand-rolled `MdStyle`-to-color span walk. GTK twin
    /// of `tui_main::shell_app::tests::
    /// driver_editor_hover_renders_code_and_bare_url_link_via_quadraui_markdown`:
    /// same markdown, same three acceptance-criteria features (bold, inline
    /// code, links), checked through GTK's own black-box surface —
    /// `screen_contains` for the paint proof (markdown syntax must not leak),
    /// `editor_hover_link_rects` for the link (a cached hit-region, painted by
    /// production code, not a hardcoded rect — see that field's own doc).
    ///
    /// quadraui only recognizes `[text](url)` links, not bare `http://`
    /// autolinks; `core::markdown::linkify_bare_urls` rewrites the source
    /// markdown before quadraui ever parses it so the bare URL below still
    /// becomes a real, clickable link on both backends.
    ///
    /// **RED against an unfixed tree:** comment out the `linkify_bare_urls`
    /// call in `Engine::show_editor_hover` and `editor_hover_link_rects` comes
    /// back without the bare-URL entry — quadraui's renderer never turns
    /// unbracketed text into a link.
    #[test]
    fn editor_hover_popup_renders_code_and_bare_url_link_via_quadraui_markdown_on_gtk() {
        let mut engine = small_engine();
        engine.show_editor_hover(
            1,
            4,
            "plain821 **bold821** and `code821` — see https://example.com/docs821",
            crate::core::engine::EditorHoverSource::Lsp,
            false,
            false,
        );
        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        assert!(
            h.driver.screen_contains("bold821"),
            "the word inside **bold821** must still paint, syntax stripped; painted texts: {:?}",
            h.driver.painted_texts()
        );
        assert!(
            !h.driver.screen_contains("**bold821**"),
            "bold markdown delimiters must not leak into painted text; painted texts: {:?}",
            h.driver.painted_texts()
        );
        assert!(
            !h.driver.screen_contains("`code821`"),
            "inline-code backticks must not leak into painted text; painted texts: {:?}",
            h.driver.painted_texts()
        );

        let link_rects = h.editor_hover_link_rects.borrow().clone();
        assert!(
            link_rects
                .iter()
                .any(|(_, _, _, _, uri)| uri == "https://example.com/docs821"),
            "the bare URL must be linkified into a real clickable link rect; got {link_rects:?}"
        );
    }
}

/// Black-box paint proof for the four panel-region surfaces #670 ported from
/// the dead `src/gtk/draw.rs` path onto `render_content`: quickfix, the
/// bottom panel (terminal/debug output), the debug toolbar, and the
/// sidebar-item hover popup. `ai_panel` — the fifth surface #670 originally
/// scoped — turned out to need a genuinely new `render.rs` adapter (no
/// existing `Backend::draw_message_list`-based chrome to port, unlike these
/// four which all had one) and was split into its own follow-up issue per
/// #670's own escape hatch, so it has no test here.
///
/// Same two-independent-harnesses methodology as `editor_popups` above
/// (see its module doc for why re-rendering one `GtkDriver` isn't safe to
/// compare against itself): each test renders once with the feature off and
/// once with it on, over otherwise-identical engine state, and asserts
/// sampled pixels differ. Each was verified to fail red by temporarily
/// commenting out its paint call in `render_content` and confirming the
/// `assert_region_changed` panic before restoring it.
#[cfg(test)]
mod panel_surfaces {
    use super::*;

    fn small_engine() -> Engine {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "fn main() {\n    println!(\"hi\");\n}\n");
        engine
    }

    /// Sample a band spanning the bottom of the whole window — where
    /// quickfix/bottom-panel/debug-toolbar all paint, below the editor's
    /// windows — across the full width. Wide enough to catch any of these
    /// bands regardless of their exact height for a given fixture.
    fn bottom_region_pixels(
        width: i32,
        height: i32,
        configure: impl FnOnce(&mut Engine),
    ) -> Vec<(u8, u8, u8)> {
        let mut engine = small_engine();
        configure(&mut engine);
        let mut h = harness(engine, width, height);
        let mut px = Vec::new();
        let y0 = (height as f64 * 0.5) as i32;
        let mut y = y0;
        while y < height {
            let mut x = 0;
            while x < width {
                px.push(h.driver.pixel(x, y));
                x += 3;
            }
            y += 2;
        }
        px
    }

    fn assert_region_changed(without: &[(u8, u8, u8)], with: &[(u8, u8, u8)], what: &str) {
        let differing = with
            .iter()
            .zip(without.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing > 0,
            "{what} must paint new pixels in the bottom chrome region; \
             {differing}/{} sampled pixels differed",
            with.len()
        );
    }

    fn make_qf_item(path: &str) -> crate::core::project_search::ProjectMatch {
        crate::core::project_search::ProjectMatch {
            file: std::path::PathBuf::from(path),
            line: 1,
            col: 1,
            line_text: "fn main() {".to_string(),
        }
    }

    // Note: these three all compare *two open/visible* states rather than
    // open-vs-closed. Opening any of these panels reserves its band by
    // shrinking `editor_area_h` (the #670 layout fix), so an open-vs-closed
    // comparison sampled over this region would pass even with the actual
    // `Backend::draw_*` call deleted — the mere reservation swaps "editor
    // text" for "panel background", which alone is enough to make sampled
    // pixels differ. Comparing two same-reservation states that differ only
    // in *content* isolates the paint call itself: it can only differ if
    // that content is actually painted. Verified each still goes red by
    // temporarily deleting its `Backend::draw_*` call and confirming the
    // `assert_region_changed` panic, then restoring it.

    #[test]
    fn quickfix_panel_paints() {
        let region = |selected: usize| {
            bottom_region_pixels(1400, 900, move |e| {
                e.quickfix_items = vec![make_qf_item("a.rs"), make_qf_item("b.rs")];
                e.quickfix_open = true;
                e.quickfix_has_focus = true;
                e.quickfix_selected = selected;
            })
        };
        assert_region_changed(
            &region(0),
            &region(1),
            "moving the quickfix selection between two open, reserved-identically panels",
        );
    }

    #[test]
    fn bottom_terminal_panel_paints() {
        // Same `terminal_open` (so `el.terminal_h` — and therefore
        // `editor_area_h` — is identical either way); only the *active* tab
        // and its content differ: the terminal grid (`Backend::draw_terminal`)
        // vs. the debug-output text display (`Backend::draw_text_display`),
        // plus which tab is highlighted in the strip `Backend::draw_tab_bar`
        // paints above them.
        let region = |kind: crate::render::BottomPanelKind| {
            bottom_region_pixels(1400, 900, move |e| {
                e.terminal_open = true;
                e.session.terminal_panel_rows = 10;
                e.dap_output_lines = vec!["stack trace line 1".to_string(), "line 2".to_string()];
                e.bottom_panel_kind = kind;
            })
        };
        assert_region_changed(
            &region(crate::render::BottomPanelKind::Terminal),
            &region(crate::render::BottomPanelKind::DebugOutput),
            "switching the bottom panel's active tab between terminal and debug output",
        );
    }

    #[test]
    fn debug_toolbar_paints() {
        // Same `debug_toolbar_visible` (so the reserved band is identical);
        // only the buttons' `enabled` state differs — several toggle from
        // disabled to enabled once a session is active and stopped, which
        // `render::debug_toolbar` reflects as different button styling.
        let region = |session_active: bool| {
            bottom_region_pixels(1400, 900, move |e| {
                e.debug_toolbar_visible = true;
                if session_active {
                    e.dap_session_active = true;
                    e.dap_stopped_thread = Some(1);
                }
            })
        };
        assert_region_changed(
            &region(false),
            &region(true),
            "an active+stopped debug session changing the toolbar's button states",
        );
    }

    /// Unlike the other three surfaces here (all in the fixed bottom-of-
    /// window band), the panel-hover popup anchors next to whatever sidebar
    /// item triggered it — near the *top* of the sidebar for `item_index:
    /// 0`, not the bottom — so `bottom_region_pixels` can't see it. Instead
    /// this samples the popup's own cached bounds (`panel_hover_popup_rect`,
    /// the same cache a future click handler would read) against an
    /// identical harness with no popup open, proving both that a popup
    /// caches non-empty bounds and that painting them actually changed
    /// pixels there — not just that the cache field was written.
    #[test]
    fn panel_hover_popup_paints_and_caches_bounds() {
        let mut engine = small_engine();
        engine.show_panel_hover(
            "source_control",
            "item0",
            0,
            "**M** `src/main.rs` — modified",
        );
        let mut with_h = harness(engine, 1400, 900);
        let (px, py, pw, ph) = with_h
            .panel_hover_popup_rect
            .get()
            .expect("panel hover popup must cache its bounds for a future click handler");
        assert!(pw > 0.0 && ph > 0.0);

        let mut without_h = harness(small_engine(), 1400, 900);
        assert!(
            without_h.panel_hover_popup_rect.get().is_none(),
            "no popup rect should be cached when none is open"
        );

        let (x0, x1) = (px as i32, (px + pw) as i32);
        let (y0, y1) = (py as i32, (py + ph) as i32);
        let mut differing = 0;
        let mut total = 0;
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                total += 1;
                if with_h.driver.pixel(x, y) != without_h.driver.pixel(x, y) {
                    differing += 1;
                }
                x += 3;
            }
            y += 2;
        }
        assert!(
            differing > 0,
            "panel hover popup must paint new pixels within its own cached bounds; \
             {differing}/{total} sampled pixels differed"
        );
    }
}

/// Black-box paint proof for the four #592 chrome/transient surfaces #671
/// ports onto `render_content`: the find/replace overlay, the tab switcher
/// popup, the separated status line, and the tab-hover tooltip. Unlike the
/// #669/#670 surfaces, two of these (`separated_status_line`, `tab_tooltip`)
/// never had a GTK painter at all — see the `#671` comments at each call
/// site in `render_content` for what each one now routes through
/// (`Backend::draw_find_replace`/`draw_list` directly for the first two,
/// the new `render::tab_hover_tooltip_paint` shared adapter for the
/// tooltip, `render::window_status_line_to_status_bar` — already shared
/// with the per-window status bar — for the separated status line).
///
/// Each test was verified to fail red by temporarily commenting out its
/// paint call in `render_content` and confirming the assertion panics,
/// then restoring it.
#[cfg(test)]
mod chrome_surfaces {
    use super::*;

    fn small_engine() -> Engine {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "fn main() {\n    println!(\"hi\");\n}\n");
        engine
    }

    fn assert_region_changed(without: &[(u8, u8, u8)], with: &[(u8, u8, u8)], what: &str) {
        let differing = with
            .iter()
            .zip(without.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing > 0,
            "{what} must paint new pixels; {differing}/{} sampled pixels differed",
            with.len()
        );
    }

    /// Sample a band hugging the top-right corner of the active editor
    /// window, in absolute pixels — where `gtk::find_replace::
    /// draw_find_replace` anchors the panel (`popup_x = gb.x + gb.width -
    /// popup_w - 10`, `popup_y = gb.y + 2`).
    fn find_replace_region_pixels(configure: impl FnOnce(&mut Engine)) -> Vec<(u8, u8, u8)> {
        let mut engine = small_engine();
        configure(&mut engine);
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        let rect = {
            let layout = h.screen_layout.borrow();
            layout
                .as_ref()
                .expect("render_content must have painted a ScreenLayout")
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active window must have painted")
                .rect
        };
        let x1 = (rect.x + rect.width) as i32;
        let x0 = (x1 - 700).max(rect.x as i32);
        let y0 = rect.y as i32;
        let y1 = (rect.y + 150.0).min(rect.y + rect.height) as i32;
        let mut px = Vec::new();
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                px.push(h.driver.pixel(x, y));
                x += 3;
            }
            y += 2;
        }
        px
    }

    #[test]
    fn find_replace_overlay_paints() {
        let without = find_replace_region_pixels(|_| {});
        let with = find_replace_region_pixels(|e| {
            e.open_find_replace();
            e.find_replace_query = "hi".to_string();
        });
        assert_region_changed(&without, &with, "an open find/replace overlay");
    }

    /// Two tabs so `open_tab_switcher` has more than one MRU entry to show
    /// (it no-ops — leaves `tab_switcher_open` false — with only one).
    fn engine_with_two_tabs() -> Engine {
        let mut engine = small_engine();
        engine.new_tab(None);
        engine
    }

    #[test]
    fn tab_switcher_popup_paints_and_caches_bounds() {
        let mut engine = engine_with_two_tabs();
        engine.open_tab_switcher();
        assert!(
            engine.tab_switcher_open,
            "fixture must actually open the switcher"
        );
        let mut with_h = harness(engine, 1400, 900);
        let (px, py, pw, ph) = with_h
            .tab_switcher_popup_rect
            .get()
            .expect("tab switcher popup must cache its bounds for click routing (#671)");
        assert!(pw > 0.0 && ph > 0.0);

        let mut without_h = harness(engine_with_two_tabs(), 1400, 900);
        assert!(
            without_h.tab_switcher_popup_rect.get().is_none(),
            "no tab switcher rect should be cached when the switcher is closed"
        );

        let (x0, x1) = (px as i32, (px + pw) as i32);
        let (y0, y1) = (py as i32, (py + ph) as i32);
        let mut differing = 0;
        let mut total = 0;
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                total += 1;
                if with_h.driver.pixel(x, y) != without_h.driver.pixel(x, y) {
                    differing += 1;
                }
                x += 3;
            }
            y += 2;
        }
        assert!(
            differing > 0,
            "tab switcher popup must paint new pixels within its own cached bounds; \
             {differing}/{total} sampled pixels differed"
        );
    }

    // ── #733 slice 1: the shared modal-overlay mouse rung ────────────────
    //
    // `handle_mouse_click_msg`'s top rung is now
    // `render::route_modal_overlay_click`, the same function TUI's
    // `handle_mouse` calls. Before this the GTK dialog arm sat ~600 lines
    // *below* the context-menu and find/replace arms, so an open modal
    // dialog did not actually have top priority — the exact precedence
    // drift #733 exists to kill.

    /// #733 acceptance, GTK half: an open in-canvas modal dialog must
    /// swallow a click that lands on the context menu painted underneath
    /// it. Asserted on the painted surface — the menu's own "Paste" item
    /// label (always enabled, so it is the item that would fire and close
    /// the menu) must still be on-screen after the click.
    ///
    /// RED against unfixed `develop`: `dispatch_context_menu_click` ran
    /// before the dialog block, so the click confirmed "Paste",
    /// `context_menu_confirm` closed the menu, and the label vanished from
    /// the next frame.
    ///
    /// The menu is anchored well inside the editor area rather than at the
    /// origin because `try_route_sidebar_mouse_event` claims sidebar-bound
    /// clicks *before* `handle_mouse_click_msg` runs at all, and that entry
    /// point has not been migrated onto the shared router yet (#733
    /// slice 4, panels).
    #[test]
    fn open_dialog_swallows_a_click_meant_for_the_context_menu() {
        let mut engine = small_engine();
        engine.start_move_file_dialog(
            std::path::Path::new("/tmp/project/foo.rs"),
            std::path::Path::new("/tmp/project"),
        );
        engine.open_editor_context_menu(700, 400);
        let mut h = harness(engine, 1400, 900);
        assert!(
            h.dialog_layout.borrow().is_some(),
            "fixture must open an in-canvas (non-native) dialog"
        );
        let paste = h
            .driver
            .find("Paste")
            .expect("the context menu must paint its always-enabled Paste item");

        h.driver.click(paste.0, paste.1);
        h.driver.render();

        assert!(
            h.driver.screen_contains("Paste"),
            "an open modal dialog must swallow the click instead of letting \
             the context menu underneath confirm an item and close"
        );
    }

    /// #733 review finding: a modal dialog must swallow a *right*-click
    /// too, not just a left one — it is supposed to eat every event while
    /// open (see the doc comment on `route_modal_overlay_click`'s dialog
    /// rung: "eats everything, including motion"). Before this fix,
    /// `MouseButton::Right` in `handle()`'s `UiEvent::MouseDown` arm never
    /// consulted `route_modal_overlay_click` at all — only the left-click
    /// path (`handle_mouse_click_msg`) did — so a right-click landing
    /// inside the dialog's painted bounds fell straight through to
    /// `handle_editor_right_click`, which opens the editor's context menu
    /// underneath the still-open dialog.
    ///
    /// RED against the review finding: before the fix, this right-click
    /// opens the editor context menu and "Paste" (its always-enabled
    /// item) paints.
    #[test]
    fn open_dialog_swallows_a_right_click_meant_for_the_editor_context_menu() {
        let engine = {
            let mut engine = small_engine();
            engine.start_move_file_dialog(
                std::path::Path::new("/tmp/project/foo.rs"),
                std::path::Path::new("/tmp/project"),
            );
            engine
        };
        let mut h = harness(engine, 1400, 900);
        assert!(
            h.dialog_layout.borrow().is_some(),
            "fixture must open an in-canvas (non-native) dialog"
        );
        assert!(
            !h.driver.screen_contains("Paste"),
            "sanity: no context menu should be open before the right-click"
        );

        h.driver.dispatch(quadraui::UiEvent::MouseDown {
            widget: None,
            button: quadraui::MouseButton::Right,
            position: quadraui::Point::new(700.0, 400.0),
            modifiers: quadraui::Modifiers::default(),
        });
        h.driver.render();

        assert!(
            !h.driver.screen_contains("Paste"),
            "an open modal dialog must swallow the right-click instead of \
             letting it open the editor's context menu underneath; \
             painted texts were {:?}",
            h.driver.painted_texts()
        );
        assert!(
            h.dialog_layout.borrow().is_some(),
            "the dialog itself must still be open — swallowed, not dismissed"
        );
    }

    /// Cross-backend parity for the tab-switcher rung the same router now
    /// owns on both sides (TUI's half is
    /// `driver_click_inside_tab_switcher_popup_dismisses_and_is_consumed`
    /// in `src/tui_main/shell_app.rs`): a click inside the painted popup
    /// dismisses it. Asserted on the painted surface via the popup's own
    /// "Open Tabs" title, and on the geometry cache that
    /// `route_modal_overlay_click` consumes.
    #[test]
    fn click_inside_tab_switcher_popup_dismisses_it() {
        let mut engine = engine_with_two_tabs();
        engine.open_tab_switcher();
        let mut h = harness(engine, 1400, 900);
        let (px, py, pw, ph) = h
            .tab_switcher_popup_rect
            .get()
            .expect("the painted popup must cache its bounds for the router");
        assert!(
            h.driver.screen_contains("Open Tabs"),
            "precondition: the popup paints its title"
        );

        h.driver
            .click((px + pw / 2.0) as f32, (py + ph / 2.0) as f32);
        h.driver.render();

        assert!(
            h.tab_switcher_popup_rect.get().is_none(),
            "the popup must stop painting after a click inside it"
        );
        assert!(
            !h.driver.screen_contains("Open Tabs"),
            "the dismissed popup's title must be gone from the painted surface"
        );
    }

    // ── #734 slice 1: the shared modal keyboard rung ─────────────────────
    //
    // `handle_key_press`'s top rung is now `render::route_modal_key`, the
    // same function `src/tui_main/shell_app.rs::handle_key_pressed` calls.
    // GTK previously opened with a *hand-rolled* context-menu rung that did
    // not go through `Engine::handle_context_menu_key`, and had no
    // top-level dialog rung at all.

    /// #734 acceptance, GTK half: an open context menu is modal, so a key
    /// it does not act on must still be consumed — not close the menu *and*
    /// move the editor cursor underneath it.
    ///
    /// `l` is the sharpest probe: `Engine::handle_context_menu_key` treats
    /// it as "confirm" (the TUI half has always done so), while GTK's
    /// hand-rolled ladder fell into its `_` arm, which closed the menu and
    /// then deliberately fell through to normal key handling — where `l` is
    /// a Normal-mode cursor-right motion.
    ///
    /// Both halves are read off the painted frame: the menu's own
    /// always-enabled "Paste" item, and the status bar's `Ln N, Col N`
    /// readout, which is where a leaked editor keypress shows up.
    ///
    /// RED against unfixed `develop`: the menu closed (first assertion
    /// passes) but the cursor advanced to `Ln 1, Col 2`.
    #[test]
    fn context_menu_key_is_consumed_instead_of_leaking_to_the_editor() {
        let mut engine = small_engine();
        engine.open_editor_context_menu(700, 400);
        let mut h = harness(engine, 1400, 900);
        assert!(
            h.driver.screen_contains("Paste"),
            "precondition: the context menu paints its always-enabled Paste item"
        );
        assert!(
            h.driver.screen_contains("Ln 1, Col 1"),
            "precondition: the cursor starts at line 1, column 1"
        );

        h.driver.type_char('l');
        h.driver.render();

        assert!(
            !h.driver.screen_contains("Paste"),
            "'l' must reach `Engine::handle_context_menu_key` and close the menu"
        );
        assert!(
            h.driver.screen_contains("Ln 1, Col 1"),
            "the key must be consumed by the modal menu, not also fall through \
             to the editor and move the cursor right"
        );
    }

    /// The dialog twin: a modal dialog must beat the activity-bar focus
    /// tier. GTK's `handle_key_press` had no dialog rung of its own — it
    /// relied on the general `Engine::handle_key` fallback at the *bottom*
    /// of the ladder, so every focus tier above it (activity bar, explorer,
    /// extension panel, settings, search, source control) could cut in
    /// front of an open modal. Four of those tiers carried an inline
    /// `if engine.dialog.is_some()` patch-up for exactly this; the
    /// activity-bar tier did not.
    ///
    /// RED against unfixed `develop`: `Escape` ran
    /// `activity_bar_focus_out()` and the dialog stayed painted.
    #[test]
    fn open_dialog_takes_keys_from_a_focused_activity_bar() {
        let mut engine = small_engine();
        engine.activity_bar_focus_in_at(0);
        engine.start_move_file_dialog(
            std::path::Path::new("/tmp/project/foo.rs"),
            std::path::Path::new("/tmp/project"),
        );
        let mut h = harness(engine, 1400, 900);
        assert!(
            h.driver.screen_contains("Move 'foo.rs'"),
            "precondition: the in-canvas modal dialog paints its title"
        );

        h.driver.press_named(quadraui::NamedKey::Escape);
        h.driver.render();

        assert!(
            !h.driver.screen_contains("Move 'foo.rs'"),
            "Escape must reach the modal dialog and dismiss it, not be spent \
             unfocusing the activity bar underneath it"
        );
    }

    /// Sample a band spanning the bottom half of the whole window — where
    /// the separated status line paints, above the terminal panel. Wide
    /// enough to catch it regardless of the fixture's exact chrome heights.
    fn bottom_region_pixels(
        width: i32,
        height: i32,
        configure: impl FnOnce(&mut Engine),
    ) -> Vec<(u8, u8, u8)> {
        let mut engine = small_engine();
        configure(&mut engine);
        let mut h = harness(engine, width, height);
        let mut px = Vec::new();
        let y0 = (height as f64 * 0.5) as i32;
        let mut y = y0;
        while y < height {
            let mut x = 0;
            while x < width {
                px.push(h.driver.pixel(x, y));
                x += 3;
            }
            y += 2;
        }
        px
    }

    /// Same `terminal_open`/`status_line_above_terminal` in both variants
    /// (so the separated-status band's own reservation — `el.separated_status_h`
    /// — is identical either way, per the same isolation rationale
    /// `panel_surfaces` documents above); only the active buffer's `dirty`
    /// flag differs, which `build_window_status_line` reflects as an extra
    /// `" [+]"` segment. Isolates the paint call from the layout reservation
    /// it sits inside.
    #[test]
    fn separated_status_line_paints() {
        let region = |dirty: bool| {
            bottom_region_pixels(1400, 900, move |e| {
                e.settings.status_line_above_terminal = false;
                e.terminal_open = true;
                e.session.terminal_panel_rows = 10;
                if dirty {
                    let id = e.active_buffer_id();
                    if let Some(buf) = e.buffer_manager.get_mut(id) {
                        buf.dirty = true;
                    }
                }
            })
        };
        assert_region_changed(
            &region(false),
            &region(true),
            "the active buffer's dirty flag changing the separated status line's content",
        );
    }

    #[test]
    fn tab_hover_tooltip_paints() {
        let region = |tooltip: Option<&str>| {
            let mut engine = small_engine();
            engine.tab_hover_tooltip = tooltip.map(|s| s.to_string());
            let mut h = harness(engine, 1400, 900);
            let mut px = Vec::new();
            let mut y = 0;
            while y < 200 {
                let mut x = 0;
                while x < 1400 {
                    px.push(h.driver.pixel(x, y));
                    x += 3;
                }
                y += 2;
            }
            px
        };
        assert_region_changed(
            &region(None),
            &region(Some("main.rs")),
            "an active tab-hover tooltip",
        );
    }

    /// Positional guard for the tooltip's Y offset (#671 review fix): a
    /// broad "did *some* pixel change" scan (like `tab_hover_tooltip_paints`
    /// above) stays green even if the tooltip paints in the wrong place, so
    /// this test pins the exact band.
    ///
    /// GTK's tab row is `tab_row_h = (lh * 1.6).ceil()` tall — `1.6x` a line
    /// height, unlike TUI's exactly-one-cell-row tab bar where `area.y + 1`
    /// cleanly clears it. An earlier version of the GTK paint call offset
    /// the tooltip by one `lh` instead of `tab_row_h`, landing its top edge
    /// *inside* the tab row's own vertical span — painting over tab labels
    /// instead of sitting in a clean band below the tab bar. Verified this
    /// test fails red against that `lh`-only offset before restoring the
    /// `tab_row_h` fix.
    ///
    /// Breadcrumbs are explicitly turned **off** here so `tab_bar_h ==
    /// tab_row_h` and the first painted window's rect — `windows[0].rect.y`
    /// (`main.y + tab_bar_h`, the editor content's top edge) — is
    /// unambiguously "right where the tab row ends", with no separate
    /// breadcrumb row to account for. (With breadcrumbs on, the correct,
    /// fixed offset — `tab_row_h`, matching TUI's `area.y + 1`, which is
    /// likewise breadcrumb-height-agnostic — actually lands *on* the
    /// breadcrumb row rather than below it, which is TUI-parity-correct but
    /// would make this test's "must not paint above `editor_top`" band
    /// ambiguous; disabling breadcrumbs removes that variable entirely.)
    #[test]
    fn tab_hover_tooltip_paints_below_tab_row_not_inside_it() {
        let width = 1400;
        let height = 900;

        // Discover the painted line height and the editor content's top
        // edge once; neither depends on whether the tooltip text itself is
        // present.
        let (lh, editor_top) = {
            let mut engine = small_engine();
            engine.settings.breadcrumbs = false;
            engine.tab_hover_tooltip = Some("main.rs".to_string());
            let h = harness(engine, width, height);
            let lh = h
                .painted_line_height()
                .expect("line height must be painted");
            let top = h
                .screen_layout
                .borrow()
                .as_ref()
                .expect("screen layout must be painted")
                .windows
                .first()
                .expect("at least one window painted")
                .rect
                .y;
            (lh, top)
        };
        let editor_top_i = editor_top.round() as i32;
        let lh_i = lh.round() as i32;
        // Breadcrumbs are off, so `editor_top == main.y + tab_row_h`
        // exactly; halfway back up from there is solidly inside the tab
        // row's own span (`tab_row_h = 1.6 * lh`, so half a line height of
        // slack never crosses its top edge at `main.y`) yet still within
        // the old buggy `lh`-only offset's paint band, so it catches the
        // regression without depending on the theme's tab-row/gutter colors
        // matching by coincidence.
        let inside_tab_row_y = (editor_top_i - lh_i / 2).max(0);

        let region = |tooltip: Option<&str>, y0: i32, y1: i32| -> Vec<(u8, u8, u8)> {
            let mut engine = small_engine();
            engine.settings.breadcrumbs = false;
            engine.tab_hover_tooltip = tooltip.map(|s| s.to_string());
            let mut h = harness(engine, width, height);
            let mut px = Vec::new();
            let mut y = y0;
            while y < y1 {
                let mut x = 0;
                while x < width {
                    px.push(h.driver.pixel(x, y));
                    x += 3;
                }
                y += 1;
            }
            px
        };

        // Inside the tab row's own vertical span -- the tooltip must not
        // paint there.
        assert_eq!(
            region(Some("main.rs"), inside_tab_row_y, inside_tab_row_y + 2),
            region(None, inside_tab_row_y, inside_tab_row_y + 2),
            "tooltip must not paint inside the tab row's own vertical span \
             -- it should sit below the tab row, not overlap tab labels"
        );

        // Immediately below the tab row (the editor content's top edge) is
        // where the tooltip should land.
        assert_ne!(
            region(Some("main.rs"), editor_top_i, editor_top_i + lh_i),
            region(None, editor_top_i, editor_top_i + lh_i),
            "tooltip must paint in the band immediately below the tab row"
        );
    }
}

/// Black-box paint + click-routing proof for the VS Code-style Command
/// Center (`◀ ▶` nav arrows + centered `🔍 <project>` search box) dropped by
/// the #540 Relm4->ShellApp cutover and never re-wired (#676).
///
/// `engine.command_center_layout` sat permanently `None` on GTK before this
/// fix — nothing painted the strip and nothing hit-tested it — even though
/// `render::build_command_center_view` / `Backend::draw_command_center` were
/// already shared, working code (TUI has used them since #635). The whole
/// `menu_end..right-edge` band was instead claimed end-to-end by
/// `window_controls_status_bar`'s background fill, which is what silently
/// ate the Command Center's real estate.
///
/// Each test below was verified to fail red against the pre-fix tree (paint
/// call absent from `render_content`, hit-test absent from `handle()`,
/// `controls_rect` spanning the full band) before this module was added.
#[cfg(test)]
mod command_center {
    use super::*;
    use crate::core::engine::PickerSource;

    /// Engine with a real `cwd` (so the search box's title isn't empty) and
    /// two tabs wired into `tab_nav_history` at index 1, so `tab_nav_back`
    /// has somewhere to go and `tab_nav_can_go_back()` reads `true` the
    /// moment the frame paints. `tab_nav_push` is what normally populates
    /// this on real navigation; this fixture pokes the (pub) fields
    /// directly instead of routing through disk I/O, mirroring
    /// `engine_with_breadcrumb_path`'s style above.
    fn engine_with_tab_history() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        let group = engine.active_group;
        let tab0 = engine.active_tab().id;
        engine.new_tab(None);
        let tab1 = engine.active_tab().id;
        assert_ne!(tab0, tab1, "fixture needs two distinct tabs");
        engine.tab_nav_history = vec![(group, tab0), (group, tab1)];
        engine.tab_nav_index = 1;
        engine
    }

    /// `true` iff any pixel in `rect` (absolute, scanned on a coarse grid to
    /// keep the test fast) differs from `bg`. Scanning a region rather than
    /// probing one point avoids a flaky false-negative if the single probed
    /// pixel happens to land on inter-glyph whitespace that still reads as
    /// background — the same reasoning `chrome_surfaces::assert_region_changed`
    /// documents above.
    fn region_has_non_background_pixel(
        driver: &mut quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        rect: quadraui::Rect,
        bg: (u8, u8, u8),
    ) -> bool {
        let (x0, y0) = (rect.x.round() as i32, rect.y.round() as i32);
        let (x1, y1) = (
            (rect.x + rect.width).round() as i32,
            (rect.y + rect.height).round() as i32,
        );
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                if driver.pixel(x, y) != bg {
                    return true;
                }
                x += 1;
            }
            y += 1;
        }
        false
    }

    #[test]
    fn command_center_paints_between_menu_labels_and_window_controls() {
        let mut h = harness(engine_with_tab_history(), 1400, 900);
        // `harness()`/`driver_with_shell` already painted a first frame at
        // construction, but repaint explicitly so this test doesn't depend
        // on that construction-time detail.
        h.driver.render();

        let layout = h
            .engine
            .borrow()
            .command_center_layout
            .borrow()
            .clone()
            .expect(
                "menu_bar_visible is forced true on GTK (App::setup), so the \
                 Command Center must paint every frame and cache its layout \
                 for click dispatch (#676) -- it must never be permanently None",
            );

        let back = layout
            .back_bounds
            .expect("the back arrow must have a painted bounds");
        let fwd = layout
            .forward_bounds
            .expect("the forward arrow must have a painted bounds");
        let search = layout
            .search_bounds
            .expect("the search box must have a painted bounds (non-empty title)");

        assert!(
            back.x < fwd.x && fwd.x + fwd.width <= search.x,
            "arrows and search box must lay out left-to-right without \
             overlapping: back={back:?} forward={fwd:?} search={search:?}"
        );

        // The core #676 regression: window controls used to claim the
        // *entire* menu_end..right-edge band, so the Command Center must
        // never paint past where the (now-narrowed) controls actually
        // start.
        let controls = h.title_bar_rect.get();
        assert!(
            controls.width > 0.0,
            "window controls must have painted a non-degenerate rect"
        );
        assert!(
            search.x + search.width <= controls.x + 0.5,
            "the Command Center must not overlap the window-control buttons \
             -- search box ends at {} but controls start at {}",
            search.x + search.width,
            controls.x
        );
        assert!(
            back.x >= 0.0 && back.x < controls.x,
            "the Command Center must sit entirely left of the window controls"
        );

        // Pixel-probe (not painted text): the arrow glyphs are Unicode ◀/▶
        // and the search box is an icon + rounded border, so — per this
        // issue's black-box test note — assert on rasterised pixels rather
        // than on `GtkDriver::find`/`screen_contains`.
        let theme = crate::render::Theme::from_name(&h.engine.borrow().settings.colorscheme);
        let bg = {
            let c = crate::render::to_quadraui_color(theme.tab_bar_bg);
            (c.r, c.g, c.b)
        };
        assert!(
            region_has_non_background_pixel(&mut h.driver, back, bg),
            "the back arrow must paint pixels distinguishable from the bar's own background"
        );
        assert!(
            region_has_non_background_pixel(&mut h.driver, fwd, bg),
            "the forward arrow must paint pixels distinguishable from the bar's own background"
        );
        assert!(
            region_has_non_background_pixel(&mut h.driver, search, bg),
            "the search box must paint pixels (border/icon/text) distinguishable \
             from the bar's own background"
        );
    }

    /// #710 item 1 / quadraui#637: the open Edit-menu dropdown ("Undo /
    /// Redo / Cut / Copy / Paste / Find / Replace" + accelerators) must
    /// paint in `settings.ui_font_size` — the menu bar's own font — not the
    /// editor font. Before quadraui#637 landed,
    /// `GtkBackend::draw_context_menu` handed the frame's shared Pango
    /// layout straight to the rasteriser without swapping in `ui_font`
    /// first (unlike `draw_menu_bar`, already fixed by quadraui#624/#705),
    /// so the bar above the dropdown tracked `ui_font_size` while the
    /// dropdown itself stayed pinned to whatever font a previous paint call
    /// left on the layout. Mirrors the established pattern in
    /// `breadcrumb_text_width_tracks_ui_font_size_not_editor_font_size` and
    /// `tab_label_width_tracks_ui_font_size_not_editor_font_size` just
    /// above: vary `ui_font_size` as the positive control (glyph extents
    /// must move), then vary `settings.font_size` alone as a regression
    /// guard (glyph extents must NOT move — vimcode's GTK runner paints the
    /// editor at a hardcoded font regardless of that setting, so there is
    /// no live paint path from it to chrome text; see that same doc
    /// comment for the full rationale). Verified this fails (both "Undo"
    /// bounds identical across `ui_font_size` 11 vs 28) with the pin rolled
    /// back to the pre-#637 rev.
    #[test]
    fn edit_menu_dropdown_item_glyphs_track_ui_font_size_not_editor_font_size() {
        fn open_edit_menu_and_find_undo(
            mut engine: Engine,
            ui_font_size: u8,
            font_size: i32,
        ) -> quadraui::Rect {
            engine.settings.ui_font_size = ui_font_size;
            engine.settings.font_size = font_size;
            let mut h = harness(engine, 1400, 900);
            h.driver.render();
            let edit = h
                .driver
                .find_bounds("Edit")
                .expect("the \"Edit\" top-level menu label must paint");
            h.driver
                .click(edit.x + edit.width / 2.0, edit.y + edit.height / 2.0);
            h.driver.render();
            h.driver
                .find_bounds("Undo")
                .expect("clicking \"Edit\" must open its dropdown with \"Undo\" painted")
        }

        let small = open_edit_menu_and_find_undo(engine_with_tab_history(), 8, 14);
        let big = open_edit_menu_and_find_undo(engine_with_tab_history(), 28, 14);
        assert!(
            big.width > small.width * 1.5 && big.height > small.height * 1.5,
            "dropdown item glyph extents must track settings.ui_font_size (8 \
             vs 28 pt): got small={small:?} big={big:?} -- if these are \
             close, draw_context_menu isn't honouring ui_font and the \
             dropdown is stuck at the editor font"
        );

        let a = open_edit_menu_and_find_undo(engine_with_tab_history(), 11, 10);
        let b = open_edit_menu_and_find_undo(engine_with_tab_history(), 11, 60);
        assert!(
            (a.width - b.width).abs() < 0.5 && (a.height - b.height).abs() < 0.5,
            "dropdown item glyph extents must NOT track settings.font_size \
             (10 vs 60) with ui_font_size held fixed: got a={a:?} b={b:?}"
        );
    }

    /// #710 item 2: the title-bar/menu-row band and the Command Center
    /// search-box pill inside it must be closer to VS Code's 35px title
    /// bar / ~26px pill than the pre-#710 `with_title_bar(1.0)` (~18px
    /// band / ~14px pill in this same headless harness -- one editor text
    /// line, visibly squat). `build_shell_config`'s `with_title_bar`
    /// multiplier is the only knob (quadraui's `AppShell` has no fixed-px
    /// band reservation API yet -- see the #710 comment on that call site),
    /// so the band is still an `lh` multiple; per this issue's acceptance
    /// criteria that means the useful assertion is "the pill height matches
    /// the intended target at the default size", pinned with a tolerance
    /// band around the ~27px this measures in the headless harness at the
    /// chosen 1.7 multiplier. The second assertion pins the residual this
    /// doc note calls out: because the GTK runner paints the editor at a
    /// hardcoded font regardless of `settings.font_size` (see the sibling
    /// dropdown-font test's doc comment), the row height a single
    /// `font_size` value produces here is already stable across
    /// `font_size` even with the bug reinstated at 1.0 -- so it does NOT
    /// alone distinguish fixed from unfixed and is kept only as the
    /// "stable across two font_size values" half of the acceptance
    /// criteria, not as the regression guard (that's the first assertion).
    #[test]
    fn title_bar_band_and_command_center_pill_hit_vs_code_parity_target() {
        let mut h = harness(engine_with_tab_history(), 1400, 900);
        h.driver.render();
        let band = h.title_bar_rect.get().height;
        let layout = h
            .engine
            .borrow()
            .command_center_layout
            .borrow()
            .clone()
            .expect("command center must have painted");
        let pill = layout
            .search_bounds
            .expect("search box must have painted bounds")
            .height
            - 4.0;

        assert!(
            (25.0..=35.0).contains(&band),
            "title-bar band height should land near VS Code's 35px title \
             bar (pre-#710 with_title_bar(1.0) measured ~18px here, one \
             editor text line): got {band}px"
        );
        assert!(
            (22.0..=31.0).contains(&pill),
            "command-centre pill height should land near VS Code's ~26px \
             pill (pre-#710 measured ~14px here): got {pill}px"
        );

        // Stable across `settings.font_size` (see doc comment above for why
        // this doesn't distinguish fixed from unfixed on its own).
        let mut engine_small = engine_with_tab_history();
        engine_small.settings.font_size = 10;
        let mut h_small = harness(engine_small, 1400, 900);
        h_small.driver.render();
        let band_small = h_small.title_bar_rect.get().height;

        let mut engine_big = engine_with_tab_history();
        engine_big.settings.font_size = 40;
        let mut h_big = harness(engine_big, 1400, 900);
        h_big.driver.render();
        let band_big = h_big.title_bar_rect.get().height;

        assert!(
            (band_small - band_big).abs() < 0.5,
            "title-bar band height must be stable across settings.font_size \
             (10 vs 40): got small={band_small} big={band_big}"
        );
    }

    #[test]
    fn command_center_click_routes_nav_and_opens_picker() {
        let mut h = harness(engine_with_tab_history(), 1400, 900);
        h.driver.render();

        assert!(
            h.engine.borrow().tab_nav_can_go_back(),
            "fixture must start with back-navigation available"
        );
        assert!(
            !h.engine.borrow().tab_nav_can_go_forward(),
            "fixture must start at the end of history"
        );
        let tab_before = h.engine.borrow().active_tab().id;

        let layout = h
            .engine
            .borrow()
            .command_center_layout
            .borrow()
            .clone()
            .expect("command center must have painted");
        let back = layout.back_bounds.expect("back arrow must be painted");
        let fwd = layout
            .forward_bounds
            .expect("forward arrow must be painted");
        let search = layout.search_bounds.expect("search box must be painted");

        // Back arrow -> tab-nav history moves backward (#676).
        h.driver
            .click(back.x + back.width / 2.0, back.y + back.height / 2.0);
        assert_ne!(
            h.engine.borrow().active_tab().id,
            tab_before,
            "clicking the back arrow must navigate tab history"
        );
        assert!(h.engine.borrow().tab_nav_can_go_forward());

        // Forward arrow -> undoes the back navigation (#676).
        h.driver
            .click(fwd.x + fwd.width / 2.0, fwd.y + fwd.height / 2.0);
        assert_eq!(
            h.engine.borrow().active_tab().id,
            tab_before,
            "clicking the forward arrow must undo the back navigation"
        );

        assert!(
            !h.engine.borrow().picker_open,
            "no picker should be open before the search box is clicked"
        );

        // Search box -> opens the unified Command Center picker (#676).
        h.driver.click(
            search.x + search.width / 2.0,
            search.y + search.height / 2.0,
        );
        assert!(
            h.engine.borrow().picker_open,
            "clicking the search box must open the picker"
        );
        assert_eq!(
            h.engine.borrow().picker_source,
            PickerSource::CommandCenter,
            "the search box must open the picker with the CommandCenter source"
        );
        // #677 audit: `picker_popup_rect` is written only inside
        // `render_content`'s picker draw branch, so this proves the popup
        // actually painted -- not merely that engine state flipped (mirrors
        // `status_bar_segment_click_opens_go_to_line_picker` /
        // `breadcrumb_segment_click_opens_the_dropdown_and_selection_dispatches`,
        // #555). The `picker_open`/`picker_source` asserts above were the
        // whole test before this audit; verified vacuous by mutation:
        // replacing `render_content`'s
        // `picker_popup_rect.set(Some((...)))` write with `set(None)`
        // (leaving the palette paint and `picker_open` untouched) left the
        // two asserts above green and only this one red.
        let (_, _, pw, ph) = h
            .picker_popup()
            .expect("the Command Center picker must actually paint, not just flip engine state");
        assert!(
            pw > 0.0 && ph > 0.0,
            "the painted picker popup must have a non-degenerate rect, got {pw}x{ph}"
        );
    }

    /// #676 design note: GTK forces `menu_bar_visible = true` at startup
    /// (`App::setup`), unlike TUI where it's optional, so in practice the
    /// Command Center is always visible on GTK. This guards the other half
    /// of that contract anyway (mirroring TUI's identical gate,
    /// `render_content_does_not_paint_menu_bar_when_hidden_via_shell_app`):
    /// when the flag is off, clicking where the Command Center used to be
    /// must no longer trigger Command Center behaviour (stale hit-region),
    /// not just clear the layout-cache field in isolation.
    ///
    /// #677 audit: the original version of this test asserted only
    /// `command_center_layout.borrow().is_none()` — a state check with no
    /// observable-behaviour probe, exactly the #553/#592 shape (a flag
    /// flips, nothing confirms the click path actually changed). A first
    /// attempt at replacing it with a raw pixel-region probe (same
    /// coordinates, `region_has_non_background_pixel` before/after) turned
    /// out to be a false-positive risk rather than a strengthening: hiding
    /// the menu bar reserves one fewer title-bar row, so `main_content`
    /// reflows upward and the tab bar's own (non-background) pixels land on
    /// the old Command Center coordinates — that probe went red against
    /// *correct*, unmodified code, which would have made this a flaky/wrong
    /// test rather than a fixed one. Click-behaviour is layout-shift-proof
    /// and directly exercises the actual risk the doc above names (a stale
    /// cached rect still accepting clicks): verified non-vacuous by
    /// mutation — commenting out the `command_center_layout.replace(None)`
    /// clear (`src/gtk/mod.rs`, the `else` arm right after the Command
    /// Center paint block) makes `assert!(!h.engine.borrow().picker_open, ...)`
    /// below fail, because `handle()`'s click dispatch still finds a
    /// (stale) `Some(layout)` to hit-test against and opens the picker.
    #[test]
    fn command_center_layout_clears_when_menu_bar_is_hidden() {
        let mut h = harness(engine_with_tab_history(), 1400, 900);
        h.driver.render();
        assert!(
            h.engine.borrow().tab_nav_can_go_back(),
            "fixture must start with back-navigation available"
        );
        let tab_before = h.engine.borrow().active_tab().id;
        let layout = h
            .engine
            .borrow()
            .command_center_layout
            .borrow()
            .clone()
            .expect("must be painted while the menu bar is visible");
        let back = layout.back_bounds.expect("back arrow must be painted");
        let search = layout.search_bounds.expect("search box must be painted");

        h.engine.borrow_mut().menu_bar_visible = false;
        h.driver.render();

        // Click at the *old* back-arrow and search-box coordinates: with the
        // menu bar hidden neither must still behave like Command Center
        // controls, even though the layout has reflowed and something else
        // (editor/tab bar) may now occupy those pixels.
        h.driver
            .click(back.x + back.width / 2.0, back.y + back.height / 2.0);
        assert_eq!(
            h.engine.borrow().active_tab().id,
            tab_before,
            "clicking the old back-arrow coordinates after hiding the menu bar \
             must not navigate tab history"
        );
        h.driver.click(
            search.x + search.width / 2.0,
            search.y + search.height / 2.0,
        );
        assert!(
            !h.engine.borrow().picker_open,
            "clicking the old search-box coordinates after hiding the menu bar \
             must not open the Command Center picker"
        );
        assert!(
            h.engine.borrow().command_center_layout.borrow().is_none(),
            "hiding the menu bar must clear the cached Command Center layout"
        );
    }
}

/// #699 Tier 2a (#701): the default theme's line-number and breadcrumb
/// foregrounds must actually *paint* dimmed, and the cursor line's number
/// must stay bright.
///
/// Both are colour-token changes on `Theme::onedark()` — `line_number_fg`
/// `#b2b2b2` → `#858585` (VS Code's `editorLineNumber.foreground`) and
/// `breadcrumb_fg` `#7f848e` → `#6c7079`. The wiring already existed, so a
/// test that asserted "the theme field holds X" would be asserting the
/// constant against itself and would stay green if the paint path stopped
/// consulting it. These probe the rendered surface instead, exactly as this
/// file's header prescribes: locate the target through the layout the frame
/// published, then read pixels.
///
/// RED-first (re-run on the #785 tree, not assumed): with both values
/// reverted on this same tree,
/// `inactive_line_numbers_dim_while_the_cursor_line_stays_bright` fails its
/// inactive-gutter assertion — the probe reads `(177, 176, 176)` instead of
/// `#858585` — and `breadcrumb_path_paints_dimmer_than_editor_body_text`
/// fails its segment-colour assertion, reading `(117, 123, 132)` instead of
/// `#6c7079`.
///
/// Two corrections to what #701 recorded here, both from re-measuring rather
/// than from any behaviour change:
///
/// - The breadcrumb figure was `(126, 130, 137)`; it is `(117, 123, 132)`
///   now because that probe reads a per-channel ceiling instead of a single
///   brightest pixel (see [`channel_ceiling`], and #785 for the flake that
///   forced the switch).
/// - #701 also recorded the reverted colour as failing the body-text ratio
///   assertion at 0.566 against a 0.55 ceiling. It does not: `#7f848e`
///   measures ≈0.535 here and slips under. The *colour* assertion is what
///   rejects it, at every possible coverage. The ratio assertion is still
///   worth its place — it is the only one that would catch `breadcrumb_fg`
///   staying dim while the editor's own text darkened to meet it — but it
///   is not a second line of defence for this particular regression, and
///   should not be relied on as one.
#[cfg(test)]
mod vscode_dimming {
    use super::*;
    use crate::core::settings::LineNumberMode;

    /// Rec. 709 relative luminance of a painted pixel.
    fn luma((r, g, b): (u8, u8, u8)) -> f64 {
        0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64
    }

    /// Brightest pixel in the half-open box `[x0, x1) × [y0, y1)`.
    ///
    /// Glyph text is antialiased *from* the background *towards* the
    /// foreground, so a fully-covered stem pixel carries the unblended
    /// foreground and every other text pixel is darker. Taking the maximum
    /// therefore recovers the fg colour the renderer was handed, and — the
    /// property this module leans on — a blend can never overshoot it: no
    /// amount of antialiasing over `background` `#1a1a1a` can produce
    /// `#b2b2b2` out of an `#858585` pen.
    fn brightest(
        h: &mut Harness<impl AppLogic>,
        x0: i32,
        x1: i32,
        y0: i32,
        y1: i32,
    ) -> (u8, u8, u8) {
        let mut best = (0u8, 0u8, 0u8);
        for x in x0..x1 {
            for y in y0..y1 {
                let p = h.driver.pixel(x, y);
                if luma(p) > luma(best) {
                    best = p;
                }
            }
        }
        best
    }

    /// Per-channel maximum over the half-open box `[x0, x1) × [y0, y1)` —
    /// the subpixel-antialiasing-correct twin of [`brightest`] (#785).
    ///
    /// [`brightest`]'s "the max-luminance pixel carries the unblended pen"
    /// argument silently assumes *grayscale* antialiasing, where a single
    /// coverage fraction drives all three channels, so the brightest pixel is
    /// the one closest to full coverage on every channel at once. Under
    /// subpixel (RGB) antialiasing — which fontconfig turns on by default on
    /// many Linux desktops, including the machine #785's Test stage ran on —
    /// each channel gets its *own* coverage out of a 3-tap LCD filter. For a
    /// stem narrower than that kernel no single pixel saturates all three at
    /// once, and which channels the brightest pixel favours depends on the
    /// glyph's subpixel phase. Measured on the breadcrumb's `#6c7079` "src"
    /// across runs of the same suite on the same machine, same painted rect:
    /// `(100, 104, 113)`, `(80, 112, 121)` and `(108, 112, 98)` — the same
    /// pen, spread up to 23/255 apart, against a [`near`] tolerance of 10.
    /// That is the ~1-run-in-40 flake #785's Test stage hit.
    ///
    /// Maximising each channel independently restores the invariant
    /// [`brightest`]'s doc leans on. A blend over a darker background can
    /// never overshoot the pen in *any* channel, so every component here is
    /// still an upper bound that no amount of antialiasing can inflate — a
    /// brighter pen therefore still fails the comparison — but each component
    /// now converges on the pen as soon as *some* pixel in the box saturates
    /// *that* channel, which no longer has to be the same pixel for all
    /// three. Use this whenever the probed glyphs are UI-chrome sized; the
    /// editor's own (larger, monospace) text saturates readily enough that
    /// [`brightest`] is still fine for the gutter and body-text probes.
    fn channel_ceiling(
        h: &mut Harness<impl AppLogic>,
        x0: i32,
        x1: i32,
        y0: i32,
        y1: i32,
    ) -> (u8, u8, u8) {
        let mut best = (0u8, 0u8, 0u8);
        for x in x0..x1 {
            for y in y0..y1 {
                let p = h.driver.pixel(x, y);
                best = (best.0.max(p.0), best.1.max(p.1), best.2.max(p.2));
            }
        }
        best
    }

    /// Per-channel closeness, loose enough to absorb glyph-rasteriser
    /// rounding (the observed worst case is 4/255) and far tighter than the
    /// 45/255 gap between the old and new line-number values.
    fn near(a: (u8, u8, u8), b: (u8, u8, u8)) -> bool {
        const TOL: i32 = 10;
        (a.0 as i32 - b.0 as i32).abs() <= TOL
            && (a.1 as i32 - b.1 as i32).abs() <= TOL
            && (a.2 as i32 - b.2 as i32).abs() <= TOL
    }

    /// `(window rect, gutter width in px, line height)` for the active pane
    /// as the last frame painted it.
    fn pane_geometry(h: &Harness<impl AppLogic>) -> (crate::core::WindowRect, f64, f64) {
        let win = h.engine.borrow().active_window_id();
        let (rect, gutter_cells) = {
            let layout = h.screen_layout.borrow();
            let w = layout
                .as_ref()
                .expect("a frame must have painted")
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active pane must be in the painted layout");
            (w.rect, w.gutter_char_width)
        };
        assert!(
            gutter_cells > 1,
            "test setup sanity: line numbers must be on, or there is no \
             gutter to probe (got {gutter_cells} cells)"
        );
        let lh = h
            .painted_line_height()
            .expect("frame must publish the line height it painted with");
        (rect, gutter_cells as f64 * h.painted_char_width(), lh)
    }

    /// Brightest pixel inside row `row`'s slice of the line-number gutter.
    fn gutter_probe(h: &mut Harness<impl AppLogic>, row: usize) -> (u8, u8, u8) {
        let (rect, gutter_px, lh) = pane_geometry(h);
        brightest(
            h,
            rect.x as i32,
            (rect.x + gutter_px) as i32,
            (rect.y + lh * row as f64).ceil() as i32,
            (rect.y + lh * (row as f64 + 1.0)).floor() as i32,
        )
    }

    /// Inactive line numbers must render at VS Code's
    /// `editorLineNumber.foreground` **and** the cursor's own line number
    /// must not — one frame, two probes.
    ///
    /// The second probe is the point: a test that only checked the dim value
    /// on some row would pass with `line_number_active_fg` broken (every row
    /// would read `#858585` and the assertion would be satisfied by whichever
    /// row it happened to sample).
    #[test]
    fn inactive_line_numbers_dim_while_the_cursor_line_stays_bright() {
        // VS Code's `editorLineNumber.foreground` — the value #701 adopts.
        const VSCODE_LINE_NUMBER_FG: (u8, u8, u8) = (0x85, 0x85, 0x85);

        let mut engine = Engine::new();
        // Line numbers default to `LineNumberMode::None`, so there is no
        // gutter to probe unless the test turns them on — this is the
        // `:set number` configuration the tokens exist for.
        engine.settings.line_numbers = LineNumberMode::Absolute;
        engine
            .buffer_mut()
            .insert(0, "aaa\nbbb\nccc\nddd\neee\nfff\n");
        let mut h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        h.window_center(win).expect("editor pane must paint");
        assert_eq!(
            h.engine.borrow().cursor().line,
            0,
            "test setup sanity: the cursor must sit on row 0 so row 0 is the \
             active gutter row and row 3 is an inactive one"
        );

        let inactive = gutter_probe(&mut h, 3);
        assert!(
            near(inactive, VSCODE_LINE_NUMBER_FG),
            "an inactive line number must paint at VS Code's \
             editorLineNumber.foreground {VSCODE_LINE_NUMBER_FG:?} (#701); \
             the gutter's brightest pixel on row 3 was {inactive:?} — the \
             pre-#701 default #b2b2b2 reads (177, 176, 176) here"
        );

        let active = gutter_probe(&mut h, 0);
        assert!(
            !near(active, VSCODE_LINE_NUMBER_FG),
            "the cursor line's number must NOT paint at the dimmed inactive \
             colour — got {active:?}, which is indistinguishable from \
             {VSCODE_LINE_NUMBER_FG:?}. Dimming line_number_fg is only a win \
             if line_number_active_fg still lifts the cursor row out of it"
        );
        let active_token =
            crate::render::to_quadraui_color(crate::render::Theme::onedark().line_number_active_fg);
        assert!(
            near(active, (active_token.r, active_token.g, active_token.b)),
            "the cursor line's number must paint at line_number_active_fg \
             ({active_token:?}); got {active:?}"
        );
        assert!(
            luma(active) > luma(inactive) + 40.0,
            "the active line number must be visibly brighter than an \
             inactive one: active {active:?} (luma {:.0}) vs inactive \
             {inactive:?} (luma {:.0})",
            luma(active),
            luma(inactive)
        );
    }

    /// A non-trailing breadcrumb segment must paint measurably dimmer than
    /// the editor's body text **in the same frame**, so the path recedes
    /// instead of competing with the code (#701).
    #[test]
    fn breadcrumb_path_paints_dimmer_than_editor_body_text() {
        // The dimmed `breadcrumb_fg` #701 adopts (a 15% dim of #7f848e).
        const BREADCRUMB_FG: (u8, u8, u8) = (0x6c, 0x70, 0x79);
        // Ratio ceiling: the crumb must sit at most a little over half of
        // body-text luminance. Measured 0.453 with #6c7079 (#785 re-measured
        // it after switching the probe to `channel_ceiling`; #701 recorded
        // 0.483 off the old single-pixel probe). See this module's doc for
        // why the pre-#701 #7f848e does *not* also fail this one.
        const MAX_RATIO: f64 = 0.55;

        let mut engine = Engine::new_for_test();
        // A real multi-component path under `cwd` is what makes
        // `build_breadcrumbs_for_group` emit more than one segment, so
        // `bc:0` is a *non-trailing* crumb and therefore uses
        // `breadcrumb_fg` rather than `breadcrumb_active_fg`.
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.cwd = cwd.clone();
        let buf = engine.active_buffer_id();
        if let Some(state) = engine.buffer_manager.get_mut(buf) {
            state.file_path = Some(cwd.join("src").join("main.rs"));
        }
        engine
            .buffer_mut()
            .insert(0, "fn main() {}\nlet alpha = beta;\nlet gamma = delta;\n");

        let mut h = harness(engine, 1400, 900);
        assert!(
            h.engine.borrow().settings.breadcrumbs,
            "fixture assumes breadcrumbs are on by default"
        );
        let win = h.engine.borrow().active_window_id();
        h.window_center(win).expect("editor pane must paint");
        let group = h.engine.borrow().active_group;

        // Locate the crumb through the layout the frame published (never
        // hardcode coordinates) and probe the rect it reported.
        let seg = {
            let layout = h.screen_layout.borrow();
            let bc = layout
                .as_ref()
                .unwrap()
                .breadcrumbs
                .iter()
                .find(|b| b.group_id == group)
                .expect("the breadcrumb bar must have painted for this group");
            let guard = bc.draw_layout.borrow();
            let sbl = guard
                .as_ref()
                .expect("the breadcrumb bar must have cached a painted layout");
            let want = quadraui::WidgetId::new("bc:0");
            let r = sbl
                .hit_regions
                .iter()
                .find_map(|(r, hit)| match hit {
                    quadraui::StatusBarHit::Segment(id) if *id == want => Some(*r),
                    _ => None,
                })
                .expect("segment 0 must have a painted hit region");
            quadraui::Rect::new(
                bc.bounds.x as f32 + r.x,
                bc.bounds.y as f32 + r.y,
                r.width,
                r.height,
            )
        };
        assert!(
            seg.width > 1.0 && seg.height > 1.0,
            "segment 0 must have painted a non-degenerate rect, got {seg:?}"
        );

        // `channel_ceiling`, not `brightest` (#785): breadcrumb labels are
        // UI-chrome sized, and under subpixel antialiasing no single pixel of
        // a 10pt stem is fully covered on all three channels at once. See
        // that helper's doc for the three different readings the same painted
        // frame produced across runs, and why per-channel maxima keep the
        // "a blend can never overshoot the pen" guarantee intact.
        let crumb = channel_ceiling(
            &mut h,
            seg.x as i32,
            (seg.x + seg.width) as i32,
            seg.y as i32,
            (seg.y + seg.height) as i32,
        );
        // Asymmetric, and deliberately not [`near`]: the two directions carry
        // different physics (#785).
        //
        // *Above* the pen is the load-bearing half. Compositing text over the
        // darker bar background can only ever move a channel from the
        // background *towards* the pen, so no coverage, phase or filter can
        // push a channel past the pen it was drawn with — a reading above
        // `#6c7079` means the renderer was handed a brighter colour, full
        // stop. `OVERSHOOT` is therefore just rasteriser rounding.
        //
        // *Below* the pen is only ever coverage loss, and says nothing about
        // which pen was used, so it can be generous. It still has to be
        // bounded: without a floor a heavily-blended brighter pen would slip
        // under the ceiling. The two together pin the pen from both sides —
        // the pre-#701 `#7f848e` would have to paint at ≤85% coverage to
        // duck `OVERSHOOT` and at ≥85% to clear `UNDERSHOOT`, which is
        // impossible, so it is rejected at *every* coverage rather than only
        // at the one this machine's fontconfig happens to produce.
        //
        // Measured here: `#6c7079` reads (100, 104, 113) (≈90% coverage —
        // subpixel antialiasing never saturates a 10pt stem, see
        // `channel_ceiling`) and `#7f848e` reads (117, 123, 132). Under the
        // old symmetric ±10 the wrong colour was being rejected by 1/255 on
        // two channels; here it misses by 5 while the right one keeps 4 in
        // hand at both ends.
        const OVERSHOOT: i32 = 4;
        const UNDERSHOOT: i32 = 12;
        let pen_matches = |a: (u8, u8, u8), b: (u8, u8, u8)| {
            let ok = |x: u8, y: u8| {
                let d = x as i32 - y as i32;
                d <= OVERSHOOT && d >= -UNDERSHOOT
            };
            ok(a.0, b.0) && ok(a.1, b.1) && ok(a.2, b.2)
        };
        assert!(
            pen_matches(crumb, BREADCRUMB_FG),
            "a non-trailing breadcrumb segment must paint at the dimmed \
             breadcrumb_fg {BREADCRUMB_FG:?} (#701) — every channel within \
             +{OVERSHOOT}/-{UNDERSHOOT}; the painted segment rect's \
             per-channel ceiling was {crumb:?}. The pre-#701 #7f848e reads \
             (117, 123, 132) here."
        );

        // Body text from the *same* frame: row 2 of the pane, past the
        // gutter. Row 0 is skipped deliberately — the block cursor sits
        // there and its inverted cell is not body text.
        let (rect, gutter_px, lh) = {
            let win_id = h.engine.borrow().active_window_id();
            let (rect, gutter_cells) = {
                let layout = h.screen_layout.borrow();
                let w = layout
                    .as_ref()
                    .unwrap()
                    .windows
                    .iter()
                    .find(|w| w.window_id == win_id)
                    .unwrap();
                (w.rect, w.gutter_char_width)
            };
            let lh = h.painted_line_height().unwrap();
            (rect, gutter_cells as f64 * h.painted_char_width(), lh)
        };
        let body = brightest(
            &mut h,
            (rect.x + gutter_px) as i32,
            (rect.x + gutter_px + 200.0) as i32,
            (rect.y + lh * 2.0) as i32,
            (rect.y + lh * 3.0) as i32,
        );
        assert!(
            luma(body) > 150.0,
            "test setup sanity: row 2 must actually have painted body text \
             to compare against, got {body:?}"
        );

        let ratio = luma(crumb) / luma(body);
        assert!(
            ratio <= MAX_RATIO,
            "breadcrumb text must recede from the editor's body text in the \
             same frame: crumb {crumb:?} (luma {:.0}) vs body {body:?} (luma \
             {:.0}) is a ratio of {ratio:.3}, above the {MAX_RATIO} ceiling \
             (#701)",
            luma(crumb),
            luma(body)
        );
    }
}

#[cfg(test)]
mod minimap {
    use super::*;

    // ── Minimap (#35) ───────────────────────────────────────────────────

    /// A buffer with a distinctive indentation shape and enough lines that
    /// the minimap has something to down-sample.
    fn engine_with_shaped_buffer() -> Engine {
        let mut engine = Engine::new();
        let text: String = (0..400)
            .map(|i| {
                let depth = if (100..300).contains(&i) { 3 } else { 0 };
                format!("{}fn item_{i}() {{ let x = {i}; }}\n", "    ".repeat(depth))
            })
            .collect();
        engine.buffer_mut().insert(0, &text);
        engine
    }

    /// Acceptance (#35): the minimap column is painted, and the width it
    /// takes is exactly what `quadraui::reserved_width` reserves — the
    /// editor pane gives up precisely that many pixels and gets them all
    /// back with `:set nominimap`.
    ///
    /// RED-first: reverting `build_screen_layout`'s rect narrowing makes the
    /// `on_w + strip.width == off_w` assertion fail, and dropping the
    /// `draw_minimap_strip` call in `render_content` collapses `painted` to
    /// 0 — both confirmed by hand before restoring the fix.
    #[test]
    fn minimap_paints_a_strip_whose_width_matches_reserved_width() {
        let mut h_on = harness(engine_with_shaped_buffer(), 1400, 900);
        let win_on = h_on.engine.borrow().active_window_id();
        assert!(
            h_on.engine.borrow().settings.minimap,
            "test setup sanity: the minimap must default on, or this test \
             isn't exercising the default at all"
        );
        h_on.window_center(win_on)
            .expect("editor pane must paint with the default settings");

        let (strip, on_w, on_cols) = {
            let layout = h_on.screen_layout.borrow();
            let l = layout.as_ref().unwrap();
            let mm = l
                .minimap
                .iter()
                .find(|m| m.window_id == win_on)
                .expect("the layout must carry a minimap for the pane when the setting is on");
            let rw = l.windows.iter().find(|w| w.window_id == win_on).unwrap();
            (mm.rect, rw.rect.width, rw.text_viewport_cols)
        };

        // The editor must get every one of those pixels back when it's off.
        let mut engine_off = engine_with_shaped_buffer();
        engine_off.settings.minimap = false;
        let h_off = harness(engine_off, 1400, 900);
        let win_off = h_off.engine.borrow().active_window_id();
        h_off
            .window_center(win_off)
            .expect("editor pane must paint with the minimap disabled");
        let (off_w, off_cols) = {
            let layout = h_off.screen_layout.borrow();
            let l = layout.as_ref().unwrap();
            assert!(
                l.minimap.is_empty(),
                "`minimap: false` must remove the minimap from the layout"
            );
            let rw = l.windows.iter().find(|w| w.window_id == win_off).unwrap();
            (rw.rect.width, rw.text_viewport_cols)
        };

        // #722: the reserved width is now a proportion of the pane's own
        // width rather than a fixed `MINIMAP_COLS` count, so the column
        // delta isn't a pinned constant any more. The pixel-exact assertion
        // right below is the real acceptance check (`build_screen_layout`
        // narrows/widens the rect by exactly `strip.width`); this is just a
        // column-domain sanity check that *some* text columns came back.
        assert!(
            off_cols > on_cols,
            "the editor must regain text columns when the minimap is off \
             (on={on_cols}, off={off_cols})"
        );
        assert_eq!(
            on_w + strip.width,
            off_w,
            "the editor must reclaim exactly the reserved width when the \
             minimap is off (on={on_w} + strip={} vs off={off_w})",
            strip.width
        );

        // …and something actually painted in that column band. Probe a grid
        // of points inside the strip and require the frame not to be a
        // uniform block of background there.
        let lh = h_on
            .painted_line_height()
            .expect("frame must publish the line height it painted with");
        let x0 = (strip.x + 2.0) as i32;
        let x1 = (strip.x + strip.width - 2.0) as i32;
        let y0 = (strip.y + lh) as i32;
        let y1 = (strip.y + strip.height - lh) as i32;
        let mut seen = std::collections::HashSet::new();
        for y in (y0..y1).step_by(3) {
            for x in x0..x1 {
                seen.insert(h_on.driver.pixel(x, y));
            }
        }
        assert!(
            seen.len() > 1,
            "the minimap column must paint content, not a uniform block: \
             sampled x in {x0}..{x1}, y in {y0}..{y1} and found only \
             {:?}",
            seen
        );
    }

    /// #728 acceptance: on an ordinary wide pane the minimap strip settles
    /// at VS Code's own ~120px width instead of scaling up with the pane —
    /// the pre-fix `rect_width * MINIMAP_WIDTH_FRACTION` formula reached
    /// ~240px on a pane this wide, roughly twice VS Code's. Driven through
    /// the real paint path (`ScreenLayout` from an actual `window_center`
    /// call), not just `minimap_reserved_width` in isolation.
    #[test]
    fn minimap_strip_settles_at_vs_code_parity_width_on_a_wide_pane() {
        let h = harness(engine_with_shaped_buffer(), 1600, 900);
        let win = h.engine.borrow().active_window_id();
        h.window_center(win)
            .expect("editor pane must paint with the default settings");

        let (strip_width, pane_width) = {
            let layout = h.screen_layout.borrow();
            let l = layout.as_ref().unwrap();
            let mm = l
                .minimap
                .iter()
                .find(|m| m.window_id == win)
                .expect("the layout must carry a minimap for the pane");
            let rw = l.windows.iter().find(|w| w.window_id == win).unwrap();
            (mm.rect.width, rw.rect.width + mm.rect.width)
        };
        let char_width = h.painted_char_width();
        let expected =
            crate::render::minimap_reserved_width(&h.engine.borrow(), pane_width, char_width);

        assert_eq!(
            strip_width, expected,
            "the real paint path must reserve exactly what \
             minimap_reserved_width computes"
        );
        assert!(
            strip_width < 150.0,
            "a 1600px pane must not blow past VS Code's ~120px minimap \
             width (got {strip_width}px — the pre-#728 formula would have \
             hit ~240px here)"
        );
    }

    /// #728 acceptance: the minimap strip must never extend into the
    /// per-window status row painted at the bottom of the same window.
    /// `build_screen_layout` reserves `status_h` off the bottom of the
    /// strip's own rect via `render::window_status_row_reserved` — the same
    /// predicate GTK's h-scrollbar geometry now shares (previously it used
    /// a diverging predicate; see `gtk::h_scrollbar_status_offset_tests`).
    #[test]
    fn minimap_strip_never_overlaps_the_per_window_status_row() {
        let mut engine = engine_with_shaped_buffer();
        engine.settings.window_status_line = true;
        let h = harness(engine, 1400, 900);
        let win = h.engine.borrow().active_window_id();
        h.window_center(win)
            .expect("editor pane must paint with the status line on");

        let lh = h
            .painted_line_height()
            .expect("frame must publish the line height it painted with");
        let layout = h.screen_layout.borrow();
        let l = layout.as_ref().unwrap();
        let mm = l
            .minimap
            .iter()
            .find(|m| m.window_id == win)
            .expect("the layout must carry a minimap for the pane");
        let rw = l.windows.iter().find(|w| w.window_id == win).unwrap();
        assert!(
            rw.status_line.is_some(),
            "test setup sanity: the per-window status line must actually \
             be painted, or this test isn't exercising the overlap risk \
             at all"
        );

        let status_row_top = rw.rect.y + rw.rect.height - lh;
        let strip_bottom = mm.rect.y + mm.rect.height;
        assert!(
            strip_bottom <= status_row_top + 0.01,
            "the minimap strip (bottom={strip_bottom}) must not extend \
             into the per-window status row (top={status_row_top})"
        );
    }

    /// Acceptance (#35): a click at the vertical middle of the strip scrolls
    /// the pane to ~50% of the file — the GTK half of the cross-backend
    /// claim, driven through the real `pixel_to_click_target` path.
    ///
    /// Deliberately does **not** trust `scroll_top()` alone (CLAUDE.md:
    /// "Assert on rendered output — never on state being populated" — a
    /// click that mutates `scroll_top` but never actually repaints the new
    /// position is exactly the #587/#592 bug shape). The fixture's
    /// indentation shape (12-space indent for lines 100..300 of 400) makes
    /// the *paint* observable even though this harness can't record
    /// editor/gutter text (`Harness::window_center`'s doc comment): the
    /// column band the indent occupies holds no syntax-colored glyph ink
    /// when a row is indented, and does when it isn't, so counting
    /// non-grayscale ("colorful") pixels in that band before/after
    /// distinguishes "repainted the new scroll position" from "only the
    /// state moved".
    ///
    /// RED-first: hardcoding `build_rendered_window`'s `scroll_top` local to
    /// `0` (so engine state moves but the paint stays pinned to the top of
    /// the file) makes the final assertion fail with the indented row still
    /// showing ~294 colorful pixels instead of 0 — confirmed by hand, along
    /// with an initial brightness-based version of this probe that turned
    /// out to be theme-dependent noise (see the color-vs-brightness note
    /// above) and had to be replaced with this colorfulness count — before
    /// restoring the fix.
    #[test]
    fn minimap_click_at_the_middle_scrolls_to_half_the_file() {
        let mut h = harness(engine_with_shaped_buffer(), 1400, 900);
        let win = h.engine.borrow().active_window_id();
        h.window_center(win).expect("editor pane must paint");

        let (strip, total, rect, gutter_px, char_w, lh) = {
            let layout = h.screen_layout.borrow();
            let l = layout.as_ref().unwrap();
            let mm = l
                .minimap
                .iter()
                .find(|m| m.window_id == win)
                .expect("minimap must be present for the active pane");
            let rw = l
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active pane must be in the painted layout");
            (
                mm.rect,
                mm.minimap.total_buffer_lines,
                rw.rect,
                rw.gutter_char_width as f64 * h.painted_char_width(),
                h.painted_char_width(),
                h.painted_line_height()
                    .expect("frame must publish the line height it painted with"),
            )
        };
        assert_eq!(
            h.engine.borrow().scroll_top(),
            0,
            "fixture must start at the top of the file"
        );

        // The 12-column band right after the gutter, on the top visible
        // row: unindented content ("fn item_0() ...") paints syntax-colored
        // glyph ink *somewhere* in this band before the click, while a row
        // from the indented band (100..300) paints nothing there but blank
        // indentation (background plus, at most, an indent-guide line —
        // grayscale, `r == g == b`). Counting *colorful* pixels (channels
        // that disagree, i.e. not grayscale) rather than comparing raw
        // brightness or the full color set keeps this theme-agnostic and
        // immune to the indent guide: a light theme makes background the
        // *brightest* color in the band rather than the ink, and indent
        // guides paint real (if faint) grayscale pixels in the same band
        // even on a correctly-repainted frame.
        let band_x0 = (rect.x + gutter_px) as i32;
        let band_x1 = (rect.x + gutter_px + 12.0 * char_w) as i32;
        let row_y0 = (rect.y).ceil() as i32;
        let row_y1 = (rect.y + lh).floor() as i32;
        let is_colorful = |(r, g, b): (u8, u8, u8)| {
            let (r, g, b) = (r as i32, g as i32, b as i32);
            const TOL: i32 = 12; // AA-rounding tolerance, matching `near()` above
            (r - g).abs() > TOL || (g - b).abs() > TOL || (r - b).abs() > TOL
        };
        let colorful_pixel_count = |h: &mut Harness<_>| -> usize {
            let mut n = 0;
            for y in row_y0..row_y1 {
                for x in band_x0..band_x1 {
                    if is_colorful(h.driver.pixel(x, y)) {
                        n += 1;
                    }
                }
            }
            n
        };
        let before = colorful_pixel_count(&mut h);
        assert!(
            before > 0,
            "test setup sanity: the unindented top row must paint some \
             syntax-colored glyph ink in the probed band, not a blank/gray \
             block"
        );

        h.driver.click(
            (strip.x + strip.width / 2.0) as f32,
            (strip.y + strip.height / 2.0) as f32,
        );
        h.driver.render();

        let scroll_top = h.engine.borrow().scroll_top();
        let frac = scroll_top as f64 / total as f64;
        assert!(
            (frac - 0.5).abs() < 0.1,
            "clicking the middle of the minimap must scroll to ~50% of the \
             file, got scroll_top={scroll_top} of {total} ({frac:.3})"
        );
        assert!(
            (100..300).contains(&scroll_top),
            "the ~50% scroll must land inside the fixture's indented band \
             (lines 100..300) for the paint probe below to be meaningful; \
             got scroll_top={scroll_top}"
        );

        // The band that used to hold "fn item_N(...)" ink must now show no
        // colorful (syntax-highlighted) pixels at all — proof the view
        // actually repainted the indented band, not just moved `scroll_top`
        // in engine state while the paint stayed on the old lines.
        let after = colorful_pixel_count(&mut h);
        assert_eq!(
            after, 0,
            "the band right after the gutter must show no syntax-colored \
             glyph ink once the view scrolls into the indented band — found \
             {after} colorful pixels; scroll_top moved to {scroll_top} but \
             the paint didn't follow it"
        );
    }

    /// Acceptance (#35): pressing and holding on the minimap and dragging
    /// keeps seeking — not just the pixel under the initial mouse-down.
    ///
    /// RED-first regression: before this fix, `pixel_to_click_target`'s
    /// minimap hit-test only ran when `mutate_focus` was true, and
    /// `handle_mouse_drag` (the drag-continuation path) always called it
    /// with `mutate_focus: false` — so a `mouse_down` + `mouse_move`
    /// gesture (this test, run through the real
    /// `handle_mouse_drag_msg` -> `handle_mouse_drag` dispatch) only ever
    /// saw the mouse-down's own seek; the follow-up drag never reached the
    /// minimap resolver at all and the assertion below failed. Confirmed by
    /// hand against the pre-fix `click.rs` before restoring the fix.
    #[test]
    fn minimap_drag_keeps_seeking_while_the_button_is_held() {
        let mut h = harness(engine_with_shaped_buffer(), 1400, 900);
        let win = h.engine.borrow().active_window_id();
        h.window_center(win).expect("editor pane must paint");

        let (strip, total) = {
            let layout = h.screen_layout.borrow();
            let mm = layout
                .as_ref()
                .unwrap()
                .minimap
                .iter()
                .find(|m| m.window_id == win)
                .expect("minimap must be present for the active pane");
            (mm.rect, mm.minimap.total_buffer_lines)
        };
        assert_eq!(
            h.engine.borrow().scroll_top(),
            0,
            "fixture must start at the top of the file"
        );

        // Press down near the top of the strip — mouse-down alone already
        // seeks there (covered by the click test above).
        h.driver.mouse_down(
            (strip.x + strip.width / 2.0) as f32,
            (strip.y + strip.height * 0.1) as f32,
        );
        let after_down = h.engine.borrow().scroll_top();

        // Continue the SAME held-button gesture down to the vertical
        // middle of the strip, without releasing the button — this
        // exercises `handle_mouse_drag`, not a fresh mouse-down.
        h.driver.mouse_move(
            (strip.x + strip.width / 2.0) as f32,
            (strip.y + strip.height / 2.0) as f32,
        );
        h.driver.mouse_up(
            (strip.x + strip.width / 2.0) as f32,
            (strip.y + strip.height / 2.0) as f32,
        );

        let after_drag = h.engine.borrow().scroll_top();
        let frac = after_drag as f64 / total as f64;
        assert!(
            (frac - 0.5).abs() < 0.1,
            "dragging while the button is held must keep seeking the \
             minimap: after mouse-down scroll_top={after_down}, after \
             continuing the drag to the strip's vertical middle it must \
             land at ~50% of {total} lines but got \
             scroll_top={after_drag} ({frac:.3})"
        );
        assert_ne!(
            after_down, after_drag,
            "the drag must move the scroll position further than the \
             initial mouse-down alone — both landed on {after_down}, so \
             the drag continuation never reached the minimap"
        );
    }

    /// `engine_with_shaped_buffer` split into two panes (`:vsplit`) — the
    /// fixture the split-minimap black-box tests below share.
    fn engine_with_split_shaped_buffer() -> Engine {
        use crate::core::window::SplitDirection;
        let mut engine = engine_with_shaped_buffer();
        engine.split_window(SplitDirection::Vertical, None);
        engine
    }

    /// #722 acceptance, painted-output tier: a `:vsplit` must paint **two**
    /// independent minimap strips, one over each pane's own buffer — not a
    /// single strip pinned to whichever pane happens to be active.
    ///
    /// GTK twin of TUI's
    /// `split_paints_two_independent_minimap_strips_via_shell_app`
    /// (`tui_main/shell_app.rs`) — same acceptance criterion, driven
    /// through the real headless `GtkDriver` paint path. Every #722 GTK
    /// test before this one (including
    /// `minimap_paints_a_strip_whose_width_matches_reserved_width` above)
    /// only ever painted a single, unsplit window; the review that
    /// reopened #722 flagged the missing split/pixel coverage by name.
    ///
    /// RED against the pre-#722 code (single `Option<RenderedMinimap>`
    /// gated on `active_window_id`): `screen.minimap` would carry no entry
    /// for the inactive pane at all, so its strip band would sample as a
    /// uniform block (plain editor background) instead of the varied
    /// braille/syntax-colour content asserted below — confirmed by hand by
    /// reverting `build_screen_layout`'s minimap map to
    /// `.find(|(id, _)| *id == active_window_id)` before restoring the fix.
    #[test]
    fn split_paints_two_independent_minimap_strips() {
        let mut h = harness(engine_with_split_shaped_buffer(), 1400, 900);

        let win_ids: Vec<_> = h.engine.borrow().windows.keys().copied().collect();
        assert_eq!(win_ids.len(), 2, "`:vsplit` must produce two windows");

        // Paint a first frame — also primes `screen_layout`/`painted_line_height`.
        for id in &win_ids {
            h.window_center(*id)
                .unwrap_or_else(|| panic!("pane {id:?} must paint"));
        }
        let lh = h
            .painted_line_height()
            .expect("frame must publish the line height it painted with");

        for id in &win_ids {
            let strip = {
                let layout = h.screen_layout.borrow();
                layout
                    .as_ref()
                    .unwrap()
                    .minimap
                    .iter()
                    .find(|m| m.window_id == *id)
                    .unwrap_or_else(|| panic!("pane {id:?} must carry its own minimap"))
                    .rect
            };
            let x0 = (strip.x + 2.0) as i32;
            let x1 = (strip.x + strip.width - 2.0) as i32;
            let y0 = (strip.y + lh) as i32;
            let y1 = (strip.y + strip.height - lh) as i32;
            let mut seen = std::collections::HashSet::new();
            for y in (y0..y1).step_by(3) {
                for x in x0..x1 {
                    seen.insert(h.driver.pixel(x, y));
                }
            }
            assert!(
                seen.len() > 1,
                "pane {id:?}'s minimap strip must paint content, not a \
                 uniform block: sampled x in {x0}..{x1}, y in {y0}..{y1}, \
                 found only {:?}",
                seen
            );
        }
    }

    /// #722 acceptance, painted-output tier: switching focus between panes
    /// of a `:vsplit` must not move either pane's text — GTK twin of TUI's
    /// `focus_change_does_not_move_either_panes_text_via_shell_app`.
    /// Coverage for the "migrates on focus change, reflowing both panes"
    /// symptom the issue called out as *worse* than the missing strip (the
    /// width reclaim was gated on the same `is_active` flag as the strip
    /// itself, so both panes reflowed on every focus change).
    ///
    /// Mutates focus directly through `Engine::focus_next_window` on the
    /// harness's shared `engine: Rc<RefCell<Engine>>` — the same "assert on
    /// engine state after an event" escape hatch the harness's own doc
    /// comment describes, used here only to *drive* the focus change
    /// (GTK's own Ctrl-W accelerator wiring is out of scope for this test)
    /// — then forces a real repaint (`h.driver.render()`, the same pattern
    /// `menu_bar_visible`'s test elsewhere in this file uses) and diffs the
    /// two *painted* window rects, which is the acceptance claim under
    /// test.
    ///
    /// RED against the pre-#722 code: focusing the right pane would widen
    /// it (reclaiming the now-inactive left pane's minimap width) and
    /// narrow the left pane by the same amount, moving both panes' painted
    /// rects — confirmed by hand by reverting the `minimap_w` reclaim to
    /// the old `is_active`-gated single value before restoring this fix.
    #[test]
    fn focus_change_does_not_move_either_panes_text() {
        let mut h = harness(engine_with_split_shaped_buffer(), 1400, 900);

        let win_ids: Vec<_> = h.engine.borrow().windows.keys().copied().collect();
        assert_eq!(win_ids.len(), 2, "`:vsplit` must produce two windows");
        for id in &win_ids {
            h.window_center(*id)
                .unwrap_or_else(|| panic!("pane {id:?} must paint"));
        }

        fn painted_rects(
            h: &Harness<impl AppLogic>,
            win_ids: &[crate::core::WindowId],
        ) -> Vec<(f64, f64, f64, f64)> {
            let layout = h.screen_layout.borrow();
            let l = layout.as_ref().unwrap();
            win_ids
                .iter()
                .map(|id| {
                    let r = l.windows.iter().find(|w| w.window_id == *id).unwrap().rect;
                    (r.x, r.y, r.width, r.height)
                })
                .collect()
        }

        let before = painted_rects(&h, &win_ids);

        let active_before = h.engine.borrow().active_window_id();
        h.engine.borrow_mut().focus_next_window();
        let active_after = h.engine.borrow().active_window_id();
        assert_ne!(
            active_before, active_after,
            "test setup sanity: focus_next_window must actually move focus \
             to the other pane, or this test isn't exercising a focus \
             change at all"
        );
        h.driver.render();

        let after = painted_rects(&h, &win_ids);

        assert_eq!(
            before, after,
            "cycling focus between panes of a `:vsplit` must not move \
             either pane's painted rect (i.e. must not reflow either \
             pane's text width); before={before:?}, after={after:?}"
        );
    }
}

/// Black-box coverage for the VimCode app icon painted left of the `File`
/// menu, VS Code style (#720, part 2 of #716).
///
/// #716 landed the app-*identity* half and explicitly deferred this one:
/// quadraui had no raster paint path at all, so a GTK-side workaround would
/// have been the per-backend hack the Platform-Neutrality Rule forbids. The
/// upstream `Image` primitive + `Backend::draw_image` (quadraui#662) closed
/// that gap; this module covers vimcode's adoption of it.
///
/// The paint is the easy half. A leading element shifts the menu bar's
/// x-origin, so every item's hit-test offset moves with it — see
/// `clicking_file_after_the_app_icon_still_opens_the_file_menu`.
#[cfg(test)]
mod app_icon {
    use super::*;

    // ── App icon left of the File menu (#720) ────────────────────────────

    /// The reserved app-icon slot for the frame the harness last painted,
    /// taken from the *renderer's own* menu row rect through the *same*
    /// `split_menu_row_for_app_icon` the paint uses — never a hardcoded
    /// chrome offset (CLAUDE.md "locate targets, never hardcode
    /// coordinates").
    fn painted_app_icon_rect<A: AppLogic>(h: &Harness<A>) -> quadraui::Rect {
        let row = h.menu_row_rect.get();
        assert!(
            row.width > 0.0 && row.height > 0.0,
            "the menu row must have been laid out by the last frame; got {row:?}"
        );
        crate::render::split_menu_row_for_app_icon(row).0
    }

    /// #720 acceptance 1: the VimCode icon renders left of `File`, at
    /// menu-bar row height.
    ///
    /// Asserts on **pixels**, not on the icon rect being computed: an
    /// unpainted slot is a flat fill (`draw_menu_bar`'s single `tab_bar_bg`
    /// rectangle), so "the slot contains more than one distinct colour" is
    /// exactly the difference between painted artwork and no artwork. The
    /// asset is a blue→cyan gradient on a rounded rect, so a real paint has
    /// many colours; the flat filler has one.
    ///
    /// RED-verified: with the `backend.draw_image(...)` call in
    /// `render_content` removed, the slot stays uniformly `tab_bar_bg` and
    /// the distinct-colour assertion fails (1 colour, not >= 8).
    #[test]
    fn app_icon_paints_left_of_the_file_menu() {
        // #720 review: without a gdk-pixbuf SVG loader on this host
        // (`librsvg2-common` is only a `Recommends` of `libgtk-4-1` on
        // Ubuntu, so a `--no-install-recommends` install can legitimately
        // lack it), `draw_image` cannot decode the app icon and paints
        // nothing -- an environment gap, not a regression in this code. CI
        // installs the loader explicitly (see `.github/workflows/ci.yml`),
        // so skip the pixel assertions rather than hard-failing when it's
        // genuinely absent.
        if crate::gtk::util::cached_app_icon_png().is_none() {
            eprintln!(
                "skipping app_icon_paints_left_of_the_file_menu: no gdk-pixbuf \
                 SVG loader on this host"
            );
            return;
        }

        let mut h = harness(Engine::new_for_test(), 1200, 800);
        h.driver.render();

        let file = h
            .driver
            .find_bounds("File")
            .expect("the File menu-bar header must paint");
        let icon = painted_app_icon_rect(&h);
        let row = h.menu_row_rect.get();

        // (a) The icon slot really is to the *left* of `File`, and `File`
        //     really did shift out of the row's leading edge.
        assert!(
            icon.x + icon.width <= file.x,
            "the app icon must sit entirely left of the File label; \
             icon={icon:?} File={file:?}"
        );
        assert!(
            file.x >= row.x + crate::render::menu_bar_app_icon_slot_width_px(row.height),
            "the File label must start after the reserved icon slot \
             (row={row:?} slot={}); got File={file:?}",
            crate::render::menu_bar_app_icon_slot_width_px(row.height)
        );

        // (b) The icon is sized from the *row height*, not the editor font —
        //     it is square and fits inside the row with vertical breathing
        //     room (#720: "size it to the menu-bar row height").
        assert_eq!(
            icon.width, icon.height,
            "the app icon slot must be square; got {icon:?}"
        );
        assert!(
            icon.y > row.y && icon.y + icon.height < row.y + row.height,
            "the icon must be inset inside the menu row, not flush against \
             its edges; icon={icon:?} row={row:?}"
        );

        // (c) Real pixels landed there.
        let mut colours = std::collections::HashSet::new();
        let x0 = icon.x.ceil() as i32;
        let x1 = (icon.x + icon.width).floor() as i32;
        let y0 = icon.y.ceil() as i32;
        let y1 = (icon.y + icon.height).floor() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                colours.insert(h.driver.pixel(x, y));
            }
        }
        assert!(
            colours.len() >= 8,
            "the app icon slot ({icon:?}) should hold rasterised artwork — a \
             blue→cyan gradient on a rounded rect — but only {} distinct \
             colour(s) were painted there ({colours:?}). One colour means the \
             slot was reserved and background-filled but `draw_image` never \
             put pixels in it (or gdk-pixbuf has no SVG loader on this host).",
            colours.len()
        );
    }

    /// #720 acceptance 2: clicking `File` still opens `File`.
    ///
    /// The leading icon shifts every menu item's x-origin. If the shift
    /// reaches the paint but not `MenuSystem::handle`'s `bar_rect` (the
    /// #552 `TabBar` bug class quadraui's `MenuBar::layout_with_leading`
    /// doc calls out), a click aimed at the *painted* `File` label lands
    /// one slot further right in the unshifted hit-test — i.e. on `Edit`.
    ///
    /// RED-verified: reverting `handle()`'s `bar_rect` to
    /// `self.menu_row_rect` (the pre-#720 full band) makes this open the
    /// *Edit* menu — "Undo" paints instead of "New Tab" — and the test
    /// fails on the `New Tab` assertion.
    #[test]
    fn clicking_file_after_the_app_icon_still_opens_the_file_menu() {
        let mut h = harness(Engine::new_for_test(), 1200, 800);
        h.driver.render();

        let file = h
            .driver
            .find_bounds("File")
            .expect("the File menu-bar header must paint");
        assert!(
            !h.driver.screen_contains("New Tab"),
            "sanity: the File dropdown must be closed before the click"
        );

        h.driver
            .click(file.x + file.width / 2.0, file.y + file.height / 2.0);
        h.driver.render();

        assert!(
            h.driver.screen_contains("New Tab"),
            "clicking the painted File label must open the *File* dropdown \
             (its first entry is \"New Tab\"); painted texts were {:?}",
            h.driver.painted_texts()
        );
        assert!(
            !h.driver.screen_contains("Undo"),
            "the click must not have opened the *Edit* menu — that is the \
             symptom of hit-testing against the unshifted menu row while \
             painting the shifted one; painted texts were {:?}",
            h.driver.painted_texts()
        );
    }
}

#[cfg(test)]
mod clipboard_paste {
    use super::*;
    use quadraui::UiEvent;

    /// #593: `Ctrl+V` did nothing on GTK. quadraui's runner reads the system
    /// clipboard and delivers `UiEvent::ClipboardPaste` straight to
    /// `ShellApp::handle`, unconditionally consuming the keypress — there is
    /// no raw `KeyPressed` fallback for an app that ignores it. Before this
    /// fix, `handle`'s catch-all `_` arm swallowed the event and the paste
    /// vanished. Mirrors TUI's
    /// `bracketed_paste_reaches_the_buffer_via_shell_app`
    /// (`tui_main/shell_app.rs`), which covers the same `Engine::route_paste`
    /// entry point from the other backend.
    ///
    /// Command line chosen as the black-box target (rather than the editor
    /// buffer) because the GTK backend does not `record_painted_text` editor
    /// text — see this module's doc comment — so a buffer paste has no
    /// painted pixels this harness can assert against. The command line
    /// paints through `Surface::CommandLine`, a quadraui primitive whose text
    /// the paint-time recording sink *does* capture (confirmed by the
    /// harness smoke test's `"EXPLORER"` assertion against the same sink).
    /// `route_paste`'s other destinations (search/replace fields, explorer
    /// rename, editor buffer) share this one dispatch arm and are covered at
    /// the engine level instead: `search_input_paste` already has coverage,
    /// and #593 adds `test_explorer_rename_route_paste`
    /// (`core/engine/tests.rs`) for the tree inline-edit target this same
    /// fix newly wires up.
    #[test]
    fn ctrl_v_paste_reaches_the_command_line_via_shell_app() {
        let mut engine = Engine::new_for_test();
        engine.mode = crate::core::Mode::Command;
        let mut h = harness(engine, 1200, 800);

        h.driver
            .dispatch(UiEvent::ClipboardPaste("ZQXW_PASTE_MARKER".to_string()));
        h.driver.render();

        assert!(
            h.driver.screen_contains(":ZQXW_PASTE_MARKER"),
            "UiEvent::ClipboardPaste must route through Engine::route_paste \
             into the command line; painted texts were {:?}",
            h.driver.painted_texts()
        );
    }

    /// #761 / #734 slice 6: `UiEvent::Accelerator` now resolves through the
    /// shared `render::dispatch_panel_accelerator` (with `GtkAccelHost`
    /// supplying GTK's hooks) instead of the deleted, GTK-only
    /// `dispatch_gtk_panel_accelerator` match statement. Dispatches the same
    /// id constant (`render::ACC_COMMAND_PALETTE`) that TUI's
    /// `command_palette_open_intercepts_keys_via_shell_app`
    /// (`tui_main/shell_app.rs`) dispatches through `TuiAccelHost` — the two
    /// tests together are the "same accelerator resolves to the same panel
    /// action on both backends" coverage #761 asks for.
    #[test]
    fn panel_accelerator_opens_command_palette_via_gtk_driver() {
        let engine = Engine::new_for_test();
        let mut h = harness(engine, 1200, 800);

        assert!(
            h.picker_popup().is_none(),
            "no picker popup should have painted before the accelerator fires"
        );

        h.driver.dispatch(UiEvent::Accelerator(
            quadraui::AcceleratorId::new(crate::render::ACC_COMMAND_PALETTE),
            quadraui::Modifiers::default(),
        ));
        h.driver.render();

        assert_eq!(
            h.engine.borrow().picker_source,
            crate::core::engine::PickerSource::Commands,
            "render::ACC_COMMAND_PALETTE must open the Commands picker \
             (the same action id TUI resolves via TuiAccelHost)"
        );
        // `picker_popup()` is written only inside `render_content`'s picker
        // draw branch, so this proves the popup actually painted — not
        // merely that engine state flipped (mirrors
        // `status_bar_segment_click_opens_go_to_line_picker`, #555/#672).
        let (_, _, pw, ph) = h
            .picker_popup()
            .expect("the command palette must actually paint, not just flip engine state");
        assert!(
            pw > 0.0 && ph > 0.0,
            "the painted picker popup must have a non-degenerate rect, got {pw}x{ph}"
        );
    }
}

#[cfg(test)]
mod scrollbar_paint {
    //! #731's re-derivation of #723: this issue deleted 22 Relm4-era widget
    //! handles that were permanently `None` under the ShellApp runner
    //! (`self.overlay`, `self.drawing_area`, and friends), including the
    //! only code that ever created a native `gtk4::Scrollbar` overlay
    //! widget (`App::sync_scrollbar` / `create_window_scrollbars`, guarded
    //! on those same dead handles). That path never ran even once under
    //! ShellApp, so #723's fix (`e02a824`, insetting that native widget
    //! past the minimap strip) was never visible on screen either — see
    //! the doc comment on the `Surface::Editor` push in
    //! `App::render_content` for the full re-diagnosis and where the fix
    //! actually needs to move (quadraui's `gtk::editor::draw_editor`,
    //! which documents that it deliberately skips scrollbars on GTK today
    //! and defers to the very host path this issue just deleted).
    //!
    //! `GtkDriver` paints into an in-memory Cairo `ImageSurface` and can
    //! only ever see Cairo-painted pixels, never native GTK overlay
    //! widgets (see this module's own doc comment) — so it cannot directly
    //! observe "no `gtk4::Scrollbar` was constructed". What it *can*
    //! observe, definitively, is that nothing paints scrollbar-colored
    //! pixels for a window that needs one, which is what a user actually
    //! sees. This test is unchanged by this issue's diff (painting was
    //! already dead beforehand — see the PR description) — it is the
    //! executable evidence for the re-diagnosis above, not a red→green bug
    //! fix: it was verified to also pass unmodified against `develop`
    //! before this issue's changes were applied.
    use super::*;

    /// Buffer with enough lines that the pane cannot show them all — the
    /// same fixture `mod tests`' `engine_with_long_buffer` uses, redefined
    /// here since these `#[cfg(test)] mod`s are siblings, not nested.
    /// Minimap and cursorline off: both default on and each paints real,
    /// non-background content across the right-edge strip this test scans
    /// (the minimap strip directly; cursorline as a full-width highlight
    /// band on the cursor's row) — either would make the assertion below a
    /// false positive unrelated to scrollbars.
    fn engine_with_long_buffer() -> Engine {
        let mut engine = Engine::new();
        let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
        engine.buffer_mut().insert(0, &text);
        engine.settings.minimap = false;
        engine.settings.cursorline = false;
        engine
    }

    /// `true` iff any pixel in `rect` differs from `bg` (coarse grid scan).
    fn region_has_non_background_pixel(
        driver: &mut quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        rect: quadraui::Rect,
        bg: (u8, u8, u8),
    ) -> bool {
        let (x0, y0) = (rect.x.round() as i32, rect.y.round() as i32);
        let (x1, y1) = (
            (rect.x + rect.width).round() as i32,
            (rect.y + rect.height).round() as i32,
        );
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                if driver.pixel(x, y) != bg {
                    return true;
                }
                x += 1;
            }
            y += 2;
        }
        false
    }

    /// No vertical scrollbar affordance is painted along the right edge of
    /// an editor pane that needs one (500 lines in an 900px-tall window).
    /// A native `gtk4::Scrollbar` would be invisible to this test either
    /// way (see module doc), but quadraui's shared rasteriser paints
    /// scrollbars as ordinary Cairo pixels on TUI (`super::draw_scrollbar`
    /// in `quadraui::tui::editor`) — if GTK ever grows the same inline
    /// paint, this test starts failing and must be updated alongside it,
    /// which is exactly the point: it pins today's (lack of) behavior so
    /// that change is deliberate, not silent.
    #[test]
    fn no_scrollbar_pixels_paint_for_an_overflowing_editor_pane() {
        let mut h = harness(engine_with_long_buffer(), 1400, 900);
        let win = h.engine.borrow().active_window_id();
        let rect = {
            let layout = h.screen_layout.borrow();
            layout
                .as_ref()
                .expect("render_content must have painted a ScreenLayout")
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active window must have painted")
                .rect
        };

        let theme = crate::render::Theme::from_name(&h.engine.borrow().settings.colorscheme);
        let bg = {
            let c = crate::render::to_quadraui_color(theme.background);
            (c.r, c.g, c.b)
        };
        let thumb = {
            let c = crate::render::to_quadraui_color(theme.scrollbar_thumb);
            (c.r, c.g, c.b)
        };
        assert_ne!(
            bg, thumb,
            "fixture sanity: the theme's scrollbar thumb color must differ \
             from its background, or a painted scrollbar would be \
             indistinguishable from this test's own baseline"
        );

        // Right-hand strip wide enough to hold any plausible scrollbar
        // (native widgets in the deleted code were ~10-12px; TUI's is one
        // character cell), short of the pane's own left edge. Bottom
        // `line_height` excluded: that row is the per-window status line
        // (window_status_line, on by default), a real, unrelated feature
        // painted in a distinct color across the full pane width.
        const STRIP_W: f64 = 16.0;
        let line_height = h
            .painted_line_height()
            .expect("render_content must publish the painted line height");
        let strip = quadraui::Rect::new(
            (rect.x + rect.width - STRIP_W).max(rect.x) as f32,
            rect.y as f32,
            STRIP_W.min(rect.width) as f32,
            (rect.height - line_height).max(0.0) as f32,
        );

        assert!(
            !region_has_non_background_pixel(&mut h.driver, strip, bg),
            "no scrollbar should be painted on GTK today (#731's \
             re-diagnosis of #723) — if this now fails, GTK has grown a \
             live scrollbar paint and this test's doc comment needs \
             updating to match, not silently deleting"
        );
    }
}

#[cfg(test)]
mod overlay_band_z_order {
    //! #735 slices 1 and 6: the GTK half of the shared frame sequence.
    //!
    //! Frame composition — which surface is laid down, in what order — used to
    //! be transcribed once per backend, and had inverted twice. Both backends
    //! now walk `render::compose_frame` and record what they composed into
    //! `composed_frame`; these tests read that record. #766 folded the overlay
    //! band into that one sequence, so what used to be a separate
    //! `Vec<OverlayOp>` is read here as `composed_frame` filtered to
    //! `FrameOp::is_overlay`.
    //!
    //! **One intrinsic difference, deliberately not converged:** GTK's menu bar
    //! *is* its client-side titlebar, so `App::setup` forces
    //! `engine.menu_bar_visible = true` unconditionally (#552) and the
    //! `MenuDropdown` / `CommandCenter` rungs are therefore always live here.
    //! TUI shows its menu row only in vscode-mode or via Alt. So the fixtures
    //! below turn the menu bar *on* for TUI too, and the two backends then
    //! assert the identical band.
    use super::*;

    // The twin lives in `tui_main/shell_app.rs`
    // (`frame_sequence_*_via_shell_app` / `overlay_band_*_via_shell_app`) and
    // asserts against the **same expected `Vec<FrameOp>`** for the same engine
    // state. A single test cannot drive both backends — the GTK `App` lives in
    // the `vimcode` bin target, `TuiShellApp` in `vcd` — so "both backends emit
    // the same sequence" is expressed as two tests sharing one expected value.
    // Both call `render::frame_sequence_fixture()` /
    // `render::overlay_band_title_bar_only_fixture()` for that value — a
    // single `#[cfg(test)]` fn in `render.rs`, compiled into both bin targets
    // — rather than each transcribing its own `Vec<FrameOp>` literal, so the
    // compiler (not comment discipline) keeps the two expectations in step.

    /// The overlay rungs of a recorded `composed_frame` — the tail `OverlayOp`
    /// used to be its own enum for, before #766 folded it in.
    fn overlay_tail(frame: &[crate::render::FrameOp]) -> Vec<crate::render::FrameOp> {
        frame.iter().copied().filter(|op| op.is_overlay()).collect()
    }

    /// A dialog both backends paint **in-canvas**.
    ///
    /// The `input` field is what forces that: `quadraui::native_dialog_options`
    /// returns `None` for a dialog carrying a text input, so `render_content`'s
    /// `FrameOp::Dialog` arm draws the generic primitive instead of queueing a
    /// real OS `AlertDialog` (#727). A plain button-only dialog would go native
    /// here and never enter the band at all, which would make the cross-backend
    /// comparison compare two different things.
    ///
    /// `tui_main/shell_app.rs`'s `in_canvas_dialog` is the byte-identical twin.
    fn in_canvas_dialog(title: &str) -> crate::core::engine::Dialog {
        crate::core::engine::Dialog {
            title: title.to_string(),
            body: vec!["body line".to_string()],
            buttons: vec![crate::core::engine::DialogButton {
                label: "OK".to_string(),
                hotkey: 'o',
                action: "ok".to_string(),
            }],
            selected: 0,
            tag: String::new(),
            input: Some(crate::core::engine::DialogInput {
                label: "Passphrase".to_string(),
                value: String::new(),
                is_password: true,
            }),
        }
    }

    /// Opens a context menu and an in-canvas modal dialog in the same frame and
    /// asserts the *painted* band is `[ContextMenu, Dialog]` — the dialog on
    /// top, byte-identical to what the TUI twin asserts.
    ///
    /// **RED-verified against unfixed `develop`.** Before #735, GTK's
    /// `render_content` painted `screen.dialog` and *then* `screen.context_menu`
    /// — the inversion this issue exists to remove. Restoring that order (hoist
    /// the `Dialog` arm's body above the `ContextMenu` arm's, out of the
    /// `compose_frame` walk) makes this fail with
    /// `[Dialog, ContextMenu]`, and trips `check_frame_order`'s
    /// `debug_assert` in `render_content` on the way. Restored before
    /// committing.
    #[test]
    fn overlay_band_paints_dialog_above_context_menu_via_gtk_driver() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.open_editor_context_menu(4, 4);
        assert!(
            engine
                .context_menu
                .as_ref()
                .is_some_and(|m| !m.items.is_empty()),
            "fixture needs a non-empty context menu — an empty one is not painted"
        );
        engine.dialog = Some(in_canvas_dialog("ZQXW735DIALOG"));

        let h = harness(engine, 1400, 900);

        assert_eq!(
            overlay_tail(&h.composed_frame.borrow()),
            vec![
                crate::render::FrameOp::MenuDropdown,
                crate::render::FrameOp::CommandCenter,
                crate::render::FrameOp::ContextMenu,
                crate::render::FrameOp::Dialog,
            ],
            "expected band differs from the TUI twin's \
             (`overlay_band_paints_dialog_above_context_menu_via_shell_app`). \
             Two orderings are pinned here: the title-bar chrome below the modal \
             stack (TUI had that inverted before #735) and the dialog above the \
             context menu (GTK had *that* inverted — a modal dialog takes every \
             event once open, per `route_modal_key` / \
             `route_modal_overlay_click`, so it must paint above the menu too)"
        );
        // Paint, not just bookkeeping (#587/#592): a recorded rung that never
        // reached the surface is exactly the failure this repo keeps hitting.
        // `dialog_layout` is set unconditionally in the same match arm right
        // after `frame.draw(backend)`, so checking it alone would stay green
        // even if the draw call never reached the Cairo surface — read the
        // rendered screen instead, exactly like the TUI twin does.
        assert!(
            h.driver.screen_contains("ZQXW735DIALOG"),
            "recorded band claims the dialog painted in-canvas, but its title \
             is not on screen — the recorder and the painter disagree"
        );
        assert!(
            h.driver.screen_contains("Go to Definition"),
            "context menu should still be visible underneath the dialog (they \
             don't overlap in this fixture) — its item label is not on screen"
        );
    }

    /// **#735's headline acceptance criterion, GTK half:** the whole frame, as
    /// one `FrameOp` sequence, must equal what the TUI twin
    /// (`frame_sequence_matches_across_backends_via_shell_app`) records for the
    /// same state.
    ///
    /// Nine rungs live, five absent — the chrome band, the title-bar band and a
    /// context menu under a modal dialog — so the assertion cannot degenerate
    /// into "whatever `FRAME_Z_ORDER` contains". Both halves read
    /// `render::frame_sequence_fixture()`, a single `#[cfg(test)]` fn compiled
    /// into both bin targets, so the compiler keeps the two expectations in
    /// step rather than comment discipline.
    ///
    /// **RED-verified against unfixed `develop`**: this test could not be
    /// written there at all — the chrome and overlay halves were two fields
    /// (`composed_chrome_band`, `painted_overlay_band`) with two order
    /// constants, so there was no single sequence to compare. With the fold in
    /// place, reordering *one rung on one backend* (hoisting `FrameOp::Dialog`'s
    /// arm body above `FrameOp::ContextMenu`'s, out of the `compose_frame`
    /// walk) makes this fail with `[.., Dialog, ContextMenu]` while the TUI
    /// twin still reads `[.., ContextMenu, Dialog]`, and trips
    /// `check_frame_order`'s `debug_assert` in `render_content` on the way.
    /// Re-introduced, observed red, restored before committing.
    #[test]
    fn frame_sequence_matches_across_backends_via_gtk_driver() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.buffer_mut().insert(0, "fn main() {}\n");
        // Explicit, not ambient (#762): a global status bar exists only when
        // per-window status lines are off, and the default is on.
        engine.settings.window_status_line = false;
        // The *settings* panel, not the explorer: its body paints fixed chrome,
        // where the explorer's would be this checkout's own directory listing.
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_SETTINGS,
        ));
        engine.wildmenu_items = vec!["ZQXWwildA".to_string(), "ZQXWwildB".to_string()];
        engine.wildmenu_selected = Some(0);
        engine.open_editor_context_menu(4, 4);
        assert!(
            engine
                .context_menu
                .as_ref()
                .is_some_and(|m| !m.items.is_empty()),
            "fixture needs a non-empty context menu — an empty one is not composed"
        );
        engine.dialog = Some(in_canvas_dialog("ZQXW766DIALOG"));

        let h = harness(engine, 1400, 900);

        assert_eq!(
            *h.composed_frame.borrow(),
            crate::render::frame_sequence_fixture(),
            "expected frame sequence differs from the TUI twin's \
             (`frame_sequence_matches_across_backends_via_shell_app`)"
        );

        // Composition, not just bookkeeping (#587/#592): the rungs the record
        // claims must have reached the Cairo surface.
        assert!(
            h.driver.screen_contains("File"),
            "MenuDropdown was composed but the menu bar never painted"
        );
        assert!(
            h.driver.screen_contains("ZQXWwildA"),
            "Wildmenu was composed but no wildmenu entry painted"
        );
        assert!(
            h.driver.screen_contains("ZQXW766DIALOG"),
            "recorded sequence claims the dialog painted in-canvas, but its \
             title is not on screen — the recorder and the painter disagree"
        );
    }

    /// A frame with no app-level overlay open records only the title-bar
    /// chrome — the recorder is not just "whatever `FRAME_Z_ORDER` contains".
    ///
    /// Guards the caches that must be cleared *before* the walk (stale
    /// `dialog_layout` / `context_menu_layout` / `picker_popup_rect` geometry
    /// is the #587 class of bug) without that being mistaken for a paint. Since
    /// #766 the absent rungs have no arm to run at all, so the clear has to be
    /// unconditional — this is the test that would catch it being folded back
    /// into an `else`. `MenuDropdown` / `CommandCenter` survive because GTK's
    /// menu bar is its titlebar and `setup()` pins it visible (#552) — see the
    /// module doc.
    #[test]
    fn overlay_band_holds_only_the_title_bar_when_no_overlay_is_open_via_gtk_driver() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        let h = harness(engine, 1400, 900);
        assert_eq!(
            overlay_tail(&h.composed_frame.borrow()),
            crate::render::overlay_band_title_bar_only_fixture(),
            "no app-level overlay was open, so only the title-bar rungs should \
             have painted — every other arm ran for its cache-clearing side \
             effect only"
        );
        // Paint, not just bookkeeping: prove the title-bar chrome the vector
        // claims painted actually reached the surface, symmetric with the
        // `screen_contains` check in the sibling test above.
        assert!(
            h.driver.screen_contains("File"),
            "recorded band claims the title-bar/menu row painted, but its \
             \"File\" menu label is not on screen"
        );
    }
}

#[cfg(test)]
mod chrome_band_order {
    //! #763 (#735 slice 2): the GTK half of the shared **chrome band** — the
    //! non-editor surfaces vimcode itself composes around the editor column.
    //!
    //! Before #763 both backends composed the same five rungs in two different
    //! orders (GTK: menu row → … → status bar → wildmenu → command line →
    //! sidebar body; TUI: menu row → sidebar body → … → wildmenu → status bar →
    //! command line), each with its own hand-written gate. Both now walk
    //! `render::compose_frame` and record what they composed into
    //! `composed_frame`; these tests read that record, filtered to the chrome
    //! half (#766 folded the overlay band into the same sequence).
    //!
    //! The TUI half lives in `tui_main/shell_app.rs`
    //! (`chrome_band_*_via_shell_app`) and asserts against the **same expected
    //! `Vec<FrameOp>`** — `render::chrome_band_fixture`, a single
    //! `#[cfg(test)]` fn compiled into both bin targets — for the same engine
    //! state, exactly as the overlay-band pair above does.
    use super::*;

    /// The chrome rungs of a recorded `composed_frame` — everything before the
    /// overlay tail.
    fn chrome_half(frame: &[crate::render::FrameOp]) -> Vec<crate::render::FrameOp> {
        frame
            .iter()
            .copied()
            .filter(|op| !op.is_overlay())
            .collect()
    }

    /// A sidebar-open, wildmenu-up, global-status-bar-on frame: every chrome
    /// rung live, composed in `FRAME_Z_ORDER`.
    ///
    /// **RED against unfixed `develop`**, in two independent ways. (1) GTK
    /// composed the wildmenu *after* the global status line and TUI composed it
    /// *before*, so no single expected vector could satisfy both; swapping the
    /// two arms' bodies back (hoisting `FrameOp::StatusBar`'s body above
    /// `FrameOp::Wildmenu`'s, out of the `compose_frame` walk) makes this fail
    /// with `[.., StatusBar, Wildmenu, ..]` and trips
    /// `check_frame_order`'s `debug_assert` in `render_content` on the
    /// way. (2) GTK composed the sidebar panel body *last*, after the command
    /// line, and measured the menu row *first*, before the editor — hoisting
    /// either arm back out of the walk drops its `FrameOp` from the record
    /// entirely. Both were re-introduced, observed red, and restored before
    /// committing.
    #[test]
    fn chrome_band_composes_in_canonical_order_via_gtk_driver() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        // Explicit, not ambient: a global status bar exists only when
        // per-window status lines are off, and the default is on.
        engine.settings.window_status_line = false;
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_SETTINGS,
        ));
        engine.wildmenu_items = vec!["ZQXWwildA".to_string(), "ZQXWwildB".to_string()];
        engine.wildmenu_selected = Some(0);

        let h = harness(engine, 1400, 900);

        assert_eq!(
            chrome_half(&h.composed_frame.borrow()),
            crate::render::chrome_band_fixture(true),
            "expected chrome band differs from the TUI twin's \
             (`chrome_band_composes_in_canonical_order_via_shell_app`)"
        );

        // Composition, not just bookkeeping (#587/#592): every rung the record
        // claims must have reached the Cairo surface.
        assert!(
            h.driver.screen_contains("File"),
            "MenuRow was composed but the menu bar never painted"
        );
        assert!(
            h.driver.screen_contains("ZQXWwildA"),
            "Wildmenu was composed but no wildmenu entry painted"
        );
        // Not `screen_contains("SETTINGS")`: that literal heading is TUI-only
        // chrome (`panels.rs::render_settings_panel` paints a hardcoded
        // `" SETTINGS"` header via `draw_settings_chrome`). GTK's `PANEL_SETTINGS`
        // arm renders straight into the AppShell-provided content rect with no
        // second header of its own — the sidebar header text above it is
        // AppShell chrome, painted by the runner *before* `render_content` (and
        // for this "bottom:" utility panel it does not track the active body,
        // which is a pre-existing, separate divergence this PR does not touch).
        // "Appearance" is the first `setting_categories()` entry — the same
        // platform-neutral category list both backends' settings forms render
        // through (`render::settings_to_form`) — so its presence proves the
        // settings *form* itself reached the Cairo surface.
        assert!(
            h.driver.screen_contains("Appearance"),
            "SidebarPanel was composed but the settings form never painted; painted: {:?}",
            h.driver.painted_texts()
        );
    }

    /// With no completion up, the `Wildmenu` rung drops out — the record is not
    /// simply "whatever `FRAME_Z_ORDER` contains".
    #[test]
    fn chrome_band_drops_the_wildmenu_rung_when_no_completion_is_up_via_gtk_driver() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.settings.window_status_line = false;
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_SETTINGS,
        ));
        assert!(
            engine.wildmenu_items.is_empty(),
            "fixture needs no wildmenu"
        );

        let h = harness(engine, 1400, 900);
        assert_eq!(
            chrome_half(&h.composed_frame.borrow()),
            crate::render::chrome_band_fixture(false),
            "no completion was up, so the Wildmenu rung must not be composed"
        );
    }

    /// Per-window status lines on ⇒ no global status bar ⇒ the `StatusBar` rung
    /// drops out, and the surviving rungs keep their canonical order.
    ///
    /// GTK's gate for this used to be `screen.global_status_bar.is_some()`
    /// inline; it is `compose_frame`'s now, and TUI's identical gate went with
    /// it.
    #[test]
    fn chrome_band_drops_the_status_bar_rung_with_per_window_status_lines_via_gtk_driver() {
        let mut engine = Engine::new();
        engine.settings.use_nerd_fonts = false;
        engine.settings.window_status_line = true;
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_SETTINGS,
        ));

        let h = harness(engine, 1400, 900);
        let band = h.composed_frame.borrow();
        assert!(
            !band.contains(&crate::render::FrameOp::StatusBar),
            "per-window status lines are on, so no global status bar exists and \
             the StatusBar rung must not be composed; got {band:?}"
        );
        assert_eq!(
            crate::render::check_frame_order(&band),
            Ok(()),
            "the surviving rungs must still be in canonical order"
        );
    }
}

#[cfg(test)]
mod editor_band_order {
    //! #764 (#735 slice 3): the GTK half of the shared **editor band** — the
    //! surfaces vimcode stacks inside `main_content_bounds`.
    //!
    //! Before #764 both backends walked the same run of rungs in two different
    //! orders, and GTK was missing one outright: `ScreenLayout::group_dividers`
    //! was populated every frame and hit-tested for divider drags, and painted
    //! by nobody, so a `Ctrl+W v` boundary on GTK was draggable but invisible.
    //! Both backends now walk `render::compose_editor_band` and record what
    //! they composed into `composed_editor_band`; these tests read that record
    //! *and* the Cairo surface underneath it.
    //!
    //! The TUI half lives in `tui_main/shell_app.rs`
    //! (`editor_band_*_via_shell_app`) and asserts against the **same expected
    //! `Vec<EditorOp>`** — `render::editor_band_fixture`, a single
    //! `#[cfg(test)]` fn compiled into both bin targets — for the same engine
    //! state, exactly as the chrome- and overlay-band pairs above do.
    use super::*;
    use crate::core::window::SplitDirection;

    /// A two-group `Ctrl+W v` split with breadcrumbs and the minimap on and a
    /// tab-hover tooltip up: every editor rung live except the tab-drag ghost,
    /// which needs a live pointer drag.
    ///
    /// Every knob is set explicitly rather than inherited from `Settings
    /// ::default()` (#762) — an ambient default that flips would silently turn
    /// this from a seven-rung assertion into a five-rung one.
    fn engine_with_every_editor_rung() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = false;
        engine.settings.breadcrumbs = true;
        engine.settings.minimap = true;
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.cwd = cwd.clone();
        let buf = engine.active_buffer_id();
        if let Some(state) = engine.buffer_manager.get_mut(buf) {
            state.file_path = Some(cwd.join("src").join("main.rs"));
        }
        engine.buffer_mut().insert(0, "fn main() {}\n");
        // Two editor groups, so `group_dividers` is non-empty — the rung GTK
        // never painted before #764.
        engine.open_editor_group(SplitDirection::Vertical);
        engine.tab_hover_tooltip = Some("ZQXW764TIP".to_string());
        engine
    }

    /// **RED against unfixed `develop`**, in two independent ways. (1) GTK
    /// never composed `EditorOp::GroupDividers` at all — it destructured
    /// `calculate_group_window_rects`'s dividers into `_dividers` and painted
    /// only `window_dividers` — so the record came back one rung short of the
    /// TUI twin's and no single expected vector could satisfy both. (2) GTK
    /// composed its tab-drag ghost ~900 lines further down, after the
    /// editor-anchored popups and the whole chrome band; hoisting that arm
    /// back out of the walk drops `TabDragOverlay` from the record and, with a
    /// drag live, trips `check_editor_band_order`'s `debug_assert` in
    /// `render_content` on the way. Both were re-introduced, observed red, and
    /// restored before committing.
    #[test]
    fn editor_band_composes_in_canonical_order_via_gtk_driver() {
        let h = harness(engine_with_every_editor_rung(), 1400, 900);

        assert_eq!(
            *h.composed_editor_band.borrow(),
            crate::render::editor_band_fixture(false),
            "expected editor band differs from the TUI twin's \
             (`editor_band_composes_in_canonical_order_via_shell_app`)"
        );

        // Composition, not just bookkeeping (#587/#592): the rungs the record
        // claims must have reached the Cairo surface.
        assert!(
            h.driver.screen_contains("ZQXW764TIP"),
            "TabTooltip was composed but the tooltip text never painted"
        );
        assert!(
            h.driver.screen_contains("main.rs"),
            "TabBars/Breadcrumbs were composed but the buffer name never painted"
        );
    }

    /// The rung this slice actually restores on GTK: a `Ctrl+W v` group
    /// boundary must paint a divider line *between* the two groups' painted
    /// tab labels — not merely be hit-testable there.
    ///
    /// Asserts on painted pixels, not on `screen.group_dividers` being
    /// populated: it was populated the whole time, which is exactly why a
    /// state assertion would have passed against the bug (`CLAUDE.md` rule 1).
    /// The divider column is *located* from the two groups' own painted tab
    /// labels rather than hardcoded.
    ///
    /// **RED against unfixed `develop`**: with the `EditorOp::GroupDividers`
    /// arm removed, nothing paints between the panes and the
    /// non-background-pixel scan below finds no column at all. Confirmed by
    /// hand before restoring the arm.
    #[test]
    fn group_divider_paints_between_the_two_groups_via_gtk_driver() {
        let mut h = harness(engine_with_every_editor_rung(), 1400, 900);
        h.driver.render();

        // Locate the boundary from the geometry the frame actually painted,
        // never from hardcoded coordinates (`CLAUDE.md` rule 1).
        let (div_x, mid_y) = {
            let layout = h.screen_layout.borrow();
            let layout = layout.as_ref().expect("a frame must have been painted");
            assert_eq!(
                layout.group_dividers.len(),
                1,
                "fixture must produce exactly one between-group divider"
            );
            let div = &layout.group_dividers[0];
            // The two groups' painted window rects must straddle it — proves
            // `position` is the boundary we think it is before probing there.
            let lefts: Vec<f64> = layout.windows.iter().map(|w| w.rect.x).collect();
            assert!(
                lefts.iter().any(|&x| x < div.position) && lefts.iter().any(|&x| x >= div.position),
                "divider at {} should sit between the two groups' windows \
                 (window lefts: {lefts:?})",
                div.position
            );
            (
                div.position as i32,
                (div.cross_start + div.cross_size / 2.0) as i32,
            )
        };

        // Both pane backgrounds, sampled well clear of the boundary. Reading
        // *both* is what makes this test able to fail: the groups sit on
        // slightly different background tones (an active/inactive pane
        // distinction), so a probe that only knew the left one would score the
        // right pane's own first column as "something painted" and pass
        // against the very bug this asserts is fixed — verified by hand, it
        // did exactly that on the first draft.
        let left_bg = h.driver.pixel(div_x - 40, mid_y);
        let right_bg = h.driver.pixel(div_x + 40, mid_y);
        let line = (div_x - 8..=div_x + 8)
            .map(|x| (x, h.driver.pixel(x, mid_y)))
            .find(|(_, p)| *p != left_bg && *p != right_bg);
        assert!(
            line.is_some(),
            "no group-divider line painted within +/-8px of x={div_x} on row \
             {mid_y}: every pixel there is one of the two pane backgrounds \
             ({left_bg:?} / {right_bg:?}). Before #764 GTK painted nothing here \
             at all while still resolving divider drags against the very same \
             rect — `ScreenLayout::group_dividers` was populated the whole \
             time, which is why a state assertion would have passed against \
             the bug."
        );
    }

    /// A **live** tab drag composes the ghost rung *inside* the editor band.
    ///
    /// **RED against unfixed `develop`**: GTK painted its drop overlay ~900
    /// lines below the editor column, after the editor-anchored popups and the
    /// entire chrome band, so the rung existed nowhere in any walk and the
    /// record could not contain it. That placement also meant a completion
    /// menu or hover popup left open when a drag starts painted *over* the
    /// drop-zone highlight that owns the pointer — see `EDITOR_Z_ORDER`.
    ///
    /// Two moves and no `mouse_up`, so the drag is still live when the frame
    /// under assertion is painted: the first move crosses the travel threshold
    /// and *starts* the drag (`TabDragMove::Crossed` -> `TabDragState::begin`),
    /// the second is the first one tracked into a drop zone.
    #[test]
    fn live_tab_drag_composes_the_ghost_rung_inside_the_band_via_gtk_driver() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_764_gtk_tab_drag_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("zqa764.txt");
        let b = dir.join("zqb764.txt");
        std::fs::write(&a, "a\n").unwrap();
        std::fs::write(&b, "b\n").unwrap();

        let mut engine = Engine::new();
        engine.new_tab(Some(&a));
        engine.new_tab(Some(&b));
        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        // Match the trailing space so `find_bounds` resolves to the *tab*
        // label rather than the per-window status line 800px lower down (the
        // same disambiguation `tab_drag_past_a_neighbour_...` documents).
        let bounds = |h: &Harness<_>, name: &str| {
            h.driver
                .find_bounds(&format!("{name}.txt "))
                .unwrap_or_else(|| panic!("tab label for {name} must be painted"))
        };
        let (from, to) = {
            let (ra, rb) = (bounds(&h, "zqa764"), bounds(&h, "zqb764"));
            let (l, r) = if ra.x < rb.x { (ra, rb) } else { (rb, ra) };
            (
                (l.x + l.width / 2.0, l.y + l.height / 2.0),
                (r.x + r.width / 2.0, r.y + r.height / 2.0),
            )
        };
        h.driver.mouse_down(from.0, from.1);
        h.driver.mouse_move(to.0, to.1);
        h.driver.mouse_move(to.0, to.1);
        h.driver.render();

        let band = h.composed_editor_band.borrow();
        assert!(
            band.contains(&crate::render::EditorOp::TabDragOverlay),
            "a drag is live, so the ghost rung must be composed inside the \
             editor band; got {band:?}"
        );
        assert_eq!(
            crate::render::check_editor_band_order(&band),
            Ok(()),
            "the ghost rung must land in canonical position, not wherever the \
             old hoisted-out paint site happened to sit"
        );
        drop(band);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A single-group frame composes no `GroupDividers` rung — the record is
    /// not just "whatever `EDITOR_Z_ORDER` contains", and the gate is real.
    #[test]
    fn unsplit_editor_composes_no_group_divider_rung_via_gtk_driver() {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = false;
        engine.buffer_mut().insert(0, "fn main() {}\n");

        let h = harness(engine, 1400, 900);
        let band = h.composed_editor_band.borrow();
        assert!(
            !band.contains(&crate::render::EditorOp::GroupDividers),
            "one editor group means no between-group boundary, so the rung \
             must not be composed; got {band:?}"
        );
        assert!(
            !band.contains(&crate::render::EditorOp::TabDragOverlay),
            "no drag is live, so the ghost rung must not be composed; got {band:?}"
        );
        assert_eq!(
            crate::render::check_editor_band_order(&band),
            Ok(()),
            "the surviving rungs must still be in canonical order"
        );
    }
}

/// #765 / #735 slice 4: the shared **bottom band**.
///
/// Both backends now walk `render::compose_bottom_band` and record what they
/// composed into `composed_bottom_band`; these tests read that record and the
/// pixels behind it.
///
/// The TUI half lives in `tui_main/shell_app.rs` (`bottom_band_*_via_shell_app`)
/// and asserts against the **same expected `Vec<BottomOp>`** —
/// `render::bottom_band_fixture`, a single `#[cfg(test)]` fn compiled into both
/// bin targets — for the same engine state, exactly as the chrome-, editor- and
/// overlay-band pairs above do.
#[cfg(test)]
mod bottom_band_order {
    use super::*;

    /// Quickfix open with an item, the bottom panel up on Debug Output, the
    /// debug toolbar visible and the per-window status line extracted: every
    /// stacked bottom rung live. The hover popup needs a live sidebar dwell and
    /// is covered separately below.
    ///
    /// Every knob is set explicitly rather than inherited from `Settings
    /// ::default()` (#762). Mirrors `shell_app.rs`'s
    /// `app_with_every_bottom_rung`.
    fn engine_with_every_bottom_rung() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = false;
        // `separated_status_line` is `Some` only for
        // `window_status_line && !status_line_above_terminal && panel open`.
        engine.settings.window_status_line = true;
        engine.settings.status_line_above_terminal = false;
        engine
            .quickfix_items
            .push(crate::core::project_search::ProjectMatch {
                file: std::path::PathBuf::from("zqxw765.rs"),
                line: 0,
                col: 0,
                line_text: "ZQXW765QF".to_string(),
            });
        engine.quickfix_open = true;
        engine.bottom_panel_open = true;
        engine.bottom_panel_kind = crate::render::BottomPanelKind::DebugOutput;
        engine.dap_output_lines.push("ZQXW765DBG".to_string());
        engine.debug_toolbar_visible = true;
        engine
    }

    /// **RED against unfixed `develop`**, in two independent ways. (1) This
    /// backend composed `SeparatedStatus` *fourth* while TUI composed it
    /// *second*, so no single expected vector could satisfy both backends at
    /// once — the record here and the record in the TUI twin disagreed. (2) The
    /// gate for `BottomPanel` was `el.terminal_h > 0.0` here and
    /// `chrome.bottom_panel.height > 0` there, two restatements of
    /// `bottom_panel_is_drawn` free to drift from it and from each other.
    /// Hoisting any arm back out of the walk drops its `BottomOp` from this
    /// record and trips `check_bottom_band_order`'s `debug_assert` in
    /// `render_content` on the way. Re-introduced, observed red, restored.
    #[test]
    fn bottom_band_composes_in_canonical_order_via_gtk_driver() {
        let h = harness(engine_with_every_bottom_rung(), 1400, 900);

        assert_eq!(
            *h.composed_bottom_band.borrow(),
            crate::render::bottom_band_fixture(false),
            "expected bottom band differs from the TUI twin's \
             (`bottom_band_composes_in_canonical_order_via_shell_app`)"
        );

        // Composition, not just bookkeeping (#587/#592): the rungs the record
        // claims must have reached the Cairo surface, mirroring the TUI twin's
        // `find_bounds("ZQXW765QF")` / `find_bounds("ZQXW765DBG")` checks.
        assert!(
            h.driver.screen_contains("ZQXW765QF"),
            "Quickfix was composed but the quickfix item text never painted"
        );
        assert!(
            h.driver.screen_contains("ZQXW765DBG"),
            "BottomPanel (Debug Output) was composed but the debug output \
             line never painted"
        );
    }

    /// The panel-hover popup is composed as the **last** bottom rung, at the
    /// top level of the band walk — not nested inside the sidebar's own chrome
    /// rung, which is where this backend used to keep it.
    ///
    /// **RED against unfixed `develop`**: there was no top-level hover rung to
    /// record at all. The paint lived inside the `FrameOp::SidebarPanel` arm,
    /// under `if let Some(q_sb) = layout.sidebar_content_bounds`, so nothing
    /// observable said whether it had run — and, more to the point, **its two
    /// cache resets lived there too**. `panel_hover_popup_rect` is what
    /// `handle_mouse_press`'s modal arbitration hit-tests clicks against, so
    /// any frame that skipped the sidebar rung left the router pointed at a
    /// popup the frame had not painted — the #587/#592 input-vs-paint shape.
    /// Hoisting the clears to before the walk (where `compose_bottom_band`
    /// cannot gate them off) is the structural half of the fix; this test
    /// pins the composition half.
    ///
    /// Note the reachability limit this test does *not* cover: the GTK shell's
    /// sidebar visibility lives in the shell adapter's own `AppShell`, not in
    /// `engine.app_shell`, so `GtkDriver` cannot collapse the sidebar mid-run
    /// to exercise the stale-cache path end to end. The clear is therefore
    /// covered structurally rather than by a failing-then-passing assertion.
    #[test]
    fn panel_hover_composes_as_the_last_bottom_rung_via_gtk_driver() {
        let mut engine = engine_with_every_bottom_rung();
        engine.show_panel_hover(
            "source_control",
            "item0",
            0,
            "**M** `src/main.rs` — modified",
        );
        let h = harness(engine, 1400, 900);

        assert_eq!(
            *h.composed_bottom_band.borrow(),
            crate::render::bottom_band_fixture(true),
            "a live sidebar dwell must add `PanelHover` to the band, after \
             every stacked rung"
        );

        // Composition, not just bookkeeping (#587/#592): the rung the record
        // claims must have reached the surface. `panel_hover_popup_rect` is
        // the rect the *paint* published, and the same cache the click router
        // reads.
        let rect = h
            .panel_hover_popup_rect
            .get()
            .expect("the composed hover rung must publish the rect it painted");
        assert!(
            rect.2 > 0.0 && rect.3 > 0.0,
            "the popup must paint a non-degenerate box, got {rect:?}"
        );
    }
}

#[cfg(test)]
mod modal_rung {
    use super::*;

    fn small_engine() -> Engine {
        let mut engine = Engine::new();
        engine
            .buffer_mut()
            .insert(0, "fn main() {\n    println!(\"hi\");\n}\n");
        engine
    }

    // ─── #751: the modal rung, finished (context menu / picker / find-replace)
    //
    // Three rungs that #733 slice 1 left transcribed per backend now go through
    // `render::route_modal_overlay_click`. Each test below has a TUI twin in
    // `src/tui_main/shell_app.rs`.

    /// Background colour of the menu row that paints `label`, sampled just
    /// inside the menu's left edge so the probe lands on the row's fill rather
    /// than on glyph pixels (`CLAUDE.md` rule 1: probe pixels, never hardcode
    /// coordinates).
    fn context_menu_row_bg<A: quadraui::AppLogic>(h: &mut Harness<A>, label: &str) -> (u8, u8, u8) {
        let bounds = h
            .driver
            .find_bounds(label)
            .unwrap_or_else(|| panic!("context menu must paint a {label:?} item"));
        let menu = h
            .context_menu_layout
            .borrow()
            .as_ref()
            .map(|l| l.bounds)
            .expect("a painted context menu must publish its layout");
        let x = (menu.x + 2.0) as i32;
        let y = (bounds.y + bounds.height / 2.0) as i32;
        h.driver.pixel(x, y)
    }

    /// #373 / #751: hovering a context-menu item must move the highlight onto
    /// it. Asserted on painted pixels — the hovered row's background changes,
    /// and the row that *was* selected loses its highlight.
    ///
    /// **RED-verified against unfixed `develop`.** GTK's `UiEvent::MouseMoved`
    /// arm did nothing at all unless the left button was held (it went straight
    /// to `handle_mouse_drag_msg`), so there was no hover rung on this backend:
    /// whichever item was selected when the menu opened stayed highlighted
    /// wherever the pointer went. Deleting the `!buttons.left` hover block in
    /// `App::handle` reproduces that and fails both assertions below. Restored
    /// before committing.
    #[test]
    fn context_menu_hover_moves_the_highlight_via_gtk_driver() {
        let mut engine = small_engine();
        engine.open_editor_context_menu(700, 400);
        let mut h = harness(engine, 1400, 900);

        // Whichever row the menu opens on, plus a second always-enabled row
        // that is definitely not it.
        let (selected_label, target_label) = {
            let engine = h.engine.borrow();
            let menu = engine.context_menu.as_ref().unwrap();
            let selected = menu.items[menu.selected].label.clone();
            let other = menu
                .items
                .iter()
                .find(|i| i.enabled && i.label != selected && !i.label.is_empty())
                .expect("the editor context menu must offer a second enabled item")
                .label
                .clone();
            (selected, other)
        };
        assert_ne!(selected_label, target_label);

        let target = h
            .driver
            .find(&target_label)
            .unwrap_or_else(|| panic!("the context menu must paint {target_label:?}"));
        let selected_bg_before = context_menu_row_bg(&mut h, &selected_label);
        let target_bg_before = context_menu_row_bg(&mut h, &target_label);
        assert_ne!(
            selected_bg_before, target_bg_before,
            "sanity: the selected row must already paint differently from an \
             unselected one, otherwise this test cannot see a highlight move"
        );

        h.driver.dispatch(quadraui::UiEvent::MouseMoved {
            position: quadraui::Point::new(target.0, target.1),
            buttons: quadraui::ButtonMask::default(),
        });
        h.driver.render();

        assert_eq!(
            context_menu_row_bg(&mut h, &target_label),
            selected_bg_before,
            "the hovered row must paint with the selection background the \
             previously selected row had — GTK painted no hover highlight at \
             all before #751 (#373)"
        );
        assert_eq!(
            context_menu_row_bg(&mut h, &selected_label),
            target_bg_before,
            "the previously selected row must lose its highlight when a \
             sibling is hovered (#373)"
        );
    }

    /// #751: the find/replace overlay must be clickable where it is *painted*.
    ///
    /// The panel is anchored to the active editor group
    /// (`quadraui::gtk::find_replace` uses `group_bounds.x + group_bounds.width`),
    /// but this backend hit-tested against the drawing-area width and a
    /// `line_height * 2.5` top edge. With the sidebar open those differ by the
    /// activity-bar + sidebar width, so the clickable panel sat a couple of
    /// hundred pixels left of the visible one. Both now come from
    /// `render::FindReplaceHitGeometry::from_panel`.
    ///
    /// **RED-verified against unfixed `develop`.** With the sidebar open the
    /// click on the painted toggle missed the old hit rect entirely and fell
    /// through to the editor, leaving the toggle's pixels unchanged.
    #[test]
    fn find_replace_toggle_click_lands_where_the_panel_painted_via_gtk_driver() {
        let mut engine = small_engine();
        engine.find_replace_open = true;
        engine.find_replace_query = "ZQXW751FR".to_string();
        let mut h = harness(engine, 1400, 900);
        assert!(
            h.driver.screen_contains("ZQXW751FR"),
            "fixture must actually paint the find/replace panel; painted: {:?}",
            h.driver.painted_texts()
        );
        assert!(
            h.painted_sidebar_bounds.get().is_some(),
            "fixture needs the sidebar open — that is the offset the old \
             drawing-area-width hit test dropped"
        );

        // `×` is painted three times on this frame (tab close, window close,
        // and the panel's own) so it cannot disambiguate the panel; `Aa` is
        // unique to it. Clicking the case-sensitivity toggle flips it, and
        // `quadraui::gtk::find_replace` fills an *active* toggle with the
        // accent colour and inverts its label — a rendered change, not a
        // state read.
        let toggle = h
            .driver
            .find_bounds("Aa")
            .expect("the panel must paint its Aa case-sensitivity toggle");
        // Sample the whole toggle cell rather than one pixel: the fill, the
        // border stroke and the glyph all change together, and which of the
        // three a single probe lands on depends on the font.
        let sample = |h: &mut Harness<_>| {
            let mut px = Vec::new();
            let (x0, x1) = ((toggle.x - 4.0) as i32, (toggle.x + toggle.width) as i32);
            let (y0, y1) = (toggle.y as i32, (toggle.y + toggle.height) as i32);
            for y in y0..y1 {
                for x in x0..x1 {
                    px.push(h.driver.pixel(x, y));
                }
            }
            px
        };
        let before = sample(&mut h);

        h.driver.click(
            toggle.x + toggle.width / 2.0,
            toggle.y + toggle.height / 2.0,
        );
        h.driver.render();

        assert_ne!(
            sample(&mut h),
            before,
            "clicking the painted Aa toggle must repaint it as active; its \
             pixels are unchanged, so the click hit-tested somewhere the panel \
             is not"
        );
    }

    /// #751: clicking the picker row that is already selected confirms it, the
    /// behaviour TUI has always had (`render::apply_picker_row_click`).
    ///
    /// **RED-verified against unfixed `develop`.** GTK's picker block only ever
    /// did `picker_selected = clicked_idx; picker_load_preview()`, so a second
    /// click on the same row was a no-op and the palette stayed open — a GTK
    /// user had to click and then press Enter.
    #[test]
    fn second_click_on_a_picker_row_confirms_it_via_gtk_driver() {
        let mut engine = small_engine();
        // A short, self-contained item list: confirming a row swaps the
        // buffer's line-ending style and closes the palette, with no file
        // system or LSP dependency.
        engine.open_picker(crate::core::engine::PickerSource::LineEndings);
        let mut h = harness(engine, 1400, 900);
        assert!(
            h.picker_popup().is_some(),
            "fixture must actually paint the command palette"
        );
        assert_eq!(
            h.engine.borrow().picker_selected,
            0,
            "fixture assumes the palette opens on row 0, so row 1 is a \
             not-yet-selected row"
        );
        let row = h
            .picker_row_center(1)
            .expect("the palette must paint at least two result rows");

        h.driver.click(row.0, row.1);
        h.driver.render();
        assert!(
            h.picker_popup().is_some(),
            "the first click only selects — the palette must still be painted"
        );

        h.driver.click(row.0, row.1);
        h.driver.render();
        assert!(
            h.picker_popup().is_none(),
            "a second click on the already-selected row must confirm it and \
             close the palette (parity with TUI); painted: {:?}",
            h.driver.painted_texts()
        );
    }
}

/// #815 — adopting `quadraui::FolderPickerController` on GTK, replacing the
/// *native* `gtk4::FileDialog` `open_folder_dialog` used to open.
///
/// The GTK half of the cross-backend pair; the TUI half lives in
/// `tui_main::shell_app`'s `folder_picker_*_via_shell_app` tests.
#[cfg(test)]
mod folder_picker {
    use super::*;
    use crate::core::engine::PickerSource;

    /// Confirming "File: Open Folder…" from the command palette must open
    /// the shared `Palette`-based picker instead of a native chooser this
    /// headless harness can never see (no `gtk4::Window` — see this module's
    /// own doc, "No window"), and navigating + confirming an entry there
    /// must actually call `Engine::open_folder`.
    ///
    /// RED against unfixed `develop`: pre-#815 `App::open_folder_dialog`
    /// called `gtk4::FileDialog::select_folder`, so the picker this test
    /// looks for did not exist as a paintable surface at all — this would
    /// fail at the first `screen_contains("Open Folder")` assertion below,
    /// after the command palette had already closed with nothing else
    /// painted in its place.
    #[test]
    fn command_palette_confirm_opens_the_shared_picker_which_navigates_and_confirms() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.open_picker(PickerSource::CommandCenter);
        let starting_cwd = engine
            .cwd
            .canonicalize()
            .unwrap_or_else(|_| engine.cwd.clone());

        let mut h = harness(engine, 1400, 900);
        assert!(
            h.driver.screen_contains("Show and Run Commands"),
            "fixture must open the command palette on its default \
             (prefix-picker) landing page; painted: {:?}",
            h.driver.painted_texts()
        );

        // ">" switches the unified picker into "Show and Run Commands" mode
        // (`picker_filter_command_center`'s prefix routing) — only then does
        // the flat `PaletteCommand` list (which "File: Open Folder…" lives
        // in) become the thing being filtered. Type-to-filter down to the
        // one item, the same way a user would.
        h.driver.type_char('>');
        for c in "Open Folder".chars() {
            h.driver.type_char(c);
        }
        h.driver.render();
        assert!(
            h.driver.screen_contains("File: Open Folder"),
            "typing must reach the picker's query and keep the matching \
             item visible; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.press_named(quadraui::NamedKey::Enter);
        h.driver.render();
        assert!(
            !h.driver.screen_contains("File: Open Folder"),
            "confirming must close the command palette; painted: {:?}",
            h.driver.painted_texts()
        );
        assert!(
            h.driver.screen_contains("Open Folder"),
            "confirming \"File: Open Folder…\" must dispatch \
             `EngineAction::OpenFolderDialog`, which `App::open_folder_dialog` \
             (#815) now answers by opening the shared \
             `FolderPickerController` palette instead of a native \
             `gtk4::FileDialog` this harness could never see; painted: {:?}",
            h.driver.painted_texts()
        );

        // Entries sort as ["..", ".", ...] — move down once to reach "."
        // (the engine's own cwd), then confirm it.
        h.driver.press_named(quadraui::NamedKey::Down);
        h.driver.press_named(quadraui::NamedKey::Enter);
        h.driver.render();

        assert!(
            !h.driver.screen_contains("Open Folder "),
            "confirming an entry must close the folder picker; painted: {:?}",
            h.driver.painted_texts()
        );
        let confirmed_cwd = {
            let engine = h.engine.borrow();
            engine
                .cwd
                .canonicalize()
                .unwrap_or_else(|_| engine.cwd.clone())
        };
        assert_eq!(
            confirmed_cwd, starting_cwd,
            "confirming \".\" must call `Engine::open_folder` with the \
             picker's root, round-tripping back to the same directory"
        );
    }
}

/// #752 / #733 slice 2 — the chrome rung: breadcrumbs, the three status
/// bands, and the global status bar, all sequenced by
/// `render::route_chrome_click` and shared verbatim with TUI's `handle_mouse`.
///
/// The GTK half of the cross-backend pair; the TUI half lives in
/// `tui_main::shell_app`'s `chrome_rung_*_via_shell_app` tests.
#[cfg(test)]
mod chrome_rung {
    use super::*;

    // ── #752: the shared chrome rung ──────────────────────────────────────

    /// An engine whose global (bottom-of-screen) status bar is painted, with
    /// a git branch decorated by ahead/behind arrows and a filename carrying
    /// several non-ASCII characters.
    ///
    /// Both are load-bearing, not colour: `↑`/`↓` are three UTF-8 bytes each
    /// and every accented Latin letter is two, while all of them occupy one
    /// monospace column. That gap between `len()` and `chars().count()` is
    /// exactly the drift #752 fixes, and a plain-ASCII fixture would make this
    /// test pass against the bug.
    ///
    /// The accented letters are Latin-1, deliberately: CJK would drift the
    /// byte count faster but is *double-width* when painted, which would break
    /// the uniform-advance arithmetic `global_status_left_run` relies on.
    fn engine_with_decorated_global_status_bar() -> Engine {
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut engine = Engine::new_for_test();
        engine.cwd = cwd.clone();
        // Off => the *global* bar is the one that paints (`build_screen_layout`).
        engine.settings.window_status_line = false;
        let buf = engine.active_buffer_id();
        if let Some(st) = engine.buffer_manager.get_mut(buf) {
            st.file_path = Some(cwd.join("rés-café-naïve-über.rs"));
        }
        engine.git_branch = Some("féature".to_string());
        engine.sc_ahead = 2;
        engine.sc_behind = 1;
        engine
    }

    /// The painted global status bar's left run: `(text, bounds)` for the one
    /// Pango run that carries `needle`.
    ///
    /// Deliberately reads the run back out of the recorded paint rather than
    /// re-deriving it from `build_status_line`: a test that located the branch
    /// with the same range the production hit-test uses would agree with the
    /// bug and could never go red (`CLAUDE.md` Testing rule 2).
    fn global_status_left_run<A: AppLogic>(
        h: &Harness<A>,
        needle: &str,
    ) -> (String, quadraui::Rect) {
        let text = h
            .driver
            .painted_texts()
            .into_iter()
            .find(|t| t.contains(needle))
            .unwrap_or_else(|| {
                panic!(
                    "the global status bar must paint a run containing {needle:?}; \
                     painted: {:?}",
                    h.driver.painted_texts()
                )
            })
            .to_string();
        let bounds = h
            .driver
            .find_bounds(needle)
            .expect("the run located above must have recorded bounds");
        (text, bounds)
    }

    /// Absolute centre of character `char_idx` within a painted monospace run.
    fn char_center(text: &str, bounds: quadraui::Rect, char_idx: usize) -> (f32, f32) {
        let advance = bounds.width / text.chars().count().max(1) as f32;
        (
            bounds.x + (char_idx as f32 + 0.5) * advance,
            bounds.y + bounds.height / 2.0,
        )
    }

    /// #752: clicking the git branch in the **global** status bar opens the
    /// branch picker — and does so at the columns the branch was *painted* at.
    ///
    /// # Why this is red against unfixed `develop`
    ///
    /// Two independent defects, both in the ~60 lines this replaced:
    ///
    ///  1. `build_status_line` measured the branch's range with `prefix.len()`
    ///     — UTF-8 **bytes** — and GTK compared a character column against it.
    ///     This fixture's filename carries five non-ASCII letters ahead of the
    ///     branch, so the clickable range started five columns right of the
    ///     painted one. A click on the branch's opening `[` fell into the gap
    ///     and did nothing.
    ///  2. The column was derived with `cached_char_width`, not the width the
    ///     frame actually painted with (the #751 bug, one band lower).
    ///
    /// Restore either and this goes red: the picker never opens.
    ///
    /// Asserts on rendered output at both ends — the click target comes from
    /// the recorded paint, and the verdict is the palette's own painted title,
    /// never `engine.picker_open`.
    #[test]
    fn global_status_branch_click_lands_where_the_branch_painted_via_gtk_driver() {
        let mut h = harness(engine_with_decorated_global_status_bar(), 1600, 900);
        h.driver.render();

        let (text, bounds) = global_status_left_run(&h, "[féature");
        let open_bracket = text
            .chars()
            .position(|c| c == '[')
            .expect("the branch decoration paints as ` [branch …]`");

        // The `[` itself: the first column of the branch, and the one the byte
        // -vs-column drift pushes out of range first.
        let (x, y) = char_center(&text, bounds, open_bracket);
        h.driver.click(x, y);
        h.driver.render();

        // Assert on the palette's own painted chrome, not on the branch name —
        // that string is already on screen in the status bar this test clicked,
        // so matching it would pass without any picker at all.
        assert!(
            h.driver.screen_contains("Switch Branch"),
            "clicking the painted branch must open the branch picker, whose \
             painted title is its own evidence; painted: {:?}",
            h.driver.painted_texts()
        );
        assert_eq!(
            h.engine.borrow().picker_source,
            crate::core::engine::PickerSource::GitBranches,
            "the branch segment must route to the GitBranches picker"
        );
    }

    /// #752 companion: a click on the global status bar that is *not* on the
    /// branch must be consumed by the bar — no cursor move, no picker.
    ///
    /// **Not RED against unfixed `develop`**, and deliberately kept anyway:
    /// it is the negative half of the pair above. The bar is now routed by
    /// `render::route_chrome_click`, whose `ChromeRoute::StatusBar` arm
    /// consumes every pixel of a status band. Without it, widening the
    /// branch segment's zone — or feeding the band a rect that is too tall —
    /// would make the test above pass while quietly stealing clicks from
    /// whatever the bar overlaps. That failure mode is invisible to a test
    /// that only ever checks the positive case.
    #[test]
    fn global_status_bar_consumes_non_branch_clicks_via_gtk_driver() {
        let mut engine = engine_with_decorated_global_status_bar();
        engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\ndelta\n");
        let mut h = harness(engine, 1600, 900);
        h.driver.render();

        let before = {
            let e = h.engine.borrow();
            (e.cursor().line, e.cursor().col)
        };
        let (text, bounds) = global_status_left_run(&h, "NORMAL");
        // Column 1 — inside ` -- NORMAL …`, far left of the branch.
        let (x, y) = char_center(&text, bounds, 1);
        h.driver.click(x, y);
        h.driver.render();

        let after = {
            let e = h.engine.borrow();
            (e.cursor().line, e.cursor().col)
        };
        assert_eq!(
            after, before,
            "a click on the status bar's mode segment must not move the editor cursor"
        );
        assert!(
            !h.engine.borrow().picker_open,
            "…and must not open the branch picker either"
        );
    }
}

/// #816: GTK adopts quadraui#705's `CommandLineLayout::hit_test` for
/// click-to-position and drag-to-select over the command/message line — the
/// GTK half of #194, which landed the TUI side only (the primitive this
/// needed didn't exist yet). `engine.command_line_rect` (cached at paint
/// time, mirroring `global_status_rect`) plus `render::command_line_click_char_idx`
/// are the same helpers TUI's `mouse::handle_mouse` uses, so this is thin
/// wiring through the shared primitive rather than new GTK-specific
/// selection code — see `handle_mouse_click_msg`'s "Command line click" rung.
///
/// **What's covered / what isn't:** click-to-reposition-cursor and
/// drag-to-select (`engine.cmd_sel`/`cmd_dragging`) both land, and
/// `route_cmdline_selection_key` (already shared with TUI) makes Ctrl+C
/// copy the selection. The VISIBLE selection highlight does not — quadraui's
/// `CommandLine` primitive has no selection field and `Backend::draw_command_line`
/// paints in the command line's own (monospace) font, so there is no
/// platform-neutral way to overlay a highlight without either painting in
/// the wrong font (`Backend::draw_status_bar` sets its own chrome font) or
/// writing GTK-specific Cairo code, which `CLAUDE.md`'s Platform-Neutrality
/// Rule forbids. That gap needs a quadraui issue (`CommandLine::selection`,
/// painted by each backend's own rasteriser) before the highlight can land.
#[cfg(test)]
mod command_line_selection {
    use super::*;
    use quadraui::Backend as _;

    /// Pixel center of column `col` (0-based, `:` is column 0) in the
    /// command line, using the *same* `command_line_rect` +
    /// `Backend::char_width` the production click/drag handlers hit-test
    /// against — not a Pango-measured text run. Using `find_bounds` instead
    /// would silently assume the driver's real glyph advance matches
    /// `char_width()`'s value exactly, which this headless harness has no
    /// reason to guarantee (quadraui#705's own paint/click round-trip test
    /// forces a specific monospace font for exactly this reason).
    fn col_center<A: AppLogic>(h: &Harness<A>, col: usize) -> (f32, f32) {
        let rect = h.engine.borrow().command_line_rect.get();
        let char_w = h.driver.backend().char_width();
        (
            rect.x + char_w * (col as f32 + 0.5),
            rect.y + rect.height / 2.0,
        )
    }

    /// Click-to-reposition asserts on the **painted cursor glyph** moving,
    /// not on `command_cursor` alone (#587/#592: a state-only assertion
    /// passes even if painting silently stopped consuming the field).
    #[test]
    fn click_in_command_line_repositions_the_painted_cursor() {
        let mut engine = Engine::new_for_test();
        engine.mode = crate::core::Mode::Command;
        engine.command_buffer = "wq".to_string();
        engine.command_cursor = 2; // starts at the end, after "wq"
        let mut h = harness(engine, 1200, 800);
        h.driver.render();

        let bounds = h
            .driver
            .find_bounds(":wq")
            .expect("command line text must paint");
        let row_y = (bounds.y + bounds.height / 2.0).round() as i32;
        // The cursor glyph paints at `text_origin_x + text_w(anchor)`; at
        // `command_cursor == 2` the anchor is the full ":wq", so it sits at
        // the right edge of the painted text.
        let end_x = (bounds.x + bounds.width + 1.0).round() as i32;
        let pixel_before = h.driver.pixel(end_x, row_y);

        // Click on column 1 ('w') — should move the cursor to
        // `command_cursor == 0`, well left of the end.
        let (cx, cy) = col_center(&h, 1);
        h.driver.click(cx, cy);
        h.driver.render();

        assert_eq!(
            h.engine.borrow().command_cursor,
            0,
            "clicking column 1 ('w') must move the command cursor there"
        );

        let pixel_after = h.driver.pixel(end_x, row_y);
        assert_ne!(
            pixel_before, pixel_after,
            "the painted cursor glyph must move away from the end of the \
             text once the click relocated it"
        );
    }

    /// Drag-selecting over the command line arms `cmd_sel`/`cmd_dragging`
    /// (mouse-populated, keyboard-cleared — `render::route_cmdline_selection_key`,
    /// already shared with TUI) and Ctrl+C copies the selected substring to
    /// the clipboard, exactly like TUI's
    /// `handle_key_pressed_cmd_sel_ctrl_c_copies_and_clears`
    /// (`tui_main/shell_app.rs`). This is the "real, observable effect"
    /// half of #816's GTK coverage — the copied text is asserted directly,
    /// not `cmd_sel`'s presence.
    #[test]
    fn drag_select_then_ctrl_c_copies_the_command_buffer_substring() {
        let mut engine = Engine::new_for_test();
        engine.mode = crate::core::Mode::Command;
        engine.command_buffer = "wq!".to_string();
        engine.command_cursor = 3;
        let copied = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        let copied_hook = std::rc::Rc::clone(&copied);
        engine.clipboard_write = Some(Box::new(move |text: &str| {
            *copied_hook.borrow_mut() = Some(text.to_string());
            Ok(())
        }));
        let mut h = harness(engine, 1200, 800);
        h.driver.render();
        // Prime `command_line_rect`/`char_width` before computing click
        // columns — both are populated by this first paint.
        h.driver
            .find_bounds(":wq!")
            .expect("command line text must paint");

        // Press then drag within column 1 ('w') to column 2 ('q') — per
        // `route_cmdline_selection_key`'s existing (inclusive) contract,
        // `sel = (1, 2)` selects the characters AT both columns: "wq".
        let (x0, y0) = col_center(&h, 1);
        let (x1, _) = col_center(&h, 2);
        h.driver.drag(x0, y0, x1, y0);
        h.driver.render();

        assert!(
            h.engine.borrow().cmd_sel.get().is_some(),
            "a drag over the command line must arm a selection"
        );

        h.driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('c'),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
            repeat: false,
        });

        assert_eq!(
            copied.borrow().as_deref(),
            Some("wq"),
            "Ctrl+C over the drag-selected span must copy exactly the \
             selected substring"
        );
        assert!(
            h.engine.borrow().cmd_sel.get().is_none(),
            "Ctrl+C must clear the selection after copying"
        );
    }
}

/// #755 slice 5: black-box coverage for the two editor rungs this slice
/// converged — the **minimap** (which GTK painted and then had no handler
/// for at all) and the **editor hover popup** (whose bespoke GTK copy ran
/// *below* the scroll-surface dispatch, so a press aimed at the popup was
/// eaten by whatever painted behind it — #229/#486 — and whose double-click
/// path never consulted the popup at all — #490).
///
/// Both assert on the painted surface, per `CLAUDE.md` rule 1. The TUI twins
/// live in `src/tui_main/shell_app.rs`.
#[cfg(test)]
mod editor_mouse_rungs {
    use super::*;

    /// 60 numbered lines — long enough that `render.rs` publishes a
    /// `ScreenLayout::minimap` entry and that the top and the middle of the
    /// file paint visibly different text, short enough to keep these tests'
    /// rasterisation cost near the rest of this file's.
    fn long_engine() -> Engine {
        // `new_for_test`, not `new`: the latter loads the developer's real
        // config (colourscheme, fonts), which both makes the fixture
        // machine-dependent and races the process-global theme other
        // pixel-probing tests in this file pin (`vscode_dimming`).
        let mut engine = Engine::new_for_test();
        let mut text = String::new();
        for i in 0..60 {
            text.push_str(&format!("line {i} content\n"));
        }
        engine.buffer_mut().insert(0, &text);
        engine
    }

    /// #35 parity coverage for the minimap rung, which #755's issue body
    /// listed as GTK-side-missing. It is not: GTK already routes the press
    /// through `render::apply_minimap_click` in `click.rs`
    /// (`handle_mouse_click`'s `ClickTarget::Minimap` bypass), the *same*
    /// shared function TUI's `handle_mouse` calls — so there was nothing to
    /// converge and this slice deliberately added no GTK minimap arm.
    ///
    /// What was missing is a black-box test proving the GTK half actually
    /// reaches the shared function through the painted geometry, so this
    /// pins it. Green before and after this slice, and stated as such in the
    /// PR: it is regression coverage, not a fix.
    ///
    /// The strip's rect is read from the published `ScreenLayout`, never
    /// hardcoded, so it survives any change to `minimap_reserved_width`.
    #[test]
    fn minimap_click_scrolls_the_editor_on_gtk() {
        let mut h = harness(long_engine(), 900, 600);
        h.driver.render();

        assert!(
            h.driver.screen_contains("line 0 content"),
            "precondition: the view starts at the top of the file; screen was {:?}",
            h.driver.painted_texts()
        );
        assert!(
            !h.driver.screen_contains("line 30 content"),
            "precondition: the middle of the file is not on screen yet"
        );

        let strip = {
            let layout = h.screen_layout.borrow();
            let mm = layout
                .as_ref()
                .expect("render_content must have painted a ScreenLayout")
                .minimap
                .first()
                .expect("a 200-line buffer must publish a minimap strip")
                .clone();
            crate::render::minimap_strip_rect(&mm)
        };
        // Vertical middle of the track → ~50% of the file.
        h.driver
            .click(strip.x + strip.width / 2.0, strip.y + strip.height / 2.0);
        h.driver.render();

        assert!(
            h.driver.screen_contains("line 30 content"),
            "clicking the vertical middle of the painted minimap strip must \
             seek the pane to ~50% of the file (#35); screen was {:?}",
            h.driver.painted_texts()
        );
    }

    /// #490 acceptance: `handle_mouse_double_click_msg` never consulted the
    /// editor hover popup, so a double-click landing on it fell straight
    /// through to `handle_mouse_double_click`, which word-selects in the
    /// editor *underneath* the popup instead of acting on the popup the user
    /// actually aimed at.
    ///
    /// Driven through a `command:` link so the outcome is legible on the
    /// painted grid (`CLAUDE.md` rule 1): the shared rung navigates and
    /// dismisses, so the popup's body text stops being painted.
    ///
    /// **RED against unfixed `develop`:** remove the
    /// `route_and_apply_editor_hover_popup` call at the top of
    /// `handle_mouse_double_click_msg` and the double-click falls through to
    /// the editor, leaving `HOVERBODY490` on the grid.
    ///
    /// The link's rect comes from what was painted, never hardcoded.
    #[test]
    fn double_click_on_editor_hover_popup_link_is_consumed_by_the_popup_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.show_editor_hover(
            0,
            3,
            "HOVERBODY490\n\n[Gotodef490](command:definition)",
            crate::core::engine::EditorHoverSource::Lsp,
            false,
            false,
        );
        let mut h = harness(engine, 900, 600);
        h.driver.render();
        assert!(
            h.driver.screen_contains("HOVERBODY490"),
            "precondition: the hover popup body must paint; screen was {:?}",
            h.driver.painted_texts()
        );

        let (lx, ly, lw, lh, uri) = h
            .editor_hover_link_rects
            .borrow()
            .first()
            .cloned()
            .expect("the popup's command link must have painted a hit rect");
        assert_eq!(uri, "command:definition");
        h.driver.dispatch(quadraui::UiEvent::DoubleClick {
            widget: None,
            position: quadraui::Point::new((lx + lw / 2.0) as f32, (ly + lh / 2.0) as f32),
        });
        h.driver.render();

        assert!(
            !h.driver.screen_contains("HOVERBODY490"),
            "a double-click on the editor hover popup must be consumed by the \
             shared popup rung — here, navigating the `command:` link and \
             closing the popup — not fall through to the editor's \
             word-select underneath (#490); screen was {:?}",
            h.driver.painted_texts()
        );
    }

    /// #755 acceptance for the popup-scrollbar arm: grabbing the thumb must
    /// preserve the cursor's offset *within* the thumb.
    ///
    /// GTK's bespoke arm hardcoded `grab_offset: 0.0`, so `dispatch_mouse_drag`
    /// treated wherever you grabbed as the thumb's *top* — the content
    /// teleported on the first drag frame even when the pointer had not
    /// moved. TUI's arm had always passed `cy - thumb.y`; the shared rung
    /// keeps that.
    ///
    /// **RED against unfixed `develop`:** with `grab_offset: 0.0` restored,
    /// the zero-distance drag below scrolls the popup and its first line
    /// stops being painted, failing the assertion.
    ///
    /// Both the thumb rect and the popup's first line come from what was
    /// painted, never hardcoded.
    #[test]
    fn grabbing_the_hover_popup_scrollbar_thumb_does_not_teleport_it_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        // Comfortably more rows than `render::EDITOR_HOVER_MAX_ROWS`, so the
        // popup paints a scrollbar with a thumb shorter than its track.
        let body: String = (0..30).map(|i| format!("hoverline{i}\n\n")).collect();
        engine.show_editor_hover(
            1,
            2,
            &body,
            crate::core::engine::EditorHoverSource::Lsp,
            true,
            false,
        );
        let mut h = harness(engine, 900, 600);
        h.driver.render();

        let sb = h
            .editor_hover_scrollbar
            .get()
            .expect("a 30-line hover body must paint a popup scrollbar");
        assert!(
            sb.thumb.height + 2.0 < sb.track.height,
            "fixture must produce a thumb shorter than its track: thumb {:?} track {:?}",
            sb.thumb,
            sb.track
        );
        assert!(
            h.driver.screen_contains("hoverline0"),
            "precondition: the popup starts at the top of its content; screen was {:?}",
            h.driver.painted_texts()
        );

        // Grab the *bottom* of the thumb, then "drag" without moving.
        let gx = sb.thumb.x + sb.thumb.width / 2.0;
        let gy = sb.thumb.y + sb.thumb.height - 1.0;
        h.driver.mouse_down(gx, gy);
        h.driver.mouse_move(gx, gy);
        h.driver.render();

        assert!(
            h.driver.screen_contains("hoverline0"),
            "grabbing the thumb's bottom edge and not moving must not scroll \
             the popup — GTK's old arm hardcoded `grab_offset: 0.0`, so the \
             content jumped by the thumb's height on the first drag frame; \
             screen was {:?}",
            h.driver.painted_texts()
        );
    }

    // ── #757 slice 2: the shared focus-owner keyboard rung ─────────────
    //
    // `render::route_focus_key` states the activity-bar → sidebar-panel
    // ladder once for both backends. GTK's old hand-rolled chain of
    // `if engine.*_has_focus` blocks had no picker gate and no terminal
    // gate, checked the explorer *above* the plugin panel (divergence 3),
    // and matched panels by focus flag only, never by the *visible* panel
    // (divergence 4). All four tests below aim at what the frame painted,
    // never at engine state — the pre-#757 bugs left engine state looking
    // perfectly reasonable while the keys went to the wrong surface.
    //
    // The TUI halves of this rung live in `src/tui_main/shell_app.rs`
    // (`activity_bar_ctrl_l_does_not_activate_via_shell_app`,
    // `explorer_inline_edit_backspace_reaches_the_engine_via_shell_app`,
    // `focused_plugin_panel_outranks_a_stale_explorer_flag_via_shell_app`,
    // `focused_settings_panel_outranks_the_default_visible_explorer_via_shell_app`).

    /// An engine with a real `cwd` (so `picker_populate_files` finds
    /// entries), the file explorer holding keyboard focus, and the Find
    /// Files palette open on top of it — the exact state a user is in after
    /// clicking into the explorer and then hitting the fuzzy-finder key.
    fn engine_with_palette_over_focused_explorer() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.explorer_has_focus = true;
        engine.open_picker(crate::core::engine::PickerSource::Files);
        engine
    }

    /// #757: an open picker must outrank a focused sidebar panel.
    ///
    /// TUI gated its whole sidebar keyboard tier on `!engine.picker_open`;
    /// GTK's chain had no such gate, so with the explorer focused every
    /// Down/Up went to the file tree and the palette the user was looking
    /// at would not move. `render::route_focus_key` returns
    /// `FocusKeyRoute::None` while a picker is open, letting the key fall
    /// through to `Engine::handle_key` → `handle_picker_key`, on both
    /// backends.
    ///
    /// # Why this asserts on pixels
    ///
    /// The palette is a `quadraui::Palette` and `GtkBackend::draw_palette`
    /// records no painted text (see this module's header, and the longer
    /// note on `breadcrumb_segment_click_opens_the_dropdown_and_selection_
    /// dispatches`), so `find` / `screen_contains` cannot see its rows. The
    /// probes read each painted row's *background*, which is the selection
    /// highlight itself — strictly stronger than reading
    /// `engine.picker_selected`, which would have passed against the bug.
    ///
    /// **Verified RED against unfixed `develop`:** restoring the
    /// `if self.engine.borrow().explorer_has_focus { … }` block above the
    /// picker fall-through sends Down to `handle_explorer_da_key`, the
    /// painted highlight stays on row 0, and the final two assertions fire.
    #[test]
    fn palette_outranks_a_focused_explorer_on_gtk() {
        let mut h = harness(engine_with_palette_over_focused_explorer(), 1400, 900);
        h.driver.render();

        assert!(
            h.engine.borrow().explorer_has_focus,
            "precondition: the explorer must hold focus — that is the flag \
             GTK's old chain consulted before anything else"
        );
        let (px, py, pw, ph) = h
            .picker_popup()
            .expect("the palette must actually paint, not just flip engine state");
        assert!(pw > 0.0 && ph > 0.0, "degenerate palette rect {pw}x{ph}");
        assert!(
            h.engine.borrow().picker_items.len() > 1,
            "fixture must list at least two files for a selection to move to"
        );

        // Row 0 is the open-state selection; row 1 is where Down must take it.
        let (p0x, p0y) = h.picker_row_probe(0).expect("row 0 must be painted");
        let (p1x, p1y) = h.picker_row_probe(1).expect("row 1 must be painted");
        let selected_bg = h.driver.pixel(p0x, p0y);
        let unselected_bg = h.driver.pixel(p1x, p1y);
        assert_ne!(
            selected_bg, unselected_bg,
            "precondition: the painted palette must highlight its selected \
             row, otherwise the probes below cannot tell the rows apart; \
             popup ({px}, {py}) {pw}x{ph}"
        );

        h.driver.press_named(quadraui::NamedKey::Down);
        h.driver.render();

        assert_eq!(
            h.driver.pixel(p1x, p1y),
            selected_bg,
            "Down must reach the open palette and move its painted highlight \
             to row 1, even though the explorer holds focus"
        );
        assert_eq!(
            h.driver.pixel(p0x, p0y),
            unselected_bg,
            "row 0 must paint as unselected once the highlight has moved"
        );
    }

    /// #757: a focused terminal must outrank a focused sidebar panel.
    ///
    /// TUI gated its whole sidebar keyboard tier on
    /// `!engine.terminal_has_focus` — the state you are in while an
    /// extension install waits on "Press Enter to close…". GTK's chain had
    /// no such gate, so with the explorer also focused those keys were eaten
    /// by the file tree. `render::route_focus_key` returns
    /// `FocusKeyRoute::None` while the terminal holds focus, on both
    /// backends.
    ///
    /// Asserts on the painted explorer inline-edit text — the same
    /// observable the TUI half
    /// (`explorer_inline_edit_backspace_reaches_the_engine_via_shell_app`)
    /// uses — and carries its own positive control: clearing
    /// `terminal_has_focus` and repeating the *identical* keypress must then
    /// edit the text, so a fixture whose edit simply could not change would
    /// fail the second half.
    ///
    /// **Verified RED against unfixed `develop`:** without the terminal gate
    /// the first Backspace reaches `handle_explorer_da_key`, the painted
    /// text drops to `ZQXWGTK75` immediately, and the first assertion fires.
    #[test]
    fn focused_terminal_outranks_a_focused_explorer_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.explorer_rebuild_rows();
        engine.explorer_has_focus = true;
        engine.session.explorer_visible = true;
        engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWGTK757".to_string(),
            "ZQXWGTK757".len(),
            None,
            None,
        );
        // The state an extension install's "Press Enter to close…" leaves
        // behind: the terminal owns the keyboard while the explorer's focus
        // flag is still set.
        engine.terminal_has_focus = true;

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        assert!(
            h.driver.screen_contains("ZQXWGTK757"),
            "precondition: the explorer's inline edit must paint; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.press_named(quadraui::NamedKey::Backspace);
        h.driver.render();
        assert!(
            h.driver.screen_contains("ZQXWGTK757"),
            "a focused terminal must outrank the focused explorer — Backspace \
             must not reach the file tree's inline edit; painted: {:?}",
            h.driver.painted_texts()
        );

        // Positive control: the very same keypress, with the terminal no
        // longer focused, must edit the text.
        h.engine.borrow_mut().terminal_has_focus = false;
        h.driver.press_named(quadraui::NamedKey::Backspace);
        h.driver.render();
        assert!(
            h.driver.screen_contains("ZQXWGTK75") && !h.driver.screen_contains("ZQXWGTK757"),
            "control: with the terminal unfocused the explorer must receive \
             Backspace; painted: {:?}",
            h.driver.painted_texts()
        );
    }

    /// #757 (divergence 3): a plugin panel's own focus must outrank a stale
    /// `explorer_has_focus` left set alongside it.
    ///
    /// `Engine::activity_bar_activate`'s ext-panel branch sets
    /// `ext_panel_active` + `ext_panel_has_focus` **without** clearing
    /// `explorer_has_focus` (unlike `focus_sidebar_panel`/
    /// `apply_activity_panel_switch`, which call `clear_sidebar_focus`
    /// first) — the state a user is in after focusing the explorer, then
    /// using the keyboard to open a plugin panel from the activity bar.
    /// GTK's old chain checked `explorer_has_focus` *before* the plugin
    /// panel, so a key meant for the panel reached the explorer instead.
    ///
    /// Because `ext_panel_active` makes the plugin panel — not the
    /// explorer — the painted sidebar body (`render::sidebar_owner`), the
    /// explorer's mutated edit state is invisible in the moment. This
    /// asserts on it anyway, the same way the moved highlight is read in
    /// `palette_outranks_a_focused_explorer_on_gtk`: it flips the sidebar
    /// back to the explorer *after* the keypress (mirroring what closing
    /// the plugin panel does — `render::sidebar_owner` falls back to
    /// `app_shell.active_panel_id()`, PANEL_EXPLORER by default, once
    /// `ext_panel_active` is cleared) and reads the painted text there, so
    /// a test that only inspected engine flags at keypress time could not
    /// tell whether the character was actually deleted.
    ///
    /// **Verified RED against unfixed `develop`:** restoring GTK's old
    /// `if engine.explorer_has_focus { … } else if engine.ext_panel_has_focus
    /// { … }` order routes Backspace to `dispatch_explorer_key`, which
    /// deletes the last character of the in-progress edit before the
    /// plugin panel is ever consulted, so re-opening the explorer view
    /// shows `ZQXWEXT75` and the final assertion fires.
    #[test]
    fn focused_plugin_panel_outranks_a_stale_explorer_flag_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.explorer_rebuild_rows();
        engine.explorer_has_focus = true; // stale, per the doc comment above
        engine.session.explorer_visible = true;
        engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWEXT757".to_string(),
            "ZQXWEXT757".len(),
            None,
            None,
        );

        engine.ext_panels.clear();
        engine.ext_panels.insert(
            "git-insights".to_string(),
            crate::core::plugin::PanelRegistration {
                name: "git-insights".to_string(),
                title: "Git Insights".to_string(),
                icon: '\u{f113}',
                fallback_icon: Some('X'),
                sections: Vec::new(),
            },
        );
        // The exact pair `activity_bar_activate`'s ext-panel branch sets.
        engine.ext_panel_active = Some("git-insights".to_string());
        engine.ext_panel_has_focus = true;

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        assert!(
            !h.driver.screen_contains("ZQXWEXT757"),
            "precondition: the explorer must not be what's painted while \
             ext_panel_active is set — GTK's `id.starts_with(\"ext:\")` arm \
             (a pre-existing gap, not this issue's to fix) renders the \
             built-in Extensions *marketplace* tree for any plugin panel \
             id rather than the specific plugin's own content, so this \
             checks the explorer's absence rather than the (unrelated)\
             marketplace's presence; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.press_named(quadraui::NamedKey::Backspace);
        h.driver.render();

        // Reveal: close the plugin panel the way its own "q"/Escape does
        // NOT (that only clears the focus flag) — clearing
        // `ext_panel_active` is what `render::sidebar_owner` falls
        // through on, exactly as leaving the panel via the activity bar
        // eventually does.
        h.engine.borrow_mut().ext_panel_active = None;
        h.driver.render();

        assert!(
            h.driver.screen_contains("ZQXWEXT757"),
            "a focused plugin panel must outrank a stale explorer focus \
             flag — Backspace must not have reached the explorer's inline \
             edit; painted: {:?}",
            h.driver.painted_texts()
        );
    }

    /// #757 (divergence 4): the *visible* panel must outrank a stale
    /// `*_has_focus` flag left set on a different, invisible panel.
    ///
    /// GTK's old chain matched panels exclusively by focus flag — never by
    /// `active_panel_is`, the *visible* panel — and checked
    /// `explorer_has_focus` before Settings. So with the explorer's focus
    /// flag left stale-true (e.g. from before the user opened Settings)
    /// and Settings actually on screen, GTK still routed every key to the
    /// (invisible) explorer.
    ///
    /// Same reveal technique as the plugin-panel test above: Settings, not
    /// the explorer, owns the sidebar body while `app_shell`'s active
    /// panel is Settings, so the explorer's mutated edit state is only
    /// checked once the active panel is switched back.
    ///
    /// **Verified RED against unfixed `develop`:** GTK's old chain checked
    /// `explorer_has_focus` (true here, stale) ahead of Settings and never
    /// consulted `active_panel_is` at all, so Backspace reached
    /// `dispatch_explorer_key` and deleted a character; switching back to
    /// the explorer view shows `ZQXWSET75` and the final assertion fires.
    #[test]
    fn visible_settings_panel_outranks_a_stale_explorer_flag_on_gtk() {
        use crate::core::engine::sidebar::{PANEL_EXPLORER, PANEL_SETTINGS};

        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.explorer_rebuild_rows();
        engine.explorer_has_focus = true; // stale
        engine.session.explorer_visible = true;
        engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWSET757".to_string(),
            "ZQXWSET757".len(),
            None,
            None,
        );
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_SETTINGS,
        ));
        // Deliberately left false: the fixture is "the visible panel
        // disagrees with every *_has_focus flag".
        engine.settings_has_focus = false;

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        assert!(
            !h.driver.screen_contains("ZQXWSET757"),
            "precondition: Settings, not the explorer, must own the \
             sidebar body while it is the active panel; painted: {:?}",
            h.driver.painted_texts()
        );

        h.driver.press_named(quadraui::NamedKey::Backspace);
        h.driver.render();

        h.engine
            .borrow_mut()
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        h.driver.render();

        assert!(
            h.driver.screen_contains("ZQXWSET757"),
            "the visible Settings panel must outrank a stale \
             explorer_has_focus — Backspace must not have reached the \
             explorer's inline edit; painted: {:?}",
            h.driver.painted_texts()
        );
    }
}

#[cfg(test)]
mod alt_rung {
    //! #759 / #734 slice 4 — the shared Alt-modifier / VSCode-mode rung.
    //!
    //! `render::route_alt_key` states this tier once for both backends. GTK
    //! had **no Alt tier at all**: `App::handle_key_press` took an `alt: bool`
    //! and used it only to feed the terminal router and to suppress the debug
    //! F-keys, then dropped it — `Engine::handle_key` has no `alt` parameter —
    //! so Alt+Left/Right, Alt+M, Alt+,/., Alt+]/[ and the whole VSCode-mode
    //! `Alt_*` set have been dead here since the #540 ShellApp cutover.
    //!
    //! Both tests below therefore fail against unfixed `develop` by
    //! construction: with no rung, the chord changes nothing at all. They are
    //! written as the mirrors of TUI's
    //! `alt_right_widens_the_painted_sidebar_via_shell_app` and
    //! `alt_z_toggles_word_wrap_only_in_vscode_mode_via_shell_app`
    //! (`src/tui_main/shell_app.rs`) and assert the same observables, so the
    //! pair is the cross-backend "same chord, same mode, same result" check
    //! the issue asks for. The spelling-identity tier — both backends' key
    //! *names* fed into one `route_alt_key` call — is
    //! `render::alt_key_router_tests`.
    use super::*;
    use quadraui::{Key, Modifiers, UiEvent};

    /// Press `key` with Alt held, exactly as `App::handle`'s `KeyPressed` arm
    /// receives it from the live GTK runner.
    fn alt_press<A: AppLogic>(driver: &mut GtkDriver<A>, key: Key, shift: bool) {
        driver.dispatch(UiEvent::KeyPressed {
            key,
            modifiers: Modifiers {
                alt: true,
                shift,
                ..Default::default()
            },
            repeat: false,
        });
    }

    /// #759: Alt+Right must widen the sidebar the frame actually **paints**.
    ///
    /// Asserted through `painted_sidebar_bounds` — the rect `render_content`
    /// filled the active panel into on the last pass — rather than through any
    /// stored width (`CLAUDE.md` rule 1). GTK's authoritative sidebar width is
    /// the runner's own `AppShell`, so a test that read `engine.app_shell`
    /// would pass against a fix that never reached the painted layout; that is
    /// exactly the mirror-state trap #454 documents on this very field.
    ///
    /// **Verified RED against unfixed `develop`:** there is no Alt rung there,
    /// so the sidebar rect is byte-identical before and after and the
    /// "must grow" assertion fires. (Equivalently, on this branch: change the
    /// `AltBase::Right` arm of `route_alt_key` to `Fallthrough`.)
    #[test]
    fn alt_right_widens_the_painted_sidebar_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        let before = h
            .painted_sidebar_bounds
            .get()
            .expect("precondition: the sidebar must be painted to be resized");
        assert!(before.width > 0.0, "degenerate sidebar rect {before:?}");

        alt_press(&mut h.driver, Key::Named(quadraui::NamedKey::Right), false);
        h.driver.render();
        let wider = h
            .painted_sidebar_bounds
            .get()
            .expect("the sidebar must still paint after Alt+Right");
        assert!(
            wider.width > before.width,
            "Alt+Right must widen the *painted* sidebar: {} -> {}",
            before.width,
            wider.width
        );

        alt_press(&mut h.driver, Key::Named(quadraui::NamedKey::Left), false);
        h.driver.render();
        let back = h
            .painted_sidebar_bounds
            .get()
            .expect("the sidebar must still paint after Alt+Left");
        assert_eq!(
            back.width, before.width,
            "Alt+Left must narrow it straight back — the two arms share one \
             clamp (`render::alt_resized_sidebar_width`)"
        );
    }

    /// #759: Alt+Z is a VS Code editor command (toggle word wrap) in VSCode
    /// mode and a plain pass-through in Vim mode. That mode-dependence is the
    /// "vscode-mode divergence" this slice converges: it used to exist only
    /// inside TUI's `handle_key_pressed`, so on GTK the chord did nothing in
    /// *either* mode.
    ///
    /// Asserts on the painted command line, where `engine.message` renders.
    /// The editor buffer is not an option here — the GTK backend does not
    /// `record_painted_text` editor glyphs (see this module's header) — and
    /// the command line is the same observable TUI's half asserts on, which is
    /// what makes the pair a cross-backend comparison rather than two
    /// unrelated tests.
    ///
    /// The menu tier above this rung cannot interfere: GTK's menu bar is
    /// always visible, so `MenuSystem::handle` sees every Alt chord, but
    /// `MenuBar::find_alt_target('z')` matches no menu label and returns
    /// `MenuEvent::Ignored`.
    ///
    /// **Verified RED against unfixed `develop`:** no Alt rung, so the command
    /// line stays empty in both modes and the VSCode-mode assertion fires.
    #[test]
    fn alt_z_toggles_word_wrap_only_in_vscode_mode_on_gtk() {
        for (vscode, expect_message) in [(true, true), (false, false)] {
            let mut engine = Engine::new_for_test();
            engine.settings.wrap = false;
            engine.settings.editor_mode = if vscode {
                crate::core::settings::EditorMode::Vscode
            } else {
                crate::core::settings::EditorMode::Vim
            };
            engine.mode = if vscode {
                crate::core::Mode::Insert
            } else {
                crate::core::Mode::Normal
            };

            let mut h = harness(engine, 1200, 800);
            h.driver.render();

            alt_press(&mut h.driver, Key::Char('z'), false);
            h.driver.render();

            assert_eq!(
                h.driver.screen_contains("Word wrap on"),
                expect_message,
                "Alt+Z must toggle word wrap in VSCode mode and do nothing in \
                 Vim mode (vscode = {vscode}); painted: {:?}",
                h.driver.painted_texts()
            );
        }
    }
}

#[cfg(test)]
mod slice7_closing_rungs {
    //! #762 / #734 slice 7 — the closing rungs, black-box on GTK.
    //!
    //! The behavioural half of the cross-backend parity assertion. Each test
    //! here has a mirror in `src/tui_main/shell_app.rs` that drives the *same*
    //! chord against the *same* engine state and asserts the *same* rendered
    //! string, so "both backends resolve this key + state to the same route"
    //! is checked end to end. The spelling-identity tier — both backends' key
    //! names fed into one resolver call — is `render::slice7_router_tests`.
    use super::*;
    use quadraui::{Key, Modifiers, UiEvent};

    fn press<A: AppLogic>(driver: &mut GtkDriver<A>, key: Key, modifiers: Modifiers) {
        driver.dispatch(UiEvent::KeyPressed {
            key,
            modifiers,
            repeat: false,
        });
    }

    /// #762: Shift+F5 is `stop`, not a shifted spelling of F5's `continue`.
    ///
    /// GTK's debug F-key block tested only `!ctrl && !alt` and never `shift`,
    /// so Shift+F5 ran *continue* and Shift+F11 ran *step-in* — the exact
    /// opposite of the intent. `render::route_debug_fkey` now states both
    /// tiers once, and TUI's
    /// `shift_f5_stops_the_debug_session_via_shell_app` asserts the identical
    /// string; that pair is the parity check.
    ///
    /// Asserts on the painted command line (where `engine.message` renders),
    /// never on a DAP field — `CLAUDE.md` rule 1.
    ///
    /// **Verified RED against unfixed `develop`:** with no `shift` branch the
    /// chord runs `continue` and the command line reads "DAP: starting Debug
    /// debug session…", so both assertions fire. Equivalently on this branch:
    /// delete the `if shift { ... }` block from `route_debug_fkey`.
    #[test]
    fn shift_f5_stops_instead_of_continuing_on_gtk() {
        let engine = Engine::new_for_test();
        let mut h = harness(engine, 1200, 800);
        h.driver.render();

        press(
            &mut h.driver,
            Key::Named(quadraui::NamedKey::F(5)),
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        h.driver.render();

        assert!(
            h.driver.screen_contains("DAP: session stopped"),
            "Shift+F5 must run `stop`; painted: {:?}",
            h.driver.painted_texts()
        );
        assert!(
            !h.driver.screen_contains("starting"),
            "Shift+F5 must NOT run `continue`; painted: {:?}",
            h.driver.painted_texts()
        );
    }

    /// #762: the debugger F-keys are global — a focused sidebar panel must
    /// not swallow them. GTK already resolved them above the panel arms; this
    /// pins that against the TUI mirror
    /// (`shift_f5_reaches_the_debugger_from_a_focused_panel_via_shell_app`),
    /// which is the backend the reorder actually fixed.
    #[test]
    fn shift_f5_reaches_the_debugger_from_a_focused_panel_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.search_has_focus = true;
        assert_eq!(
            crate::render::route_focus_key(&engine, engine.sidebar_has_focus()),
            crate::render::FocusKeyRoute::Search,
            "precondition: the search panel must own the keyboard"
        );

        let mut h = harness(engine, 1200, 800);
        h.driver.render();
        press(
            &mut h.driver,
            Key::Named(quadraui::NamedKey::F(5)),
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        h.driver.render();

        assert!(
            h.driver.screen_contains("DAP: session stopped"),
            "Shift+F5 must reach the debugger past a focused sidebar panel; \
             painted: {:?}",
            h.driver.painted_texts()
        );
    }

    // No GTK black-box test for the Ctrl+L rung: with the rung removed the
    // GtkDriver renders byte-identically, because GTK's spelling of the chord
    // (`key_name = "l"`, `ctrl = true`) is not a cursor motion in
    // `Engine::handle_key` either. A test here could not fail, and
    // `CLAUDE.md` rule 2 says a test that cannot fail is not coverage. The
    // rung is covered by TUI's RED-verified
    // `ctrl_l_is_consumed_and_never_edits_the_buffer_via_shell_app` and, for
    // GTK's own key spelling, by
    // `render::slice7_router_tests::ctrl_l_is_a_force_redraw_from_either_backends_spelling`.

    /// #762: `render::post_key_epilogue`'s sidebar-autohide behaviour, wired
    /// into GTK for the first time by `run_post_key_epilogue`. Before this
    /// PR `autohide_panels` never fired on GTK when focus returned to the
    /// editor, so a sidebar panel opened via the activity bar (or a stale
    /// session) stayed pinned open forever.
    ///
    /// Asserts on a unique marker painted by the explorer's in-progress
    /// rename field (the same technique
    /// `focused_plugin_panel_outranks_a_stale_explorer_flag_on_gtk` uses),
    /// never on `app_shell.sidebar_visible()` directly — `CLAUDE.md` rule 1.
    ///
    /// **Verified RED against unfixed `develop`:** with `run_post_key_epilogue`
    /// calling only `advance_macro_playback` + the nav-overflow arm (no
    /// `render::post_key_epilogue` call at all), the marker is still painted
    /// after the keypress and the final assertion fires.
    #[test]
    fn sidebar_autohides_when_focus_returns_to_the_editor_on_gtk() {
        use crate::core::engine::sidebar::PANEL_EXPLORER;

        let mut engine = Engine::new_for_test();
        engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.explorer_rebuild_rows();
        engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWAUTOHIDE757".to_string(),
            "ZQXWAUTOHIDE757".len(),
            None,
            None,
        );
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        engine.session.explorer_visible = true;
        engine.settings.autohide_panels = true;
        // No `*_has_focus` flag is set: the sidebar is merely visible, and
        // keyboard focus is the editor — exactly the state a user is in
        // after clicking back into a buffer with the explorer left open.

        let mut h = harness(engine, 1400, 900);
        h.driver.render();
        assert!(
            h.driver.screen_contains("ZQXWAUTOHIDE757"),
            "precondition: the sidebar starts open with the explorer visible; \
             painted: {:?}",
            h.driver.painted_texts()
        );

        // Any key that falls through to `Engine::handle_key` drains the
        // post-key epilogue.
        press(&mut h.driver, Key::Char('l'), Modifiers::default());
        h.driver.render();

        assert!(
            !h.driver.screen_contains("ZQXWAUTOHIDE757"),
            "autohide_panels must close the sidebar once a key reaches the \
             editor with no panel focused; painted: {:?}",
            h.driver.painted_texts()
        );
    }

    /// #762: the other half of `PostKeyEpilogue` new to GTK — Ctrl-W h/l
    /// overflowing left with **no** sidebar panel visible must put keyboard
    /// focus on the activity bar, not silently go nowhere. GTK's overflow
    /// arm used to call `focus_sidebar_panel` unconditionally, so with the
    /// sidebar hidden the keypress had no panel to focus and was dropped.
    ///
    /// Drives it end to end rather than reading `engine.activity_bar_focused`
    /// after the first key (`CLAUDE.md` rule 1): a second keypress, `x`
    /// (delete-char-under-cursor in Normal mode), is bound to nothing in
    /// `render::activity_bar_key_action` — so it is silently swallowed if
    /// the activity bar actually holds keyboard focus, and only reaches
    /// `Engine::handle_key` (deleting a character) if focus never moved
    /// there. This avoids depending on the *separate* runner ↔ shadow
    /// visibility sync `ActivityBarKeyAction::Activate` would additionally
    /// need to actually reveal a panel — orthogonal to the rung this test
    /// covers.
    ///
    /// **Verified RED against unfixed `develop`:** removing
    /// `PostKeyEpilogue::focus_activity_bar` (and its handling in
    /// `run_post_key_epilogue`) leaves `activity_bar_focused` false after the
    /// first keypress, so `x` falls through to `Engine::handle_key`, deletes
    /// the `f` of `fn`, and the final assertion fires.
    #[test]
    fn ctrl_w_overflow_focuses_the_activity_bar_when_no_sidebar_is_open_on_gtk() {
        let mut engine = Engine::new_for_test();
        engine.buffer_mut().insert(0, "fn main() {}\n");
        engine.app_shell.hide_sidebar();
        engine.session.explorer_visible = false;
        // Simulate Ctrl-W h having just overflowed left with no adjacent
        // editor group — the signal `Engine::focus_window_direction` leaves
        // for the backend to consume in its post-key epilogue.
        engine.window_nav_overflow = Some(false);

        let mut h = harness(engine, 1400, 900);
        h.driver.render();

        // Drain the epilogue that resolves the overflow signal.
        press(&mut h.driver, Key::Char('l'), Modifiers::default());
        h.driver.render();

        // Reveal: `x` must be swallowed by the (unbound) activity-bar action
        // table rather than deleting a character.
        press(&mut h.driver, Key::Char('x'), Modifiers::default());
        h.driver.render();

        assert!(
            h.driver.screen_contains("fn main() {}"),
            "Ctrl-W overflow with no sidebar visible must focus the activity \
             bar so a subsequent key is not spent editing the buffer; \
             painted: {:?}",
            h.driver.painted_texts()
        );
    }
}

#[cfg(test)]
mod issue_813_exit_via_reaction {
    //! #813: `App::backend` is now typed against the narrow
    //! [`super::TextMetricsBackend`] trait rather than the concrete GTK
    //! backend struct, and `save_session_and_exit` was ported off a
    //! `glib::idle_add_local_once(process::exit)` callback onto
    //! `App::exit_requested` + `quadraui::Reaction::Exit` — the same hook
    //! `GtkDriver`'s own `dispatch` already understands (`exited()`/
    //! `EventOutcome::Exit`). This is the one black-box-observable half of
    //! that change: the old code path called `process::exit` from a deferred
    //! `glib` callback that never runs under this headless harness (no glib
    //! main loop here), so `:qall!` used to leave the driver's `dispatch`
    //! returning `Reaction::Continue`/`Redraw` forever and `exited()` always
    //! `false` — a live process would still have quit (eventually, once the
    //! idle callback ran), but nothing in-process ever observed it. Now the
    //! flag is checked synchronously before `handle` returns, so the exit
    //! is observable the same frame.
    use super::*;
    use quadraui::UiEvent;

    /// **Verified RED against unfixed `develop`:** reverting `App::handle`
    /// to call `self.save_session_and_exit()` without the `exit_requested`/
    /// `Reaction::Exit` wrapper (i.e. restoring the old
    /// `glib::idle_add_local_once(|| std::process::exit(0))` body) leaves
    /// this `dispatch` returning `Reaction::Continue` and `exited()` false,
    /// so both assertions fire.
    #[test]
    fn qall_bang_returns_reaction_exit_on_gtk() {
        let engine = Engine::new_for_test();
        let mut h = harness(engine, 800, 600);
        h.driver.render();

        // Enter command-line mode and run `:qall!` — unconditional quit
        // regardless of dirty-buffer state (`execute.rs`'s `"qall!"` arm),
        // so this doesn't depend on the fixture's buffer state.
        for c in [':', 'q', 'a', 'l', 'l', '!'] {
            h.driver.type_char(c);
        }
        let reaction = h.driver.press_named(quadraui::NamedKey::Enter);

        assert_eq!(
            reaction,
            quadraui::Reaction::Exit,
            "`:qall!` must surface as Reaction::Exit so the runner tears \
             down the window instead of relying on a deferred process::exit \
             that a headless harness (no glib main loop) never runs"
        );
        assert!(
            h.driver.exited(),
            "GtkDriver::exited() must latch once `:qall!` runs"
        );
    }

    /// A second dispatch after exit must stay latched at `Reaction::Exit`
    /// rather than re-entering `App::handle_dispatch` on an app that has
    /// already asked to quit — mirrors `GtkDriver::dispatch`'s own
    /// short-circuit (`if self.exited { return Reaction::Exit; }`), so this
    /// also guards against a future edit to `save_session_and_exit` that
    /// clears `exit_requested` after firing once.
    #[test]
    fn dispatch_after_exit_stays_exited_on_gtk() {
        let engine = Engine::new_for_test();
        let mut h = harness(engine, 800, 600);
        h.driver.render();

        for c in [':', 'q', 'a', 'l', 'l', '!'] {
            h.driver.type_char(c);
        }
        h.driver.press_named(quadraui::NamedKey::Enter);
        assert!(h.driver.exited());

        let reaction = h.driver.dispatch(UiEvent::KeyPressed {
            key: quadraui::Key::Char('x'),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });
        assert_eq!(reaction, quadraui::Reaction::Exit);
    }
}
