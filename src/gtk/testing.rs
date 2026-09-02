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
pub(super) struct Harness<A: AppLogic> {
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
    /// #727: a native message dialog queued by `render_content`'s
    /// edge-trigger check, awaiting `tick()` to drain it. Tests read this
    /// with `Cell::take` directly (never calling `tick()`, which would
    /// actually try to pop a real `gtk4::AlertDialog` and block forever
    /// with no display / no user to click it) to observe *that* a present
    /// was queued without running the blocking call itself.
    pub pending_native_dialog: Rc<Cell<Option<quadraui::MessageDialogOptions>>>,
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
pub(super) fn harness(engine: Engine, width: i32, height: i32) -> Harness<impl AppLogic> {
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
    let panel_hover_popup_rect = Rc::clone(&app.panel_hover_popup_rect);
    let tab_switcher_popup_rect = Rc::clone(&app.tab_switcher_popup_rect);
    let status_segment_map = Rc::clone(&app.status_segment_map);
    let separated_status_bar_rect = Rc::clone(&app.separated_status_bar_rect);
    let title_bar_rect = Rc::clone(&app.title_bar_rect);
    let menu_row_rect = Rc::clone(&app.menu_row_rect);
    let dialog_layout = Rc::clone(&app.dialog_layout);
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
        panel_hover_popup_rect,
        tab_switcher_popup_rect,
        status_segment_map,
        separated_status_bar_rect,
        title_bar_rect,
        menu_row_rect,
        dialog_layout,
        native_dialog_shown,
        pending_native_dialog,
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

    /// AI: selecting the panel must paint its content — the 14th and last of
    /// #592's `ScreenLayout` fields, and the straggler #670 deferred (#730).
    ///
    /// Asserts on the *rendered* header text via `screen_contains`, never on
    /// `ai_panel`/`ai_has_focus` state alone — `ScreenLayout.picker` sat
    /// populated on GTK for months while nothing painted it (CLAUDE.md
    /// rule 1 / #587).
    #[test]
    fn ai_panel_paints_its_header() {
        let h = panel_harness(PANEL_AI);
        assert!(
            h.driver.screen_contains("AI ASSISTANT"),
            "selecting the AI panel must paint its header (#730)"
        );
    }

    /// AI: a press in the message-history area must focus the panel without
    /// opening the input box, and a press in the input box's own band must
    /// also activate text entry — the same "click focuses, click-in-input
    /// edits" split `git_panel_click_activates_the_commit_box_but_not_the_
    /// header` exercises for the git sidebar's commit box (#544/#730).
    ///
    /// The input band is always the bottom-most one `render::draw_ai_
    /// sidebar_panel` paints (header, then message history, then separator +
    /// input), so a point near the very bottom edge is reliably inside it
    /// without re-deriving the exact row math here.
    #[test]
    fn ai_panel_click_focuses_panel_and_activates_input_box() {
        let mut h = panel_harness(PANEL_AI);
        // Give the input box enough text to wrap across several rows, so its
        // band grows well past the ~1-row margin `window_edge` (checked
        // before sidebar routing, for CSD edge-resize) reserves along the
        // window's outer bottom edge. Without this, a click low enough to
        // land in the (1-row-tall, empty-input) input band also lands in
        // that resize margin and never reaches sidebar routing at all.
        h.engine.borrow_mut().ai_input = "x ".repeat(120);
        assert!(!h.engine.borrow().ai_has_focus);
        assert!(!h.engine.borrow().ai_input_active);

        let sb = h.painted_sidebar_bounds.get().unwrap();
        let lh = h.painted_line_height.get().unwrap() as f32;

        // A press a few rows down (comfortably below the 1-row header, still
        // well above the input box) must focus the panel but leave the input
        // box inactive.
        h.driver.click(sb.x + 20.0, sb.y + lh * 3.0);
        assert!(
            h.engine.borrow().ai_has_focus,
            "a click in the panel body must focus it (#544)"
        );
        assert!(
            !h.engine.borrow().ai_input_active,
            "a click in the message-history area must not activate the input box"
        );

        // A press well inside the (now multi-row) input box's band — but
        // clear of the window's bottom-edge resize margin — must also
        // activate text entry.
        h.driver.click(sb.x + 20.0, sb.y + sb.height - lh * 2.5);
        assert!(
            h.engine.borrow().ai_input_active,
            "a click in the input box must activate it (#544)"
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
/// RED-first (run, not assumed): with both values reverted on this same
/// tree, `inactive_line_numbers_dim_while_the_cursor_line_stays_bright`
/// fails its inactive-gutter assertion — the probe reads `(177, 176, 176)`
/// instead of `#858585` — and
/// `breadcrumb_path_paints_dimmer_than_editor_body_text` fails its
/// segment-colour assertion, reading `(126, 130, 137)` instead of `#6c7079`
/// (a 0.566 body-text luminance ratio, also over the 0.55 ceiling the
/// second assertion enforces).
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
        // body-text luminance. Measured 0.483 with #6c7079; the pre-#701
        // #7f848e measures 0.566 and fails this too.
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

        let crumb = brightest(
            &mut h,
            seg.x as i32,
            (seg.x + seg.width) as i32,
            seg.y as i32,
            (seg.y + seg.height) as i32,
        );
        assert!(
            near(crumb, BREADCRUMB_FG),
            "a non-trailing breadcrumb segment must paint at the dimmed \
             breadcrumb_fg {BREADCRUMB_FG:?} (#701); brightest pixel in the \
             painted segment rect was {crumb:?} — the pre-#701 #7f848e reads \
             (126, 130, 137) here"
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
