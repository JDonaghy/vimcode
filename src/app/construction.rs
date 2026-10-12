use super::*;

/// Set up system clipboard callbacks on the engine via
/// `backend.services().clipboard()`.
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
/// `self.backend`'s modal-pump-depth machinery, so borrowing it here — a
/// second `Backend` instance from the runner's own, each with its own
/// independent `PlatformServices` — is safe: `Clipboard::read_text`/
/// `write_text` talk straight to the OS clipboard (`arboard` on GTK,
/// matching what TUI's `TuiPlatformServices` already uses), not to any
/// runner-owned state.
///
/// `backend` is typed `Box<dyn quadraui::Backend>` directly, so this
/// names no concrete toolkit type and works unchanged for GTK, macOS, and
/// Win-GUI — every `App::new`/`App::new_portable` caller passes its own
/// concrete backend through the same `Rc<RefCell<Box<dyn
/// quadraui::Backend>>>` seam.
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

    // `Engine::acp_attach_clipboard_image`'s image twin of
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
/// [`App::save_workspace_as_dialog`], deferred to the next `tick()`.
///
/// Neither of those methods has a `backend: &mut dyn quadraui::Backend`
/// in scope, but `PlatformServices` is only reachable through that
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
/// the guard exists to prevent.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PendingFileDialog {
    OpenFile,
    SaveWorkspaceAs,
}

/// Parse the button index out of a `"dialog:btn:N"` id — the synthesized
/// id convention `render::dialog_panel_to_quadraui_dialog` uses (backends
/// dispatch clicks by index via `Engine::dialog_click_button(idx)`, since
/// `DialogPanel.buttons` carries no engine-side id). Shared by the
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
/// Now that `GtkBackend::draw_image` includes a decode cache, there is
/// nothing left for this function to fork on — every backend gets the
/// plain SVG builder.
pub(crate) fn app_icon_image_for_paint() -> quadraui::Image {
    crate::render::app_icon_image()
}

/// The change-detection decision `render::run_shared_tick_chores`
/// gates its `WindowControl::set_title` call on — pulled out of that call
/// site into its own pure, `Backend`-free function so the dedup guard
/// itself (not just its call site) is directly unit-testable.
///
/// Returns `true` (and records `title` as the new baseline in `*last`)
/// exactly when `title` differs from the last title this function was
/// told got written; returns `false` (leaving `*last` untouched) when it
/// is unchanged, so the caller must skip the write.
///
/// This pure function isolates the *decision* `run_shared_tick_chores`
/// make before ever touching a `Backend`, with zero I/O, so a regression
/// in the guard itself is a fast, Linux-runnable unit test, independent
/// of real Windows hardware.
pub(crate) fn dedup_window_title(last: &mut Option<String>, title: &str) -> bool {
    if last.as_deref() == Some(title) {
        false
    } else {
        *last = Some(title.to_string());
        true
    }
}

/// Same guard, for `Backend::set_caret_shape` — see
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

/// Whether `App::tick_dispatch` must re-arm a future `tick` via
/// `Backend::request_frame_in` so `Engine::poll_terminal` keeps draining
/// a terminal pane's PTY output.
///
/// GTK/TUI/macOS call `tick` unconditionally every
/// `quadraui::runtime::IDLE_POLL_CEILING` (250ms) regardless of whether
/// anything asked to be woken, so a terminal pane's output drains on its
/// own there. Win-GUI has **no** such fallback — `WinBackend::request_
/// frame_in` is the only way to wake the `WM_TIMER` handler. Without this
/// re-arm, a Win-GUI terminal pane's shell output is never drained.
///
/// A pure, `Backend`-free decision function, mirroring
/// [`dedup_window_title`]'s shape, so the decision itself is directly
/// unit-testable.
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
    //! Direct coverage for [`dedup_window_title`]/
    //! [`dedup_caret_shape`] — see [`dedup_window_title`]'s own doc for why
    //! these pure functions are unit-testable in isolation.

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
    //! Isolated unit coverage for [`terminal_poll_rearm_delay`]'s decision
    //! in a vacuum — zero `Backend` of any kind, so it is fast and trivially
    //! RED/GREEN. See [`terminal_poll_rearm_delay`]'s own doc for
    //! driver-tier coverage via `quadraui::tui::testing::TuiDriver` in
    //! `src/tui_main/app_on_tui_tests.rs`'s `terminal_poll_rearm_1668` module.

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
    /// The single `App` constructor every GUI/TUI entry point calls.
    ///
    /// `backend` is supplied by the caller rather than constructed here.
    /// `src/gtk/mod.rs::run`, `src/tui_main/mod.rs::run`,
    /// `src/macos/mod.rs::run` and `src/win/mod.rs::run` each pass their own
    /// concrete backend.
    ///
    /// Nothing here needs a live GTK display: display-dependent setup
    /// (`add_icon_theme_search_path`, CSS loading) has been moved or removed.
    /// A GTK-only `gio::FileMonitor` for settings was deleted in favor of the
    /// portable `Engine::check_settings_reload` mechanism.
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
            // Every non-GTK GUI backend (macOS, Win-GUI) bundles the icon
            // font too. Reads `units.is_gui_backend` rather than assuming
            // `true` — every caller passes `UnitProfile::px()`, whose
            // `is_gui_backend` is `true`, so this stops assuming the answer.
            crate::icons::set_gui_backend(units.is_gui_backend);
            crate::icons::set_nerd_fonts(e.settings.use_nerd_fonts());
            e.startup(file_path.as_deref());
            e
        };
        setup_gtk_clipboard(&mut engine, backend.clone());

        let last_colorscheme = engine.settings.colorscheme.clone();
        (Rc::new(RefCell::new(engine)), last_colorscheme)
    }

    /// Test/headless twin of [`Self::new_portable`] — the seam for
    /// `tui_main::testing::tui_driver` to build on, so an App-on-TUI scenario
    /// runs through the same construction path `tui_main::run` uses (real
    /// `Engine::startup`, real clipboard wiring), rather than the headless
    /// [`Self::new_headless_with_backend`] shortcut.
    ///
    /// Differs from [`Self::new_portable`] in exactly two ways:
    ///
    /// - **No `core::swap::register_emergency_engine` call.** The process-
    /// global `EMERGENCY_ENGINE` static would hold a dangling `*const Engine`
    /// when this function's caller's `App` drops — a use-after-free the next
    /// test's panic hook could dereference.
    /// - **Passes `live: false`** to [`Self::assemble`] — that flag gates
    /// `tick_dispatch`'s `backend.set_caret_shape` call, which writes raw
    /// escape sequences straight to the test process's real stdout.
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
    /// every backend's `shell_config` builder resolves icons from.
    ///
    /// Before this, the pre-#1434 TUI shell carried its own icon literal,
    /// independent of this match — allowing icon drift across backends.
    /// Now every backend resolves through this single table.
    ///
    /// Returns `None` for any id this table doesn't know — extension panels
    /// resolve their own icon before reaching a `shell_config` builder.
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
    /// before calling `run_with_shell`.
    ///
    /// The engine's `AppShell` initialises every `PanelDefinition.icon` to
    /// `""` because the engine is backend-agnostic, so the panel-icon
    /// mapping happens here — a product decision, not a platform one.
    ///
    /// This is now the *only* place the panel/title-bar/sidebar-clamp logic
    /// lives — every GUI entry point (GTK, macOS, and Win-GUI) calls through
    /// it.
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
        // Plugin-provided panels (e.g. the Git Insights extension) live
        // in `engine.ext_panels`, not in the engine's `AppShell` — nothing
        // registers them there — so they have to be appended explicitly or
        // the runner's activity bar renders no icon for them at all.
        // `ext_activity_panels` already carries each panel's resolved icon,
        // so the id→glyph match above deliberately needs no arm for them.
        top_panels.extend(self.engine.borrow().ext_activity_panels());

        // On the `cell` profile (TUI, and any future `App`-hosted backend
        // whose `BackendCaps::window_chrome` is `false`), the menu bar
        // starts hidden and fully hideable, so it needs a hamburger panel
        // to reveal it. `px()` backends (GTK/macOS/Win) never take this
        // branch, so their `top_panels` list is unaffected.
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

        // Reserve a full-width title-bar band across the top of the shell.
        // `App::render_content` paints vimcode's own menu bar and inline
        // window controls into it, so a GUI backend that does not reserve
        // it loses the menu bar entirely.
        //
        // `height_lh` is a line-height multiple (no fixed-px reservation
        // API exists yet). A multiple of 2.0 measures ~34px — closer to
        // VS Code's ~35px than prior values, and leaves adequate margin.
        // This is not an absolute guarantee, as the live line height tracks
        // user-settable font sizes; a user who shrinks font_size at runtime
        // can still shrink the band under macOS's traffic-light height.
        let mut cfg = quadraui::ShellConfig::new("VimCode", top_panels)
            .with_bottom_items(bottom_items)
            .with_title_bar(self.units.title_bar_lh);
        // `with_title_bar` above always sets `has_title_bar: true` —
        // correct for GTK/macOS/Win, whose menu bar is either the CSD
        // titlebar or hidden behind a real OS one. The `cell` profile's
        // menu bar is fully hideable instead, so its initial reservation
        // follows the engine's own `menu_bar_visible`.
        if !self.units.is_gui_backend {
            cfg.has_title_bar = self.engine.borrow().menu_bar_visible;
        }
        // Opt into the client-side titlebar so a capable backend (macOS
        // today) puts the reserved band in the real titlebar, beside the
        // native traffic lights. Requested unconditionally rather than
        // gated on `target_os = "macos"` (the Platform-Neutrality Rule) —
        // GTK and Win-GUI simply don't honour this field yet, so setting
        // it there is inert today. `UnitProfile::cell()` leaves it off.
        if self.units.client_side_titlebar {
            cfg = cfg.with_client_side_titlebar();
        }
        // On GTK/macOS/Win the activity bar's row height is fixed
        // (VS Code parity), so sizing its width from the editor font
        // makes it oblong — `UnitProfile::px()` pins a fixed-pixel width.
        // `UnitProfile::cell()` leaves this `None`, so the default
        // (3-line-height multiple) stays in charge.
        cfg.activity_bar_width_px = self.units.activity_bar_width_px;
        // Seed the *initial* editor font from `settings.font_family`/
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
        // Resolved through `app_support::resolve_editor_font` rather
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
        // The width the sidebar *opens* at is a line-height
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
        // The shared Alt rung clamps sidebar width, so Alt+Left/Right
        // resolve identically on every backend. #1798 requires
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
        // Reserve one line-height each for the always-present
        // command line and the (near-always-present) status line at the
        // *shell* level.  With `AppShell::with_status_bar_under_main_column`
        // (#1917) the activity bar and sidebar now run the full window
        // height and these bands are carved from the main column only,
        // matching VS Code's layout.  The reservations themselves are
        // still needed: they tell `AppShell::compute_layout` how much
        // vertical space to subtract from `main_content_bounds` for the
        // status bar and command line — the new mode only changes
        // *where* that subtraction is applied (main-column width, not
        // full window width).
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
        cfg = cfg
            .with_status_bar_under_main_column()
            .with_command_line()
            .with_status_bar();
        cfg
    }

    /// Build the `App` struct itself from already-prepared, display-
    /// independent inputs.
    ///
    /// Split out of [`App::new`] so headless test harnesses can reach the
    /// same field initialisation without re-running display-dependent setup
    /// that would require a live display.
    ///
    /// Everything below this line is plain `Rc`/`Cell`/`RefCell` allocation;
    /// none of it touches any display. `backend` is taken as a parameter
    /// rather than constructed here.
    pub(crate) fn assemble(
        engine: Rc<RefCell<Engine>>,
        deferred: DeferredQueue,
        last_colorscheme: String,
        backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
        live: bool,
    ) -> Self {
        // Computed before `engine` moves into the struct literal
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
            // Default window geometry, matching `SessionState::default()`.
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
            // Seeded to `None` rather than the active panel — GTK
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
    /// **no** display-dependent setup — for headless tests.
    ///
    /// Skips clipboard wiring, settings-file monitoring, and registering the
    /// emergency engine (whose process-global contract would create a dangling
    /// pointer when the test's `App` drops).
    ///
    /// Takes the engine behind an `Rc` so the caller keeps a handle for
    /// assertions.
    #[cfg(all(feature = "gui", any(test, feature = "test-support")))]
    pub(crate) fn new_headless(engine: Rc<RefCell<Engine>>) -> Self {
        Self::new_headless_with_backend(
            engine,
            Rc::new(RefCell::new(Box::new(backend::GtkBackend::new()))),
            render::UnitProfile::px(),
        )
    }

    /// Backend-parameterised core of [`App::new_headless`] — the same seam
    /// [`App::new_portable`] is to [`App::new`], for the test constructors.
    ///
    /// `new_headless` hardcoded `GtkBackend`, which made it unusable from
    /// tests that need to paint through a different backend's rasterisers.
    /// Taking the backend as a parameter keeps **one** headless constructor
    /// per `Box<dyn quadraui::Backend>` trait object.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new_headless_with_backend(
        engine: Rc<RefCell<Engine>>,
        backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
        units: render::UnitProfile,
    ) -> Self {
        // This constructor is the shared headless `App` test seam for
        // every backend — GTK, macOS driver-tier test, and TUI harness.
        // Resolve `use_nerd_fonts()` from `units.is_gui_backend` to match
        // the way `App::new`/`App::new_portable` decide it.
        crate::icons::set_gui_backend(units.is_gui_backend);
        let (use_nerd_fonts, last_colorscheme) = {
            let e = engine.borrow();
            (e.settings.use_nerd_fonts(), e.settings.colorscheme.clone())
        };
        // Path-qualified rather than via the `use` at the top of this file —
        // that import is `gui`-gated and this constructor is not.
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
