//! `struct App` — the backend-neutral editor shell application (#785, stage 1 of #47).
//!
//! Until this file existed, `App`, its inherent `impl` blocks and its
//! `impl quadraui::ShellApp for App` all lived inside `src/gtk/mod.rs`,
//! which made ~6,900 lines of *portable* shell logic look like GTK code and
//! left no place for a second native backend (the macOS wrapper #47 is
//! about) to reuse any of it. Hoisting the type to `crate::app` is the
//! mechanical half of that split: `src/gtk/mod.rs` keeps `run()`,
//! `build_shell_config()` and the genuinely GTK-only helpers, and this file
//! keeps everything a second backend would otherwise have had to
//! re-implement.
//!
//! # Why this module no longer needs `#[cfg(feature = "gui")]` (#862)
//!
//! #813 retired the biggest blocker the #47 re-audit found: `backend` used
//! to be typed as the concrete GTK backend struct, which is what forced
//! every one of the ~19 modal-stack/drag-state handle call sites — and by
//! extension this whole file — to depend on it. It is now typed against
//! [`TextMetricsBackend`], a narrow local trait for the text-measurement
//! hooks that still have no portable `quadraui::Backend` equivalent; see
//! that trait's doc comment. #861 closed the trait's remaining GTK leak:
//! its context-setter used to take a `pango::Context` by name, which meant
//! the trait — and so `App::backend`'s field type — could never be
//! implemented by a non-GTK backend no matter what that backend could
//! measure. It now takes the context type-erased, so the trait itself names
//! no toolkit type.
//!
//! #862 closed the three items the previous revision of this doc comment
//! listed as the remaining blockers to dropping the `gui` gate:
//!
//! 1. **The platform-typed fields** (`window`, `css_provider`) are now
//!    type-erased behind the small local traits
//!    [`PlatformWindowHandle`]/[`PlatformCssProvider`] (the same shape as
//!    [`TextMetricsBackend`] and `Engine::clipboard_read`/`clipboard_write`,
//!    #417). A third field used to live in this list, `settings_monitor`,
//!    holding a GTK-only `gio::FileMonitor` behind a `Box<dyn Any>`
//!    drop-guard; #949 deleted it outright rather than type-erasing it —
//!    `Engine::check_settings_reload`'s portable mtime poll (already the
//!    sole reload mechanism on TUI, and already called from GTK's own
//!    `handle_poll_tick` every tick) made it redundant, and deleting it
//!    closed the settings-hot-reload gap on macOS/Win-GUI that this file's
//!    `new_portable` doc table used to list as deliberately skipped.
//! 2. **The platform hook call sites** (colorscheme reload, OS window title /
//!    default size / minimize / maximized-check, CSD capture) now go through
//!    those same traits and compile for every feature set; only window
//!    *discovery* (`find_visible_window` — quadraui has no portable "find the
//!    runner's window" surface yet) and the handful of literal
//!    `gtk4::Settings`/`gtk4::IconTheme` calls inside `App::new`/
//!    `handle_poll_tick` stay behind inline `#[cfg(feature = "gui")]`.
//! 3. **`crate::gtk::{click, css, util}`.** The portable majority of these —
//!    `pixel_to_click_target` and the rest of the click-resolution/tab-bar
//!    pixel-geometry functions, `make_theme_css`/`STATIC_CSS`, `open_url`/
//!    `install_bundled_icon_font` — moved to the backend-neutral
//!    `crate::click`/`crate::css`/`crate::app_support`, which `src/gtk/{click,
//!    css,mod,util}.rs` now re-export so nothing else in `crate::gtk` had to
//!    change. The genuinely GTK-only remainder —
//!    `click::build_editor_click_context` (the Pango/Cairo text-measurement
//!    context, see [`TextMetricsBackend`]), `css::load_css` and `util`'s
//!    pixbuf/log helpers — stayed in `crate::gtk` and is reached from here
//!    through explicit `#[cfg(feature = "gui")]` call sites
//!    (`app_icon_image_for_paint`, `App::new`, the `TextMetricsBackend` impl).
//!
//! None of this was a "route around it" job: per `CLAUDE.md`'s
//! Platform-Neutrality Rule, the parts that stayed behind the `gui` feature
//! are exactly the parts that still need quadraui-side infrastructure (a
//! backend-neutral window-chrome/file-watcher/file-picker surface) rather
//! than new per-backend code — see `docs/IRREDUCIBLE_SURFACE.md`.
//!
//! #937's quadraui pin bump deprecated `Backend::draw_status_bar`
//! (quadraui#819, replacement is `draw_status_bar_interactive`) that this
//! file's status-bar paint calls still use; migrating them to the
//! hover/pressed `InteractionState` API is an unrelated refactor deferred to
//! a follow-up, so it's silenced here rather than left as a stray warning
//! under `-D warnings`.
#![allow(deprecated)]

#[cfg(feature = "gui")]
use gtk4::gdk;
#[cfg(feature = "gui")]
use gtk4::pango;
#[cfg(feature = "gui")]
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::core;
#[cfg(feature = "gui")]
use crate::icons;
use crate::render;

use core::engine::EngineAction;
use core::{Engine, WindowRect};
use render::Theme;

use copypasta_ext::ClipboardProviderExt;

use crate::app_support::*;
use crate::click::*;
use crate::core::engine::sidebar::*;
use crate::css::*;
#[cfg(feature = "gui")]
use crate::gtk::backend;
#[cfg(feature = "win")]
use crate::win::backend as win_backend;

// ─── Panel-key accelerator registry ─────────────────────────────────────────
//
// The 14-entry `PanelAccelerator` id table (`render::ACC_*`) and the
// dispatcher itself (`render::dispatch_panel_accelerator`) are shared with
// TUI (#761 / #734 slice 6) — see the rung's header comment in `render.rs`.
// What's left here is registration (this backend's own `quadraui::Backend`
// instance) and [`GtkAccelHost`], the five-hook impl for the actions that
// need GTK's `DeferredQueue` seam.

// `register_panel_accelerators` (the 14-entry id table + registration loop)
// moved to `render::register_panel_accelerators` in #823 item 1 — it was
// byte-identical to `tui_main`'s copy and had no backend-specific step.
// Called from `ShellApp::setup` (#587) — mirrors `tui_main`'s call at
// startup.

/// [`render::PanelAcceleratorHost`] impl for GTK: each hook just queues the
/// matching [`DeferredAction`] — GTK's `UiEvent::Accelerator` arm has no
/// engine-mutation seam of its own for these five actions (see the rung's
/// header comment in `render.rs`), so the real work happens in `tick()`
/// (which does have `&mut App`) on the next frame, same as every other
/// App-only GTK callback.
struct GtkAccelHost<'a> {
    deferred: &'a DeferredQueue,
}

impl render::PanelAcceleratorHost for GtkAccelHost<'_> {
    fn toggle_sidebar(&mut self, _engine: &mut Engine) {
        self.deferred.send(DeferredAction::ToggleSidebar);
    }
    fn focus_explorer(&mut self, _engine: &mut Engine) {
        self.deferred.send(DeferredAction::ToggleFocusExplorer);
    }
    fn focus_search(&mut self, _engine: &mut Engine) {
        self.deferred.send(DeferredAction::ToggleFocusSearch);
    }
    fn open_terminal(&mut self, _engine: &mut Engine) {
        self.deferred.send(DeferredAction::ToggleTerminal);
    }
    fn terminal_toggle_max(&mut self, _engine: &mut Engine) {
        self.deferred.send(DeferredAction::ToggleTerminalMaximize);
    }
}

/// [`render::ShellShadowSyncHost`] impl for GTK (#1062): GTK has no panel id
/// that's absent from the shadow `engine.app_shell` other than the `ext:`
/// ids `render::sync_shell_event_shadow` already excludes itself, so this is
/// a unit struct answering `false` unconditionally.
struct GtkShellShadowHost;

impl render::ShellShadowSyncHost for GtkShellShadowHost {
    fn panel_absent_from_shadow(&self, _panel_id: &quadraui::WidgetId) -> bool {
        false
    }
}

/// [`render::EngineActionHost`] impl for GTK (#1063) — see the rung's header
/// comment in `render.rs`. Unlike [`GtkAccelHost`] above, these hooks run
/// with full `&mut App` in hand (`App::dispatch_engine_action`, the sole
/// caller, has no engine borrow outstanding when it builds this), so there's
/// no need to defer to `tick()` via `DeferredQueue` — except most method
/// bodies below read/mutate the `engine: &mut Engine` parameter
/// `apply_engine_action` hands them directly, rather than going through
/// `self.engine.borrow()/borrow_mut()` the way the pre-#1063 `App` methods
/// they replace did. That's not stylistic: `apply_engine_action`'s own
/// `engine` parameter is already a live `RefMut` borrow of that same
/// `Rc<RefCell<Engine>>` — reaching for a second, independent
/// `self.app.engine.borrow_mut()` from in here would double-borrow the same
/// `RefCell` and panic at runtime. `open_terminal`/`open_workspace_dialog`
/// below used to be `App::new_terminal_tab`/`App::open_workspace_dialog`
/// verbatim, until this rewrite left both with no other caller (menu, key
/// and macro dispatch all go through here now) and #1063 deleted them
/// rather than ship dead code.
struct GtkEngineActionHost<'a> {
    app: &'a mut App,
}

impl GtkEngineActionHost<'_> {
    /// Shared by [`Self::quit`] and [`Self::quit_with_unsaved`]'s
    /// no-unsaved-changes branch — inlines `App::save_session_and_exit`
    /// against the already-borrowed `engine` instead of calling it (see this
    /// struct's own doc for why).
    fn save_session_and_exit(app: &App, engine: &mut Engine) {
        engine.session.window.width = app
            .window
            .as_ref()
            .map(|w| w.win_default_width())
            .unwrap_or(800);
        engine.session.window.height = app
            .window
            .as_ref()
            .map(|w| w.win_default_height())
            .unwrap_or(600);
        engine.save_session_state();
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        app.exit_requested.set(true);
    }
}

impl render::EngineActionHost for GtkEngineActionHost<'_> {
    /// Was `App::new_terminal_tab`; see this struct's own doc.
    fn open_terminal(&mut self, engine: &mut Engine) {
        let cols = self.app.terminal_cols();
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_new_tab(cols, rows);
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::toggle_terminal_maximize`.
    fn toggle_terminal_maximize(&mut self, engine: &mut Engine) {
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: self.app.terminal_cols(),
            terminal_max_rows: self.app.terminal_target_maximize_rows(),
        };
        engine.handle_ui_event(
            crate::core::engine::UiEvent::Accelerator(
                crate::core::engine::AcceleratorId::new("terminal.toggle_maximize"),
                quadraui::Modifiers::default(),
            ),
            ctx,
        );
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::run_command_in_terminal`.
    fn run_in_terminal(&mut self, engine: &mut Engine, cmd: String) {
        let cols = self.app.terminal_cols();
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_run_command(&cmd, cols, rows);
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::open_folder_dialog`.
    fn open_folder_dialog(&mut self, engine: &mut Engine) {
        let controller = quadraui::FolderPickerController::new(
            engine.cwd.clone(),
            vec![".vimcode-workspace".to_string()],
            engine.settings.show_hidden_files,
        );
        *self.app.folder_picker.borrow_mut() = Some(controller);
        self.app.draw_needed.set(true);
    }
    /// Was `App::open_workspace_dialog` (see this struct's own doc), inlining
    /// the `refresh_file_tree` / `refresh_explorer` / `reveal_path_in_explorer`
    /// chain it called — `queue_explorer_draw` (that chain's last step) is a
    /// documented no-op under the `ShellApp` runner, so dropping it changes
    /// nothing.
    fn open_workspace_dialog(&mut self, engine: &mut Engine) {
        engine.explorer_rebuild_rows();
        if let Some(path) = engine.file_path().cloned() {
            engine.explorer_reveal_path(&path);
        }
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::save_workspace_as_dialog` — touches no engine state, so
    /// this one calls straight through.
    fn save_workspace_as_dialog(&mut self, _engine: &mut Engine) {
        self.app.save_workspace_as_dialog();
    }
    /// Inlines `App::open_recent_dialog`.
    fn open_recent_dialog(&mut self, engine: &mut Engine) {
        if engine.session.recent_workspaces.is_empty() {
            engine.message = "No recent workspaces".to_string();
        } else {
            engine.open_picker(crate::core::engine::PickerSource::RecentWorkspaces);
        }
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::sync_sidebar_from_engine` — a redraw trigger only under
    /// the `ShellApp` runner (see that method's own doc comment).
    fn sidebar_toggled(&mut self, _engine: &mut Engine) {
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::show_quit_confirm`.
    fn quit_with_unsaved(&mut self, engine: &mut Engine) {
        if !engine.has_any_unsaved() {
            Self::save_session_and_exit(self.app, engine);
            return;
        }
        engine.show_quit_confirm();
        self.app.draw_needed.set(true);
    }
    /// Inlines `App::quit_confirmed` (itself just `save_session_and_exit`).
    fn quit(&mut self, engine: &mut Engine) {
        Self::save_session_and_exit(self.app, engine);
    }
    /// Matches the former inline `EngineAction::QuitWithError` arm in
    /// `dispatch_engine_action` exactly — no `save_session_state` (unlike
    /// `quit` above). This asymmetry is GTK-specific, not something
    /// `tui_main::handle_action` itself does: TUI's `Quit`/`SaveQuit` and
    /// `QuitWithError` arms both call `save_session` (`tui_main/mod.rs`),
    /// i.e. TUI treats the two variants *symmetrically*. The divergence is
    /// cross-backend (GTK skips the save on `QuitWithError`, TUI doesn't),
    /// preserved here exactly as it behaved pre-#1063 rather than changed
    /// as a side effect of this convergence.
    fn quit_with_error(&mut self, engine: &mut Engine) -> ! {
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        std::process::exit(1);
    }
}

/// Work that a GTK callback with no `&mut App` in hand must hand back to the
/// next frame.
///
/// #732 tranche 3: the six deferrals below are all that is left of the
/// Relm4-era `Msg` bus. They are genuine deferrals, not translations — each
/// originates somewhere that cannot call an `&mut self` method at all:
/// [`GtkAccelHost`], which holds only a clone of the queue. (The 200 ms
/// yank-highlight one-shot used to be a seventh, scheduled via a one-shot
/// toolkit timer; #813 ported it to the portable `yank_hl_deadline`
/// poll-in-`tick` pattern TUI already used — see [`App::yank_hl_deadline`] —
/// so it no longer needs this queue at all. #949 dropped an eighth,
/// `SettingsFileChanged`, the same way: the GTK-only `gio::FileMonitor` that
/// used to construct it is gone, replaced by `Engine::check_settings_reload`'s
/// portable mtime poll, which `handle_poll_tick` now calls directly every
/// tick instead of waiting on a deferred signal.)
///
/// quadraui has no deferral seam of its own to move onto — `ShellApp::tick`
/// *is* the seam (its doc names "draining channels" as the intended use), and
/// the queued payload is necessarily app-specific, so the queue stays here
/// rather than becoming a quadraui gap to file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferredAction {
    /// Redraw after an accelerator mutated engine state directly.
    Resize,
    /// Toggle focus between the explorer and the editor.
    ToggleFocusExplorer,
    /// Toggle focus between the search panel and the editor.
    ToggleFocusSearch,
    /// Toggle sidebar visibility.
    ToggleSidebar,
    /// Toggle the integrated terminal panel open/closed.
    ToggleTerminal,
    /// Toggle the "terminal maximized" state.
    ToggleTerminalMaximize,
}

/// Shared queue of [`DeferredAction`]s, drained by `ShellApp::tick`.
#[derive(Clone)]
pub(crate) struct DeferredQueue(Rc<RefCell<VecDeque<DeferredAction>>>);

impl DeferredQueue {
    // Only `App::new`/`App::new_headless` (both `gui`-gated) call this today.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    fn new() -> Self {
        DeferredQueue(Rc::new(RefCell::new(VecDeque::new())))
    }

    /// Enqueue an action for processing in the next `tick()` call.
    fn send(&self, action: DeferredAction) {
        self.0.borrow_mut().push_back(action);
    }

    /// Take all pending actions, leaving the queue empty.
    fn drain(&self) -> Vec<DeferredAction> {
        let mut q = self.0.borrow_mut();
        q.drain(..).collect()
    }
}

/// Narrow extension trait for the text-measurement hooks that
/// `quadraui::Backend` has no portable equivalent for yet (#813): a backend
/// needs some way to be told the current line height / char width, and (for
/// backends whose click-time hit-testing wants per-glyph accuracy, like
/// GTK's Pango) some way to be handed a fresh measurement context each
/// frame.
///
/// `App::backend` used to be typed as the concrete `backend::GtkBackend` —
/// the sole reason `struct App` couldn't compile without the GTK toolkit
/// in scope — purely so these calls would resolve. Retyping the field to a
/// bare `Box<dyn quadraui::Backend>` would drop them; this supertrait lets
/// `App::backend` hold one trait object that still exposes both the 19
/// generic `modal_stack_handle`/`drag_state_handle` call sites (via the
/// `Backend` supertrait bound) *and* these narrow ones, without naming a
/// concrete backend type anywhere outside its `impl` below.
///
/// #861: `set_text_measurement_context` used to be `set_pango_context(ctx:
/// pango::Context)`, which meant *no non-GTK backend could implement this
/// trait at all* — the signature named a GTK/Pango type, so `App` could
/// never hold a macOS or Win backend regardless of what that backend could
/// actually measure. The context is now passed type-erased
/// (`Box<dyn Any>`): the only caller that produces one
/// (`click::build_editor_click_context`, GTK-only) and the only
/// implementation that consumes one (`GtkBackend` below, via `downcast`)
/// agree on the concrete type out of band, so the trait itself never names
/// it. A backend with no persistent-context concept — TUI's fixed-width
/// grid needs none; quadraui's macOS text measurement
/// (`quadraui::macos::text::measure_text(&CTFont, &str)`) takes the font
/// per call instead of storing one — can implement this as a no-op.
///
/// #969: `set_text_measurement_context` has a default (empty) body —
/// genuinely optional, per its own doc above, for any backend with no
/// persistent-context concept. `set_current_line_height`/
/// `set_current_char_width` deliberately have **no default**: both are
/// load-bearing for click correctness (`App::explorer_ui_event` /
/// `App::route_ai_chat_event` re-apply them, immediately before hit-testing,
/// to undo the #540/#819 drift guard's namesake drift), so every impl must
/// write *something* for them rather than silently inheriting a no-op. That
/// alone does not stop an impl from writing an empty body anyway — #967 did
/// exactly that on `MacBackend` — which is what
/// [`crate::harness::assert_text_metrics_backend_applies_metrics`] is for:
/// it round-trips a value through the trait object and the
/// `quadraui::Backend` getter these setters are supposed to feed, so a stub
/// fails a test instead of shipping silently.
pub(crate) trait TextMetricsBackend: quadraui::Backend {
    // Only called from the `gui`-gated editor-click-context block in
    // `render_content` today (its one producer,
    // `click::build_editor_click_context`, is GTK-only). Default body: a
    // backend with no persistent-context concept (TUI, macOS, Win-GUI —
    // see the trait doc above) can simply not override this.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    fn set_text_measurement_context(&mut self, _ctx: Box<dyn std::any::Any>) {}
    fn set_current_line_height(&mut self, line_height: f64);
    fn set_current_char_width(&mut self, char_width: f64);
}

#[cfg(feature = "gui")]
impl TextMetricsBackend for backend::GtkBackend {
    fn set_text_measurement_context(&mut self, ctx: Box<dyn std::any::Any>) {
        // The only producer (`click::build_editor_click_context`) hands us
        // a `pango::Context`; anything else is a caller bug, not something
        // this backend can act on, so it's silently dropped rather than
        // panicking.
        if let Ok(pango_ctx) = ctx.downcast::<pango::Context>() {
            backend::GtkBackend::set_pango_context(self, *pango_ctx);
        }
    }

    fn set_current_line_height(&mut self, line_height: f64) {
        backend::GtkBackend::set_current_line_height(self, line_height);
    }

    fn set_current_char_width(&mut self, char_width: f64) {
        backend::GtkBackend::set_current_char_width(self, char_width);
    }
}

/// [`TextMetricsBackend`] for quadraui's `WinBackend` (#866, the Win-GUI
/// twin of #859's `MacBackend` impl in `src/macos/mod.rs`).
///
/// - `set_text_measurement_context` is a no-op, same reasoning as the
///   `MacBackend` impl: DirectWrite measurement
///   (`WinBackend::measure_text`/`draw_text`) takes the string per call
///   rather than storing a context, and the trait's one context producer
///   (`click::build_editor_click_context`) is GTK-only and its call site in
///   `render_content` is `#[cfg(feature = "gui")]`, so nothing ever calls
///   this here.
/// - The two metric setters forward to `WinBackend`'s own public
///   `set_current_line_height`/`set_current_char_width` (`f32`, matching
///   DirectWrite's unit — GTK's are `f64` Pango units), the Win-GUI
///   counterparts of `GtkBackend`'s methods of the same name at the pinned
///   rev `9eede7fd`.
#[cfg(feature = "win")]
impl TextMetricsBackend for win_backend::WinBackend {
    // `set_text_measurement_context` is deliberately not overridden here —
    // the trait's default (empty) body is exactly this backend's no-op, per
    // the reasoning above (#969).

    fn set_current_line_height(&mut self, line_height: f64) {
        win_backend::WinBackend::set_current_line_height(self, line_height as f32);
    }

    fn set_current_char_width(&mut self, char_width: f64) {
        win_backend::WinBackend::set_current_char_width(self, char_width as f32);
    }
}

/// Narrow seam over the OS top-level window handle (#862), the same shape as
/// [`TextMetricsBackend`] above and `Engine::clipboard_read`/`clipboard_write`
/// (#417): `App::window` stores one of these type-erased, so the shared
/// paint/title-sync/minimize methods can call it without naming a toolkit
/// type. Method names are prefixed `win_*` to avoid colliding with the
/// `gtk4::prelude` extension-trait methods of the same name on the one
/// concrete impl below (both would otherwise be applicable to `&gtk4::Window`
/// inside that impl, which is an ambiguous call, not a recursive one).
pub(crate) trait PlatformWindowHandle {
    fn win_default_width(&self) -> i32;
    fn win_default_height(&self) -> i32;
    fn win_set_title(&self, title: &str);
    fn win_is_maximized(&self) -> bool;
    fn win_minimize(&self);
    // Only called from `capture_window_and_apply_csd`'s `gui`-gated inner
    // block today — window *discovery* has no portable equivalent yet (see
    // that method's doc comment), so nothing calls this outside `gui`.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    fn win_set_decorated(&self, decorated: bool);
}

#[cfg(feature = "gui")]
impl PlatformWindowHandle for gtk4::Window {
    fn win_default_width(&self) -> i32 {
        gtk4::prelude::GtkWindowExt::default_width(self)
    }
    fn win_default_height(&self) -> i32 {
        gtk4::prelude::GtkWindowExt::default_height(self)
    }
    fn win_set_title(&self, title: &str) {
        gtk4::prelude::GtkWindowExt::set_title(self, Some(title));
    }
    fn win_is_maximized(&self) -> bool {
        gtk4::prelude::GtkWindowExt::is_maximized(self)
    }
    fn win_minimize(&self) {
        gtk4::prelude::GtkWindowExt::minimize(self);
    }
    fn win_set_decorated(&self, decorated: bool) {
        gtk4::prelude::GtkWindowExt::set_decorated(self, decorated);
    }
}

// #866: deliberately no `impl PlatformWindowHandle for` any Win-GUI type.
// quadraui's `win` module (pinned rev `9eede7fd`) exposes no public
// top-level-window handle at all — `WinBackend`'s `hwnd` field is private
// and `#[cfg(target_os = "windows")]`-gated, and nothing in `win::run` or
// `win::services` hands one back to a `ShellApp` caller. This is the same
// gap `src/macos/mod.rs` documents for `MacBackend` (no `PlatformWindowHandle`
// impl there either) — window *discovery* has no portable equivalent yet on
// either backend, matching this trait's own doc comment above
// (`win_set_decorated`'s `allow(dead_code)` already prices that in). `App`'s
// `window` field simply stays `None` on the `new_portable` path both
// backends use, exactly as it does for macOS today. Writing raw `windows`-
// crate calls here to invent a handle would be new per-backend feature
// logic — CLAUDE.md's Platform-Neutrality Rule says that gap belongs in a
// quadraui issue (a public window-handle accessor next to `WinBackend`),
// not in this file.

/// Narrow seam over the platform stylesheet provider (#862) — same shape as
/// [`PlatformWindowHandle`] above. `App::css_provider` stores one of these
/// type-erased so the colorscheme-reload/`setup` methods can reload it
/// without naming `gtk4::CssProvider`.
pub(crate) trait PlatformCssProvider {
    fn load_css_data(&self, css: &str);
}

#[cfg(feature = "gui")]
impl PlatformCssProvider for gtk4::CssProvider {
    fn load_css_data(&self, css: &str) {
        self.load_from_data(css);
    }
}

pub(crate) struct App {
    pub(crate) engine: Rc<RefCell<Engine>>,
    /// Set to true in update() whenever a draw is needed; cleared by the #[watch] block.
    /// This prevents the 20/sec SearchPollTick timer from unconditionally calling queue_draw().
    pub(crate) draw_needed: Rc<Cell<bool>>,
    /// Set by [`App::save_session_and_exit`] when the user quits normally
    /// (`Quit`/`SaveQuit`/`quit_menu`/the unsaved-changes confirm dialog).
    /// Checked at the end of `ShellApp::handle`/`tick` (#813) so a
    /// `dispatch_engine_action`/`apply_dialog_action` call nested arbitrarily
    /// deep — including inside an early-return arm such as the menu-activated
    /// path — still surfaces as [`quadraui::Reaction::Exit`] to the runner.
    /// Replaces the previous idle-callback process-exit hack: quadraui's
    /// own `Reaction::Exit` (`ReactionSink::request_exit` in the GTK
    /// runner) already does the equivalent teardown for every other
    /// backend. `EngineAction::QuitWithError` (`:cquit`) is deliberately
    /// **not** routed through this flag — it needs a nonzero process exit
    /// code, which `Reaction::Exit` cannot carry, so it keeps a direct
    /// `std::process::exit(1)` (mirrors `tui_main::handle_action`).
    pub(crate) exit_requested: Cell<bool>,
    /// Deadline for clearing the yank highlight, armed by
    /// `run_post_key_epilogue` when `epilogue.arm_yank_highlight` is set and
    /// polled by `tick_dispatch` (#813). Replaces a one-shot toolkit timer
    /// with the same portable poll-in-`tick` pattern TUI already used
    /// (`yank_hl_deadline` in `tui_main/shell_app.rs`) — no toolkit timer
    /// needed.
    pub(crate) yank_hl_deadline: Cell<Option<std::time::Instant>>,
    /// A file dialog requested this frame, drained by the next `tick()`
    /// call (which has the `backend` handle `PlatformServices` needs).
    /// See [`PendingFileDialog`] (#572).
    pub(crate) pending_file_dialog: Cell<Option<PendingFileDialog>>,
    pub(crate) cached_line_height: f64,
    pub(crate) cached_char_width: f64,
    /// Position of the wheel event currently being handled, in **absolute**
    /// surface pixels (the same frame `render_content` paints in). Read by
    /// `handle_mouse_scroll_msg` to route the wheel to the registered scroll surface
    /// or editor pane under the cursor (#240) — matches TUI behaviour.
    ///
    /// Written by `ShellApp::handle`'s `UiEvent::Scroll` arm from the event's
    /// own `position`. It used to be written by the Relm4 build's
    /// `EventControllerMotion`; the #540 ShellApp migration removed that
    /// controller and left no writer, so this stayed `None` forever and every
    /// wheel event fell through to the focused window while the
    /// `dispatch_scroll` surface routing (terminal scrollback, editor-hover
    /// popup, debug output) never ran at all. Sourcing it from the wheel event
    /// itself — rather than from a preceding motion event — is what makes it
    /// impossible to regress the same way again (#646).
    pub(crate) last_editor_pointer: Rc<Cell<Option<(f64, f64)>>>,
    /// Cached line height for the UI font (sidebars, panels).
    /// Computed alongside `cached_line_height` in `CacheFontMetrics`.
    pub(crate) cached_ui_line_height: f64,
    /// Cached dialog layout from the last `render_content` paint (#546) —
    /// mirrors `context_menu_layout` below. Button-click and outside-click
    /// hit-testing both read `DialogLayout::hit_test` off this instead of a
    /// hand-rolled per-backend rect cache.
    pub(crate) dialog_layout: Rc<RefCell<Option<quadraui::DialogLayout>>>,
    /// #815: the shared `quadraui::FolderPickerController`, adopted from the
    /// old TUI-local `FolderPickerState`/native `gtk4::FileDialog` split.
    /// `render_content` (`&self`) needs to *read* it while `handle_key_press`
    /// (`&mut self`) mutates it, hence the `RefCell` — mirrors
    /// `dialog_layout` above. `TuiShellApp` carries the identical field type.
    pub(crate) folder_picker: RefCell<Option<quadraui::FolderPickerController>>,
    /// Edge-trigger flag for #727's native message-dialog path: `true`
    /// once a native present has been queued (or already shown) for the
    /// `engine.dialog` currently open. A native `AlertDialog` cannot be
    /// re-presented every frame the way the in-canvas `Dialog` primitive
    /// is repainted, so `render_content` only queues one when this is
    /// `false`, then sets it `true`. Reset to `false` by `render_content`
    /// when `engine.dialog` goes back to `None` (dialog closed), arming
    /// the trigger for the next open.
    ///
    /// `Rc`-wrapped (like `dialog_layout` above) so `testing::harness` can
    /// keep a handle after `App` is moved into the driver — the #727 test
    /// asserts the present-exactly-once behaviour by repainting several
    /// frames and checking this never re-queues.
    pub(crate) native_dialog_shown: Rc<Cell<bool>>,
    /// A native message dialog queued by `render_content`'s edge-trigger
    /// check, drained by `tick()`. Mirrors `PendingFileDialog` (#572):
    /// `PlatformServices::show_message_dialog` blocks via quadraui's
    /// nested-mainloop pump, which must not run from inside the paint
    /// callback `render_content` runs under, so the request is stashed
    /// here and the actual call happens in `tick()` instead.
    ///
    /// `Rc`-wrapped for the same testing reason as `native_dialog_shown`.
    pub(crate) pending_native_dialog: Rc<Cell<Option<quadraui::MessageDialogOptions>>>,
    /// Shared with the drawing-area resize callback so scrollbars can be
    /// repositioned synchronously (before each frame) without going through
    /// Relm4's async message queue.
    pub(crate) line_height_cell: Rc<Cell<f64>>,
    pub(crate) char_width_cell: Rc<Cell<f64>>,
    /// Current mouse position, updated directly from the motion callback (no Relm4 message).
    pub(crate) mouse_pos_cell: Rc<Cell<(f64, f64)>>,
    /// True while user is drag-selecting text inside a find/replace input field.
    pub(crate) fr_input_dragging: bool,
    pub(crate) deferred: DeferredQueue,
    /// Last content written to system clipboard.
    /// Used to avoid redundant writes on every keystroke.
    pub(crate) last_clipboard_content: Option<String>,
    /// Which tab close button (×) the mouse is over: (group_id.0, tab_idx).
    pub(crate) tab_close_hover: Option<(usize, usize)>,
    /// Absolute tight close-glyph rects captured in `render_content`. Consumed
    /// by `tab_close_hit_test` (hover) so it hit-tests against the exact drawn
    /// geometry — including the activity-bar/sidebar x-offset — instead of
    /// re-deriving group rects from a `(0,0)` content origin (which ignored the
    /// offset and made hover never fire in ShellApp mode). (#515)
    pub(crate) cached_tab_close_abs: Rc<RefCell<TabCloseAbsMap>>,
    /// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`),
    /// captured in `render_content`. Feeds the tab drop-zone computation so a
    /// short drag inside a group's own tab bar resolves to a `TabReorder` (with
    /// an insertion bar) rather than a new-split overlay. (#515)
    pub(crate) cached_tab_slots_abs: Rc<RefCell<TabSlotsAbsMap>>,
    /// Pixel-accurate per-group tab-bar hit geometry from the ShellApp
    /// `render_content` pass (via `Backend::tab_bar_layout`). Consumed by the
    /// GTK tab-bar click hit-test instead of the char-cell `hit_regions`, which
    /// don't match GTK's proportional-font tab layout. (#515)
    pub(crate) cached_tab_pixel_hits: Rc<RefCell<TabPixelHitMap>>,
    /// Cached per-window status bar segment hit zones from draw_window_status_bar.
    pub(crate) status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Painted rect of the separated status line's status bar (#671/#672),
    /// or `None` if the last frame drew no separated line
    /// (`window_status_line` off, `status_line_above_terminal` on, or no
    /// bottom panel open — see `compute_editor_layout`'s `has_separated`).
    /// Exists purely so the `status_segment_map` entry keyed by
    /// `active_window_id` (inserted right alongside this) can be located by
    /// pixel — the live click path itself needs no such cache, since
    /// `status_segment_map`'s `local_x` is already bar-relative and
    /// `window_zone_hit_test`/`screen_zone_hit_test` resolve the y-band
    /// independently. Mirrors the existing `picker_popup_rect` /
    /// `tab_switcher_popup_rect` "painted rect for click+test" pattern.
    pub(crate) separated_status_bar_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Segment hit zones for the **global** (bottom-of-screen) status bar,
    /// local to `Engine::global_status_rect`'s own origin (#752).
    ///
    /// Kept in its own field rather than in `status_segment_map` because that
    /// map is keyed by `WindowId` and the global bar belongs to no window — it
    /// shows the active buffer's summary in the shell's bottom band. Populated
    /// by `render_content` from `Backend::status_bar_layout`, the same
    /// measurement pass that positions the bar's glyphs, so the git-branch
    /// segment's clickable span is by construction the span it painted at.
    pub(crate) global_status_zones: Rc<RefCell<render::StatusZones>>,
    /// Cached ScreenLayout from the last draw_editor paint pass. Click handlers
    /// read this instead of recomputing geometry from engine state (#344).
    pub(crate) cached_screen_layout: Rc<RefCell<Option<render::ScreenLayout>>>,
    /// Accumulated `quadraui::FrameHitMap` covering the `Editor`/`TabBar`
    /// zones painted in `render_content` (#449). Built via
    /// `quadraui::ScreenLayout::hit_map()` (quadraui#425): pushes the SAME
    /// `Editor`/`TabBar` objects and rects already painted at their existing
    /// call sites, purely for hit-testing, so it can never reorder or
    /// duplicate real painting. `click::pixel_to_click_target` consults this
    /// first to resolve the top-level Editor/TabBar zone, falling back to
    /// `render::screen_zone_hit_test`'s manual rect-walk for
    /// breadcrumb/divider zones (which have no `FrameZone` equivalent) and
    /// for the brief window before the first paint populates this cache.
    pub(crate) cached_frame_hit_map: Rc<RefCell<Option<quadraui::FrameHitMap>>>,
    /// Parallel table for resolving `FrameZone::TabBar { idx }`, keyed by the
    /// *global* surface index `FrameZone::TabBar { idx }` actually carries —
    /// `ScreenLayout::zone_for`'s `idx` enumerates ALL surfaces pushed into
    /// `cached_frame_hit_map` (editors THEN tab bars), not a per-tab-bar
    /// count, so a tab bar's global index is offset by however many editor
    /// surfaces were pushed before it. A plain `Vec` indexed 0.. (the
    /// original #449 shape) silently mismatched by that offset and made
    /// every `FrameZone::TabBar` lookup miss whenever at least one editor
    /// window was on screen — i.e. always — falling back to
    /// `screen_zone_hit_test` without ever exercising the new path. Keying
    /// by the real global `idx` instead of position fixes that regardless
    /// of how many editor windows precede the tab bars.
    pub(crate) cached_tab_bar_zones:
        Rc<RefCell<HashMap<usize, (core::window::GroupId, quadraui::Rect)>>>,
    /// A sidebar panel claimed the current press, so the rest of the gesture
    /// (`MouseMoved` with the left button held, then `MouseUp`) belongs to it —
    /// that is what lets a panel scrollbar thumb or a tree drag keep tracking
    /// once the pointer strays outside the sidebar. Cleared on release. An
    /// *unclaimed* drag is never intercepted, so an editor text-selection drag
    /// that wanders over the sidebar still finalises in the editor (#544).
    pub(crate) sidebar_pointer_captured: Cell<bool>,
    /// Git-sidebar band geometry (header / commit input / toolbar slab) as the
    /// last `render_content` pass painted it, from `render::sc_sidebar_bands`.
    /// `route_sc_sidebar_event` resolves presses against this so the click and
    /// paint derivations cannot drift (#544).
    pub(crate) cached_sc_bands: Cell<Option<render::ScSidebarBands>>,
    /// Debug-sidebar action-button row rect as last painted. The hit regions in
    /// `engine.dap_sidebar_action_hits` are relative to this rect's origin, so
    /// the router needs it to translate an absolute press (#544).
    pub(crate) cached_dap_action_rect: Cell<Option<quadraui::Rect>>,
    /// Per-group tab-drop geometry (absolute pixel bounds) computed each frame in
    /// `render_content`. Both the drag overlay (same frame) and the drag hit-test
    /// in `handle_mouse_drag_msg` (next mouse-move) read this, so the drop-zone
    /// detection and the highlight always use one identical bounds source. (#515)
    pub(crate) cached_drop_groups: Rc<RefCell<Vec<render::TabDropGroup>>>,
    /// Effective tab-bar height (px) paired with `cached_drop_groups`.
    pub(crate) cached_drop_tbh: Rc<Cell<f32>>,
    /// Backend (line_height, char_width) captured at the instant the file
    /// explorer tree was rendered. The backend's `current_line_height` is mutable
    /// per-frame state and may differ by click time, which made the explorer
    /// hit-test resolve the wrong row (it ran `tree_layout` at a different line
    /// height than `draw_tree` used). Re-applied before hit-testing so draw and
    /// hit agree. (#540 ShellApp port)
    pub(crate) cached_explorer_metrics: Rc<Cell<(f64, f64)>>,
    /// `(line_height, char_width)` the AI panel's `ChatController` was last
    /// painted with — the same drift `cached_explorer_metrics` guards
    /// against (#544/#819). `Backend::line_height`/`char_width` on GTK
    /// read a mutable "current" field any widget's render pass can
    /// overwrite for its own metrics; by the time a later mouse/keyboard
    /// event reaches `route_ai_chat_event`, whatever painted *last* this
    /// frame or the previous one may have left a different value there.
    /// Re-applied via `TextMetricsBackend::set_current_line_height`/
    /// `set_current_char_width` before `ChatController::handle` runs, so its
    /// row-wrap math (`total_rows`/`visible_rows`) matches what `render()`
    /// used rather than silently reading a smaller/larger viewport and
    /// throwing off the scrollbar clamp.
    pub(crate) cached_ai_chat_metrics: Rc<Cell<(f64, f64)>>,
    /// Pixel y-offset where the debug toolbar was last drawn.
    pub(crate) debug_toolbar_y_offset: Rc<Cell<f64>>,
    /// Pixel height of the debug toolbar (last draw).
    pub(crate) debug_toolbar_height: Rc<Cell<f64>>,
    /// Cached menu-dropdown hit regions from the last draw of the
    /// dropdown overlay. Each entry is `(x, y, w, h, action_id)`
    /// where `action_id` is e.g. `menu:7`. Click + motion handlers
    /// walk this list to map (x, y) → engine-side
    /// `MENU_STRUCTURE.items` index instead of computing row indices
    /// True while the user drags the terminal header row to resize the panel.
    pub(crate) terminal_resize_dragging: bool,
    /// True while the user drags the terminal split divider left/right.
    pub(crate) terminal_split_dragging: bool,
    /// The divider currently grabbed — an editor-group boundary or a
    /// `:split`/`:vsplit` window boundary (#582; each group's `WindowLayout`
    /// numbers its own splits independently, so the owning group travels with
    /// it). #753 collapsed the two mutually-exclusive `group_divider_dragging`
    /// / `window_divider_dragging` fields into the shared
    /// [`render::DividerGrab`], which TUI holds too.
    pub(crate) divider_grab: Option<render::DividerGrab>,
    /// Tab drag-and-drop arm → threshold → track → commit machine. #753
    /// replaced the four parallel fields (`tab_dragging`, `tab_drag_start`,
    /// `tab_drag_source`, `tab_drag_drop_zone`) with the shared
    /// [`render::TabDragState`], which TUI holds too.
    pub(crate) tab_drag: render::TabDragState,
    /// OS top-level window handle, type-erased behind [`PlatformWindowHandle`]
    /// (#862) so the shared paint/title-sync/minimize methods below can call
    /// it without naming `gtk4::Window` — set in `ShellApp::setup` once the
    /// runner creates the window.
    pub(crate) window: Option<Box<dyn PlatformWindowHandle>>,
    /// Editor content bounds + tab-bar height as used by the LAST
    /// `render_content` pass, in the same **absolute** DA coordinate frame
    /// that mouse events arrive in (#550, #582).
    ///
    /// `render_content` derives `editor_bounds` from
    /// `AppShellLayout::main_content_bounds`, whose origin is offset by the
    /// activity bar / sidebar (x) and the title-bar band (y). The divider
    /// hit-test and drag handlers used to re-derive their own bounds at
    /// `(0.0, 0.0)` with a *different* height formula — so every divider they
    /// computed sat roughly one activity-bar-width left (and one title-bar
    /// height up) of the line actually painted. `:vsplit` dividers were
    /// consequently unhittable and the press fell through to text-selection
    /// (#582 iteration-2 smoke failure); `:split` only appeared to work
    /// because its y-error was small enough that a click on the per-window
    /// status bar landed inside the 6px band by luck.
    ///
    /// Caching what the renderer actually used — rather than recomputing —
    /// makes hit-test-agrees-with-paint true *by construction* instead of by
    /// two formulas being kept in sync by hand.
    pub(crate) cached_editor_bounds: Cell<Option<(core::WindowRect, f64)>>,
    /// Menu bar row rect (full content width, `lh` tall) computed in
    /// `render_content` each frame. Reused by `handle()` so `MenuSystem`'s
    /// click/key routing tests against the exact rect the bar was drawn
    /// into. (#552)
    ///
    /// `Rc`-wrapped (like [`Self::title_bar_rect`]) so the headless test
    /// harness can clone a handle and *aim* pixel probes at the row the
    /// renderer actually used, instead of hardcoding chrome coordinates (#720).
    pub(crate) menu_row_rect: Rc<Cell<quadraui::Rect>>,
    /// The sub-rect of [`Self::menu_row_rect`] the menu *items* were actually
    /// drawn into — `menu_row_rect` minus the app-icon slot at its leading
    /// edge (#720). Equal to `menu_row_rect` when the icon slot is empty
    /// (menu bar hidden / zero-height row).
    ///
    /// `handle()` routes `MenuSystem` clicks against **this**, not
    /// `menu_row_rect`: the icon shifts every item's x-origin right, so a
    /// hit-test run against the unshifted band would resolve a click on
    /// `File` to whatever item now sits one slot to its left. Written once
    /// per frame by `render_content` from the same
    /// `render::split_menu_row_for_app_icon` call that positions the paint,
    /// so paint and hit-test cannot disagree (the #552 `TabBar` bug class,
    /// which quadraui's `MenuBar::layout_with_leading` doc calls out for
    /// exactly this feature).
    pub(crate) menu_items_rect: Cell<quadraui::Rect>,
    /// Rect of the drawn inline window-control buttons (minimize/maximize/
    /// close), to the right of the menu items within `menu_row_rect`. (#552)
    /// `Rc`-wrapped (like `picker_popup_rect` etc.) so the headless test
    /// harness can clone a handle and assert the Command Center (#676)
    /// never overlaps it.
    pub(crate) title_bar_rect: Rc<Cell<quadraui::Rect>>,
    /// Hover/press/click tracker for the window-control buttons, shared with
    /// quadraui's own `full_chrome_demo` reference title bar (quadraui#402)
    /// — replaces a hand-rolled `StatusBarLayout::hit_test` call with the
    /// same primitive so the buttons get real hover/press highlighting and
    /// click-on-release semantics instead of firing on press. (#552)
    pub(crate) title_bar_interaction: RefCell<quadraui::StatusBarInteraction>,
    /// Last time sc_refresh() was called for the Git sidebar auto-refresh.
    pub(crate) last_sc_refresh: std::time::Instant,
    /// Link hit rects populated during hover popup draw: (rect, url, is_native).
    pub(crate) panel_hover_link_rects: Rc<RefCell<Vec<(quadraui::Rect, String, bool)>>>,
    /// Popup bounding rect — set during draw, used for motion hit-testing.
    #[allow(dead_code)]
    pub(crate) panel_hover_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Editor hover popup bounding rect — set during draw, used for click hit-testing.
    pub(crate) editor_hover_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Completion popup layout — set during draw, used for hit-test in
    /// the click handler. None when the popup isn't visible.
    pub(crate) completion_layout: Rc<RefCell<Option<quadraui::CompletionsLayout>>>,
    /// Context menu layout — set during draw, used for hit-test in
    /// both click and motion handlers. None when no menu is visible.
    pub(crate) context_menu_layout: Rc<RefCell<Option<quadraui::ContextMenuLayout>>>,
    /// Tab-switcher popup bounding rect — set during
    /// draw, used for `ModalStack` registration in the click
    /// handler. (B.5b Stage 7.)
    pub(crate) tab_switcher_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// The frame rungs this frame actually composed, in composition order
    /// (#735, folded into one sequence by #766).
    ///
    /// Written by the single [`render::compose_frame`] walk in
    /// `render_content` — every arm that draws pushes its own
    /// [`render::FrameOp`], and arms whose surface turned out to be absent do
    /// not. It is the *observable* that makes "both backends compose the frame
    /// in the same order" testable: `TuiShellApp` keeps the identical field,
    /// and the two backends' recorded sequences are asserted equal against the
    /// same expected `Vec<FrameOp>` (`render::frame_sequence_fixture`).
    ///
    /// Before #766 this was *two* fields — `painted_overlay_band` and
    /// `composed_chrome_band` — so "the frame's sequence" was still two
    /// observables a backend could get individually right and jointly wrong.
    ///
    /// Cheap enough to keep in release builds (at most
    /// `FRAME_Z_ORDER.len()` pushes into a reused `Vec` per frame) and
    /// useful there too — `check_frame_order` turns a z-order
    /// inversion into a diagnosable string rather than a visual mystery.
    pub(crate) composed_frame: Rc<RefCell<Vec<render::FrameOp>>>,
    /// The editor-band twin of [`Self::composed_frame`] (#764): written
    /// by the [`render::compose_editor_band`] walk, one [`render::EditorOp`]
    /// pushed by every arm that actually composed its rung. Same `Rc`
    /// rationale, same role — it is what makes "both backends compose the
    /// editor column in the same order" assertable rather than promised in
    /// comments, which is how this backend came to omit the group dividers
    /// entirely while still hit-testing drags against them.
    pub(crate) composed_editor_band: Rc<RefCell<Vec<render::EditorOp>>>,
    /// The bottom-band twin of [`Self::composed_editor_band`] (#765): written
    /// by the [`render::compose_bottom_band`] walk, one [`render::BottomOp`]
    /// pushed by every arm that actually composed its rung. Same `Rc`
    /// rationale, same role — and the same class of defect behind it: this
    /// backend used to nest the panel-hover popup inside the sidebar rung,
    /// where it could neither paint nor *clear its own click-routing cache*
    /// once the sidebar collapsed.
    pub(crate) composed_bottom_band: Rc<RefCell<Vec<render::BottomOp>>>,
    /// Picker/command-palette popup rect **as the last frame
    /// actually painted it** — see [`App::compute_picker_popup_bounds`] for
    /// why the click path must not re-derive it (#555).
    pub(crate) picker_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Folder-picker popup rect **as the last frame actually
    /// painted it** (#815) — same shape and the same "click
    /// path reads the painted rect, never re-derives it" rationale as
    /// [`Self::picker_popup_rect`] just above. Cleared whenever the picker
    /// is closed, alongside the other overlay-tail caches (#766).
    pub(crate) folder_picker_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Line height the last frame actually painted with, published by
    /// `render_content` — see [`App::painted_line_height`] (#555).
    pub(crate) painted_line_height: Rc<Cell<Option<f64>>>,
    /// Character-cell advance the last frame actually painted with — the
    /// horizontal twin of [`Self::painted_line_height`], and published for
    /// exactly the same reason (#751).
    ///
    /// `render_content` paints at `cached_char_width.max(backend.char_width())`,
    /// but click-time hit-tests read the plain `cached_char_width`, which is
    /// seeded once in `setup()` from the runner's *default* metrics. With a
    /// real font those differ (8.0 vs. ~9.14 in the headless harness), so a
    /// cell-unit overlay hit-tested against the smaller value drifted further
    /// right the further into the panel the pointer went — the find/replace
    /// toggles resolved to the *input field* four cells to their left.
    pub(crate) painted_char_width: Rc<Cell<Option<f64>>>,
    /// The sidebar content area the last frame painted a panel into
    /// (`ShellContext::layout.sidebar_content_bounds`), or `None` when the
    /// sidebar was hidden. Published purely so the headless harness can aim a
    /// click at the panel the renderer actually drew instead of guessing pixel
    /// offsets — the same "locate targets, never hardcode coords" rule
    /// `screen_layout` / `tab_slots_abs` exist for (#544).
    pub(crate) painted_sidebar_bounds: Rc<Cell<Option<quadraui::Rect>>>,
    /// Link hit rects populated during editor hover popup draw: (rect, url).
    pub(crate) editor_hover_link_rects: Rc<RefCell<Vec<(quadraui::Rect, String)>>>,
    /// Editor hover popup scrollbar geometry (#215). Populated by
    /// `draw_editor_hover_popup`; consumed by click + drag handlers
    /// in this file.
    pub(crate) editor_hover_scrollbar: Rc<Cell<Option<render::PopupScrollbarHit>>>,
    /// CSS provider registered with the GTK display — updated when colorscheme changes.
    ///
    /// `None` only under the headless test harness ([`App::new_headless`], #646):
    /// `gtk4::CssProvider::new()` asserts `gtk::init` has run, which it cannot
    /// with no display, and a provider that is attached to no `GdkDisplay`
    /// styles nothing anyway. Always `Some` in a live run.
    pub(crate) css_provider: Option<Box<dyn PlatformCssProvider>>,
    /// Colorscheme name at the time the CSS was last applied.
    pub(crate) last_colorscheme: String,
    /// A second, standalone `quadraui::Backend`-impl handle, distinct from
    /// the `&mut dyn quadraui::Backend` the `ShellApp` runner hands
    /// `setup`/`handle`/`tick` — this one is owned outright by `App` for
    /// click hit-testing and the two `TextMetricsBackend` calls that need a
    /// long-lived handle across borrow-drop points (see the #560 comment in
    /// `render_content`). Owns the canonical accelerators / event-queue /
    /// viewport / services / modal-stack / drag-state. Call sites reach
    /// modal-stack and drag-state via `self.backend.borrow().modal_stack_handle()`
    /// and `drag_state_handle()` (B.5b Stage 11 dropped the alias `Rc` clones
    /// that previously lived at `App.modal_stack` / `App.drag_state`). The
    /// `init` drain timer holds a clone and pumps `poll_events()` every 16 ms.
    ///
    /// Typed against [`TextMetricsBackend`] rather than the concrete
    /// `backend::GtkBackend` (#813) — see that trait's doc comment for why
    /// a bare `Box<dyn quadraui::Backend>` isn't quite enough on its own.
    pub(crate) backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
    /// #1064: what this app believes the **runner's** `AppShell` (the
    /// `ShellAdapter`-owned instance that paints the activity bar and
    /// sidebar header — NOT `engine.app_shell`, the shadow copy
    /// `render_content` reads for panel content) currently has as its
    /// active panel. Updated only from [`Self::on_shell_event`]'s
    /// `PanelChanged` notifications — the single channel through which the
    /// runner reports its own state — and compared against the shadow's
    /// `active_panel_id()`/`ext_panel_active` in
    /// [`quadraui::ShellApp::take_requested_panel`] to detect an
    /// **app-initiated** switch (e.g. `Engine::process_pending_sidebar`'s
    /// DAP `dap_wants_sidebar` reveal, or the `toggle_focus_explorer`/
    /// `toggle_focus_search` keyboard accelerators) the runner would
    /// otherwise never learn about. Mirrors `TuiShellApp::last_shell_panel`
    /// verbatim — see that field's own doc for the full rationale. Plain
    /// field, not `Rc`/`RefCell`: `take_requested_panel` and
    /// [`Self::on_shell_event`] both take `&mut self`, so no interior
    /// mutability is needed (matching `TuiShellApp`'s own field).
    pub(crate) last_shell_panel: Option<quadraui::WidgetId>,
    /// #1064: set by `take_requested_panel` just before it returns `Some`,
    /// consumed by the `PanelChanged` arm of [`Self::on_shell_event`].
    /// `ShellAdapter::apply_requested_panel` re-notifies the app with the
    /// same `PanelChanged` a mouse click produces — but for a
    /// reconciliation echo the engine *already* holds that state, so the
    /// echo must only update [`Self::last_shell_panel`] and must NOT
    /// re-run the click path in `switch_panel`/`draw_needed`, which for an
    /// already-active `ext:` panel would toggle the sidebar back **off**
    /// (see `render::apply_activity_panel_switch`'s `already_showing`
    /// arm). Mirrors `TuiShellApp::suppress_shell_panel_echo`.
    pub(crate) suppress_shell_panel_echo: bool,
}

/// Decode an activity bar widget ID into a panel ID for [`App::switch_panel`].
/// Dead in ShellApp mode until the activity bar DA is re-wired (#448-C follow-on).
#[allow(dead_code)]
fn activity_id_to_panel_id(id: &str) -> Option<String> {
    match id {
        "activity:explorer" => Some(PANEL_EXPLORER.to_string()),
        "activity:search" => Some(PANEL_SEARCH.to_string()),
        "activity:debug" => Some(PANEL_DEBUG.to_string()),
        "activity:git" => Some(PANEL_GIT.to_string()),
        "activity:extensions" => Some(PANEL_EXTENSIONS.to_string()),
        "activity:ai" => Some(PANEL_AI.to_string()),
        "activity:settings" => Some(PANEL_SETTINGS.to_string()),
        other => other
            .strip_prefix("activity:ext:")
            .map(|name| format!("ext:{name}")),
    }
}

/// Map GDK key names to the engine's expected key names.
///
/// This is the canonical superset mapping — callers that only care about a
/// subset simply ignore the extra translations (they're harmless).
fn map_gtk_key_name(gdk_name: &str) -> &str {
    match gdk_name {
        "Return" | "KP_Enter" => "Return",
        "Escape" => "Escape",
        "BackSpace" => "BackSpace",
        "Delete" => "Delete",
        "Tab" => "Tab",
        "ISO_Left_Tab" => "BackTab",
        "Up" => "Up",
        "Down" => "Down",
        "Left" => "Left",
        "Right" => "Right",
        "Home" => "Home",
        "End" => "End",
        "Page_Down" | "KP_Page_Down" => "PageDown",
        "Page_Up" | "KP_Page_Up" => "PageUp",
        "space" => " ",
        "slash" => "/",
        "question" => "?",
        other => other,
    }
}

fn gtk_key_name_to_quadraui(mapped: &str, ctrl: bool) -> Option<quadraui::UiEvent> {
    use quadraui::{Key, Modifiers, NamedKey, UiEvent};
    let key = match mapped {
        "Down" => Key::Named(NamedKey::Down),
        "Up" => Key::Named(NamedKey::Up),
        "Home" => Key::Named(NamedKey::Home),
        "End" => Key::Named(NamedKey::End),
        "PageDown" => Key::Named(NamedKey::PageDown),
        "PageUp" => Key::Named(NamedKey::PageUp),
        "Tab" => Key::Named(NamedKey::Tab),
        "Return" => Key::Named(NamedKey::Enter),
        " " => Key::Char(' '),
        "j" => Key::Char('j'),
        "k" => Key::Char('k'),
        "g" => Key::Char('g'),
        "G" => Key::Char('G'),
        _ => return None,
    };
    Some(UiEvent::KeyPressed {
        key,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::default()
        },
        repeat: false,
    })
}

/// Map a GDK key name and extract the unicode character for input-mode handlers.
///
/// Returns `(mapped_key_name, unicode)`.  Special keys return `None` for unicode;
/// single-character key names return the character as `Some(ch)`.
fn map_gtk_key_with_unicode(gdk_name: &str) -> (&str, Option<char>) {
    match gdk_name {
        "Return" | "KP_Enter" => ("Return", None),
        "Escape" => ("Escape", None),
        "BackSpace" => ("BackSpace", None),
        "Delete" => ("Delete", None),
        "Up" => ("Up", None),
        "Down" => ("Down", None),
        "Left" => ("Left", None),
        "Right" => ("Right", None),
        "Home" => ("Home", None),
        "End" => ("End", None),
        "Tab" => ("Tab", None),
        "ISO_Left_Tab" => ("BackTab", None),
        "Page_Up" => ("Page_Up", None),
        "Page_Down" => ("Page_Down", None),
        "question" => ("?", Some('?')),
        "slash" => ("/", Some('/')),
        other => {
            let mut chars = other.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                (other, Some(ch))
            } else {
                (other, None)
            }
        }
    }
}

/// Set up system clipboard callbacks on the engine via copypasta_ext.
///
/// On X11 we prefer `x11_bin` (xclip/xsel subprocesses) over `try_context`'s
/// default `x11_fork`: the fork variant opens its own in-process X11 connection
/// and contends with GTK's main-thread X11 event loop. Subprocess reads do not.
///
/// Only `App::new` (`gui`-gated) calls this today; it names no toolkit type
/// (`copypasta_ext` is a plain, unconditional dependency) so it stays
/// un-gated itself, allowed rather than `#[cfg]`-gated.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
fn setup_gtk_clipboard(engine: &mut Engine) {
    let ctx: Option<Box<dyn ClipboardProviderExt>> = {
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        ))]
        if copypasta_ext::display::is_x11() {
            copypasta_ext::x11_bin::ClipboardContext::new()
                .ok()
                .map(|c| Box::new(c) as Box<dyn ClipboardProviderExt>)
                .or_else(|| {
                    // xclip/xsel aren't on PATH, so `x11_bin` failed. Do NOT
                    // fall back to `copypasta_ext::try_context()` here — on
                    // X11 that prefers `x11_fork::ClipboardContext` by
                    // default (#587 Problem 2, discovered via manual GTK
                    // smoke test): its `set_contents` calls `fork()` inside
                    // this GTK4 process, and its `get_contents` opens its
                    // own in-process X11 connection. Forking a process with
                    // GTK's thread pool, glib workers, gdbus and Cairo/Mesa
                    // threads risks the child inheriting a mutex locked by a
                    // thread that doesn't exist in the child and hanging
                    // forever; the extra connection also contends with
                    // GTK's main-thread X11 event loop per the module doc
                    // above. Any machine without xclip/xsel installed hit
                    // this fallback and froze the whole app on clipboard
                    // access. `X11ClipboardContext` (used here directly,
                    // bypassing `x11_fork`) does the same I/O on a
                    // background thread with a bounded (3s) read timeout
                    // and never calls `fork()`.
                    use copypasta_ext::copypasta::x11_clipboard::{Clipboard, X11ClipboardContext};
                    X11ClipboardContext::<Clipboard>::new()
                        .ok()
                        .map(|c| Box::new(c) as Box<dyn ClipboardProviderExt>)
                })
        } else {
            copypasta_ext::try_context()
        }
        #[cfg(not(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )))]
        copypasta_ext::try_context()
    };

    let Some(ctx) = ctx else { return };
    // `engine.clipboard_{read,write}` are `Fn` (shared-ref callbacks), but
    // `ClipboardProviderExt::{get,set}_contents` take `&mut self`. Wrap the
    // provider in `Rc<RefCell<…>>` so both closures can share it and acquire
    // a mutable borrow at call time.
    let ctx = Rc::new(RefCell::new(ctx));

    let read_ctx = ctx.clone();
    engine.clipboard_read = Some(Box::new(move || {
        read_ctx
            .borrow_mut()
            .get_contents()
            .map_err(|e| format!("clipboard read: {e}"))
    }));

    let write_ctx = ctx;
    engine.clipboard_write = Some(Box::new(move |text: &str| {
        write_ctx
            .borrow_mut()
            .set_contents(text.to_string())
            .map_err(|e| format!("clipboard write: {e}"))
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
fn dialog_btn_index(id: &quadraui::WidgetId) -> Option<usize> {
    id.as_str()
        .strip_prefix("dialog:btn:")
        .and_then(|s| s.parse::<usize>().ok())
}

/// The app icon to paint in the menu row. Under `gui`, defers to
/// `crate::gtk::util::app_icon_image` (the once-rasterised PNG — see its doc
/// comment for why the raw SVG must never reach `Backend::draw_image`
/// directly). No non-GTK backend paints this yet, so the fallback is the
/// plain, un-rasterised builder — never exercised in production today, but
/// keeps this function (and so `render_content`) resolving without `gui`.
fn app_icon_image_for_paint() -> quadraui::Image {
    #[cfg(feature = "gui")]
    {
        crate::gtk::util::app_icon_image()
    }
    #[cfg(not(feature = "gui"))]
    {
        crate::render::app_icon_image()
    }
}

/// Create a new `App` instance.
///
/// All widget-dependent setup (window handle, CSS) is deferred to
/// `ShellApp::setup()`, called by the runner once the window exists.
impl App {
    /// `backend` is supplied by the caller rather than constructed here
    /// (#861): before this, `App::assemble` hardcoded
    /// `Box::new(backend::GtkBackend::new())`, so nothing upstream of this
    /// function — including `App::new` itself — had any seam to hand
    /// `App` a different `TextMetricsBackend` impl. `src/gtk/mod.rs::run`
    /// is the only caller today and it still passes a `GtkBackend`, but
    /// the choice of concrete type now lives at the call site instead of
    /// being baked into `App`.
    #[cfg(feature = "gui")]
    pub(crate) fn new(
        file_path: Option<PathBuf>,
        backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
    ) -> Self {
        // Icon search path setup.
        if let Some(home) = std::env::var_os("HOME") {
            let icon_dir = std::path::PathBuf::from(home).join(".local/share/icons");
            if let Some(display) = gdk::Display::default() {
                let icon_theme = gtk4::IconTheme::for_display(&display);
                icon_theme.add_search_path(&icon_dir);
            }
        }
        install_bundled_icon_font();

        let mut engine = {
            let mut e = Engine::new();
            // #999: record this as a GUI backend *before* resolving
            // `use_nerd_fonts()` — GUI bundles the icon font, so an unset
            // setting inherits `true` regardless of OS.
            icons::set_gui_backend(true);
            icons::set_nerd_fonts(e.settings.use_nerd_fonts());
            e.startup(file_path.as_deref());
            e
        };
        setup_gtk_clipboard(&mut engine);

        let initial_theme = Theme::from_name(&engine.settings.colorscheme);
        let css_provider: Option<Box<dyn PlatformCssProvider>> =
            Some(Box::new(crate::gtk::css::load_css(&initial_theme)));
        let last_colorscheme = engine.settings.colorscheme.clone();
        if let Some(gtk_settings) = gtk4::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(!initial_theme.is_light());
        }

        let engine = Rc::new(RefCell::new(engine));
        unsafe {
            crate::core::swap::register_emergency_engine(
                engine.as_ptr() as *const crate::core::Engine
            );
        }

        let deferred = DeferredQueue::new();

        // #949: settings.json hot-reload used to need a GTK-only
        // `gio::FileMonitor` constructed here. It's gone — `handle_poll_tick`
        // now calls `Engine::check_settings_reload`'s portable mtime poll on
        // every tick, the same mechanism TUI has always used, so there is
        // nothing left for this constructor to set up.
        Self::assemble(engine, deferred, css_provider, last_colorscheme, backend)
    }

    /// Backend-neutral twin of [`App::new`] (#859) — what a wrapper over a
    /// non-GTK quadraui backend calls to get the *same* `App`, and therefore
    /// the same `impl ShellApp`, the GTK entry point runs.
    ///
    /// This is [`App::new`] minus exactly three steps in its prologue that
    /// need a live GTK display, each of which is a platform *resource*
    /// rather than a decision:
    ///
    /// | skipped | why | what replaces it |
    /// |---|---|---|
    /// | `gdk::Display` icon-theme search path | GDK-only; no portable icon-theme concept exists off GTK | nothing — no other backend has an icon theme to seed |
    /// | `crate::gtk::css::load_css` | `unwrap()`s `gdk::Display::default()` | `css_provider: None` — a GTK stylesheet styles nothing on another toolkit |
    /// | `gtk4::Settings::set_gtk_application_prefer_dark_theme` | GTK-only | the backend's own light/dark handling |
    ///
    /// A fourth row used to live in this table: the GTK-only
    /// `gio::FileMonitor` on `settings.json`, replaced here by
    /// `settings_monitor: None` — a known hot-reload gap off GTK. #949
    /// deleted the monitor from [`App::new`] entirely rather than adding a
    /// portable equivalent here, since `Engine::check_settings_reload`'s
    /// mtime poll (already the sole mechanism on TUI) now runs from the
    /// shared `handle_poll_tick`, which both constructors' `App`s reach via
    /// `ShellApp::tick`. That closes this gap **for macOS** for free —
    /// nothing needed adding here at all — because quadraui's
    /// `macos::run` keeps the same `IDLE_POLL_CEILING` (250ms) idle-tick
    /// fallback GTK does (quadraui#940's `idlePollTick:` timer).
    ///
    /// **Win-GUI is only half-fixed, per the very doc this claim leans
    /// on** (quadraui#832/#940's `AppLogic::tick` table, `runner.rs`):
    /// Windows gets *no* idle-poll fallback at all — `tick` there only
    /// runs after a batch of native events or an explicit
    /// `RedrawAfter`/`request_frame_in` ask, neither of which this diff
    /// arranges. So a future Win-GUI backend picks up an
    /// externally-edited `settings.json` while the user is actively
    /// generating native events (typing, moving the mouse), but not while
    /// the app sits idle — the exact "edit settings.json externally, come
    /// back to it" scenario hot-reload exists for. Whoever builds the
    /// Win-GUI backend (quadraui#19–#31) needs an explicit periodic
    /// `RedrawAfter`/`request_frame_in` nudge for this to work there the
    /// way it does on GTK/macOS/TUI; there is no such backend in this
    /// repo yet, so this is not a live regression today, only a caveat
    /// for that future work.
    ///
    /// **`install_bundled_icon_font()` is *not* in that skipped list (#920).**
    /// It used to be — `App::new` called it and this constructor didn't, so
    /// every non-GTK backend (macOS first, per #920's repro) silently shipped
    /// no icon font at all. The function itself now picks its destination and
    /// cache-refresh step per platform (`~/.local/share/fonts` + `fc-cache`
    /// off macOS, `~/Library/Fonts` and no cache step on macOS — see
    /// `app_support::icon_font_dest_dir`), so calling it here is correct for
    /// every backend this constructor serves, the same way it already was for
    /// GTK.
    ///
    /// Everything else — engine construction and startup, nerd-font
    /// selection, the clipboard provider (`setup_gtk_clipboard` names no
    /// toolkit type and already `cfg`s its X11 branch off on macOS), the
    /// emergency-engine registration the panic hook's swap flush needs, and
    /// the whole of [`App::assemble`] — is shared verbatim, so the two
    /// constructors cannot drift on anything that affects behaviour.
    ///
    /// `backend` is the caller's [`TextMetricsBackend`], the seam #861 opened
    /// and `src/gtk/mod.rs::run` names in its own comment as "the seam a
    /// future non-GTK wrapper (#859) would pass a different
    /// `TextMetricsBackend` impl through".
    ///
    /// The `allow(dead_code)` is feature-shaped, not a silencer: the callers
    /// are `crate::macos::run` (double-gated on `macos` + `target_os =
    /// "macos"`) and, since #866, `crate::win::run` (`win`, un-target-gated
    /// — see that module's doc comment for why). Keeping the function itself
    /// **un**gated means every lane still type-checks it.
    #[cfg_attr(
        not(any(feature = "win", all(feature = "macos", target_os = "macos"))),
        allow(dead_code)
    )]
    pub(crate) fn new_portable(
        file_path: Option<PathBuf>,
        backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
    ) -> Self {
        // #920: `App::new` calls this too (for GTK/Pango); it has to happen
        // here as well or every non-GTK backend — macOS first — ships a
        // bundled icon font that never reaches disk. The function itself
        // picks the right destination and cache-refresh step per platform.
        install_bundled_icon_font();

        let mut engine = {
            let mut e = Engine::new();
            // #999: same GUI-backend-then-resolve ordering as `App::new`
            // above — every non-GTK GUI backend this constructor serves
            // (macOS, Win-GUI) bundles the icon font too.
            crate::icons::set_gui_backend(true);
            crate::icons::set_nerd_fonts(e.settings.use_nerd_fonts());
            e.startup(file_path.as_deref());
            e
        };
        setup_gtk_clipboard(&mut engine);

        let last_colorscheme = engine.settings.colorscheme.clone();

        let engine = Rc::new(RefCell::new(engine));
        // SAFETY: identical contract to `App::new`'s own call — the `Rc` is
        // moved into the returned `App`, which the caller hands straight to
        // a `run_with_shell` that owns it for the rest of the process, so
        // the pointer never dangles. `crate::macos::run` is the only caller
        // and does exactly that.
        unsafe {
            crate::core::swap::register_emergency_engine(
                engine.as_ptr() as *const crate::core::Engine
            );
        }

        Self::assemble(
            engine,
            DeferredQueue::new(),
            None,
            last_colorscheme,
            backend,
        )
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
                p.icon = match p.id.as_str() {
                    "panel:explorer" => crate::icons::EXPLORER.s().to_string(),
                    "panel:search" => crate::icons::SEARCH.s().to_string(),
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
        // #557: plugin-provided panels (e.g. the Git Insights extension) live
        // in `engine.ext_panels`, not in the engine's `AppShell` — nothing
        // registers them there — so they have to be appended explicitly or
        // the runner's activity bar renders no icon for them at all.
        // `ext_activity_panels` already carries each panel's resolved icon,
        // so the id→glyph match above deliberately needs no arm for them.
        top_panels.extend(self.engine.borrow().ext_activity_panels());

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
        // `ShellConfig::with_title_bar` to fall back to (unlike
        // `with_activity_bar_width_px` below, which exists for exactly this
        // reason on the activity bar). Concretely: a user who runs
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
            .with_title_bar(2.0)
            // #940/quadraui#947: opt into the client-side titlebar so a
            // capable backend (macOS today) puts the reserved band *in* the
            // real titlebar, beside the native traffic lights, instead of
            // underneath it. Requested unconditionally rather than gated on
            // `target_os = "macos"` (the Platform-Neutrality Rule) — GTK and
            // Win-GUI simply don't honour this field yet
            // (`ShellConfig::client_side_titlebar`'s own doc, and
            // `ACCEPTED_DEFAULTS` in quadraui's `tests/conformance/caps.rs`),
            // so setting it there is inert today and each backend adopts it
            // on its own schedule with no vimcode-side change needed.
            .with_client_side_titlebar()
            // #719/quadraui#657: the activity bar's row height is the fixed
            // `ACTIVITY_ROW_PX = 48.0` (VS Code parity), so sizing its
            // *width* from the editor font makes it oblong. Pin to 48px.
            .with_activity_bar_width_px(48.0);
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
        cfg = cfg.with_editor_font(
            self.engine.borrow().settings.font_family.clone(),
            self.engine.borrow().settings.font_size as f32,
        );
        // #759: the shared Alt rung clamps sidebar width, so Alt+Left/Right
        // resolve identically on every backend.
        cfg.min_sidebar_width = render::ALT_SIDEBAR_WIDTH_MIN as f32;
        cfg.max_sidebar_width = render::ALT_SIDEBAR_WIDTH_MAX as f32;
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
    /// soundness trap #635 documented on `TuiShellApp::live`).
    ///
    /// Everything below this line is plain `Rc`/`Cell`/`RefCell` allocation;
    /// none of it touches GDK. `backend` is taken as a parameter rather than
    /// constructed here (#861) — see [`App::new`]'s doc comment.
    ///
    /// Named no toolkit type in its own signature even before #862 (#861
    /// already erased `backend`'s concrete type), so it stays un-gated
    /// itself; only `App::new`/`App::new_headless` — its sole callers today,
    /// both `gui`-gated — construct the arguments this needs.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    fn assemble(
        engine: Rc<RefCell<Engine>>,
        deferred: DeferredQueue,
        css_provider: Option<Box<dyn PlatformCssProvider>>,
        last_colorscheme: String,
        backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
    ) -> Self {
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
            deferred,
            last_clipboard_content: None,
            tab_close_hover: None,
            cached_tab_close_abs: Rc::new(RefCell::new(HashMap::new())),
            cached_tab_slots_abs: Rc::new(RefCell::new(HashMap::new())),
            cached_tab_pixel_hits: Rc::new(RefCell::new(HashMap::new())),
            status_segment_map: Rc::new(RefCell::new(HashMap::new())),
            separated_status_bar_rect: Rc::new(Cell::new(None)),
            global_status_zones: Rc::new(RefCell::new(Vec::new())),
            cached_screen_layout: Rc::new(RefCell::new(None)),
            cached_frame_hit_map: Rc::new(RefCell::new(None)),
            sidebar_pointer_captured: Cell::new(false),
            cached_sc_bands: Cell::new(None),
            cached_dap_action_rect: Cell::new(None),
            cached_tab_bar_zones: Rc::new(RefCell::new(HashMap::new())),
            cached_drop_groups: Rc::new(RefCell::new(Vec::new())),
            cached_drop_tbh: Rc::new(Cell::new(0.0)),
            cached_explorer_metrics: Rc::new(Cell::new((16.0, 8.0))),
            cached_ai_chat_metrics: Rc::new(Cell::new((16.0, 8.0))),
            debug_toolbar_y_offset: Rc::new(Cell::new(0.0)),
            debug_toolbar_height: Rc::new(Cell::new(0.0)),
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            divider_grab: None,
            tab_drag: render::TabDragState::default(),
            window: None,
            cached_editor_bounds: Cell::new(None),
            menu_row_rect: Rc::new(Cell::new(quadraui::Rect::default())),
            menu_items_rect: Cell::new(quadraui::Rect::default()),
            title_bar_rect: Rc::new(Cell::new(quadraui::Rect::default())),
            title_bar_interaction: RefCell::new(quadraui::StatusBarInteraction::new()),
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
            picker_popup_rect: Rc::new(Cell::new(None)),
            folder_picker_popup_rect: Rc::new(Cell::new(None)),
            painted_sidebar_bounds: Rc::new(Cell::new(None)),
            painted_line_height: Rc::new(Cell::new(None)),
            painted_char_width: Rc::new(Cell::new(None)),
            editor_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            editor_hover_scrollbar: Rc::new(Cell::new(None)),
            css_provider,
            last_colorscheme,
            backend,
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
        }
    }

    /// Build an `App` around a caller-supplied, fully in-memory [`Engine`] with
    /// **no** display-dependent setup — the GTK twin of `TuiShellApp::new` for
    /// tests (#646). Feed the result to `crate::gtk::testing::harness`, which
    /// wraps it in `quadraui::gtk::testing::driver_with_shell`.
    ///
    /// Deliberately skips, relative to [`App::new`]:
    ///
    /// - `gdk::Display::default()` icon-theme search paths and
    ///   `install_bundled_icon_font` (writes to `~/.local/share/fonts` and
    ///   shells out to `fc-cache` — a test must not touch the user's system).
    /// - `load_css`, which `unwrap()`s `gdk::Display::default()` and therefore
    ///   panics with no `DISPLAY`. `css_provider` is left `None` — even
    ///   `gtk4::CssProvider::new()` asserts `gtk::init` has run, and a provider
    ///   attached to no display styles nothing.
    /// - `gtk4::Settings::default()` (needs a display).
    /// - `setup_gtk_clipboard`, which probes X11 / spawns `xclip`.
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
    pub(super) fn new_headless(engine: Rc<RefCell<Engine>>) -> Self {
        Self::new_headless_with_backend(
            engine,
            Rc::new(RefCell::new(Box::new(backend::GtkBackend::new()))),
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
    /// per backend — the `TextMetricsBackend` trait object is already the
    /// only place either backend's concrete type appears.
    ///
    /// Ungated on `gui` deliberately: every caller is a test lane, and the
    /// macOS lane (`--no-default-features --features macos`) compiles no GTK
    /// at all.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new_headless_with_backend(
        engine: Rc<RefCell<Engine>>,
        backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
    ) -> Self {
        // #999: this constructor is the shared headless `App` test seam for
        // every GUI backend (GTK, and the macOS driver-tier test per this
        // fn's own doc), so it's a GUI backend for `use_nerd_fonts()`
        // resolution purposes the same as `App::new`/`App::new_portable`.
        crate::icons::set_gui_backend(true);
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
            None,
            last_colorscheme,
            backend,
        )
    }
}

impl App {
    /// Run a file dialog requested via [`PendingFileDialog`] (#572), using
    /// the runner-owned `backend`'s `PlatformServices` — `show_file_open_dialog`
    /// / `show_file_save_dialog` block (via quadraui's nested-mainloop pump,
    /// #427) until the user picks or cancels, then this returns synchronously.
    fn run_pending_file_dialog(
        &mut self,
        req: PendingFileDialog,
        backend: &mut dyn quadraui::Backend,
    ) {
        use quadraui::FileDialogOptions;
        match req {
            PendingFileDialog::OpenFile => {
                let path = backend.services().show_file_open_dialog(FileDialogOptions {
                    title: Some("Open File".to_string()),
                    ..Default::default()
                });
                if let Some(path) = path {
                    let _ = self
                        .engine
                        .borrow_mut()
                        .open_file_with_mode(&path, crate::core::engine::OpenMode::Permanent);
                    self.refresh_file_tree();
                }
            }
            PendingFileDialog::SaveWorkspaceAs => {
                let path = backend.services().show_file_save_dialog(FileDialogOptions {
                    title: Some("Save Workspace As".to_string()),
                    initial_filename: Some(".vimcode-workspace".to_string()),
                    ..Default::default()
                });
                if let Some(path) = path {
                    self.engine.borrow_mut().save_workspace_as(&path);
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Present the native message dialog queued by `render_content`'s
    /// edge-trigger check (#727), using the runner-owned `backend`'s
    /// `PlatformServices` — `show_message_dialog` blocks (via quadraui's
    /// nested-mainloop pump, #666, the same adapter #427's file dialogs
    /// use) until the user picks a button or dismisses it. Mirrors
    /// `run_pending_file_dialog` above.
    ///
    /// Maps the response back through the same `"dialog:btn:N"` id
    /// convention and `Engine::dialog_click_button` / `Engine::dialog_cancel`
    /// the in-canvas `DialogHit::Button(id)` mouse path
    /// (`handle_mouse_click_msg`) already uses — `None` (dismissed with no
    /// button chosen: Escape, close box) maps to `dialog_cancel()`, the
    /// same outcome the in-canvas dialog's Escape key produces — so both
    /// paths funnel through the identical `EngineAction` outcomes.
    fn run_pending_native_dialog(
        &mut self,
        opts: quadraui::MessageDialogOptions,
        backend: &mut dyn quadraui::Backend,
    ) {
        let choice = backend.services().show_message_dialog(opts);
        // Reset the edge-trigger flag *here*, before running the engine
        // callback below, rather than only lazily on the next
        // `render_content` call that observes `screen.dialog == None`. If
        // `dialog_click_button`/`dialog_cancel` ever opens a second dialog
        // synchronously (e.g. a chained "save failed, retry?" prompt), that
        // new dialog needs `native_dialog_shown == false` to be seen as a
        // fresh no-dialog-to-dialog edge and get queued for its own native
        // present — a stale `true` left over from the dialog that just
        // closed would otherwise suppress it silently. No such chain exists
        // in `process_dialog_result` today, but resetting eagerly here
        // costs nothing and removes the latent trap either way.
        self.native_dialog_shown.set(false);
        let action = match choice.as_ref().and_then(dialog_btn_index) {
            Some(idx) => self.engine.borrow_mut().dialog_click_button(idx),
            None => self.engine.borrow_mut().dialog_cancel(),
        };
        self.apply_dialog_action(action);
        self.draw_needed.set(true);
    }

    /// Apply the `EngineAction` produced by dismissing a dialog — clears
    /// `explorer_needs_refresh` (some dialog outcomes, e.g. "Discard &
    /// Close", can trigger a sidebar refresh) and handles quit/save-quit.
    /// Shared by the in-canvas mouse-click path
    /// (`handle_mouse_click_msg`'s dialog-button block) and the native
    /// message-dialog path (`run_pending_native_dialog`, #727) so both
    /// produce exactly the same outcome for a given `EngineAction`.
    fn apply_dialog_action(&mut self, action: EngineAction) {
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }
        match action {
            EngineAction::Quit | EngineAction::SaveQuit => {
                self.save_session_and_exit();
            }
            _ => {}
        }
    }

    /// Open the tab context menu for `tab_idx` in `group_id`, anchored at the
    /// click's pixel position.
    ///
    /// #732 tranche 1: was `Msg::TabRightClick`, constructed by
    /// `ShellApp::handle` from a `UiEvent::MouseDown` it already held and
    /// immediately decoded again by `dispatch`.
    fn handle_tab_right_click(
        &mut self,
        group_id: core::window::GroupId,
        tab_idx: usize,
        x: f64,
        y: f64,
    ) {
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        let cx = (x / cw) as u16;
        let cy = (y / lh) as u16;
        self.engine
            .borrow_mut()
            .open_tab_context_menu(group_id, tab_idx, cx, cy);
        self.draw_needed.set(true);
    }

    /// Open the editor (buffer text) context menu at the click's pixel
    /// position, unless a focused modal wants to swallow the click.
    fn handle_editor_right_click(&mut self, x: f64, y: f64) {
        // Swallow if the click landed on a focused modal that
        // wants to consume it (#216 — editor hover popup).
        self.reconcile_editor_hover_modal();
        let stack_rc = self.backend.borrow().modal_stack_handle();
        let in_modal = stack_rc
            .borrow()
            .hit_test(quadraui::Point {
                x: x as f32,
                y: y as f32,
            })
            .is_some();
        if in_modal {
            return;
        }
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        let cx = (x / cw) as u16;
        let cy = (y / lh) as u16;
        self.engine.borrow_mut().open_editor_context_menu(cx, cy);
        self.draw_needed.set(true);
    }

    /// Handle a window/viewport resize.
    fn handle_resize(&mut self) {
        // #731: both branches here were gated on `self.overlay` /
        // `self.drawing_area`, permanently `None` under the
        // ShellApp runner (nothing assigns either field) — so
        // this was already a no-op: the backend viewport is
        // re-derived every frame by the runner itself, and
        // terminal-pane resize-on-window-resize has not fired
        // since the #540 cutover. Re-deriving live terminal
        // pane sizing needs a way to read the live DA's pixel
        // size without a widget handle — see `terminal_cols`.
        self.draw_needed.set(true);
    }

    /// Ctrl+Click — plant a secondary cursor at the clicked buffer position.
    ///
    /// The retired `Msg::CtrlMouseClick` also carried `width`/`height`, but
    /// the arm bound both to `_`, so they are dropped from the signature
    /// rather than threaded through unused.
    fn handle_ctrl_mouse_click(&mut self, x: f64, y: f64) {
        let layout_ref = self.cached_screen_layout.borrow();
        if let Some(ref layout) = *layout_ref {
            let mut engine = self.engine.borrow_mut();
            if !engine.picker_open {
                if let ClickTarget::BufferPos(_, line, col) = pixel_to_click_target(
                    &mut engine,
                    &**self.backend.borrow(),
                    x,
                    y,
                    self.cached_line_height,
                    self.cached_char_width,
                    layout,
                    &self.cached_tab_pixel_hits.borrow(),
                    self.cached_frame_hit_map.borrow().as_ref(),
                    &self.cached_tab_bar_zones.borrow(),
                    true, // real click: focus/tab/gutter side effects are intended
                ) {
                    engine.add_cursor_at_pos(line, col);
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Double-click in the editor drawing area at the given pixel position.
    ///
    /// As with [`App::handle_ctrl_mouse_click`], the `width`/`height` the
    /// retired `Msg::MouseDoubleClick` carried were bound to `_` and are
    /// dropped from the signature.
    fn handle_mouse_double_click_msg(&mut self, x: f64, y: f64) {
        // #490: a double-click landing on the editor hover popup used to fall
        // straight through to the editor's word-select underneath, because
        // this handler never consulted the popup at all. It runs the same
        // shared rung the single-click path does, first.
        if self.route_and_apply_editor_hover_popup(x, y) {
            return;
        }
        let mut engine = self.engine.borrow_mut();
        if engine.picker_open {
            let in_tree_mode = engine.picker_source
                == crate::core::engine::PickerSource::CommandCenter
                && engine.picker_query == "@";
            if in_tree_mode && engine.picker_toggle_expand() {
                engine.picker_load_preview();
            } else {
                let _action = engine.picker_confirm();
            }
            self.draw_needed.set(true);
        } else {
            // Breadcrumb double-click: same shared resolution as the
            // single-click path above (#555). This used to re-derive
            // the bar's geometry by hand — `y >= lh && y < lh * 2.0`
            // plus a per-`char_width` walk over the *active* group's
            // segments — which is pre-#540 Relm4 geometry: under the
            // ShellApp runner the breadcrumb row sits below the title
            // bar, the menu bar and a `1.6 * lh` tab row, so that band
            // never contained it (double-click was dead) while still
            // matching chrome rows that could fire the wrong segment.
            let mut bc_handled = false;
            if engine.settings.breadcrumbs {
                let lh = self.painted_line_height();
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    match render::resolve_breadcrumb_click(&layout.breadcrumbs, x, y, lh) {
                        render::BreadcrumbClickResult::Hit(group_id, idx) => {
                            drop(layout_ref);
                            engine.handle_breadcrumb_double_click(group_id, idx);
                            bc_handled = true;
                        }
                        render::BreadcrumbClickResult::OnBar => {
                            bc_handled = true;
                        }
                        render::BreadcrumbClickResult::Miss => {}
                    }
                }
            }
            if !bc_handled {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    handle_mouse_double_click(
                        &mut engine,
                        &**self.backend.borrow(),
                        x,
                        y,
                        self.cached_line_height,
                        self.cached_char_width,
                        layout,
                        &self.cached_tab_pixel_hits.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                    );
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Mouse wheel over the editor drawing area.
    ///
    /// `delta_y` arrives in **GTK's raw polarity** (positive = wheel down) —
    /// see the negation comment at the `UiEvent::Scroll` call site in
    /// `ShellApp::handle`.
    fn handle_mouse_scroll_msg(&mut self, delta_x: f64, delta_y: f64) {
        let mut engine = self.engine.borrow_mut();
        // Picker open: scroll the picker results.
        //
        // #191: previously used `(delta_y * 3.0).round()`, which
        // rounded small trackpad deltas (dy<0.17) down to 0 and
        // made scrolling feel dead. `.ceil()` on the absolute
        // value guarantees every non-zero event advances at
        // least one row, and the `5.0` amplification is closer
        // to native-app conventions for wheel notches.
        if engine.picker_open && delta_y.abs() > 0.01 {
            let step = (delta_y.abs() * 5.0).ceil() as isize;
            let delta = if delta_y > 0.0 { step } else { -step };
            engine.picker_scroll(delta, 20);
            drop(engine);
            self.draw_needed.set(true);
            return;
        }
        // Route scroll through dispatch_scroll using cached scroll surfaces.
        if let Some((px, py)) = self.last_editor_pointer.get() {
            let surfaces = engine.scroll_surfaces.borrow();
            let scroll_events = quadraui::dispatch_scroll(
                &self.backend.borrow().modal_stack_handle().borrow(),
                &surfaces,
                quadraui::Point {
                    x: px as f32,
                    y: py as f32,
                },
                quadraui::ScrollDelta::new(delta_x as f32, delta_y as f32),
            );
            drop(surfaces);
            for sev in &scroll_events {
                if let quadraui::UiEvent::Scroll {
                    widget: Some(id),
                    delta,
                    ..
                } = sev
                {
                    match id.as_str() {
                        "editor_hover" => {
                            let step = (delta.y * 3.0).round() as i32;
                            engine.editor_hover_scroll(step);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        "debug_output" => {
                            engine.handle_debug_output_scroll(delta.y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        "terminal_scrollback" => {
                            // #533: single shared scroll entry point.
                            // delta.y < 0 = up (into history); > 0 =
                            // down (toward live).  Policy + forwarding
                            // live in Engine::handle_terminal_scroll.
                            engine.handle_terminal_scroll(delta.y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
        // #240: route to the window under the pointer, falling back
        // to the active window when the pointer is missing or over
        // a non-window region. Hovering an unfocused group's pane
        // scrolls *that* pane without changing focus or moving its
        // cursor — matches TUI behaviour.
        // #646: resolve the hovered pane against the bounds
        // `render_content` actually painted with (`cached_editor_bounds`,
        // absolute coords including the activity-bar/sidebar x-offset and
        // the title-bar y-offset), not against a re-derived
        // `(0, 0, da.width(), …)` rect. `self.drawing_area` is never
        // assigned under the ShellApp runner — the runner owns the single
        // DrawingArea — so the old `if let Some(da)` arm never ran and this
        // was unconditionally `None`; and even had it run, a `(0, 0)`
        // origin is the exact coordinate-frame mismatch #582 fixed for
        // divider hit-testing.
        let hovered_window_id = self
            .last_editor_pointer
            .get()
            .zip(self.cached_editor_bounds.get())
            .and_then(|((x, y), (editor_bounds, tab_bar_height))| {
                let (rects, _) = engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
                rects
                    .iter()
                    .find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
                    .map(|(id, _)| *id)
            });
        if delta_y.abs() > 0.01 {
            let scroll_count = (delta_y * 3.0).round().abs() as usize;
            let active_id = engine.active_window_id();
            let target = hovered_window_id.unwrap_or(active_id);
            if target == active_id {
                let dir = if delta_y > 0.0 { 1 } else { -1 };
                engine.scroll_viewport_with_cursor(dir, scroll_count);
            } else {
                let dir = if delta_y > 0.0 { 1 } else { -1 };
                engine.scroll_viewport_with_cursor_for_window(target, dir, scroll_count);
            }
            engine.sync_scroll_binds();
        }
        if delta_x.abs() > 0.01 {
            let win_id = engine.active_window_id();
            let current = engine.view().scroll_left;
            let scroll_amount = (delta_x * 3.0).round() as isize;
            let new_left = (current as isize + scroll_amount).max(0) as usize;
            engine.set_scroll_left_for_window(win_id, new_left);
        }
        drop(engine);
        self.draw_needed.set(true);
    }

    /// Clear the yank highlight after the flash duration has elapsed.
    fn clear_yank_highlight(&mut self) {
        self.engine.borrow_mut().clear_yank_highlight();
        self.draw_needed.set(true);
    }

    /// `settings.json` changed on disk — reload it and, if the reload took,
    /// refresh the file tree (`show_hidden_files` may have flipped).
    fn settings_file_changed(&mut self) {
        if self.engine.borrow_mut().check_settings_reload() {
            self.refresh_file_tree();
            self.draw_needed.set(true);
        }
    }

    /// Reveal `target` in the explorer sidebar: expand all ancestors,
    /// rebuild the row list, select the matching row, scroll into view,
    /// and queue a redraw of the explorer DrawingArea. Phase A.2b-2
    /// replacement for `highlight_file_in_tree` (which operated on the
    /// native `gtk4::TreeView`).
    fn reveal_path_in_explorer(&self, target: &Path) {
        if let Ok(mut engine) = self.engine.try_borrow_mut() {
            engine.explorer_reveal_path(target);
            drop(engine);
            self.queue_explorer_draw();
        }
    }

    fn refresh_explorer(&self) {
        self.engine.borrow_mut().explorer_rebuild_rows();
        self.queue_explorer_draw();
    }

    /// Save the current session state and request a clean shutdown.
    ///
    /// Sets [`App::exit_requested`] rather than calling `process::exit`
    /// itself (#813) — `ShellApp::handle`/`tick` check the flag once they
    /// return and surface [`quadraui::Reaction::Exit`] to the runner, which
    /// tears the window down via `ReactionSink::request_exit`
    /// (`gtk/run.rs`), the same mechanism every other quadraui backend uses.
    fn save_session_and_exit(&self) {
        let mut engine = self.engine.borrow_mut();
        // GTK-only: capture the live window geometry into session state
        // *before* `save_session_state` persists it — `Engine` has no
        // window handle of its own to read this from (#823 item 5).
        engine.session.window.width = self
            .window
            .as_ref()
            .map(|w| w.win_default_width())
            .unwrap_or(800);
        engine.session.window.height = self
            .window
            .as_ref()
            .map(|w| w.win_default_height())
            .unwrap_or(600);
        engine.save_session_state();
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        drop(engine);
        self.exit_requested.set(true);
    }

    /// Dispatch an `EngineAction` produced by `handle_key`, macro playback,
    /// or a fired menu item (`handle_menu_action`, below).
    ///
    /// `is_macro`: when true, `OpenTerminal` toggles instead of creating a new
    /// tab, and dialog-open actions are suppressed (macros can't drive
    /// dialogs) — handled here, before ever reaching `apply_engine_action`,
    /// since neither is something a shared applier should know about (a
    /// menu click is never `is_macro`, so this whole branch is dead for that
    /// caller). Every other variant — the exhaustive general-purpose case —
    /// is `render::apply_engine_action` (#1063), the same function
    /// `tui_main::dispatch_post_key_action` now calls too; see that
    /// function's rung header comment in `render.rs`.
    fn dispatch_engine_action(&mut self, action: EngineAction, is_macro: bool) {
        if is_macro {
            match &action {
                EngineAction::OpenTerminal => {
                    self.toggle_terminal();
                    return;
                }
                EngineAction::OpenFolderDialog
                | EngineAction::OpenWorkspaceDialog
                | EngineAction::SaveWorkspaceAsDialog
                | EngineAction::OpenRecentDialog => return,
                _ => {}
            }
        }
        let engine_rc = self.engine.clone();
        let mut host = GtkEngineActionHost { app: self };
        render::apply_engine_action(action, &mut engine_rc.borrow_mut(), &mut host);
    }

    /// Return focus to the main editor drawing area when a sidebar loses
    /// focus.
    ///
    /// #731: was `if let Some(ref drawing) = *self.drawing_area.borrow()`
    /// — that field is permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has been a no-op since #540. Kept as
    /// a named function (rather than deleting every call site) so the
    /// intent stays legible at each of its ~10 callers; a real fix needs a
    /// live way to grab GTK keyboard focus on the editor DA from here,
    /// which nothing in this file currently has under ShellApp.
    fn focus_editor_if_needed(&self, _still_focused: bool) {}

    /// Sync the unnamed `"` register (and explicit `+` register) to the system clipboard
    /// whenever their content changes (clipboard=unnamedplus semantics).
    fn sync_plus_register_to_clipboard(&mut self) {
        let engine = self.engine.borrow();
        // Check both `"` (auto-yank) and `+` (explicit clipboard writes from plugins)
        let new_content = engine
            .registers
            .get(&'+')
            .filter(|(s, _)| !s.is_empty())
            .map(|(s, _)| s.clone())
            .or_else(|| {
                engine
                    .registers
                    .get(&'"')
                    .filter(|(s, _)| !s.is_empty())
                    .map(|(s, _)| s.clone())
            });

        if new_content != self.last_clipboard_content {
            if let (Some(ref content), Some(ref cb)) = (&new_content, &engine.clipboard_write) {
                let _ = cb(content.as_str());
            }
            drop(engine);
            self.last_clipboard_content = new_content;
        }
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::too_many_arguments)]
    /// `ctx` is threaded in solely for the shared Alt rung's
    /// [`render::AltKeyOutcome::ResizeSidebar`] arm: GTK's authoritative
    /// sidebar width *is* the runner's `AppShell` (TUI keeps its own copy and
    /// syncs it out at end of dispatch), and `ShellContext::shell_mut` is the
    /// only handle to it. `ui_event` (#815) is the raw event `key_name`/
    /// `unicode`/... were decoded from, needed by the folder-picker rung —
    /// mirrors TUI's `KeyDispatchState::ui_event`.
    fn handle_key_press(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        ui_event: &quadraui::UiEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        // ── Shared modal keyboard rung (#734 slice 1) ──────────────────
        // Bound to a local first: a `RefCell::borrow()` temporary in a `match`
        // scrutinee lives for the whole `match`, and the arms `borrow_mut()`.
        let modal_route = render::route_modal_key(&self.engine.borrow());
        match modal_route {
            render::ModalKeyRoute::Engine => {
                let action = {
                    let mut engine = self.engine.borrow_mut();
                    engine.handle_key(&key_name, unicode, ctrl)
                };
                self.dispatch_engine_action(action, false);
                self.queue_explorer_draw();
                self.draw_needed.set(true);
                return;
            }
            render::ModalKeyRoute::ContextMenu => {
                self.dispatch_context_menu_key(&key_name, unicode);
                return;
            }
            render::ModalKeyRoute::None => {}
        }

        // ── Shared folder-picker rung (#815) ────────────────────────────
        // Above every other tier: once `open_folder_dialog` (below) has
        // populated `folder_picker`, every key belongs to the picker.
        // `FolderPickerController::handle` owns the key→intent mapping
        // itself (Escape/Enter/Up/Down/k/j/-/Backspace/printable,
        // Ctrl-gated) — this rung just feeds it the raw event and applies
        // the outcome. Mirrors TUI's identical rung in `handle_key_pressed`
        // (`shell_app.rs`), same precedence relative to the modal rung above.
        if self.folder_picker.borrow().is_some() {
            self.apply_folder_picker_event(ui_event);
            self.draw_needed.set(true);
            return;
        }

        // Dismiss any panel hover popup on key press.
        self.engine.borrow_mut().dismiss_panel_hover_now();

        // ── Shared Ctrl+L force-redraw rung (#762 / #734 slice 7) ──────
        // New on GTK: there was no Ctrl+L tier here at all, so the chord fell
        // through to whichever tier came next instead of being consumed.
        if render::is_force_redraw_key(&key_name, unicode, ctrl) {
            self.draw_needed.set(true);
            return;
        }

        // ── Shared clipboard-paste pre-load rung (#760 / #734 slice 5) ─────
        // No Ctrl+Shift+V arm to converge here: quadraui's runner intercepts
        // that chord and redelivers it as `UiEvent::ClipboardPaste`.
        render::preload_paste_clipboard(&mut self.engine.borrow_mut(), &key_name, unicode, ctrl);

        // ── Shared focus-owner keyboard rung (#757 / #734 slice 2) ─────
        // GTK keeps no "the sidebar band holds the keyboard" latch of its own,
        // so it passes `Engine::sidebar_has_focus()` — the disjunction of the
        // very flags the resolver's arms test, making that gate a no-op here.
        let focus_route = {
            let engine = self.engine.borrow();
            let band = engine.sidebar_has_focus();
            render::route_focus_key(&engine, band)
        };

        if focus_route == render::FocusKeyRoute::ActivityBar {
            self.handle_activity_bar_key(&key_name, ctrl);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared terminal (PTY) keyboard rung (#758 / #734 slice 3) ──
        // Above the debug F-keys, the same slot TUI uses: a focused terminal
        // takes F5/F9/F10/F11 to the PTY, as vim/htop expect.
        if render::route_terminal_key(
            &mut self.engine.borrow_mut(),
            &key_name,
            unicode,
            ctrl,
            shift,
            alt,
        ) {
            self.sync_plus_register_to_clipboard();
            self.draw_needed.set(true);
            return;
        }

        // ── Shared debugger F-key rung (#762 / #734 slice 7) ───────────
        // Global, above the sidebar panels. The `shift` half is new here:
        // this block used to test only `!ctrl && !alt`, so Shift+F5 ran
        // *continue* instead of *stop*.
        match render::route_debug_fkey(&key_name, ctrl, shift, alt) {
            Some(render::DebugFKey::Command(cmd)) => {
                let _ = self.engine.borrow_mut().execute_command(cmd);
                self.draw_needed.set(true);
                return;
            }
            Some(render::DebugFKey::EngineKey(name)) => {
                let action = self.engine.borrow_mut().handle_key(name, None, false);
                self.dispatch_engine_action(action, false);
                self.draw_needed.set(true);
                return;
            }
            None => {}
        }

        // ── Shared focus-owner *dispatch* rung (#762 / #734 slice 7) ───
        // Slice 2 shared only the *routing*; `render::dispatch_sidebar_panel_key`
        // now states the six pure-`Engine` arms too, and TUI's
        // `handle_focus_owner_key` calls the same function (after its own
        // crossterm-spelling translation) — this is no longer GTK-only. It
        // hands back `None` for the two it cannot own — Debug needs a live
        // `Backend`, Explorer is a backend widget — which the fallback match
        // below still spells out.
        let (sc_mapped, sc_unicode) = map_gtk_key_with_unicode(key_name.as_str());
        let mapped = map_gtk_key_name(key_name.as_str());
        let panel_key = match focus_route {
            render::FocusKeyRoute::SourceControl => sc_mapped,
            _ => mapped,
        };
        let shared = render::dispatch_sidebar_panel_key(
            &mut self.engine.borrow_mut(),
            focus_route,
            panel_key,
            unicode,
            sc_unicode,
            ctrl,
            alt,
        );
        if shared.is_some() && focus_route == render::FocusKeyRoute::ExtPanel {
            self.sync_plus_register_to_clipboard();
        }
        let panel_still_focused: Option<bool> = match shared {
            Some(still_focused) => Some(still_focused),
            None => match self.dispatch_focus_owner_residual(
                focus_route,
                &key_name,
                unicode,
                ctrl,
                ui_event,
            ) {
                Some(outcome) => match outcome {
                    Some(still_focused) => Some(still_focused),
                    None => return,
                },
                None => None,
            },
        };
        if let Some(still_focused) = panel_still_focused {
            self.focus_after_sidebar_key(still_focused);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared Alt-modifier / VSCode-mode rung (#759 / #734 slice 4) ──
        // Bound to a local first, for the same reason the modal rung at the
        // top of this method is: a `RefCell::borrow_mut()` temporary in a
        // `match` scrutinee lives for the whole `match`.
        let alt_outcome = render::route_alt_key(
            &mut self.engine.borrow_mut(),
            &key_name,
            unicode,
            shift,
            alt,
        );
        match alt_outcome {
            render::AltKeyOutcome::ResizeSidebar(delta) => {
                let current = ctx.shell().sidebar_width().round().max(0.0) as u16;
                let next = render::alt_resized_sidebar_width(current, delta);
                ctx.shell_mut().set_sidebar_width(next as f32);
                self.draw_needed.set(true);
                return;
            }
            render::AltKeyOutcome::Handled => {
                self.draw_needed.set(true);
                return;
            }
            render::AltKeyOutcome::Fallthrough => {}
        }

        // ── Shared hover-popup copy rung (#762 / #734 slice 7) ─────────
        let hover_copy = render::route_hover_popup_copy(&self.engine.borrow(), &key_name, ctrl);
        if let Some(text) = hover_copy {
            let mut engine = self.engine.borrow_mut();
            if let Some(ref cb) = engine.clipboard_write {
                let _ = cb(text.as_str());
            }
            engine.message = "Hover text copied".to_string();
            drop(engine);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared command-line selection rung (#816) ──────────────────
        // The keyboard side of a command/message-line mouse selection —
        // TUI's `handle_key_pressed` has run this since #762/#734 slice 7;
        // GTK never reached it because `cmd_sel` was TUI-only local state.
        // #816 moved it onto `Engine` and wired GTK's mouse handlers (see
        // `handle_mouse_click_msg` / `handle_mouse_drag_msg`) to populate it
        // via `CommandLineLayout::hit_test`, so the same rung now applies
        // here too. `Clear` deliberately falls through to `handle_key` below.
        {
            let sel = self.engine.borrow().cmd_sel.get();
            let route =
                render::route_cmdline_selection_key(&self.engine.borrow(), unicode, ctrl, sel);
            match route {
                render::CmdSelKeyRoute::Copy(text) => {
                    let engine = self.engine.borrow();
                    if !text.is_empty() {
                        if let Some(ref cb) = engine.clipboard_write {
                            let _ = cb(text.as_str());
                        }
                    }
                    engine.cmd_sel.set(None);
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
                render::CmdSelKeyRoute::Clear => self.engine.borrow().cmd_sel.set(None),
                render::CmdSelKeyRoute::Keep => {}
            }
        }

        let action = {
            let mut engine = self.engine.borrow_mut();
            let a = engine.handle_key(&key_name, unicode, ctrl);
            // After any key press in insert mode, reset the AI completion
            // debounce timer so a new suggestion fires after idle.
            if engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
                engine.ai_completion_reset_timer();
            }
            a
        };

        self.dispatch_engine_action(action, false);
        self.draw_needed.set(true);

        // ── Shared post-key epilogue (#762 / #734 slice 7) ─────────────
        self.run_post_key_epilogue(ctx);
        self.draw_needed.set(true);
    }

    /// The Debug, Ai and Explorer halves of the focus-owner dispatch — the
    /// three arms [`render::dispatch_sidebar_panel_key`] hands back as `None`
    /// because they need state this backend owns (a live `Backend` for the
    /// DAP `SidebarSystem` and the AI `ChatController`; the explorer
    /// `DrawingArea`).
    ///
    /// `None` — not one of these three, keep dispatching.
    /// `Some(Some(still_focused))` — handled; run the focus epilogue.
    /// `Some(None)` — handled completely; the caller must return.
    fn dispatch_focus_owner_residual(
        &mut self,
        route: render::FocusKeyRoute,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        ui_event: &quadraui::UiEvent,
    ) -> Option<Option<bool>> {
        let mapped = map_gtk_key_name(key_name);
        match route {
            render::FocusKeyRoute::Debug => {
                let mut engine = self.engine.borrow_mut();
                let rect = engine.dap_sidebar_body_rect.get();
                render::populate_dap_sidebar_system(&engine);
                let consumed = if let Some(ui_event) = gtk_key_name_to_quadraui(mapped, ctrl) {
                    let backend_rc = self.backend.clone();
                    let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                        &ui_event,
                        &mut **backend_rc.borrow_mut(),
                        rect,
                    );
                    engine.dispatch_dap_sidebar_event(sidebar_event)
                } else {
                    false
                };
                if !consumed {
                    engine.dispatch_dap_sidebar_action_key(mapped);
                }
                Some(Some(engine.dap_sidebar_has_focus))
            }
            render::FocusKeyRoute::Ai => {
                // Unlike Debug (nav-only), the AI panel's `ChatController`
                // needs the real, un-round-tripped `UiEvent` — `KeyPressed`
                // *or* `CharTyped` — so free-form typed text reaches its
                // input buffer; `gtk_key_name_to_quadraui`'s reconstruction
                // only covers a handful of named/nav keys (#819).
                let mut engine = self.engine.borrow_mut();
                let rect = engine.ai_chat_rect.get();
                let theme = render::Theme::from_name(&engine.settings.colorscheme);
                let backend_rc = self.backend.clone();
                // Re-apply the metrics `render()` painted the panel with —
                // see `cached_ai_chat_metrics`'s doc for why this can't be
                // skipped.
                let metrics = self.cached_ai_chat_metrics.get();
                {
                    let mut b = backend_rc.borrow_mut();
                    b.set_current_line_height(metrics.0);
                    b.set_current_char_width(metrics.1);
                }
                let still_focused = render::route_ai_chat_event(
                    &mut engine,
                    ui_event,
                    rect,
                    &theme,
                    &mut **backend_rc.borrow_mut(),
                );
                Some(Some(still_focused))
            }
            render::FocusKeyRoute::Explorer => {
                // Explorer keys used to be routed through a per-DrawingArea
                // key controller when the DA had focus (#732 retired the
                // `Msg` variant it sent; nothing has produced it since #540).
                self.handle_explorer_da_key(mapped.to_string(), unicode, ctrl);
                self.draw_needed.set(true);
                Some(None)
            }
            _ => None,
        }
    }

    /// Reconcile the runner's own `AppShell` (`ctx.shell()`/`ctx.shell_mut()`)
    /// with `engine.app_shell`'s (the "shadow" copy's) current sidebar
    /// visibility, pushing `show_panel`/`hide_sidebar` through `ctx` if the
    /// two have drifted.
    ///
    /// #1057: extracted out of [`Self::run_post_key_epilogue`] (its
    /// original, and until now only, caller — see that method's own doc for
    /// *why* the two copies need reconciling at all) so
    /// [`Self::on_shell_event_ctx`] can call it too. That second call site
    /// exists because of a gap this issue's fix exposed: `AppShell` never
    /// runs its own toggle for a *bottom* item (`BottomItemClicked`) — it
    /// only ever reports the click, both directions — so
    /// `on_shell_event`'s `BottomItemClicked` arm is 100% responsible for
    /// deciding the new visibility, entirely inside `engine.app_shell`. Left
    /// unsynced, the runner's own `AppShell` (which is what actually
    /// determines whether `render_content`'s sidebar column exists in the
    /// composited frame — `engine.app_shell` only decides *which panel's
    /// content* to paint inside it) never learns the sidebar closed: a
    /// second Settings click flipped `engine.app_shell.sidebar_visible()` to
    /// `false` correctly, but the runner kept laying out and painting the
    /// sidebar as if nothing had changed. Verified directly: before this
    /// method gained its `on_shell_event_ctx` call site, `bottom_item_
    /// second_click_collapses_sidebar`'s `gtk`/`tui` arms in `src/harness.rs`
    /// went red at the second-click assertion — the engine-side state
    /// (`sidebar_visible()`) was already correct, only the paint wasn't.
    fn sync_runner_sidebar_visibility(&self, ctx: &quadraui::ShellContext<'_>) {
        let shadow_visible = self.engine.borrow().app_shell.sidebar_visible();
        if ctx.shell().sidebar_visible() != shadow_visible {
            if shadow_visible {
                if let Some(id) = self.engine.borrow().app_shell.active_panel_id().cloned() {
                    ctx.shell_mut().show_panel(&id);
                }
            } else {
                ctx.shell_mut().hide_sidebar();
            }
        }
    }

    /// GTK's half of the shared after-every-editor-keypress epilogue.
    /// [`render::post_key_epilogue`] applies everything `Engine` owns; this
    /// applies the residues that need GTK: macro playback (whose
    /// `EngineAction`s only `dispatch_engine_action` can run), the runner ↔
    /// shadow sidebar-visibility sync (below), the deferred clipboard write,
    /// and the GLib one-shot behind the yank highlight.
    ///
    /// `ctx` is threaded in for that visibility sync: `render::post_key_epilogue`'s
    /// autohide arm calls `engine.app_shell.hide_sidebar()`, but
    /// `engine.app_shell` is only the "shadow" copy of shell state — GTK's
    /// actual painted layout (whether the sidebar column exists at all) comes
    /// from the runner's own `AppShell`, reachable solely through
    /// `ShellContext::shell_mut` (see `handle_key_press`'s doc comment on
    /// `ctx`, and TUI's identical shadow/runner split documented on
    /// `TuiShellApp::on_shell_event`). Without pushing the change through,
    /// `should_autohide_sidebar` flips a flag nothing paints from and the
    /// sidebar visually stays open.
    fn run_post_key_epilogue(&mut self, ctx: &quadraui::ShellContext<'_>) {
        loop {
            let (has_more, action) = {
                let mut engine = self.engine.borrow_mut();
                engine.advance_macro_playback()
            };
            self.dispatch_engine_action(action, true);
            if !has_more {
                break;
            }
        }

        // GTK recomputes the quickfix scroll offset statelessly each frame
        // (`draw_bottom_chrome`), so it hands the rung no scroll field.
        let epilogue = render::post_key_epilogue(&mut self.engine.borrow_mut(), None);
        if epilogue.focus_sidebar {
            let current = self.current_active_panel_id();
            let panel_id = if is_ext_panel_id(&current) {
                PANEL_EXPLORER.to_string()
            } else {
                current
            };
            self.engine.borrow_mut().focus_sidebar_panel(&panel_id);
            self.sync_sidebar_from_engine();
        } else if epilogue.focus_activity_bar {
            // New on GTK (#762): the overflow arm used to call
            // `focus_sidebar_panel` unconditionally, so with no sidebar
            // visible the keypress went nowhere. The shared rung has already
            // put the cursor on the activity bar; this just re-syncs.
            self.sync_sidebar_from_engine();
        }

        // Sync the unnamed register to the system clipboard if it changed.
        // The comparison is O(1); actual write is deferred to the background thread.
        self.sync_plus_register_to_clipboard();

        // ── Runner ↔ shadow sidebar-visibility sync (#762) ──────────────
        // The only place above that flips *visibility* (as opposed to which
        // panel/focus owns an already-visible sidebar) is the autohide arm
        // inside `render::post_key_epilogue`, which has no field of its own
        // to report through — so this just reconciles the two copies
        // unconditionally, the same way TUI's `on_shell_event` tail does.
        self.sync_runner_sidebar_visibility(ctx);

        // If a yank just happened, arm a 200 ms deadline; `tick_dispatch`
        // polls it and clears the highlight once it elapses (#813 — ported
        // off a one-shot toolkit timer, mirrors TUI's `yank_hl_deadline` in
        // `tui_main/shell_app.rs`).
        if epilogue.arm_yank_highlight {
            self.yank_hl_deadline.set(Some(
                std::time::Instant::now() + std::time::Duration::from_millis(200),
            ));
        }
    }

    fn handle_poll_tick(&mut self) {
        // Reload CSS if the colorscheme changed (e.g. via :colorscheme command).
        {
            let current = self.engine.borrow().settings.colorscheme.clone();
            if current != self.last_colorscheme {
                let theme = Theme::from_name(&current);
                let combined = format!("{STATIC_CSS}\n{}", make_theme_css(&theme));
                if let Some(p) = &self.css_provider {
                    p.load_css_data(&combined);
                }
                // Update GTK dark/light preference for native widgets & menus.
                #[cfg(feature = "gui")]
                if let Some(gtk_settings) = gtk4::Settings::default() {
                    gtk_settings.set_gtk_application_prefer_dark_theme(!theme.is_light());
                }
                self.last_colorscheme = current;
                self.draw_needed.set(true);
            }
        }

        // #949: reload settings.json if it changed on disk. This used to be
        // driven by a GTK-only `gio::FileMonitor` that sent a
        // `DeferredAction::SettingsFileChanged` on a native file-change
        // event; that watcher is gone, and `check_settings_reload`'s
        // portable mtime poll (inside `settings_file_changed`) now runs
        // unconditionally every tick instead — the same mechanism TUI's own
        // `tick` has always used. quadraui's GTK/macOS idle-poll fallback
        // ceiling is 250ms (`runner.rs`'s `ShellApp::tick` doc), so the
        // reload lag here matches what TUI already ships, not a regression
        // from the watcher's near-immediate `ChangesDoneHint`.
        self.settings_file_changed();

        // #731: a ~135-line block used to live here polling
        // `self.mouse_pos_cell` at 20Hz for four distinct hover features —
        // h-scrollbar hover, tab-close (×) hover + tab tooltip, debug
        // toolbar button hover, and LSP hover-on-dwell popups
        // (`Engine::editor_hover_mouse_move`). All four were gated on a
        // `da_size` derived from `self.drawing_area`, permanently `None`
        // under the ShellApp runner (nothing assigns it) — so none of the
        // four have worked since the #540 cutover, and nothing else in
        // this file writes `h_sb_hovered`/`tab_close_hover`/
        // `debug_button_hovered`/calls `editor_hover_mouse_move`. This is
        // the single biggest confirmed-dead surface this issue found (see
        // the PR description) — restoring it needs a live, correctly
        // absolute-coordinate DA size (the removed code used a `(0, 0)`
        // origin the neighboring comment already flagged as the #582/#646
        // coordinate-frame bug, so it was not simply "wire the same code
        // back up"), which is follow-up work, not a dead-code deletion.
        //
        // Sync per-window viewport dimensions from the paint-time ScreenLayout
        // so ensure_cursor_visible uses exact geometry.  This block is outside
        // the `da_size` guard because `cached_screen_layout` is populated by
        // render_content() regardless of whether `self.drawing_area` is set —
        // which it is not under the quadraui ShellApp runner (the runner owns
        // the single DrawingArea, not vimcode).
        {
            let layout_ref = self.cached_screen_layout.borrow();
            if let Some(ref layout) = *layout_ref {
                let mut engine = self.engine.borrow_mut();
                for rw in &layout.windows {
                    engine.set_viewport_for_window(
                        rw.window_id,
                        rw.lines.len().max(1),
                        rw.text_viewport_cols.max(1),
                    );
                }
            }
        }

        // Run all periodic background work (LSP, DAP, terminal, search, etc.)
        // poll_idle() consumes dap_wants_sidebar internally.
        let idle_dirty = self.engine.borrow_mut().poll_idle();
        if idle_dirty {
            self.sync_sidebar_from_engine();
        }
        // Format-on-save + :wq/:x deferred quit
        if self.engine.borrow().format_save_quit_ready {
            self.engine.borrow_mut().format_save_quit_ready = false;
            self.quit_confirmed();
        }
        // Run pending terminal commands (needs backend-supplied terminal size).
        if self.engine.borrow().pending_terminal_command.is_some() {
            let cmd = self
                .engine
                .borrow_mut()
                .pending_terminal_command
                .take()
                .unwrap();
            self.run_command_in_terminal(cmd);
        }
        let active_panel = self.current_active_panel_id();
        // Explorer refresh after confirmed file move.
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }
        // Auto-refresh SC panel periodically (gated on sidebar visibility).
        if self.current_sidebar_visible()
            && (active_panel == PANEL_GIT || active_panel == PANEL_EXPLORER)
            && self.last_sc_refresh.elapsed() >= std::time::Duration::from_secs(2)
        {
            self.engine.borrow_mut().sc_refresh_async();
            self.last_sc_refresh = std::time::Instant::now();
        }
        if self.engine.borrow_mut().poll_sc_refresh() {
            self.draw_needed.set(true);
        }
        // Check for panel reveal request from plugins.
        // Extract into a separate binding so the RefMut drops before the
        // re-borrows inside the body (Rust 2021 temporary lifetime rule).
        let pending_panel = self.engine.borrow_mut().ext_panel_focus_pending.take();
        if let Some(panel_name) = pending_panel {
            {
                let mut engine = self.engine.borrow_mut();
                if !engine.app_shell.sidebar_visible() {
                    engine.app_shell.toggle_sidebar();
                }
                engine.ext_panel_has_focus = true;
                engine.ext_panel_active = Some(panel_name);
            }
            self.sync_sidebar_widgets();
        }
        // Sync the OS window title with the active buffer name (taskbar/pager).
        let win_title = self
            .engine
            .borrow()
            .active_buffer_name()
            .map(|n| format!("VimCode \u{2014} {}", n))
            .unwrap_or_else(|| "VimCode".to_string());
        if let Some(ref w) = self.window {
            w.win_set_title(&win_title);
        }
    }

    /// Map a pixel x-offset within the editor hover popup's content
    /// area to a character column on `content_line`, using Pango to
    /// measure proportional UI-font widths (#218). The legacy code
    /// did `(rel_x / cached_char_width)` which drifts as the column
    /// index grows because UI_FONT is proportional. Heading rows
    /// (font scale > 1.0) need the scale applied to the layout so
    /// `xy_to_index` returns the right position.
    ///
    /// #731: the Pango-measured path below was gated on
    /// `self.drawing_area`, permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has always taken the approximate
    /// `rel_x / char-width`-style fallback in practice — see `terminal_cols`
    /// for the same "no live font-metrics source without a widget handle"
    /// root cause.
    /// Run the shared editor-hover-popup rung (#755) against this frame's
    /// painted geometry and apply whatever it decides.
    ///
    /// Returns `true` when the press belonged to the popup and must not fall
    /// through to the editor. Called from `handle_mouse_click_msg` **above**
    /// the scroll-surface dispatch — this backend used to run its bespoke
    /// copy ~90 lines *below* it, which is why a click aimed at the popup's
    /// own scrollbar was swallowed by the surface painted behind it
    /// (#229/#486) — and from `handle_mouse_double_click_msg`, which never
    /// consulted the popup at all, so a double-click on it fell through to
    /// the editor's word-select (#490).
    fn route_and_apply_editor_hover_popup(&self, x: f64, y: f64) -> bool {
        let (visible, has_focus) = {
            let engine = self.engine.borrow();
            (engine.editor_hover.is_some(), engine.editor_hover_has_focus)
        };
        let links = self.editor_hover_link_rects.borrow();
        let route = render::route_editor_hover_popup_click(
            visible,
            &render::EditorHoverPopupState {
                popup: self.editor_hover_popup_rect.get(),
                links: &links,
                scrollbar: self.editor_hover_scrollbar.get(),
                has_focus,
                // `draw_editor_hover_popup` insets its text by 4px on both
                // axes; the content grid is the editor's own cell size.
                content: render::PopupContentMetrics {
                    pad_x: 4.0,
                    pad_y: 4.0,
                    col_width: self.cached_char_width.max(1.0) as f32,
                    line_height: self.cached_line_height.max(1.0) as f32,
                },
            },
            x,
            y,
        );
        let effect = render::apply_editor_hover_popup_route(&mut self.engine.borrow_mut(), route);
        if let Some(url) = effect.open_url {
            open_url(&url);
        }
        if let Some(target) = effect.begin_drag {
            let drag_rc = self.backend.borrow().drag_state_handle();
            drag_rc.borrow_mut().begin(target);
            // Seek immediately, with the same thumb-aware math the drag
            // frames will use. This backend used to run a *second*,
            // ratio-based calculation at press time, so the thumb jumped
            // once on press and again on the first drag frame.
            let drag = drag_rc.borrow().clone();
            for ev in quadraui::dispatch_mouse_drag(
                &drag,
                quadraui::Point {
                    x: x as f32,
                    y: y as f32,
                },
                Default::default(),
            ) {
                if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                    if widget.as_str() == "editor_hover" {
                        self.engine.borrow_mut().editor_hover_set_scroll(new_offset);
                    }
                }
            }
        }
        self.draw_needed.set(true);
        effect.consumed
    }

    /// Run the shared panel-hover-popup click rung (#1067) against this
    /// frame's painted link rects.
    ///
    /// Before #1067 this backend painted and cached `panel_hover_link_rects`
    /// (`render::panel_hover_popup_paint`, called from `render_content`) but
    /// never read them back on click — clicking a link in the source-control
    /// / extension-panel item dwell tooltip was a complete no-op on GTK,
    /// where TUI already copied the URL (or ran the `command:` link) via its
    /// own inline hit test in `mouse::handle_mouse`. `panel_hover_popup_paint`'s
    /// own doc traces this back to the #540 Relm4->ShellApp migration
    /// retiring `Msg::PanelHoverClick` without a replacement.
    ///
    /// Returns `true` when the press landed on a link and must not fall
    /// through to whatever is painted underneath.
    fn route_and_apply_panel_hover_popup(&self, x: f64, y: f64) -> bool {
        let links = self.panel_hover_link_rects.borrow();
        let route = render::route_panel_hover_popup_click(&links, x, y);
        drop(links);
        if route == render::PanelHoverPopupRoute::None {
            return false;
        }
        let effect = render::apply_panel_hover_popup_route(&mut self.engine.borrow_mut(), route);
        if let Some(url) = effect.open_url {
            open_url(&url);
        }
        self.draw_needed.set(true);
        effect.consumed
    }

    /// Popup-content column under `rel_x` (pixels from the content origin).
    ///
    /// Used by the hover-selection *drag* follow-through; the press itself
    /// goes through `render::route_editor_hover_popup_click`, which divides by
    /// the same `col_width`. Before #755 this returned `rel_x as usize` — a
    /// column per *pixel* — so a drag-selection inside the popup ran off the
    /// end of the line on the first few pixels of travel and never agreed
    /// with the column the press had chosen.
    fn pixel_to_editor_hover_col(&self, rel_x: f64, _content_line: usize) -> usize {
        (rel_x.max(0.0) / self.cached_char_width.max(1.0)) as usize
    }

    /// Push or pop the editor hover popup on the modal stack so
    /// click dispatch can decide modal-vs-base for both left- and
    /// right-clicks (#216). The popup is registered whenever it's
    /// visible (focused or not) so right-clicks anywhere inside it
    /// stop falling through to the editor's context menu. Picker-
    /// style reconcile: `push` dedupes on id, so calling this every
    /// click is safe.
    fn reconcile_editor_hover_modal(&self) {
        let editor_hover_id = quadraui::WidgetId::new("editor_hover");
        let engine = self.engine.borrow();
        let visible = engine.editor_hover.is_some();
        let rect = self.editor_hover_popup_rect.get();
        drop(engine);
        let stack_rc = self.backend.borrow().modal_stack_handle();
        let mut stack = stack_rc.borrow_mut();
        match (visible, rect) {
            (true, Some(rect)) => {
                stack.push(editor_hover_id, rect);
            }
            _ => {
                stack.pop(&editor_hover_id);
            }
        }
    }

    /// Route a left-click against the currently open engine-drawn context
    /// menu — `engine.context_menu` is shared by the editor, tab-bar, and
    /// explorer sources, so this applies uniformly regardless of which one
    /// opened it. Mirrors the modal-stack arbitration
    /// (`quadraui::dispatch_mouse_down` for outside-click dismissal,
    /// `ContextMenuLayout::hit_test` for inner row resolution) that used to
    /// live inline in `handle_mouse_click_msg` (Phase B.5b Stage 4).
    ///
    /// Returns `true` iff a menu was open and this call consumed the click
    /// (dismissed it, fired an item, or kept it open on an inert row) — the
    /// caller should treat that as "handled, stop routing". Returns `false`
    /// when no menu was open, after defensively popping any stale
    /// modal-stack entry left by an Esc/Enter close the click handler never
    /// saw; the caller should then proceed with its own routing.
    ///
    /// Callable from both `handle_mouse_click_msg` (main-content clicks) and
    /// `try_route_sidebar_mouse_event` (#546 FAILED-2: an explorer-sourced
    /// menu typically renders inside the sidebar's own content bounds, so
    /// without giving it priority there too, clicks on it fell straight
    /// through to `TreeController`'s row hit-test underneath instead of
    /// firing the menu action or dismissing it).
    fn dispatch_context_menu_click(&mut self, x: f64, y: f64) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        if self.engine.borrow().context_menu.is_none() {
            // Defensive cleanup: the menu may have closed via Esc/Enter while
            // no click was seen by us. Pop any stale entry.
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&cm_id);
            return false;
        }

        // Keep the menu's painted bounds on the modal stack so any other modal
        // that might be open (picker, dialog) is arbitrated against it by the
        // *drag* guard, which still consults the stack.
        if let Some(bounds) = self.context_menu_layout.borrow().as_ref().map(|l| l.bounds) {
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .push(cm_id, bounds);
        }

        match self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress) {
            render::ModalOverlayRoute::ContextMenu(route) => {
                self.apply_context_menu_route(route);
            }
            // A dialog or a toast outranks the menu; the shared router already
            // said so, and re-deciding that here is what let the two backends
            // drift in the first place.
            _ => self.draw_needed.set(true),
        }
        true
    }

    /// Editor content bounds + tab-bar height **as last painted**, in the
    /// absolute DA coordinate frame mouse events arrive in (#582).
    ///
    /// Divider hit-testing must run against the geometry the renderer used, not
    /// a parallel re-derivation: `render_content` anchors `editor_bounds` at
    /// `AppShellLayout::main_content_bounds` (offset right by the activity
    /// bar/sidebar, down by the title-bar band), so any handler that rebuilt
    /// bounds at `(0.0, 0.0)` hit-tested a phantom divider displaced by that
    /// offset — the `:vsplit` failure in #582.
    ///
    /// `None` only before the first frame has been painted, when there is no
    /// divider on screen to hit anyway.
    fn painted_editor_bounds(&self) -> Option<(core::WindowRect, f64)> {
        self.cached_editor_bounds.get()
    }

    /// Left edge of the bottom panel as last painted — the same `x`
    /// `render_content` hands `draw_tab_bar` / the terminal pane, i.e. the
    /// editor's left edge, right of the activity bar and sidebar.
    ///
    /// [`render::BottomPanelMetrics::panel_left`] (#754). Falls back to `0.0`
    /// before the first frame, when there is no panel on screen to click.
    fn painted_bottom_panel_left(&self) -> f64 {
        self.cached_editor_bounds
            .get()
            .map(|(r, _)| r.x)
            .unwrap_or(0.0)
    }

    /// Both divider lists for the frame just painted, plus whether `(x, y)`
    /// lands on a group's tab bar — everything
    /// [`render::route_divider_grab`] needs from this backend.
    ///
    /// Derived from [`Self::painted_editor_bounds`] rather than a fresh
    /// drawing-area measurement, for the #582 reason recorded there.
    /// #753 named it because the click arm and the drag arm both needed it and
    /// each used to re-derive its own half.
    ///
    /// `on_tab_bar` exists so a click on a group's tab bar reaches the tab
    /// handlers instead of arming a group-divider drag; it is deliberately
    /// GTK-only (see `render::DividerState::on_tab_bar`). It is also skipped
    /// entirely in single-group mode, where `group_dividers` is empty and
    /// nothing could match anyway.
    fn painted_divider_geometry(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(
        Vec<core::window::GroupDivider>,
        Vec<core::window::WindowDivider>,
        bool,
    )> {
        let (content_bounds, tab_bar_h) = self.painted_editor_bounds()?;
        let engine = self.engine.borrow();
        let single = engine.group_layout.is_single_group();
        let group_dividers = if single {
            Vec::new()
        } else {
            engine.group_layout.dividers(content_bounds, &mut 0)
        };
        let on_tab_bar = !single
            && engine
                .group_layout
                .calculate_group_rects(content_bounds, tab_bar_h)
                .iter()
                .any(|(gid, grect)| {
                    if engine.is_tab_bar_hidden(*gid) {
                        return false;
                    }
                    let ty = grect.y - tab_bar_h;
                    y >= ty && y < ty + tab_bar_h && x >= grect.x && x < grect.x + grect.width
                });
        let (window_rects, _) = engine.calculate_group_window_rects(content_bounds, tab_bar_h);
        let window_dividers = engine.calculate_window_dividers(&window_rects);
        Some((group_dividers, window_dividers, on_tab_bar))
    }

    /// The [`render::EditorOp::Windows`] rung: paint every editor window's
    /// text plus its per-window status line, then the `:split`/`:vsplit`
    /// divider lines *within* each group.
    ///
    /// `window_editors` collects each window's owned `quadraui::Editor` for
    /// the caller's `FrameHitMap` (#449), so the map hit-tests the SAME
    /// objects that were painted rather than a second copy that could drift.
    ///
    /// TUI's twin is `render_impl::render_all_windows`, which also paints its
    /// within-group separators (`render_separators`) from the same rung.
    fn paint_editor_windows_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        lh: f64,
        window_editors: &mut Vec<quadraui::Editor>,
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};
        for rw in &screen.windows {
            let editor = render::to_q_editor(rw);
            let rect = editor.rect;
            let mut frame = QSL::new();
            frame.push(Surface::Editor {
                rect,
                editor: &editor,
            });
            frame.draw(backend);
            window_editors.push(editor);

            // Per-window status bar (when `window_status_line` is true, which
            // is the default; `global_status_bar` is None in that mode).
            let Some(ref status) = rw.status_line else {
                continue;
            };
            let bar_y = rw.rect.y + rw.rect.height - lh;
            let sb_rect = quadraui::Rect::new(
                rw.rect.x as f32,
                bar_y as f32,
                rw.rect.width as f32,
                lh as f32,
            );
            let win_bar = render::window_status_line_to_status_bar(
                status,
                quadraui::WidgetId::new(format!("status:{}", rw.window_id.0)),
            );
            // #672: recover segment hit zones the same way the dead
            // `draw.rs::draw_window_status_bar` did, so `pixel_to_click_target`'s
            // `WindowZone::StatusBar` arm has a real `status_segment_map` entry
            // to resolve against instead of an always-empty one.
            // `draw_status_bar` lays segments out bar-relative from `(0, 0)`
            // regardless of `sb_rect`'s own origin (see
            // `route_debug_sidebar_event`'s doc comment for the same
            // "`StatusBar::layout` always starts at 0,0" contract), which is
            // exactly the window-relative `local_x` `window_zone_hit_test`
            // hit-tests with — no coordinate translation needed.
            //
            // #764: this is the layout the *paint* resolved, not a second
            // `status_bar_layout` re-measure of the same bar as it used to be —
            // same reasoning as `render::PaintedTabBar::hits`.
            let sb_layout = backend.draw_status_bar(sb_rect, &win_bar, None, None);
            self.status_segment_map.borrow_mut().insert(
                rw.window_id.0,
                render::status_bar_zones_from_layout(&sb_layout),
            );
        }

        // `:split`/`:vsplit` boundaries had no visual of their own in GTK
        // before #582 — nothing told the user where to grab. Painted via
        // quadraui's `Split` primitive rather than hand-rolled Cairo. Both
        // axes: the #582 iteration-2 smoke found `:split` only *seemed*
        // draggable because the per-window status bar happens to sit one line
        // above the boundary and reads as a divider, which is a coincidence of
        // an unrelated feature, not a handle.
        //
        // These are the *within*-group dividers, hence part of this rung
        // rather than `EditorOp::GroupDividers` — the same split TUI makes,
        // where `render_all_windows` paints them via `render_separators`.
        render::draw_dividers_as_splits(backend, &screen.window_dividers, |div| {
            quadraui::WidgetId::new(format!("wdiv:{}:{}", div.group_id.0, div.split_index))
        });
    }

    /// The [`render::EditorOp::TabBars`] rung: paint one tab bar per editor
    /// group and recover the pixel hit geometry the rasteriser resolved.
    ///
    /// Multi-group (post-split) layouts get a bar per group at the top edge of
    /// its own bounds; a single group is a split of one and gets one
    /// full-width bar at the editor top (#515/#551).
    ///
    /// Painting goes through `render::paint_tab_bars` →
    /// `Backend::draw_tab_bar_icons` rather than a `Surface::TabBar` push,
    /// because quadraui's `Surface` enum carries no icon sidecar (adding a
    /// field to it would be the same hard break on downstream consumers that
    /// kept the icons off `TabItem` in the first place). With an empty sidecar
    /// the two are byte-identical — quadraui's `draw_tab_bar` forwards to
    /// `draw_tab_bar_icons` with `&[]` — so this is a pure superset of the old
    /// call (#703). `hit_bars` still collects a `Surface::TabBar` for the
    /// caller's `FrameHitMap`: that map is only ever consumed via `hit_map()`
    /// (never drawn) and its zones are whole-bar rects, which icons do not
    /// move.
    /// Compose the [`render::FrameOp::SidebarPanel`] rung: the *active panel's
    /// body*, into the content rect `AppShell` reserved for it.
    ///
    /// Extracted out of `render_content`'s walk (#766) because it was 210 of
    /// the walk's lines on its own — a `match` over seven panel ids, each with
    /// its own hit-test-cache publication — and #766's whole point is that
    /// `render_content` reads as the frame's *order*, not as the frame's
    /// contents. The surrounding sidebar chrome (activity bar, header,
    /// separator) is quadraui's, painted by the runner before `render_content`
    /// is entered; this fills only `q_sb`.
    #[allow(clippy::too_many_arguments)]
    fn paint_sidebar_panel_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &render::ScreenLayout,
        theme: &Theme,
        q_sb: quadraui::Rect,
        lh: f64,
        cw: f64,
    ) {
        // Which panel is active? #823 item 7: was its own restatement of
        // `render::sidebar_owner`'s resolution.
        let active_id: String = render::sidebar_owner(engine).panel_id_string();

        match active_id.as_str() {
            PANEL_EXPLORER => {
                render::populate_explorer_tree_controller(engine, theme);
                // Capture the exact metrics the tree is drawn with so the
                // click hit-test (which reads the backend's mutable
                // current_line_height at a later, possibly-different time) can
                // re-apply them and resolve the correct row. (#540)
                self.cached_explorer_metrics
                    .set((backend.line_height() as f64, backend.char_width() as f64));
                engine.explorer_tree_rect.set(q_sb);
                engine.explorer_viewport_rows.set(q_sb.height as usize);
                engine.explorer_tree.borrow().render(backend, q_sb);
            }
            PANEL_SEARCH => {
                // #1065: `search_sidebar_system` never had `set_backend_info`
                // called on this backend — the exact #971 gap
                // (`gui_sidebar_system_metrics`'s own doc) that left
                // `SidebarSystem::handle_cached` returning
                // `SidebarEvent::Ignored` unconditionally, fixed for
                // `sc_sidebar_system` (this match's `PANEL_GIT` arm) and
                // `ext_sidebar_system` (`refresh_ext_sidebar_metrics`) but
                // missed here. Every content-row press *and* every wheel
                // notch over the search results list silently no-op'd —
                // `search_panel_click_focuses_the_query_field`'s click landed
                // on the query text box, a separate hit-test that never goes
                // through `handle_cached`, so it never caught this.
                let search_lh = backend.line_height();
                engine
                    .search_sidebar_system
                    .borrow_mut()
                    .set_backend_info(search_lh, render::gui_sidebar_system_metrics(search_lh));
                render::populate_search_sidebar_system(engine, &engine.cwd);
                engine.search_sidebar_body_rect.set(q_sb);
                engine.search_sidebar_system.borrow().render(backend, q_sb);
            }
            PANEL_DEBUG => {
                let (title_bar, action_bar) =
                    render::debug_sidebar_chrome_to_status_bars(&screen.debug_sidebar, theme);
                let title_rect = quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, lh as f32);
                let action_rect =
                    quadraui::Rect::new(q_sb.x, q_sb.y + lh as f32, q_sb.width, lh as f32);
                let body_y = q_sb.y + 2.0 * lh as f32;
                let body_h = (q_sb.height - 2.0 * lh as f32).max(0.0);
                let body_rect = quadraui::Rect::new(q_sb.x, body_y, q_sb.width, body_h);
                let _ = backend.draw_status_bar(title_rect, &title_bar, None, None);
                let hits = backend.draw_status_bar(action_rect, &action_bar, None, None);
                engine.dap_sidebar_action_hits.replace(Some(hits));
                // `hits` are relative to `action_rect`'s origin; the click
                // router needs the rect to translate into that space (#544).
                self.cached_dap_action_rect.set(Some(action_rect));
                engine.dap_sidebar_body_rect.set(body_rect);
                render::populate_dap_sidebar_system(engine);
                engine
                    .dap_sidebar_system
                    .borrow()
                    .render(backend, body_rect);
            }
            PANEL_GIT => {
                if let Some(ref sc) = screen.source_control {
                    // Header row + commit-input box (#480). Previously
                    // entirely unpainted under ShellApp — the only place
                    // that ever drew them was the dead
                    // `draw.rs::draw_source_control_panel` Cairo painter,
                    // which has zero live callers (superseded by this
                    // `render_content` path back when the 14 legacy DAs
                    // were collapsed into one, #493). Paint them for
                    // real now that quadraui#222 (TextInput) has landed,
                    // through the same `render::sc_*` adapters TUI uses
                    // so the two renderers can't drift.
                    // Band geometry (header / commit box / slab) comes from
                    // the shared `render::sc_sidebar_bands` so the click
                    // router in `try_route_sidebar_mouse_event` resolves a
                    // press against the *same* derivation that painted it
                    // (#544). `SC_COMMIT_BORDER_PX` is the primitive's 1px
                    // border top+bottom — GTK's native unit is pixels,
                    // unlike TUI's whole-cell border (see
                    // `render::sc_commit_input_box_height` doc).
                    let bands = render::sc_sidebar_bands(
                        &sc.commit_message,
                        q_sb,
                        lh as f32,
                        SC_COMMIT_BORDER_PX,
                    );
                    self.cached_sc_bands.set(Some(bands));
                    let header_bar = render::sc_header_status_bar(sc, theme);
                    let _ = backend.draw_status_bar(bands.header, &header_bar, None, None);

                    let ti = render::sc_commit_message_to_text_input(sc);
                    backend.draw_text_input(bands.commit_input, &ti);

                    // Render the toolbar-slab + section list below the
                    // header + commit input.
                    let slab_rect = bands.slab;
                    render::draw_sc_sidebar_panel(backend, engine, sc, slab_rect);
                    let body_rect = engine
                        .sc_panel_layout
                        .borrow()
                        .as_ref()
                        .map(|l| l.content_bounds)
                        .unwrap_or(slab_rect);
                    engine.sc_sidebar_body_rect.set(body_rect);
                    // #971: without this, `sc_sidebar_system.handle_cached`
                    // returns `Ignored` unconditionally and every
                    // content-row press (header collapse, row select) is a
                    // silent no-op — see `render::gui_sidebar_system_metrics`'s
                    // own doc for the full story. Reads `backend.line_height()`
                    // directly — not the `lh` parameter above, whose
                    // `self.cached_line_height.max(backend.line_height())`
                    // derivation (`render_content`'s own top) can lag behind
                    // what `backend` reports by the time `render()` a few
                    // lines down actually reads it — so the metrics
                    // `handle_cached` hit-tests against can never disagree
                    // with what this exact `render()` call paints.
                    let sc_lh = backend.line_height();
                    engine
                        .sc_sidebar_system
                        .borrow_mut()
                        .set_backend_info(sc_lh, render::gui_sidebar_system_metrics(sc_lh));
                    render::populate_sc_sidebar_system(engine, theme);
                    engine.sc_sidebar_system.borrow().render(backend, body_rect);

                    // Branch picker / create popup (dual-mode Palette,
                    // quadraui#224) and help dialog (Dialog + DialogTable,
                    // quadraui#225) — both keyboard-reachable via
                    // `dispatch_sc_sidebar_key_unified` even though the
                    // git sidebar has no live mouse-click routing yet
                    // (#449 tracks that separately). Render over the
                    // whole sidebar content area, same popup-over-panel
                    // z-order TUI uses.
                    if let Some(ref bp) = sc.branch_picker {
                        let palette = render::sc_branch_picker_to_palette(bp);
                        let popup_w = q_sb.width.min(40.0 * cw as f32);
                        let popup_h = if bp.create_mode {
                            4.0 * lh as f32
                        } else {
                            (q_sb.height * 0.6).min(15.0 * lh as f32)
                        };
                        let popup_x = q_sb.x + (q_sb.width - popup_w) / 2.0;
                        let popup_y = q_sb.y + 2.0 * lh as f32;
                        backend.draw_palette(
                            quadraui::Rect::new(popup_x, popup_y, popup_w, popup_h),
                            &palette,
                        );
                    }

                    if sc.help_open {
                        let viewport = q_sb;
                        let (dialog, dlayout) =
                            render::sc_help_dialog_layout(viewport, cw as f32, lh as f32);
                        backend.draw_dialog(&dialog, &dlayout);
                    }
                } else {
                    // Git panel is the active tab but there's no repo open
                    // (e.g. the user closed it, or switched to a non-git
                    // folder, without also switching sidebar tabs) — nothing
                    // paints this frame. Clear the cached band geometry so a
                    // stray click doesn't get resolved against stale
                    // coordinates from the last time a repo *was* open
                    // (`route_sc_sidebar_event` reads this cache directly).
                    self.cached_sc_bands.set(None);
                }
            }
            PANEL_EXTENSIONS => {
                Self::refresh_ext_sidebar_metrics(backend, engine);
                render::populate_ext_sidebar_system(engine);
                engine.ext_sidebar_body_rect.set(q_sb);
                engine.ext_sidebar_system.borrow().render(backend, q_sb);
            }
            PANEL_SETTINGS => {
                render::populate_settings_form_controller(engine);
                engine
                    .settings_form_controller
                    .borrow_mut()
                    .render_and_cache(backend, q_sb);
            }
            id if id.starts_with("ext:") => {
                // Extension panel — render via ext_sidebar_system.
                Self::refresh_ext_sidebar_metrics(backend, engine);
                render::populate_ext_sidebar_system(engine);
                engine.ext_sidebar_body_rect.set(q_sb);
                engine.ext_sidebar_system.borrow().render(backend, q_sb);
            }
            PANEL_AI => {
                // #819: adopts quadraui's `ChatController` — one shared
                // `render()` for both backends, like `explorer_tree` and
                // `ext_sidebar_system` above, replacing the hand-painted
                // `draw_ai_sidebar_panel`. `ai_chat_rect` is cached on
                // `Engine` (not here) so `route_ai_chat_event` re-derives
                // the identical layout `render()` painted (#544/#582/#646).
                render::populate_ai_chat_controller(engine, theme);
                engine.ai_chat_rect.set(q_sb);
                engine.ai_chat.borrow().render(backend, q_sb);
                // `cached_explorer_metrics`'s drift guard, ported: `backend`'s
                // "current" line_height/char_width are mutable and can be
                // overwritten by whatever paints next this frame or the
                // next, so capture what `render()` actually used here for
                // `route_ai_chat_event` to re-apply before `handle()` (#819).
                self.cached_ai_chat_metrics
                    .set((backend.line_height() as f64, backend.char_width() as f64));
            }
            _ => {}
        }

        // The sidebar-item hover popup used to be composed here,
        // nested inside this rung. #765 lifted it out to
        // `BottomOp::PanelHover`: as a *sidebar* rung it could
        // only run while `sidebar_content_bounds` was `Some`,
        // so collapsing the sidebar left
        // `panel_hover_popup_rect` pinned at its last painted
        // value and `handle_mouse_press` went on arbitrating
        // clicks against a popup that was no longer on screen.
    }

    /// Re-derive `ext_sidebar_system`'s backend metrics from what `backend`
    /// is about to paint with (#971).
    ///
    /// Without this, `ext_sidebar_system.handle_cached` returns `Ignored`
    /// unconditionally and every content-row press on the plugin ext panel
    /// (header collapse, row select) is a silent no-op — see
    /// `render::gui_sidebar_system_metrics`'s own doc for the full story.
    /// Reads `backend.line_height()` directly rather than accepting a
    /// cached `lh` parameter — see the `PANEL_GIT` arm's identical comment
    /// in [`Self::paint_sidebar_panel_rung`] on why: a cached value can lag
    /// behind what `backend` reports by the time `render()` actually reads
    /// it, so the metrics `handle_cached` hit-tests against could disagree
    /// with what this exact `render()` call paints. Shared by the
    /// `PANEL_EXTENSIONS` arm and the `id if id.starts_with("ext:")` arm
    /// above, which were previously two verbatim copies of this same
    /// four-line snippet.
    fn refresh_ext_sidebar_metrics(backend: &mut dyn quadraui::Backend, engine: &Engine) {
        let ext_lh = backend.line_height();
        engine
            .ext_sidebar_system
            .borrow_mut()
            .set_backend_info(ext_lh, render::gui_sidebar_system_metrics(ext_lh));
    }

    /// Compose the editor-anchored popups: completion menu, LSP hover, editor
    /// hover (rich markdown), diff peek and signature help.
    ///
    /// Extracted out of `render_content` (#766) for the same reason as
    /// [`Self::paint_sidebar_panel_rung`] — it was ~160 lines of geometry in
    /// the middle of what #766 makes a statement of the frame's *order*.
    /// Deliberately **not** a `FrameOp` rung: these are anchored to the active
    /// window's cursor rather than to a band, and both backends compose them at
    /// exactly this point (TUI through `shell_app.rs`'s `paint_editor_popups`),
    /// between the editor band and the bottom band.
    ///
    /// `main` is `AppShellLayout::main_content_bounds` — the clip viewport
    /// every popup is placed inside.
    fn paint_editor_popups_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        theme: &Theme,
        main: quadraui::Rect,
        lh: f64,
        cw: f64,
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};

        // ── Draw editor-anchored popups (on top of everything else) ────────────
        // Completion menu, LSP hover, editor hover (rich markdown), diff peek,
        // signature help. (#669) Ported from the dead `src/gtk/draw.rs` path —
        // same class of gap as the breadcrumb note above (#547): the #540
        // Relm4->ShellApp migration dropped this paint step even though the
        // engine has populated these `screen.*` fields unchanged the whole
        // time. Content comes from the same shared `render::` adapters TUI's
        // `paint_editor_popups` uses (`completion_menu_to_quadraui_completions`,
        // `hover_popup_to_quadraui_tooltip`, `signature_help_to_quadraui_tooltip`,
        // `diff_peek_to_quadraui_tooltip`, `editor_hover_popup_paint`); geometry
        // is expressed in GTK's native pixel units (`lh`/`cw`) rather than TUI's
        // cell units, which those adapters accept as an explicit `unit_w`/
        // `unit_h` scale — mirroring how `Completions::layout`/
        // `RichTextPopup::layout` already take an explicit `line_height`/
        // `row_height` rather than assuming cells.
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as f64 * cw;
            let h_scroll = active_win.scroll_left as f64 * cw;
            let win_x = active_win.rect.x;
            let win_y = active_win.rect.y;
            let win_viewport = quadraui::Rect::new(
                active_win.rect.x as f32,
                active_win.rect.y as f32,
                active_win.rect.width as f32,
                active_win.rect.height as f32,
            );

            // Completion popup — cache the layout so the click handler
            // (B.5b Stage 5) can hit-test items and register the popup on
            // the modal stack.
            *self.completion_layout.borrow_mut() = None;
            if let (Some(menu), Some((cursor_pos, _))) = (&screen.completion, &active_win.cursor) {
                let cursor_x = win_x + gutter_w + cursor_pos.col as f64 * cw - h_scroll;
                let cursor_y = win_y + cursor_pos.view_line as f64 * lh;
                // Longest candidate + 2 cells of padding/border, floored at 100px.
                let popup_w = ((menu.max_width + 2) as f64 * cw).max(100.0);
                let max_popup_h = 10.0 * lh;
                let completions = render::completion_menu_to_quadraui_completions(menu);
                let q_layout = completions.layout(
                    cursor_x as f32,
                    cursor_y as f32,
                    lh as f32,
                    win_viewport,
                    popup_w as f32,
                    max_popup_h as f32,
                    |_| quadraui::CompletionItemMeasure::new(lh as f32),
                );
                let mut frame = QSL::new();
                frame.push(Surface::Completions {
                    completions: &completions,
                    layout: &q_layout,
                });
                frame.draw(backend);
                *self.completion_layout.borrow_mut() = Some(q_layout);
            }

            // Simple LSP hover popup (plain text, non-interactive).
            if let Some(ref hover) = screen.hover {
                let anchor_view = hover.anchor_line.saturating_sub(active_win.scroll_top) as f64;
                let anchor_x = win_x + gutter_w + hover.anchor_col as f64 * cw - h_scroll;
                let anchor_y = win_y + anchor_view * lh;
                let (tooltip, tip_layout) = render::hover_popup_to_quadraui_tooltip(
                    hover,
                    anchor_x as f32,
                    anchor_y as f32,
                    main,
                    cw as f32,
                    lh as f32,
                );
                let mut frame = QSL::new();
                frame.push(Surface::Tooltip {
                    tooltip: &tooltip,
                    layout: &tip_layout,
                });
                frame.draw(backend);
            }

            // Signature-help popup (insert mode, cursor inside a call).
            if let Some(ref sig) = screen.signature_help {
                let anchor_view = sig.anchor_line.saturating_sub(active_win.scroll_top) as f64;
                let anchor_x = win_x + gutter_w + sig.anchor_col as f64 * cw - h_scroll;
                let anchor_y = win_y + anchor_view * lh;
                let (tooltip, tip_layout) = render::signature_help_to_quadraui_tooltip(
                    sig,
                    anchor_x as f32,
                    anchor_y as f32,
                    main,
                    theme,
                    cw as f32,
                    lh as f32,
                );
                let mut frame = QSL::new();
                frame.push(Surface::Tooltip {
                    tooltip: &tooltip,
                    layout: &tip_layout,
                });
                frame.draw(backend);
            }

            // Diff-peek popup (inline git hunk preview).
            if let Some(ref peek) = screen.diff_peek {
                let anchor_view = peek.anchor_line.saturating_sub(active_win.scroll_top) as f64;
                let anchor_x = win_x + gutter_w;
                let anchor_y = win_y + anchor_view * lh;
                let (tooltip, tip_layout) = render::diff_peek_to_quadraui_tooltip(
                    peek,
                    anchor_x as f32,
                    anchor_y as f32,
                    main,
                    theme,
                    cw as f32,
                    lh as f32,
                );
                let mut frame = QSL::new();
                frame.push(Surface::Tooltip {
                    tooltip: &tooltip,
                    layout: &tip_layout,
                });
                frame.draw(backend);
            }

            // Editor hover popup (rich markdown; `gh` key, diagnostic/
            // annotation/plugin hovers, or mouse dwell). Bounds/link rects/
            // scrollbar geometry are cached for the click + drag handlers
            // (#215), same as `draw.rs::draw_editor_hover_popup` did.
            self.editor_hover_popup_rect.set(None);
            self.editor_hover_link_rects.borrow_mut().clear();
            self.editor_hover_scrollbar.set(None);
            if let Some(ref eh) = screen.editor_hover {
                let anchor_view = eh.anchor_line.saturating_sub(eh.frozen_scroll_top) as f64;
                let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left) as f64;
                let anchor_x = win_x + gutter_w + vis_col * cw;
                let anchor_y = win_y + anchor_view * lh;
                let (links, rect, sb) = render::editor_hover_popup_paint(
                    backend,
                    eh,
                    anchor_x as f32,
                    anchor_y as f32,
                    win_viewport,
                    theme,
                    cw as f32,
                    lh as f32,
                );
                self.editor_hover_popup_rect.set(rect);
                *self.editor_hover_link_rects.borrow_mut() = links;
                self.editor_hover_scrollbar.set(sb);
            }
        }
    }

    /// Per-frame state pushes that must happen before anything is composed.
    ///
    /// Theme, nerd-font flag and UI font are re-synced *every* frame so a
    /// runtime `:set colorscheme` / `:set nonerdfonts` / `:set guifont` /
    /// `:set ui_font_size=N` reaches the rasterisers on the very next paint
    /// instead of never; the two hit-test registries are cleared so a panel or
    /// window that closed this frame leaves no stale entry for
    /// `dispatch_scroll` / `pixel_to_click_target` to resolve against
    /// (#592/#672). Extracted out of `render_content` (#766) so that function
    /// reads as the frame's *order* and nothing else.
    fn sync_per_frame_backend_state(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        theme: &Theme,
    ) {
        backend.set_theme(render::to_quadraui_theme(theme));
        // (#547) Re-synced every frame so runtime toggles (`:set
        // nonerdfonts`) take effect immediately, matching TUI.
        render::sync_nerd_fonts(backend, engine);
        // Re-synced every frame so a runtime `:set guifont`/font-size change
        // takes effect immediately (#217/#672). Ported from the dead
        // `draw.rs::draw_editor`'s top-of-frame call, which was the only
        // live caller — `UI_FONT()` (used a few paint calls below for the
        // raw-Pango chrome that doesn't go through `Backend::set_ui_font`)
        // read the process-global atomic this writes, so before this port it
        // silently stayed pinned at the default size forever.
        sync_ui_font_size(&engine.settings);
        // #705 item 3 / quadraui#624: push the same UI_FONT() family+size
        // onto the *paint* backend's `ui_font`, which `draw_status_bar`
        // (breadcrumbs, per-window/global status lines), `draw_tree`
        // (explorer), `draw_tab_bar_icons`, and `draw_menu_bar` now all
        // honour for both paint and their no-paint measurement twins
        // (quadraui#624). Before this call `ui_font` on the paint backend
        // (a *separate* `GtkBackend` instance from `self.backend`, which
        // `sync_nerd_fonts` above already keeps synced for click-time
        // hit-testing) was never touched, so it
        // sat at quadraui's own "Sans 11" default forever: chrome text
        // didn't track `settings.ui_font_size`, and — per #700's item 3 —
        // status-bar-painted breadcrumb text had no font of its own to
        // decouple it from whatever font a prior draw call in the frame
        // left on the shared Pango layout. Re-set every frame (not just
        // once from `setup()`) so a runtime `:set ui_font_size=N` takes
        // effect immediately, matching `sync_ui_font_size`/`sync_nerd_fonts`
        // just above.
        backend.set_ui_font(&UI_FONT());
        // #947 / quadraui#422: push `settings.font_family`/`font_size` onto
        // the *paint* backend's editor font every frame, so a runtime
        // `:set guifont`/`:set font_size=N`/`zoomin`/`zoomout` reaches the
        // painted editor text on the very next frame — mirroring
        // `set_ui_font` immediately above. Before this call nothing in
        // vimcode ever read `settings.font_family`/`font_size` (`grep -rn
        // "set_editor_font" src/` returned zero hits before this issue), so
        // the editor painted at whatever default quadraui's `GtkBackend`/
        // `MacBackend` ship with ("Monospace 11") regardless of the setting.
        backend.set_editor_font(
            &engine.settings.font_family,
            engine.settings.font_size as f32,
        );

        // #672: scroll surfaces are re-registered from scratch every frame
        // (mirrors TUI's `render_impl.rs` `scroll_surfaces.borrow_mut().clear()`)
        // so a panel that closes — or moves — doesn't leave a stale entry
        // behind for `dispatch_scroll`/`dispatch_click` to hit-test against.
        // Ported from the dead `draw.rs::draw_editor`'s equivalent top-of-frame
        // clear, which was this list's only writer under `ShellApp` (#592/#672).
        engine.scroll_surfaces.borrow_mut().clear();
        // Per-window status bar segment hit zones (#672): `click.rs`'s
        // `pixel_to_click_target` reads this to resolve `WindowZone::StatusBar`
        // clicks (goto-line, change-language, switch-branch, ...) to a
        // `StatusAction`, but under `ShellApp` nothing ever populated it — the
        // dead `draw.rs::draw_window_status_bar` was the only writer, so every
        // per-window status bar segment click silently resolved to
        // `ClickTarget::None`. Cleared here and re-inserted per window below
        // (and for the separated status line further down) so a window that
        // closes doesn't leave a stale, now-wrong entry keyed by its id.
        self.status_segment_map.borrow_mut().clear();
    }

    /// Compose the **editor band** (#764, #735 slice 3) and recover this
    /// frame's `FrameHitMap` from it.
    ///
    /// Walks `render::compose_editor_band` — the single ordered artefact both
    /// backends walk for the editor column, exactly as `FRAME_Z_ORDER` is for
    /// the surrounding chrome and the app-level overlays. Extracted out of
    /// `render_content` (#766) so that function reads as the frame's *order*;
    /// the TUI twin has been a method (`TuiShellApp::paint_editor_band`) since
    /// #764 for the same reason.
    ///
    /// `band` carries the editor column's origin and width (its height is not
    /// used); `metrics` is `(line_height, char_width)` and `tab_metrics` is
    /// `(tab_row_h, tab_bar_h)`, both in pixels.
    #[allow(clippy::too_many_arguments)]
    fn compose_editor_band_rungs(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &render::ScreenLayout,
        theme: &Theme,
        band: quadraui::Rect,
        metrics: (f64, f64),
        tab_metrics: (f64, f64),
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};
        let (lh, cw) = metrics;
        let (tab_row_h, tab_bar_h) = tab_metrics;

        // ══ Editor band (#764, #735 slice 3) ═════════════════════════════════
        //
        // Composed from `render::compose_editor_band` — the single ordered
        // artefact both backends walk for the editor column, exactly as
        // `CHROME_Z_ORDER` (further down) is for the surrounding chrome and
        // `FRAME_Z_ORDER`'s overlay tail for the app-level overlays. Geometry
        // stays here, in pixels; only the *order* and the *gates* moved.
        //
        // Two things the walk changed on GTK, both recorded in
        // `EDITOR_Z_ORDER`'s doc comment: the between-*group* divider lines
        // are painted at all now (they were populated every frame and
        // hit-tested for drags, and drawn by nobody — a `Ctrl+W v` boundary
        // was draggable but invisible, #592's exact shape), and the tab-drag
        // ghost is composed *here* rather than ~900 lines further down, after
        // the editor-anchored popups that used to paint over it.
        //
        // `window_editors` stashes each window's owned `quadraui::Editor` past
        // the walk (#449) so the `FrameHitMap` built after it can reference the
        // SAME objects just painted, instead of constructing a second copy that
        // could drift from what's on screen; `hit_bars` does the same for the
        // tab bars. Both are folded into `hit_frame` *after* the walk, editors
        // first, because `FrameZone::TabBar { idx }` is keyed by the global
        // surface index — see `cached_tab_bar_zones`'s doc comment for why a
        // plain `Vec` indexed from 0 was wrong. Accumulating them during the
        // walk instead would make `hit_frame` hold a borrow of `window_editors`
        // across loop iterations, which is the borrow the two-phase shape here
        // exists to avoid.
        //
        // #731: no vertical (or horizontal) scrollbar is painted for the
        // editor on GTK today. quadraui's `gtk::editor::draw_editor` (the
        // rasteriser `Surface::Editor` below calls into) documents that it
        // deliberately skips scrollbars on GTK and defers to "the host" —
        // meaning the Relm4-era native `gtk4::Scrollbar` overlay path this
        // issue deleted (`sync_scrollbar`/`create_window_scrollbars`, plus
        // the pixel-inset math in the now-deleted
        // `native_scrollbar_margin_start`: `rect.x + rect.width -
        // minimap_width - scrollbar_width - 2.0`, clamped to `rect.x`).
        // That path never ran under the ShellApp runner (nothing assigns
        // `self.overlay`/`self.drawing_area`), so it was already dead
        // before this cleanup — this was not a working feature this PR
        // broke.
        //
        // TUI's equivalent rasteriser (`quadraui::tui::editor`) paints an
        // inline vertical + horizontal scrollbar column as part of the
        // `Editor` primitive itself, narrowing the text viewport to make
        // room — see `super::draw_scrollbar` calls in that module. GTK has
        // no equivalent; closing this gap means teaching
        // `quadraui::gtk::editor::draw_editor` to do the same (Cairo-paint
        // a scrollbar inside `editor.rect`, mirroring TUI), which also
        // makes the minimap inset automatic (the viewport is already
        // narrowed by `minimap_reserved_width` before the rect reaches the
        // rasteriser) — no separate margin-inset formula would be needed
        // there. That is quadraui-side work per `CLAUDE.md`'s
        // Platform-Neutrality Rule (file a quadraui issue; do not
        // reintroduce GTK-specific scrollbar widget plumbing here). #723's
        // fix (`e02a824`) targeted the dead native-widget path and cannot
        // have been visible on screen; it needs re-verifying once this
        // lands there.
        let mut window_editors: Vec<quadraui::Editor> = Vec::with_capacity(screen.windows.len());
        let mut hit_bars: Vec<(core::window::GroupId, quadraui::Rect, &quadraui::TabBar)> =
            Vec::new();
        // Reset the pixel-accurate hit caches; repopulated by the `TabBars`
        // rung below so the click / hover hit-tests use the exact drawn
        // geometry (#515). Cleared here, before the walk, rather than from an
        // absent-rung branch: `compose_editor_band` returns only the live
        // rungs, so a frame with no tab bars has no arm to clear them from.
        self.cached_tab_pixel_hits.borrow_mut().clear();
        self.cached_tab_close_abs.borrow_mut().clear();
        self.cached_tab_slots_abs.borrow_mut().clear();

        let mut composed_editor: Vec<render::EditorOp> = Vec::new();
        for op in render::compose_editor_band(
            engine,
            screen,
            self.tab_drag.is_dragging(),
            engine.terminal_maximized,
        ) {
            match op {
                render::EditorOp::Windows => {
                    self.paint_editor_windows_rung(backend, screen, lh, &mut window_editors)
                }
                // #35/#722: minimap strips on every window's right edge (one
                // entry per `WindowId` in `screen.minimap`, not just the active
                // window's) — one call, the font-scaling rasteriser is
                // quadraui's.
                render::EditorOp::Minimap => {
                    render::draw_minimap_strip(backend, screen);
                }
                // ── Draw tab bar(s) — one per editor group ───────────────────
                // Multi-group (post-split) layouts have a tab bar per group,
                // each drawn at the top edge of its own bounds. Single-group
                // draws one full-width bar at the editor top (#515/#551).
                render::EditorOp::TabBars => self.paint_tab_bars_rung(
                    backend,
                    engine,
                    screen,
                    tab_row_h,
                    tab_bar_h,
                    &mut hit_bars,
                ),
                // ── Draw breadcrumb bar(s) below tab bar(s) ──────────────────
                // (#547) `render_content` is the active ShellApp draw path
                // since the #540 Relm4→ShellApp migration; the legacy
                // `draw.rs::draw_editor` path that used to draw breadcrumbs is
                // dead (no callers) and this step was never ported over, so
                // breadcrumbs stopped rendering even though layout space for
                // them was still reserved (`tab_bar_h` above) and clicks were
                // still hit-tested against them.
                render::EditorOp::Breadcrumbs => {
                    render::paint_breadcrumb_bars(backend, screen, engine.terminal_maximized);
                }
                // ── Group divider lines (#764) ───────────────────────────────
                // The `Ctrl+W v` / `Ctrl+W s` boundaries *between* editor
                // groups, as opposed to the `Windows` rung's within-group
                // `:split` lines. GTK painted nothing here at all before #764
                // while still resolving drags against the same rects through
                // `screen_zone_hit_test` — see `ScreenLayout::group_dividers`
                // for the full verdict.
                //
                // Rasterised through the same `Split` primitive the
                // within-group dividers use. TUI rasterises this rung cell by
                // cell instead, because it carries the #481 guard that
                // suppresses a divider column immediately beside a
                // neighbouring window's scrollbar — a coalescence problem that
                // exists only in a character grid. See
                // `render::draw_dividers_as_splits` for why the two
                // rasterisers legitimately differ while the rung does not.
                render::EditorOp::GroupDividers => {
                    render::draw_dividers_as_splits(backend, &screen.group_dividers, |div| {
                        quadraui::WidgetId::new(format!("gdiv:{}", div.split_index))
                    });
                }
                // ── Tab drag overlay ─────────────────────────────────────────
                // When a tab drag is in progress, paint the drop-zone
                // highlight + insertion bar over the editor column.
                //
                // The per-group drop geometry is computed here, from the
                // absolute bounds in the shared screen layout plus the per-tab
                // slot positions the `TabBars` rung just captured, and stashed
                // so the drag hit-test (`handle_mouse_drag_msg`) and this
                // overlay use one identical source.
                //
                // Origin convention: `gtb.bounds` are always absolute (built
                // from absolute window rects), so there is no origin offset to
                // apply — adding (x,y) again would double-count it and shift
                // the highlight off the group (the prior "covers half the
                // group" bug, #515).
                render::EditorOp::TabDragOverlay => {
                    let eff_tbh = self.cache_tab_drop_geometry(screen, engine, tab_bar_h);
                    let groups = self.cached_drop_groups.borrow();
                    let (mx, my) = self.mouse_pos_cell.get();
                    render::paint_tab_drop_overlay(
                        backend,
                        self.tab_drag.zone(),
                        &groups,
                        (mx as f32, my as f32),
                        eff_tbh,
                        2.0,
                        lh as f32,
                    );
                }
                // ── Tab-hover tooltip (#671) ─────────────────────────────────
                // Small popup shown when the mouse lingers over a tab, naming
                // the buffer under the cursor. `screen.tab_tooltip` was
                // populated by the engine the whole time (#592's root cause)
                // but had no GTK painter at all — unlike quickfix/panel_hover
                // (#670) there was no dead `draw.rs` version to port either;
                // `draw.rs:425` painted it with raw Cairo/Pango, never through
                // `Backend`. Routed through the shared
                // `render::tab_hover_tooltip_paint` (TUI calls the same
                // function with its 1.0/1.0 cell scale instead of GTK's
                // `cw`/`lh` pixel scale) so paint logic isn't reimplemented per
                // backend. Positioned one *tab row* below the top of the editor
                // column — `tab_row_h` (computed above), not `lh` — mirroring
                // TUI's `area.y + 1`: TUI's tab bar is exactly one *cell row*
                // tall regardless of the breadcrumbs setting (see `mouse.rs`'s
                // `tab_bar_rows`), so its `+1` clears only the tab row itself,
                // same as GTK's `tab_row_h` here (as opposed to `tab_bar_h`,
                // which also reserves the breadcrumb row when that setting is
                // on — using `lh` alone landed the tooltip's top edge inside
                // the tab row's own vertical span, painting over tab labels
                // instead of below them, since GTK's tab row is `1.6×` a line
                // height, not `1×` like TUI's).
                render::EditorOp::TabTooltip => {
                    if let Some(ref tooltip_text) = screen.tab_tooltip {
                        render::tab_hover_tooltip_paint(
                            backend,
                            band.x,
                            band.y + tab_row_h as f32,
                            band.width,
                            tooltip_text,
                            theme,
                            cw as f32,
                            lh as f32,
                        );
                    }
                }
            }
            composed_editor.push(op);
        }
        *self.composed_editor_band.borrow_mut() = composed_editor;
        // Same contract as the chrome/overlay bands: read back through the
        // field rather than the local, so the *stored* observable is what gets
        // validated — a frame that recorded one thing and composed another
        // would be a lie the tests then trusted.
        if let Err(why) = render::check_editor_band_order(&self.composed_editor_band.borrow()) {
            debug_assert!(false, "GTK {why}");
        }

        // ── Recover a FrameHitMap for Editor/TabBar zone detection (#449) ──────
        // Pure `.push()` accumulation into a *separate* `ScreenLayout`, built
        // from the same `Editor` objects painted by the `Windows` rung above
        // (`window_editors`, same order as `screen.windows` so
        // `FrameZone::Editor { idx }` maps straight back to
        // `cached_layout.windows[idx]`) plus the `TabBar` surfaces the
        // `TabBars` rung recorded. `ScreenLayout::hit_map()` (quadraui#425)
        // makes no `backend.draw_*()` calls, so accumulating into it can never
        // reorder or repeat the real painting done above — see
        // `click::pixel_to_click_target` for the consumer side.
        //
        // Editors are pushed first and the tab bars after, so the first tab
        // bar's `FrameZone::TabBar { idx }` is `window_editors.len()`, not `0`
        // — the *global* surface index `cached_tab_bar_zones` is keyed by.
        let mut hit_frame = QSL::new();
        for editor in &window_editors {
            hit_frame.push(Surface::Editor {
                rect: editor.rect,
                editor,
            });
        }
        let mut tab_bar_zones: HashMap<usize, (core::window::GroupId, quadraui::Rect)> =
            HashMap::new();
        for (surface_idx, (group_id, rect, bar)) in (window_editors.len()..).zip(hit_bars) {
            hit_frame.push(Surface::TabBar {
                rect,
                bar,
                hovered_close: None,
            });
            tab_bar_zones.insert(surface_idx, (group_id, rect));
        }
        *self.cached_frame_hit_map.borrow_mut() = Some(hit_frame.hit_map());
        *self.cached_tab_bar_zones.borrow_mut() = tab_bar_zones;

        // Refresh the drop geometry the *drag hit-test* reads
        // (`handle_mouse_drag_msg` → `compute_tab_drop_zone`), unconditionally
        // and for every frame — a drag has to be able to *start*, which means
        // the cache must be current on frames where no drag is live and the
        // `TabDragOverlay` rung therefore never ran. The rung calls this same
        // function rather than carrying a second copy of the computation, so
        // the overlay and the hit-test can only ever agree.
        self.cache_tab_drop_geometry(screen, engine, tab_bar_h);
    }

    /// Compose the **bottom band** (#765, #735 slice 4): the chrome vimcode
    /// stacks below the editor column — quickfix, the terminal/debug bottom
    /// panel, the debug toolbar, the separated status line and the sidebar
    /// hover popup.
    ///
    /// Walks `render::compose_bottom_band`; geometry stays here, in pixels,
    /// mirroring `compute_editor_layout`'s `unit_h = line_height` convention.
    /// Extracted out of `render_content` (#766) so that function reads as the
    /// frame's *order* and nothing else. `main` is
    /// `AppShellLayout::main_content_bounds`.
    #[allow(clippy::too_many_arguments)]
    fn compose_bottom_band_rungs(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &render::ScreenLayout,
        layout: &quadraui::AppShellLayout,
        theme: &Theme,
        main: quadraui::Rect,
        el: &render::EditorLayout,
        editor_area_h: f64,
        metrics: (f64, f64),
    ) {
        let (lh, cw) = metrics;
        let _ = cw;

        // ══ Bottom band (#765, #735 slice 4) ═════════════════════════════════
        //
        // Composed from `render::compose_bottom_band` — the single ordered
        // artefact both backends walk for the chrome stacked below the editor
        // column, exactly as `EDITOR_Z_ORDER` (above) is for the column itself
        // and `CHROME_Z_ORDER` (below) for the surrounding chrome. Geometry
        // stays here, in pixels, mirroring `compute_editor_layout`'s
        // `unit_h = line_height` convention; only the *order* and the *gates*
        // moved. `BOTTOM_Z_ORDER`'s doc comment records the five divergences
        // this convergence closed — including the two this backend owned: the
        // panel-hover popup nested inside the sidebar rung where it could not
        // clear its own click-routing cache, and the debug output's body
        // starting a row too low.
        //
        // `editor_area_h` above (`el.editor_bottom`) already reserves the whole
        // stack, so these bands sit directly below it with no gap and no
        // overlap with `status_y`.
        //
        // Caches whose owning rung may be gated off are cleared *here*, before
        // the walk, never from an `else` arm inside it: `compose_bottom_band`
        // returns only the live rungs, so an absent rung has no arm to run.
        // Clearing from inside the walk is precisely the mistake that left this
        // backend routing clicks at a popup it had stopped painting.
        engine.bottom_panel_geometry.replace(None);
        self.debug_toolbar_y_offset.set(0.0);
        self.debug_toolbar_height.set(0.0);
        self.separated_status_bar_rect.set(None);
        self.panel_hover_popup_rect.set(None);
        self.panel_hover_link_rects.borrow_mut().clear();

        let quickfix_y = main.y as f64 + editor_area_h;
        let terminal_y = quickfix_y + el.quickfix_h;
        let debug_toolbar_y = terminal_y + el.terminal_h;
        let separated_status_y = debug_toolbar_y + el.debug_toolbar_h;
        let mut composed_bottom: Vec<render::BottomOp> = Vec::new();
        for op in
            render::compose_bottom_band(engine, screen, layout.sidebar_content_bounds.is_some())
        {
            match op {
                render::BottomOp::Quickfix => {
                    let Some(ref qf) = screen.quickfix else {
                        continue;
                    };
                    // GTK has no persistent `quickfix_scroll_top` to advance
                    // from key events (unlike TUI's `TuiShellApp`), so the
                    // "keep the selection visible" offset is recomputed
                    // statelessly each frame — through the shared
                    // `quickfix_scroll_top`, so the two backends cannot
                    // disagree about what that means.
                    let visible_rows = ((el.quickfix_h / lh) as usize).saturating_sub(1);
                    render::paint_quickfix_rung(
                        backend,
                        qf,
                        quadraui::Rect::new(
                            main.x,
                            quickfix_y as f32,
                            main.width,
                            el.quickfix_h as f32,
                        ),
                        render::quickfix_scroll_top(qf, visible_rows),
                    );
                    composed_bottom.push(render::BottomOp::Quickfix);
                }
                render::BottomOp::BottomPanel => {
                    render::paint_bottom_panel_rung(
                        backend,
                        engine,
                        screen,
                        theme,
                        quadraui::Rect::new(
                            main.x,
                            terminal_y as f32,
                            main.width,
                            el.terminal_h as f32,
                        ),
                        render::BottomPanelUnits::px(lh, cw),
                    );
                    composed_bottom.push(render::BottomOp::BottomPanel);
                }
                render::BottomOp::DebugToolbar => {
                    let rect =
                        quadraui::Rect::new(main.x, debug_toolbar_y as f32, main.width, lh as f32);
                    render::draw_debug_toolbar(backend, engine, rect);
                    self.debug_toolbar_y_offset.set(debug_toolbar_y);
                    self.debug_toolbar_height.set(lh);
                    composed_bottom.push(render::BottomOp::DebugToolbar);
                }
                // Shown below the terminal band when `window_status_line` is on
                // but `status_line_above_terminal` is off (see
                // `compute_editor_layout`'s `has_separated`).
                // `el.editor_bottom` already reserved `el.separated_status_h`
                // of vertical space right here — between the debug toolbar and
                // `status_y` below.
                render::BottomOp::SeparatedStatus => {
                    let Some(ref status) = screen.separated_status_line else {
                        continue;
                    };
                    let sb_rect = quadraui::Rect::new(
                        main.x,
                        separated_status_y as f32,
                        main.width,
                        el.separated_status_h as f32,
                    );
                    // #672: segment hit-zone recovery keyed by
                    // `active_window_id` — the separated line shows the active
                    // window's status, so that's the id `pixel_to_click_target`
                    // looks its zones up under. The layout comes back from the
                    // paint itself now rather than from a second
                    // `status_bar_layout` call on the same rect.
                    let sb_layout = render::paint_separated_status_rung(backend, status, sb_rect);
                    self.status_segment_map.borrow_mut().insert(
                        screen.active_window_id.0,
                        render::status_bar_zones_from_layout(&sb_layout),
                    );
                    self.separated_status_bar_rect.set(Some(sb_rect));
                    composed_bottom.push(render::BottomOp::SeparatedStatus);
                }
                // Source-control / extension-panel item dwell tooltip, rendered
                // markdown, via the shared `quadraui::RichTextPopup` path.
                // Clamped against the full content viewport so it can extend
                // rightward into the editor area past the sidebar's own bounds
                // — which is why it is composed here, after everything it can
                // overhang, rather than inside the sidebar rung where it used
                // to live.
                render::BottomOp::PanelHover => {
                    // `sidebar_open` — the gate this rung was composed behind
                    // — *is* `sidebar_content_bounds.is_some()`, so this
                    // `else` is unreachable; kept as the same
                    // `let … else { continue }` shape the sibling arms use
                    // rather than an `unwrap` that would panic if the gate and
                    // the anchor ever stopped agreeing.
                    let Some(q_sb) = layout.sidebar_content_bounds else {
                        continue;
                    };
                    let hover_viewport = main;
                    let (links, rect) = render::panel_hover_popup_paint(
                        backend,
                        screen,
                        theme,
                        q_sb.x + q_sb.width,
                        q_sb.y,
                        hover_viewport,
                        main.width,
                        lh as f32,
                    );
                    self.panel_hover_popup_rect.set(rect);
                    *self.panel_hover_link_rects.borrow_mut() = links;
                    composed_bottom.push(render::BottomOp::PanelHover);
                }
            }
        }
        *self.composed_bottom_band.borrow_mut() = composed_bottom;
        // Debug-only: a rung hoisted back out of the walk, or composed early,
        // shows up here as a diagnosable string rather than a visual mystery.
        if let Err(why) = render::check_bottom_band_order(&self.composed_bottom_band.borrow()) {
            debug_assert!(false, "{why}");
        }
    }

    fn paint_tab_bars_rung<'a>(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        screen: &'a render::ScreenLayout,
        tab_row_h: f64,
        tab_bar_h: f64,
        hit_bars: &mut Vec<(core::window::GroupId, quadraui::Rect, &'a quadraui::TabBar)>,
    ) {
        let painted = render::paint_tab_bars(
            backend,
            engine,
            screen,
            tab_row_h,
            tab_bar_h,
            self.tab_close_hover
                .map(|(gid, i)| (core::window::GroupId(gid), i)),
        );
        let mut pixel_hits = self.cached_tab_pixel_hits.borrow_mut();
        let mut close_abs = self.cached_tab_close_abs.borrow_mut();
        let mut slots_abs = self.cached_tab_slots_abs.borrow_mut();
        for bar in painted {
            // Recover the exact pixel geometry the rasteriser just drew and
            // cache it (relative to the bar's left edge) for hit-testing.
            //
            // #764: `bar.hits` is what the *paint* returned, not the separate
            // `tab_bar_layout_icons` re-measure this used to make. The icon
            // reservation widens every decorated tab, so an icon-less or
            // differently-fonted twin reports slot and close bounds shifted
            // left of the painted glyphs — i.e. the close × of tab N lands
            // inside tab N+1's painted slot, and clicking it closes the wrong
            // tab. Exactly the measure/paint desync of #654; reading the
            // paint's own answer makes it unreachable rather than merely
            // fixed (#703).
            let ph = tab_hits_to_pixel_hits(&bar.hits, bar.bar, bar.rect.x as f64);
            let bar_top = bar.rect.y as f64;
            close_abs.insert(
                bar.group_id.0,
                abs_close_record(&ph.close, bar.rect.x as f64, bar_top, bar_top + tab_row_h),
            );
            slots_abs.insert(bar.group_id.0, abs_visible_slots(&bar.hits));
            pixel_hits.insert(bar.group_id.0, ph);
            hit_bars.push((bar.group_id, bar.rect, bar.bar));
        }
    }

    /// Recompute the per-group tab-drop geometry from this frame's screen
    /// layout and stash it in `cached_drop_groups` / `cached_drop_tbh`;
    /// returns the effective tab-bar height the same call resolved.
    ///
    /// One source for two consumers that must never disagree: the drag
    /// hit-test (`handle_mouse_drag_msg` → `render::compute_tab_drop_zone`)
    /// and the `EditorOp::TabDragOverlay` rung's own highlight. `render_content`
    /// calls this unconditionally once per frame — a drag has to be able to
    /// *start*, so the cache must be current on frames where no drag is live —
    /// and the rung calls it again rather than carrying a second copy of the
    /// computation.
    ///
    /// Origin convention: `gtb.bounds` are always absolute (built from absolute
    /// window rects), so there is no origin offset to apply — adding the editor
    /// column's `(x, y)` again would double-count it and shift the highlight
    /// off the group (the prior "covers half the group" bug, #515). This used
    /// to branch on `editor_group_split.is_some()` because the single-group arm
    /// of `screen_to_drop_group_bounds` derived its rect from a caller-supplied
    /// origin/size instead; `group_tab_bars` now covers one group too, so both
    /// the branch and the parameters it fed are gone (#551).
    fn cache_tab_drop_geometry(
        &self,
        screen: &render::ScreenLayout,
        engine: &Engine,
        tab_bar_h: f64,
    ) -> f32 {
        let bounds = render::screen_to_drop_group_bounds(screen);
        // Per-tab slot x-positions (absolute) are captured by the `TabBars`
        // rung while drawing. Feeding them here makes a drag inside a group's
        // own tab bar resolve to a `TabReorder` (insertion bar) instead of
        // falling through to a new-split/center overlay. (#515)
        let slots_abs = self.cached_tab_slots_abs.borrow();
        let (groups, eff_tbh) =
            render::build_tab_drop_groups(&bounds, engine, tab_bar_h as f32, &slots_abs);
        drop(slots_abs);
        *self.cached_drop_groups.borrow_mut() = groups;
        self.cached_drop_tbh.set(eff_tbh);
        eff_tbh
    }

    /// Re-resolve a tab-drag press point to `(group, tab index)`, or `None`
    /// when the press was not on a tab after all.
    ///
    /// GTK arms the drag for the whole tab-bar band (its tab geometry is
    /// proportional-font pixel bounds, resolved by `pixel_to_click_target`, not
    /// the exact cell hit TUI gets for free), so the confirmation that
    /// `render::TabDragMove::Crossed` asks for is a real second hit-test here.
    fn tab_drag_source_at(&self, x: f64, y: f64) -> Option<(core::window::GroupId, usize)> {
        let layout_ref = self.cached_screen_layout.borrow();
        let layout = layout_ref.as_ref()?;
        let mut engine = self.engine.borrow_mut();
        let target = pixel_to_click_target(
            &mut engine,
            &**self.backend.borrow(),
            x,
            y,
            self.cached_line_height,
            self.cached_char_width,
            layout,
            &self.cached_tab_pixel_hits.borrow(),
            self.cached_frame_hit_map.borrow().as_ref(),
            &self.cached_tab_bar_zones.borrow(),
            true, // resolving the original tab-bar mouse-down; switching tabs is intended
        );
        if !matches!(target, ClickTarget::TabBar) {
            return None;
        }
        // The tab was already switched by `pixel_to_click_target`, so the
        // active group + active tab *is* the drag source.
        let gid = engine.active_group;
        let tidx = engine
            .editor_groups
            .get(&gid)
            .map(|g| g.active_tab)
            .unwrap_or(0);
        Some((gid, tidx))
    }

    /// Resolve the modal-overlay rung (#733) for one point/action against
    /// the layouts the last frame actually painted
    /// (`dialog_layout`, `tab_switcher_popup_rect`, `completion_layout`,
    /// `Engine::toast_layout`), never freshly recomputed ones (#582/#646).
    ///
    /// Shared by every mouse-button path that needs to know whether a
    /// modal overlay owns the event — left-click dispatch
    /// (`handle_mouse_click_msg`, `ModalMouseAction::LeftPress`) and
    /// right-click dispatch (the `MouseButton::Right` arm of `handle`,
    /// `ModalMouseAction::Other`) both call this rather than re-deriving
    /// the state. TUI's `handle_mouse` already funnels every mouse event
    /// through one call to `render::route_modal_overlay_click`; this is
    /// GTK's equivalent single call site.
    fn route_modal_overlay(
        &self,
        x: f64,
        y: f64,
        action: render::ModalMouseAction,
    ) -> render::ModalOverlayRoute {
        let engine_ref = self.engine.borrow();
        let toast = engine_ref.toast_layout.borrow().clone();
        let dialog = self.dialog_layout.borrow().clone();
        let completion = self.completion_layout.borrow().clone();
        let context_menu = self.context_menu_layout.borrow().clone();
        let tab_switcher_bounds = self.tab_switcher_popup_rect.get();
        let lh = self.painted_line_height() as f32;
        // Both geometries come from what the last frame PAINTED — the picker's
        // own published rect, and the `FindReplacePanel` the frame was built
        // from — never a re-derivation off the drawing-area size (#555/#582).
        let picker = self.picker_popup_rect.get().map(|rect| {
            render::PickerHitGeometry::new(
                rect,
                lh,
                engine_ref.picker_preview.is_some(),
                &render::gtk_picker_rows(lh),
                &engine_ref,
            )
        });
        let screen_ref = self.cached_screen_layout.borrow();
        let find_replace = screen_ref
            .as_ref()
            .and_then(|s| s.find_replace.as_ref())
            .map(|panel| {
                render::FindReplaceHitGeometry::from_panel(
                    panel,
                    (self.painted_char_width() as f32, lh),
                    &render::GTK_FIND_REPLACE_ANCHOR,
                )
            });

        render::route_modal_overlay_click(
            &render::ModalOverlayState {
                toast: toast.as_ref(),
                dialog_open: engine_ref.dialog.is_some(),
                dialog: dialog.as_ref(),
                context_menu_open: engine_ref.context_menu.is_some(),
                context_menu: context_menu.as_ref(),
                // The GTK rasteriser strokes the menu's border *inside*
                // `ContextMenuLayout::bounds`, so there is no frame outside it.
                context_menu_border: 0.0,
                tab_switcher_open: engine_ref.tab_switcher_open,
                tab_switcher_bounds,
                completion_open: engine_ref.completion_idx.is_some(),
                completion: completion.as_ref(),
                picker_open: engine_ref.picker_open,
                picker,
                find_replace_open: engine_ref.find_replace_open,
                find_replace,
            },
            x as f32,
            y as f32,
            action,
        )
    }

    /// Apply a unified-picker verdict from
    /// [`render::route_modal_overlay_click`].
    ///
    /// The verdict itself — which result row, thumb vs. track, inside vs.
    /// outside — is `render::PickerHitGeometry`'s, shared with TUI's
    /// `handle_mouse`. Before #751 each backend resolved it from its own
    /// re-derivation of the popup geometry, and the two had already drifted:
    /// GTK jumped the offset proportionally on a track click and grabbed the
    /// thumb at zero, TUI paged the track and grabbed with an offset, and
    /// clicking an already-selected row confirmed it on TUI but did nothing on
    /// GTK.
    fn apply_picker_route(&mut self, route: render::PickerRoute) {
        let picker_id = quadraui::WidgetId::new("picker");
        let Some(rect) = self.picker_popup_rect.get() else {
            return;
        };
        let lh = self.painted_line_height() as f32;
        let geo = {
            let engine = self.engine.borrow();
            render::PickerHitGeometry::new(
                rect,
                lh,
                engine.picker_preview.is_some(),
                &render::gtk_picker_rows(lh),
                &engine,
            )
        };
        // Keep the stack in step: the drag guard in `handle_mouse_drag_msg`
        // consults it to stop a gesture leaking to the editor behind the modal
        // (#192).
        self.backend
            .borrow()
            .modal_stack_handle()
            .borrow_mut()
            .push(picker_id.clone(), geo.bounds);

        match route {
            render::PickerRoute::Row(idx) => {
                render::apply_picker_row_click(&mut self.engine.borrow_mut(), idx);
            }
            render::PickerRoute::ScrollbarThumb { grab_offset } => {
                self.backend
                    .borrow()
                    .drag_state_handle()
                    .borrow_mut()
                    .begin(geo.drag_target(picker_id, grab_offset));
            }
            render::PickerRoute::ScrollbarTrack { toward_end } => {
                render::apply_picker_scroll_offset(
                    &mut self.engine.borrow_mut(),
                    geo.paged_offset(toward_end),
                    geo.visible_rows,
                );
            }
            render::PickerRoute::Consume => {}
            render::PickerRoute::Dismiss => {
                self.engine.borrow_mut().close_picker();
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&picker_id);
            }
        }
    }

    /// Apply a context-menu verdict from [`render::route_modal_overlay_click`].
    ///
    /// Returns `true` when the event was consumed. The route itself — which
    /// item, hover vs. click, dismiss vs. keep-open — is decided once in
    /// `render.rs` and shared with TUI's `handle_mouse`; what stays here is
    /// GTK's own plumbing (modal-stack bookkeeping, file-tree refresh).
    fn apply_context_menu_route(&mut self, route: render::ContextMenuRoute) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        let pop_stack = |app: &Self| {
            app.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&cm_id);
        };
        match route {
            render::ContextMenuRoute::Item(idx) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(ref mut cm) = engine.context_menu {
                    cm.selected = idx;
                }
                let _act = engine.context_menu_confirm();
                let needs_tree_refresh = engine.explorer_needs_refresh;
                if needs_tree_refresh {
                    engine.explorer_needs_refresh = false;
                }
                drop(engine);
                pop_stack(self);
                if needs_tree_refresh {
                    self.refresh_file_tree();
                }
            }
            render::ContextMenuRoute::Hover(idx) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(ref mut cm) = engine.context_menu {
                    if cm.selected == idx {
                        return true;
                    }
                    cm.selected = idx;
                }
            }
            render::ContextMenuRoute::Consume => {}
            render::ContextMenuRoute::Dismiss => {
                self.engine.borrow_mut().close_context_menu();
                pop_stack(self);
            }
            render::ContextMenuRoute::Fallthrough => return false,
        }
        self.draw_needed.set(true);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_mouse_click_msg(&mut self, x: f64, y: f64, width: f64, height: f64, alt: bool) {
        // ── Folder picker mouse handling (#815) ─────────────────────────
        // Checked before every other rung: like a modal dialog, the picker
        // swallows every click while open rather than competing for z-order
        // through `route_modal_overlay_click` / `MOUSE_ARBITRATION_ORDER` —
        // see `render::route_folder_picker_click`'s doc comment. TUI's
        // `mouse::handle_mouse` checks the identical shared helper.
        if self.folder_picker.borrow().is_some() {
            self.route_and_apply_folder_picker_click(x, y);
            return;
        }

        self.reconcile_editor_hover_modal();

        // ── Modal-overlay rung (#733) ─────────────────────────────────────
        //
        // Toast → dialog → tab switcher → completion, sequenced ONCE in
        // `render::route_modal_overlay_click` and shared verbatim with
        // TUI's `handle_mouse`. This backend used to hand-roll the order
        // (toast, then tab switcher, then completion, with the dialog
        // ~600 lines further down, *below* find/replace) while TUI ran a
        // different one — the precedence drift #733 exists to kill.
        let modal_route = self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress);
        match modal_route {
            render::ModalOverlayRoute::Toast(hit) => {
                if self.engine.borrow_mut().handle_toast_hit(hit) {
                    self.draw_needed.set(true);
                    return;
                }
            }
            render::ModalOverlayRoute::Dialog(hit) => {
                match hit {
                    quadraui::DialogHit::Button(id) => {
                        if let Some(idx) = dialog_btn_index(&id) {
                            let action = self.engine.borrow_mut().dialog_click_button(idx);
                            self.apply_dialog_action(action);
                        }
                    }
                    quadraui::DialogHit::Outside => {
                        let mut engine = self.engine.borrow_mut();
                        engine.dialog = None;
                        engine.pending_move = None;
                    }
                    quadraui::DialogHit::Body | quadraui::DialogHit::BodyToolbarButton(_) => {}
                }
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::ContextMenu(route) => {
                if self.apply_context_menu_route(route) {
                    return;
                }
            }
            render::ModalOverlayRoute::TabSwitcher { inside } => {
                // Click anywhere dismisses; inside also consumes so the
                // editor underneath doesn't take a cursor move through it.
                self.engine.borrow_mut().tab_switcher_open = false;
                self.draw_needed.set(true);
                if inside {
                    return;
                }
            }
            render::ModalOverlayRoute::Completion(hit) => {
                let consumed = self.engine.borrow_mut().handle_completion_click(hit);
                self.draw_needed.set(true);
                if consumed {
                    return;
                }
            }
            render::ModalOverlayRoute::UnifiedPicker(hit) => {
                self.apply_picker_route(hit);
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::FindReplace(hit) => {
                if let render::FindReplaceRoute::Target { target, is_input } = hit {
                    if is_input {
                        self.fr_input_dragging = true;
                    }
                    self.engine.borrow_mut().handle_find_replace_click(target);
                }
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::Swallow => return,
            render::ModalOverlayRoute::None => {}
        }

        // ── Editor hover popup rung (#755) ────────────────────────────────
        //
        // Link click, scrollbar grab, focus-or-select and dismiss-on-outside,
        // sequenced ONCE in `render::route_editor_hover_popup_click` and
        // shared verbatim with TUI's `handle_mouse`. The ~100 lines this
        // replaced sat *below* the scroll-surface dispatch, so a press aimed
        // at the popup's own scrollbar was consumed by the surface painted
        // behind it (#229/#486). It runs above that dispatch now — where TUI
        // always had it — because the popup paints on top of the editor.
        if self.route_and_apply_editor_hover_popup(x, y) {
            return;
        }

        // ── Panel-hover popup link click (#1067) ──────────────────────────
        //
        // Shared with TUI's `mouse::handle_mouse` via `render::
        // route_panel_hover_popup_click` + `render::
        // apply_panel_hover_popup_route` — see `route_and_apply_panel_hover_
        // popup`'s doc for why this backend never had it before. Checked
        // above the scroll-surface dispatch for the same reason the editor
        // hover popup is: the popup paints on top of whatever is under it.
        if self.route_and_apply_panel_hover_popup(x, y) {
            return;
        }

        // ── Scroll-surface click dispatch (scrollbar thumb-drag + track-page). ──
        {
            let surfaces = self.engine.borrow().scroll_surfaces.borrow().clone();
            let modal = self.backend.borrow().modal_stack_handle().borrow().clone();
            let mut drag = self.backend.borrow().drag_state_handle().borrow().clone();
            let click_events = quadraui::dispatch_click(
                &modal,
                &surfaces,
                &[],
                &mut drag,
                quadraui::Point {
                    x: x as f32,
                    y: y as f32,
                },
                quadraui::MouseButton::Left,
                Default::default(),
            );
            *self.backend.borrow().drag_state_handle().borrow_mut() = drag;
            for cev in &click_events {
                match cev {
                    quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } => {
                        // #825: shared with TUI's click table and the drag
                        // path (#756) — `render::apply_scroll_offset` is the
                        // union of every id either backend emits here. GTK
                        // only ever registers `debug_output`/
                        // `terminal_scrollback` into `scroll_surfaces`
                        // (`explorer:sb`/`ext_panel:sb` are TUI-only), so
                        // this is a like-for-like replacement of the two
                        // hand-rolled arms above, not a behavior change.
                        if render::apply_scroll_offset(
                            &mut self.engine.borrow_mut(),
                            widget.as_str(),
                            *new_offset,
                            render::ScrollApplyContext {
                                picker_visible_rows: 0,
                            },
                        ) {
                            self.draw_needed.set(true);
                            return;
                        }
                    }
                    quadraui::UiEvent::MouseDown {
                        widget: Some(id), ..
                    } if id.as_str() == "debug_output" => {
                        return;
                    }
                    _ => {}
                }
            }
        }

        // #751: the context-menu, find/replace and unified-picker rungs that
        // used to be transcribed here — ~370 lines — are now decided by
        // `render::route_modal_overlay_click` at the top of this handler and
        // applied by `apply_context_menu_route` / `apply_picker_route`. The
        // shared router also fixed their order: this backend arbitrated the
        // context menu *below* find/replace and the picker, while
        // `render::FRAME_Z_ORDER` paints it above both.

        // ── Chrome rung (#752) ────────────────────────────────────────────
        //
        // Breadcrumbs → status bands → global status bar, sequenced ONCE in
        // `render::route_chrome_click` and shared verbatim with TUI's
        // `handle_mouse`. What used to live here was the breadcrumb arm, and
        // ~60 lines further down a git-branch hit test that re-derived
        // `build_status_line`'s formatting by hand and measured it in UTF-8
        // bytes against a character column. Both are gone.
        if self.route_and_apply_chrome_click(x, y, render::ChromeMouseAction::LeftPress) {
            return;
        }

        // ── Command line click — start text selection (#816) ─────────────
        // TUI's twin rung (`mouse::handle_mouse`) has done this since #194;
        // GTK never could, for lack of a character-offset hit test on its
        // pixel-painted command line. quadraui#705's `CommandLineLayout`
        // closed that gap — `render::command_line_click_char_idx` hit-tests
        // `engine.command_line_rect` (cached at paint time, just above) the
        // same way TUI's press rung does, so this is thin wiring, not a new
        // GTK-specific selection implementation.
        if render::command_line_selection_allowed(&self.engine.borrow()) {
            let data = render::build_command_line(&self.engine.borrow());
            // #947: was `self.backend.borrow().char_width()` — the click
            // backend's OWN `current_char_width`, which nothing here ever
            // seeded (it stays at `GtkBackend::new()`'s hardcoded default,
            // 8.0px), not the width this frame actually painted the command
            // line with. That silently drifted from the real painted
            // `painted_char_width()` (#751's fix for the identical class of
            // bug elsewhere) — masked before #947 because the old
            // hardcoded-11pt paint's char width (~8.8px) was close enough to
            // the stale 8.0px default not to cross a column boundary at
            // small click offsets; #947 wiring the real (larger) default
            // `settings.font_size` (14pt, ~11px) through to paint widened
            // the gap enough to resolve clicks one column off.
            let char_width = self.painted_char_width() as f32;
            let rect = self.engine.borrow().command_line_rect.get();
            let point = quadraui::Point::new(x as f32, y as f32);
            if let Some(char_idx) =
                render::command_line_click_char_idx(rect, &data.text, char_width, point)
            {
                let mut engine = self.engine.borrow_mut();
                if matches!(
                    engine.mode,
                    crate::core::Mode::Command | crate::core::Mode::Search
                ) {
                    let buf_len = engine.command_buffer.chars().count();
                    engine.command_cursor = char_idx.saturating_sub(1).min(buf_len);
                }
                engine.cmd_sel.set(Some((char_idx, char_idx)));
                engine.cmd_dragging.set(true);
                drop(engine);
                self.draw_needed.set(true);
                return;
            }
        }

        // Debug toolbar click: resolve via cached ToolbarLayout on engine (#510).
        {
            let dbg_y = self.debug_toolbar_y_offset.get();
            let dbg_h = self.debug_toolbar_height.get();
            if dbg_h > 0.0 && y >= dbg_y && y < dbg_y + dbg_h {
                let idx = self.engine.borrow().debug_button_hit(x as f32, y as f32);
                self.engine.borrow_mut().debug_button_pressed = idx;
                self.draw_needed.set(true);
                if let Some(i) = idx {
                    if let Some(btn) = render::DEBUG_BUTTONS.get(i) {
                        let _ = self.engine.borrow_mut().execute_command(btn.action);
                        return;
                    }
                }
                return;
            }
        }

        // #733: the dialog rung moved to the shared modal-overlay router
        // at the top of this handler (`render::route_modal_overlay_click`),
        // which TUI's `handle_mouse` calls too. Control only reaches here
        // when no dialog is open, so what used to be the `else` arm of the
        // dialog block is now unconditional. The `ModalStack` push/pop dance
        // that arm maintained is gone with it: `DialogHit::Outside` already
        // answers the inside/outside question the stack round-trip was
        // recomputing.
        {
            // #752: the git-branch hit test that used to open this block —
            // ~60 lines re-deriving `build_status_line`'s own formatting, then
            // comparing a `cached_char_width`-derived column against a UTF-8
            // *byte* range — is now the global-status-bar rung of
            // `render::route_chrome_click`, called at the top of this handler.
            //
            // Clicking in the editor clears every sidebar's keyboard focus.
            // Without this, focus stays on whichever sidebar grabbed it last
            // (Source Control, Extensions, Settings, AI, DAP, …) and the
            // editor key handler keeps routing keys to that sidebar's
            // handler — so the editor "can't be interacted with" until the
            // user explicitly Escapes out of the sidebar. The DAP-only
            // version of this clear was incomplete; tracked all fields via
            // `clear_sidebar_focus()` instead.
            self.engine.borrow_mut().clear_sidebar_focus();
            // ── Bottom panel (tab strip / toolbar / terminal content) — #754 ──
            // Zone, split hit-test and pane-cell translation are all
            // `render::route_bottom_panel_click`, shared verbatim with TUI's
            // `handle_mouse`. What this replaced computed the pane column as a
            // bare `x / cached_char_width` against a *window-absolute* `x`,
            // while `render_content` paints the panel at the editor's left
            // edge — so with the sidebar open every terminal click landed
            // roughly `(activity_bar + sidebar) / char_width` columns right of
            // the glyph aimed at. `panel_left` is now a required input.
            let route = render::route_bottom_panel_click(
                &self.engine.borrow(),
                x,
                y,
                render::BottomPanelMetrics {
                    panel_left: self.painted_bottom_panel_left(),
                    col_width: self.cached_char_width.max(1.0),
                },
            );
            if let Some(route) = route {
                if !matches!(route, render::BottomPanelRoute::TabBar) {
                    self.terminal_resize_dragging = false;
                }
                let ctx = crate::core::engine::UiEventContext {
                    // #1058: was `self.terminal_cols()` (pinned at 80) — this
                    // handler already has the real live panel width, so use
                    // it. This is what feeds `ToggleSplit`'s initial
                    // full_cols when opening a split.
                    terminal_cols: self.terminal_panel_cols(width),
                    terminal_max_rows: self.terminal_target_maximize_rows(),
                };
                let effect =
                    render::apply_bottom_panel_route(&mut self.engine.borrow_mut(), route, x, ctx);
                self.terminal_split_dragging |= effect.split_drag;
                self.terminal_resize_dragging |= effect.resize_drag;
                if effect.relayout {
                    self.handle_resize();
                    return;
                }
                self.draw_needed.set(true);
            } else {
                {
                    let mut engine = self.engine.borrow_mut();
                    // Clicking outside the terminal panel returns focus to the editor.
                    engine.terminal_has_focus = false;
                }

                // Dropdown clicks are fully handled by the menu_dropdown_da overlay
                // widget (which has can_target=true while a menu is open).
                // If we reach here, no menu is open and we proceed with normal handling.

                // ── H scrollbar hit-test (before editor click) ────────────────
                // If the click lands on a Cairo h scrollbar:
                //   - on the thumb → start a DragTarget::ScrollbarX drag.
                //   - on the empty track → page-jump toward the click.
                // Either way, consume the click.
                {
                    let lh = self.cached_line_height;
                    let cw = self.cached_char_width;
                    let engine = self.engine.borrow();
                    let rects = compute_editor_window_rects(&engine, width, height, lh);
                    if let Some((win_id, scroll_left)) =
                        h_scrollbar_hit_test(&engine, x, y, &rects, cw, lh)
                    {
                        let win_rect = rects.iter().find(|(id, _)| *id == win_id).map(|(_, r)| *r);
                        let geom = win_rect
                            .and_then(|rect| h_scrollbar_geometry(&engine, win_id, &rect, cw, lh));
                        drop(engine);
                        if let Some((
                            track_x,
                            _ty,
                            track_w,
                            _sb_h,
                            thumb_x,
                            thumb_w,
                            scroll_range,
                            _,
                        )) = geom
                        {
                            let max_scroll = scroll_range.round() as usize;
                            let page_cols = (track_w / cw).floor() as usize;
                            // #1061: shared with TUI's own h/v scrollbar
                            // click handlers (`tui_main/mouse.rs`) via
                            // `render::resolve_editor_scrollbar_click` —
                            // see that function's doc for the full
                            // rationale.
                            match render::resolve_editor_scrollbar_click(
                                x as f32,
                                thumb_x as f32,
                                (thumb_x + thumb_w) as f32,
                                page_cols,
                                max_scroll,
                                scroll_left,
                            ) {
                                render::EditorScrollbarClick::PageTo(new_left) => {
                                    let mut engine = self.engine.borrow_mut();
                                    engine.set_scroll_left_for_window(win_id, new_left);
                                    self.draw_needed.set(true);
                                    return;
                                }
                                render::EditorScrollbarClick::BeginDrag { grab_offset } => {
                                    let drag_rc = self.backend.borrow().drag_state_handle();
                                    drag_rc
                                        .borrow_mut()
                                        .begin(quadraui::DragTarget::ScrollbarX {
                                            widget: quadraui::WidgetId::new(format!(
                                                "editor:h_sb:{}",
                                                win_id.0
                                            )),
                                            track_start: track_x as f32,
                                            track_length: track_w as f32,
                                            thumb_length: thumb_w as f32,
                                            max_scroll,
                                            grab_offset,
                                            inverted: false,
                                        });
                                    self.draw_needed.set(true);
                                    return;
                                }
                            }
                        }
                    }
                }

                // ── V scrollbar hit-test (before divider) — #1026/#987 ────────
                // Mirrors the H-scrollbar rung immediately above:
                //   - on the thumb → start a DragTarget::ScrollbarY drag.
                //   - on the empty track → page-jump toward the click.
                // Either way, consume the click *before* the divider hit-test
                // below gets a look. Without this rung, `handle_mouse_click_msg`
                // had no vertical-scrollbar hit-test at all, so a click on a
                // window's own scrollbar column (the last `cell_width` before
                // its edge, painted since quadraui#968) fell straight through
                // to `route_divider_grab` — inert on any window, and silently
                // resizing the split for any window whose scrollbar-adjacent
                // side happened to sit inside the divider's own grab margin.
                //
                // Window rects come from `self.painted_editor_bounds()` (the
                // same cached, already-painted `content_bounds`/`tab_bar_h`
                // the divider rung below reads via `painted_divider_geometry`)
                // rather than `compute_editor_window_rects`'s `width`/`height`
                // recompute: that helper always assumes the editor area starts
                // at `x = 0`, which only holds with the activity bar/sidebar at
                // zero width. With either painted, its real left edge is
                // `AppShellLayout::main_content_bounds.x`, so a rect rebuilt
                // from `(0, 0, width, height)` lands columns off from what was
                // actually drawn — verified while building this rung: TUI's
                // own conformance fixture (activity bar always reserves a
                // real column, no sidebar needed to see it) reproduced exactly
                // that drift.
                if let Some((content_bounds, tab_bar_h)) = self.painted_editor_bounds() {
                    let lh = self.cached_line_height;
                    let cw = self.cached_char_width;
                    let engine = self.engine.borrow();
                    let (rects, _dividers) =
                        engine.calculate_group_window_rects(content_bounds, tab_bar_h);
                    if let Some((win_id, scroll_top)) =
                        v_scrollbar_hit_test(&engine, x, y, &rects, cw, lh)
                    {
                        let win_rect = rects.iter().find(|(id, _)| *id == win_id).map(|(_, r)| *r);
                        let geom = win_rect
                            .and_then(|rect| v_scrollbar_geometry(&engine, win_id, &rect, cw, lh));
                        drop(engine);
                        if let Some((
                            _track_x,
                            track_y,
                            _track_w,
                            track_h,
                            thumb_y,
                            thumb_h,
                            scroll_range,
                            _,
                        )) = geom
                        {
                            let max_scroll = scroll_range.round() as usize;
                            let page_rows = (track_h / lh.max(1.0)).floor() as usize;
                            // #1061: shared with TUI's own h/v scrollbar
                            // click handlers (`tui_main/mouse.rs`) via
                            // `render::resolve_editor_scrollbar_click` —
                            // see that function's doc for the full
                            // rationale.
                            match render::resolve_editor_scrollbar_click(
                                y as f32,
                                thumb_y as f32,
                                (thumb_y + thumb_h) as f32,
                                page_rows,
                                max_scroll,
                                scroll_top,
                            ) {
                                render::EditorScrollbarClick::PageTo(new_top) => {
                                    let mut engine = self.engine.borrow_mut();
                                    engine.set_scroll_top_for_window(win_id, new_top);
                                    engine.sync_scroll_binds();
                                    self.draw_needed.set(true);
                                    return;
                                }
                                render::EditorScrollbarClick::BeginDrag { grab_offset } => {
                                    let drag_rc = self.backend.borrow().drag_state_handle();
                                    drag_rc
                                        .borrow_mut()
                                        .begin(quadraui::DragTarget::ScrollbarY {
                                            widget: quadraui::WidgetId::new(format!(
                                                "editor:v_sb:{}",
                                                win_id.0
                                            )),
                                            track_start: track_y as f32,
                                            track_length: track_h as f32,
                                            thumb_length: thumb_h as f32,
                                            max_scroll,
                                            grab_offset,
                                            inverted: false,
                                        });
                                    self.draw_needed.set(true);
                                    return;
                                }
                            }
                        }
                    }
                }

                // ── Divider hit-test (#753 shared rung) ───────────────────────
                // Editor-group boundaries then `:split`/`:vsplit` boundaries
                // (#582), sequenced by `render::route_divider_grab`. GTK's only
                // contribution is its own painted geometry and its own grab
                // margin — a symmetric 6px around the thin drawn line, against
                // continuous positions (`quantize: false`); TUI's cell metrics
                // differ, the ordering does not.
                if let Some((group_dividers, window_dividers, on_tab_bar)) =
                    self.painted_divider_geometry(x, y)
                {
                    if let Some(grab) = render::route_divider_grab(
                        &render::DividerState {
                            group_dividers: &group_dividers,
                            window_dividers: &window_dividers,
                            metrics: render::GTK_DIVIDER_METRICS,
                            on_tab_bar,
                        },
                        x,
                        y,
                    ) {
                        self.divider_grab = Some(grab);
                        return;
                    }
                }

                {
                    let mut engine = self.engine.borrow_mut();

                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    let (click_result, engine_action) = {
                        let layout_ref = self.cached_screen_layout.borrow();
                        if let Some(ref layout) = *layout_ref {
                            handle_mouse_click(
                                &mut engine,
                                &**self.backend.borrow(),
                                x,
                                y,
                                alt,
                                self.cached_line_height,
                                self.cached_char_width,
                                layout,
                                &self.cached_tab_pixel_hits.borrow(),
                                self.cached_frame_hit_map.borrow().as_ref(),
                                &self.cached_tab_bar_zones.borrow(),
                            )
                        } else {
                            (None, None)
                        }
                    };
                    match engine_action {
                        Some(core::engine::EngineAction::ToggleSidebar) => {
                            drop(engine);
                            self.sync_sidebar_from_engine();
                            return;
                        }
                        Some(core::engine::EngineAction::OpenTerminal) => {
                            // Create the terminal tab immediately (not via
                            // the deferred `DeferredAction::ToggleTerminal`)
                            // so the panel appears on this same draw cycle.
                            let cols = self.terminal_cols();
                            let rows = engine.session.terminal_panel_rows;
                            engine.terminal_new_tab(cols, rows);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        _ => {}
                    }
                    match click_result {
                        Some(true) => {
                            drop(engine);
                            self.show_close_tab_confirm();
                            self.draw_needed.set(true);
                            return;
                        }
                        Some(false) => {
                            // Buffer click — fire hooks and reveal file
                        }
                        None => {
                            // Engine-drawn action menu is already opened with the
                            // correct anchor by click.rs::handle_mouse_click. The
                            // engine-drawn renderer at draw.rs:906 + click dispatch
                            // at line ~6022 take over from here (#395).
                            if engine.context_menu.as_ref().is_some_and(|cm| {
                                matches!(
                                    cm.target,
                                    core::engine::ContextMenuTarget::EditorActionMenu { .. }
                                )
                            }) {
                                drop(engine);
                                self.draw_needed.set(true);
                                return;
                            }
                            // Tab bar / split button click — skip hooks.
                            // Record drag start position for tab drag-and-drop.
                            self.tab_drag.arm(x, y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                    }

                    // Fire cursor_move hook so plugins (e.g. git-insights blame)
                    // see the new cursor position after a mouse click.
                    engine.fire_cursor_move_hook();
                    drop(engine);
                    self.draw_needed.set(true);
                }
            }
        } // close else (dialog not open)
    }

    /// Line height the last frame actually painted with, falling back to the
    /// `setup()`-seeded `cached_line_height` before the first frame.
    ///
    /// Every click hit-test that measures *painted* geometry must use this
    /// rather than `cached_line_height` (#555) — see the note where
    /// `render_content` publishes it.
    fn painted_line_height(&self) -> f64 {
        self.painted_line_height
            .get()
            .unwrap_or(self.cached_line_height)
            .max(1.0)
    }

    /// Assemble this backend's [`render::ChromeState`] from the geometry the
    /// last frame actually painted, run the shared chrome rung over it, and
    /// apply whatever it decides. Returns `true` when the event was consumed.
    ///
    /// Every rect fed in here is a *painted* one — `status_segment_map` and
    /// `separated_status_bar_rect` are filled by `render_content` from the
    /// same `Surface::StatusBar` rects it draws, `global_status_rect` likewise
    /// (#752), and the breadcrumb bars carry their own draw-time layout. That
    /// is the #555 rule: never hit-test against freshly recomputed geometry.
    fn route_and_apply_chrome_click(
        &mut self,
        x: f64,
        y: f64,
        action: render::ChromeMouseAction,
    ) -> bool {
        let lh = self.painted_line_height();

        let layout_ref = self.cached_screen_layout.borrow();
        let Some(ref screen) = *layout_ref else {
            return false;
        };
        let engine = self.engine.borrow();
        let segment_map = self.status_segment_map.borrow();

        // The separated status line is listed first: it is painted in its own
        // full-width band *outside* every window's rect, so it can never be
        // reached through the per-window bars' geometry, and a click in that
        // band must not fall through to whatever sits underneath it.
        let mut bands: Vec<render::StatusBand<'_>> = Vec::new();
        if let Some(rect) = self.separated_status_bar_rect.get() {
            if let Some(zones) = segment_map.get(&screen.active_window_id.0) {
                bands.push(render::StatusBand { rect, zones });
            }
        }
        for rw in &screen.windows {
            if rw.status_line.is_none() || rw.rect.height <= lh {
                continue;
            }
            let Some(zones) = segment_map.get(&rw.window_id.0) else {
                continue;
            };
            // The status line occupies the window's bottom row — the same
            // `rect.height - lh` `render_content` subtracts before painting it.
            bands.push(render::StatusBand {
                rect: quadraui::Rect::new(
                    rw.rect.x as f32,
                    (rw.rect.y + rw.rect.height - lh) as f32,
                    rw.rect.width as f32,
                    lh as f32,
                ),
                zones,
            });
        }

        // The global bar last, spatially and in arbitration: it is the bottom
        // band of the shell, below every window.
        let global_rect = engine.global_status_rect.get();
        let global_zones;
        if global_rect.width > 0.0 && global_rect.height > 0.0 {
            global_zones = self.global_status_zones.borrow().clone();
            bands.push(render::StatusBand {
                rect: global_rect,
                zones: &global_zones,
            });
        }

        // The same shared hit test, with the same tolerances, the window-split
        // divider rung in `handle_mouse_click_msg` runs — see
        // `render::ChromeState::on_window_divider` (#582/#752).
        let on_window_divider =
            self.painted_editor_bounds()
                .is_some_and(|(content_bounds, tab_bar_h)| {
                    let (window_rects, _) =
                        engine.calculate_group_window_rects(content_bounds, tab_bar_h);
                    render::divider_hit_test(
                        &engine.calculate_window_dividers(&window_rects),
                        x,
                        y,
                        (6.0, 6.0),
                        (6.0, 6.0),
                        false,
                    )
                    .is_some()
                });

        let route = render::route_chrome_click(
            &render::ChromeState {
                breadcrumbs_enabled: engine.settings.breadcrumbs,
                breadcrumbs: &screen.breadcrumbs,
                line_height: lh,
                status_bands: &bands,
                on_window_divider,
            },
            action,
            x,
            y,
        );

        drop(segment_map);
        drop(engine);
        drop(layout_ref);

        match route {
            render::ChromeRoute::None => return false,
            render::ChromeRoute::Breadcrumb { group_id, idx } => {
                self.engine
                    .borrow_mut()
                    .handle_breadcrumb_click(group_id, idx);
            }
            render::ChromeRoute::StatusAction(action) => {
                let cols = self.terminal_cols();
                let follow_up =
                    render::apply_status_action(&mut self.engine.borrow_mut(), &action, cols);
                if matches!(
                    follow_up,
                    Some(crate::core::engine::EngineAction::ToggleSidebar)
                ) {
                    self.sync_sidebar_from_engine();
                }
            }
            render::ChromeRoute::BreadcrumbBar | render::ChromeRoute::StatusBar => {}
        }
        self.draw_needed.set(true);
        true
    }

    /// Character-cell advance the last frame actually painted with — the
    /// horizontal twin of [`Self::painted_line_height`]. See the field's doc
    /// for why `cached_char_width` is the wrong number at click time (#751).
    fn painted_char_width(&self) -> f64 {
        self.painted_char_width
            .get()
            .unwrap_or(self.cached_char_width)
            .max(1.0)
    }

    /// Compute the picker popup's bounds in DA-local pixels. Shared by
    /// the click handler (to push into the modal stack) and the drag
    /// guard (to decide if a drag started inside the popup).
    ///
    /// Prefers the rect the last frame **actually painted**
    /// (`picker_popup_rect`), and only re-derives from `width`/`height` when
    /// no frame has painted the picker yet.
    ///
    /// #555: re-deriving was wrong on two counts, and together they put the
    /// hit rect in a different place than the pixels. `render_content` centres
    /// the popup in `backend.viewport()` (the whole window) at
    /// `gtk_picker_sizing(line_height)`, whereas both callers here pass the
    /// `width`/`height` of `ctx.layout.main_content_bounds` — the editor area
    /// only, minus activity bar / sidebar / title bar — anchored at `(0, 0)`,
    /// and a `line_h: 1.0, header_h: 0.0` sizing. So with any shell chrome
    /// present the modal rect pushed onto the `ModalStack` was both offset and
    /// differently sized from the visible popup: clicks on the painted
    /// dropdown either missed the modal entirely or resolved to the wrong
    /// result row. That is what made the breadcrumb dropdown look inert once
    /// it finally started painting.
    fn compute_picker_popup_bounds(&self, width: f64, height: f64) -> quadraui::Rect {
        if let Some(rect) = self.picker_popup_rect.get() {
            return rect;
        }
        let engine = self.engine.borrow();
        let has_preview = engine.picker_preview.is_some();
        drop(engine);
        let sizing = render::PickerSizing {
            header_h: 0.0,
            line_h: 1.0,
            ..render::gtk_picker_sizing(1.0)
        };
        let geo =
            render::PickerGeometry::compute(width as f32, height as f32, has_preview, &sizing);
        quadraui::Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h)
    }

    // ── Drag-follow-through rung (#756, mouse-ladder slice 6) ────────────────
    //
    // Which gesture owns a move-with-the-button-held is
    // `render::route_mouse_drag`, sequenced ONCE and shared verbatim with TUI's
    // `handle_mouse`. This backend used to state its own order here — armed
    // scrollbar → hover popup → modal swallow → tab drag → divider → split →
    // resize → terminal → editor — while TUI stated a different one, and each
    // knew scrollbar widget ids the other did not. See the rung's banner in
    // `render.rs`.
    fn handle_mouse_drag_msg(&mut self, x: f64, y: f64, width: f64, height: f64) {
        // Keep the picker's modal-stack entry fresh before anything hit-tests
        // the stack: the popup's size depends on `has_preview`, which can change
        // mid-picker.
        let picker_open = self.engine.borrow().picker_open;
        {
            let picker_id = quadraui::WidgetId::new("picker");
            let stack_rc = self.backend.borrow().modal_stack_handle();
            let mut stack = stack_rc.borrow_mut();
            if picker_open {
                let rect = self.compute_picker_popup_bounds(width, height);
                stack.push(picker_id, rect);
            } else {
                stack.pop(&picker_id);
            }
        }

        let bottom_metrics = render::BottomPanelMetrics {
            panel_left: self.painted_bottom_panel_left(),
            col_width: self.cached_char_width.max(1.0),
        };
        let drag_rc = self.backend.borrow().drag_state_handle();
        let stack_rc = self.backend.borrow().modal_stack_handle();
        let route = {
            let engine = self.engine.borrow();
            let layout_ref = self.cached_screen_layout.borrow();
            let state = render::MouseDragState {
                layout: layout_ref.as_ref(),
                armed_target: render::drag_state_arms_scrollbar(&drag_rc.borrow()),
                hover_popup_selecting: engine.editor_hover_has_focus
                    && engine
                        .editor_hover
                        .as_ref()
                        .is_some_and(|h| h.selection.is_some())
                    && self.editor_hover_popup_rect.get().is_some(),
                modal_hit: stack_rc
                    .borrow()
                    .hit_test(quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    })
                    .is_some(),
                // GTK has no canvas sidebar separator or explorer drag-and-drop:
                // the separator is a `gtk::Paned` and the file tree is a native
                // widget with its own DnD. Stated here rather than omitted so
                // the asymmetry is visible at the call site.
                //
                // Command-line selection *is* shared now (#816):
                // `engine.cmd_dragging` is armed by `handle_mouse_click_msg`'s
                // press rung the same way TUI's `mouse::handle_mouse` arms its
                // local `cmd_dragging` — quadraui#705's `CommandLineLayout`
                // closed the "no character hit test" gap the old comment here
                // recorded.
                sidebar_resizing: false,
                sidebar_dnd: false,
                sidebar_body: None,
                command_line_selecting: engine.cmd_dragging.get(),
                tab_dragging: self.tab_drag.is_armed_or_dragging(),
                divider_grabbed: self.divider_grab.is_some(),
                terminal_split_dragging: self.terminal_split_dragging,
                terminal_panel_resizing: self.terminal_resize_dragging,
                // #756 review: mirrors TUI's guard — see the field's doc
                // comment in `render.rs`. GTK's `EditorText` arm doesn't run
                // through the shared `DragState`, but it drives the same
                // `Engine::mouse_drag`, so `mouse_drag_active` is just as
                // valid a "already extending" signal here.
                text_selection_active: engine.mouse_drag_active,
                in_terminal_content: render::in_terminal_pane_content(
                    &engine,
                    x,
                    y,
                    bottom_metrics,
                ),
                cell: (
                    self.cached_char_width.max(1.0),
                    self.cached_line_height.max(1.0),
                ),
            };
            render::route_mouse_drag(&state, x, y)
        };

        match route {
            render::MouseDragRoute::ArmedTarget => {
                let events = quadraui::dispatch_mouse_drag(
                    &drag_rc.borrow(),
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    Default::default(),
                );
                let picker_visible_rows = if picker_open {
                    let lh = self.cached_line_height.max(1.0);
                    let has_preview = self.engine.borrow().picker_preview.is_some();
                    render::PickerGeometry::compute(
                        width as f32,
                        height as f32,
                        has_preview,
                        &render::gtk_picker_sizing(lh as f32),
                    )
                    .visible_rows
                } else {
                    0
                };
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                        // #756: the widget-id → scroll-state table is
                        // `render::apply_scroll_offset`, shared with TUI. The
                        // copy this replaced knew `picker` and `editor:h_sb:N`
                        // and nothing else — see the rung's banner in
                        // `render.rs`, point 2, for why two half-tables is a
                        // silent trap rather than a live bug.
                        render::apply_scroll_offset(
                            &mut self.engine.borrow_mut(),
                            widget.as_str(),
                            *new_offset,
                            render::ScrollApplyContext {
                                picker_visible_rows,
                            },
                        );
                    }
                }
            }
            render::MouseDragRoute::HoverPopupSelection => {
                if let Some(quadraui::Rect { x: px, y: py, .. }) =
                    self.editor_hover_popup_rect.get()
                {
                    let px = px as f64;
                    let py = py as f64;
                    let padding = 4.0;
                    let lh = self.cached_line_height.max(1.0);
                    let scroll = self
                        .engine
                        .borrow()
                        .editor_hover
                        .as_ref()
                        .map(|h| h.scroll_top)
                        .unwrap_or(0);
                    let rel_x = x - px - padding;
                    let rel_y = y - py - padding;
                    let content_line = (rel_y / lh).max(0.0) as usize + scroll;
                    let content_col = self.pixel_to_editor_hover_col(rel_x, content_line);
                    self.engine
                        .borrow_mut()
                        .editor_hover_extend_selection(content_line, content_col);
                }
            }
            render::MouseDragRoute::TabDrag => {
                // `64.0` is the squared 8-device-pixel threshold.
                match self.tab_drag.handle_move(x, y, 64.0) {
                    render::TabDragMove::Tracking => {
                        // Cursor and the cached per-group bounds are both in
                        // absolute surface coordinates, so the hit-test matches
                        // what the overlay draws (#515).
                        let groups = self.cached_drop_groups.borrow();
                        let zone = render::compute_tab_drop_zone(
                            x as f32,
                            y as f32,
                            &groups,
                            self.cached_drop_tbh.get(),
                        );
                        drop(groups);
                        self.tab_drag.track(zone);
                    }
                    render::TabDragMove::Crossed { press_x, press_y } => {
                        // Unlike TUI, this backend's arm fires for the whole
                        // tab-bar band, so the press has to be re-resolved to
                        // confirm it was on a tab. If it was not, disarm and
                        // re-route the same event with the machine idle — the
                        // one rung that can decline after being asked.
                        if let Some(source) = self.tab_drag_source_at(press_x, press_y) {
                            self.tab_drag.begin(source, x, y);
                        } else {
                            self.tab_drag.disarm();
                            self.draw_needed.set(true);
                            self.handle_mouse_drag_msg(x, y, width, height);
                            return;
                        }
                    }
                    render::TabDragMove::Pending | render::TabDragMove::Idle => {}
                }
            }
            render::MouseDragRoute::Divider => {
                if let (Some(grab), Some((group_dividers, window_dividers, _))) =
                    (self.divider_grab, self.painted_divider_geometry(x, y))
                {
                    render::apply_divider_drag(
                        &mut self.engine.borrow_mut(),
                        grab,
                        &group_dividers,
                        &window_dividers,
                        x,
                        y,
                    );
                }
            }
            render::MouseDragRoute::TerminalSplitDivider => {
                if self.cached_char_width > 0.0 {
                    let min_x = self.cached_char_width * 5.0;
                    let max_x = (width - Self::TERMINAL_PANEL_SB_W - self.cached_char_width * 5.0)
                        .max(min_x);
                    let clamped_x = x.clamp(min_x, max_x);
                    let left_cols = (clamped_x / self.cached_char_width) as u16;
                    self.engine
                        .borrow_mut()
                        .terminal_split_set_drag_cols(left_cols);
                }
            }
            render::MouseDragRoute::TerminalPanelResize => {
                if self.cached_line_height > 0.0 {
                    let global_status_rows = if self.engine.borrow().settings.window_status_line {
                        0.0
                    } else {
                        1.0
                    };
                    let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                    let available = (height - y - status_h).max(0.0);
                    // Leave at least 4 editor lines visible (+ tab bar chrome)
                    let min_editor_lines = 4.0 + 1.0;
                    let max_rows =
                        ((height - status_h - min_editor_lines * self.cached_line_height)
                            / self.cached_line_height) as u16;
                    let max_rows = max_rows.saturating_sub(2).max(5);
                    let new_rows = ((available / self.cached_line_height) as u16)
                        .saturating_sub(2)
                        .clamp(5, max_rows);
                    self.engine.borrow_mut().session.terminal_panel_rows = new_rows;
                }
            }
            render::MouseDragRoute::Minimap => {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    render::apply_minimap_click(&mut engine, layout, x, y);
                }
            }
            render::MouseDragRoute::TerminalContent => {
                // #533: shared drag handler — tries forward_mouse(Move) when the
                // child has mouse reporting, falls back to local selection.
                render::apply_terminal_content_drag(
                    &mut self.engine.borrow_mut(),
                    x,
                    y,
                    bottom_metrics,
                );
            }
            render::MouseDragRoute::EditorText => {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    handle_mouse_drag(
                        &mut engine,
                        &**self.backend.borrow(),
                        x,
                        y,
                        // #947/#555: was `self.cached_line_height`/
                        // `self.cached_char_width` — those are seeded once
                        // in `setup()` (before the runner's first real
                        // font-metrics measurement ever runs) and only
                        // refreshed by `tick_dispatch`/`WindowResized`, so a
                        // driver that never fires a tick between `setup()`
                        // and a drag (every headless `GtkDriver` test) reads
                        // them permanently stale. `painted_line_height()`/
                        // `painted_char_width()` are #555's fix for exactly
                        // this class of bug — set fresh every
                        // `render_content` frame — and every OTHER
                        // painted-geometry hit-test in this file already
                        // uses them; this arm was the one holdout.
                        self.painted_line_height(),
                        self.painted_char_width(),
                        layout,
                        &self.cached_tab_pixel_hits.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                    );
                }
            }
            render::MouseDragRoute::CommandLine => {
                // #816: same `command_line_click_char_idx` helper the press
                // rung uses — extends `cmd_sel`'s head via
                // `CommandLineLayout::hit_test`, mirroring TUI's identical
                // drag arm in `mouse::handle_mouse`.
                if let Some(mut sel) = self.engine.borrow().cmd_sel.get() {
                    let data = render::build_command_line(&self.engine.borrow());
                    // #947: same fix as the press rung above — use the real
                    // painted char width, not the click backend's never-seeded
                    // `current_char_width` default.
                    let char_width = self.painted_char_width() as f32;
                    let rect = self.engine.borrow().command_line_rect.get();
                    let point = quadraui::Point::new(x as f32, y as f32);
                    if let Some(char_idx) =
                        render::command_line_click_char_idx(rect, &data.text, char_width, point)
                    {
                        sel.1 = char_idx;
                        self.engine.borrow().cmd_sel.set(Some(sel));
                    }
                }
            }
            // #192: a drag inside an open modal with nothing armed is swallowed
            // so it cannot leak to the editor underneath.
            render::MouseDragRoute::ModalSwallow
            | render::MouseDragRoute::SidebarResize
            | render::MouseDragRoute::SidebarBody
            | render::MouseDragRoute::None => {}
        }
        self.draw_needed.set(true);
    }

    /// `width` is the live terminal-panel pixel width — the same
    /// `ctx.layout.main_content_bounds.width` `UiEvent::MouseMoved` already
    /// threads into `handle_mouse_drag_msg` — needed to finalize a
    /// terminal-split divider drag with the real pixel→cell conversion
    /// instead of a fixed guess (#1058).
    fn handle_mouse_up_msg(&mut self, width: f64) {
        // Clear debug toolbar pressed state (#510).
        if self.engine.borrow().debug_button_pressed.is_some() {
            self.engine.borrow_mut().debug_button_pressed = None;
            self.draw_needed.set(true);
        }

        // Phase B.4: clear any active cross-backend drag state. The
        // dispatcher returns a MouseUp event we could forward to the
        // engine later, but today no consumer cares about mouse-up
        // beyond clearing drag state.
        {
            let drag_rc = self.backend.borrow().drag_state_handle();
            let mut drag = drag_rc.borrow_mut();
            if drag.is_active() {
                let stack_rc = self.backend.borrow().modal_stack_handle();
                let stack = stack_rc.borrow();
                let _events = quadraui::dispatch_mouse_up(
                    &stack,
                    &mut drag,
                    quadraui::Point { x: 0.0, y: 0.0 },
                    quadraui::MouseButton::Left,
                );
            }
        }

        // Tab drag drop (#753 — the same `handle_release` TUI calls; it also
        // clears any armed-but-never-dragged press, which is what the bare
        // `tab_drag_start = None` this replaced was for).
        if self.tab_drag.handle_release(&mut self.engine.borrow_mut()) {
            self.draw_needed.set(true);
        }
        if self.terminal_split_dragging {
            self.terminal_split_dragging = false;
            if self.cached_char_width > 0.0 {
                let engine = self.engine.borrow();
                let left_cols = if engine.terminal_split_left_cols > 0 {
                    engine.terminal_split_left_cols
                } else if !engine.terminal_panes.is_empty() {
                    engine.terminal_panes[0].session.cols()
                } else {
                    0
                };
                let rows = engine.session.terminal_panel_rows;
                drop(engine);
                if left_cols > 0 {
                    // #1058: was `let da_w = 800.0;` — a fixed guess that
                    // only produced the right column count in a window that
                    // happened to be exactly 800px wide. `width` is the real
                    // live panel width the caller (`UiEvent::MouseUp`) reads
                    // off `ctx.layout.main_content_bounds`, same source
                    // `handle_mouse_drag_msg`'s `TerminalSplitDivider` arm
                    // already uses while the drag is in progress.
                    let total_cols = self.terminal_panel_cols(width);
                    let right_cols = total_cols.saturating_sub(left_cols);
                    self.engine
                        .borrow_mut()
                        .terminal_split_finalize_drag(left_cols, right_cols, rows);
                }
            }
        }
        if self.terminal_resize_dragging {
            self.terminal_resize_dragging = false;
            let rows = self.engine.borrow().session.terminal_panel_rows;
            // #731: was `if let Some(da) = self.drawing_area…`, permanently
            // `None` under the ShellApp runner — see `terminal_cols`.
            let cols = self.terminal_cols();
            self.engine.borrow_mut().terminal_resize(cols, rows);
            let _ = self.engine.borrow().session.save();
        }
        self.divider_grab = None;
        self.engine.borrow().cmd_dragging.set(false);
        {
            let mut engine = self.engine.borrow_mut();
            engine.mouse_drag_active = false;
            engine.mouse_drag_origin_window = None;
            // #533: auto-copy terminal selection on mouse-release, mirroring
            // TUI.  terminal_autocopy_selection() is a no-op when the
            // terminal isn't focused or has no selection.
            engine.terminal_autocopy_selection();
        }
        self.draw_needed.set(true);
    }

    /// Toggle the integrated terminal panel open/closed.
    fn toggle_terminal(&mut self) {
        let needs_new_tab = {
            let engine = self.engine.borrow();
            (!engine.terminal_open || !engine.terminal_has_focus)
                && engine.terminal_panes.is_empty()
        };
        if needs_new_tab {
            // Use the actual drawing area width so the PTY matches the visible panel.
            let cols = self.terminal_cols();
            let rows = self.engine.borrow().session.terminal_panel_rows;
            self.engine.borrow_mut().terminal_new_tab(cols, rows);
        } else {
            self.engine.borrow_mut().toggle_terminal();
        }
        self.draw_needed.set(true);
    }

    /// Toggle the "terminal maximized" state (panel fills editor area).
    fn toggle_terminal_maximize(&mut self) {
        // Phase B.2: route through engine's UiEvent dispatch — same
        // path as the keybinding above + the EngineAction handler
        // + the toolbar click handler.
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: self.terminal_cols(),
            terminal_max_rows: self.terminal_target_maximize_rows(),
        };
        self.engine.borrow_mut().handle_ui_event(
            crate::core::engine::UiEvent::Accelerator(
                crate::core::engine::AcceleratorId::new("terminal.toggle_maximize"),
                quadraui::Modifiers::default(),
            ),
            ctx,
        );
        self.draw_needed.set(true);
    }

    /// Open a new terminal tab rooted at `dir`.
    fn open_terminal_at(&mut self, dir: PathBuf) {
        let cols = self.terminal_cols();
        let rows = self.engine.borrow().session.terminal_panel_rows;
        self.engine
            .borrow_mut()
            .terminal_new_tab_at(cols, rows, Some(&dir));
        self.draw_needed.set(true);
    }

    /// Run `cmd` in a visible terminal pane (used for extension installs).
    fn run_command_in_terminal(&mut self, cmd: String) {
        let cols = self.terminal_cols();
        let rows = self.engine.borrow().session.terminal_panel_rows;
        self.engine
            .borrow_mut()
            .terminal_run_command(&cmd, cols, rows);
        self.draw_needed.set(true);
    }

    /// #731: was `if let Some(ref da) = *self.menu_dropdown_da.borrow()`
    /// — that field is permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has been a no-op since #540. The menu
    /// bar is repainted every frame by `render_content` from engine state
    /// instead (see the `ActivityBarActivation::MenuToggled` comment).
    /// Kept as a named no-op so its two call sites stay self-documenting.
    fn sync_menu_overlay(&self) {}

    /// Dispatch a menu action by command string, as produced by
    /// `quadraui::MenuEvent::Activated`.
    fn handle_menu_action(&mut self, action: String) {
        match action.as_str() {
            "open_file_dialog" => {
                self.open_file_dialog();
            }
            "open_folder_dialog" => {
                self.open_folder_dialog();
            }
            "open_workspace_dialog" => {
                self.engine.borrow_mut().open_workspace_from_file();
                self.refresh_file_tree();
            }
            "save_workspace_as_dialog" => {
                self.save_workspace_as_dialog();
            }
            "openrecent" => {
                self.open_recent_dialog();
            }
            "find" => {
                self.engine.borrow_mut().open_find_replace();
                self.draw_needed.set(true);
            }
            "quit_menu" => {
                if self.engine.borrow().has_any_unsaved() {
                    self.show_quit_confirm();
                } else {
                    self.save_session_and_exit();
                }
            }
            // #1063: used to restate a subset of `dispatch_engine_action`'s
            // match by hand (four variants named explicitly behind a bare
            // `_ => {}`) instead of calling it — exactly the shape that hid
            // #984 for months, since a menu item wired to a fifth variant
            // nobody had added an arm for would silently no-op instead of
            // failing to compile. `dispatch_engine_action(_, false)` is
            // exhaustive (via `render::apply_engine_action`, #1063) and a
            // menu activation is never a macro, so this is the same
            // behavior for every variant this catch-all used to name, plus
            // real handling — not a silent no-op — for every one it didn't.
            _ => {
                let engine_action = self.engine.borrow_mut().dispatch_menu_action(&action);
                self.dispatch_engine_action(engine_action, false);
            }
        }
        self.sync_menu_overlay();
        self.draw_needed.set(true);
    }

    /// Effective sidebar visibility — reads directly from
    /// `engine.app_shell` (owned by quadraui per #385). Replaces the
    /// former `App.sidebar_visible` local cache so GTK and engine state
    /// can never drift.
    fn current_sidebar_visible(&self) -> bool {
        self.engine.borrow().app_shell.sidebar_visible()
    }

    /// Effective active panel id, accounting for ext-panel synthetic IDs.
    /// #823 item 7: was its own restatement of `render::sidebar_owner`'s
    /// resolution; now just that plus
    /// `SidebarOwner::panel_id_string` (see its doc comment).
    fn current_active_panel_id(&self) -> String {
        render::sidebar_owner(&self.engine.borrow()).panel_id_string()
    }

    /// Re-sync GTK widget tree from engine sidebar state. Was previously
    /// `sync_sidebar_from_engine` which copied into local cache fields;
    /// the cache is gone (engine.app_shell is the single source of truth)
    /// so this is now just a redraw trigger.
    fn sync_sidebar_from_engine(&mut self) {
        self.sync_sidebar_widgets();
    }

    /// Queue a redraw after sidebar visibility/focus state changes.
    ///
    /// Used to update GTK widget visibility (revealer + panel boxes) and
    /// grab focus on the active panel DA under the pre-#540 Relm4 widget
    /// tree. Under the ShellApp runner there is no such widget tree to
    /// sync — `render_content` repaints the whole sidebar from
    /// `engine.app_shell` every frame — so this is now just the redraw
    /// trigger (#731).
    fn sync_sidebar_widgets(&mut self) {
        self.draw_needed.set(true);
    }

    /// Toggle sidebar visibility.
    fn toggle_sidebar_panel(&mut self) {
        self.engine.borrow_mut().toggle_sidebar();
        self.sync_sidebar_from_engine();
    }

    /// Switch the sidebar to a different panel.
    ///
    /// #754: the ext-panel-vs-built-in bookkeeping this used to spell out is
    /// `render::apply_activity_panel_switch`, shared with TUI's activity-bar
    /// arm. The only thing left here is this backend's own widget re-sync,
    /// which differs by branch (a plugin panel does not move
    /// `app_shell.active_panel_id()`, so `sync_sidebar_from_engine` has nothing
    /// to sync for it).
    fn switch_panel(&mut self, panel_id: String) {
        let is_ext = panel_id.starts_with("ext:");
        render::apply_activity_panel_switch(&mut self.engine.borrow_mut(), &panel_id);
        if is_ext {
            self.sync_sidebar_widgets();
        } else {
            self.sync_sidebar_from_engine();
        }
    }

    /// Explorer CRUD action triggered by a keyboard shortcut or context
    /// menu. The string table moved to
    /// [`crate::core::settings::ExplorerAction::from_action_str`] in #823
    /// item 6 — see its doc comment for why only this 5-string resolver
    /// (not the surrounding dispatch) is shared with TUI.
    fn explorer_action(&mut self, action_str: String) {
        if let Some(action) = crate::core::settings::ExplorerAction::from_action_str(&action_str) {
            self.engine.borrow_mut().dispatch_explorer_crud(action);
            self.queue_explorer_draw();
            self.draw_needed.set(true);
        }
    }

    /// Refresh the file tree from the current working directory.
    fn refresh_file_tree(&mut self) {
        self.refresh_explorer();
        if let Some(path) = self.engine.borrow().file_path().cloned() {
            self.reveal_path_in_explorer(&path);
        }
        self.draw_needed.set(true);
    }

    /// Toggle focus between the explorer and the editor.
    fn toggle_focus_explorer(&mut self) {
        if self.engine.borrow().explorer_has_focus {
            self.engine.borrow_mut().explorer_has_focus = false;
        } else {
            let mut engine = self.engine.borrow_mut();
            engine.ext_panel_active = None;
            engine.focus_sidebar_panel(PANEL_EXPLORER);
            drop(engine);
            self.sync_sidebar_widgets();
        }
        self.draw_needed.set(true);
    }

    /// Toggle focus between the search panel and the editor.
    fn toggle_focus_search(&mut self) {
        if self.current_active_panel_id() == PANEL_SEARCH && self.current_sidebar_visible() {
            // Just give the editor DA back keyboard focus.
        } else {
            let mut engine = self.engine.borrow_mut();
            engine.ext_panel_active = None;
            engine.focus_sidebar_panel(PANEL_SEARCH);
            drop(engine);
            self.sync_sidebar_widgets();
        }
        self.draw_needed.set(true);
    }

    /// `UiEvent` (scroll, mouse) over the explorer panel — routed through
    /// `TreeController::handle` for scrollbar interaction.
    /// Sidebar routing for the Explorer panel (#540/#754).
    ///
    /// The `TreeController` widget dispatch itself — populate, re-apply the
    /// paint-time metrics, `handle()`, resolve a `ContextMenuRequested` —
    /// is [`render::route_explorer_tree_event`], shared with TUI's
    /// `TuiShellApp::handle_mouse_event` explorer intercept. What stays here
    /// is GTK-only plumbing: which events this panel claims at all
    /// (`dominated`), pulling the metrics/backend/theme it needs to make the
    /// call, and its own draw-invalidation bookkeeping.
    fn explorer_ui_event(&mut self, ev: quadraui::UiEvent) {
        let dominated = matches!(
            ev,
            quadraui::UiEvent::MouseDown { .. }
                | quadraui::UiEvent::DoubleClick { .. }
                | quadraui::UiEvent::MouseUp { .. }
                | quadraui::UiEvent::Scroll { .. }
        ) || matches!(
            ev,
            quadraui::UiEvent::MouseMoved {
                buttons: quadraui::ButtonMask { left: true, .. },
                ..
            }
        );
        if !dominated {
            return;
        }
        let rect = self.engine.borrow().explorer_tree_rect.get();
        if rect.width <= 0.0 {
            return;
        }
        let theme = {
            let eng = self.engine.borrow();
            render::Theme::from_name(&eng.settings.colorscheme)
        };
        // Re-apply the metrics the tree was drawn with so the hit-test row
        // math matches the rendered rows (#540). `set_current_line_height`/
        // `set_current_char_width` are inherent on `GtkBackend`, not trait
        // methods on `dyn Backend`, so they must be set from here rather
        // than inside the shared function.
        let metrics = self.cached_explorer_metrics.get();
        let backend_rc = self.backend.clone();
        let mut b = backend_rc.borrow_mut();
        b.set_current_line_height(metrics.0);
        b.set_current_char_width(metrics.1);
        let tree_event = {
            let mut engine = self.engine.borrow_mut();
            render::route_explorer_tree_event(&mut engine, &ev, rect, metrics, &theme, &mut **b)
        };
        drop(b);

        // `None` means either the event was fully resolved inside
        // `route_explorer_tree_event` (a `ContextMenuRequested` — #546) or
        // the rect wasn't paintable; either way this panel already did
        // everything it needs to.
        let Some(tree_event) = tree_event else {
            self.queue_explorer_draw();
            self.draw_needed.set(true);
            return;
        };
        if matches!(ev, quadraui::UiEvent::DoubleClick { .. }) {
            self.engine
                .borrow_mut()
                .dispatch_explorer_tree_event(tree_event);
        } else if matches!(ev, quadraui::UiEvent::MouseDown { .. }) {
            self.engine
                .borrow_mut()
                .handle_explorer_mouse_event(tree_event);
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// Find the runner-created top-level window once it is mapped/visible.
    /// Returns `None` until then — see `capture_window_and_apply_csd`. (#552)
    ///
    /// Window discovery is inherently platform-specific (quadraui has no
    /// portable "find the runner's window" surface yet — #862 module doc item
    /// 2), so unlike `window`'s other call sites this one has no non-GTK
    /// branch to fall back to; it stays behind the `gui` feature entirely.
    #[cfg(feature = "gui")]
    fn find_visible_window() -> Option<Box<dyn PlatformWindowHandle>> {
        // `list_toplevels` asserts GTK is initialized, which it never is under
        // the headless test harness (#646). `run()` calls `gtk4::init()` before
        // building the `App`, so this is unconditionally `true` in a live run
        // and the guard costs production nothing.
        if !gtk4::is_initialized() {
            return None;
        }
        gtk4::Window::list_toplevels()
            .into_iter()
            .filter_map(|obj| obj.downcast::<gtk4::Window>().ok())
            .find(|w| w.is_visible())
            .map(|w| Box::new(w) as Box<dyn PlatformWindowHandle>)
    }

    /// Capture the runner's GTK window (if not already captured) and drop
    /// GTK's server-side WM titlebar in favour of the drawn CSD row from
    /// `render_content`. Called from both `setup()` (fast path, usually too
    /// early — the runner hasn't called `window.present()` yet) and `tick()`
    /// (reliable path — retried every frame until the window is mapped).
    /// (#552)
    fn capture_window_and_apply_csd(&mut self) {
        #[cfg(feature = "gui")]
        {
            if self.window.is_some() {
                return;
            }
            if let Some(w) = Self::find_visible_window() {
                w.win_set_decorated(false);
                self.window = Some(w);
            }
        }
    }

    /// Forward a pointer event over the sidebar content area to the active panel's
    /// controller. In ShellApp mode the sidebar has no dedicated per-panel
    /// `DrawingArea`, so events the Relm4 build delivered straight to each panel's
    /// DA must be routed here instead. Returns `true` when the event was
    /// consumed. (#540 ShellApp port, #544 non-explorer panels)
    ///
    /// The panel arms mirror `render_content`'s own `match active_id` — each one
    /// feeds the very controller (`TreeController` / `SidebarSystem` /
    /// `FormController`) that painted the panel, at the rect it painted into.
    /// That is the whole reason most arms are just a line or two of dispatch
    /// and carry no GTK-specific hit-test: the geometry already lives in the
    /// shared controller, exactly as `tui_main::shell_app`'s equivalent
    /// intercepts use it. A few panels (settings, extensions, debug/git via
    /// their helper functions below) need a bit more — focus bookkeeping or
    /// translating a press into a chrome band's local coordinate space — but
    /// none of them re-derive hit geometry the painter doesn't already own.
    ///
    /// # Drag / release follow-through
    ///
    /// A press claimed here sets `sidebar_pointer_captured`, and while that is
    /// set the subsequent `MouseMoved`(left held) / `MouseUp` are routed to the
    /// same panel so scrollbar thumbs and tree drags track the pointer. An
    /// *unclaimed* move/release is deliberately left alone, so an editor
    /// text-drag that happens to cross into the sidebar still finalizes through
    /// the editor's own mouse-up path.
    fn try_route_sidebar_mouse_event(
        &mut self,
        event: &quadraui::UiEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) -> bool {
        use quadraui::UiEvent;

        let Some(sb) = ctx.layout.sidebar_content_bounds else {
            self.sidebar_pointer_captured.set(false);
            return false;
        };
        let dragging = self.sidebar_pointer_captured.get();
        let pos = match event {
            UiEvent::MouseDown { position, .. }
            | UiEvent::DoubleClick { position, .. }
            | UiEvent::Scroll { position, .. } => *position,
            // Follow-through only: never *start* an interaction from a move or
            // a release (see the doc comment above).
            UiEvent::MouseUp { position, .. } if dragging => {
                self.sidebar_pointer_captured.set(false);
                *position
            }
            UiEvent::MouseMoved {
                position,
                buttons: quadraui::ButtonMask { left: true, .. },
            } if dragging => *position,
            _ => return false,
        };
        // A captured drag keeps its grab even when the pointer leaves the
        // sidebar — otherwise dragging a scrollbar thumb sideways would silently
        // hand the rest of the gesture to the editor.
        let starts_interaction =
            !matches!(event, UiEvent::MouseUp { .. } | UiEvent::MouseMoved { .. });
        if starts_interaction
            && (pos.x < sb.x
                || pos.x >= sb.x + sb.width
                || pos.y < sb.y
                || pos.y >= sb.y + sb.height)
        {
            return false;
        }
        // Only a *press* moves keyboard focus into the panel. A wheel notch is
        // deliberately excluded: hovering-and-scrolling must not steal focus,
        // the same rule the editor's own wheel path follows (#240/#646).
        let is_press = matches!(
            event,
            UiEvent::MouseDown { .. } | UiEvent::DoubleClick { .. }
        );

        // An open picker / command palette is painted *over* the sidebar and
        // owns every press while it is up (#555). `render_content` centres the
        // popup on the whole window, so with the sidebar open its left half
        // sits on top of the explorer tree — and without this the tree's row
        // hit-test underneath ate those presses before they could reach
        // `handle_mouse_click_msg`'s picker block. The dropdown a breadcrumb
        // click opens therefore looked completely inert on its left half:
        // rows highlighted nothing, selection never moved.
        //
        // Falling through is also what makes *dismissal* correct: a press on
        // the sidebar while the picker is up reaches the picker's own
        // modal-stack dispatch, which resolves it as an outside-click and
        // closes the popup (rather than silently driving the tree beneath it).
        if self.engine.borrow().picker_open {
            return false;
        }

        // An engine-drawn context menu (editor / tab-bar / explorer — they
        // all share `engine.context_menu`) takes priority over the sidebar's
        // own click routing. An explorer-sourced menu typically renders
        // inside these same sidebar bounds, so without this a click on it —
        // an item, or an outside-click meant to dismiss it — fell straight
        // through to `TreeController`'s row hit-test underneath: the menu
        // *looked* interactive but every click acted on the tree row instead
        // (#546 FAILED-2). Only a left press drives the menu's own
        // hit-test/dismissal (mirrors `handle_mouse_click_msg`); any other
        // press/double-click/scroll while a menu is open is swallowed here
        // rather than leaking through to the tree underneath it.
        if self.engine.borrow().context_menu.is_some() {
            if matches!(
                event,
                UiEvent::MouseDown {
                    button: quadraui::MouseButton::Left,
                    ..
                }
            ) {
                self.dispatch_context_menu_click(pos.x as f64, pos.y as f64);
            }
            self.draw_needed.set(true);
            return true;
        }

        // Which panel owns the sidebar body? `render::sidebar_owner` states
        // that precedence once (#754) — `ext_panel_active` first, then
        // `app_shell.active_panel_id()`, Explorer as the fallback — so the
        // click router, the hover router and the painter can never disagree
        // about who is on screen. This used to be an inline `format!("ext:{}")`
        // here and an `if …is_some() / else if active_panel_is(…)` chain on
        // TUI.
        let owner = render::sidebar_owner(&self.engine.borrow());

        let consumed = match &owner {
            render::SidebarOwner::Explorer => {
                self.explorer_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Search => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.search_set_focus(true);
                }
                engine.handle_search_sidebar_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Debug => {
                self.route_debug_sidebar_event(event, pos, starts_interaction)
            }
            render::SidebarOwner::Git => {
                self.route_sc_sidebar_event(event, pos, starts_interaction)
            }
            render::SidebarOwner::Extensions => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_sidebar_has_focus = true;
                }
                engine.handle_ext_sidebar_ui_event(event.clone());
                if matches!(event, UiEvent::DoubleClick { .. }) {
                    engine.ext_open_selected_readme();
                }
                true
            }
            render::SidebarOwner::Settings => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.settings_has_focus = true;
                }
                // `handle_settings_form_ui_event`'s own `bool` return (whether
                // `FormController` recognized a row/field under the point) is
                // deliberately ignored here: the position is already confirmed
                // to be inside the sidebar's content bounds (checked above),
                // so even a click on empty panel padding belongs to this panel,
                // not the editor underneath it. Honoring `false` would let that
                // click fall through to `handle_mouse_click_msg` at sidebar-local
                // coordinates, which is exactly the leak every other arm in this
                // match also guards against by returning `true` unconditionally.
                render::handle_settings_form_ui_event(&mut engine, event, sb);
                true
            }
            render::SidebarOwner::ExtPanel(_) => {
                // Plugin-provided panel: `render_content` paints it through the
                // same `ext_sidebar_system` at the same rect, so it routes the
                // same way.
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_sidebar_has_focus = true;
                }
                engine.handle_ext_sidebar_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Ai => self.route_ai_sidebar_event(event, starts_interaction),
            // Unknown panel id: nothing was painted, so there is nothing
            // for a click to hit — let it fall through rather than
            // swallow it.
            render::SidebarOwner::Unknown => false,
        };

        if consumed {
            if starts_interaction {
                self.sidebar_pointer_captured
                    .set(matches!(event, UiEvent::MouseDown { .. }));
            }
            self.draw_needed.set(true);
        }
        consumed
    }

    /// Sidebar routing for the Debug panel (#544/#754).
    ///
    /// `render_content` stacks two chrome rows above the body: a title bar and
    /// an action-button bar whose `StatusBarLayout` it stashes in
    /// `engine.dap_sidebar_action_hits`. Those hit regions are **bar-relative**
    /// (`StatusBar::layout` lays out from `0,0`; `quadraui::gtk::draw_status_bar`
    /// returns them verbatim), so the press has to be translated into the
    /// action row's own space before hit-testing
    /// ([`render::dap_sidebar_action_click_at`]). Everything below goes to
    /// the shared `SidebarSystem` at the body rect it painted into
    /// ([`render::dispatch_dap_sidebar_body_event`]) — the same two shared
    /// functions TUI calls for this panel.
    fn route_debug_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let action_rect = self.cached_dap_action_rect.get();
        let body_rect = self.engine.borrow().dap_sidebar_body_rect.get();
        if body_rect.width <= 0.0 {
            return false;
        }
        let mut engine = self.engine.borrow_mut();
        if starts_interaction {
            engine.dap_sidebar_has_focus = true;
        }
        // Chrome band (title + action row) — above the body rect.
        if starts_interaction && pos.y < body_rect.y {
            if let Some(ar) = action_rect {
                render::dap_sidebar_action_click_at(&mut engine, pos.x - ar.x, pos.y - ar.y);
            }
            // Claimed either way: the press landed on this panel's own chrome,
            // so it must not leak through to the editor beneath (#637's rule
            // for the TUI twin of this intercept).
            return true;
        }
        let backend_rc = self.backend.clone();
        render::dispatch_dap_sidebar_body_event(
            &mut engine,
            event,
            body_rect,
            &mut **backend_rc.borrow_mut(),
        );
        true
    }

    /// Sidebar routing for the git ("source control") panel (#544/#754).
    ///
    /// The panel is three stacked bands — header, commit-message input, and the
    /// toolbar slab + change sections. `render_content` derives them via
    /// `render::sc_sidebar_bands` and caches the result here, so this resolves a
    /// press against the exact geometry that was painted rather than
    /// re-deriving it (the pre-#544 handler assumed `DrawingArea`-local
    /// coordinates with the panel top at `y == 0`, which the ShellApp painter
    /// never produces). The dispatch itself is
    /// [`render::route_sc_sidebar_click`], shared with TUI.
    fn route_sc_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let Some(bands) = self.cached_sc_bands.get() else {
            return false;
        };
        let mut engine = self.engine.borrow_mut();
        render::route_sc_sidebar_click(&mut engine, event, pos, &bands, starts_interaction);
        true
    }

    /// Sidebar routing for the AI panel (#544/#754/#819).
    ///
    /// `render_content` caches the panel rect in `Engine::ai_chat_rect` at
    /// paint time — resolving a press against that means the click router
    /// can never derive a different layout than the one actually on screen
    /// (#544/#582/#646). The dispatch itself is [`render::route_ai_chat_event`],
    /// which needs a live `Backend` (like [`Self::route_debug_sidebar_event`])
    /// for `ChatController::handle`'s own layout/hit-test math. Consumes the
    /// press unconditionally like every other panel arm in
    /// `try_route_sidebar_mouse_event` — a click on empty panel padding still
    /// belongs to this panel, not the editor underneath it.
    fn route_ai_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        starts_interaction: bool,
    ) -> bool {
        let mut engine = self.engine.borrow_mut();
        let rect = engine.ai_chat_rect.get();
        if rect.width <= 0.0 {
            return false;
        }
        if starts_interaction {
            engine.ai_has_focus = true;
        }
        let theme = render::Theme::from_name(&engine.settings.colorscheme);
        let backend_rc = self.backend.clone();
        // Re-apply the metrics `render()` painted the panel with — see
        // `cached_ai_chat_metrics`'s doc for why this can't be skipped.
        let metrics = self.cached_ai_chat_metrics.get();
        {
            let mut b = backend_rc.borrow_mut();
            b.set_current_line_height(metrics.0);
            b.set_current_char_width(metrics.1);
        }
        render::route_ai_chat_event(
            &mut engine,
            event,
            rect,
            &theme,
            &mut **backend_rc.borrow_mut(),
        );
        true
    }

    fn handle_explorer_da_key(&mut self, key_name: String, unicode: Option<char>, ctrl: bool) {
        // #734 slice 1: the #426 explorer-ctx-menu intercept and the
        // dialog patch-up ("route keys to the dialog handler, not the
        // explorer dispatch") that used to open this function are gone —
        // both were local re-statements of rungs `render::route_modal_key`
        // now resolves at the top of `handle_key_press`, above the
        // `explorer_has_focus` rung that is this function's only caller.

        // Panel-nav shortcuts before engine dispatch.
        let (pk_toggle, pk_explorer, pk_search) = {
            let eng = self.engine.borrow();
            (
                eng.settings.panel_keys.toggle_sidebar.clone(),
                eng.settings.panel_keys.focus_explorer.clone(),
                eng.settings.panel_keys.focus_search.clone(),
            )
        };
        let printable = match (ctrl, unicode) {
            (true, Some(c)) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            (false, Some(c)) => c.to_string(),
            _ => key_name.clone(),
        };
        if printable == pk_toggle {
            self.toggle_sidebar_panel();
            return;
        }
        if printable == pk_explorer {
            self.toggle_focus_explorer();
            return;
        }
        if printable == pk_search {
            self.toggle_focus_search();
            return;
        }

        use crate::core::engine::ExplorerKeyResult;
        let result = self
            .engine
            .borrow_mut()
            .dispatch_explorer_key(&key_name, unicode, ctrl);

        match result {
            ExplorerKeyResult::Unfocused => {
                self.engine.borrow_mut().explorer_has_focus = false;
            }
            ExplorerKeyResult::FocusToolbar => {
                // engine.activity_bar_focus_in_at(1) was already called inside
                // dispatch_explorer_key. Redraw the activity bar for the
                // selection highlight; key events route through the editor DA
                // whose handle_key_press checks activity_bar_focused and
                // dispatches to handle_activity_bar_key. The activity bar DA
                // has no EventControllerKey, so grab_focus on it drops keys.
                self.engine.borrow_mut().explorer_has_focus = false;
            }
            _ => {}
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// #731: was a redraw hint on `self.explorer_sidebar_da_ref`, permanently
    /// `None` under the ShellApp runner. `render_content` repaints the whole
    /// sidebar from engine state every frame, so callers only need
    /// `self.draw_needed.set(true)` — kept as a named no-op (rather than
    /// touching every call site) so the intent at each call site stays
    /// legible.
    fn queue_explorer_draw(&self) {}

    /// After a sidebar panel processes a key, queue a redraw of the activity
    /// bar if the engine just set `activity_bar_focused`, and in all cases
    /// give GTK widget focus to the editor DA so its `handle_key_press` can
    /// route the next key via engine flags (`activity_bar_focused`,
    /// `ext_panel_has_focus`, …).
    ///
    /// Why the editor DA, not the activity bar DA?  The activity bar DA has
    /// no `EventControllerKey`; routing GTK focus there drops subsequent key
    /// events.  The editor DA's capture-phase controller checks engine focus
    /// flags and dispatches to `handle_activity_bar_key` when needed — the
    /// same engine-flag routing that the TUI backend uses.
    ///
    /// `fallback_focused` is the "panel still has focus" flag passed through
    /// to `focus_editor_if_needed` when neither activity-bar nor editor focus
    /// applies (i.e. the sidebar panel kept focus → don't steal it).
    fn focus_after_sidebar_key(&self, fallback_focused: bool) {
        if self.engine.borrow().activity_bar_focused {
            // Activity bar has logical focus — `render_content` repaints it
            // every frame from engine state, so there's no separate redraw
            // hint to give here. Key routing flows through the editor DA.
            self.focus_editor_if_needed(false);
        } else {
            self.focus_editor_if_needed(fallback_focused);
        }
    }

    /// Handle a key press while the activity bar has keyboard focus. The key
    /// table itself is shared (`render::activity_bar_key_action`); this is the
    /// GTK sink for the actions it names.
    fn handle_activity_bar_key(&mut self, key_name: &str, ctrl: bool) {
        use render::ActivityBarKeyAction;
        match render::activity_bar_key_action(map_gtk_key_name(key_name), ctrl) {
            ActivityBarKeyAction::MoveDown => self.engine.borrow_mut().activity_bar_move_down(),
            ActivityBarKeyAction::MoveUp => self.engine.borrow_mut().activity_bar_move_up(),
            ActivityBarKeyAction::Activate => {
                use crate::core::engine::sidebar::ActivityBarActivation;
                let activation = self.engine.borrow_mut().activity_bar_activate();
                match activation {
                    // The menu bar is repainted every frame by
                    // `render_content`'s `ShellApp` path (no dedicated overlay
                    // DA to invalidate under the #540 cutover).
                    ActivityBarActivation::MenuToggled => self.draw_needed.set(true),
                    ActivityBarActivation::PanelFocused
                    | ActivityBarActivation::ExtPanelFocused(_) => {
                        self.sync_sidebar_from_engine();
                    }
                    ActivityBarActivation::NoOp => {}
                }
            }
            ActivityBarKeyAction::FocusOut => self.engine.borrow_mut().activity_bar_focus_out(),
            ActivityBarKeyAction::Collapse => {
                let mut engine = self.engine.borrow_mut();
                engine.activity_bar_focus_out();
                engine.collapse_sidebar();
            }
            ActivityBarKeyAction::Ignore => {}
        }
        // Suppress the default engine key handler — key is consumed.
    }

    /// #734 slice 1: the single GTK-side sink for the shared context-menu
    /// key rung (`render::ModalKeyRoute::ContextMenu`).
    ///
    /// Replaces two hand-rolled copies — the block that opened
    /// `handle_key_press` and `handle_explorer_ctx_menu_key` (#426) on the
    /// explorer DA path — both of which reimplemented selection movement
    /// inline instead of calling `Engine::handle_context_menu_key`, and so
    /// disagreed with TUI on `l` (confirm), `q`/`h` (close) and disabled-item
    /// skipping. The engine owns all of that now; the only GTK-specific part
    /// left is dispatching the confirmed action, since `new_file` /
    /// `open_terminal` / `find_in_folder` need backend plumbing.
    fn dispatch_context_menu_key(&mut self, key_name: &str, unicode: Option<char>) {
        let effective_key = if key_name.is_empty() {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        } else {
            key_name.to_string()
        };
        let target = self.engine.borrow().context_menu_target_path();
        let action = {
            let mut engine = self.engine.borrow_mut();
            let (_consumed, action) = engine.handle_context_menu_key(&effective_key);
            action
        };
        if let (Some(ref act), Some((ref path, _is_dir))) = (action, target) {
            self.dispatch_explorer_ctx_action(act, path);
        }
        let needs_refresh = {
            let mut engine = self.engine.borrow_mut();
            let r = engine.explorer_needs_refresh;
            engine.explorer_needs_refresh = false;
            r
        };
        if needs_refresh {
            self.refresh_file_tree();
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// #426: Map the action string returned by `context_menu_confirm` for
    /// an explorer ctx menu to the appropriate backend Msg. Engine-side
    /// actions (copy_path, reveal, select_for_diff, etc.) were already
    /// handled inside `context_menu_confirm`; this only covers actions
    /// that require GTK plumbing.
    fn dispatch_explorer_ctx_action(&mut self, action: &str, target: &std::path::Path) {
        match action {
            "new_file" | "new_folder" | "rename" | "delete" | "move_file" => {
                self.explorer_action(action.to_string());
            }
            "open_terminal" => {
                let dir = if target.is_dir() {
                    target.to_path_buf()
                } else {
                    target
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                };
                self.open_terminal_at(dir);
            }
            "find_in_folder" => {
                self.toggle_focus_search();
            }
            _ => {} // engine-handled actions (copy_path, reveal, etc.)
        }
    }

    /// Minimize the application window (inline window-control button).
    fn window_minimize(&mut self) {
        if let Some(ref w) = self.window {
            w.win_minimize();
        }
    }

    /// Maximize or restore the application window (inline window-control
    /// button).
    ///
    /// Goes through `Backend::toggle_window_maximize` (#813) rather than
    /// driving the OS window handle's maximize/unmaximize directly — the
    /// same toggle the CSD-titlebar double-click gesture already calls
    /// (`handle_dispatch`'s `UiEvent::DoubleClick` arm) — so there is only
    /// one place that operates the real OS window instead of two that have
    /// to agree.
    fn window_toggle_maximize(&mut self, backend: &mut dyn quadraui::Backend) {
        backend.toggle_window_maximize();
    }

    /// Close the application window (inline window-control button).
    ///
    /// Routes through `show_quit_confirm` (#857) instead of driving the real
    /// OS window's `close()` directly: `close()` emits GTK's `close-request`
    /// signal *synchronously, on the same stack* — quadraui's handler for
    /// that signal re-enters `backend.borrow_mut()` while this dispatch path
    /// still holds it (see quadraui `run.rs:616`/`896`), which panics with
    /// `BorrowMutError` inside a signal trampoline that cannot unwind and so
    /// aborts the process instead of just panicking. `window_toggle_maximize`
    /// above already avoids the equivalent trap for maximize (#813) by
    /// routing through the engine instead of the OS window handle; this is
    /// the same fix applied to close. `show_quit_confirm` either raises the
    /// unsaved-changes dialog or sets `exit_requested`, which
    /// `ShellApp::handle` turns into `quadraui::Reaction::Exit` — the runner
    /// then tears the window down with `destroy()`, which does not re-enter
    /// `close-request`.
    fn window_close(&mut self) {
        self.show_quit_confirm();
    }

    /// User triggered quit; exit straight away when nothing is unsaved,
    /// otherwise raise the "unsaved changes" confirmation dialog.
    ///
    /// #823 item 4: the dialog body used to be restated here (byte-identical
    /// to `Engine::show_quit_confirm`, `core/engine/panels.rs`) instead of
    /// calling it — same `DialogButton` literals, just copy-pasted. TUI
    /// already calls the engine method directly (`tui_main/shell_app.rs`,
    /// `tui_main/mouse.rs`).
    fn show_quit_confirm(&mut self) {
        if !self.engine.borrow().has_any_unsaved() {
            self.save_session_and_exit();
            return;
        }
        self.engine.borrow_mut().show_quit_confirm();
        self.draw_needed.set(true);
    }

    /// Show a native "Open File" dialog.
    fn open_file_dialog(&mut self) {
        // Deferred to tick(), which has the runner-owned `backend`
        // handle PlatformServices needs — see PendingFileDialog (#572).
        self.pending_file_dialog
            .set(Some(PendingFileDialog::OpenFile));
        self.draw_needed.set(true);
    }

    /// Show the shared folder/workspace picker modal (#815).
    ///
    /// Before #815 this opened a *native* `gtk4::FileDialog` in
    /// "select folder" mode: at the time, `quadraui::PlatformServices` had no
    /// directory-select primitive (only `show_file_open_dialog` /
    /// `show_file_save_dialog`, both file pickers), so a native chooser was
    /// the only option. `quadraui::FolderPickerController` (shipped
    /// 2026-05-25, quadraui#166) made that escape hatch unnecessary — it does
    /// its own filesystem walk — so this opens the identical `Palette`-based
    /// picker TUI does; see `FrameOp::FolderPicker` and
    /// `handle_key_press`'s folder-picker rung. TUI's
    /// `new_folder_picker_controller` builds the same controller.
    ///
    /// `PlatformServices` has since gained `show_folder_open_dialog`
    /// (quadraui#935) — the premise above is now stale on its own, but the
    /// decision it led to isn't: this stays on `FolderPickerController`
    /// deliberately (#815; see issue #945), because it's the shape that
    /// behaves identically on every backend, whereas
    /// `show_folder_open_dialog` is a native-only primitive that TUI can't
    /// implement the same way GTK/macOS/Win-GUI would.
    fn open_folder_dialog(&mut self) {
        let engine = self.engine.borrow();
        let controller = quadraui::FolderPickerController::new(
            engine.cwd.clone(),
            vec![".vimcode-workspace".to_string()],
            engine.settings.show_hidden_files,
        );
        drop(engine);
        *self.folder_picker.borrow_mut() = Some(controller);
        self.draw_needed.set(true);
    }

    /// Drive an open folder picker with one raw `UiEvent` and apply the
    /// result. The decision (key→intent, filesystem walk, filtering,
    /// scroll) is entirely `quadraui::FolderPickerController`'s own (#815);
    /// this only applies the `Confirmed`/`Cancelled` outcomes to GTK-local
    /// state and the engine. Mirrors TUI's identical
    /// `apply_folder_picker_event` (`shell_app.rs`) — same controller, same
    /// outcome handling; the popup rect comes from the painted-rect cache
    /// (`folder_picker_popup_rect`, #582/#646) here instead of a fresh
    /// `Backend::viewport()` call, since this method has no live `backend`
    /// handle.
    fn apply_folder_picker_event(&mut self, event: &quadraui::UiEvent) {
        let Some(popup_rect) = self.folder_picker_popup_rect.get() else {
            return;
        };
        let lh = self.cached_line_height.max(1.0) as f32;
        let visible_rows = render::folder_picker_visible_rows(popup_rect, lh);
        let outcome = {
            let mut picker_ref = self.folder_picker.borrow_mut();
            let Some(picker) = picker_ref.as_mut() else {
                return;
            };
            picker.handle(event, visible_rows)
        };
        match outcome {
            quadraui::FolderPickerEvent::Confirmed { path } => {
                *self.folder_picker.borrow_mut() = None;
                self.engine.borrow_mut().open_folder(&path);
                self.refresh_file_tree();
            }
            quadraui::FolderPickerEvent::Cancelled => {
                *self.folder_picker.borrow_mut() = None;
            }
            quadraui::FolderPickerEvent::Consumed | quadraui::FolderPickerEvent::Ignored => {}
        }
    }

    /// Resolve a mouse press at `(x, y)` against the open folder picker's
    /// painted popup and apply the result. Mirrors TUI's identical block in
    /// `mouse::handle_mouse` — same shared `render::route_folder_picker_click`
    /// / `render::set_folder_picker_selected`, same "select row" / "consume"
    /// / "dismiss" outcomes.
    fn route_and_apply_folder_picker_click(&mut self, x: f64, y: f64) {
        let Some(rect) = self.folder_picker_popup_rect.get() else {
            // No painted rect to hit-test against (shouldn't happen while
            // `folder_picker` is open, since `render_content` always caches
            // one when it paints) — dismiss defensively rather than leave an
            // unreachable modal up.
            *self.folder_picker.borrow_mut() = None;
            self.draw_needed.set(true);
            return;
        };
        let lh = self.cached_line_height.max(1.0) as f32;
        let Some((scroll_top, total_filtered)) = self
            .folder_picker
            .borrow()
            .as_ref()
            .map(|p| (p.scroll_top(), p.filtered().len()))
        else {
            return;
        };
        let route = render::route_folder_picker_click(
            rect,
            x as f32,
            y as f32,
            lh,
            scroll_top,
            total_filtered,
        );
        match route {
            render::FolderPickerClickRoute::SelectRow(idx) => {
                if let Some(picker) = self.folder_picker.borrow_mut().as_mut() {
                    render::set_folder_picker_selected(picker, idx);
                    let visible_rows = render::folder_picker_visible_rows(rect, lh);
                    picker.sync_scroll(visible_rows);
                }
            }
            render::FolderPickerClickRoute::Consume => {}
            render::FolderPickerClickRoute::Dismiss => {
                *self.folder_picker.borrow_mut() = None;
            }
        }
        self.draw_needed.set(true);
    }

    /// Show a native "Save Workspace As" dialog.
    fn save_workspace_as_dialog(&mut self) {
        // Deferred to tick() — see PendingFileDialog (#572).
        self.pending_file_dialog
            .set(Some(PendingFileDialog::SaveWorkspaceAs));
        self.draw_needed.set(true);
    }

    /// Show the "Open Recent" workspace picker.
    fn open_recent_dialog(&mut self) {
        // #274: replaced the native gtk4::Dialog with the engine's
        // unified picker (PickerSource::RecentWorkspaces). Picker
        // confirm calls open_folder + sets explorer_needs_refresh
        // so the file tree rebuilds on the next render — no
        // backend-specific Msg dispatch needed here.
        let mut engine = self.engine.borrow_mut();
        if engine.session.recent_workspaces.is_empty() {
            engine.message = "No recent workspaces".to_string();
        } else {
            engine.open_picker(crate::core::engine::PickerSource::RecentWorkspaces);
        }
        drop(engine);
        self.draw_needed.set(true);
    }

    /// User confirmed quit — save session state then exit the process.
    fn quit_confirmed(&mut self) {
        // Save session state then exit the process.
        self.save_session_and_exit();
    }

    /// User clicked ✕ on a tab with unsaved changes — ask what to do.
    ///
    /// #823 item 4: was a byte-identical restatement of
    /// `Engine::show_close_tab_confirm` (`core/engine/panels.rs`) instead of
    /// calling it.
    fn show_close_tab_confirm(&mut self) {
        self.engine.borrow_mut().show_close_tab_confirm();
        self.draw_needed.set(true);
    }

    /// #731: was `if let Some(da) = self.drawing_area…` — that field is
    /// permanently `None` under the ShellApp runner (see its removal in
    /// #731), so this always took the `else` branch. The real fix is a way
    /// to read the live DA's pixel width without a widget handle (e.g. from
    /// `backend: &mut dyn quadraui::Backend`, which none of this method's
    /// callers currently have in scope) — until then this is pinned at the
    /// fallback, same as it was silently pinned at runtime before the dead
    /// field was deleted.
    ///
    /// Callers that DO have a live pixel width in scope (a click/drag's own
    /// `width` parameter, or `ctx.layout.main_content_bounds` off the
    /// `ShellContext` `handle_dispatch` already receives) must call
    /// [`Self::terminal_panel_cols`] instead — see #1058, where the
    /// terminal-split finalize path used this method's `80` fallback (and a
    /// separate `da_w = 800.0` guess) rather than converting the real width,
    /// so any window that wasn't exactly 800px wide split the terminal into
    /// the wrong column counts.
    #[allow(dead_code)]
    fn terminal_cols(&self) -> u16 {
        80
    }

    /// Terminal panel pixel width reserved for the panel's own vertical
    /// scrollbar — the same strip `render_content` paints one into and
    /// `MouseDragRoute::TerminalSplitDivider` already clamps the divider
    /// drag against (`handle_mouse_drag_msg`).
    const TERMINAL_PANEL_SB_W: f64 = 6.0;

    /// Convert a *live* terminal-panel pixel width to a column count using
    /// the last-painted char advance (`cached_char_width`) — the real
    /// pixel→cell conversion `terminal_cols()` cannot do because it has no
    /// width in scope. Falls back to `terminal_cols()`'s pinned `80` only
    /// when no char width has been measured yet (`cached_char_width <= 0.0`,
    /// i.e. before the first paint).
    fn terminal_panel_cols(&self, width: f64) -> u16 {
        if self.cached_char_width > 0.0 {
            ((width - Self::TERMINAL_PANEL_SB_W).max(0.0) / self.cached_char_width) as u16
        } else {
            self.terminal_cols()
        }
    }

    /// #731: see `terminal_cols` — was `if let Some(da) =
    /// self.drawing_area…`, permanently `None`, so this always took the
    /// `else` branch.
    fn terminal_target_maximize_rows(&self) -> u16 {
        10
    }
}

// ── Dormant ShellApp impl (#448-B) ──────────────────────────────────────────
// This impl compiles alongside the Relm4 path but is NOT wired up.
impl App {
    /// Paint the title-bar band: the menu bar + any open dropdown, the app-icon
    /// slot, and the inline window controls.
    ///
    /// One function because all three draw into the *same* strip and their
    /// order within it is fixed by a rasteriser detail rather than by taste:
    /// `MenuSystem::render` calls `draw_menu_bar` across the whole band, so the
    /// icon slot and the controls must follow it or get erased (the #552
    /// round-2/3 "buttons render blank" regression). Kept off
    /// [`render::FRAME_Z_ORDER`]'s overlay tail because TUI has neither an app icon nor
    /// in-canvas window controls, so they cannot be part of a *shared*
    /// sequence; they ride the `MenuDropdown` rung instead (#735).
    #[allow(clippy::too_many_arguments)]
    fn paint_title_bar_band(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        theme: &Theme,
        menu_row_rect: quadraui::Rect,
        menu_items_rect: quadraui::Rect,
        app_icon_rect: quadraui::Rect,
        controls_rect: Option<quadraui::Rect>,
    ) {
        {
            // `menu_items_rect`, not `menu_row_rect` — the app icon owns the
            // leading slot (#720). `MenuSystem::render` positions the open
            // dropdown from this same rect, so passing the narrowed one is
            // what keeps a dropdown under the label that opened it.
            engine.menu_system.borrow().render(backend, menu_items_rect);

            // ── App icon, left of `File` (#720) ──────────────────────────
            // `draw_menu_bar` above only filled `menu_items_rect`, so the
            // reserved slot still shows the frame-clear colour
            // (`theme.background`) rather than the bar's own `tab_bar_bg`.
            // Painting an *item-less* `MenuBar` across the slot fills it
            // through the very same rasteriser as the strip beside it, so
            // the two backgrounds cannot drift apart the way a hand-picked
            // theme colour would.
            if app_icon_rect.width > 0.0 && app_icon_rect.height > 0.0 {
                let filler = quadraui::MenuBar {
                    id: quadraui::WidgetId::new("app_icon_slot"),
                    items: Vec::new(),
                    open_item: None,
                    focused_item: None,
                };
                let _ = backend.draw_menu_bar(
                    quadraui::Rect::new(
                        menu_row_rect.x,
                        menu_row_rect.y,
                        (menu_items_rect.x - menu_row_rect.x).max(0.0),
                        menu_row_rect.height,
                    ),
                    &filler,
                );
                // `app_icon_image_for_paint` (this file), not
                // `render::app_icon_image`: the former hands over a
                // once-rasterised small PNG instead of the 1024x1024 SVG,
                // which `Backend::draw_image` would otherwise re-render
                // through librsvg on *every* frame (+16.5ms per repaint —
                // see `crate::gtk::util::app_icon_image`'s doc comment).
                let _ = backend.draw_image(app_icon_rect, &app_icon_image_for_paint());
            }
        }

        // ── Inline window controls (min/max/close) — after the bar (#552) ────
        // `menu_system.render()` above repaints `draw_menu_bar` across the full
        // `menu_row_rect` band, so the controls must be painted *after* it or
        // they get erased (the round-2/3 "buttons render blank" regression).
        // The controls sit in the title-bar band, to the right of the menu
        // labels; the dropdown body drops *below* the band, so painting here
        // never covers an open dropdown.
        //
        // #735 moved this from the very end of `render_content` (below the
        // dialog and context menu) to here. It is title-bar chrome, so the
        // modal rungs of `render::FRAME_Z_ORDER` now paint over it — which is
        // the point: a modal dialog covering the window controls is what
        // "modal" means, and it is what TUI already did with everything it
        // painted into its own title-bar row.
        if let Some(controls_rect) = controls_rect {
            let maximized = self.window.as_ref().is_some_and(|w| w.win_is_maximized());
            let controls_bar = render::window_controls_status_bar(theme, maximized);
            let interaction = self.title_bar_interaction.borrow();
            let hits = backend.draw_status_bar(
                controls_rect,
                &controls_bar,
                interaction.hovered_id(),
                interaction.pressed_id(),
            );
            interaction.set_layout(hits);
        }
    }
}

impl App {
    /// The actual body of `ShellApp::handle`, moved to an inherent
    /// method (#813) so the trait impl can wrap it with a single
    /// `exit_requested` check that covers every early-return arm below
    /// (including ones nested inside `dispatch_engine_action` /
    /// `apply_dialog_action` / menu handling) without editing each one.
    fn handle_dispatch(
        &mut self,
        event: quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) -> quadraui::Reaction {
        use quadraui::{Key, MouseButton, UiEvent};

        // ── Menu system intercept (#552) ─────────────────────────────────────
        // GTK's menu bar is always visible (see `ShellApp::setup`) and its
        // dropdown overlay must intercept keys/clicks before the sidebar or
        // editor sees them — same precedence TUI uses (mod.rs "MenuSystem
        // intercept" block) via the identical shared `menu_system.handle()`.
        let (menu_bar_visible, menu_system) = {
            let eng = self.engine.borrow();
            (eng.menu_bar_visible, eng.menu_system.clone())
        };
        if menu_bar_visible || menu_system.borrow().is_open() {
            // `menu_items_rect`, not `menu_row_rect` (#720): the app icon
            // occupies a leading slot, so the items the last frame *painted*
            // start one slot right of the band's left edge. Hit-testing
            // against the full band would resolve a click on `File` to
            // whatever label now sits a slot to its left. `render_content`
            // writes this from the same `split_menu_row_for_app_icon` call
            // that positions the paint.
            let bar_rect = self.menu_items_rect.get();
            let menu_event = menu_system.borrow_mut().handle(&event, backend, bar_rect);
            match menu_event {
                quadraui::MenuEvent::Activated(id) => {
                    self.handle_menu_action(id.as_str().to_string());
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
                quadraui::MenuEvent::StateChanged | quadraui::MenuEvent::Consumed => {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
                quadraui::MenuEvent::Ignored => {}
            }
        }

        // ── Command Center: nav arrows + search box (#676) ────────────────────
        // Checked before the window-control buttons below and the CSD
        // titlebar drag-to-move fallback further down, so a click in the
        // command center (which sits inside the title-bar band the
        // drag-to-move check would otherwise claim) routes to tab-nav / the
        // picker instead of starting a window drag. Mirrors TUI's
        // `mouse.rs` "Menu bar row click — command center only" precedence.
        // The nav-arrow / search-box actions are the shared
        // `render::apply_command_center_hit` (#752) — the pre-#540 Relm4
        // `Msg::MruNavBack` / `MruNavForward` / `OpenCommandCenter` variants
        // for this exact action, already wired end-to-end but never
        // dispatched from anywhere since the cutover. This block was their
        // first live caller (#676); #732 turned them into plain methods,
        // and #752 converged those methods with TUI's identical match arm
        // into the one function in `render.rs`.
        if let UiEvent::MouseDown {
            button: MouseButton::Left,
            position,
            ..
        } = &event
        {
            let cc_hit = self
                .engine
                .borrow()
                .command_center_layout
                .borrow()
                .as_ref()
                .map(|l| l.hit_test(position.x, position.y));
            // `Bar` (command-center background, not an interactive segment)
            // and `Outside`/`None` fall through so the drag-to-move fallback
            // below still works for genuine empty-band clicks.
            if let Some(hit) = cc_hit {
                if crate::render::apply_command_center_hit(&mut self.engine.borrow_mut(), hit) {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
            }
        }

        // ── Inline window-control buttons: minimize/maximize/close (#552) ───
        // Shared `StatusBarInteraction` hover/press/click tracker — the same
        // primitive quadraui's own `full_chrome_demo` reference title bar
        // uses (quadraui#402) — instead of a hand-rolled `StatusBarHit`
        // lookup. Gets the buttons real hover/press highlighting for free
        // and click-on-release semantics (a press that drags off the button
        // before release no longer fires it), matching native window
        // controls. Runs on every event (not just MouseDown) so hover state
        // updates as the pointer moves.
        {
            let rect = self.title_bar_rect.get();
            if rect.width > 0.0 {
                let action = self.title_bar_interaction.borrow_mut().handle(&event, rect);
                match action {
                    quadraui::StatusBarAction::Clicked(id) => {
                        match id.as_str() {
                            render::WINDOW_MINIMIZE_ACTION => self.window_minimize(),
                            render::WINDOW_MAXIMIZE_ACTION => self.window_toggle_maximize(backend),
                            render::WINDOW_CLOSE_ACTION => self.window_close(),
                            _ => {}
                        }
                        self.draw_needed.set(true);
                        return quadraui::Reaction::Redraw;
                    }
                    quadraui::StatusBarAction::Redraw => {
                        self.draw_needed.set(true);
                        return quadraui::Reaction::Redraw;
                    }
                    quadraui::StatusBarAction::Ignored => {}
                }
            }
        }

        // ── Outer window border: edge-resize cursor hint (quadraui#406) ──
        // Pure side effect on hover — hint the resize pointer over the outer
        // window border, default everywhere else (including the non-resizable
        // full-width CSD title bar, which owns the top edge). Falls through so
        // the editor/sidebar hover handling below still runs. Mirrors
        // `full_chrome_demo`'s `MouseMoved` arm. GTK-only; TUI `set_cursor`
        // is a documented no-op.
        if let UiEvent::MouseMoved { position, .. } = &event {
            let shape = if ctx.in_title_bar(position.x, position.y) {
                quadraui::PointerShape::Default
            } else {
                match ctx.window_edge(position.x, position.y, backend.line_height()) {
                    Some(edge) => quadraui::PointerShape::Resize(edge),
                    None => quadraui::PointerShape::Default,
                }
            };
            backend.set_cursor(shape);
        }

        // ── Sidebar hover — #754 rung ─────────────────────────────────────
        // This backend already *painted* `screen.panel_hover` (the
        // `RichTextPopup` block in `render_content`) and already tracked the
        // popup's own rect, but nothing on this side ever set
        // `engine.panel_hover` or `engine.sc_button_hovered`: the router was
        // ~78 lines of TUI-only code. That is the #499/#484 mechanism — paint
        // without input on one backend, input without a second painter on the
        // other. `render::route_sidebar_hover` is now the single router and
        // both backends call it.
        if let UiEvent::MouseMoved { position, .. } = &event {
            if let Some(sb) = ctx.layout.sidebar_content_bounds {
                let lh = backend.line_height();
                let on_popup = self.panel_hover_popup_rect.get().is_some_and(|r| {
                    position.x >= r.x
                        && position.x < r.x + r.width
                        && position.y >= r.y
                        && position.y < r.y + r.height
                });
                let owner = render::sidebar_owner(&self.engine.borrow());
                let changed = render::route_sidebar_hover(
                    &mut self.engine.borrow_mut(),
                    &owner,
                    position.x,
                    position.y,
                    render::SidebarBodyGeometry {
                        bounds: sb,
                        row_h: lh.max(1.0),
                        header_rows: 1.0,
                    },
                    true,
                    on_popup,
                );
                if changed {
                    self.draw_needed.set(true);
                }
            }
        }

        // ── CSD titlebar background: drag-to-move / double-click-maximize ──
        // (quadraui#400) + outer window border: edge-resize (quadraui#406).
        // Runs after the menu-item intercept and the window-control-button
        // check above, so both take priority — only a press/double-click that
        // lands in the title bar band but misses every interactive segment
        // (menu item, min/max/close button) reaches here, matching
        // `Backend::begin_window_drag`'s documented contract. The title bar
        // takes priority over the top window edge (a full-width CSD header
        // owns it), so `in_title_bar` is checked before `window_edge` —
        // mirrors quadraui's `full_chrome_demo` reference. TUI has no window,
        // so `begin_window_drag`/`begin_window_resize`/`toggle_window_maximize`
        // are all documented no-ops there; this path is GTK-only.
        match &event {
            UiEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } if ctx.in_title_bar(position.x, position.y) => {
                backend.begin_window_drag();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::DoubleClick { position, .. } if ctx.in_title_bar(position.x, position.y) => {
                backend.toggle_window_maximize();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } => {
                // #816: the command line paints in the window's literal last
                // `line_height` pixels — exactly the margin `window_edge`
                // treats as the bottom resize border — so without this guard
                // a click on it landing here first (before
                // `handle_mouse_click_msg`'s command-line rung ever ran)
                // ordered `begin_window_resize` instead of ever reaching the
                // click. No prior GTK feature lived in that exact band to
                // expose the conflict; the command line is the first.
                //
                // `render::point_over_command_line` checks BOTH axes — see
                // its doc comment for why a y-only version silently disables
                // the window's only S/SW/SE resize grab (#816 review).
                let over_command_line = render::point_over_command_line(
                    self.engine.borrow().command_line_rect.get(),
                    *position,
                );
                if !over_command_line {
                    if let Some(edge) =
                        ctx.window_edge(position.x, position.y, backend.line_height())
                    {
                        // #1026/#987 review: `begin_window_resize`'s own doc
                        // contract is explicit — it returns `false` "when the
                        // backend owns no window (TUI...)" and callers
                        // "should treat `false` as a no-op, not an error".
                        // This call site used to discard that return value
                        // and swallow the click unconditionally, so on TUI
                        // (`ctx.window_edge`'s margin math is generic
                        // geometry, not GTK-gated — it fires for any backend
                        // near the outer window bounds) a click on a
                        // window's own rightmost column — exactly where a
                        // vertical scrollbar column sits when that window is
                        // flush with the screen's own right edge — never
                        // reached `handle_mouse_click_msg` at all. Only
                        // consume the event when the backend actually armed
                        // a resize.
                        if backend.begin_window_resize(edge) {
                            self.draw_needed.set(true);
                            return quadraui::Reaction::Redraw;
                        }
                    }
                }
            }
            _ => {}
        }

        // Pointer events over the sidebar content area are forwarded to the active
        // panel's controller before the editor click path sees them. In ShellApp
        // mode there is no per-panel DrawingArea, so without this the file explorer
        // never receives clicks. (#540 ShellApp port)
        if self.try_route_sidebar_mouse_event(&event, ctx) {
            return if self.draw_needed.get() {
                self.draw_needed.set(false);
                quadraui::Reaction::Redraw
            } else {
                quadraui::Reaction::Continue
            };
        }

        match event {
            UiEvent::KeyPressed {
                key,
                modifiers,
                repeat,
            } => {
                // #815: kept alongside the decoded `key_name`/`unicode` below
                // so the folder-picker rung in `handle_key_press` can feed
                // `FolderPickerController::handle` the *original* event
                // instead of a re-encoded one — mirrors TUI's `dap_event`
                // (`shell_app.rs`).
                let raw_event = UiEvent::KeyPressed {
                    key: key.clone(),
                    modifiers,
                    repeat,
                };
                let (key_name, unicode) = match key {
                    Key::Char(c) => (c.to_string(), Some(c)),
                    Key::Named(_) => {
                        // #826: `Escape`/`Enter`->`Return`/`Backspace`->
                        // `BackSpace`/`Delete`/`Tab`/`Home`/`End`/the arrows/
                        // F-keys are byte-identical to TUI's spelling, so
                        // those go through the shared
                        // `render::engine_key_from_ui` — the same decoder
                        // TUI's dispatch now calls — instead of restating an
                        // identical table a second time here.
                        //
                        // #1060: the remaining four keys that used to keep
                        // GTK's own spelling now go through the same shared
                        // decoder too, matching TUI's spelling exactly
                        // (`render::engine_key_from_ui` spellings on the
                        // right):
                        //  * `BackTab`: `"BackTab"` -> `"ISO_Left_Tab"`.
                        //    `panels.rs`/`ext_panel.rs`'s hover-key arm
                        //    already dual-aliased both spellings; the three
                        //    sites that only recognised `"BackTab"`
                        //    (`search.rs`'s `handle_search_input_key`,
                        //    `source_control.rs`'s sidebar nav,
                        //    `ext_panel.rs`'s `dispatch_ext_sidebar_key_unified`)
                        //    now also accept `"ISO_Left_Tab"` — TUI already
                        //    sent that spelling to all three and was
                        //    silently dropping Shift+Tab there before this
                        //    fix, so this closes a live TUI bug, not just a
                        //    GTK one. The main-editor command-line wildmenu
                        //    and Ctrl+Shift+Tab tab-switcher-backward binds
                        //    (`keys.rs`) already only recognised
                        //    `"ISO_Left_Tab"`, so GTK gains working
                        //    Ctrl+Shift+Tab / Shift+Tab-in-`:`-wildmenu as a
                        //    side effect.
                        //  * `PageUp`/`PageDown`: `"PageUp"`/`"PageDown"` ->
                        //    `"Page_Up"`/`"Page_Down"`. Every consumer
                        //    (`source_control.rs`, `ext_panel.rs`,
                        //    `search.rs`, `explorer_ops.rs`,
                        //    `canonical_terminal_key_name`) already only
                        //    recognised the TUI spelling, so this closes the
                        //    pre-existing GTK gap noted at the old comment
                        //    here rather than needing its own audit.
                        //  * `Insert`: was `"Insert"`, and the shared decoder
                        //    used to return `None` for `NamedKey::Insert`
                        //    (dropping the key entirely — GTK's terminal PTY
                        //    passthrough bypassed the shared decoder just to
                        //    avoid that). `render::engine_key_from_ui` now
                        //    has an `NamedKey::Insert => Some(("Insert", ..))`
                        //    arm so both backends get the same, still-working
                        //    `"Insert"` spelling.
                        //
                        // `key_name` doesn't reach engine consumers raw: it
                        // passes through a second, GTK-local decode layer —
                        // `map_gtk_key_name` / `map_gtk_key_with_unicode`
                        // below in this file — before `handle_key_press`
                        // dispatches it. Both tables already had an
                        // `"ISO_Left_Tab"` arm from before this PR, but it
                        // was dead code on the GTK path (GTK never produced
                        // that spelling as `key_name` pre-#1060). Making
                        // `"ISO_Left_Tab"` live here is what surfaced their
                        // disagreement: `map_gtk_key_name` round-trips it to
                        // `"BackTab"` correctly, but `map_gtk_key_with_unicode`
                        // used to collapse both `"Tab"` and `"ISO_Left_Tab"`
                        // to plain `"Tab"`, silently turning Shift+Tab into
                        // Tab for the one route that consumes its output
                        // (`FocusKeyRoute::SourceControl`'s `sc_mapped`).
                        // Fixed alongside this comment so `map_gtk_key_with_unicode`
                        // now matches `map_gtk_key_name`'s `"ISO_Left_Tab" =>
                        // "BackTab"` round-trip.
                        let n = render::engine_key_from_ui(&key, modifiers, true)
                            .map(|(name, _, _)| name)
                            .unwrap_or_default();
                        (n, None)
                    }
                };
                if !key_name.is_empty() || unicode.is_some() {
                    self.handle_key_press(
                        key_name,
                        unicode,
                        modifiers.ctrl,
                        modifiers.shift,
                        modifiers.alt,
                        &raw_event,
                        ctx,
                    );
                }
            }
            UiEvent::CharTyped(c) => {
                // Ctrl-modified characters arrive via KeyPressed; CharTyped is
                // for IME-composed printable characters only. Per
                // `FolderPickerController::handle`'s own contract this is
                // never the folder picker's typing source either, so passing
                // it through as the "raw event" is correct, not a stand-in.
                self.handle_key_press(
                    c.to_string(),
                    Some(c),
                    false,
                    false,
                    false,
                    &UiEvent::CharTyped(c),
                    ctx,
                );
            }
            UiEvent::Accelerator(id, _mods) => {
                let mut host = GtkAccelHost {
                    deferred: &self.deferred,
                };
                if let Some(action) = render::dispatch_panel_accelerator(
                    id.as_str(),
                    &mut self.engine.borrow_mut(),
                    &mut host,
                ) {
                    // `dispatch_panel_accelerator` already mutated `engine`
                    // directly for these five (no `GtkAccelHost` hook — see
                    // `render.rs`), but they still need geometry recomputed
                    // before the next paint — matches the pre-#761 per-arm
                    // `deferred.send(DeferredAction::Resize)`.
                    use render::PanelAccelerator::*;
                    if matches!(
                        action,
                        FuzzyFinder | LiveGrep | CommandPalette | AddCursor | SelectAllMatches
                    ) {
                        self.deferred.send(DeferredAction::Resize);
                    }
                }
                self.draw_needed.set(true);
            }
            UiEvent::MenuActivated(id) => {
                // #901: fired by a *native* OS menu bar (macOS `NSMenu` via
                // `Backend::install_menu_bar`) — see the doc comment on
                // `UiEvent::MenuActivated` distinguishing it from the drawn
                // `MenuSystem`'s click path, which resolves to
                // `quadraui::MenuEvent::Activated` further up in this same
                // method and calls this identical dispatcher. One action
                // path for both, not two: `render::build_menu_defs` gave
                // every leaf item's `WidgetId` the same string as its
                // `MENU_STRUCTURE` `action` field, so `id.as_str()` here is
                // exactly the command string `handle_menu_action` expects.
                self.handle_menu_action(id.as_str().to_string());
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::ContextMenuItemActivated(id) => {
                // #902: fired by a *native* right-click popup (macOS
                // `NSMenu` via `Backend::show_context_menu`, only reachable
                // when `render::context_menu_should_be_native` resolved
                // `true`). `id` is one of `context_menu_panel_to_quadraui_
                // context_menu`'s synthesised `"context:N"` ids — the exact
                // same ids `route_modal_overlay_click`'s in-window hit-test
                // (`ContextMenuHit::Item` → `context_menu_hit_to_idx`)
                // resolves, so routing the activation through
                // `apply_context_menu_route` reuses that one conversion
                // instead of duplicating it.
                let idx = crate::core::engine::context_menu_hit_to_idx(
                    &quadraui::ContextMenuHit::Item(id),
                );
                let route = match idx {
                    Some(idx) => render::ContextMenuRoute::Item(idx),
                    None => render::ContextMenuRoute::Dismiss,
                };
                self.apply_context_menu_route(route);
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::ContextMenuDismissed => {
                // #902: the native popup was dismissed without a selection
                // (Escape, click-away). Same close path a `ContextMenuRoute
                // ::Dismiss` from the in-window hit-test already takes.
                self.apply_context_menu_route(render::ContextMenuRoute::Dismiss);
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::MouseDown {
                button,
                position,
                modifiers,
                ..
            } => {
                let main = ctx.layout.main_content_bounds;
                let (w, h) = (main.width as f64, main.height as f64);
                match button {
                    MouseButton::Left if modifiers.ctrl => {
                        self.handle_ctrl_mouse_click(position.x as f64, position.y as f64);
                    }
                    MouseButton::Left => {
                        self.handle_mouse_click_msg(
                            position.x as f64,
                            position.y as f64,
                            w,
                            h,
                            modifiers.alt,
                        );
                    }
                    MouseButton::Right => {
                        let rx = position.x as f64;
                        let ry = position.y as f64;
                        // ── Modal-overlay rung (#733 review) ────────────
                        // A modal dialog eats every event, including
                        // right-clicks, so it can't be right-clicked
                        // through to the editor/tab context menu
                        // underneath — TUI's `handle_mouse` already
                        // returns unconditionally for any event kind
                        // while `engine.dialog.is_some()`. This backend's
                        // left-click path goes through
                        // `route_modal_overlay_click` via
                        // `handle_mouse_click_msg`, but the right-click
                        // path used to skip straight to tab/editor
                        // resolution below without consulting it, so a
                        // right-click on an open dialog opened the
                        // editor's context menu behind it. Route through
                        // the same shared rung (`ModalMouseAction::Other`)
                        // before doing anything else.
                        let modal_route =
                            self.route_modal_overlay(rx, ry, render::ModalMouseAction::Other);
                        if modal_route == render::ModalOverlayRoute::Swallow {
                            self.draw_needed.set(true);
                        } else {
                            // #546 FAILED-1: this used to unconditionally build
                            // `EditorRightClick`, so right-clicking a tab opened
                            // the *editor's* context menu (identical item list to
                            // right-clicking in the buffer) instead of a
                            // tab-specific one. Resolve the click against the
                            // last-painted tab-bar geometry first — read-only, no
                            // engine mutation — and only fall back to the editor
                            // menu when it isn't over a tab.
                            let tab_target = {
                                let engine = self.engine.borrow();
                                let layout_ref = self.cached_screen_layout.borrow();
                                layout_ref.as_ref().and_then(|layout| {
                                    resolve_tab_right_click(
                                        &engine,
                                        rx,
                                        ry,
                                        self.cached_line_height,
                                        self.cached_char_width,
                                        layout,
                                        &self.cached_tab_pixel_hits.borrow(),
                                        self.cached_frame_hit_map.borrow().as_ref(),
                                        &self.cached_tab_bar_zones.borrow(),
                                    )
                                })
                            };
                            if let Some((group_id, tab_idx)) = tab_target {
                                self.handle_tab_right_click(group_id, tab_idx, rx, ry);
                            } else {
                                self.handle_editor_right_click(rx, ry);
                            }
                        }
                    }
                    _ => {}
                }
                // Mouse clicks always require a redraw (cursor movement, selection,
                // focus change). draw_needed may already be set by the handler
                // above, but set it unconditionally so handle() returns
                // Reaction::Redraw even when a handler takes an early-return path.
                self.draw_needed.set(true);
            }
            UiEvent::DoubleClick { position, .. } => {
                self.handle_mouse_double_click_msg(position.x as f64, position.y as f64);
                self.draw_needed.set(true);
            }
            UiEvent::MouseMoved { position, buttons } => {
                self.mouse_pos_cell
                    .set((position.x as f64, position.y as f64));
                // ── Modal-overlay hover rung (#751) ─────────────────────
                // An open context menu tracks the pointer, exactly as TUI's
                // `handle_mouse` has always done. This backend had no hover
                // arm at all, so whichever item was selected when the menu
                // opened stayed highlighted wherever the pointer went (#373)
                // — and a keyboard Down after a mouse hover then moved from
                // the wrong row.
                if !buttons.left {
                    if let render::ModalOverlayRoute::ContextMenu(route) = self.route_modal_overlay(
                        position.x as f64,
                        position.y as f64,
                        render::ModalMouseAction::Move,
                    ) {
                        self.apply_context_menu_route(route);
                    }
                }
                if buttons.left {
                    let main = ctx.layout.main_content_bounds;
                    self.handle_mouse_drag_msg(
                        position.x as f64,
                        position.y as f64,
                        main.width as f64,
                        main.height as f64,
                    );
                }
            }
            UiEvent::MouseUp { .. } => {
                let main = ctx.layout.main_content_bounds;
                self.handle_mouse_up_msg(main.width as f64);
            }
            UiEvent::Scroll {
                delta, position, ..
            } => {
                // #646: record where the wheel event happened before dispatching.
                // `handle_mouse_scroll_msg` takes only the delta, and reads the pointer
                // back out of `last_editor_pointer` to decide which window (or
                // registered scroll surface) the wheel targets. Nothing set that
                // cell after the #540 Relm4→ShellApp migration removed the
                // `EventControllerMotion` that used to — see the field's doc — so
                // it was permanently `None` and every wheel event fell through to
                // the *focused* window regardless of the pointer (#240 behaviour
                // dead on GTK, still live on TUI). A wheel event carries its own
                // position, so use that directly rather than depending on a
                // preceding motion event.
                self.last_editor_pointer
                    .set(Some((position.x as f64, position.y as f64)));
                // #554: **negate y back to GTK's raw polarity.**
                //
                // Two conventions meet at this line and they disagree:
                //
                // - GDK's `EventControllerScroll` reports *positive dy = wheel
                //   down*.
                // - `UiEvent::Scroll.delta` follows quadraui's convention,
                //   *positive y = up toward the top of the content*.
                //   `quadraui::gtk::events::gdk_scroll_to_uievent` is what
                //   flips one into the other — it constructs
                //   `ScrollDelta::new(dx, -dy)`.
                //
                // Everything downstream of `handle_mouse_scroll_msg` — the
                // `delta_y > 0.0 => dir = 1` viewport step, the `picker_scroll`
                // sign, `Engine::handle_terminal_scroll`'s "> 0 = toward live"
                // policy — was written against GTK's raw polarity and is
                // unchanged since before the #540 Relm4→ShellApp migration.
                // Pre-migration the Relm4 `connect_scroll` closure fed it GTK's
                // `dy` directly (as the retired `Msg::MouseScroll`'s payload
                // — the whole bus is gone as of #732)
                // and *separately* pushed the negated `gdk_scroll_to_uievent`
                // form onto the backend event queue. The migration deleted that
                // closure and left the runner's already-negated `UiEvent::Scroll`
                // as the only source, so every wheel notch reached the engine
                // with the sign flipped and the editor scrolled backwards.
                //
                // Only y is negated: `gdk_scroll_to_uievent` passes `dx`
                // through unchanged, so `delta.x` is already GTK-raw.
                self.handle_mouse_scroll_msg(delta.x as f64, -(delta.y as f64));
            }
            UiEvent::WindowResized { .. } => {
                // Runner sets new line_height/char_width after resize.
                self.cached_line_height = backend.line_height() as f64;
                self.cached_char_width = backend.char_width() as f64;
                self.line_height_cell.set(self.cached_line_height);
                self.char_width_cell.set(self.cached_char_width);
                self.handle_resize();
            }
            UiEvent::WindowClose => {
                self.show_quit_confirm();
            }
            // #593: quadraui's runner reads the system clipboard on Ctrl+V /
            // Ctrl+Shift+V / middle-click and delivers the text here,
            // unconditionally consuming the key — there is no raw KeyPressed
            // fallback to catch a paste with. `Engine::route_paste` is the
            // same focus-priority router TUI's `UiEvent::ClipboardPaste` arm
            // already calls (`tui_main/shell_app.rs`), so this one arm covers
            // the command line, search/replace fields, explorer rename, and
            // the editor buffer — see that fn's doc for the full priority
            // chain.
            UiEvent::ClipboardPaste(text) => {
                self.engine.borrow_mut().route_paste(&text);
                self.draw_needed.set(true);
            }
            _ => {}
        }

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }

    /// The actual body of `ShellApp::tick` — see `handle_dispatch`'s
    /// doc comment (#813). `run_pending_native_dialog`, reachable from
    /// here via `apply_dialog_action`, can also request exit.
    fn tick_dispatch(&mut self, backend: &mut dyn quadraui::Backend) -> quadraui::Reaction {
        // Keep cached metrics up to date.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);

        // Retry the window capture until the runner has mapped it — see
        // `capture_window_and_apply_csd` (#552). No-ops once `self.window`
        // is `Some`.
        self.capture_window_and_apply_csd();

        // Drain the actions async GTK callbacks queued for this frame.
        for action in self.deferred.drain() {
            match action {
                DeferredAction::Resize => self.handle_resize(),
                DeferredAction::ToggleFocusExplorer => self.toggle_focus_explorer(),
                DeferredAction::ToggleFocusSearch => self.toggle_focus_search(),
                DeferredAction::ToggleSidebar => self.toggle_sidebar_panel(),
                DeferredAction::ToggleTerminal => self.toggle_terminal(),
                DeferredAction::ToggleTerminalMaximize => self.toggle_terminal_maximize(),
            }
        }

        // Poll the yank-highlight deadline armed by `run_post_key_epilogue`
        // (#813 — see `yank_hl_deadline`'s doc comment).
        if let Some(deadline) = self.yank_hl_deadline.get() {
            if std::time::Instant::now() >= deadline {
                self.clear_yank_highlight();
                self.yank_hl_deadline.set(None);
                self.draw_needed.set(true);
            }
        }

        // Run a file dialog requested this frame — needs the runner-owned
        // `backend` handle for `PlatformServices` (#572). See
        // `PendingFileDialog` for why this can't happen in the
        // `open_file_dialog` / `save_workspace_as_dialog` handlers themselves.
        if let Some(req) = self.pending_file_dialog.take() {
            self.run_pending_file_dialog(req, backend);
        }

        // Run a native message dialog queued by `render_content`'s
        // edge-trigger check (#727) — same reason as the file dialog above:
        // needs the runner-owned `backend` for `PlatformServices`, which
        // `render_content`'s paint callback must not block inside.
        if let Some(opts) = self.pending_native_dialog.take() {
            self.run_pending_native_dialog(opts, backend);
        }

        // Periodic background work: LSP, DAP, git, search, etc.
        self.handle_poll_tick();

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }
}

impl quadraui::ShellApp for App {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // Seed cached metrics from runner defaults.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.cached_ui_line_height = self.cached_line_height;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);
        // (#547) Seed the backend's nerd-fonts flag. The only prior call
        // site was the `Msg::CacheFontMetrics` arm, which stopped firing after
        // the #540 ShellApp migration, silently freezing quadraui's GTK backend
        // at its default of `false` — the cause of the explorer treeview
        // falling back to ASCII icons. (That arm had still never regained a
        // producer, so #732 deleted it; this call is the live replacement.)
        render::sync_nerd_fonts(backend, &self.engine.borrow());
        // (#937) Register the bundled Nerd Font subset and point the
        // backend's fallback cascade at it — required for glyphs to resolve
        // at all on Core Text/DirectWrite backends (macOS/Win-GUI); see
        // `render::register_nerd_font_fallback`'s doc for why this is a
        // one-time `setup()` call, not part of the per-frame sync above.
        render::register_nerd_font_fallback(backend);

        // Try to grab the runner-created GTK window now so minimize/maximize/
        // close work and the server-side WM titlebar is dropped in favour of
        // the drawn CSD row; `setup()` runs before `run_with_shell`'s runner
        // calls `window.present()`, so it is very likely not yet mapped and
        // this lookup finds nothing. `tick()` retries every frame until the
        // window is mapped, which is the reliable path (#552).
        self.capture_window_and_apply_csd();

        // GTK draws its own VSCode-style menu bar (File/Edit/View/...) — it
        // acts as the client-side titlebar, always visible (unlike TUI, which
        // only shows it in vscode-mode or via Alt). Historical GTK behaviour
        // pre-#540; menu defs were never re-populated after the ShellApp
        // migration deleted the Relm4 headerbar wiring. (#552)
        //
        // #901: a backend that declares `BackendCaps::native_menu` (macOS's
        // `MacBackend`) has a real OS menu bar — installing the *drawn* row
        // on top of it would paint a redundant in-window menu underneath the
        // system one (the bug this issue exists to fix). Same `MenuDef`s
        // either way — `render::menu_defs_to_menu_bar` just reshapes them —
        // so the two paths can never disagree about what's in the menu.
        let is_vscode_mode = self.engine.borrow().is_vscode_mode();
        let menu_defs = render::build_menu_defs(is_vscode_mode);
        if backend.backend_caps().native_menu {
            let bar = render::menu_defs_to_menu_bar(&menu_defs);
            // `install_menu_bar`'s only in-tree implementation (macOS's
            // `MacBackend`) asserts it is called on the real AppKit main
            // thread and panics otherwise — a documented quadraui
            // limitation with no portable pre-check exposed through the
            // `Backend` trait. Every real invocation of `ShellApp::setup`
            // *is* on the main thread (`quadraui::macos::shell_runner`'s
            // only entry point), so this never fires outside a test
            // harness — but `quadraui::macos::testing::driver_with_shell`
            // (used by `src/macos/mod.rs::mac_driver_tests`) necessarily
            // calls `setup` from a spawned test thread, same as every
            // other `#[test]` fn, per Rust's own test runner. Catching it
            // here keeps `setup()` — which every backend, including the
            // ones with no native menu, must be able to complete without
            // aborting the process — from taking the whole test process
            // down over a call this method doesn't otherwise depend on.
            // Filed upstream: `install_menu_bar` should degrade
            // gracefully off-main-thread the way its own sibling test
            // helpers already do (`menu_bar_install.rs`'s `let Some(mtm)
            // = MainThreadMarker::new() else { return }`), not hard
            // `.expect()`.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                backend.install_menu_bar(&bar);
            }))
            .is_err()
            {
                eprintln!(
                    "vimcode: Backend::install_menu_bar panicked (quadraui \
                     main-thread assertion, see vimcode#901) -- the native \
                     menu bar may be missing"
                );
            }
            self.engine.borrow_mut().menu_bar_visible = false;
        } else {
            self.engine.borrow_mut().menu_bar_visible = true;
        }
        self.engine
            .borrow()
            .menu_system
            .borrow_mut()
            .set_menus(menu_defs);

        // Apply initial CSS (no-op under the headless test harness, which has
        // no display to attach a provider to — see the field's doc, #646).
        if let Some(p) = &self.css_provider {
            let theme = Theme::from_name(&self.engine.borrow().settings.colorscheme);
            let combined = format!("{STATIC_CSS}\n{}", make_theme_css(&theme));
            p.load_css_data(&combined);
        }

        // Register the panel-keys accelerator set (toggle sidebar, fuzzy
        // finder, live grep, command palette, ...) on the runner's backend.
        // This was previously only wired for TUI (`tui_main::run` calls it
        // right after `TuiBackend::new()`); the GTK side's registration
        // function existed but was never called after the ShellApp
        // migration, so none of these 14 global shortcuts — including
        // Ctrl+Shift+P for the command palette — ever fired on GTK (#587).
        render::register_panel_accelerators(backend, &self.engine.borrow().settings.panel_keys);
    }

    fn render_content(
        &self,
        backend: &mut dyn quadraui::Backend,
        layout: &quadraui::AppShellLayout,
    ) {
        let engine = self.engine.borrow();
        let theme = Theme::from_name(&engine.settings.colorscheme);
        self.sync_per_frame_backend_state(backend, &engine, &theme);

        let lh = self.cached_line_height.max(backend.line_height() as f64);
        let cw = self.cached_char_width.max(backend.char_width() as f64);
        // Publish the value this frame paints with so click-time hit-tests can
        // use it (#555). `render_content` takes `&self`, so it cannot write
        // the plain `cached_line_height` field — which is seeded once in
        // `setup()` from the runner's *default* metrics and can therefore be
        // smaller than the `lh` every frame actually paints with. Hit-testing
        // painted geometry against the smaller value put row boundaries in the
        // wrong place (the picker resolved clicks two rows off) and clipped
        // the bottom of every single-row band, breadcrumbs included.
        self.painted_line_height.set(Some(lh));
        self.painted_char_width.set(Some(cw));

        let main = layout.main_content_bounds;
        let (x, y, w, h) = (
            main.x as f64,
            main.y as f64,
            main.width as f64,
            main.height as f64,
        );
        if w < 1.0 || h < 1.0 {
            return;
        }

        // ── Layout ────────────────────────────────────────────────────────────
        let tab_row_h = render::tab_row_height_px(lh);
        let tab_bar_h = render::tab_bar_height_px(lh, engine.settings.breadcrumbs);
        let per_window_status = engine.settings.window_status_line;
        let el = render::compute_editor_layout(&engine, h, lh, false);
        // `el.status_bar_h` is `compute_editor_layout`'s single source of
        // truth for this (identical formula to the `wildmenu_px`/
        // `status_rows` locals this replaced); reusing it here — instead of
        // recomputing a second copy — is what makes `editor_area_h` below
        // `el.editor_bottom` correctly reserve quickfix's band too.
        let status_bar_h = el.status_bar_h;
        // `el.editor_bottom` already subtracts quickfix_h/terminal_h/
        // debug_toolbar_h/separated_status_h/status_bar_h from `h` (menu_h
        // is 0 for GTK — the menu bar lives outside `main_content_bounds`,
        // see `compute_editor_layout`'s `menu_in_viewport` doc). Before
        // #670 this was hand-rolled here without the `quickfix_h` term, so
        // an open quickfix panel never reserved space and editor content
        // painted straight through where the panel now paints.
        let editor_area_h = el.editor_bottom.max(0.0);

        let editor_bounds = WindowRect::new(x, y, w, editor_area_h);
        // Hand the exact bounds/tab-bar-height this frame painted with to the
        // click + drag handlers, so divider hit-tests land on the painted line
        // instead of on a second, differently-originated guess (#582).
        self.cached_editor_bounds
            .set(Some((editor_bounds, tab_bar_h)));
        let (window_rects, _dividers) =
            engine.calculate_group_window_rects(editor_bounds, tab_bar_h);

        // #700: the breadcrumb row's own painted bounds must agree with the
        // fixed-pixel space `tab_bar_h` (above) already reserved for it above
        // the window content — plain `build_screen_layout` would assume the
        // breadcrumb row is exactly one `lh`-tall editor text line, which is
        // no longer true now that the row is a fixed 22px regardless of
        // `settings.font_size`.
        let screen = render::build_screen_layout_with_breadcrumb_row(
            &engine,
            &theme,
            &window_rects,
            lh,
            cw,
            false,
            render::BREADCRUMB_ROW_HEIGHT_PX,
            backend.scrollbar_reserve() as f64,
            render::gtk_minimap_sizing(),
        );

        // Cache for click handlers (move into RefCell, then borrow back for drawing).
        *self.cached_screen_layout.borrow_mut() = Some(screen);
        let screen_ref = self.cached_screen_layout.borrow();
        let screen = screen_ref.as_ref().unwrap();

        // #560 / #947: give the *click* backend a correctly-fonted editor
        // Pango context so mouse clicks resolve columns via the per-glyph
        // Pango inverse rather than a naive uniform-cell division.
        //
        // vimcode keeps a SEPARATE `GtkBackend` (`self.backend`) for click-time
        // hit-testing than the one quadraui's ShellApp runner creates and
        // paints with (`quadraui::gtk::run` owns the single DrawingArea and its
        // backend; see the `self.drawing_area` note in `tick`). The runner's
        // backend is the one that stashes `last_editor_pango_layout` during the
        // `frame.draw(backend)` calls below and is handed to `render_content`
        // as `backend` — but it is NOT `self.backend`, and the trait exposes no
        // way to copy its Pango context across. So `self.backend`, used by
        // `pixel_to_click_target -> editor_col_at_x`, had neither a stashed
        // editor layout nor a Pango context of its own and fell through to
        // `EditorLayout::col_at_x`'s uniform per-cell division: exact for
        // monospace glyphs, but drifting +1 column for every preceding wide
        // glyph (emoji ✅/🟡/❌/⏭, CJK) — the reported #560 symptom.
        //
        // quadraui#971 added `GtkBackend::editor_pango_layout()`, a last-resort
        // fallback inside `editor_col_at_x` that builds a layout from the
        // backend's *own* `pango_ctx` fonted with its own `editor_font_*`
        // state (set via `Backend::set_editor_font`) — so a `GtkBackend` that
        // has never painted a frame still resolves per-glyph, as long as (a)
        // it has a `pango_ctx` at all, which only `set_text_measurement_context`
        // below gives it, and (b) its `editor_font_*` matches what got painted.
        // Before #971 this seam had to hand-fake (b) by measuring a probe glyph
        // against the *painted* `char_width` and hardcoding the `"Monospace"`
        // family to match `quadraui::gtk::run`'s old default — see the #947
        // issue's "the trap" for why the two had to be kept in lockstep by hand.
        // Now that `sync_per_frame_backend_state` above pushes the *live*
        // `settings.font_family`/`font_size` onto the paint backend via
        // `set_editor_font`, we push the exact same values onto the click
        // backend here — both track the setting, so there is nothing left to
        // reproduce by probing. Re-set every frame so a runtime `:set guifont`
        // takes effect immediately.
        self.backend.borrow_mut().set_editor_font(
            &engine.settings.font_family,
            engine.settings.font_size as f32,
        );
        #[cfg(feature = "gui")]
        if let Some(click_ctx) = crate::gtk::click::build_editor_click_context() {
            self.backend
                .borrow_mut()
                .set_text_measurement_context(Box::new(click_ctx));
        }

        // ══ Editor band (#764, #735 slice 3) ═════════════════════════════════
        // Composed from `render::compose_editor_band`, then the `FrameHitMap`
        // is recovered from the very objects that walk painted — see
        // `compose_editor_band_rungs`.
        self.compose_editor_band_rungs(
            backend,
            &engine,
            screen,
            &theme,
            quadraui::Rect::new(x as f32, y as f32, w as f32, 0.0),
            (lh, cw),
            (tab_row_h, tab_bar_h),
        );

        // ── Editor-anchored popups (on top of the editor band) ────────────────
        // Completion menu, LSP hover, editor hover (rich markdown), diff peek,
        // signature help — see `paint_editor_popups_rung`. Not a `FrameOp`
        // rung: they are anchored to the *active window's* cursor rather than
        // to a band, and TUI composes them through its own
        // `paint_editor_popups` at exactly this point in the frame, between
        // the editor band and the bottom band.
        self.paint_editor_popups_rung(
            backend,
            screen,
            &theme,
            quadraui::Rect::new(x as f32, y as f32, w as f32, h as f32),
            lh,
            cw,
        );

        // ══ Bottom band (#765, #735 slice 4) ═════════════════════════════════
        // Composed from `render::compose_bottom_band` — see
        // `compose_bottom_band_rungs`.
        self.compose_bottom_band_rungs(
            backend,
            &engine,
            screen,
            layout,
            &theme,
            quadraui::Rect::new(x as f32, y as f32, w as f32, h as f32),
            &el,
            editor_area_h,
            (lh, cw),
        );

        // ══ Frame sequence (#766, #735 slice 6) ══════════════════════════════
        //
        // Composed from `render::compose_frame` — the single ordered artefact
        // both backends walk for everything around and on top of the editor
        // column. Slices 1-4 landed this as *four* ladders (editor, bottom,
        // chrome, overlay); slice 6 folds the chrome and overlay halves into
        // one `FrameOp` sequence, so the frame is no longer "a composer plus a
        // special-cased top band" a backend could get individually right and
        // jointly wrong. Geometry and rasterisation stay here, in pixels; only
        // the *order* and the *gates* are shared. `FRAME_Z_ORDER`'s doc comment
        // records which rungs changed position and the divergences that closed.
        //
        // Caches whose "absent" branch used to live in an `else` are cleared
        // here, before the walk: `compose_frame` returns only the live rungs,
        // so an absent rung has no arm left to run. Every arm gates itself on
        // the *value* it needs and, when it composes, records the rung by name
        // (`push(FrameOp::Dialog)`, never `push(op)`) — with `push(op)` the
        // record would follow the pattern the walk is at, so swapping two arms'
        // bodies would compose them in the wrong order while still recording
        // the right one.
        let status_y = y + h - status_bar_h;
        self.engine
            .borrow()
            .global_status_rect
            .set(quadraui::Rect::default());
        self.global_status_zones.borrow_mut().clear();
        self.painted_sidebar_bounds
            .set(layout.sidebar_content_bounds);

        // The title-bar band's rects are published unconditionally — empty when
        // the shell reserved nothing this frame — because `handle()`'s click
        // routing reads them on a *later* frame and must never resolve against
        // a stale value (the #695 paint/hit-test disagreement in miniature).
        // The `MenuRow` arm below overwrites them when the rung is live.
        let menu_row_rect = layout.title_bar_bounds.unwrap_or_default();
        self.menu_row_rect.set(menu_row_rect);
        self.menu_items_rect.set(menu_row_rect);
        self.title_bar_rect.set(quadraui::Rect::default());
        let mut app_icon_rect = quadraui::Rect::default();
        let mut menu_items_rect = menu_row_rect;
        let mut controls_rect: Option<quadraui::Rect> = None;
        let mut command_center_rect: Option<quadraui::Rect> = None;

        // The overlay tail's caches are cleared here for the same reason
        // (#766): the tail is part of this one walk now, so a rung whose gate
        // is off has no arm left to run and cannot clear its own cache from an
        // `else`. A stale `dialog_layout` / `context_menu_layout` /
        // `picker_popup_rect` / `tab_switcher_popup_rect` resolves the next
        // click against last frame's geometry — the #587 class of bug.
        self.engine.borrow().command_center_layout.replace(None);
        self.picker_popup_rect.set(None);
        self.folder_picker_popup_rect.set(None);
        self.tab_switcher_popup_rect.set(None);
        *self.context_menu_layout.borrow_mut() = None;
        *self.dialog_layout.borrow_mut() = None;
        self.engine.borrow().toast_layout.replace(None);
        // #727's native-dialog edge trigger, hoisted out of the `Dialog` arm
        // (#766): a *native* dialog is not a frame rung, so the arm no longer
        // runs for it. A native dialog must be presented exactly once per open
        // — `native_dialog_shown` is the edge: the first `render_content` call
        // to see a given open queues the present (via `pending_native_dialog`,
        // drained by `tick()` since the blocking `PlatformServices` call can't
        // run from inside this paint callback, mirroring `PendingFileDialog`
        // #572) and flips the flag; a *closed* dialog re-arms it.
        match screen
            .dialog
            .as_ref()
            .map(render::dialog_panel_to_quadraui_dialog)
            .as_ref()
            .and_then(quadraui::native_dialog_options)
        {
            Some(opts) => {
                if !self.native_dialog_shown.get() {
                    self.native_dialog_shown.set(true);
                    self.pending_native_dialog.set(Some(opts));
                }
            }
            None => {
                if screen.dialog.is_none() {
                    self.native_dialog_shown.set(false);
                }
            }
        }

        let popup_vp = backend.viewport();
        let popup_viewport = quadraui::Rect::new(0.0, 0.0, popup_vp.width, popup_vp.height);
        // Built once, before the walk, because its presence gate and its
        // `ToastStack` arm need the same value and `build_toast_stack` is not
        // free.
        let toast_stack = render::build_toast_stack(&engine);

        let mut presence =
            render::FramePresence::from_screen(screen, layout, render::FrameMetrics::px(lh, cw));
        presence.toast_stack = toast_stack.is_some();
        // #815: shared with TUI now — see `render::FrameOp::FolderPicker`.
        presence.folder_picker = self.folder_picker.borrow().is_some();
        // #727: a natively-expressible dialog is presented by the OS, not
        // composed into this frame, so the rung is not live. `dialog_layout`
        // stays cleared above and nothing is recorded — the sequence describes
        // what reached the canvas.
        presence.dialog = screen.dialog.is_some() && !self.native_dialog_shown.get();

        // #939: measure the title-bar band whenever the Command Center rung
        // is live, independent of whether `FrameOp::MenuRow` itself composes.
        // Before this, `controls_rect`/`command_center_rect` were populated
        // *only* inside the `MenuRow` match arm below, which is gated on
        // `presence.menu_row` — i.e. on `menu_bar_visible`. A native-menu
        // backend (macOS's `MacBackend`) sets `menu_bar_visible = false` to
        // suppress the redundant in-window row under AppKit's real menu bar
        // (#901), which left `command_center_rect` permanently `None` and
        // the `FrameOp::CommandCenter` arm's `command_center_rect.filter(...)`
        // guard always failed — the omnibar never painted even once its own
        // presence gate was split from `menu_row`'s (see
        // `render::FramePresence::from_screen`).
        //
        // When the drawn row itself is suppressed, measure with an *empty*
        // `MenuBar` and no controls bar: `measure_title_bar_bands` collapses
        // the menu-item and controls slots to zero width in that case (see
        // its doc), so the Command Center gets the *entire* band rather than
        // reserving room for labels and buttons that will never paint. This
        // is also why `controls_rect` stays `None` on a native-menu backend:
        // the only arm that ever paints from it (`FrameOp::MenuDropdown`'s
        // `paint_title_bar_band`) stays gated on `presence.menu_dropdown` —
        // itself still coupled to `menu_bar_visible` — so this does not
        // resurrect drawn window controls under AppKit's own traffic lights.
        if presence.command_center {
            // #940: a client-side-titlebar-capable backend (macOS's
            // `MacBackend`, which honours `ShellConfig::client_side_titlebar`
            // as of quadraui#947 — requested unconditionally in
            // `shell_config`) reports how much of the band's leading edge its
            // own native controls already occupy. `Rect::default()` — the
            // trait default, and the only value GTK/Win-GUI/TUI ever return —
            // means "nothing of the backend's own is in this band", so
            // `render::backend_draws_own_window_controls` is `false` and
            // `leading_inset` is `0.0` there: every line below is then a
            // no-op and this arm behaves exactly as it did before #940.
            let control_inset = backend.titlebar_control_inset();
            let leading_inset = control_inset.width.max(0.0);
            let inset_menu_row_rect =
                render::inset_titlebar_row_leading_edge(menu_row_rect, leading_inset);

            let (real_icon_rect, real_items_rect) =
                render::split_menu_row_for_app_icon(menu_row_rect, leading_inset);
            let (items_for_measure, bar_for_measure) = if presence.menu_row {
                // `app_icon_rect` is only ever assigned here, so on macOS
                // (where `presence.menu_row` is always `false` — #901, the
                // AppKit system menu bar owns the drawn row) it never picks
                // up a real value. That is fine today: its only reader
                // (`FrameOp::MenuDropdown`'s `paint_title_bar_band`) is
                // itself gated on `presence.menu_dropdown`, which stays
                // coupled to `menu_bar_visible` and so is also always
                // `false` on macOS — the app icon genuinely does not paint
                // via this path there yet (pre-existing from #939/#901, not
                // a #940 regression; the omnibar is the only thing #940
                // actually offsets clear of the native controls).
                app_icon_rect = real_icon_rect;
                (real_items_rect, engine.menu_system.borrow().menu_bar())
            } else {
                (
                    inset_menu_row_rect,
                    quadraui::MenuBar {
                        id: quadraui::WidgetId::new("native_menu_row_suppressed"),
                        items: Vec::new(),
                        open_item: None,
                        focused_item: None,
                    },
                )
            };
            menu_items_rect = items_for_measure;
            self.menu_items_rect.set(menu_items_rect);

            // A backend that draws its own controls must never *also* get
            // vimcode's drawn `controls_bar` — two sets of window controls is
            // exactly the bug #940 exists to prevent (see the module doc's
            // "keeps the native traffic lights" section). `presence.menu_row`
            // still gates it the same way it always did on every other
            // backend. Pure decision extracted to
            // `render::should_draw_window_controls` so it has a unit test
            // independent of any backend/driver (see that function's tests).
            let draw_controls =
                render::should_draw_window_controls(presence.menu_row, control_inset);
            let maximized = self.window.as_ref().is_some_and(|w| w.win_is_maximized());
            let controls_bar =
                draw_controls.then(|| render::window_controls_status_bar(&theme, maximized));
            let bands = render::measure_title_bar_bands(
                backend,
                menu_row_rect,
                items_for_measure,
                &bar_for_measure,
                controls_bar.as_ref(),
            );
            self.title_bar_rect.set(bands.controls);
            controls_rect = draw_controls.then_some(bands.controls);
            command_center_rect = Some(bands.command_center);
        }

        let mut composed: Vec<render::FrameOp> = Vec::new();
        for op in render::compose_frame(&presence) {
            match op {
                // ── Menu bar row (client-side chrome; #552): measure only ────
                // quadraui's `run_with_shell` GTK runner (single-DA
                // architecture, #217) creates the window undecorated with no
                // native titlebar/menu hosting. `ShellConfig::with_title_bar()`
                // (set in `run()`) reserves a full-width band across the top of
                // the *entire* shell — above the activity bar and sidebar too,
                // not just `main_content_bounds` — so GTK's drawn menu bar +
                // inline window controls span the whole window like a real
                // titlebar, and the activity bar/sidebar/main content the runner
                // hands us are already shifted down to make room. Mirrors the
                // pre-#540 Relm4 headerbar and TUI's identical row via the same
                // shared `engine.menu_system` / `Backend::draw_menu_bar`.
                //
                // This is layout-only (`menu_bar_layout`, no draw): the bar
                // itself — and the whole `menu_row_rect` band — is painted from
                // the `FrameOp::MenuDropdown` arm below.
                // Drawing the controls or the Command Center *here* (as this
                // used to) is pointless because that later `menu_system.render()`
                // repaints `draw_menu_bar` across the entire band and erases
                // them (#552 round-2/3 "buttons render blank"), so this rung
                // only stashes their target rects.
                //
                // #939: the actual measurement — the #720 app-icon split,
                // `menu_items_rect`, `controls_rect`, `command_center_rect` —
                // moved above the walk, into the `presence.command_center`
                // block, because the Command Center rung now composes even
                // when this one does not (native-menu backends). `menu_row`
                // implies `command_center` (both require the band to exist;
                // `menu_row` additionally requires `menu_bar_visible`), so
                // that block has already run with the *real* menu bar by the
                // time this arm is reached — there is nothing left to do here
                // but record that the rung composed.
                render::FrameOp::MenuRow => {
                    composed.push(render::FrameOp::MenuRow);
                }

                // ── Sidebar panel body ───────────────────────────────────────
                // The quadraui AppShell chrome (activity bar, sidebar header,
                // separator) is painted by the runner before `render_content`
                // is entered; this fills only the content area it exposes.
                render::FrameOp::SidebarPanel => {
                    if let Some(q_sb) = layout.sidebar_content_bounds {
                        self.paint_sidebar_panel_rung(
                            backend, &engine, screen, &theme, q_sb, lh, cw,
                        );
                        composed.push(render::FrameOp::SidebarPanel);
                    }
                }

                // ── Wildmenu bar (command Tab completion) ────────────────────
                render::FrameOp::Wildmenu => {
                    if let Some(ref wm) = screen.wildmenu {
                        // Shares the global bar's row when per-window status
                        // lines are on (there is no global bar to sit under).
                        let wm_y = if per_window_status {
                            status_y
                        } else {
                            status_y + lh
                        };
                        let wm_rect =
                            quadraui::Rect::new(x as f32, wm_y as f32, w as f32, lh as f32);
                        render::paint_wildmenu_rung(backend, wm, &theme, wm_rect);
                        composed.push(render::FrameOp::Wildmenu);
                    }
                }

                // ── Global status bar ────────────────────────────────────────
                // #752: publish the painted rect for `route_chrome_click`, the
                // twin of TUI's call site. The bespoke branch hit-test this
                // replaces re-derived the band from
                // `height - lh * rows - wildmenu_px` in the click handler — a
                // second copy of the arithmetic, and one that had no way to
                // know what was really drawn.
                render::FrameOp::StatusBar => {
                    if let Some(ref bar) = screen.global_status_bar {
                        let sb_rect =
                            quadraui::Rect::new(x as f32, status_y as f32, w as f32, lh as f32);
                        let sb_layout =
                            render::paint_global_status_bar_rung(backend, &engine, bar, sb_rect);
                        // Same zone recovery as the per-window and separated bars above.
                        *self.global_status_zones.borrow_mut() =
                            render::status_bar_zones_from_layout(&sb_layout);
                        composed.push(render::FrameOp::StatusBar);
                    }
                }

                // ── Command line ─────────────────────────────────────────────
                render::FrameOp::CommandLine => {
                    let cmd_y = status_y + (status_bar_h - lh);
                    let cmd = render::command_line_view(&screen.command);
                    let cmd_rect = quadraui::Rect::new(x as f32, cmd_y as f32, w as f32, lh as f32);
                    // #816: publish the painted rect for
                    // `render::command_line_click_char_idx` — the exact twin
                    // of `global_status_rect` above, and TUI's identical
                    // cache in `shell_app.rs`'s own `FrameOp::CommandLine`
                    // arm.
                    self.engine.borrow().command_line_rect.set(cmd_rect);
                    backend.draw_command_line(cmd_rect, &cmd);
                    composed.push(render::FrameOp::CommandLine);
                }

                // ── Folder / workspace picker modal (#815) ───────────────────
                // `quadraui::FolderPickerController::render` paints through
                // the shared `Palette` primitive — the identical method
                // TUI's `FrameOp::FolderPicker` arm calls, just with this
                // backend's own (pixel-unit) popup rect.
                render::FrameOp::FolderPicker => {
                    if let Some(ref picker) = *self.folder_picker.borrow() {
                        let popup_rect =
                            render::folder_picker_popup_rect(popup_viewport, lh as f32);
                        picker.render(popup_rect, backend);
                        // Cache the *painted* rect (#582/#646) — key/mouse
                        // handling read this instead of re-deriving it.
                        self.folder_picker_popup_rect.set(Some(popup_rect));
                        composed.push(render::FrameOp::FolderPicker);
                    }
                }

                // ── Menu dropdown overlay ────────────────────────────────────
                // First rung of the band: `MenuSystem::render` repaints
                // `draw_menu_bar` across the whole title-bar strip, so nothing
                // that wants to survive may be drawn into that band before it.
                // #735 moved the *modal* rungs above it (they used to paint
                // underneath on GTK and on top on TUI) — a modal dialog now
                // covers an open dropdown on both backends, matching
                // `route_modal_overlay_click`'s own "a dialog eats everything"
                // arbitration.
                // #766: the `engine.menu_bar_visible` check that used to open
                // this arm is `FramePresence::from_screen`'s now — and stricter,
                // because it also requires the shell to have reserved a band at
                // least one text line tall. This arm painted the whole title bar
                // into a degenerate rect before the fold.
                render::FrameOp::MenuDropdown => {
                    self.paint_title_bar_band(
                        backend,
                        &engine,
                        &theme,
                        menu_row_rect,
                        menu_items_rect,
                        app_icon_rect,
                        controls_rect,
                    );
                    composed.push(render::FrameOp::MenuDropdown);
                }

                // ── Command Center: nav arrows + search box (#676) ────────────
                // Painted *after* `menu_system.render()` above, which repaints
                // `draw_menu_bar` across the entire `menu_row_rect` band and
                // would erase anything drawn here first — the identical
                // ordering hazard documented on the window controls (#552
                // round-2/3 "buttons render blank"). This is the VS Code-style
                // Command Center dropped by the #540 Relm4→ShellApp cutover and
                // never re-wired: it used to live in the deleted `impl
                // SimpleComponent for App` `view!` scaffolding. Cached into
                // `engine.command_center_layout` for `handle()`'s click
                // hit-test, mirroring TUI's `shell_app.rs` (#635 Stage 6b item
                // A) and `mouse.rs`'s "Menu bar row click — command center
                // only".
                render::FrameOp::CommandCenter => {
                    if let Some(cc_rect) = command_center_rect.filter(|r| r.width >= 1.0) {
                        let title = engine
                            .cwd
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "VimCode".to_string());
                        let cc = render::build_command_center_view(
                            engine.tab_nav_can_go_back(),
                            engine.tab_nav_can_go_forward(),
                            &title,
                        );
                        render::paint_command_center_rung(backend, &engine, cc_rect, &cc);
                        composed.push(render::FrameOp::CommandCenter);
                    }
                }

                // ── Find/replace overlay (#671) ──────────────────────────────
                // Confirmed by #592 to open in engine state (`KEYDBG: OVERLAY
                // STATE OPEN: ["find_replace"]`) with nothing painting on GTK.
                // Unlike quickfix/panel_hover (#670) there *was* a dead painter
                // to port — `draw.rs::draw_find_replace_popup` — but it routed
                // through `Surface::FindReplace` with a rect the rasteriser
                // ignores; calling `Backend::draw_find_replace` directly (same
                // trait method TUI's `TuiShellApp::render_content` calls) is
                // simpler and identical in effect. The GTK rasteriser positions
                // the panel from its own `panel.group_bounds` (already absolute
                // pixel coordinates — #550, same as TUI's absolute cell
                // coordinates) and reads `current_line_height` /
                // `current_char_width` off the backend (set once per frame by
                // quadraui's GTK runner before `render_content` runs), so the
                // `rect` argument here is unused by the GTK rasteriser too;
                // passed for parity with the trait's signature and the TUI call
                // site.
                render::FrameOp::FindReplace => {
                    if let Some(ref find_replace) = screen.find_replace {
                        render::paint_find_replace_rung(backend, find_replace, popup_viewport);
                        composed.push(render::FrameOp::FindReplace);
                    }
                }

                // ── Picker / command-palette overlay (#587) ──────────────────
                // Same class of bug #546 fixed for dialog/context-menu: the
                // palette was painted only by the dead legacy `draw_editor`
                // Cairo path (`draw.rs::draw_picker_popup`), which has zero live
                // callers under ShellApp. So `Ctrl+Shift+P` opened the picker in
                // engine state (`picker_open = true`, items populated) but
                // nothing ever painted — the "command palette fails to open
                // silently" symptom. Geometry comes from the same generic
                // helpers the legacy path used (`PickerGeometry` +
                // `gtk_picker_sizing`), so no Pango/Cairo access is needed here.
                render::FrameOp::UnifiedPicker => {
                    if let Some(ref picker) = screen.picker {
                        let rect = render::paint_picker_rung(
                            backend,
                            picker,
                            popup_viewport,
                            &render::gtk_picker_sizing(lh as f32),
                        );
                        // Hand the *painted* rect to the click/drag handlers (#555).
                        self.picker_popup_rect.set(Some(rect));
                        composed.push(render::FrameOp::UnifiedPicker);
                    }
                }

                // ── Tab switcher popup (Ctrl+Tab MRU list) (#671) ────────────
                // `self.tab_switcher_popup_rect` already exists and is read by
                // `handle_mouse_press`'s "Tab switcher modal arbitration" block
                // (added ahead of this painter, expecting to be fed) — this is
                // the first frame that actually sets it. Sizing/positioning
                // ported from the dead `draw.rs::draw_tab_switcher_popup_list`
                // (pixel-tuned clamp(350, 600) width, unlike TUI's
                // percent-of-terminal-columns sizing, which wouldn't make sense
                // in pixel space); content comes from the same shared
                // `render::tab_switcher_to_quadraui_list_view` adapter TUI's
                // `TuiShellApp::render_content` uses, through
                // `Backend::draw_list`.
                render::FrameOp::TabSwitcher => {
                    if let Some(ref ts) = screen.tab_switcher {
                        // #733: geometry comes from the shared
                        // `TabSwitcherGeometry` so the rect handed to
                        // `route_modal_overlay_click` below is the rect that was
                        // painted, and TUI resolves the identical popup through
                        // the same code with its own sizing constant.
                        if let Some(geo) = render::TabSwitcherGeometry::compute(
                            popup_viewport,
                            ts.items.len(),
                            &render::gtk_tab_switcher_sizing(lh as f32),
                        ) {
                            let list =
                                render::tab_switcher_to_quadraui_list_view(ts, geo.visible_rows);
                            backend.draw_list(geo.bounds, &list);
                            self.tab_switcher_popup_rect.set(Some(geo.bounds));
                            composed.push(render::FrameOp::TabSwitcher);
                        }
                    }
                }

                // ── Context menu (#546) ──────────────────────────────────────
                // The ShellApp render path never painted `screen.context_menu`
                // at all — its draw + click-geometry cache was populated only by
                // the dead legacy `draw_editor` Cairo path (src/gtk/draw.rs),
                // which has zero live callers under ShellApp, leaving right-click
                // menus invisible and unclickable. Drawn with only generic
                // `Backend` metrics (`render::context_menu_generic_layout`,
                // shared with TUI) since this fn has no raw Pango/Cairo access.
                render::FrameOp::ContextMenu => {
                    if let Some(panel) =
                        screen.context_menu.as_ref().filter(|p| !p.items.is_empty())
                    {
                        // #902: a native popup (`Backend::show_context_menu`)
                        // paints nothing in-window — no layout to cache, and
                        // no in-window rung to record as painted. Gated on
                        // the same `BackendCaps::native_menu` capability
                        // #901 uses for the menu bar, via the `menu_style`
                        // setting.
                        let native = render::context_menu_should_be_native(
                            engine.settings.menu_style,
                            backend.backend_caps(),
                        );
                        let mlayout = render::paint_context_menu_rung(
                            backend,
                            panel,
                            popup_viewport,
                            cw,
                            lh,
                            0.0,
                            native,
                        );
                        let painted = mlayout.is_some();
                        *self.context_menu_layout.borrow_mut() = mlayout;
                        if painted {
                            composed.push(render::FrameOp::ContextMenu);
                        }
                    }
                }

                // ── Modal dialog (#546) ──────────────────────────────────────
                // Same #546 story as the context menu above: invisible AND
                // undismissable by mouse under ShellApp — `dialog.is_some()`
                // stayed true forever and `handle_mouse_click_msg`'s dialog block
                // swallowed all subsequent clicks. #735 moved it *above* the
                // context menu (it used to paint underneath on GTK, and on top
                // on TUI): a dialog is the surface `route_modal_overlay_click`
                // hands every event to, so it must also be the surface the user
                // can see.
                //
                // #727: a natively-expressible `screen.dialog` (no `DialogTable`,
                // no text input — `quadraui::native_dialog_options` is the single
                // source of truth for that split) goes through a real OS
                // `AlertDialog` instead of this in-canvas primitive, and is
                // therefore *not* live as a frame rung at all: nothing is
                // composed into this frame, so nothing is recorded. Both halves
                // of that split — the presence gate and the once-per-open native
                // present — are stated before the walk; this arm is the
                // in-canvas half only. A dialog reaching here carries a
                // `DialogTable` or a text input (e.g. the SSH-passphrase
                // prompt), which no native alert facility hosts.
                render::FrameOp::Dialog => {
                    if let Some(panel) = screen.dialog.as_ref() {
                        let dlayout =
                            render::paint_dialog_rung(backend, panel, popup_viewport, cw, lh);
                        *self.dialog_layout.borrow_mut() = Some(dlayout);
                        composed.push(render::FrameOp::Dialog);
                    }
                }

                // ── Toast overlay (#454) — top of the band ───────────────────
                // Anchored to the full window viewport (matches TUI's
                // `layout.window_bounds`), not just `main_content_bounds`, so it
                // sits in the bottom-right corner of the whole app like the
                // TUI/VSCode toasts. `Backend::draw_toast_stack` does its own
                // pango measurement internally (unlike `dialog`/`context_menu`
                // above, whose generic layout is computed vimcode-side), so its
                // returned layout is the only source of truth — cached for
                // `handle_mouse_click_msg`'s hit-test → `handle_toast_hit`
                // dispatch, and the first rung `route_modal_overlay_click`
                // arbitrates.
                render::FrameOp::ToastStack => {
                    if let Some(ref stack) = toast_stack {
                        render::paint_toast_stack_rung(backend, &engine, stack, popup_viewport);
                        composed.push(render::FrameOp::ToastStack);
                    }
                }
            }
        }

        *self.composed_frame.borrow_mut() = composed;
        // Read back through the field rather than the local, so the *stored*
        // observable is what gets validated — a frame that recorded one thing
        // and composed another would be a lie the tests then trusted.
        if let Err(why) = render::check_frame_order(&self.composed_frame.borrow()) {
            debug_assert!(false, "GTK {why}");
        }
    }

    fn handle(
        &mut self,
        event: quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) -> quadraui::Reaction {
        let reaction = self.handle_dispatch(event, backend, ctx);
        if self.exit_requested.get() {
            return quadraui::Reaction::Exit;
        }
        reaction
    }

    fn tick(&mut self, backend: &mut dyn quadraui::Backend) -> quadraui::Reaction {
        let reaction = self.tick_dispatch(backend);
        if self.exit_requested.get() {
            return quadraui::Reaction::Exit;
        }
        reaction
    }

    /// #1064 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 12): the
    /// app-initiated half of the runner ↔ shadow panel sync. Was
    /// unoverridden on `App` (the trait default always returns `None`), so
    /// `ShellAdapter::apply_requested_panel` — polled once after every
    /// `handle()`/`tick()` dispatch — never saw a switch to apply, no
    /// matter what the engine did to its own `app_shell`/`ext_panel_active`.
    ///
    /// The gap: `App::render_content` paints the sidebar's *content* by
    /// reading `engine.app_shell`/`engine.ext_panel_active` directly (the
    /// shadow), so a panel switch the engine makes on its own — with no
    /// runner click involved, e.g. `Engine::process_pending_sidebar`'s DAP
    /// `dap_wants_sidebar` reveal, or `Self::toggle_focus_explorer`/
    /// `Self::toggle_focus_search`'s keyboard accelerators — always painted
    /// the *right* content. But the runner's own chrome (the activity-bar
    /// highlight, and the sidebar-header title `quadraui::AppShell::render`
    /// paints from **its own**, entirely separate, `active_panel()`) has no
    /// other channel to learn about the change — it only ever moves in
    /// response to `AppShell::handle`'s own click hit-testing, or this poll
    /// — so it silently kept showing the previous panel's title forever.
    ///
    /// `TuiShellApp::take_requested_panel` already had this override (see
    /// its own doc for the shared mechanics, mirrored verbatim here); this
    /// is the same logic against `App`'s fields.
    fn take_requested_panel(&mut self) -> Option<quadraui::WidgetId> {
        let engine = self.engine.borrow();
        if !engine.app_shell.sidebar_visible() {
            return None;
        }
        // #557: an extension panel takes over the sidebar body *without*
        // touching the shadow `app_shell`'s active-panel id (`switch_panel`'s
        // `render::apply_activity_panel_switch` call leaves it alone for an
        // `ext:` id), so `engine.ext_panel_active` — not `active_panel_id()`
        // — is what the runner has to follow while one is open.
        if let Some(name) = engine.ext_panel_active.as_deref() {
            let id = quadraui::WidgetId::new(crate::core::engine::sidebar::ext_panel_id(name));
            if self.last_shell_panel.as_ref() == Some(&id) {
                return None;
            }
            self.suppress_shell_panel_echo = true;
            return Some(id);
        }
        let current = engine.app_shell.active_panel_id()?.clone();
        if self.last_shell_panel.as_ref() == Some(&current) {
            return None;
        }
        self.suppress_shell_panel_echo = true;
        Some(current)
    }

    fn on_shell_event(&mut self, event: &quadraui::AppShellEvent) {
        use quadraui::AppShellEvent;
        // #1062: the shadow-`engine.app_shell` sync, unconditionally and
        // first — see `render::sync_shell_event_shadow`'s rung comment for
        // why this call has to come before any of the id-specific branching
        // below rather than be repeated inside each arm. GTK has no id that
        // needs `ShellShadowSyncHost::panel_absent_from_shadow` to answer
        // `true` (it has no hamburger panel), so `GtkShellShadowHost` is a
        // unit struct.
        {
            let mut engine = self.engine.borrow_mut();
            render::sync_shell_event_shadow(event, &mut engine, &GtkShellShadowHost);
        }
        match event {
            AppShellEvent::PanelChanged { panel_id } => {
                // #1064: record what the runner's own `AppShell` now
                // believes is active, whether this notification came from
                // a real click or from `take_requested_panel`'s own echo
                // below — see `Self::last_shell_panel`'s doc.
                self.last_shell_panel = Some(panel_id.clone());
                if std::mem::take(&mut self.suppress_shell_panel_echo) {
                    // Echo of our own `take_requested_panel` reconciliation:
                    // the engine already holds this state (an app-initiated
                    // switch, e.g. a DAP reveal or a panel-focus keyboard
                    // accelerator) — re-running `switch_panel` below would
                    // toggle an already-active `ext:` panel back **off**
                    // (`render::apply_activity_panel_switch`'s
                    // `already_showing` arm treats a second "click" on the
                    // active plugin panel as a close).
                    return;
                }
                // #557: plugin-provided panels are now real `PanelDefinition`s
                // in the runner's `AppShell` (`build_shell_config`), so their
                // icon clicks arrive here like any built-in panel's. They are
                // *not* engine-`AppShell` panels though — `render_content`
                // dispatches on `engine.ext_panel_active`, which
                // `sync_shell_event_shadow` deliberately leaves untouched for
                // an `ext:` id (see that function's doc) — so route them
                // through the existing `switch_panel` handler that owns the
                // ext-panel focus/toggle bookkeeping.
                if is_ext_panel_id(panel_id.as_str()) {
                    self.switch_panel(panel_id.as_str().to_string());
                    return;
                }
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarHidden => {
                // #557: this is also how a *second* click on an open
                // extension panel's icon arrives — `sync_shell_event_shadow`
                // already dropped the plugin panel's claim (its
                // `SidebarHidden` arm clears the same two fields
                // unconditionally). Re-opening still works:
                // `AppShell::handle_activity_click` reports a click on the
                // active panel as `PanelChanged`, not `SidebarHidden`, once
                // the sidebar is hidden.
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarResized { .. } => {}
            AppShellEvent::BottomItemClicked { id } => {
                // The runner treats bottom activity-bar items as action
                // buttons (not sidebar panels), so it never toggles or
                // hides on its own — it only ever reports the click
                // (TUI's `on_shell_event`, same arm, carries the matching
                // comment). #1057: this used to unconditionally
                // `show_panel`, so a second click on an already-open
                // bottom item (e.g. "bottom:settings") re-showed it
                // instead of collapsing the sidebar like VS Code does for
                // an active-tab click — while TUI, one click handler
                // over, already ran the toggle. Route through
                // `switch_panel`, the same shared
                // `render::apply_activity_panel_switch` call site
                // `PanelChanged`'s ext-panel arm above already uses, so
                // both backends make the identical toggle decision from
                // one place instead of drifting again.
                self.switch_panel(id.as_str().to_string());
            }
            _ => {}
        }
    }

    /// #1057: the ctx-aware override TUI's `TuiShellApp` already had (its
    /// own title-bar sync, quadraui#617) — `App` only implemented the
    /// deprecated ctx-less [`Self::on_shell_event`] until now, so nothing
    /// here could ever push a shell-state change back into the runner's own
    /// `AppShell` on the same frame an event fires.
    ///
    /// That gap stayed invisible as long as every `AppShellEvent` arm's
    /// runner-visible outcome was something the runner had *already*
    /// decided before calling in — `PanelChanged`/`SidebarHidden` for a top
    /// panel: the runner's own `AppShell` toggles itself first (that's
    /// *why* it reports one or the other), and `on_shell_event` just
    /// mirrors that decision into `engine.app_shell`, the shadow copy.
    /// `BottomItemClicked` breaks that assumption: the runner never toggles
    /// a *bottom* item itself (see that arm's own doc, above — it only
    /// ever reports the click), so the toggle-to-hide decision is 100% made
    /// inside `on_shell_event`, entirely within `engine.app_shell`, with no
    /// way to tell the runner. Without this override, `engine.app_shell.
    /// sidebar_visible()` correctly flips to `false` on a second Settings
    /// click, but the runner's own `AppShell` — which is what actually
    /// determines whether `render_content`'s sidebar column exists in the
    /// composited frame, not `engine.app_shell` — never learns, and keeps
    /// painting the sidebar as if nothing changed. Push the shadow's new
    /// state through the same [`Self::sync_runner_sidebar_visibility`] the
    /// key-dispatch epilogue already uses for the same reason (#762).
    ///
    /// Verified directly: before this override existed,
    /// `bottom_item_second_click_collapses_sidebar`'s `gtk`/`tui` arms in
    /// `src/harness.rs` went red at the second-click assertion — the
    /// engine-side state was already correct (`sidebar_visible() == false`),
    /// only the paint wasn't following it.
    fn on_shell_event_ctx(
        &mut self,
        event: &quadraui::AppShellEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        #[allow(deprecated)]
        self.on_shell_event(event);
        self.sync_runner_sidebar_visibility(ctx);
    }
}

#[cfg(test)]
mod portable_entry_point_tests {
    //! #859: coverage for the two backend-neutral seams the macOS wrapper
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
        // #940: the opt-in itself must reach the runner — a capable backend
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
    }

    /// #949 review: makes the "closes the macOS/Win-GUI settings hot-reload
    /// gap for free" claim testable. `handle_poll_tick` used to be reached
    /// only via a GTK-only `gio::FileMonitor` callback
    /// (`DeferredAction::SettingsFileChanged`, now deleted); it now calls
    /// `settings_file_changed` — and so `Engine::check_settings_reload`'s
    /// portable mtime poll — unconditionally, every tick, for every GUI
    /// backend `App` serves (this constructor is the backend-neutral one:
    /// see `App::new_headless`'s doc). This drives that exact call site
    /// directly against a real `App` + `Engine`, pointed at a private temp
    /// file via `core::settings::TestSettingsPathGuard` (see that type's
    /// doc for why a thread-local override, not a `$HOME` mutation — the
    /// seam this test needed and the codebase didn't have before this fix
    /// round), and asserts the reload actually took: `line_numbers` moves
    /// from the constructor's default (`None`) to what the on-disk file
    /// says (`Absolute`) purely from calling `handle_poll_tick()`, with no
    /// `DeferredAction`/file-monitor callback involved anywhere.
    ///
    /// This is a state assertion, not a painted-pixel one — CLAUDE.md's
    /// "assert on rendered output, not state" rule exists to catch a paint
    /// path that never reads the state it populates (#587/#592). That
    /// specific failure mode doesn't apply here: `check_settings_reload`
    /// mutates `engine.settings` directly, and every frame already reads
    /// `engine.settings` (colorscheme, the line-number gutter, tabstop,
    /// …) — there is no separate "did the paint path get wired up"
    /// question left to ask, only "did the poll fire", which this answers
    /// unambiguously. A genuine pixel-level check — repaint via
    /// `GtkDriver` after the reload and assert the gutter appears — needs
    /// `GtkDriver`/`ConformanceDriver` to expose a way to pump
    /// `AppLogic::tick` headlessly; as of this repo's pinned quadraui rev
    /// neither does (`GtkDriver` has no `tick()`/mutable-`Backend`
    /// accessor, unlike `quadraui::tui::testing::TuiDriver::tick()` —
    /// confirmed by reading `quadraui/src/gtk/testing.rs` and
    /// `quadraui/src/testing/mod.rs::ConformanceDriver` at the pinned rev;
    /// this repo's own `crate::gtk::testing` module doc already says as
    /// much: "No main loop. `tick()` is never pumped by the driver.").
    /// Adding that pump is quadraui-side test infrastructure, not a
    /// vimcode backend fix, so per CLAUDE.md's Platform-Neutrality Rule it
    /// belongs in a quadraui issue, not a vimcode-side workaround.
    #[cfg(feature = "gui")]
    #[test]
    fn handle_poll_tick_reloads_settings_changed_on_disk() {
        use crate::core::settings::{LineNumberMode, TestSettingsPathGuard};

        let tmp = std::env::temp_dir().join(format!(
            "vimcode_test_949_handle_poll_tick_{:?}.json",
            std::thread::current().id()
        ));
        std::fs::write(&tmp, r#"{"line_numbers":"Absolute"}"#).expect("write temp settings.json");
        let _guard = TestSettingsPathGuard::install(tmp.clone());

        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        assert_eq!(
            engine.borrow().settings.line_numbers,
            LineNumberMode::None,
            "precondition: the constructor's default must differ from the \
             on-disk value, or a reload would be indistinguishable from a no-op"
        );
        let mut app = App::new_headless(Rc::clone(&engine));

        app.handle_poll_tick();

        assert_eq!(
            engine.borrow().settings.line_numbers,
            LineNumberMode::Absolute,
            "handle_poll_tick did not pick up the externally-edited settings file"
        );

        let _ = std::fs::remove_file(&tmp);
    }
}
