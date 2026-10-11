use super::*;

#[cfg(test)]
mod portable_entry_point_tests {
    //! Coverage for the two backend-neutral seams the macOS wrapper
    //! (`src/macos/mod.rs`) runs through. Both are un-gated, so these run in
    //! the ordinary Linux lanes even though `crate::macos` itself compiles
    //! only on a Mach-O host — which is the point: this fleet has no macOS
    //! cross-toolchain (see `src/macos/mod.rs`'s "Verifying this file without
    //! a Mac"), so without these the wrapper's inputs would be unguarded
    //! everywhere vimcode actually builds.

    use super::*;

    /// Any quadraui shell runner — `gtk::`, `tui::` or `macos::` — takes
    /// `A: ShellApp + 'static`. `crate::macos::run` relies on `App`
    /// satisfying it, and that call site is invisible to every lane this
    /// fleet can compile, so pin the bound here instead: `App` gaining a
    /// borrowed field would kill `'static` while leaving every existing GTK
    /// test green.
    ///
    /// Deliberately a bound assertion rather than a call — `run_with_shell`
    /// enters an event loop and never returns.
    #[test]
    fn app_is_runnable_by_any_quadraui_shell_runner() {
        fn assert_runnable<A: quadraui::ShellApp + 'static>() {}
        assert_runnable::<App>();
    }

    /// [`App::shell_config`] must hand the runner an activity bar that can
    /// actually be painted: every panel resolved to a non-empty glyph (the
    /// engine leaves `PanelDefinition.icon` empty because it is
    /// backend-agnostic), settings split out as a *bottom* item rather than
    /// left in the top list, and the title-bar band reserved — without it
    /// `render_content` has nowhere to paint the menu bar and the app opens
    /// with no menus at all.
    ///
    /// Needs a constructed `App`, so it rides the `gui`-gated headless
    /// constructor; the function under test is not `gui`-gated.
    #[cfg(feature = "gui")]
    #[test]
    fn shell_config_resolves_every_activity_bar_icon_and_reserves_the_title_bar() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(engine);
        let cfg = app.shell_config();

        assert!(
            !cfg.panels.is_empty(),
            "no top-pinned panels: the activity bar would paint nothing"
        );
        for p in cfg.panels.iter().chain(cfg.bottom_items.iter()) {
            assert!(
                !p.icon.is_empty(),
                "panel {:?} reached the runner with an unresolved icon",
                p.id
            );
        }
        assert!(
            cfg.panels.iter().any(|p| p.id.as_str() == "panel:explorer"),
            "explorer missing from the top-pinned panels"
        );
        assert!(
            cfg.bottom_items
                .iter()
                .any(|p| p.id.as_str() == "bottom:settings"),
            "settings must be bottom-pinned, not left in the top list"
        );
        assert!(
            !cfg.panels
                .iter()
                .any(|p| p.id.as_str().starts_with("bottom:")),
            "a bottom: panel leaked into the top-pinned list"
        );

        assert!(
            cfg.has_title_bar && cfg.title_bar_height_lh > 1.0,
            "title-bar band not reserved: render_content paints the menu bar into it"
        );
        // The opt-in itself must reach the runner — a capable backend
        // (macOS's `MacBackend`) reads this flag to fold the reserved band
        // into the real titlebar instead of reserving space underneath it.
        // GTK/Win-GUI don't honour it yet (see the call site's comment in
        // `shell_config`), so setting it here is inert on them today, but
        // that is exactly why this must be a plain, unconditional assertion
        // rather than one gated on `target_os` — the Platform-Neutrality
        // Rule means `shell_config` cannot special-case macOS to set it.
        assert!(
            cfg.client_side_titlebar,
            "shell_config must opt into the client-side titlebar (quadraui#947) \
             unconditionally, not behind a target_os gate"
        );
        assert_eq!(cfg.min_sidebar_width, render::ALT_SIDEBAR_WIDTH_MIN as f32);
        assert_eq!(cfg.max_sidebar_width, render::ALT_SIDEBAR_WIDTH_MAX as f32);
        // The sidebar must *open* at the profile's own width rather
        // than inherit `ShellConfig::new`'s cell-flavoured 20.0, which on a
        // GUI backend is ~460px and left an 800px window no room for a
        // second editor tab. (This is a `gui`-gated test, so `self.units`
        // here is `UnitProfile::px()`.)
        assert_eq!(
            cfg.default_sidebar_width,
            render::UnitProfile::px().sidebar_width_lh,
            "shell_config must take the sidebar's opening width from the unit \
             profile, not leave quadraui's cell-flavoured 20.0 default in place"
        );
        // `compute_layout` clamps the opening width through the bounds above,
        // so an opening width outside them is silently discarded.
        assert!(
            cfg.default_sidebar_width >= cfg.min_sidebar_width
                && cfg.default_sidebar_width <= cfg.max_sidebar_width,
            "the opening width ({}) must survive compute_layout's own clamp \
             to {}..={}",
            cfg.default_sidebar_width,
            cfg.min_sidebar_width,
            cfg.max_sidebar_width,
        );
    }

    /// Makes the "closes the macOS/Win-GUI settings hot-reload
    /// gap for free" claim testable. `handle_poll_tick` calls
    /// `settings_file_changed` unconditionally every tick for every GUI
    /// backend (this constructor is the backend-neutral one).
    /// This drives that call site directly against a real `App` + `Engine`,
    /// pointed at a private temp file via `core::settings::TestSettingsPathGuard`
    /// (see that type's doc for why a thread-local override, not a `$HOME`
    /// mutation), and asserts the reload actually took: `line_numbers` moves
    /// from the constructor's default (`None`) to what the on-disk file
    /// says (`Absolute`) purely from calling `handle_poll_tick()`, with no
    /// file-monitor callback involved.
    ///
    /// This is a state assertion, not a painted-pixel one — state mutations
    /// like `engine.settings` are read by every frame (colorscheme,
    /// line-number gutter, tabstop, …), so there is no separate paint-path
    /// question, only whether the poll fired. A genuine pixel-level check
    /// needs `GtkDriver`/`ConformanceDriver` to expose a way to pump
    /// `AppLogic::tick` headlessly; as of the pinned quadraui rev neither
    /// does. Adding that pump is quadraui-side test infrastructure, so per
    /// CLAUDE.md's Platform-Neutrality Rule it belongs upstream, not here.
    #[cfg(feature = "gui")]
    #[test]
    fn handle_poll_tick_reloads_settings_changed_on_disk() {
        use crate::core::settings::{LineNumberMode, TestSettingsPathGuard};

        let tmp = std::env::temp_dir().join(format!(
            "vimcode_test_949_handle_poll_tick_{:?}.json",
            std::thread::current().id()
        ));
        std::fs::write(&tmp, r#"{"line_numbers":"Relative"}"#).expect("write temp settings.json");
        let _guard = TestSettingsPathGuard::install(tmp.clone());

        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        assert_eq!(
            engine.borrow().settings.line_numbers,
            LineNumberMode::Absolute,
            "precondition: the constructor's default must differ from the \
             on-disk value, or a reload would be indistinguishable from a no-op"
        );
        let mut app = App::new_headless(Rc::clone(&engine));
        let mut backend = quadraui::gtk::GtkBackend::new();

        app.handle_poll_tick(&mut backend);

        assert_eq!(
            engine.borrow().settings.line_numbers,
            LineNumberMode::Relative,
            "handle_poll_tick did not pick up the externally-edited settings file"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    /// `App::handle_poll_tick` and [`App::window_minimize`] route through
    /// `Backend::window()` rather than the GTK-only `self.window`/
    /// `PlatformWindowHandle` seam. That seam is `None` on macOS and Win-GUI,
    /// making both silent no-ops there. Black-box coverage of the positive
    /// path (a title reaching the OS window, minimize actually iconifying it)
    /// cannot be added here because:
    ///
    /// 1. `GtkBackend::window()` returns `Some` only once `self.window` is
    ///    set via `GtkBackend::set_window`, which needs a real
    ///    `gtk4::ApplicationWindow` requiring GTK initialized against a live
    ///    display. This repo's headless-testing strategy (`src/gtk/testing.rs`)
    ///    never calls `gtk::init()` (see that module's doc), so there is no
    ///    headless path to non-`None` `GtkBackend::window()`.
    /// 2. `GtkDriver::backend()` returns `&GtkBackend`, not `&mut GtkBackend`
    ///    — there is no `backend_mut()` — but `Backend::window()` needs
    ///    `&mut self`, so even a driver with a window attached couldn't
    ///    reach it from tests.
    /// 3. A hand-rolled fake `Backend` cannot work: `quadraui::Backend` is
    ///    a sealed trait, and `quadraui::testing::RecordingBackend` does not
    ///    override `window()`, inheriting the trait's default `None`.
    ///
    /// Live windowed runs are the only thing exercising the `Some` branch,
    /// which is why manual verification is a `SMOKE_TESTS` item. Closing
    /// this needs a quadraui-side testing hook (e.g. `GtkDriver::backend_mut()`)
    /// — a quadraui issue to file, not a vimcode workaround.
    #[cfg(feature = "gui")]
    #[test]
    fn gtk_backend_window_is_none_without_a_live_window_so_title_sync_and_minimize_stay_black_box_untestable(
    ) {
        use quadraui::Backend;

        let mut backend = quadraui::gtk::GtkBackend::new();
        assert!(
            backend.window().is_none(),
            "GtkBackend reported a window without ever calling set_window -- \
             if this fires, quadraui has changed and #1124's title-sync/\
             minimize path can likely now get real black-box coverage; see \
             this test's doc comment"
        );
    }

    /// `App::handle_poll_tick` drains `App::tab_visible_counts`,
    /// triggering `Engine::post_draw_apply_widths` — the contract every UI
    /// backend must call after each completed paint. A tab scrolled out of
    /// the visible tab bar by a resize, sidebar toggle, or opening many tabs
    /// could stay off-screen forever on GTK without this drain, with nothing
    /// to bring it back until an unrelated change touched `tab_scroll_offset`.
    /// TUI self-corrected within two frames via the same drain.
    ///
    /// Drives `App::handle_poll_tick` directly against a real `App` + `Engine`
    /// (see `handle_poll_tick_reloads_settings_changed_on_disk`'s doc for why).
    /// `tab_visible_counts` is normally populated by `paint_tab_bars_rung` from
    /// geometry already painted — here it's pushed by hand to isolate
    /// the wiring bug from the already-tested paint step.
    ///
    /// RED-verified: reverting the `tab_visible_counts` drain calling
    /// `post_draw_apply_widths` makes this red — `tab_scroll_offset` stays `0`.
    /// Confirmed by hand before committing.
    #[cfg(feature = "gui")]
    #[test]
    fn handle_poll_tick_scrolls_the_active_tab_back_into_view_on_gtk() {
        let engine = Rc::new(RefCell::new(Engine::new()));
        // Ten tabs, active tab is the last one opened (`new_tab` always
        // activates the tab it just created).
        for _ in 0..9 {
            engine.borrow_mut().new_tab(None);
        }
        let group_id = engine.borrow().active_group;
        assert_eq!(engine.borrow().active_group().active_tab, 9);
        assert_eq!(
            engine.borrow().active_group().tab_scroll_offset,
            0,
            "precondition: nothing has ever narrowed the bar, so the engine's \
             own default offset must start at 0 or this test wouldn't show \
             `handle_poll_tick` moving it"
        );

        let mut app = App::new_headless(Rc::clone(&engine));
        let mut backend = quadraui::gtk::GtkBackend::new();

        // Stand in for this frame's `TabBars` rung reporting a tab bar too
        // narrow to fit all ten tabs starting from offset 0 — exactly what
        // `paint_tab_bars_rung` would have pushed had a real frame painted
        // first.
        app.tab_visible_counts.borrow_mut().push((group_id, 20));

        app.handle_poll_tick(&mut backend);

        assert!(
            engine.borrow().active_group().tab_scroll_offset > 0,
            "handle_poll_tick did not apply this frame's painted tab-bar \
             width -- the active tab (index 9) can stay scrolled out of \
             view forever on GTK (#1165)"
        );
    }

    /// The GTK mirror of the pre-#1434 TUI shell's
    /// `take_requested_panel_echo_does_not_steal_focus` — a *reconciliation*
    /// `PanelChanged` (the one `Self::take_requested_panel` synthesizes to steer
    /// the runner's `AppShell` back to what the shadow `engine.app_shell`
    /// already believes, e.g. after a DAP reveal or keyboard accelerator)
    /// must only update `Self::last_shell_panel` bookkeeping — never call
    /// `Engine::focus_sidebar_panel` the way a genuine activity-bar click does.
    /// Without the `suppress_shell_panel_echo` guard, app-initiated switches
    /// would steal keyboard focus into the panel from whatever the app
    /// just focused (e.g. the editor, mid-DAP-launch).
    #[cfg(feature = "gui")]
    #[test]
    fn take_requested_panel_echo_does_not_steal_focus_on_gtk() {
        use quadraui::ShellApp;

        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        engine
            .borrow_mut()
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        engine.borrow_mut().session.explorer_visible = true;
        assert!(!engine.borrow().explorer_has_focus);

        let mut app = App::new_headless(Rc::clone(&engine));
        let _ = app.take_requested_panel(); // returns Some(explorer), arms suppress
        app.dispatch_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_EXPLORER),
        });

        assert!(
            !engine.borrow().explorer_has_focus,
            "the take_requested_panel echo must only update the runner-state \
             belief, not steal focus like a user click"
        );
    }

    /// `App::restore_window_geometry` gates on `Backend::window()`
    /// returning `Some`, exactly like `capture_window_and_apply_csd` does
    /// for CSD (see `gtk_backend_window_is_none_without_a_live_window_
    /// so_title_sync_and_minimize_stay_black_box_untestable`'s doc above)
    /// — so the one thing headlessly testable here is the
    /// retry-until-mapped contract: calling it against a `GtkBackend`
    /// nobody has attached a window to must be a complete no-op (no
    /// panic, and crucially `window_geometry_restored` stays `false` so
    /// `tick()` keeps retrying next frame instead of giving up on a
    /// window that simply isn't mapped yet).
    ///
    /// RED-verification note: symmetric with the neighbouring
    /// `gtk_backend_window_is_none_...` test's own note — before this fix
    /// `restore_window_geometry` did not exist at all (nothing restored
    /// anything, the bug this issue is about), so there is no
    /// "unfixed but present" version of *this* method to turn red. Its
    /// job is guarding the fix's shape (idempotent no-op until mapped)
    /// against regression going forward; the actual restore-something
    /// behaviour this issue fixes can only be confirmed on a live window
    /// — see this PR's `SMOKE_TESTS`.
    #[cfg(feature = "gui")]
    #[test]
    fn restore_window_geometry_is_a_noop_until_the_window_is_mapped() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        engine.borrow_mut().session.window = core::session::WindowGeometry {
            width: 1000,
            height: 700,
            x: Some(10),
            y: Some(20),
            maximized: true,
        };
        let mut app = App::new_headless(Rc::clone(&engine));
        let mut backend = quadraui::gtk::GtkBackend::new();

        app.restore_window_geometry(&mut backend);

        assert!(
            !app.window_geometry_restored.get(),
            "restore_window_geometry must not mark itself done before \
             Backend::window() ever returns Some, or tick() would give up \
             retrying and the saved geometry would never actually be \
             applied once the window is mapped"
        );
    }

    /// `App::cached_window_geometry` is the save-side snapshot that
    /// `App::quit_and_save_session`/`App::save_session_and_exit` funnel through
    /// before `Engine::save_session_state` persists it. Before this fix,
    /// those sites wrote only `width`/`height` into `engine.session.window`
    /// — `x`/`y`/`maximized` were never saved no matter what `sync_window_title`
    /// cached, the exact bug this issue reports. This drives the cells directly
    /// (`sync_window_title`'s live-window dependency is the structural gap that
    /// `gtk_backend_window_is_none_...` documents, headlessly unreachable)
    /// and asserts the snapshot carries all five fields through — a data-plumbing
    /// assertion: these cells feed a session-file write, not a screen.
    #[cfg(feature = "gui")]
    #[test]
    fn cached_window_geometry_snapshots_position_and_maximized() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(Rc::clone(&engine));

        app.cached_window_width.set(1000);
        app.cached_window_height.set(700);
        app.cached_window_x.set(Some(50));
        app.cached_window_y.set(Some(75));
        app.cached_window_maximized.set(true);

        let geo = app.cached_window_geometry();

        assert_eq!(geo.width, 1000);
        assert_eq!(geo.height, 700);
        assert_eq!(geo.x, Some(50));
        assert_eq!(geo.y, Some(75));
        assert!(geo.maximized);
    }

    /// The black-box GTK coverage the acceptance criteria
    /// asked for ("start with a session holding e.g. 1000x700 and verify
    /// the window's reported size; start with `maximized: true` and
    /// verify it's maximized") and the review found missing.
    /// `restore_window_geometry_is_a_noop_until_the_window_is_mapped`
    /// above only proves the retry-until-mapped *contract* — it never
    /// attaches a window, so it cannot observe a single pixel of the
    /// restore behaviour this issue is actually about.
    ///
    /// This drives a real `gtk4::ApplicationWindow`, attached via
    /// `quadraui::gtk::GtkBackend::set_window` (`pub fn`, the same call
    /// `quadraui::gtk::run::activate` makes in production — see that
    /// function for the identical `ApplicationWindow::builder()` shape
    /// this mirrors), then calls the *production* `restore_window_geometry`
    /// against it and reads back the *real* `WindowControl::bounds()`/
    /// `is_maximized()` — not a cached `Cell`, not a struct field.
    ///
    /// **RED verification: NOT executed, and this says so explicitly**
    /// rather than repeating the review's own complaint about an
    /// unverifiable claim. This worker's sandbox is macOS with no live
    /// desktop session attached to the test process, and — independent of
    /// that — `cargo test`'s worker-thread model makes `gtk4::init()`
    /// unusable here at all (see the macOS note below); there was no way
    /// to actually run this test to green, let alone flip
    /// `restore_window_geometry` back to a no-op and watch it go red, in
    /// this environment. This test's own runtime probe (`catch_unwind`
    /// around `gtk4::init()`) reflects that honestly by skipping rather
    /// than asserting anything. Whoever next runs this on a Linux machine
    /// with a live desktop should do that RED/GREEN check by hand — revert
    /// `restore_window_geometry`'s body to a no-op, confirm both
    /// assertions fail, restore it, confirm both pass — and record having
    /// done so, since nobody has yet.
    ///
    /// `#[ignore]`d for the identical reason as `src/gtk/testing.rs`'s
    /// `setup_gtk_clipboard_round_trips_yank_and_paste_through_real_
    /// backend_1100`: `gtk4::init()` needs a live windowing session
    /// (X11/Wayland, or macOS's native windowing). CI's headless
    /// GUI-feature job asserts `DISPLAY`/`WAYLAND_DISPLAY` are both unset
    /// before running `cargo test`, so this must stay `#[ignore]`d rather
    /// than runtime-skip only; run manually with `cargo test -- --ignored
    /// restore_window_geometry_applies_size_and_maximized_to_a_real_window`
    /// on a machine with a live desktop.
    ///
    /// **macOS note**: `gtk4-rs` asserts `gtk4::init()` runs on the
    /// process's main thread (Cocoa's own requirement) and *panics*
    /// rather than returning `Err` when it doesn't — and `cargo test`
    /// always runs each test on a worker thread, never the main one. The
    /// probe below wraps the call in `catch_unwind` (with the panic hook
    /// silenced for its duration, so a graceful skip doesn't print a
    /// misleading backtrace) specifically to turn that unconditional
    /// macOS panic into the same graceful skip a missing display gets on
    /// Linux — so on macOS this test *always* skips under the ordinary
    /// `cargo test` harness, `--ignored` or not; verifying it for real
    /// requires a single-main-threaded runner (a plain `fn main` calling
    /// the test function directly), which is out of scope for this issue.
    /// Linux (X11/Wayland) has no such restriction and is the platform
    /// this test can actually exercise end-to-end.
    #[cfg(feature = "gui")]
    #[test]
    #[ignore = "needs a live windowing session — gtk4::init() has no display \
                to talk to in headless CI; run with `cargo test -- --ignored` \
                on a machine with a live desktop (see doc comment)"]
    fn restore_window_geometry_applies_size_and_maximized_to_a_real_window() {
        use gtk4::prelude::*;
        use quadraui::Backend;

        let _paint = crate::test_paint::PaintGuard::acquire();

        // See this test's doc comment's "macOS note": `gtk4::init()`
        // panics (does not return `Err`) when called off the main thread,
        // which every `cargo test` worker thread is. Silence the panic
        // hook for the duration of the probe so that expected panic
        // doesn't print a backtrace that looks like a real failure.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let init_result = std::panic::catch_unwind(gtk4::init);
        std::panic::set_hook(previous_hook);

        if !matches!(init_result, Ok(Ok(()))) {
            eprintln!(
                "skipping restore_window_geometry_applies_size_and_maximized_to_a_real_window: \
                 gtk4::init() did not succeed on this thread/platform (no live windowing \
                 session, or — on macOS — cargo test's worker thread isn't the main thread; \
                 see this test's doc comment)"
            );
            return;
        }

        let gapp = gtk4::Application::builder()
            .application_id("dev.vimcode.test.restore-window-geometry-1529")
            .build();
        let window = gtk4::ApplicationWindow::builder()
            .application(&gapp)
            .default_width(320)
            .default_height(240)
            .build();
        window.present();
        // Let the initial `present()` request land before seeding the
        // "before" measurement below, and after every later mutation, so
        // `bounds()`/`is_maximized()` read the window's settled state
        // rather than racing GTK4's asynchronous resize/maximize requests.
        let pump = || while gtk4::glib::MainContext::default().iteration(false) {};
        pump();

        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        engine.borrow_mut().session.window = core::session::WindowGeometry {
            width: 1000,
            height: 700,
            x: None,
            y: None,
            maximized: false,
        };
        let mut app = App::new_headless(Rc::clone(&engine));
        let mut backend = quadraui::gtk::GtkBackend::new();
        backend.set_window(window.clone());

        app.restore_window_geometry(&mut backend);
        pump();

        let bounds = backend
            .window()
            .expect("set_window was just called")
            .bounds()
            .expect("bounds() must succeed once a window is attached");
        assert_eq!(
            bounds.width.round() as i32,
            1000,
            "restore_window_geometry did not apply the saved width to the \
             real window"
        );
        assert_eq!(
            bounds.height.round() as i32,
            700,
            "restore_window_geometry did not apply the saved height to the \
             real window"
        );
        assert!(
            !matches!(backend.window().unwrap().is_maximized(), Ok(true)),
            "window must not be maximized when the saved session says \
             maximized: false"
        );

        // Second case, same window: `maximized: true` must actually
        // maximize it. `window_geometry_restored` reset by hand since a
        // real run only restores once per process lifetime; this test
        // exercises both branches of `clamped.maximized` against the one
        // window rather than tearing down and reattaching a second one.
        app.window_geometry_restored.set(false);
        engine.borrow_mut().session.window.maximized = true;
        app.restore_window_geometry(&mut backend);
        pump();

        assert!(
            matches!(backend.window().unwrap().is_maximized(), Ok(true)),
            "restore_window_geometry did not maximize the real window when \
             the saved session says maximized: true"
        );
    }

    /// A tab-bar width correction that doesn't move the active
    /// tab's resolved scroll offset must not schedule a redraw — the
    /// shared-code half of "spurious full repaints: idle tick returns
    /// Redraw ~4×/s with several tabs open". Backend-neutral (`TuiBackend`,
    /// no `gui` feature needed) because the bug lives in
    /// `render::run_shared_tick_chores`/`Engine::post_draw_apply_widths`,
    /// shared by both backends. This proves the fix reaches the real tick path
    /// (`App::handle_poll_tick` → `run_shared_tick_chores` → `self.draw_needed`),
    /// the exact signal `tick_dispatch` turns into `Reaction::Redraw`/`Continue`.
    ///
    /// Pushes directly into `App::tab_visible_counts` (the same technique
    /// `handle_poll_tick_scrolls_the_active_tab_back_into_view_on_gtk` uses),
    /// isolating the wiring bug from the already-tested paint step. On a
    /// pixel-measuring backend the measured width can wobble by a sub-pixel unit;
    /// this test reproduces that shape directly.
    ///
    /// RED-verified: reverting `Engine::post_draw_apply_widths` to its
    /// pre-#1722 contract makes this fail — a one-column wobble sets
    /// `draw_needed` instead of leaving it clear. Confirmed by hand.
    #[test]
    fn handle_poll_tick_does_not_redraw_on_a_cosmetic_tab_width_wobble() {
        let engine = Rc::new(RefCell::new(Engine::new()));
        // Three tabs, active = last (idx 2); each "[No Name]" tab is 16
        // display columns wide (see `Engine::tab_display_width`).
        engine.borrow_mut().new_tab(None);
        engine.borrow_mut().new_tab(None);
        let group_id = engine.borrow().active_group;

        let mut app = App::new_headless_with_backend(
            Rc::clone(&engine),
            Rc::new(RefCell::new(
                Box::new(quadraui::tui::TuiBackend::new()) as Box<dyn quadraui::Backend>
            )),
            render::UnitProfile::cell(),
        );
        let mut backend: Box<dyn quadraui::Backend> = Box::new(quadraui::tui::TuiBackend::new());

        // First tick: bar only wide enough for one tab (16 <= 30 < 32) —
        // a real correction, since the active tab (idx 2) wasn't visible
        // at the engine's default offset.
        app.tab_visible_counts.borrow_mut().push((group_id, 30));
        app.handle_poll_tick(&mut *backend);
        assert!(
            app.draw_needed.get(),
            "test setup: narrowing the bar enough to hide the active tab \
             must schedule a redraw"
        );
        app.draw_needed.set(false);

        // Second tick: one column narrower, still only enough for exactly
        // one tab — the resolved scroll offset is identical, so nothing
        // painted this frame would differ from what's already on screen.
        app.tab_visible_counts.borrow_mut().push((group_id, 29));
        app.handle_poll_tick(&mut *backend);
        assert!(
            !app.draw_needed.get(),
            "a one-column tab-bar width wobble with no effect on which tab \
             is scrolled into view must not schedule a redraw (#1722) — on \
             a pixel-measuring backend (GTK/Win-GUI/macOS) this is exactly \
             the kind of sub-pixel-rounding noise that forced a redraw \
             every ~250ms idle tick with several tabs open"
        );
    }
}
