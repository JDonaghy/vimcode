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

use std::cell::RefCell;
use std::rc::Rc;

use quadraui::gtk::testing::{driver_with_shell, GtkDriver};
use quadraui::AppLogic;

use super::App;
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
    /// Absolute visible tab-slot x-ranges per group (`group_id.0` →
    /// `[(x0, x1)]`) captured by the last `render_content` pass — the tab-bar
    /// twin of `screen_layout`'s window rects, so a test can aim a click at the
    /// tab the rasteriser actually drew instead of guessing pixel offsets
    /// (#553).
    pub tab_slots_abs: Rc<RefCell<super::TabSlotsAbsMap>>,
    /// Absolute close-button (`×`) geometry per group: `(bar_top, bar_bottom,
    /// per-tab Option<(x0, x1)>)`, keyed by `group_id.0`. Same provenance and
    /// purpose as [`Self::tab_slots_abs`] (#553).
    pub tab_close_abs: Rc<RefCell<super::TabCloseAbsMap>>,
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

    /// Centre point (absolute pixels) of tab `tab_idx` in `group_id`'s tab bar,
    /// as the last frame painted it. `None` if that tab was scrolled off or the
    /// group drew no tab bar.
    ///
    /// Like [`Self::window_center`], this reads the geometry the *rasteriser*
    /// reported (`Backend::tab_bar_layout`), so tests never hardcode tab
    /// coordinates (#553).
    ///
    /// Reads its y-centre from `tab_close_abs` (the bar's `(top, bottom)`) and
    /// its x-centre from the separate `tab_slots_abs` map — both populated
    /// together, for the same `group_id`, from the same `render_content` pass
    /// (`App::cached_tab_close_abs` / `cached_tab_slots_abs` are inserted back
    /// to back per group in the tab-bar draw loop). If that ever split across
    /// two passes, a group present in only one map would silently return
    /// `None` here instead of a stale-but-visible tab centre — acceptable for
    /// a test helper, but worth knowing if this invariant ever changes.
    pub fn tab_center(
        &self,
        group_id: crate::core::window::GroupId,
        tab_idx: usize,
    ) -> Option<(f32, f32)> {
        let (bar_top, bar_bottom, _) = *self.tab_close_abs.borrow().get(&group_id.0)?;
        let slots = self.tab_slots_abs.borrow();
        let &(x0, x1) = slots.get(&group_id.0)?.get(tab_idx)?;
        if x1 <= x0 {
            return None;
        }
        Some(((x0 + x1) / 2.0, ((bar_top + bar_bottom) / 2.0) as f32))
    }

    /// Centre point (absolute pixels) of tab `tab_idx`'s close (`×`) button.
    /// `None` if that tab drew no close button this frame (#553).
    pub fn tab_close_center(
        &self,
        group_id: crate::core::window::GroupId,
        tab_idx: usize,
    ) -> Option<(f32, f32)> {
        let close = self.tab_close_abs.borrow();
        let (bar_top, bar_bottom, per_tab) = close.get(&group_id.0)?;
        let (x0, x1) = (*per_tab.get(tab_idx)?)?;
        Some((
            ((x0 + x1) / 2.0) as f32,
            ((bar_top + bar_bottom) / 2.0) as f32,
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
    let tab_slots_abs = Rc::clone(&app.cached_tab_slots_abs);
    let tab_close_abs = Rc::clone(&app.cached_tab_close_abs);
    Harness {
        driver: driver_with_shell(app, config, width, height),
        engine,
        screen_layout,
        tab_slots_abs,
        tab_close_abs,
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

        let wheel_down_at = |h: &mut Harness<_>, x: f32, y: f32| {
            h.driver.dispatch(UiEvent::Scroll {
                widget: None,
                position: Point::new(x, y),
                delta: ScrollDelta::new(0.0, 1.0),
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
            .tab_center(group, 0)
            .expect("the single-group tab bar must have painted tab 0");
        h.driver.click(x, y);

        assert_eq!(
            h.engine.borrow().editor_groups[&group].active_tab,
            0,
            "clicking tab 0 in a single-group layout must activate it"
        );
    }

    /// #553: with a single tab group, clicking a tab's × must close it.
    #[test]
    fn single_group_tab_close_button_closes_that_tab() {
        let mut h = harness(engine_with_three_tabs_one_group(), 1400, 900);
        let group = h.engine.borrow().active_group;
        let before = h.engine.borrow().editor_groups[&group].tabs.len();
        assert_eq!(before, 3);

        let (x, y) = h
            .tab_close_center(group, 0)
            .expect("the single-group tab bar must have painted tab 0's close button");
        h.driver.click(x, y);

        assert_eq!(
            h.engine.borrow().editor_groups[&group].tabs.len(),
            before - 1,
            "clicking a tab's × in a single-group layout must close it"
        );
    }
}
