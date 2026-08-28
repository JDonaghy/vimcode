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
    let painted_sidebar_bounds = Rc::clone(&app.painted_sidebar_bounds);
    let completion_layout = Rc::clone(&app.completion_layout);
    let editor_hover_popup_rect = Rc::clone(&app.editor_hover_popup_rect);
    let panel_hover_popup_rect = Rc::clone(&app.panel_hover_popup_rect);
    let tab_switcher_popup_rect = Rc::clone(&app.tab_switcher_popup_rect);
    let status_segment_map = Rc::clone(&app.status_segment_map);
    let separated_status_bar_rect = Rc::clone(&app.separated_status_bar_rect);
    Harness {
        driver: driver_with_shell(app, config, width, height),
        engine,
        screen_layout,
        picker_popup_rect,
        painted_line_height,
        painted_sidebar_bounds,
        completion_layout,
        editor_hover_popup_rect,
        panel_hover_popup_rect,
        tab_switcher_popup_rect,
        status_segment_map,
        separated_status_bar_rect,
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
    /// 1. `Msg::MouseScroll` reads the pointer out of `App::last_editor_pointer`,
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
        // quadraui-convention delta straight through to `Msg::MouseScroll`
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
    ///           ──ShellApp::handle──▶  Msg::MouseScroll.delta_y
    ///                (negates back)      (+ = down, GTK-raw — what every
    ///                                     downstream consumer expects)
    /// ```
    ///
    /// The #540 Relm4→ShellApp migration deleted the `connect_scroll` closure
    /// that fed `Msg::MouseScroll` GTK's raw `dy` and left the runner's
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
    /// to. `Msg::MouseScroll` hit-tests it (mod.rs `"debug_output" =>` arm)
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
    /// The pre-fix path reached `Msg::SettingsClick`, whose geometry was read
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
