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
    Harness {
        driver: driver_with_shell(app, config, width, height),
        engine,
        screen_layout,
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
}
