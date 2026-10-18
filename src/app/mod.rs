//! `struct App` — the backend-neutral editor shell application.
//!
//! `App`, its inherent `impl` blocks and its `impl quadraui::ShellApp for App`
//! are portable shell logic shared across native backends. This file keeps
//! everything a second backend would otherwise have had to re-implement,
//! leaving platform-specific code in the backend modules.
//!
//! # Why this module no longer needs `#[cfg(feature = "gui")]`
//!
//! `backend` is typed `Box<dyn quadraui::Backend>` with no local supertrait,
//! and all platform-typed fields have been deleted or type-erased:
//!
//! 1. **The platform-typed fields.** `window` was deleted once
//!    `quadraui::Backend::window()` (`WindowControl`) became a full
//!    replacement. `settings_monitor` was deleted; `Engine::check_settings_reload`'s
//!    portable mtime poll replaced it. `css_provider` was deleted; the
//!    equivalent stylesheet moved into `GtkPlatformServices`.
//! 2. **The platform hook call sites** (colorscheme reload, OS window title /
//!    size / maximized-check / decoration / minimize) now go through
//!    `quadraui::Backend::window()` (`WindowControl`) and compile for every
//!    feature set. Only the `gdk::Display`/`gtk4::IconTheme` icon-search-path
//!    setup — [`crate::gtk::util::add_icon_theme_search_path`] — stays
//!    genuinely GTK-only, called directly by `crate::gtk::run` instead of
//!    from inside this file.
//! 3. **`crate::gtk::{click, css, util}`.** The portable majority moved to
//!    the backend-neutral `crate::click`/`crate::app_support`. `css.rs` is
//!    gone. The genuinely GTK-only remainder — `util`'s icon and log helpers
//!    plus the icon-theme search path — stayed in `crate::gtk` and is reached
//!    through explicit `#[cfg(feature = "gui")]` call sites.
//!
//! Per the Platform-Neutrality Rule, the parts that stay behind the `gui`
//! feature are exactly the parts that still need quadraui-side infrastructure
//! rather than new per-backend code.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::core;
use crate::render;

use core::engine::EngineAction;
use core::engine::PluginViewHost;
use core::{Engine, WindowRect};
use render::Theme;

use crate::app_support::*;
use crate::click::*;
use crate::core::engine::sidebar::*;
#[cfg(all(feature = "gui", any(test, feature = "test-support")))]
use crate::gtk::backend;

// ─── Panel-key accelerator registry ─────────────────────────────────────────
//
// The 15-entry `PanelAccelerator` id table (`render::ACC_*`) and the
// dispatcher itself (`render::dispatch_panel_accelerator`) are shared with
// TUI (#761 / #734 slice 6) — see the rung's header comment in `render.rs`.
// What's left here is registration (this backend's own `quadraui::Backend`
// instance); `render::dispatch_panel_accelerator` queues onto `self.deferred`
// directly for the five actions that need GTK's `DeferredQueue` seam.

/// Work that a GTK callback with no `&mut App` in hand must hand back to the next frame.
///
/// These six deferrals are the remnants of the Relm4-era `Msg` bus.
/// They are genuine deferrals: each originates somewhere that cannot call
/// an `&mut self` method — `render::dispatch_panel_accelerator`'s
/// `UiEvent::Accelerator` arm holds only a clone of the queue.
/// A yank-highlight one-shot used to queue here too but now uses the portable
/// `yank_hl_deadline` poll-in-`tick` pattern. A settings-reload signal used to
/// queue here; `Engine::check_settings_reload`'s portable mtime poll replaced it.
///
/// quadraui has no deferral seam of its own — `ShellApp::tick` *is* the seam —
/// and the queued payload is app-specific, so the queue stays here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeferredAction {
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
    pub(crate) fn new() -> Self {
        DeferredQueue(Rc::new(RefCell::new(VecDeque::new())))
    }

    /// Enqueue an action for processing in the next `tick()` call.
    pub(crate) fn send(&self, action: DeferredAction) {
        self.0.borrow_mut().push_back(action);
    }

    /// Take all pending actions, leaving the queue empty.
    pub(crate) fn drain(&self) -> Vec<DeferredAction> {
        let mut q = self.0.borrow_mut();
        q.drain(..).collect()
    }
}

// `set_current_line_height`/`set_current_char_width` are now
// `quadraui::Backend` methods (JDonaghy/quadraui#1086), so `App::backend`
// is typed `Box<dyn quadraui::Backend>` directly. Every call site below
// calls the `quadraui::Backend` method instead of a local trait.

// `quadraui::Backend::window()` (`WindowControl`) provides portable access
// to `is_maximized()`/`set_decorated()` on all windowed backends and TUI.
// Every caller now reaches it directly. `WindowControl` is backed on every
// backend (GTK, macOS, Win-GUI, and TUI), so none of this needs a gate.

// `Backend::set_theme` reloads the stylesheet on every call.
// `sync_per_frame_backend_state` already makes that call every frame,
// so there is nothing left for `App` itself to own or reload.

pub(crate) struct App {
    pub(crate) engine: Rc<RefCell<Engine>>,
    /// Set to true in update() whenever a draw is needed; cleared by the #[watch] block.
    /// This prevents the 20/sec SearchPollTick timer from unconditionally calling queue_draw().
    pub(crate) draw_needed: Rc<Cell<bool>>,
    /// Set by [`App::save_session_and_exit`] when the user quits normally
    /// (`Quit`/`SaveQuit`/`quit_menu`/the unsaved-changes confirm dialog).
    /// Checked at the end of `ShellApp::handle`/`tick` so a
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
    /// polled by `tick_dispatch`. Replaces a one-shot toolkit timer
    /// with the same portable poll-in-`tick` pattern TUI already used
    /// (`yank_hl_deadline` in `tui_main/shell_app.rs`) — no toolkit timer
    /// needed.
    pub(crate) yank_hl_deadline: Cell<Option<std::time::Instant>>,
    /// A file dialog requested this frame, drained by the next `tick()`
    /// call (which has the `backend` handle `PlatformServices` needs).
    /// See [`PendingFileDialog`].
    pub(crate) pending_file_dialog: Cell<Option<PendingFileDialog>>,
    pub(crate) cached_line_height: f64,
    pub(crate) cached_char_width: f64,
    /// `Theme::from_name(colorscheme).activity_active_accent`, keyed by the
    /// colorscheme name it was computed from. `paint_active_accent`
    /// (`src/app/shell.rs`) runs on every dispatched event, which would
    /// otherwise double `pre_render`/`render_content`'s own per-frame
    /// `Theme::from_name` call — for a custom theme loaded from
    /// `~/.config/vimcode/themes/`, that call is a filesystem read plus a
    /// JSON parse, not just a struct literal. Recomputed only when the
    /// colorscheme name changes.
    pub(crate) cached_active_accent: RefCell<Option<(String, quadraui::Color)>>,
    /// Absolute surface pixel position of the wheel event currently being handled
    /// (the same frame `render_content` paints in). Read by `handle_mouse_scroll_msg`
    /// to route the wheel to the registered scroll surface or editor pane under the
    /// cursor — matches TUI behaviour.
    ///
    /// Written by `ShellApp::handle`'s `UiEvent::Scroll` arm from the event's own
    /// `position`. Sourcing it from the wheel event itself ensures the position
    /// always matches the current frame's scroll event.
    pub(crate) last_editor_pointer: Rc<Cell<Option<(f64, f64)>>>,
    /// Cached line height for the UI font (sidebars, panels).
    /// Computed alongside `cached_line_height` in `CacheFontMetrics`.
    pub(crate) cached_ui_line_height: f64,
    /// Cached dialog layout from the last `render_content` paint —
    /// mirrors `context_menu_layout` below. Button-click and outside-click
    /// hit-testing both read `DialogLayout::hit_test` off this instead of a
    /// hand-rolled per-backend rect cache.
    pub(crate) dialog_layout: Rc<RefCell<Option<quadraui::DialogLayout>>>,
    /// Shared `quadraui::FolderPickerController` (adopted from TUI's local `FolderPickerState`/
    /// native `gtk4::FileDialog` split). `render_content` (`&self`) reads it while
    /// `handle_key_press` (`&mut self`) mutates it, hence the `RefCell`.
    pub(crate) folder_picker: RefCell<Option<quadraui::FolderPickerController>>,
    /// Edge-trigger flag for native message dialogs: `true` once a native present
    /// has been queued or shown for the `engine.dialog` currently open. A native
    /// `AlertDialog` cannot be re-presented every frame like the in-canvas `Dialog`,
    /// so `render_content` only queues one when this is `false`, then sets it `true`.
    /// Reset to `false` when `engine.dialog` goes back to `None`.
    ///
    /// `Rc`-wrapped so `testing::harness` can keep a handle after `App` is moved —
    /// the test asserts the present-exactly-once behaviour by repainting several
    /// frames and checking this never re-queues.
    pub(crate) native_dialog_shown: Rc<Cell<bool>>,
    /// A native message dialog queued by `render_content`'s edge-trigger
    /// check, drained by `tick()`. Mirrors `PendingFileDialog`:
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
    /// Explorer drag-and-drop source row, armed on press and disarmed either into
    /// [`Self::explorer_drag_active`] (once the pointer moves to a different row)
    /// or back to `None` on release. Plain row indices, not platform-specific state.
    pub(crate) explorer_drag_src: Option<usize>,
    /// `(src_row, target_row)` once an explorer drag-and-drop gesture has actually
    /// started (moved off the source row) — `target_row` is `None` while the pointer
    /// is outside the tree, keeping the gesture armed without a drop target.
    pub(crate) explorer_drag_active: Option<(usize, Option<usize>)>,
    pub(crate) deferred: DeferredQueue,
    /// Last content written to system clipboard. Used to avoid redundant writes.
    pub(crate) last_clipboard_content: Option<String>,
    /// Which tab close button (×) the mouse is over: (group_id.0, tab_idx).
    pub(crate) tab_close_hover: Option<(usize, usize)>,
    /// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`),
    /// captured in `render_content`. Feeds the tab drop-zone computation so a
    /// short drag inside a group's own tab bar resolves to a `TabReorder` (with
    /// an insertion bar) rather than a new-split overlay.
    pub(crate) cached_tab_slots_abs: Rc<RefCell<TabSlotsAbsMap>>,
    /// Per-group `(Rect, TabBarLayout)` — the exact pixel-accurate geometry
    /// `render_content` just painted (via `Backend::draw_tab_bar_icons_layout`).
    /// Consumed by tab-bar click hit-testing instead of char-cell `hit_regions`,
    /// which don't match proportional-font tab layout.
    pub(crate) cached_group_tab_bar_layouts: Rc<RefCell<GroupTabBarLayoutMap>>,
    /// Cached per-window status bar segment hit zones from draw_window_status_bar.
    pub(crate) status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Painted rect of the separated status line's status bar,
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
    /// local to `Engine::global_status_rect`'s own origin.
    ///
    /// Kept in its own field rather than in `status_segment_map` because that
    /// map is keyed by `WindowId` and the global bar belongs to no window — it
    /// shows the active buffer's summary in the shell's bottom band. Populated
    /// by `render_content` from `Backend::status_bar_layout`, the same
    /// measurement pass that positions the bar's glyphs, so the git-branch
    /// segment's clickable span is by construction the span it painted at.
    pub(crate) global_status_zones: Rc<RefCell<render::StatusZones>>,
    /// Cached ScreenLayout from the last draw_editor paint pass. Click handlers
    /// read this instead of recomputing geometry from engine state.
    pub(crate) cached_screen_layout: Rc<RefCell<Option<render::ScreenLayout>>>,
    /// Accumulated `quadraui::FrameHitMap` covering the `Editor`/`TabBar`
    /// zones painted in `render_content`. Built via
    /// `quadraui::ScreenLayout::hit_map()`: pushes the SAME
    /// `Editor`/`TabBar` objects and rects already painted at their existing
    /// call sites, purely for hit-testing, so it can never reorder or
    /// duplicate real painting. `click::pixel_to_click_target` consults this
    /// first to resolve the top-level Editor/TabBar zone, falling back to
    /// `render::screen_zone_hit_test`'s manual rect-walk for
    /// breadcrumb/divider zones (which have no `FrameZone` equivalent) and
    /// for the brief window before the first paint populates this cache.
    pub(crate) cached_frame_hit_map: Rc<RefCell<Option<quadraui::FrameHitMap>>>,
    /// Parallel table for resolving `FrameZone::TabBar { idx }`, keyed by the
    /// global surface index. `ScreenLayout::zone_for`'s `idx` enumerates ALL
    /// surfaces in `cached_frame_hit_map` (editors THEN tab bars), so a tab bar's
    /// global index is offset by however many editors were pushed before it.
    /// Keying by the global `idx` instead of position fixes lookup.
    pub(crate) cached_tab_bar_zones:
        Rc<RefCell<HashMap<usize, (core::window::GroupId, quadraui::Rect)>>>,
    /// A sidebar panel claimed the current press, so the rest of the gesture
    /// (`MouseMoved` with the left button held, then `MouseUp`) belongs to it —
    /// that is what lets a panel scrollbar thumb or a tree drag keep tracking
    /// once the pointer strays outside the sidebar. Cleared on release. An
    /// *unclaimed* drag is never intercepted, so an editor text-selection drag
    /// that wanders over the sidebar still finalises in the editor.
    pub(crate) sidebar_pointer_captured: Cell<bool>,
    /// Git-sidebar band geometry (header / commit input / toolbar slab) as the
    /// last `render_content` pass painted it, from `render::sc_sidebar_bands`.
    /// `route_sc_sidebar_event` resolves presses against this so the click and
    /// paint derivations cannot drift.
    pub(crate) cached_sc_bands: Cell<Option<render::ScSidebarBands>>,
    /// Per-group tab-drop geometry (absolute pixel bounds) computed each frame in
    /// `render_content`. Both the drag overlay (same frame) and the drag hit-test
    /// in `handle_mouse_drag_msg` (next mouse-move) read this, so the drop-zone
    /// detection and the highlight always use one identical bounds source.
    pub(crate) cached_drop_ctx: Rc<RefCell<render::TabDropCtx>>,
    /// Backend (line_height, char_width) captured when the file explorer tree
    /// was rendered. The backend's `current_line_height` is mutable per-frame state
    /// and may differ by click time, making the explorer hit-test resolve the wrong
    /// row. Re-applied before hit-testing so draw and hit agree.
    pub(crate) cached_explorer_metrics: Rc<Cell<(f64, f64)>>,
    /// `(line_height, char_width)` the AI panel's `ChatController` was last
    /// painted with — the same drift `cached_explorer_metrics` guards
    /// against. `Backend::line_height`/`char_width` on GTK
    /// read a mutable "current" field any widget's render pass can
    /// overwrite for its own metrics; by the time a later mouse/keyboard
    /// event reaches `route_ai_chat_event`, whatever painted *last* this
    /// frame or the previous one may have left a different value there.
    /// Re-applied via `quadraui::Backend::set_current_line_height`/
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
    /// The divider currently grabbed — an editor-group boundary or a `:split`/`:vsplit`
    /// window boundary (each group's `WindowLayout` numbers its own splits independently).
    pub(crate) divider_grab: Option<render::DividerGrab>,
    /// Tab drag-and-drop state machine: arm → threshold → track → commit.
    pub(crate) tab_drag: render::TabDragState,
    /// Whether `capture_window_and_apply_csd` has already dropped the server-side
    /// WM titlebar via `WindowControl::set_decorated(false)`. `set_decorated` is
    /// idempotent; this flag skips the repeat `Backend::window()` call on every
    /// subsequent tick.
    pub(crate) csd_applied: Cell<bool>,
    /// Cached window width/height, refreshed every tick
    /// (`handle_poll_tick`) from `WindowControl::bounds()` rather than read
    /// live at quit time: quit runs through `render::apply_engine_action`/
    /// `handle_menu_action`/dialog-button call chains with no live
    /// `backend: &mut dyn quadraui::Backend` in scope (only `tick`/`setup`/
    /// paint entry points have one) — the same "no backend in scope"
    /// problem `cached_line_height`/`cached_char_width` solve for text
    /// metrics, solved the same way here.
    ///
    /// Only refreshed while the window is *not* maximized. `bounds()`
    /// reports the window's live allocated size, which under GTK is the
    /// full-screen extent while maximized; the deleted `PlatformWindowHandle`
    /// seam avoided that by reading `gtk4::Window::default_width`/
    /// `default_height` instead, GTK properties documented (and used by
    /// GNOME's own save-window-state guidance) to freeze at the last
    /// non-maximized size while maximized/fullscreen/tiled — no
    /// `WindowControl` method reports that directly. Skipping the write
    /// while maximized reproduces the same "last known non-maximized size"
    /// behaviour without needing one: the fields simply keep whatever they
    /// were last set to (or the `800`×`600` default below) until the window
    /// un-maximizes again.
    pub(crate) cached_window_width: Cell<i32>,
    pub(crate) cached_window_height: Cell<i32>,
    /// Cached window position, refreshed alongside `cached_window_width`/`cached_window_height`.
    /// `None` before the first successful non-sentinel `bounds()` read, while maximized,
    /// and permanently on GTK/Wayland where `WindowControl::bounds` always answers `(0.0, 0.0)`.
    /// On macOS/Win-GUI, this caches the real position and is used for session restore.
    pub(crate) cached_window_x: Cell<Option<i32>>,
    pub(crate) cached_window_y: Cell<Option<i32>>,
    /// Cached maximized state, refreshed every tick from
    /// `WindowControl::is_maximized()` — unlike
    /// `cached_window_width`/`height`/`x`/`y`, this one is read
    /// *unconditionally*, specifically so it can flip to `true` at the one
    /// moment those size/position caches freeze (see their own docs).
    pub(crate) cached_window_maximized: Cell<bool>,
    /// Whether `App::restore_window_geometry` has already applied
    /// `session.window` to the runner's window this run. Mirrors
    /// `csd_applied`'s "retry every tick until the window is mapped" gate —
    /// `Backend::window()` is `None` on the `setup()` fast path (the
    /// runner hasn't called `window.present()` yet) and reliably `Some` by
    /// the first `tick()`, same as `capture_window_and_apply_csd`'s own doc
    /// explains. Idempotent either way (re-applying the same geometry is
    /// harmless), but this avoids fighting a user resize/move that happens
    /// to land before the window is confirmed mapped.
    pub(crate) window_geometry_restored: Cell<bool>,
    /// Editor content bounds + tab-bar height as used by the LAST `render_content` pass,
    /// in the same **absolute** DA coordinate frame that mouse events arrive in.
    ///
    /// `render_content` derives `editor_bounds` from `AppShellLayout::main_content_bounds`,
    /// which is offset by the activity bar / sidebar (x) and the title-bar band (y).
    /// Caching what the renderer used makes hit-test-agrees-with-paint true *by construction*.
    pub(crate) cached_editor_bounds: Cell<Option<(core::WindowRect, f64)>>,
    /// Main-content pixel height last painted by `render_content` — the `h`
    /// argument it hands `render::compute_editor_layout` (before that call
    /// subtracts the status bar / debug toolbar / quickfix / terminal bands
    /// out of it). Distinct from `cached_editor_bounds`'s rect height, which
    /// is `compute_editor_layout`'s *output* (`editor_bottom`, i.e. already
    /// net of those bands) — reusing that would feed the already-reduced
    /// figure back in as the total and double-subtract.
    ///
    /// [`App::terminal_maximize_target_rows`] replays the same
    /// `compute_editor_layout` call against this cached value for callers
    /// (accelerators, menu/tick paths) with no live viewport height of their
    /// own in scope. Defaults to `600.0`, matching `cached_window_height`'s
    /// own pre-first-paint default.
    pub(crate) cached_main_content_height: Cell<f64>,
    /// Menu bar row rect (full content width, `lh` tall) computed in
    /// `render_content` each frame. Reused by `handle()` so `MenuSystem`'s
    /// click/key routing tests against the exact rect the bar was drawn
    /// into.
    ///
    /// `Rc`-wrapped (like [`Self::title_bar_rect`]) so the headless test
    /// harness can clone a handle and *aim* pixel probes at the row the
    /// renderer actually used, instead of hardcoding chrome coordinates.
    pub(crate) menu_row_rect: Rc<Cell<quadraui::Rect>>,
    /// The sub-rect of [`Self::menu_row_rect`] the menu *items* were actually drawn into —
    /// `menu_row_rect` minus the app-icon slot at its leading edge.
    ///
    /// `handle()` routes `MenuSystem` clicks against **this**: the icon shifts every
    /// item's x-origin right, so a hit-test run against the unshifted band would
    /// resolve a click on `File` to whatever item sits one slot to its left. Written
    /// by `render_content` from the same call that positions the paint, so they cannot disagree.
    pub(crate) menu_items_rect: Cell<quadraui::Rect>,
    /// Rect of the drawn inline window-control buttons (minimize/maximize/
    /// close), to the right of the menu items within `menu_row_rect`.
    /// `Rc`-wrapped (like `picker_popup_rect` etc.) so the headless test
    /// harness can clone a handle and assert the Command Center
    /// never overlaps it.
    pub(crate) title_bar_rect: Rc<Cell<quadraui::Rect>>,
    /// Hover/press/click tracker for the window-control buttons, shared with
    /// quadraui's own `full_chrome_demo` reference title bar
    /// — replaces a hand-rolled `StatusBarLayout::hit_test` call with the
    /// same primitive so the buttons get real hover/press highlighting and
    /// click-on-release semantics instead of firing on press.
    pub(crate) title_bar_interaction: RefCell<quadraui::StatusBarInteraction>,
    /// The last inline window-control action dispatched (`render::
    /// WINDOW_MINIMIZE_ACTION` / `_MAXIMIZE_ACTION` / `_CLOSE_ACTION`), or
    /// `None` before any click.
    ///
    /// A pure test-observability seam, `Rc`-wrapped like [`Self::
    /// title_bar_rect`] so the headless test harness can clone a handle —
    /// minimize/maximize genuinely have no other headlessly-observable
    /// effect. `WindowControl::minimize`/`Backend::toggle_window_maximize`
    /// both route through `Backend::window()`, which this crate's own GTK
    /// test harness deliberately reports `None` from (see `src/gtk/
    /// testing.rs`'s module doc, "No window" — there is no live
    /// `gtk4::Window` for a click to actually iconify/zoom), so a black-box
    /// test asserting "the click dispatched `minimize`" has nothing else to
    /// read. `window_close`'s equivalent bypass is `show_quit_confirm`'s own
    /// engine-visible state (`native_dialog_shown`/`exit_requested`); this
    /// field is the same idea for the two buttons with no engine-visible
    /// state of their own to piggyback on. Written unconditionally
    /// (production code, not `cfg(test)`) exactly like every other `Rc<Cell<
    /// _>>` seam in this struct — cheap enough that it costs nothing outside
    /// a test.
    pub(crate) last_window_control_action: Rc<Cell<Option<&'static str>>>,
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
    /// The frame rungs this frame actually composed, in composition order.
    ///
    /// Written by the single [`render::compose_frame`] walk in `render_content`.
    /// Every arm that draws pushes its own [`render::FrameOp`]; arms whose surface
    /// turned out to be absent do not. This observable makes "both backends compose
    /// the frame in the same order" testable: the pre-#1434 TUI shell keeps the
    /// identical field, and the backends' recorded sequences are asserted equal.
    pub(crate) composed_frame: Rc<RefCell<Vec<render::FrameOp>>>,
    /// The editor-band twin of [`Self::composed_frame`]: written
    /// by the [`render::compose_editor_band`] walk, one [`render::EditorOp`]
    /// pushed by every arm that actually composed its rung. Same `Rc`
    /// rationale, same role — it is what makes "both backends compose the
    /// editor column in the same order" assertable rather than promised in
    /// comments, which is how this backend came to omit the group dividers
    /// entirely while still hit-testing drags against them.
    pub(crate) composed_editor_band: Rc<RefCell<Vec<render::EditorOp>>>,
    /// The bottom-band twin of [`Self::composed_editor_band`]: written by the
    /// [`render::compose_bottom_band`] walk, one [`render::BottomOp`] pushed by
    /// every arm that actually composed its rung.
    pub(crate) composed_bottom_band: Rc<RefCell<Vec<render::BottomOp>>>,
    /// Per-group tab-bar visible-column budget as this frame's `TabBars` rung
    /// actually painted them. Populated by `paint_tab_bars_rung`,
    /// drained by `handle_poll_tick`.
    pub(crate) tab_visible_counts: Rc<RefCell<Vec<(core::window::GroupId, usize)>>>,
    /// Picker/command-palette popup rect **as the last frame
    /// actually painted it** — see [`App::compute_picker_popup_bounds`] for
    /// why the click path must not re-derive it.
    pub(crate) picker_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Folder-picker popup rect **as the last frame actually
    /// painted it** — same shape and the same "click
    /// path reads the painted rect, never re-derives it" rationale as
    /// [`Self::picker_popup_rect`] just above. Cleared whenever the picker
    /// is closed, alongside the other overlay-tail caches.
    pub(crate) folder_picker_popup_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Line height the last frame actually painted with, published by
    /// `render_content` — see [`App::painted_line_height`].
    pub(crate) painted_line_height: Rc<Cell<Option<f64>>>,
    /// Character-cell advance the last frame actually painted with — the
    /// horizontal twin of [`Self::painted_line_height`], and published for
    /// exactly the same reason.
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
    /// `screen_layout` / `tab_slots_abs` exist for.
    pub(crate) painted_sidebar_bounds: Rc<Cell<Option<quadraui::Rect>>>,
    /// Link hit rects populated during editor hover popup draw: (rect, url).
    pub(crate) editor_hover_link_rects: Rc<RefCell<Vec<(quadraui::Rect, String)>>>,
    /// Editor hover popup scrollbar geometry. Populated by
    /// `draw_editor_hover_popup`; consumed by click + drag handlers
    /// in this file.
    pub(crate) editor_hover_scrollbar: Rc<Cell<Option<render::PopupScrollbarHit>>>,
    /// Colorscheme name as of the last `handle_poll_tick` check — lets that method
    /// detect a runtime `:colorscheme` change and schedule a redraw.
    pub(crate) last_colorscheme: String,
    /// The OS/terminal window title as of the last tick that actually wrote it
    /// via `WindowControl::set_title`. `None` until the first write. Cached to avoid
    /// redundant writes on every tick — especially important on TUI where
    /// `TuiBackend::set_title` writes an OSC 0/2 escape sequence to `std::io::stdout()`.
    /// An idle session would write a fresh escape every poll cycle even if the
    /// title text never changed.
    pub(crate) last_window_title: Option<String>,
    /// Same guard, for `Backend::set_caret_shape` — see `last_window_
    /// title`'s doc just above for the shared reasoning (including the "not
    /// confirmed as the flicker cause" caveat). `tick_dispatch`'s `self.live`
    /// gate (real terminal only, never a test driver) already keeps this one
    /// out of `cargo test`'s stdout, but it still repainted the identical
    /// DECSCUSR cursor-style escape (`ratatui::crossterm::cursor::
    /// SetCursorStyle`, via `TuiBackend::set_caret_shape`) every idle tick in
    /// a real session, with no change-detection of its own.
    pub(crate) last_caret_shape: Option<quadraui::EditorCursorShape>,
    /// A backend instance owned by `App`, distinct from the runner's live
    /// `&mut dyn quadraui::Backend` — used by callers that have no runner
    /// backend reference of their own:
    ///
    /// - **Clipboard/dialog services** (`setup_gtk_clipboard`,
    ///   `PendingFileDialog` handling): `engine.clipboard_read`/`clipboard_write`
    ///   are callbacks invoked from deep inside `core::engine` code with no
    ///   `Backend` parameter of their own, long after any `handle`/`render` call
    ///   that held the runner's live backend has returned.
    /// - **The click-drift guard's metric setters** (`explorer_ui_event`,
    ///   `route_ai_sidebar_event`, DAP-sidebar key route): `set_current_line_height`/
    ///   `set_current_char_width` calls from dispatch call sites that cannot
    ///   thread a live `backend: &dyn quadraui::Backend` reference.
    ///
    /// Typed `Box<dyn quadraui::Backend>` directly.
    pub(crate) backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
    /// The geometry unit this `App` instance paints in, chosen once at construction
    /// by the caller that already knows its own backend — never inferred from a
    /// runtime "am I GTK?" check. Every hardcoded `render::gtk_*` value (tab-row
    /// height, minimap sizing, picker/tab-switcher/find-replace geometry, divider
    /// hit tolerances, sidebar metrics, activity-bar width, title-bar height) now
    /// reads from this field. `UnitProfile::px()` reproduces every value hardcoded
    /// before #1426 exactly; `UnitProfile::cell()` makes the same `App` paint
    /// correctly on a cell grid.
    pub(crate) units: render::UnitProfile,
    /// What this app believes the **runner's** `AppShell` currently has as its
    /// active panel. Updated only from [`Self::on_shell_event`]'s `PanelChanged`
    /// notifications — the single channel through which the runner reports its own
    /// state — and compared against the shadow's `active_panel_id()`/`ext_panel_active`
    /// in [`quadraui::ShellApp::take_requested_panel`] to detect an **app-initiated**
    /// switch (e.g. `Engine::process_pending_sidebar`'s DAP reveal, or toggle-focus
    /// keyboard accelerators).
    pub(crate) last_shell_panel: Option<quadraui::WidgetId>,
    /// Set by `take_requested_panel` just before it returns `Some`, consumed by
    /// the `PanelChanged` arm of [`Self::on_shell_event`]. `ShellAdapter::apply_requested_panel`
    /// re-notifies the app with the same `PanelChanged` a mouse click produces —
    /// but for this reconciliation echo, the engine already holds that state, so
    /// the echo must only update [`Self::last_shell_panel`] and must NOT re-run
    /// the click path, which for an already-active `ext:` panel would toggle the
    /// sidebar back off.
    pub(crate) suppress_shell_panel_echo: bool,
    /// Cached copy of `quadraui::BackendCaps::kitty_keyboard`, read once in
    /// `ShellApp::setup` (a live capability probe, not a construction-time decision)
    /// and threaded into every `render::engine_key_from_ui` call. Defaults `false`,
    /// which resolves disambiguation the same conservative way a terminal without
    /// the kitty protocol needs.
    pub(crate) keyboard_enhanced: bool,
    /// A one-shot startup notice queued in [`App::assemble`] and drained by the
    /// first `tick()`/`handle_poll_tick`. `None` on every GUI-backend `App` (GTK/macOS/Win-GUI
    /// bundle the icon font); populated only for a cell-profile (TUI-via-`App`)
    /// construction where `settings.use_nerd_fonts` was never explicitly set.
    pub(crate) pending_startup_msg: Option<String>,
    /// `true` for a real, running application (`App::new`/`App::new_portable`),
    /// `false` for test/headless construction (`App::new_headless_with_backend`).
    /// Gates one call: `tick_dispatch`'s `backend.set_caret_shape` write, which
    /// on TUI writes straight to `std::io::stdout()` unconditionally, emitting a
    /// raw DECSCUSR escape into the test process's stdout on every tick without
    /// this gate. GTK/macOS/Win-GUI never override the hook (no-op there).
    pub(crate) live: bool,
}

mod construction;
mod dialogs;
mod dispatch;
mod input;
mod mouse_tabs;
mod paint;
mod shell;
#[cfg(test)]
mod shell_tests;
mod sidebar;

use construction::PendingFileDialog;
#[allow(unused_imports)]
pub(crate) use construction::{dedup_window_title, setup_gtk_clipboard};
