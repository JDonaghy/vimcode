use super::*;

/// Set up system clipboard callbacks on the engine via
/// `backend.services().clipboard()` (issue #1100 — quadraui#991's
/// `PlatformServices` seam, replacing the bespoke `copypasta_ext` stack).
///
/// `backend` is `App`'s own held handle — `Rc<RefCell<Box<dyn
/// quadraui::Backend>>>`, distinct from the runner-owned `&mut dyn
/// quadraui::Backend` `ShellApp::setup`/`handle`/`tick` receive only
/// transiently (see [`PendingFileDialog`]'s doc for why that distinction
/// matters for file dialogs). Cloning the `Rc` into each closure lets
/// `engine.clipboard_read`/`clipboard_write` — plain `Fn` callbacks with no
/// backend parameter of their own, called from deep inside `core::engine`
/// code that has no `Backend` handle at all — reach the clipboard on every
/// call, long after this function itself returns.
///
/// Unlike the file-dialog case, clipboard access needs none of
/// `self.backend`'s modal-pump-depth machinery (`ModalPumpDepth`/
/// `pump_until_ready` only guard the nested main-loop wait `gtk4::FileDialog`/
/// `AlertDialog` need), so borrowing it here — a second `Backend` instance
/// from the runner's own, each with its own independent `PlatformServices` —
/// is safe: `Clipboard::read_text`/`write_text` talk straight to the OS
/// clipboard (`arboard` on GTK, matching what TUI's `TuiPlatformServices`
/// already uses), not to any runner-owned state.
///
/// `backend` is typed `Box<dyn quadraui::Backend>` directly (#1497), so this
/// names no concrete toolkit type and works unchanged for GTK, macOS, and
/// Win-GUI — every `App::new`/`App::new_portable` caller passes its own
/// concrete backend through the same `Rc<RefCell<Box<dyn
/// quadraui::Backend>>>` seam #861 opened.
///
/// ## #587 follow-up: does `arboard`'s X11 connection contend with GTK's?
///
/// #587's hang was `copypasta_ext`'s `x11_fork` variant calling `fork()`
/// inside this GTK4 process — a multi-threaded fork is what risked a
/// deadlocked child, not "a second X11 connection" per se. `arboard` never
/// forks; its Linux backend owns a background thread with its own XCB
/// connection, entirely separate from GDK's. That is *already* running in
/// every GTK session today regardless of this change: `quadraui::gtk::run`
/// unconditionally constructs its own `GtkBackend` (and therefore its own
/// `GtkPlatformServices`, and therefore its own `arboard::Clipboard::new()`)
/// before `ShellApp::setup` ever runs — see
/// `quadraui::gtk::run::run`/`quadraui::gtk::backend::GtkBackend::new`. That
/// same arboard-backed clipboard is also already exercised interactively
/// through quadraui's own `sidebar_search`/`text_input` Ctrl-Shift-V/
/// middle-click paste paths (quadraui#120/`fd0029f`). No hang has ever been
/// reported against that code path, so this diff adds a *second* independent
/// `arboard::Clipboard` instance to an already-arboard-using process, not a
/// wholly new interaction with GTK's main loop.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub(crate) fn setup_gtk_clipboard(
    engine: &mut Engine,
    backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
) {
    let read_backend = backend.clone();
    let read_image_backend = backend.clone();
    engine.clipboard_read = Some(Box::new(move || {
        read_backend
            .borrow()
            .services()
            .clipboard()
            .read_text()
            .ok_or_else(|| "clipboard empty or unavailable".to_string())
    }));

    engine.clipboard_write = Some(Box::new(move |text: &str| {
        backend
            .borrow()
            .services()
            .clipboard()
            .write_text_result(text)
            .map_err(|e| format!("clipboard write: {e:?}"))
    }));

    // #1464: `Engine::acp_attach_clipboard_image`'s image twin of
    // `clipboard_read` above — GTK's `Clipboard` impl overrides
    // `read_image`, so this is a real decoded-pixel read, not the
    // `Err(BackendError::Unsupported)` default a backend without one
    // returns.
    engine.clipboard_read_image = Some(Box::new(move || {
        read_image_backend
            .borrow()
            .services()
            .clipboard()
            .read_image()
    }));
}

/// A native file dialog requested by [`App::open_file_dialog`] /
/// [`App::save_workspace_as_dialog`], deferred to the next `tick()` (#572).
///
/// Neither of those methods has a
/// `backend: &mut dyn quadraui::Backend` in scope, but `PlatformServices`
/// (and the re-entrancy-guarded nested-mainloop pump backing it, see
/// `quadraui::gtk::services` #427) is only reachable through that
/// runner-owned `backend` parameter. `tick()` receives it every frame, so
/// the request is stashed here and drained there instead of threading
/// `backend` through the whole `dispatch_engine_action`/`handle_menu_action`
/// call graph.
///
/// Deliberately **not** routed through `self.backend` (`App`'s own
/// `Rc<RefCell<GtkBackend>>`, used for modal-stack/drag-state handles) —
/// that is a separate `GtkBackend` instance from the one
/// `quadraui::gtk::run::run` constructs internally and passes as the
/// trait-object `backend` param. Its `PlatformServices` has its own,
/// unshared `pump_depth` counter, so pumping the nested main loop through
/// it would not signal the runner's own event controllers to skip their
/// `backend.borrow_mut()` calls — reintroducing the double-borrow panic
/// #427's guard exists to prevent.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PendingFileDialog {
    OpenFile,
    SaveWorkspaceAs,
}

/// Parse the button index out of a `"dialog:btn:N"` id — the synthesized
/// id convention `render::dialog_panel_to_quadraui_dialog` uses (backends
/// dispatch clicks by index via `Engine::dialog_click_button(idx)`, since
/// `DialogPanel.buttons` carries no engine-side id). Shared (#727) by the
/// in-canvas `DialogHit::Button(id)` hit-test path and the native
/// message-dialog response mapping so both parse the id the same way.
pub(crate) fn dialog_btn_index(id: &quadraui::WidgetId) -> Option<usize> {
    id.as_str()
        .strip_prefix("dialog:btn:")
        .and_then(|s| s.parse::<usize>().ok())
}

/// The app icon to paint in the menu row: the one shared builder
/// ([`crate::render::app_icon_image`]), handed to `Backend::draw_image`
/// unchanged for every backend.
///
/// #1102: before quadraui#1014 added a decode cache to `GtkBackend::draw_image`
/// itself, this forked on `#[cfg(feature = "gui")]` — GTK got a
/// once-rasterised small PNG (`crate::gtk::util::app_icon_image`, deleted)
/// because handing the raw 1024×1024 SVG to the then-uncached `draw_image`
/// meant librsvg re-rendered it every repaint (+16.5 ms/frame on the headless
/// GTK harness); every other backend got the plain SVG builder, unexercised
/// in production since none of them painted this yet. Now that the cache
/// lives inside quadraui, there is nothing left for this function to fork on.
pub(crate) fn app_icon_image_for_paint() -> quadraui::Image {
    crate::render::app_icon_image()
}

/// #1634: the change-detection decision `render::run_shared_tick_chores`
/// gates its `WindowControl::set_title` call on — pulled out of that call
/// site into its own pure, `Backend`-free function so the dedup guard
/// itself (not just its call site) is directly unit-testable.
///
/// Returns `true` (and records `title` as the new baseline in `*last`)
/// exactly when `title` differs from the last title this function was
/// told got written; returns `false` (leaving `*last` untouched) when it
/// is unchanged, so the caller must skip the write.
///
/// # Why a pure function, not a `Backend`-call-count driver test
///
/// The natural black-box shape for this fix — construct a counting/mock
/// `Backend`, drive two ticks with no state change, assert `set_title` is
/// called once instead of twice — is not achievable in this crate:
///
/// - `quadraui::Backend` is a **sealed trait** (`pub(crate) mod sealed`
///   in `quadraui/src/backend.rs`, restated at the very top of the
///   `Backend` trait's own doc). An external crate — vimcode included —
///   cannot write `impl quadraui::Backend for MyMock` at all; the only
///   externally-constructible full implementor quadraui ships is
///   `quadraui::testing::RecordingBackend`, and that type does not
///   override `Backend::window()` (its trait default returns `None`, so
///   `run_shared_tick_chores`'s `if let Some(w) = backend.window()` body —
///   the guard under test — would never even execute against it) or
///   `Backend::set_caret_shape` (trait default is a no-op with no call
///   recorded), so it cannot distinguish "guard present" from "guard
///   absent" for either write this fix touches.
/// - Even the one real `Backend` this crate *can* construct in-process
///   (`quadraui::tui::TuiBackend`, via `quadraui::tui::testing::
///   driver_with_shell`) offers no interception point for this specific
///   pair of writes: `TuiBackend::set_title`/`set_caret_shape` both write
///   straight to real `std::io::stdout()` with no `Terminal`/`Buffer`
///   indirection and no test-mode guard of their own (see each method's
///   own doc in `quadraui/src/tui/backend.rs`, and `set_caret_shape`'s in
///   particular: "there is no real terminal under `TestBackend`") — which
///   is exactly why `tests/conpty_idle_flicker.rs` exists as a *real*
///   ConPTY driver test instead of an in-process one, and exactly why
///   that file's own module doc says #1583's in-process idle-stability
///   test "can only see bytes that flow through the `ratatui::Terminal`'s
///   own `Write` sink."
///
/// This function is the closest available substitute: it isolates the
/// *decision* `run_shared_tick_chores`/`App::tick_dispatch` make before
/// ever touching a `Backend`, with zero I/O, so a regression in the guard
/// itself — the actual code change this issue's fix iteration shipped —
/// is a fast, Linux-runnable, RED/GREEN unit test
/// (`window_title_dedup_tests`, below), independent of real Windows
/// hardware. It cannot prove what happens downstream of the write (that
/// remains `tests/conpty_idle_flicker.rs` and real-hardware/operator
/// verification's job), only that the write is skipped exactly when it
/// should be.
pub(crate) fn dedup_window_title(last: &mut Option<String>, title: &str) -> bool {
    if last.as_deref() == Some(title) {
        false
    } else {
        *last = Some(title.to_string());
        true
    }
}

/// #1634: same guard, for `Backend::set_caret_shape` — see
/// [`dedup_window_title`]'s doc for the full reasoning (including why a
/// `Backend`-call-count driver test is not achievable in this crate).
pub(crate) fn dedup_caret_shape(
    last: &mut Option<quadraui::EditorCursorShape>,
    shape: quadraui::EditorCursorShape,
) -> bool {
    if *last == Some(shape) {
        false
    } else {
        *last = Some(shape);
        true
    }
}

/// #1668: whether `App::tick_dispatch` must re-arm a future `tick` via
/// `Backend::request_frame_in` so `Engine::poll_terminal` (reached via
/// `poll_idle`, inside `handle_poll_tick`/`render::run_shared_tick_chores`)
/// keeps draining a terminal pane's PTY output.
///
/// GTK/TUI/macOS call `tick` unconditionally every
/// `quadraui::runtime::IDLE_POLL_CEILING` (250ms — see
/// `quadraui::runner::ShellApp::tick`'s own per-backend table) regardless
/// of whether anything asked to be woken, so a terminal pane's output
/// drains on its own there even with no explicit re-arm. Win-GUI has
/// **no** such fallback (same table: "Windows | none") — confirmed
/// against the pinned rev: `quadraui::win::run::wndproc` only ever calls
/// `AppLogic::tick` from its own `WM_TIMER` handler
/// (`grep -n "tick(ws" quadraui/src/win/run.rs` has exactly one call
/// site), and that timer only fires once something has called
/// `Backend::request_frame_in` (`WinBackend::request_frame_in`'s
/// `SetTimer`). Dispatching a keypress goes through `App::handle`/
/// `dispatch_event`, never `tick` — so typing into an open terminal pane
/// does not drain its output either. Without this re-arm, a Win-GUI
/// terminal pane's shell output is never drained past the very first
/// frame: vimcode#1668's "panel opens blank and stays blank; typing
/// `echo hello-vimcode` + Enter produces no visible output at all" report.
///
/// A pure, `Backend`-free decision function, mirroring
/// [`dedup_window_title`]'s shape, so the decision itself (not just its
/// call site) is directly unit-testable
/// (`terminal_poll_rearm_tests`, below) — fast, Linux-runnable, no
/// `Backend` of any kind required.
///
/// # Driver-tier coverage lives alongside the TUI port, not here
///
/// [`dedup_window_title`]'s doc explains why a `Backend`-call-count
/// driver test is unreachable via `quadraui::testing::RecordingBackend`
/// (a sealed trait, and that mock's `request_frame_in` is a documented
/// no-op) — that reasoning still holds here, for that one mock. It does
/// **not** hold for every in-process `Backend`, though: the real
/// `quadraui::tui::TuiBackend` a `quadraui::tui::testing::TuiDriver`
/// wraps *does* record every `request_frame_in` call
/// (`TuiBackend::frame_requests`/`pending_frame_delay`, quadraui#832,
/// built for exactly this "prove an app's scheduling decision, not just
/// its painted output" need). `tick_dispatch`'s terminal-pane re-arm
/// above is platform-neutral code — gated only on
/// `Engine::terminal_panes`, never on which concrete `Backend` is
/// plugged in — so driving it through `TuiDriver::tick()` exercises the
/// exact same decision Win-GUI's `WM_TIMER` loop depends on. See
/// `src/tui_main/app_on_tui_tests.rs`'s `terminal_poll_rearm_1668`
/// module for that black-box coverage (RED-verified against this
/// function always returning `None`).
///
/// What that TUI-side driver test still cannot reach is `WinBackend`
/// itself: `WinBackend::attach_headless` (what `WinDriver::new` uses)
/// never sets `self.hwnd`, so `request_frame_in` degrades to its
/// documented "no window to nudge yet" no-op through that harness, and
/// there is no Windows-native equivalent of `TuiDriver::tick()` to drive
/// a real `WM_TIMER` cycle headlessly either (see
/// `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry). Between the pure-
/// function unit test, the `TuiDriver`-based driver test, and that
/// documented gap, what remains unverified by anything in this repo is
/// only the one link this fix cannot touch: whether a real Win32
/// `SetTimer`/`WM_TIMER` cycle actually re-fires `tick` on real Windows
/// hardware — `tick_dispatch` asking for it is now covered twice over.
pub(crate) fn terminal_poll_rearm_delay(
    any_terminal_pane_open: bool,
) -> Option<std::time::Duration> {
    if any_terminal_pane_open {
        Some(std::time::Duration::from_millis(100))
    } else {
        None
    }
}

#[cfg(test)]
mod window_title_dedup_tests {
    //! #1634: direct coverage for [`dedup_window_title`]/
    //! [`dedup_caret_shape`] — see [`dedup_window_title`]'s own doc for why
    //! these pure functions, rather than a `Backend`-call-count driver
    //! test, are this fix's unit coverage.
    //!
    //! RED-verified: replacing either function's body with `true` (i.e.
    //! reverting to the pre-#1634 "write unconditionally every tick"
    //! behaviour) makes each module's second assertion below fail — the
    //! repeated call with an unchanged value stops returning `false`.
    //! Restored before committing.

    use super::*;

    #[test]
    fn window_title_write_is_skipped_only_when_unchanged() {
        let mut last: Option<String> = None;

        assert!(
            dedup_window_title(&mut last, "main.rs"),
            "first call must report a write is needed (no prior baseline)"
        );
        assert_eq!(last.as_deref(), Some("main.rs"));

        assert!(
            !dedup_window_title(&mut last, "main.rs"),
            "a second call with the identical title must report the write \
             should be skipped — this is the exact #1634 regression: \
             `run_shared_tick_chores` used to call `WindowControl::set_title` \
             unconditionally on every idle tick even when the title never \
             changed"
        );
        assert_eq!(
            last.as_deref(),
            Some("main.rs"),
            "skipping the write must not disturb the cached baseline"
        );

        assert!(
            dedup_window_title(&mut last, "right.rs"),
            "a genuinely changed title must still report a write is needed"
        );
        assert_eq!(last.as_deref(), Some("right.rs"));

        assert!(
            !dedup_window_title(&mut last, "right.rs"),
            "and immediately dedups again once the new value is the baseline"
        );
    }

    #[test]
    fn caret_shape_write_is_skipped_only_when_unchanged() {
        let mut last: Option<quadraui::EditorCursorShape> = None;

        assert!(
            dedup_caret_shape(&mut last, quadraui::EditorCursorShape::Block),
            "first call must report a write is needed (no prior baseline)"
        );
        assert_eq!(last, Some(quadraui::EditorCursorShape::Block));

        assert!(
            !dedup_caret_shape(&mut last, quadraui::EditorCursorShape::Block),
            "a second call with the identical shape must report the write \
             should be skipped — the exact #1634 regression for \
             `Backend::set_caret_shape`: `App::tick_dispatch` used to call \
             it unconditionally on every idle tick even when the caret \
             shape never changed"
        );

        assert!(
            dedup_caret_shape(&mut last, quadraui::EditorCursorShape::Bar),
            "a genuinely changed shape must still report a write is needed"
        );
        assert_eq!(last, Some(quadraui::EditorCursorShape::Bar));

        assert!(
            !dedup_caret_shape(&mut last, quadraui::EditorCursorShape::Bar),
            "and immediately dedups again once the new value is the baseline"
        );
    }
}

#[cfg(test)]
mod terminal_poll_rearm_tests {
    //! #1668: isolated unit coverage for [`terminal_poll_rearm_delay`]'s
    //! decision in a vacuum — zero `Backend` of any kind, so it is fast
    //! and trivially RED/GREEN. This is *not* this fix's only coverage:
    //! see [`terminal_poll_rearm_delay`]'s own doc for why a real
    //! `Backend`-call-count driver test is unreachable for
    //! `quadraui::testing::RecordingBackend`/`WinDriver` specifically but
    //! *is* reachable via `quadraui::tui::testing::TuiDriver` — that
    //! black-box half lives in
    //! `src/tui_main/app_on_tui_tests.rs`'s `terminal_poll_rearm_1668`
    //! module, driving the real, shared `App::tick_dispatch` and
    //! asserting on `TuiBackend::frame_requests`/`pending_frame_delay`.
    //!
    //! RED-verified: reverting `terminal_poll_rearm_delay` to always
    //! return `None` (the pre-#1668 behaviour — nothing ever re-arms a
    //! tick for an open terminal pane) makes the first assertion below
    //! fail. Restored before committing.

    use super::*;

    #[test]
    fn rearm_is_requested_only_while_a_terminal_pane_is_open() {
        assert_eq!(
            terminal_poll_rearm_delay(true),
            Some(std::time::Duration::from_millis(100)),
            "with at least one open terminal pane, `tick_dispatch` must \
             ask to be woken again soon so `Engine::poll_terminal` keeps \
             draining the pane's PTY output — this is the exact #1668 \
             regression on Win-GUI: `win::run`'s message loop only calls \
             `tick` again when something explicitly asks via \
             `Backend::request_frame_in`/`SetTimer`, so with no re-arm a \
             terminal pane's shell output (prompt, echoed input, command \
             output) is never drained past the very first frame"
        );

        assert_eq!(
            terminal_poll_rearm_delay(false),
            None,
            "with no terminal pane open, there is nothing to keep polling \
             for — must not request a wake-up"
        );
    }
}

/// Create a new `App` instance.
///
/// All widget-dependent setup (window handle) is deferred to
/// `ShellApp::setup()`, called by the runner once the window exists.
impl App {
    /// The single `App` constructor every GUI/TUI entry point calls (#1498
    /// folded the once-GTK-only `App::new` into this — see this file's
    /// module doc for the history: by the time #1498 landed, the two
    /// constructors differed by exactly one GDK-only step,
    /// [`crate::gtk::util::add_icon_theme_search_path`], which
    /// [`crate::gtk::run`] now calls directly before this constructor
    /// instead of `App::new` calling it inline).
    ///
    /// `backend` is supplied by the caller rather than constructed here
    /// (#861): before this, `App::assemble` hardcoded
    /// `Box::new(backend::GtkBackend::new())`, so nothing upstream of this
    /// function had any seam to hand `App` a different `quadraui::Backend`
    /// impl. `src/gtk/mod.rs::run`, `src/tui_main/mod.rs::run`,
    /// `src/macos/mod.rs::run` and `src/win/mod.rs::run` are today's
    /// callers, each passing its own concrete backend.
    ///
    /// Nothing here needs a live GTK display: the two steps that used to
    /// ([`crate::gtk::util::add_icon_theme_search_path`] and the deleted
    /// `css::load_css`, GDK-only and gone respectively — the latter since
    /// JDonaghy/quadraui#1091 gave `GtkPlatformServices` its own equivalent
    /// stylesheet, reloaded every frame by `sync_per_frame_backend_state`'s
    /// `Backend::set_theme` call) both moved or vanished. `settings_monitor`
    /// (a GTK-only `gio::FileMonitor`) was deleted outright by #949 rather
    /// than replaced — `Engine::check_settings_reload`'s portable mtime
    /// poll, already the sole reload mechanism on TUI, made it redundant on
    /// every backend via the shared `handle_poll_tick`. A GTK-only
    /// `gtk4::Settings` dark/light-variant push used to live here too;
    /// quadraui#1016 moved that into `Backend::set_theme` itself, so it
    /// needed no portable replacement either.
    pub(crate) fn new_portable(
        file_path: Option<PathBuf>,
        backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
    ) -> Self {
        let (engine, last_colorscheme) = Self::build_portable_engine(file_path, &backend, units);
        // SAFETY: the `Rc` is moved into the returned `App`, which the
        // caller hands straight to a `run_with_shell` that owns it for the
        // rest of the process, so the pointer never dangles.
        // `crate::gtk::run`/`crate::tui_main::run`/`crate::macos::run`/
        // `crate::win::run` are today's callers and each does exactly that.
        // [`Self::new_portable_for_test`] below is a second caller of
        // [`Self::build_portable_engine`], but deliberately **not** of
        // `register_emergency_engine` — see its own doc for why a second,
        // test-scoped caller of that unsafe fn would be unsound.
        unsafe {
            crate::core::swap::register_emergency_engine(
                engine.as_ptr() as *const crate::core::Engine
            );
        }

        Self::assemble(
            engine,
            DeferredQueue::new(),
            last_colorscheme,
            backend,
            units,
            true,
        )
    }

    /// Shared prologue for [`Self::new_portable`] and
    /// [`Self::new_portable_for_test`]: build the real, startup-run `Engine`
    /// (not the headless fixture [`Self::new_headless_with_backend`] wraps)
    /// plus the clipboard wiring and last-known colorscheme both callers
    /// need before deciding what to do about the emergency-engine pointer
    /// and `App::live`.
    #[cfg_attr(
        not(any(
            feature = "win",
            all(feature = "macos", target_os = "macos"),
            test,
            feature = "test-support"
        )),
        allow(dead_code)
    )]
    pub(crate) fn build_portable_engine(
        file_path: Option<PathBuf>,
        backend: &Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
    ) -> (Rc<RefCell<Engine>>, String) {
        let mut engine = {
            let mut e = Engine::new();
            // #999: same GUI-backend-then-resolve ordering as `App::new`
            // above — every non-GTK GUI backend this constructor serves
            // (macOS, Win-GUI) bundles the icon font too. #1426: reads
            // `units.is_gui_backend` rather than a hardcoded `true` — every
            // caller of this constructor today (macOS, Win-GUI) passes
            // `UnitProfile::px()`, whose `is_gui_backend` is `true`, so this
            // is not a behaviour change; it just stops the constructor from
            // assuming its own answer.
            crate::icons::set_gui_backend(units.is_gui_backend);
            crate::icons::set_nerd_fonts(e.settings.use_nerd_fonts());
            e.startup(file_path.as_deref());
            e
        };
        setup_gtk_clipboard(&mut engine, backend.clone());

        let last_colorscheme = engine.settings.colorscheme.clone();
        (Rc::new(RefCell::new(engine)), last_colorscheme)
    }

    /// Test/headless twin of [`Self::new_portable`] — the seam
    /// `tui_main::testing::tui_driver`/`tui_driver_with` (#1500) build on,
    /// so an App-on-TUI scenario runs through the *exact same* construction
    /// path `tui_main::run`/`crate::macos::run` use (real `Engine::startup`,
    /// real clipboard wiring), rather than the headless
    /// [`Self::new_headless_with_backend`] shortcut every `crate::harness`
    /// scenario uses on a caller-supplied fixture `Engine`.
    ///
    /// Differs from [`Self::new_portable`] in exactly two ways, both because
    /// this `App` is dropped at the end of a test function rather than
    /// living "for the rest of the process":
    ///
    /// - **No `core::swap::register_emergency_engine` call.** That fn's own
    ///   safety contract requires the pointee to outlive the process;
    ///   registering it here would leave the process-global
    ///   `EMERGENCY_ENGINE` static holding a dangling `*const Engine` the
    ///   moment this function's caller's `App`/driver drops — a
    ///   use-after-free the next test's panic hook (or an external crate's
    ///   own crash hook, since this constructor backs a `test-support`
    ///   seam) could dereference. Exactly the hazard
    ///   [`Self::new_headless_with_backend`]'s own doc names, and the one
    ///   the pre-#1434 TUI shell's `setup`'s `if self.live` gate exists to avoid on the
    ///   production TUI path (`src/tui_main/shell_app.rs`).
    /// - **Passes `live: false`** to [`Self::assemble`], not `true` — see
    ///   `App::live`'s own doc: that flag gates `tick_dispatch`'s
    ///   `backend.set_caret_shape` call, whose only real-writing override
    ///   (`TuiBackend`) writes a raw DECSCUSR escape sequence straight to
    ///   the test process's real stdout with no test-mode guard of its own.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new_portable_for_test(
        file_path: Option<PathBuf>,
        backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
    ) -> Self {
        let (engine, last_colorscheme) = Self::build_portable_engine(file_path, &backend, units);
        Self::assemble(
            engine,
            DeferredQueue::new(),
            last_colorscheme,
            backend,
            units,
            false,
        )
    }

    /// Map a built-in activity-bar panel id to its glyph — the single table
    /// every backend's `shell_config` builder resolves icons from (#1107).
    ///
    /// Before this, the pre-#1434 TUI shell's own `build_shell_config` carried
    /// its own icon literal (zipped positionally against
    /// `sidebar::FIXED_ACTIVITY_PANEL_IDS`), entirely independent of this
    /// match — which is exactly how the search panel ended up resolving to
    /// two different glyphs across backends for months (`SEARCH_COD` on
    /// GTK, `SEARCH` on TUI, converged by #950, but the *machinery* that let
    /// them drift in the first place — two hand-maintained tables — stayed
    /// in place until now). `shell_config_resolves_the_same_search_icon_on_
    /// every_backend` below pins the fact directly.
    ///
    /// Returns `None` for any id this table doesn't know — extension panels
    /// resolve their own icon before ever reaching a `shell_config` builder
    /// (see `Engine::ext_activity_panels`), so a caller should leave an
    /// unmatched panel's icon untouched rather than blank it out.
    pub(crate) fn resolve_builtin_panel_icon(id: &str) -> Option<&'static str> {
        Some(match id {
            "panel:explorer" => crate::icons::EXPLORER.s(),
            "panel:search" => crate::icons::SEARCH.s(),
            "panel:debug" => crate::icons::RUN_AND_DEBUG.s(),
            "panel:git" => crate::icons::SOURCE_CONTROL.s(),
            "panel:extensions" => crate::icons::EXTENSIONS.s(),
            "panel:ai" => crate::icons::AI_CHAT.s(),
            "panel:board" => crate::icons::BOARD.s(),
            "bottom:settings" => crate::icons::SETTINGS.s(),
            crate::core::engine::sidebar::HAMBURGER_PANEL_ID => crate::icons::HAMBURGER.s(),
            _ => return None,
        })
    }

    /// Derive the runner's [`quadraui::ShellConfig`] from this `App`'s engine
    /// state — the backend-neutral core of what every GUI entry point needs
    /// before it can call `run_with_shell` (#859).
    ///
    /// The engine's `AppShell` initialises every `PanelDefinition.icon` to
    /// `""` because the engine is backend-agnostic, so *somebody* has to map
    /// panel ID → glyph; doing it here rather than per-backend is the whole
    /// point, since the mapping is a product decision, not a platform one.
    ///
    /// **The duplication this used to have with `src/gtk/mod.rs` is gone
    /// (#866).** Through #859, `src/gtk/mod.rs::build_shell_config` carried
    /// its own full copy of the panel-icon-mapping logic below plus two
    /// GTK/WM-only builders (`with_app_id` / `with_icon_name`, which carry
    /// `crate::gtk::util::APP_ID` — an X11/Wayland identity string with no
    /// macOS/Windows meaning) — left unmerged because #859 was explicitly
    /// forbidden from touching `src/gtk/` (a `smoke_tests.capability_rules`
    /// boundary; see that issue). #866's Files list lifts that specific
    /// restriction for exactly this one function: `build_shell_config` is
    /// now a thin `app.shell_config().with_app_id(…).with_icon_name(…)`
    /// wrapper, so this method is the *only* place the panel/title-bar/
    /// sidebar-clamp logic lives — every GUI entry point (GTK, macOS, and
    /// now Win-GUI) calls through it, and a future change here can no
    /// longer silently miss one backend the way three independent copies
    /// could have.
    #[cfg_attr(
        not(any(
            feature = "gui",
            feature = "win",
            all(feature = "macos", target_os = "macos")
        )),
        allow(dead_code)
    )]
    pub(crate) fn shell_config(&self) -> quadraui::ShellConfig {
        // The engine stores all panels (including "bottom:settings") in a
        // single `panels()` slice; `ShellConfig` wants top-pinned panels in
        // its first arg and bottom-pinned items via `with_bottom_items()`, so
        // split on the "bottom:" ID prefix.
        let panels_with_icons: Vec<_> = self
            .engine
            .borrow()
            .app_shell
            .panels()
            .iter()
            .cloned()
            .map(|mut p| {
                if let Some(icon) = Self::resolve_builtin_panel_icon(p.id.as_str()) {
                    p.icon = icon.to_string();
                }
                p
            })
            .collect();
        let (mut top_panels, bottom_items): (Vec<_>, Vec<_>) = panels_with_icons
            .into_iter()
            .partition(|p| !p.id.as_str().starts_with("bottom:"));
        // #557: plugin-provided panels (e.g. the Git Insights extension) live
        // in `engine.ext_panels`, not in the engine's `AppShell` — nothing
        // registers them there — so they have to be appended explicitly or
        // the runner's activity bar renders no icon for them at all.
        // `ext_activity_panels` already carries each panel's resolved icon,
        // so the id→glyph match above deliberately needs no arm for them.
        top_panels.extend(self.engine.borrow().ext_activity_panels());

        // #1427: on the `cell` profile (TUI, and any future `App`-hosted
        // backend whose `BackendCaps::window_chrome` is `false` — see
        // `App::setup`'s three-way branch) the menu bar starts hidden and is
        // fully hideable, so it needs a hamburger `PanelDefinition` to
        // reveal it — the pre-#1434 TUI shell's `build_shell_config`'s own
        // first panel, ported here verbatim (same id/icon/title/tooltip).
        // `px()` backends (GTK/macOS/Win) never take this branch, so their
        // shadow-`app_shell`-derived `top_panels` list is unaffected — the
        // hamburger is *runner-only* either way (see
        // `render::reclaim_hamburger_sidebar_reservation`'s doc for why the
        // shadow never gets a matching entry).
        if !self.units.is_gui_backend {
            top_panels.insert(
                0,
                quadraui::PanelDefinition {
                    id: quadraui::WidgetId::new(crate::core::engine::sidebar::HAMBURGER_PANEL_ID),
                    icon: Self::resolve_builtin_panel_icon(
                        crate::core::engine::sidebar::HAMBURGER_PANEL_ID,
                    )
                    .unwrap_or_default()
                    .to_string(),
                    title: "Menu".to_string(),
                    tooltip: "Menu".to_string(),
                },
            );
        }

        // (#552/#710) Reserve a full-width title-bar band across the top of
        // the shell. `App::render_content` paints vimcode's own menu bar and
        // inline window controls into it, so a GUI backend that does not
        // reserve it loses the menu bar entirely. `height_lh` is a
        // line-height *multiple* (no fixed-px reservation API exists yet).
        //
        // #940 raised this from `1.7` to `2.0`. `1.7` measured only ~29px in
        // `gtk::testing::command_center`'s headless harness — a single
        // pixel over the ~28pt macOS traffic-light cluster this band now
        // shares space with (quadraui#947's client-side titlebar, opted
        // into below), which is not a safety margin, it is rounding noise:
        // any font metrics difference between that harness and a real
        // window (different backend, different DPI/hinting) could put the
        // band at or under 28px and collide with the native controls this
        // issue exists to keep clip-free. `2.0` measures ~34px in the same
        // harness — VS Code's own title bar is ~35px, so this is *closer*
        // to VS Code parity than `1.7` was, not a compromise for it — and
        // leaves ~6px of margin instead of ~1px.
        //
        // This is still not an absolute guarantee: `height_lh` is a
        // *multiple* of the live line height, which tracks
        // `settings.ui_font_size`/`settings.font_size` (both user-settable
        // down to 6, see `core::settings`'s clamps), and is fixed once here
        // at `ShellConfig`-construction time while `AppShell` recomputes the
        // band's pixel height from the *current* line height every frame —
        // there is no runtime hook to re-derive the multiple when the user
        // later shrinks their font, and no fixed-pixel-floor knob on
        // `ShellConfig::with_title_bar` to fall back to (unlike the
        // activity bar's own fixed-pixel-width field below, which exists
        // for exactly this reason). Concretely: a user who runs
        // `:set font_size=6` at runtime can still shrink the band under
        // macOS's real traffic-light height, and nothing in this file can
        // stop that without quadraui growing a `with_title_bar_min_px`-style
        // option (mirroring #657's activity-bar pattern) that this call
        // site could opt into. That is a quadraui issue to file, not a
        // vimcode-side fix (Platform-Neutrality Rule) — `2.0` closes the
        // realistic gap at default and adjusted-but-still-reasonable font
        // sizes; the pathological extreme remains open pending that API.
        let mut cfg = quadraui::ShellConfig::new("VimCode", top_panels)
            .with_bottom_items(bottom_items)
            .with_title_bar(self.units.title_bar_lh);
        // #1427: `with_title_bar` above always sets `has_title_bar: true` —
        // correct for GTK/macOS/Win, whose menu bar is either the CSD
        // titlebar (`window_chrome`) or hidden behind a real OS one
        // (`native_menu`), so the band is reserved unconditionally from
        // frame zero. The `cell` profile's menu bar is fully hideable
        // instead (`App::setup`'s three-way branch), so its *initial*
        // reservation has to follow the engine's own already-resolved
        // `menu_bar_visible` (true only in vscode-mode — `Engine::new`'s own
        // default) the same way the pre-#1434 TUI shell's `build_shell_config`'s
        // `menu_bar_visible` parameter did pre-#1427. Every dispatch after
        // frame zero keeps this in sync via `render::sync_menu_bar_title_row`.
        if !self.units.is_gui_backend {
            cfg.has_title_bar = self.engine.borrow().menu_bar_visible;
        }
        // #940/quadraui#947: opt into the client-side titlebar so a
        // capable backend (macOS today) puts the reserved band *in* the
        // real titlebar, beside the native traffic lights, instead of
        // underneath it. Requested unconditionally (via `self.units`, which
        // is `true` on every GUI backend) rather than gated on `target_os =
        // "macos"` (the Platform-Neutrality Rule) — GTK and Win-GUI simply
        // don't honour this field yet (`ShellConfig::client_side_titlebar`'s
        // own doc, and `ACCEPTED_DEFAULTS` in quadraui's
        // `tests/conformance/caps.rs`), so setting it there is inert today
        // and each backend adopts it on its own schedule with no
        // vimcode-side change needed. `UnitProfile::cell()` (the `tui`
        // harness arm) leaves it off, matching the pre-#1434 TUI shell's
        // `build_shell_config`, which never calls this either.
        if self.units.client_side_titlebar {
            cfg = cfg.with_client_side_titlebar();
        }
        // #719/quadraui#657: on GTK/macOS/Win the activity bar's row height
        // is fixed (VS Code parity), so sizing its *width* from the editor
        // font makes it oblong — `UnitProfile::px()` pins a fixed-pixel
        // width. `UnitProfile::cell()` leaves this `None`, so
        // `ShellConfig::activity_bar_width`'s own default (a 3-line-height
        // multiple) stays in charge, matching the pre-#1434 TUI shell's
        // `build_shell_config`'s explicit `3.0`. Assigned to the field
        // directly — `ShellConfig`'s own builder method does the identical
        // one-line `Some(..)` assignment.
        cfg.activity_bar_width_px = self.units.activity_bar_width_px;
        // #947: seed the *initial* editor font from `settings.font_family`/
        // `font_size` before the runner's first frame, not just via
        // `sync_per_frame_backend_state`'s per-frame `set_editor_font` call.
        //
        // quadraui's GTK runner measures `char_width()`/`line_height()` from
        // `Backend::editor_font_pango_string()` ONCE PER FRAME, but that
        // measurement happens *before* it calls into `App::render_content`
        // (`gtk/run.rs`'s per-frame prologue seeds `current_char_width`/
        // `current_line_height`, then hands control to the `ShellApp`).
        // `sync_per_frame_backend_state`'s `set_editor_font` call, made
        // *inside* `render_content`, therefore only takes effect for the
        // *next* frame's measurement — for exactly one frame (the very
        // first), the backend's `editor_font_*` would still be its own
        // built-in default ("Monospace 11") rather than
        // `settings.font_size` (14 by default), so that first frame's grid
        // math (gutter width, cursor position, click resolution) would use
        // the wrong `char_width`/`line_height` even though the glyphs
        // painted that same frame already used the new font description.
        // `with_editor_font` closes that gap: the runner applies it via
        // `Backend::set_editor_font` during its own one-time `setup()`,
        // before the first frame's measurement ever runs, so frame 1 is
        // already consistent — `sync_per_frame_backend_state`'s per-frame
        // call remains the only thing that matters for a runtime `:set
        // guifont`/`:set font_size=N`/`zoomin`/`zoomout` after that.
        //
        // #1542: resolved through `app_support::resolve_editor_font` rather
        // than reading `settings.font_family`/`font_size` verbatim, so a
        // user who has never customized either setting gets *this*
        // backend's platform-native convention (Menlo 12 on macOS,
        // Consolas 14 on Win-GUI, ...) instead of one hardcoded literal on
        // every backend — see that function's doc. `self.backend` is
        // already the live concrete backend by this point (`App::new_portable`
        // stores it before `build_shell_config`/`shell_config` ever runs),
        // so `default_fonts()` answers for real here, not a guess.
        let (editor_family, editor_size_pt) =
            resolve_editor_font(&self.engine.borrow().settings, &**self.backend.borrow());
        cfg = cfg.with_editor_font(editor_family, editor_size_pt);
        // #1798: the width the sidebar *opens* at is a line-height
        // **multiple** (quadraui's `AppShell::compute_layout` multiplies it
        // by `line_height`), so leaving it at `ShellConfig::new`'s generic
        // `20.0` meant a 20-column strip on TUI but a ~460px slab on a GUI
        // backend — which left an 800px-wide window no room to paint a
        // second editor tab. `UnitProfile::sidebar_width_lh` carries the
        // per-unit value; see its field doc for why the GUI profile uses
        // exactly `ALT_SIDEBAR_WIDTH_MIN` and not less.
        //
        // Read from `self.units` for the same reason `activity_bar_width_px`
        // above is: the value differs per *unit*, not per *backend*, so it
        // belongs in the one profile each backend already picks at
        // construction rather than in a `cfg!`/`if gtk` branch here
        // (Platform-Neutrality Rule).
        cfg.default_sidebar_width = self.units.sidebar_width_lh;
        // #759: the shared Alt rung clamps sidebar width, so Alt+Left/Right
        // resolve identically on every backend. These two stay *shared*
        // (unlike the opening width above) — they bound the rung itself, and
        // quadraui's own `set_sidebar_width` clamp must not be narrower than
        // it on one backend and not another. #1798 therefore also requires
        // `default_sidebar_width >= min_sidebar_width`: `compute_layout`
        // clamps the opening width through this floor, so an opening width
        // below it would silently resolve back up to it.
        debug_assert!(
            self.units.sidebar_width_lh >= render::ALT_SIDEBAR_WIDTH_MIN as f32,
            "a sidebar that opens below the shared Alt rung's floor cannot be \
             narrowed back to its opening width by Alt+Left (#1798)"
        );
        cfg.min_sidebar_width = render::ALT_SIDEBAR_WIDTH_MIN as f32;
        cfg.max_sidebar_width = render::ALT_SIDEBAR_WIDTH_MAX as f32;
        // #1877: reserve one line-height each for the always-present
        // command line and the (near-always-present) status line at the
        // *shell* level, so `AppShellLayout::activity_bar_bounds` — and
        // `sidebar_header_bounds`/`sidebar_content_bounds` — stop one-plus
        // rows above the window's bottom edge instead of running the full
        // window height. Without this, `AppShell::compute_layout` has no
        // idea vimcode paints its own bottom chrome underneath
        // `main_content_bounds`, so the activity bar's bottom-pinned
        // Settings button ends up pinned to the *window's* bottom edge —
        // the exact same y-range vimcode's own status/command-line rows
        // occupy — instead of ending above them the way VS Code's does.
        //
        // `with_status_bar()`/`with_command_line()` each reserve a static
        // `line_height.round()` row — not vimcode's own dynamic
        // `render::status_bar_height_px`/wildmenu/quickfix bottom-chrome
        // total, which quadraui's `ShellConfig` has no knob for (`#[cfg]`-
        // gating a dynamic per-frame height onto `AppShellLayout` would be
        // new backend-shaped decision logic, not thin wiring — see
        // `CLAUDE.md`'s Platform-Neutrality Rule). Two static rows exactly
        // matches the common case (one status row + the always-present
        // command line) that this issue reports; a frame with wildmenu or
        // quickfix *also* open still has a few extra rows of bottom chrome
        // this reservation doesn't know about, a strictly smaller residual
        // gap than today's "reserves nothing at all". The inverse also
        // exists and is worth naming rather than leaving implicit: with
        // `laststatus=0` (or `1` with a single window, `global_status_bar`
        // stays `None`) vimcode's actual bottom chrome is one row, not
        // two, so this static reservation over-reserves by a row — the
        // activity bar/sidebar stop one row higher than strictly
        // necessary. Harmless on its own (an extra row of the activity
        // bar's own `theme.tab_bar_bg` fill, not a gap), but it widens
        // the same reclaimed-strip accounting `bottom_chrome_reservation_
        // fill_rect` below has to get right either way. Neither band is
        // painted by quadraui itself — `render_content` keeps painting
        // vimcode's own status/command-line rows into
        // `main_content_bounds` exactly as before; see
        // `render::main_content_true_height`'s doc for how `render_content`
        // un-does this reservation's shrink of `main_content_bounds`
        // before computing editor/status/command-line geometry, so this
        // reservation affects only the activity bar/sidebar's painted
        // height, never where the editor or its chrome actually paint.
        cfg = cfg.with_command_line().with_status_bar();
        cfg
    }

    /// Build the `App` struct itself from already-prepared, display-*independent*
    /// inputs.
    ///
    /// Split out of [`App::new`] (#646) so the headless GTK test harness
    /// ([`App::new_headless`]) can reach the same field initialisation without
    /// re-running `new`'s display-dependent prologue — `load_css` unwraps
    /// `gdk::Display::default()` and panics outright with no `DISPLAY`, and
    /// `register_emergency_engine` would leave a dangling `*const Engine` in a
    /// process-global once a short-lived test's `App` is dropped (the same
    /// soundness trap #635 documented on the pre-#1434 TUI shell's `live`).
    ///
    /// Everything below this line is plain `Rc`/`Cell`/`RefCell` allocation;
    /// none of it touches GDK. `backend` is taken as a parameter rather than
    /// constructed here (#861) — see [`App::new_portable`]'s doc comment.
    ///
    /// Named no toolkit type in its own signature even before #862 (#861
    /// already erased `backend`'s concrete type), so it stays un-gated
    /// itself; called from every constructor above/below (`new_portable`,
    /// `new_portable_for_test`, `new_headless_with_backend`).
    pub(crate) fn assemble(
        engine: Rc<RefCell<Engine>>,
        deferred: DeferredQueue,
        last_colorscheme: String,
        backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
        live: bool,
    ) -> Self {
        // #1428: computed before `engine` moves into the struct literal
        // below — see `App::pending_startup_msg`'s own doc for why this
        // has to be shared across every constructor rather than living
        // only in `new_portable`.
        let pending_startup_msg = if !units.is_gui_backend {
            let e = engine.borrow();
            let resolved_nerd_fonts = e.settings.use_nerd_fonts();
            let nerd_fonts_undiscovered =
                e.settings.use_nerd_fonts.is_none() && !resolved_nerd_fonts;
            if nerd_fonts_undiscovered {
                Some(
                    "Using ASCII fallback icons. If your terminal has a Nerd Font, run \
                     :CheckNerdFonts to check and enable them."
                        .to_string(),
                )
            } else {
                None
            }
        } else {
            None
        };
        App {
            engine,
            draw_needed: Rc::new(Cell::new(false)),
            exit_requested: Cell::new(false),
            yank_hl_deadline: Cell::new(None),
            pending_file_dialog: Cell::new(None),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
            last_editor_pointer: Rc::new(Cell::new(None)),
            cached_ui_line_height: 20.0,
            dialog_layout: Rc::new(RefCell::new(None)),
            folder_picker: RefCell::new(None),
            native_dialog_shown: Rc::new(Cell::new(false)),
            pending_native_dialog: Rc::new(Cell::new(None)),
            line_height_cell: Rc::new(Cell::new(24.0)),
            char_width_cell: Rc::new(Cell::new(9.0)),
            mouse_pos_cell: Rc::new(Cell::new((-1.0, -1.0))),
            fr_input_dragging: false,
            explorer_drag_src: None,
            explorer_drag_active: None,
            deferred,
            last_clipboard_content: None,
            tab_close_hover: None,
            cached_tab_slots_abs: Rc::new(RefCell::new(HashMap::new())),
            cached_group_tab_bar_layouts: Rc::new(RefCell::new(HashMap::new())),
            status_segment_map: Rc::new(RefCell::new(HashMap::new())),
            separated_status_bar_rect: Rc::new(Cell::new(None)),
            global_status_zones: Rc::new(RefCell::new(Vec::new())),
            cached_screen_layout: Rc::new(RefCell::new(None)),
            cached_frame_hit_map: Rc::new(RefCell::new(None)),
            sidebar_pointer_captured: Cell::new(false),
            cached_sc_bands: Cell::new(None),
            cached_tab_bar_zones: Rc::new(RefCell::new(HashMap::new())),
            cached_drop_ctx: Rc::new(RefCell::new(render::TabDropCtx::default())),
            cached_explorer_metrics: Rc::new(Cell::new((16.0, 8.0))),
            cached_ai_chat_metrics: Rc::new(Cell::new((16.0, 8.0))),
            debug_toolbar_y_offset: Rc::new(Cell::new(0.0)),
            debug_toolbar_height: Rc::new(Cell::new(0.0)),
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            divider_grab: None,
            tab_drag: render::TabDragState::default(),
            csd_applied: Cell::new(false),
            // Matches `SessionState::default()`'s window geometry
            // (`core/session.rs`) — the same fallback `save_session_and_exit`
            // used before #1234 when `self.window` was `None`.
            cached_window_width: Cell::new(800),
            cached_window_height: Cell::new(600),
            cached_window_x: Cell::new(None),
            cached_window_y: Cell::new(None),
            cached_window_maximized: Cell::new(false),
            window_geometry_restored: Cell::new(false),
            cached_editor_bounds: Cell::new(None),
            cached_main_content_height: Cell::new(600.0),
            menu_row_rect: Rc::new(Cell::new(quadraui::Rect::default())),
            menu_items_rect: Cell::new(quadraui::Rect::default()),
            title_bar_rect: Rc::new(Cell::new(quadraui::Rect::default())),
            title_bar_interaction: RefCell::new(quadraui::StatusBarInteraction::new()),
            last_window_control_action: Rc::new(Cell::new(None)),
            last_sc_refresh: std::time::Instant::now(),
            panel_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            panel_hover_popup_rect: Rc::new(Cell::new(None)),
            editor_hover_popup_rect: Rc::new(Cell::new(None)),
            completion_layout: Rc::new(RefCell::new(None)),
            context_menu_layout: Rc::new(RefCell::new(None)),
            tab_switcher_popup_rect: Rc::new(Cell::new(None)),
            composed_frame: Rc::new(RefCell::new(Vec::new())),
            composed_editor_band: Rc::new(RefCell::new(Vec::new())),
            composed_bottom_band: Rc::new(RefCell::new(Vec::new())),
            tab_visible_counts: Rc::new(RefCell::new(Vec::new())),
            picker_popup_rect: Rc::new(Cell::new(None)),
            folder_picker_popup_rect: Rc::new(Cell::new(None)),
            painted_sidebar_bounds: Rc::new(Cell::new(None)),
            painted_line_height: Rc::new(Cell::new(None)),
            painted_char_width: Rc::new(Cell::new(None)),
            editor_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            editor_hover_scrollbar: Rc::new(Cell::new(None)),
            last_colorscheme,
            last_window_title: None,
            last_caret_shape: None,
            backend,
            units,
            // #1064: seeded to `None` rather than the active panel — GTK
            // has no hamburger `PanelDefinition` (unlike TUI's `AppShell`,
            // which activates index 0 = hamburger at construction while
            // the shadow starts on Explorer), so the runner's initial
            // active panel already matches the shadow's default
            // (`shell_config`'s `top_panels` come straight from
            // `engine.app_shell.panels()`). The first `take_requested_panel`
            // poll still reconciles cleanly from `None`: it just re-applies
            // whichever panel is already active, a harmless no-op switch.
            last_shell_panel: None,
            suppress_shell_panel_echo: false,
            keyboard_enhanced: false,
            pending_startup_msg,
            live,
        }
    }

    /// Build an `App` around a caller-supplied, fully in-memory [`Engine`] with
    /// **no** display-dependent setup — the GTK twin of the pre-#1434 TUI shell's `new` for
    /// tests (#646). Feed the result to `crate::gtk::testing::harness`, which
    /// wraps it in `quadraui::gtk::testing::driver_with_shell`.
    ///
    /// Deliberately skips, relative to [`App::new_portable`]:
    ///
    /// - `crate::gtk::util::add_icon_theme_search_path`'s
    ///   `gdk::Display::default()` icon-theme search path — GDK-only, and
    ///   `crate::gtk::run` (its sole caller) is skipped entirely here.
    /// - `setup_gtk_clipboard` (#1100), which would install
    ///   `backend.services().clipboard()` callbacks. Every other test in
    ///   `src/gtk/testing.rs` that needs `engine.clipboard_read`/
    ///   `clipboard_write` installs its own capture-only closure directly
    ///   instead of calling through here, for exactly this reason — the
    ///   one exception,
    ///   `setup_gtk_clipboard_round_trips_yank_and_paste_through_real_backend_1100`,
    ///   builds its own real (non-headless) `GtkBackend` and calls
    ///   `setup_gtk_clipboard` explicitly rather than going through this
    ///   constructor.
    /// - `Engine::startup`, which would restore *the developer's real last
    ///   session*. Tests pass the exact buffers/groups they mean to assert on.
    /// - `core::swap::register_emergency_engine`, whose contract is that the
    ///   engine outlives the process; a test's `App` is dropped at the end of
    ///   the test function, leaving a dangling `*const Engine` for any later
    ///   test's panic hook to dereference (#635 documents the same trap on the
    ///   TUI side).
    /// - The `gio` settings-file monitor (no runtime main loop to service it).
    ///
    /// Takes the engine behind an `Rc` so the caller keeps a handle for
    /// assertions — `GtkDriver` only exposes the opaque `ShellAdapter`, with no
    /// accessor back to the concrete `App` (the same constraint the TUI tests
    /// document on `driver_with_shell`).
    #[cfg(all(feature = "gui", any(test, feature = "test-support")))]
    pub(crate) fn new_headless(engine: Rc<RefCell<Engine>>) -> Self {
        Self::new_headless_with_backend(
            engine,
            Rc::new(RefCell::new(Box::new(backend::GtkBackend::new()))),
            render::UnitProfile::px(),
        )
    }

    /// Backend-parameterised core of [`App::new_headless`] — the same seam
    /// [`App::new_portable`] is to [`App::new`], for the *test* constructors
    /// (#896).
    ///
    /// `new_headless` hardcoded `GtkBackend`, which made it unusable from the
    /// macOS driver-tier test that `src/macos/mod.rs`'s "Verifying this file
    /// without a Mac" note defers to #859 stage 3: that test has to hand the
    /// same `App` a `MacBackend` instead, because the whole point is to paint
    /// through quadraui's macOS rasterisers. Taking the backend as a
    /// parameter keeps **one** headless constructor rather than a second copy
    /// per backend — the `Box<dyn quadraui::Backend>` trait object is already
    /// the only place either backend's concrete type appears.
    ///
    /// Ungated on `gui` deliberately: every caller is a test lane, and the
    /// macOS lane (`--no-default-features --features macos`) compiles no GTK
    /// at all.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new_headless_with_backend(
        engine: Rc<RefCell<Engine>>,
        backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
    ) -> Self {
        // #999/#1426: this constructor is the shared headless `App` test
        // seam for every backend `crate::harness::build_app_and_config`
        // wraps — GTK, the macOS driver-tier test, and (since #1425) the
        // `tui` harness arm — so `units.is_gui_backend` (not a hardcoded
        // `true`) decides `use_nerd_fonts()` resolution the same way
        // `App::new`/`App::new_portable` do for their own single backend.
        // Before this, every `tui`-arm scenario built through
        // `build_app_and_config` ran with `is_gui_backend` wrongly forced
        // `true`.
        crate::icons::set_gui_backend(units.is_gui_backend);
        let (use_nerd_fonts, last_colorscheme) = {
            let e = engine.borrow();
            (e.settings.use_nerd_fonts(), e.settings.colorscheme.clone())
        };
        // Path-qualified rather than via the `use` at the top of this file —
        // that import is `gui`-gated and this constructor is not (#896).
        crate::icons::set_nerd_fonts(use_nerd_fonts);
        Self::assemble(
            engine,
            DeferredQueue::new(),
            last_colorscheme,
            backend,
            units,
            false,
        )
    }
}
