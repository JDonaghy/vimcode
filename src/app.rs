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
//! extension this whole file — to depend on it. It was then typed against
//! `TextMetricsBackend`, a narrow local trait for the two text-measurement
//! setters (`set_current_line_height`/`set_current_char_width`) that had no
//! portable `quadraui::Backend` equivalent yet; #1497 deleted that trait
//! once JDonaghy/quadraui#1086 put both methods directly on `Backend`, so
//! `backend` is typed `Box<dyn quadraui::Backend>` now with no local
//! supertrait at all.
//!
//! #862 closed the three items the previous revision of this doc comment
//! listed as the remaining blockers to dropping the `gui` gate:
//!
//! 1. **The platform-typed fields.** `window` was type-erased
//!    (`PlatformWindowHandle`) until #1234 deleted it outright once
//!    `quadraui::Backend::window()` (`WindowControl`, quadraui#950) became a
//!    full replacement — see that issue's note further down for why the
//!    field itself, not just its type erasure, is gone. A second field used
//!    to live in this list, `settings_monitor`, holding a GTK-only
//!    `gio::FileMonitor` behind a `Box<dyn Any>` drop-guard; #949 deleted it
//!    outright rather than type-erasing it — `Engine::check_settings_reload`'s
//!    portable mtime poll (already the sole reload mechanism on TUI, and
//!    already called from GTK's own `handle_poll_tick` every tick) made it
//!    redundant, and deleting it closed the settings-hot-reload gap on
//!    macOS/Win-GUI that this file's `new_portable` doc table used to list
//!    as deliberately skipped. A third field, `css_provider` (a
//!    `PlatformCssProvider`-erased `gtk4::CssProvider` theming the native
//!    file dialog's fallback widgets, the same shape the now-deleted
//!    `TextMetricsBackend` used to have, #1497), was deleted outright by
//!    #1498 once JDonaghy/quadraui#1091 gave `GtkPlatformServices` its own
//!    equivalent stylesheet, reloaded every frame by
//!    `sync_per_frame_backend_state`'s `Backend::set_theme` call.
//! 2. **The platform hook call sites** (colorscheme reload, OS window title /
//!    size / maximized-check / decoration / minimize) now go through
//!    `quadraui::Backend::window()` (`WindowControl`, quadraui#950) and
//!    compile for every feature set. #1234 deleted the last of this file's
//!    own window-handle plumbing — the local `PlatformWindowHandle` trait,
//!    its `gtk4::Window` impl, and the `find_visible_window`
//!    `gtk4::Window::list_toplevels()` discovery scan that fed it — once
//!    `WindowControl::is_maximized`/`set_decorated` shipped upstream:
//!    `GtkBackend` already tracks its own top-level window handle
//!    internally, so `app.rs` never had a genuine discovery gap, only a
//!    missing *portable accessor* to reach a window quadraui's own backend
//!    already had a handle to. Only the `gdk::Display`/`gtk4::IconTheme`
//!    icon-search-path setup — [`crate::gtk::util::add_icon_theme_search_path`]
//!    since #1498 folded `App::new` into [`App::new_portable`] — stays
//!    genuinely GTK-only, called directly by `crate::gtk::run` instead of
//!    from inside this file; quadraui has no portable icon-theme
//!    search-path surface. A `gtk4::Settings` dark/light-variant push used
//!    to live here too (the old `App::new` and `handle_poll_tick` both);
//!    quadraui#1016 moved it into `Backend::set_theme`, so both call sites
//!    were deleted rather than kept behind the gate.
//! 3. **`crate::gtk::{click, css, util}`.** The portable majority of these —
//!    `pixel_to_click_target` and the rest of the click-resolution/tab-bar
//!    pixel-geometry functions, `open_url` — moved to the backend-neutral
//!    `crate::click`/`crate::app_support`, which `src/gtk/{click,mod,util}.rs`
//!    now re-export so nothing else in `crate::gtk` had to change. `css.rs`
//!    (`make_theme_css`/`STATIC_CSS`/`load_css`) is gone entirely as of
//!    #1498 — see item 1 above. The genuinely GTK-only remainder —
//!    `util`'s icon-install/log helpers plus the icon-theme search path —
//!    stayed in `crate::gtk` and is reached from here (or, for the
//!    icon-theme path, from `crate::gtk::run` directly) through explicit
//!    `#[cfg(feature = "gui")]` call sites. (`click::build_editor_click_context`,
//!    the Pango/Cairo text-measurement context builder this list used to
//!    name here too, is `#[cfg(test)]`-only since #1104 — see its doc
//!    comment.) `app_icon_image_for_paint` used to be a third such site —
//!    GTK got a pre-rasterised PNG, every other backend the raw SVG — until
//!    quadraui#1014 added a decode cache to `Backend::draw_image` itself
//!    (#1102), so it now hands every backend the same
//!    [`crate::render::app_icon_image`] with no fork at all.
//!
//! None of this was a "route around it" job: per `CLAUDE.md`'s
//! Platform-Neutrality Rule, the parts that stayed behind the `gui` feature
//! are exactly the parts that still need quadraui-side infrastructure (a
//! backend-neutral window-chrome/file-watcher/file-picker surface) rather
//! than new per-backend code — see `docs/IRREDUCIBLE_SURFACE.md`.
//!
//! #1490 migrated this file's `Backend::draw_status_bar` (quadraui#819)
//! calls to `draw_status_bar_interactive`, so the file-level deprecation
//! suppression that used to sit here no longer covers that. #1491 removed
//! the other tenant too — the deprecated `ShellApp::on_shell_event` shim
//! `on_shell_event_ctx` used to call directly — by pulling the shared body
//! into a plain [`App::dispatch_shell_event`] both the (now default, no-op)
//! trait override and `on_shell_event_ctx` can call without dispatching
//! through the deprecated trait method. This file needs no deprecation
//! suppression of any kind anymore.

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
// directly (#1499) for the five actions that need GTK's `DeferredQueue` seam.

// `register_panel_accelerators` (the 15-entry id table + registration loop)
// moved to `render::register_panel_accelerators` in #823 item 1 — it was
// byte-identical to `tui_main`'s copy and had no backend-specific step.
// Called from `ShellApp::setup` (#587) — mirrors `tui_main`'s call at
// startup.

/// Work that a GTK callback with no `&mut App` in hand must hand back to the
/// next frame.
///
/// #732 tranche 3: the six deferrals below are all that is left of the
/// Relm4-era `Msg` bus. They are genuine deferrals, not translations — each
/// originates somewhere that cannot call an `&mut self` method at all:
/// `render::dispatch_panel_accelerator`'s `UiEvent::Accelerator` arm, which
/// only ever holds a clone of the queue. (The 200 ms
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
    fn new() -> Self {
        DeferredQueue(Rc::new(RefCell::new(VecDeque::new())))
    }

    /// Enqueue an action for processing in the next `tick()` call.
    pub(crate) fn send(&self, action: DeferredAction) {
        self.0.borrow_mut().push_back(action);
    }

    /// Take all pending actions, leaving the queue empty.
    fn drain(&self) -> Vec<DeferredAction> {
        let mut q = self.0.borrow_mut();
        q.drain(..).collect()
    }
}

// #1497: `TextMetricsBackend`, the narrow local supertrait that used to
// live here, is gone. It existed only because `set_current_line_height`/
// `set_current_char_width` were inherent methods on each pixel backend
// (`GtkBackend`/`MacBackend`/`WinBackend`) with no portable
// `quadraui::Backend` equivalent to reach them through `&mut dyn Backend`
// — closing that gap was JDonaghy/quadraui#1086, filed from this trait's
// own doc comment. quadraui#1086 landed `Backend::set_current_line_height`/
// `set_current_char_width` (`f32`, default no-op; GTK/macOS/Win-GUI
// override to forward onto their existing inherent setters) at the pinned
// rev, so `App::backend` is typed `Box<dyn quadraui::Backend>` directly now
// and every former `TextMetricsBackend::set_current_*` call site below
// calls the `quadraui::Backend` method instead.

// #1234: `PlatformWindowHandle`, the local seam that used to live here, is
// gone. It existed because quadraui's `WindowControl` had no portable
// `is_maximized()`/`set_decorated()` at the time; those shipped upstream
// (`f352462`, #862) and are present at this crate's pinned rev, so every
// caller now reaches `quadraui::Backend::window()` (`WindowControl`,
// quadraui#950) directly instead — see `capture_window_and_apply_csd`
// (CSD/decoration), `paint_title_bar_band`/`render_content`
// (`is_maximized`), and `App::cached_window_width`/`cached_window_height`
// (session-restore size, cached rather than read live — see that field's own
// doc for why). `WindowControl` is backed on every windowed backend (GTK,
// macOS, Win-GUI all `impl WindowControl` at the pinned rev) plus TUI
// (title-only, via the OSC 0/2 escape — see `src/tui_main/shell_app.rs`), so
// none of this needs a `#[cfg(feature = "gui")]` gate or a per-backend impl
// the way the deleted trait's sole `gtk4::Window` impl did.

// #1498: `PlatformCssProvider`, the local seam that used to live here, is
// gone. It existed to reload a `gtk4::CssProvider` theming the native file
// dialog's fallback widgets (sidebar/scrollbar/popover) on every colorscheme
// change; JDonaghy/quadraui#1091 gave `GtkPlatformServices` its own
// equivalent stylesheet, rebuilt from `Theme` on every `Backend::set_theme`
// call (which `sync_per_frame_backend_state` already makes every frame), so
// there is nothing left for `App` itself to own or reload.

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
    /// `dialog_layout` above. the pre-#1434 TUI shell carries the identical field type.
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
    /// Explorer drag-and-drop source row, armed on press
    /// (`TreeControllerEvent::RowSelected`) and disarmed either into
    /// [`Self::explorer_drag_active`] (once the pointer moves to a
    /// different row) or back to `None` on release. Plain row indices, not
    /// GTK-specific state — mirrors the pre-#1434 TUI shell's identically-named field
    /// (#1429; see `render::apply_explorer_drag_move`/`apply_explorer_drop`,
    /// the shared functions both backends apply this through).
    pub(crate) explorer_drag_src: Option<usize>,
    /// `(src_row, target_row)` once an explorer drag-and-drop gesture has
    /// actually started (moved off the source row) — `target_row` is `None`
    /// while the pointer is outside the tree, keeping the gesture armed
    /// without a drop target. Mirrors the pre-#1434 TUI shell's `explorer_drag_active`
    /// (#1429).
    pub(crate) explorer_drag_active: Option<(usize, Option<usize>)>,
    pub(crate) deferred: DeferredQueue,
    /// Last content written to system clipboard.
    /// Used to avoid redundant writes on every keystroke.
    pub(crate) last_clipboard_content: Option<String>,
    /// Which tab close button (×) the mouse is over: (group_id.0, tab_idx).
    pub(crate) tab_close_hover: Option<(usize, usize)>,
    /// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`),
    /// captured in `render_content`. Feeds the tab drop-zone computation so a
    /// short drag inside a group's own tab bar resolves to a `TabReorder` (with
    /// an insertion bar) rather than a new-split overlay. (#515)
    pub(crate) cached_tab_slots_abs: Rc<RefCell<TabSlotsAbsMap>>,
    /// Per-group `(Rect, TabBarLayout)` — the exact pixel-accurate geometry
    /// the ShellApp `render_content` pass just painted (via
    /// `Backend::draw_tab_bar_icons_layout`). Consumed by the GTK tab-bar
    /// click hit-test (`GroupTabBarLayoutMap`, #1491) instead of the
    /// char-cell `hit_regions`, which don't match GTK's proportional-font tab
    /// layout. (#515)
    pub(crate) cached_group_tab_bar_layouts: Rc<RefCell<GroupTabBarLayoutMap>>,
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
    /// Per-group tab-drop geometry (absolute pixel bounds) computed each frame in
    /// `render_content`. Both the drag overlay (same frame) and the drag hit-test
    /// in `handle_mouse_drag_msg` (next mouse-move) read this, so the drop-zone
    /// detection and the highlight always use one identical bounds source. (#515;
    /// #1370 switched the payload from vimcode's own `TabDropGroup` to
    /// `render::TabDropCtx`, the index-aligned shape quadraui's `resolve_tab_drop`
    /// / `drop_zone_hit_test` both key off of.)
    pub(crate) cached_drop_ctx: Rc<RefCell<render::TabDropCtx>>,
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
    /// Whether `capture_window_and_apply_csd` has already dropped the
    /// server-side WM titlebar via `WindowControl::set_decorated(false)`
    /// (#552). `set_decorated` is idempotent, so this only exists to skip
    /// the repeat `Backend::window()` call/dispatch on every subsequent
    /// tick once it has taken effect once — not for correctness. Replaced
    /// the `self.window.is_some()` check #1234 deleted along with the
    /// `gtk4::Window`-typed `window` field it guarded.
    pub(crate) csd_applied: Cell<bool>,
    /// Cached window width/height (#1234), refreshed every tick
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
    /// Cached window position, refreshed alongside `cached_window_width`/
    /// `cached_window_height` in `sync_window_title` (#1529). `None`
    /// before the first successful *non-sentinel* `bounds()` read, while
    /// maximized (same freeze as width/height), and permanently on
    /// GTK/Wayland — `WindowControl::bounds` structurally always answers
    /// `x: 0.0, y: 0.0` there (see that method's own doc), which
    /// `sync_window_title` treats as "no real position" and skips caching
    /// rather than writing `Some(0)` (#1529 review: an earlier version of
    /// this cached the sentinel unconditionally on the theory that
    /// `WindowControl::set_bounds` being unconditionally `Unsupported` on
    /// the same backend made it harmless — true only for a GTK-authored
    /// session file read back by GTK; a `Some(0), Some(0)` value read on a
    /// `set_bounds`-capable backend, e.g. via synced dotfiles, would be
    /// misapplied as a genuine position). macOS/Win-GUI report (and later
    /// restore) a real, non-`(0, 0)` position in the overwhelming common
    /// case, so this only misses the rare case of a window genuinely
    /// parked at the screen origin — falling back to OS default placement
    /// there, not a functional regression.
    pub(crate) cached_window_x: Cell<Option<i32>>,
    pub(crate) cached_window_y: Cell<Option<i32>>,
    /// Cached maximized state, refreshed every tick from
    /// `WindowControl::is_maximized()` (#1529) — unlike
    /// `cached_window_width`/`height`/`x`/`y`, this one is read
    /// *unconditionally*, specifically so it can flip to `true` at the one
    /// moment those size/position caches freeze (see their own docs).
    pub(crate) cached_window_maximized: Cell<bool>,
    /// Whether `App::restore_window_geometry` has already applied
    /// `session.window` to the runner's window this run (#1529). Mirrors
    /// `csd_applied`'s "retry every tick until the window is mapped" gate —
    /// `Backend::window()` is `None` on the `setup()` fast path (the
    /// runner hasn't called `window.present()` yet) and reliably `Some` by
    /// the first `tick()`, same as `capture_window_and_apply_csd`'s own doc
    /// explains. Idempotent either way (re-applying the same geometry is
    /// harmless), but this avoids fighting a user resize/move that happens
    /// to land before the window is confirmed mapped.
    pub(crate) window_geometry_restored: Cell<bool>,
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
    /// own pre-first-paint default. (#1421)
    pub(crate) cached_main_content_height: Cell<f64>,
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
    /// The last inline window-control action dispatched (`render::
    /// WINDOW_MINIMIZE_ACTION` / `_MAXIMIZE_ACTION` / `_CLOSE_ACTION`), or
    /// `None` before any click. (#1530)
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
    /// The frame rungs this frame actually composed, in composition order
    /// (#735, folded into one sequence by #766).
    ///
    /// Written by the single [`render::compose_frame`] walk in
    /// `render_content` — every arm that draws pushes its own
    /// [`render::FrameOp`], and arms whose surface turned out to be absent do
    /// not. It is the *observable* that makes "both backends compose the frame
    /// in the same order" testable: the pre-#1434 TUI shell keeps the identical field,
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
    /// Per-group tab-bar visible-column budget (`click::tab_bar_available_cols`),
    /// as this frame's `TabBars` rung actually painted them — the GTK twin of
    /// the pre-#1434 TUI shell's `tab_visible_counts` (#1165). `paint_tab_bars`'s
    /// doc used to say "TUI reads `hits.available_cols` for
    /// `set_tab_visible_count`; GTK reads the full `hits` for its pixel hit
    /// maps" (before #1491 dropped `TabBarHits` entirely) as if that were a
    /// deliberate backend-specific split — it wasn't: GTK simply never called
    /// `Engine::post_draw_apply_widths` at all, so a tab scrolled out of view
    /// by a resize/sidebar-toggle/new-tab could stay off-screen forever on
    /// this backend, while TUI self-corrected within two frames. Populated by
    /// `paint_tab_bars_rung`, drained by `handle_poll_tick`, mirroring TUI's
    /// clear-at-paint/read-at-tick cadence exactly.
    pub(crate) tab_visible_counts: Rc<RefCell<Vec<(core::window::GroupId, usize)>>>,
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
    /// Colorscheme name as of the last `handle_poll_tick` check — lets that
    /// method detect a runtime `:colorscheme` change and schedule a redraw.
    /// Used to also gate reloading a GTK-only CSS provider theming the
    /// native file dialog; #1498 deleted that provider (JDonaghy/
    /// quadraui#1091 moved the equivalent stylesheet into
    /// `GtkPlatformServices` itself), leaving only the change-detection use
    /// below.
    pub(crate) last_colorscheme: String,
    /// #1634: the OS/terminal window title as of the last tick that actually
    /// wrote it via `WindowControl::set_title` (`render::run_shared_tick_
    /// chores`). `None` until the first write. Every other cached-value
    /// guard `handle_poll_tick`'s chores use (`last_colorscheme` right
    /// above, `last_caret_shape` below) compares before writing; this one
    /// didn't, so `run_shared_tick_chores` called `w.set_title(&win_title)`
    /// unconditionally on *every* tick — on TUI, `TuiBackend::set_title`
    /// writes a real OSC 0/2 escape sequence straight to `std::io::
    /// stdout()` (bypassing the `ratatui::Terminal`'s buffered `Write`
    /// entirely), so an idle session wrote a fresh title-set escape every
    /// poll cycle (`quadraui::runtime::IDLE_POLL_CEILING`, ~250ms) even
    /// though the title text never changed — real waste on every idle tick,
    /// invisible to #1583's in-process idle-stability test since it only
    /// inspects the `TestBackend`/vt100 buffer `Terminal::draw` writes to,
    /// and `set_title`'s direct-to-stdout write never goes through that
    /// sink at all (live or test-driven `TuiBackend` alike).
    ///
    /// **Not confirmed as #1634's flicker cause.** A raw-ConPTY capture
    /// (`tests/conpty_idle_flicker.rs`, on real Windows 11 hardware) of an
    /// idle session showed the byte stream silent at this window's output
    /// *both* with this guard reverted and with it in place — i.e. whatever
    /// downstream consumes the pseudo console (ConPTY itself, and/or a real
    /// terminal emulator) already appears to suppress a redundant OSC 0/2
    /// title write before it reaches a reader on the other end, so this
    /// guard's fix is a real, worthwhile efficiency win (a syscall + a
    /// write vimcode no longer makes 4×/second for nothing) but is not
    /// shown to be what the operator saw flicker. See that test file's
    /// module doc for the full finding and what it rules out.
    pub(crate) last_window_title: Option<String>,
    /// #1634: same guard, for `Backend::set_caret_shape` — see `last_window_
    /// title`'s doc just above for the shared reasoning (including the "not
    /// confirmed as the flicker cause" caveat). `tick_dispatch`'s `self.live`
    /// gate (real terminal only, never a test driver) already keeps this one
    /// out of `cargo test`'s stdout, but it still repainted the identical
    /// DECSCUSR cursor-style escape (`ratatui::crossterm::cursor::
    /// SetCursorStyle`, via `TuiBackend::set_caret_shape`) every idle tick in
    /// a real session, with no change-detection of its own.
    pub(crate) last_caret_shape: Option<quadraui::EditorCursorShape>,
    /// A second, standalone `quadraui::Backend`-impl handle, distinct from
    /// the `&mut dyn quadraui::Backend` the `ShellApp` runner hands
    /// `setup`/`handle`/`tick` — owned outright by `App` for the callers that
    /// have no live runner backend reference of their own to reach instead:
    ///
    /// - **Clipboard/dialog services** (`setup_gtk_clipboard`,
    ///   `PendingFileDialog` handling): `engine.clipboard_read`/
    ///   `clipboard_write` are plain `Fn` callbacks invoked from deep inside
    ///   `core::engine` code with no `Backend` parameter of their own at all,
    ///   long after any `handle`/`render` call that held the runner's live
    ///   backend has returned — this handle is what they close over.
    /// - **The click-drift guard's two metric setters** (`explorer_ui_event`,
    ///   `route_ai_sidebar_event`, the DAP-sidebar key route):
    ///   `quadraui::Backend::set_current_line_height`/`set_current_char_width`
    ///   (JDonaghy/quadraui#1086) re-apply the metrics the tree/panel was
    ///   painted with, called from dispatch-tree call sites that #1104 could
    ///   not thread a live `backend: &dyn quadraui::Backend` reference into.
    ///
    /// #1104 removed the third historical reason this field existed: click/
    /// drag/modal-stack hit-testing (`pixel_to_click_target` and friends)
    /// used to resolve against `self.backend`'s own, separately-synced
    /// `modal_stack_handle()`/`drag_state_handle()`/Pango state, entirely
    /// distinct from the runner's own — see `render_content`'s doc comment
    /// at the old sync call site. That whole call chain is threaded the
    /// runner's live backend now, so `self.backend`'s modal stack and drag
    /// state are dead weight for it (nothing clicks against them anymore) —
    /// only the two uses above still read this field.
    ///
    /// The `init` drain timer holds a clone and pumps `poll_events()` every
    /// 16 ms.
    ///
    /// Typed against `Box<dyn quadraui::Backend>` directly, not the
    /// concrete `backend::GtkBackend` (#813) — the now-deleted
    /// `TextMetricsBackend` local supertrait used to sit between the two
    /// (#1497; see the module doc's "Why this module no longer needs
    /// `#[cfg(feature = "gui")]`" section).
    pub(crate) backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
    /// #1426: the geometry unit this `App` instance paints in, chosen once at
    /// construction by the caller that already knows its own backend
    /// (GTK/macOS/Win pass [`render::UnitProfile::px`], the `tui` harness arm
    /// passes [`render::UnitProfile::cell`]) — never inferred from a runtime
    /// "am I GTK?" check, per the Platform-Neutrality Rule. Every fork this
    /// file used to hardcode as `render::gtk_*`/`render::GTK_*` (tab-row
    /// height, minimap sizing, picker/tab-switcher/find-replace geometry,
    /// divider hit tolerances, sidebar `MsvLayoutMetrics`, the activity-bar
    /// width and title-bar height fed to [`Self::shell_config`], and
    /// [`crate::icons::set_gui_backend`]'s argument) now reads from this
    /// field instead. `UnitProfile::px()` reproduces every value this file
    /// hardcoded before #1426 exactly, so GTK/macOS/Win behaviour is
    /// unchanged; `UnitProfile::cell()` is what makes the *same* `App` paint
    /// correctly on a cell grid — see `UnitProfile`'s own doc for the two
    /// constructors.
    pub(crate) units: render::UnitProfile,
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
    /// otherwise never learn about. Mirrors the pre-#1434 TUI shell's `last_shell_panel`
    /// verbatim — see that field's own doc for the full rationale. Plain
    /// field, not `Rc`/`RefCell`: `take_requested_panel` and
    /// [`Self::on_shell_event`] both take `&mut self`, so no interior
    /// mutability is needed (matching the pre-#1434 TUI shell's own field).
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
    /// arm). Mirrors the pre-#1434 TUI shell's `suppress_shell_panel_echo`.
    pub(crate) suppress_shell_panel_echo: bool,
    /// #1428: cached copy of `quadraui::BackendCaps::kitty_keyboard`, read
    /// once in `ShellApp::setup` (a live capability probe, not a
    /// construction-time decision — see the pre-#1434 TUI shell's `keyboard_enhanced`'s
    /// own doc, ported here verbatim) and threaded into every
    /// `render::engine_key_from_ui` call this file makes. Defaults `false`
    /// (every constructor's assembled default, same as the pre-#1434 TUI shell's `new`),
    /// which resolves the disambiguation the same conservative way a
    /// terminal without the kitty protocol needs — a hardcoded `true` here
    /// used to feed `engine_key_from_ui` the wrong answer on any such
    /// terminal (#826), the same class of bug TUI's own field exists to
    /// avoid.
    pub(crate) keyboard_enhanced: bool,
    /// #1428: a one-shot startup notice queued in [`App::assemble`] and
    /// drained by the first `tick()`/`handle_poll_tick` — mirrors
    /// the pre-#1434 TUI shell's `pending_startup_msg` verbatim (see that field's own
    /// doc for why the nudge exists and why it can only be computed once,
    /// at construction, rather than every frame). `None` on every
    /// GUI-backend `App` (`units.is_gui_backend` — GTK/macOS/Win-GUI all
    /// bundle the icon font, so `nerd_fonts_undiscovered` is never true
    /// for them); populated only for a `cell`-profile (TUI-via-`App`)
    /// construction where `settings.use_nerd_fonts` was never explicitly
    /// set and the backend-derived default resolved to ASCII fallback
    /// icons.
    pub(crate) pending_startup_msg: Option<String>,
    /// #1428: `true` for a real, running application (`App::new`/
    /// `App::new_portable`), `false` for every test/headless construction
    /// (`App::new_headless_with_backend`, which every test seam —
    /// `crate::gtk::testing`, the macOS driver-tier test, and the `tui`
    /// harness arm — funnels through). Mirrors the pre-#1434 TUI shell's `live`'s own
    /// doc: gates exactly one call, `tick_dispatch`'s
    /// `backend.set_caret_shape` write, from running under a test harness.
    /// `Backend::set_caret_shape`'s only real-writing override
    /// (`TuiBackend`, quadraui#1015) writes straight to the real process
    /// `std::io::stdout()` unconditionally — no test-mode guard of its
    /// own — so calling it during a `conformance_harness`/`app_on_tui_
    /// tests` driver's `tick()` would emit a raw DECSCUSR escape sequence
    /// into the test process's real stdout on every tick, exactly the
    /// corruption the pre-#1434 TUI shell's `live` exists to prevent. GTK/macOS/Win-GUI
    /// never override the hook (a genuine no-op there), so this gate only
    /// ever changes behaviour for a `TuiBackend`-backed `App` — today that
    /// is test-only, since no live TUI-via-`App` entry point exists yet
    /// (`tui_main::run` still runs the pre-#1434 TUI shell, not `App` — see
    /// `GOALS.md`'s milestone #7).
    pub(crate) live: bool,
}

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
fn dialog_btn_index(id: &quadraui::WidgetId) -> Option<usize> {
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
fn app_icon_image_for_paint() -> quadraui::Image {
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
fn dedup_caret_shape(
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
    fn build_portable_engine(
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
    fn assemble(
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
    pub(super) fn new_headless(engine: Rc<RefCell<Engine>>) -> Self {
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
        // Bodies shared with TUI's synchronous call sites — see
        // `render::run_open_file_dialog`/`run_save_workspace_as_dialog`'s
        // rung header comment (#1125) for why this stays one implementation
        // with two thin call sites instead of a per-backend copy.
        match req {
            PendingFileDialog::OpenFile => {
                let opened = {
                    let mut engine = self.engine.borrow_mut();
                    render::run_open_file_dialog(&mut engine, backend)
                };
                if opened.is_some() {
                    self.refresh_file_tree();
                }
            }
            PendingFileDialog::SaveWorkspaceAs => {
                let mut engine = self.engine.borrow_mut();
                render::run_save_workspace_as_dialog(&mut engine, backend);
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
    fn handle_editor_right_click(&mut self, backend: &dyn quadraui::Backend, x: f64, y: f64) {
        // Swallow if the click landed on a focused modal that
        // wants to consume it (#216 — editor hover popup).
        self.reconcile_editor_hover_modal(backend);
        let stack_rc = backend.modal_stack_handle();
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

    /// After `handle_dispatch` may have opened a context menu
    /// (`Engine::open_*_context_menu`, from any surface — editor, tab,
    /// Explorer, Board, ...), show it immediately if the backend resolves
    /// `MenuStyle` to `Native` (#1580, and the root-cause fix for the
    /// macOS right-click bug it also closes). `render::show_context_menu_now`
    /// does this through quadraui's own single-call
    /// `quadraui::ContextMenuController::open` (quadraui#1187) rather than
    /// calling `Backend::show_context_menu` directly.
    ///
    /// Called once from `Self::handle`'s single choke point, gated on the
    /// `context_menu` open *transition* (`None` -> `Some`) rather than
    /// from each individual `open_*_context_menu` call site — every
    /// right-click/keyboard path that opens a menu funnels through
    /// `handle_dispatch` before returning to `handle`, so one check there
    /// covers all of them without per-surface backend threading. See
    /// `render::show_context_menu_now`'s doc for why this must run from
    /// event-handling code and never from `render_content`'s paint rung
    /// (`FrameOp::ContextMenu`): `MacBackend::show_context_menu` blocks on
    /// AppKit's modal popup loop, and running that from inside a paint
    /// closure re-enters painting while the closure still holds the
    /// borrows it needs to finish its own frame.
    fn open_context_menu_now_if_native(&mut self, backend: &mut dyn quadraui::Backend) {
        if backend.effective_menu_style() != quadraui::ResolvedMenuStyle::Native {
            return;
        }
        let panel = self
            .engine
            .borrow()
            .context_menu
            .as_ref()
            .map(render::context_menu_state_to_panel);
        let Some(panel) = panel else {
            return;
        };
        if panel.items.is_empty() {
            return;
        }
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        render::show_context_menu_now(backend, &panel, cw, lh);
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
    fn handle_ctrl_mouse_click(&mut self, backend: &dyn quadraui::Backend, x: f64, y: f64) {
        let layout_ref = self.cached_screen_layout.borrow();
        if let Some(ref layout) = *layout_ref {
            let mut engine = self.engine.borrow_mut();
            if !engine.picker_open {
                let drag_rc = backend.drag_state_handle();
                let mut drag = drag_rc.borrow_mut();
                if let ClickTarget::BufferPos(_, line, col) = pixel_to_click_target(
                    &mut engine,
                    backend,
                    x,
                    y,
                    self.cached_line_height,
                    self.cached_char_width,
                    layout,
                    &self.cached_group_tab_bar_layouts.borrow(),
                    self.cached_frame_hit_map.borrow().as_ref(),
                    &self.cached_tab_bar_zones.borrow(),
                    true, // real click: focus/tab/gutter side effects are intended
                    &mut drag,
                    false, // Ctrl+click has no Alt-fine-seek concept
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
    fn handle_mouse_double_click_msg(&mut self, backend: &dyn quadraui::Backend, x: f64, y: f64) {
        // #490: a double-click landing on the editor hover popup used to fall
        // straight through to the editor's word-select underneath, because
        // this handler never consulted the popup at all. It runs the same
        // shared rung the single-click path does, first.
        if self.route_and_apply_editor_hover_popup(backend, x, y) {
            return;
        }
        // ── Editor-tab-hosted plugin view double-click (#1631) ─────────
        //
        // Mirrors `handle_mouse_click_msg`'s tab-hosted press routing: a
        // body-kind view's `List`/`Table` row activates (`ItemActivated`)
        // rather than just selecting, and a field-stack `Form` falls back
        // to `render::handle_plugin_view_tab_ui_event` exactly as the press
        // handler does (that function's own `DoubleClick` arm probes it as
        // a `MouseDown` for `FormController`).
        if let Some((name, rect)) = self.plugin_view_tab_hit(x, y) {
            let mut engine = self.engine.borrow_mut();
            engine.clear_sidebar_focus();
            let event = quadraui::UiEvent::DoubleClick {
                widget: None,
                position: quadraui::Point::new(x as f32, y as f32),
            };
            let backend_rc = self.backend.clone();
            let mut b = backend_rc.borrow_mut();
            let consumed = render::route_plugin_view_body_event(
                &mut engine,
                &name,
                PluginViewHost::Tab,
                &event,
                rect,
                &mut **b,
            );
            drop(b);
            if consumed.is_none() {
                render::handle_plugin_view_tab_ui_event(&mut engine, &name, &event, rect);
            }
            drop(engine);
            self.draw_needed.set(true);
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
                    let drag_rc = backend.drag_state_handle();
                    handle_mouse_double_click(
                        &mut engine,
                        backend,
                        x,
                        y,
                        self.cached_line_height,
                        self.cached_char_width,
                        layout,
                        &self.cached_group_tab_bar_layouts.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                        &mut drag_rc.borrow_mut(),
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
    fn handle_mouse_scroll_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        delta_x: f64,
        delta_y: f64,
    ) {
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
        // ── Editor-tab-hosted plugin view scroll (#1631) ───────────────
        //
        // Mirrors the press/double-click tab routing in
        // `handle_mouse_click_msg`/`handle_mouse_double_click_msg`: a wheel
        // notch over a body-kind view's window scrolls its own content
        // instead of falling through to the generic per-window viewport
        // scroll below, and a field-stack `Form` falls back to
        // `render::handle_plugin_view_tab_ui_event` the same way those two
        // handlers do. `delta_y` is negated back to quadraui's polarity —
        // see this function's own doc comment — since `route_plugin_view_
        // body_event`'s `Scroll` arm (shared with the sidebar's already-
        // quadraui-polarity `UiEvent`s) expects it.
        if delta_y.abs() > 0.01 {
            if let Some((px, py)) = self.last_editor_pointer.get() {
                if let Some((name, rect)) = self.plugin_view_tab_hit(px, py) {
                    engine.clear_sidebar_focus();
                    let event = quadraui::UiEvent::Scroll {
                        widget: None,
                        position: quadraui::Point::new(px as f32, py as f32),
                        delta: quadraui::ScrollDelta::new(delta_x as f32, -(delta_y as f32)),
                    };
                    let backend_rc = self.backend.clone();
                    let mut b = backend_rc.borrow_mut();
                    let consumed = render::route_plugin_view_body_event(
                        &mut engine,
                        &name,
                        PluginViewHost::Tab,
                        &event,
                        rect,
                        &mut **b,
                    );
                    drop(b);
                    if consumed.is_none() {
                        render::handle_plugin_view_tab_ui_event(&mut engine, &name, &event, rect);
                    }
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
            }
        }
        // Route scroll through dispatch_scroll using cached scroll surfaces.
        if let Some((px, py)) = self.last_editor_pointer.get() {
            let surfaces = engine.scroll_surfaces.borrow();
            let scroll_events = quadraui::dispatch_scroll(
                &backend.modal_stack_handle().borrow(),
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
        //
        // #1433: resolved via `render::find_window_at` against
        // `self.cached_screen_layout` — the same painted-window lookup the
        // double-click branch above already uses — instead of a second,
        // hand-rolled `calculate_group_window_rects` scan. `find_window_at`
        // is the *shared* hit-test both backends' mouse code already routes
        // through everywhere else (`tui_main::mouse`'s `render::
        // find_window_at` call sites), so this was the one remaining
        // GTK-only reimplementation of it — inlined here since #240, before
        // `find_window_at` existed as a shared helper.
        let hovered_window_id = self.last_editor_pointer.get().and_then(|(x, y)| {
            let layout = self.cached_screen_layout.borrow();
            let layout = layout.as_ref()?;
            let idx = render::find_window_at(layout, x, y)?;
            Some(layout.windows[idx].window_id)
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

    /// Snapshot the cached window geometry (#1234, extended #1529 for
    /// position/maximized) into a [`core::session::WindowGeometry`] ready to
    /// write into `engine.session.window` before `Engine::save_session_state`
    /// persists it.
    ///
    /// Reads the `cached_window_*` cells rather than a live `backend`
    /// handle: every call site that needs to save on quit
    /// ([`Self::quit_and_save_session`] and, through it,
    /// [`Self::save_session_and_exit`] below) runs through
    /// `apply_engine_action`/`run_shared_tick_chores`/menu/dialog call chains
    /// with no live `backend: &mut dyn quadraui::Backend` in scope — only
    /// `tick`/`setup`/paint entry points have one — the same "no backend in
    /// scope" problem `cached_line_height`/`cached_char_width` solve for text
    /// metrics, solved the same way here. A free function (shared by both
    /// sites) rather than duplicated field-copies keeps the five-field list
    /// in one place.
    fn cached_window_geometry(&self) -> core::session::WindowGeometry {
        core::session::WindowGeometry {
            width: self.cached_window_width.get(),
            height: self.cached_window_height.get(),
            x: self.cached_window_x.get(),
            y: self.cached_window_y.get(),
            maximized: self.cached_window_maximized.get(),
        }
    }

    /// Save session state and request a clean shutdown, given an already
    /// mutably-borrowed `engine`.
    ///
    /// Sets [`App::exit_requested`] rather than calling `process::exit`
    /// itself (#813) — `ShellApp::handle`/`tick` check the flag once they
    /// return and surface [`quadraui::Reaction::Exit`] to the runner, which
    /// tears the window down via `ReactionSink::request_exit`
    /// (`gtk/run.rs`), the same mechanism every other quadraui backend uses.
    ///
    /// Takes `engine` as a parameter rather than borrowing `self.engine`
    /// itself (unlike [`Self::save_session_and_exit`] below) because its
    /// callers — `render::apply_engine_action`'s `Quit`/`SaveQuit`/
    /// `QuitWithUnsaved` arms and `render::run_shared_tick_chores`'s
    /// format-on-save-then-quit chore — already hold `engine: &mut Engine`
    /// borrowed from this same `Rc<RefCell<Engine>>` for the whole call; a
    /// second, independent `self.engine.borrow_mut()` from in here would
    /// double-borrow and panic at runtime (#1063, #1248, folded here by
    /// #1499).
    pub(crate) fn quit_and_save_session(&self, engine: &mut Engine) {
        // Capture the cached window geometry into session state *before*
        // `save_session_state` persists it — `Engine` has no window handle
        // of its own to read this from (#823 item 5). See
        // `cached_window_geometry`'s own doc for why this reads cached
        // cells rather than a live `backend` handle (#1234, #1529).
        engine.session.window = self.cached_window_geometry();
        engine.save_session_state();
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        self.exit_requested.set(true);
    }

    /// Save the current session state and request a clean shutdown.
    ///
    /// Thin wrapper around [`Self::quit_and_save_session`] for call sites
    /// that don't already hold `engine` borrowed — see that method's own
    /// doc for why the two can't simply be one.
    fn save_session_and_exit(&self) {
        let mut engine = self.engine.borrow_mut();
        self.quit_and_save_session(&mut engine);
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
        render::apply_engine_action(action, &mut engine_rc.borrow_mut(), self);
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
    ///
    /// Thin wrapper — see [`render::sync_register_to_clipboard`] (#1239) for
    /// the shared implementation TUI's `sync_tui_clipboard` also delegated to
    /// before #1434 deleted that wrapper along with the rest of the
    /// pre-#1434 TUI shell.
    fn sync_plus_register_to_clipboard(&mut self) {
        render::sync_register_to_clipboard(
            &mut self.engine.borrow_mut(),
            &mut self.last_clipboard_content,
        );
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::too_many_arguments)]
    /// `ctx` is threaded in solely for the shared Alt rung's
    /// [`render::AltKeyOutcome::ResizeSidebar`] arm: GTK's authoritative
    /// sidebar width *is* the runner's `AppShell` (TUI keeps its own copy and
    /// syncs it out at end of dispatch), and `ShellContext::shell_mut` is the
    /// only handle to it. `ui_event` (#815) is the raw event `key_name`/
    /// `unicode`/... were decoded from, needed by the folder-picker rung —
    /// mirrors TUI's `KeyDispatchState::ui_event`. `backend` (#1428) is
    /// solely for the Ctrl+L rung's `backend.request_full_repaint()` call —
    /// GTK's `DrawingArea` repaints in full every frame regardless (no
    /// incremental diff to desync), so this is a no-op there; see
    /// `render::is_force_redraw_key`'s own doc for why the hook still needs
    /// calling from every backend rather than being TUI-only code.
    fn handle_key_press(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        ui_event: &quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        // ── Shared toast-stack keyboard-focus rung (#1577) ──────────────
        // Non-modal, unlike every rung below: `Engine::handle_toast_focus_key`
        // only ever consumes a key once something has explicitly given the
        // stack focus (`:Notifications` / `panel_keys.focus_notifications`)
        // — every other key, and every key while unfocused, comes back
        // `false` untouched, so checking this first (ahead of even the
        // modal-dialog rung) is safe and replaces the old hardcoded `N`
        // hijack (`keys.rs`, removed) with a real keyboard-focus cursor.
        if self.engine.borrow_mut().handle_toast_focus_key(ui_event) {
            self.draw_needed.set(true);
            return;
        }

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
        // `insert_ctrl_x_pending` carves out `<C-x><C-l>` (whole-line
        // completion, #1160) — see `render::is_force_redraw_key`'s doc.
        let insert_ctrl_x_pending = {
            let engine = self.engine.borrow();
            engine.mode == crate::core::Mode::Insert && engine.insert_ctrl_x_pending
        };
        if render::is_force_redraw_key(&key_name, unicode, ctrl, insert_ctrl_x_pending) {
            // #1428: was an ordinary redraw only — no rung called
            // `Backend::request_full_repaint` on GTK at all. Harmless
            // no-op there today (Cairo's `DrawingArea` repaints in full
            // every frame, no incremental diff to desync — see
            // `render::is_force_redraw_key`'s own doc), but load-bearing
            // the moment `App` backs a real terminal (the `tui` harness
            // arm today, a future live TUI-via-`App` entry point
            // eventually): without this, Ctrl+L would only request an
            // ordinary `Reaction::Redraw`, which a diffing backend's next
            // paint could resolve as "nothing changed" for any cell
            // written outside its own diff tracking.
            backend.request_full_repaint();
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

        // ── Shared Ctrl-W sidebar chord rung (#1419, closes #406) ──────
        // GTK kept no per-keypress chord latch of its own, so `Ctrl-W`
        // followed by `h`/`l` in a sidebar panel silently did nothing here
        // while TUI's TUI-only `TuiSidebar::pending_ctrl_w` handled it.
        // `Engine::sidebar_ctrl_w_pending` is the shared latch both backends
        // now read/write through `route_sidebar_chord_key`; GTK has no
        // "sidebar band holds focus" shadow flag to clear on
        // `SidebarChordAction::FocusOut` (unlike TUI's `sidebar.has_focus`),
        // so there is nothing else to do here besides redraw.
        if focus_route != render::FocusKeyRoute::None
            && render::route_sidebar_chord_key(
                &mut self.engine.borrow_mut(),
                &key_name,
                unicode,
                ctrl,
            )
            .is_some()
        {
            self.draw_needed.set(true);
            return;
        }

        // ── Shared focus-owner *dispatch* rung (#762 / #734 slice 7) ───
        // Slice 2 shared only the *routing*; `render::dispatch_sidebar_panel_key`
        // now states the six pure-`Engine` arms too, and TUI's
        // `handle_focus_owner_key` calls the same function (after its own
        // crossterm-spelling translation) — this is no longer GTK-only. It
        // hands back `None` for the two it cannot own — Debug needs a live
        // `Backend`, Explorer is a backend widget — which the fallback match
        // below still spells out.
        //
        // #1422: `key_name` is already the shared `render::engine_key_from_ui`
        // spelling (`"Page_Up"`/`"Page_Down"`, `"ISO_Left_Tab"`, …) — the same
        // one TUI's `engine_name()` produces — so there is no second,
        // GTK-local mapping to apply here. The old `map_gtk_key_name` /
        // `map_gtk_key_with_unicode` pair round-tripped `key_name` through a
        // GDK-spelled table (`"Page_Up"` -> `"PageUp"`, `"ISO_Left_Tab"` ->
        // `"BackTab"`) whose output every downstream consumer already accepts
        // in its *un*-mapped, `key_name` form too (`panels.rs`/`search.rs`/
        // `ext_panel.rs`/`source_control.rs` all dual-accept `"ISO_Left_Tab"`
        // since #1060; nothing reads plain `"PageUp"`/`"PageDown"` at all —
        // see issue #1422). `sc_unicode` collapses into `unicode` the same
        // way: for a `Key::Char` press `unicode` is already `Some(c)`
        // independent of `ctrl` (decoded once, above, when `key_name`/
        // `unicode` were built from the raw `UiEvent`), matching what
        // TUI's Source-Control arm (`shell_app.rs`) re-derives with `ctrl`
        // forced off; for a `Key::Named` press both were always `None`.
        let panel_key = key_name.as_str();
        let shared = render::dispatch_sidebar_panel_key(
            &mut self.engine.borrow_mut(),
            focus_route,
            panel_key,
            unicode,
            unicode,
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
            ctrl,
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
            render::AltKeyOutcome::Fallthrough => {
                // #1764: an Alt-modified *printable-character* key that no
                // rung above claimed must not reach `Engine::handle_key` as
                // if the modifier never existed — that function takes no
                // `alt` parameter at all, so the fallthrough below would
                // redeliver it as the *bare* key, which in Insert/Replace
                // mode means inserting the character literally. Treating it
                // as an implicit Escape instead (and discarding the letter)
                // is the better of the two unverifiable guesses: Vim mode
                // has no `<M-x>` mappings for a letter to mean anything
                // else, while a real pty collapsing a fast Escape-then-letter
                // into this exact chord is the documented #1763/#1764
                // mechanism — see `render::alt_mnemonic_open_allowed`'s own
                // doc for the matching other half of this fix (keeping the
                // menu bar from stealing the chord first). Escape is a
                // harmless no-op in `Normal` mode (though *not* in the
                // `Visual*` family, which `alt_mnemonic_open_allowed` also
                // admits and where Escape exits Visual mode — still the
                // right substitution there, since the alternative is
                // literal-text corruption, not a no-op either way), so this
                // is safe to apply across every mode this rung can see.
                //
                // Gated on `render::alt_chord_is_printable_char` (review
                // finding, #1764 round 1), not on `alt` alone: a real pty's
                // Escape-then-letter fusion only ever produces a
                // printable-char chord (`alt_chord_is_printable_char`'s own
                // doc), so narrowing to that shape keeps the #1764 fix while
                // restoring every named-key Alt fallthrough that predates it
                // — `Alt+Enter`/`Alt+BackSpace` in Insert mode,
                // `Alt+Up`/`Alt+Down` moving the cursor in Vim mode outside
                // VSCode mode, and `Alt+]`/`Alt+[`/`Home`/`End`/`Delete`/
                // `Page_Up`/`Page_Down` falling through exactly as
                // `route_alt_key`'s own arms document.
                //
                // Not gated on `menu_bar_toggleable` the way
                // `alt_mnemonic_open_allowed` is: on macOS GUI, `Option+
                // <letter>` is the system dead-key/special-character
                // modifier, so a printable-char Alt chord can arrive there
                // too without any pty involved. Pre-#1764 an `Option+e` in
                // Insert mode inserted `e` literally (already the wrong
                // character for that key combo); post-#1764 it exits Insert
                // mode instead, which is a more surprising failure in
                // isolation — but this crate has no `alt`-aware dead-key
                // decoding on any backend today, so "literal wrong
                // character" was never the correct behaviour to preserve
                // either. Left as a known, narrow trade-off (quadraui's
                // macOS backend's dead-key handling, if it grows one, is the
                // real fix) rather than threading a third backend-shaped
                // condition through this already-shared rung.
                if alt && render::alt_chord_is_printable_char(&key_name, unicode) {
                    let action = self.engine.borrow_mut().handle_key("Escape", None, false);
                    self.dispatch_engine_action(action, false);
                    self.draw_needed.set(true);
                    // No `run_post_key_epilogue(ctx)` call, matching every
                    // sibling early-return rung above (`ModalKeyRoute::Engine`
                    // included, which also dispatches an engine action and
                    // returns without it) — review finding confirmed: the
                    // epilogue's only work (sidebar autohide/focus,
                    // explorer-after-move refresh, quickfix scroll clamp) is
                    // driven by state a plain Insert→Normal Escape never
                    // touches, so skipping it here changes nothing it would
                    // have done.
                    return;
                }
            }
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
        match route {
            render::FocusKeyRoute::Debug => {
                // #1422: pass the real `ui_event` straight to `SidebarSystem::
                // handle` instead of reconstructing a `UiEvent` from `key_name`
                // through the old `gtk_key_name_to_quadraui` table (which only
                // covered a dozen named/nav keys and silently skipped `.handle`
                // for everything else). Mirrors TUI's identical, unconditional
                // `.handle(ui_event, backend, rect)` call for this route
                // (`shell_app.rs`'s `handle_focus_owner_key`) — see
                // `dispatch_dap_sidebar_event`'s own doc: it already reports
                // `Ignored` as `false` so an unrecognised key still falls
                // through to `dispatch_dap_sidebar_action_key` below exactly
                // as before.
                let mut engine = self.engine.borrow_mut();
                let rect = engine.dap_sidebar_body_rect.get();
                render::populate_dap_sidebar_system(&engine);
                let backend_rc = self.backend.clone();
                let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                    ui_event,
                    &mut **backend_rc.borrow_mut(),
                    rect,
                );
                let consumed = engine.dispatch_dap_sidebar_event(sidebar_event);
                if !consumed {
                    engine.dispatch_dap_sidebar_action_key(key_name);
                }
                Some(Some(engine.dap_sidebar_has_focus))
            }
            render::FocusKeyRoute::Ai => {
                // Unlike Debug (nav-only), the AI panel's `ChatController`
                // needs the real, un-round-tripped `UiEvent` — `KeyPressed`
                // *or* `CharTyped` — so free-form typed text reaches its
                // input buffer.
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
                    quadraui::Backend::set_current_line_height(&mut **b, metrics.0 as f32);
                    quadraui::Backend::set_current_char_width(&mut **b, metrics.1 as f32);
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
                self.handle_explorer_da_key(key_name.to_string(), unicode, ctrl);
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
        // #1427: moved to `render::sync_runner_sidebar_visibility`, shared
        // with the pre-#1434 TUI shell — see its own doc, including the #1029/#988
        // hamburger guard this method used to lack entirely (harmless on
        // GTK/macOS/Win, which never register the hamburger panel).
        render::sync_runner_sidebar_visibility(&self.engine.borrow(), ctx);
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
    /// the pre-#1434 TUI shell's `on_shell_event`). Without pushing the change through,
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

    fn handle_poll_tick(&mut self, backend: &mut dyn quadraui::Backend) {
        // Schedule a redraw if the colorscheme changed (e.g. via
        // `:colorscheme`). #1498: this used to also reload a GTK-only CSS
        // provider theming the native file dialog here; that provider is
        // gone (JDonaghy/quadraui#1091 moved the equivalent stylesheet into
        // `GtkPlatformServices`, reloaded every frame by
        // `sync_per_frame_backend_state`'s `Backend::set_theme` call), so
        // this block's only remaining job is the redraw schedule.
        {
            let current = self.engine.borrow().settings.colorscheme.clone();
            if current != self.last_colorscheme {
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

        // Explorer refresh after confirmed file move — GTK-only, no TUI
        // counterpart (see `Self::explorer_needs_refresh`'s own doc).
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }

        // #1248: the rest of this function's chores — per-window viewport
        // sync, tab-visibility re-check, window title, yank-highlight clear,
        // idle/SC polling, deferred quit, terminal command, ext-panel focus,
        // platform-action drain — are shared with the pre-#1434 TUI shell's
        // `tick`. See `render::run_shared_tick_chores`'s header comment for
        // the full list.
        let engine_rc = self.engine.clone();
        let needs_redraw = {
            let mut engine = engine_rc.borrow_mut();
            render::run_shared_tick_chores(&mut engine, self, backend)
        };
        if needs_redraw {
            self.draw_needed.set(true);
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
    fn route_and_apply_editor_hover_popup(
        &self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) -> bool {
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
                // #1429: `units.hover_popup_pad` — was hardcoded to GTK's
                // 4px/4px inset even on the `tui` harness arm (`App` on
                // `TuiBackend`), disagreeing with that arm's own painted
                // 2-cell/1-cell popup frame.
                content: render::PopupContentMetrics {
                    pad_x: self.units.hover_popup_pad.0,
                    pad_y: self.units.hover_popup_pad.1,
                    col_width: self.cached_char_width.max(1.0) as f32,
                    line_height: self.cached_line_height.max(1.0) as f32,
                },
            },
            x,
            y,
        );
        let effect = render::apply_editor_hover_popup_route(&mut self.engine.borrow_mut(), route);
        if let Some(url) = effect.open_url {
            // #1134: `Engine::open_url` validates `is_safe_url` and queues a
            // `PendingPlatformAction::OpenUrl` for `tick_dispatch` to carry
            // out through `PlatformServices` — this method has no `backend`
            // handle of its own (see `Engine::pending_platform_actions`'s doc).
            self.engine.borrow_mut().open_url(&url);
        }
        if let Some(target) = effect.begin_drag {
            let drag_rc = backend.drag_state_handle();
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
            // #1134: see `route_and_apply_editor_hover_popup`'s identical
            // comment above — `Engine::open_url` validates + queues.
            self.engine.borrow_mut().open_url(&url);
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
    fn reconcile_editor_hover_modal(&self, backend: &dyn quadraui::Backend) {
        let editor_hover_id = quadraui::WidgetId::new("editor_hover");
        let engine = self.engine.borrow();
        let visible = engine.editor_hover.is_some();
        let rect = self.editor_hover_popup_rect.get();
        drop(engine);
        let stack_rc = backend.modal_stack_handle();
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
    fn dispatch_context_menu_click(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        if self.engine.borrow().context_menu.is_none() {
            // Defensive cleanup: the menu may have closed via Esc/Enter while
            // no click was seen by us. Pop any stale entry.
            backend.modal_stack_handle().borrow_mut().pop(&cm_id);
            return false;
        }

        // Keep the menu's painted bounds on the modal stack so any other modal
        // that might be open (picker, dialog) is arbitrated against it by the
        // *drag* guard, which still consults the stack.
        if let Some(bounds) = self.context_menu_layout.borrow().as_ref().map(|l| l.bounds) {
            backend
                .modal_stack_handle()
                .borrow_mut()
                .push(cm_id, bounds);
        }

        match self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress) {
            render::ModalOverlayRoute::ContextMenu(route) => {
                self.apply_context_menu_route(backend, route);
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

    /// Resolve `(x, y)` against the currently-painted `screen.windows`,
    /// returning the name and rect of the editor-tab-hosted plugin view
    /// under it, if any (#1627, #1631).
    ///
    /// Shared by every mouse route that needs to know whether a pixel
    /// belongs to a `vimcode.ui.register_view` view opened as an editor-area
    /// tab (`Engine::open_plugin_view_tab`) — press, double-click and wheel
    /// alike — so the three routes can't disagree about which window a
    /// pixel landed in. Walks the same `screen.windows` rects `click.rs`
    /// itself resolves clicks against.
    fn plugin_view_tab_hit(&self, x: f64, y: f64) -> Option<(String, quadraui::Rect)> {
        self.cached_screen_layout.borrow().as_ref().and_then(|l| {
            l.windows.iter().find_map(|w| {
                let name = w.plugin_view.clone()?;
                let rect: quadraui::Rect = w.rect.into();
                rect.contains(quadraui::Point::new(x as f32, y as f32))
                    .then_some((name, rect))
            })
        })
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
    pub(crate) fn paint_editor_windows_rung(
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

            // #1627: an editor-tab-hosted `vimcode.ui.register_view` view
            // paints a `Form`, not buffer text — the same shared
            // `quadraui::Form` primitive + adapter
            // (`render::plugin_view_to_form`) the sidebar body already paints
            // through, just routed through its own `FormController`/rect pair
            // (`Engine::plugin_view_tab_form_controller`/`_tab_form_rect`) so
            // a simultaneously-visible sidebar view can't clobber this one's
            // cached click geometry. `window_editors` still gets `editor`
            // (an empty placeholder — `rw.lines` is `vec![]` for a
            // plugin-view window, see `render::build_rendered_window`'s
            // short-circuit) so its indices stay 1:1 with `screen.windows`
            // for `compose_editor_band_rungs`' later `FrameHitMap` build.
            if let Some(view_name) = &rw.plugin_view {
                let engine = self.engine.borrow();
                engine.plugin_view_tab_form_rect.set(rect);
                // #1631: same body-kind branch as the sidebar arm
                // (`paint_sidebar_panel_rung`'s `ext:` case) — see that
                // arm's comment for why `plugin_view_tab_form_rect` is
                // reused as "the rect the active body last painted into"
                // for every body kind, not just the field-stack `Form`.
                let kind = engine
                    .plugin_views
                    .get(view_name)
                    .and_then(|v| v.body.as_ref())
                    .map(|b| b.kind_name());
                match kind {
                    Some("list") => {
                        render::paint_plugin_view_list(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                            rect,
                            backend,
                        );
                    }
                    Some("tree") => {
                        if render::populate_plugin_view_tree_controller(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                        ) {
                            engine
                                .plugin_view_tab_tree_controller
                                .borrow()
                                .render(backend, rect);
                        }
                    }
                    Some("table") => {
                        render::paint_plugin_view_table(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                            rect,
                            backend,
                        );
                    }
                    Some("text_view") => {
                        render::paint_plugin_view_text(
                            &engine,
                            view_name,
                            PluginViewHost::Tab,
                            rw.is_active,
                            rect,
                            backend,
                        );
                    }
                    _ => {
                        if render::populate_plugin_view_tab_form_controller(
                            &engine,
                            view_name,
                            rw.is_active,
                        ) {
                            engine
                                .plugin_view_tab_form_controller
                                .borrow_mut()
                                .render_and_cache(backend, rect);
                        }
                    }
                }
                drop(engine);
                window_editors.push(editor);
                continue;
            }

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
            // #1690: with exactly one window (no `:split`/`:vsplit`), paint
            // a plain backdrop fill edge-to-edge across the real window/
            // terminal width *before* the real (window-bounded) bar below —
            // VS Code's status bar runs under the activity bar and sidebar
            // too, not just the editor pane (see the issue's side-by-side
            // pixel sampling). `AppShell::render` paints the sidebar/
            // activity bar *before* `render_content` ever runs (see
            // `shell_adapter::render`), so this backdrop, painted here,
            // after, simply draws over their bottom edge, capping them —
            // with no `AppShellLayout`/sidebar-height change needed. See
            // `render::paint_status_backdrop`'s doc for why this is a
            // separate, content-free paint rather than widening `sb_rect`
            // itself. With two or more windows there is no single
            // VS-Code-shaped bar to backdrop this way — each split keeps
            // its own, Vim-style, window-bounded status line, unchanged.
            if screen.windows.len() == 1 {
                let real_bg = win_bar
                    .left_segments
                    .first()
                    .or(win_bar.right_segments.first())
                    .map(|s| s.bg);
                let backdrop_rect =
                    quadraui::Rect::new(0.0, bar_y as f32, backend.viewport().width, lh as f32);
                render::paint_status_backdrop(
                    backend,
                    &format!("status-backdrop:{}", rw.window_id.0),
                    backdrop_rect,
                    real_bg,
                );
            }
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
            // same reasoning as `render::PaintedTabBar::layout`.
            let sb_layout = backend.draw_status_bar_interactive(
                sb_rect,
                &win_bar,
                &quadraui::InteractionState::new(),
            );
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
                // #1389: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059) in a single call — `None`/`None` here
                // reproduce this arm's pre-existing "no chrome" behaviour
                // exactly (`layout.body_rect == q_sb`); TUI's
                // `panels::render_explorer_sidebar_content` uses the same
                // composer with `background: Some(tab_bar_bg)`. `render_with`
                // takes the body as a closure with no `Send + 'static` bound,
                // so the `!Send`, `Rc<RefCell<_>>`-backed `TreeController` on
                // `Engine` can be the body directly instead of the
                // hand-copied `render::paint_sidebar_panel_chrome` split
                // #1242 needed before #1059 existed.
                // #1693: `StatusBars` reserves one row above the tree for
                // the view-actions toolbar (New File / New Folder /
                // Refresh / Collapse All / "..." overflow) — the same
                // `SidebarPanelChrome` variant the Debug sidebar's title/
                // action bars already use (`debug_sidebar_chrome`, just
                // below). `body_rect` (what the closure receives) is
                // already narrowed below the chrome row, so
                // `explorer_tree_rect`/`explorer_viewport_rows` need no
                // change to account for it.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::SidebarPanelChrome::StatusBars(vec![
                        render::explorer_toolbar_status_bar(theme),
                    ]),
                    scrollbar_gutter: None,
                };
                render::populate_explorer_tree_controller(engine, theme);
                // Capture the exact metrics the tree is drawn with so the
                // click hit-test (which reads the backend's mutable
                // current_line_height at a later, possibly-different time) can
                // re-apply them and resolve the correct row. (#540)
                self.cached_explorer_metrics
                    .set((backend.line_height() as f64, backend.char_width() as f64));
                let layout = panel.render_with(backend, q_sb, |backend, body_rect| {
                    engine.explorer_tree_rect.set(body_rect);
                    engine.explorer_viewport_rows.set(body_rect.height as usize);
                    engine.explorer_tree.borrow().render(backend, body_rect);
                });
                engine
                    .explorer_toolbar_hits
                    .replace(layout.status_bar_hit_regions);
            }
            PANEL_SEARCH => {
                // #1065: `search_sidebar_system` never had `set_backend_info`
                // called on this backend — the exact #971 gap
                // (`render::UnitProfile::sidebar_system_metrics`'s own doc)
                // that left `SidebarSystem::handle_cached` returning
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
                    .set_backend_info(search_lh, (self.units.sidebar_system_metrics)(search_lh));
                render::populate_search_sidebar_system(engine, &engine.cwd, theme);
                engine.search_sidebar_body_rect.set(q_sb);
                engine.search_sidebar_system.borrow().render(backend, q_sb);
            }
            PANEL_DEBUG => {
                // #1392: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059) with `SidebarPanelChrome::StatusBars`
                // (quadraui#1061, `render::debug_sidebar_chrome`) instead of
                // slicing `q_sb` into a title row + action row by hand and
                // calling `Backend::draw_status_bar` on each directly. The
                // returned layout's `status_bar_hit_regions` are already in
                // `q_sb`'s own absolute pixel space, so they're stored
                // straight onto `engine.dap_sidebar_action_hits` for
                // `route_debug_sidebar_event` to read — no more separately
                // cached `action_rect` to translate a press into (that was
                // a second, independently-derived copy of the same
                // geometry the paint used, which is exactly what let paint
                // and click disagree). TUI's `render_debug_sidebar` builds
                // the identical chrome through the same helper.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::debug_sidebar_chrome(&screen.debug_sidebar, theme),
                    scrollbar_gutter: None,
                };
                let layout = panel.render_with(backend, q_sb, |backend, body_rect| {
                    engine.dap_sidebar_body_rect.set(body_rect);
                    render::populate_dap_sidebar_system(engine);
                    engine
                        .dap_sidebar_system
                        .borrow()
                        .render(backend, body_rect);
                });
                engine
                    .dap_sidebar_action_hits
                    .replace(layout.status_bar_hit_regions);
            }
            PANEL_GIT => {
                // #1390: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059, the composer the `PANEL_EXPLORER` arm
                // uses, #1389) instead of operating on `q_sb` directly with
                // no wrapper. `chrome: SidebarPanelChrome::None` — the
                // shell's own sidebar header already titles this panel
                // "SOURCE CONTROL", so `sc_header_status_bar`'s row below
                // (live branch/ahead-behind) is body content, not a second
                // title (#1256's double-header bug). `None` chrome reserves
                // no rows, so `body_rect` is pixel-identical to `q_sb` —
                // shadow `q_sb` with it below rather than threading a second
                // name through every band/popup rect in this arm.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::SidebarPanelChrome::None,
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    let q_sb = body_rect;
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
                            sc.has_focus,
                        );
                        self.cached_sc_bands.set(Some(bands));
                        let header_bar = render::sc_header_status_bar(sc, theme);
                        let _ = backend.draw_status_bar_interactive(
                            bands.header,
                            &header_bar,
                            &quadraui::InteractionState::new(),
                        );

                        let ti = render::sc_commit_message_to_text_input(sc);
                        backend.draw_text_input(bands.commit_input, &ti);

                        // Render the toolbar-slab + section list below the
                        // header + commit input.
                        let slab_rect = bands.slab;
                        render::draw_sc_sidebar_panel(backend, engine, sc, slab_rect);
                        // Focused-hint row (#1361): shared with TUI through
                        // `render::sc_hint_status_bar`, painted only when
                        // `ScSidebarBands::hint` was reserved (i.e. the panel
                        // has focus) so it can't show unreserved space.
                        if let Some(hint_rect) = bands.hint {
                            let hint_bar = render::sc_hint_status_bar(theme);
                            let _ = backend.draw_status_bar_interactive(
                                hint_rect,
                                &hint_bar,
                                &quadraui::InteractionState::new(),
                            );
                        }
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
                        // silent no-op — see `render::UnitProfile::
                        // sidebar_system_metrics`'s own doc for the full
                        // story. Reads `backend.line_height()`
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
                            .set_backend_info(sc_lh, (self.units.sidebar_system_metrics)(sc_lh));
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
                });
            }
            PANEL_EXTENSIONS => {
                // #1343: the shell's own `AppShell` sidebar header already
                // paints " EXTENSIONS " above `q_sb` — this arm paints only
                // the search/filter row (#1256 found GTK painting neither
                // row at all).
                //
                // #1391: composed through `SidebarPanelBody::render_with`
                // (quadraui#1059, the composer the `PANEL_EXPLORER`/
                // `PANEL_GIT` arms use) with `SidebarPanelChrome::Search`
                // (quadraui#1061) instead of the bespoke `render::
                // paint_sidebar_search_row` (deleted by this issue) — see
                // `render::search_only_chrome`'s doc. TUI's `panels::
                // render_ext_sidebar` builds the identical chrome through
                // the same helper.
                let panel = render::SidebarPanelBody {
                    background: None,
                    chrome: render::search_only_chrome(
                        &engine.ext_sidebar_query,
                        "Search extensions (press /)",
                        engine.ext_sidebar_input_active,
                        theme,
                    ),
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    Self::refresh_ext_sidebar_metrics(backend, engine, self.units);
                    render::populate_ext_sidebar_system(engine);
                    engine.ext_sidebar_body_rect.set(body_rect);
                    engine
                        .ext_sidebar_system
                        .borrow()
                        .render(backend, body_rect);
                });
            }
            PANEL_BOARD => {
                // #521: generic Board panel host — the *only* GTK-specific
                // code here is picking which `screen.board` field to read
                // and where to put the rect; the actual rasterisation is
                // `Backend::draw_board`, the same call TUI's
                // `panels::render_board_panel` makes (Platform-Neutrality
                // Rule: no bespoke board drawing per backend).
                if let Some(ref board) = screen.board {
                    if let Some(ref model) = board.model {
                        let layout = backend.draw_board(q_sb, model);
                        engine.board_layout.replace(Some(layout));
                    } else {
                        engine.board_layout.replace(None);
                        if let Some(ref status) = board.status {
                            let bar = render::board_status_bar(status, theme);
                            let rect = quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, lh as f32);
                            let _ = backend.draw_status_bar_interactive(
                                rect,
                                &bar,
                                &quadraui::InteractionState::new(),
                            );
                        }
                    }
                }
            }
            PANEL_SETTINGS => {
                // #1343: Settings is a shell bottom item that owns the
                // sidebar header (quadraui#1055, #1356's pin bump) — the
                // shell paints " SETTINGS " above `q_sb`, so this arm
                // paints only the filter row, instead of the pre-#1343
                // `SidebarPanelChrome::None` (no chrome at all — #1256's
                // `settings_filter_row_is_painted::gtk` gap).
                //
                // #1391: composed through `SidebarPanelBody::render_with`
                // with `SidebarPanelChrome::Search` (quadraui#1061) instead
                // of the bespoke `render::paint_sidebar_search_row` (deleted
                // by this issue) — see `render::search_only_chrome`'s doc.
                // TUI's `panels::render_settings_panel` builds the identical
                // chrome through the same helper.
                //
                // #1574: `background` is `Some(theme.tab_bar_bg)`, not
                // `None` — the same sidebar background the `PANEL_EXPLORER`
                // arm's doc comment above says TUI passes for this composer.
                // With `None`, nothing fills the strip under the settings
                // scrollbar's gutter, so the backend's own clear colour
                // (dark on macOS regardless of the active colourscheme)
                // showed through there instead of the themed sidebar bg.
                // GTK-only: `src/gtk/testing.rs`'s
                // `settings_panel_scrollbar_gutter_paints_theme_tab_bar_bg`
                // covers it with a pixel probe (that module's own doc
                // records why an equivalent TUI probe can't reproduce this
                // one — `FormController` already paints an opaque per-row
                // background there regardless of this field).
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::search_only_chrome(
                        &engine.settings_query,
                        "",
                        engine.settings_input_active,
                        theme,
                    ),
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    // Cache the exact rect this frame painted the form into
                    // (mirrors TUI's `panels::render_settings_panel`, #1238)
                    // — `try_route_sidebar_mouse_event`'s `Settings` arm
                    // reads this back instead of the raw, unshrunk `q_sb`,
                    // which would resolve clicks against `FormController`'s
                    // row geometry one search-row off from what was
                    // actually painted (#1343 review: this is exactly the
                    // `settings_row_click_below_its_glyph_hits_its_own_row_gtk`
                    // regression a stale `sb` reintroduced).
                    engine.settings_form_rect.set(body_rect);
                    render::populate_settings_form_controller(engine);
                    engine
                        .settings_form_controller
                        .borrow_mut()
                        .render_and_cache(backend, body_rect);
                });
            }
            id if id
                .strip_prefix("ext:")
                .is_some_and(|name| engine.is_plugin_view(name)) =>
            {
                // #146: a `vimcode.ui.register_view` panel — the plugin declared
                // a widget tree, so paint it as a `quadraui::Form` through the
                // *same* shared `FormController` the `PANEL_SETTINGS` arm above
                // uses, rather than as `ExtPanelItem` tree rows. This is the
                // whole per-surface cost of plugin UI: one arm in the shared
                // shell, zero lines in `src/gtk/` or `src/tui_main/`.
                //
                // #1631: a view whose `render()` returned a `kind = "..."`
                // table instead of `fields` paints through the matching
                // `ListView`/`TreeView`/`DataTable`/`TextDisplay` primitive
                // instead — still one arm here, still zero lines in
                // `src/gtk/`/`src/tui_main/`. `plugin_view_form_rect` is
                // reused as "the rect the active body last painted into"
                // regardless of which body kind is active (mutually
                // exclusive per view, so there is no collision).
                let name = id.strip_prefix("ext:").unwrap_or(id).to_string();
                let panel = render::SidebarPanelBody {
                    background: Some(theme.tab_bar_bg),
                    chrome: render::SidebarPanelChrome::Header(format!(
                        " {}",
                        screen
                            .ext_panel
                            .as_ref()
                            .map(|p| p.title.clone())
                            .unwrap_or_default()
                    )),
                    scrollbar_gutter: None,
                };
                panel.render_with(backend, q_sb, |backend, body_rect| {
                    // Same contract as the Settings arm: cache the exact rect
                    // painted, because the click routers re-derive row
                    // geometry from it.
                    engine.plugin_view_form_rect.set(body_rect);
                    engine.ext_panel_content_rect.set(q_sb);
                    let has_focus = engine.ext_panel_has_focus;
                    let kind = engine
                        .plugin_views
                        .get(&name)
                        .and_then(|v| v.body.as_ref())
                        .map(|b| b.kind_name());
                    match kind {
                        Some("list") => {
                            render::paint_plugin_view_list(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                                body_rect,
                                backend,
                            );
                        }
                        Some("tree") => {
                            if render::populate_plugin_view_tree_controller(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                            ) {
                                engine
                                    .plugin_view_tree_controller
                                    .borrow()
                                    .render(backend, body_rect);
                            }
                        }
                        Some("table") => {
                            render::paint_plugin_view_table(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                                body_rect,
                                backend,
                            );
                        }
                        Some("text_view") => {
                            render::paint_plugin_view_text(
                                engine,
                                &name,
                                PluginViewHost::Sidebar,
                                has_focus,
                                body_rect,
                                backend,
                            );
                        }
                        _ => {
                            if render::populate_plugin_view_form_controller(engine) {
                                engine
                                    .plugin_view_form_controller
                                    .borrow_mut()
                                    .render_and_cache(backend, body_rect);
                            }
                        }
                    }
                });
            }
            id if id.starts_with("ext:") => {
                // #1089: a plugin-provided panel — paint its own sections +
                // items via `render::ext_panel_to_tree_view` +
                // `Backend::draw_tree`, the same adapter
                // `tui_main::panels::render_ext_panel` uses, instead of
                // falling through to `ext_sidebar_system` (the extension
                // *marketplace* — INSTALLED/AVAILABLE — which is what this
                // arm painted before this fix, unconditionally, for every
                // `ext:<name>` id).
                //
                // #1242: chrome + body now composed through quadraui#1041's
                // `SidebarPanelBody::render` (`render::ExtPanelTreeBody`,
                // same `&dyn BackendWidget` path `panels::render_ext_panel`
                // uses). `scrollbar_gutter: None` — unlike TUI, this arm has
                // never painted one.
                if let Some(ref panel) = screen.ext_panel {
                    // Cache the whole content rect (chrome included),
                    // verbatim — mirrors `render_ext_panel`'s own
                    // `ext_panel_content_rect` write so a future hover/geometry
                    // consumer can't tell which backend painted this frame.
                    engine.ext_panel_content_rect.set(q_sb);

                    let input_visible = panel.input_active || !panel.input_text.is_empty();
                    let header_title = format!(" {}", panel.title);
                    let sidebar_panel = render::SidebarPanelBody {
                        background: None,
                        chrome: if input_visible {
                            render::SidebarPanelChrome::HeaderAndSearch {
                                header: header_title,
                                query: panel.input_text.clone(),
                                placeholder: String::new(),
                                active: panel.input_active,
                            }
                        } else {
                            render::SidebarPanelChrome::Header(header_title)
                        },
                        scrollbar_gutter: None,
                    };
                    let body =
                        render::ExtPanelTreeBody(render::ext_panel_to_tree_view(panel, theme));
                    let layout = sidebar_panel.render(backend, q_sb, &body);

                    if layout.body_rect.height > 0.0 {
                        // #1089: cache the exact `Backend::tree_layout` this
                        // frame painted with — the click router
                        // (`render::route_ext_panel_click`, shared with TUI's
                        // `mouse::handle_mouse`) reads this instead of
                        // re-deriving row geometry from a uniform row height.
                        // See `Engine::ext_panel_tree_layout`'s own doc for why
                        // that matters here: this backend pitches a tree's
                        // header rows shorter than its item rows.
                        let tree_layout = backend.tree_layout(layout.body_rect, &body.0);
                        engine
                            .ext_panel_tree_layout
                            .replace(Some((layout.body_rect, tree_layout)));
                    } else {
                        engine.ext_panel_tree_layout.replace(None);
                    }
                } else {
                    engine.ext_panel_tree_layout.replace(None);
                }
            }
            PANEL_AI => {
                // #819: adopts quadraui's `ChatController` — one shared
                // `render()` for both backends, like `explorer_tree` and
                // `ext_sidebar_system` above, replacing the hand-painted
                // `draw_ai_sidebar_panel`. `ai_chat_rect` is cached on
                // `Engine` (not here) so `route_ai_chat_event` re-derives
                // the identical layout `render()` painted (#544/#582/#646).
                render::populate_ai_chat_controller(engine, theme, backend);
                // #1513: the pinned plan block gets a band carved off the
                // top of the panel rect *before* `ChatController` ever sees
                // the remainder — `ai_chat_rect` (and therefore every
                // `route_ai_chat_event` hit-test) is the shrunk rect, so a
                // click inside the band never reaches the transcript below
                // it. Zero-height (no-op split) while there's no plan.
                let plan_h = render::ai_plan_band_height(engine, backend.line_height());
                let (plan_rect, chat_rect) = if plan_h > 0.0 {
                    (
                        quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, plan_h),
                        quadraui::Rect::new(
                            q_sb.x,
                            q_sb.y + plan_h,
                            q_sb.width,
                            (q_sb.height - plan_h).max(0.0),
                        ),
                    )
                } else {
                    (quadraui::Rect::new(q_sb.x, q_sb.y, 0.0, 0.0), q_sb)
                };
                render::paint_ai_plan_band(backend, engine, theme, plan_rect);
                engine.ai_chat_rect.set(chat_rect);
                engine.ai_chat.borrow().render(backend, chat_rect);
                // #956 (ACP-5): slash-command completions, painted on top —
                // no-op unless the input matches an agent-declared command.
                // `chat_rect` (not `q_sb`), per `paint_ai_command_completions`'s
                // own contract: the same rect `ai_chat.render()` just used,
                // so the popup anchors off the input box's *actual* position
                // — unaffected by the #1513 plan-band split in the common
                // case (the band only eats space off the top), but wrong to
                // silently keep passing the unshrunk sidebar rect here now
                // that the two can differ.
                render::paint_ai_command_completions(backend, engine, chat_rect);
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
    /// `render::UnitProfile::sidebar_system_metrics`'s own doc for the full
    /// story. Reads `backend.line_height()` directly rather than accepting a
    /// cached `lh` parameter — see the `PANEL_GIT` arm's identical comment
    /// in [`Self::paint_sidebar_panel_rung`] on why: a cached value can lag
    /// behind what `backend` reports by the time `render()` actually reads
    /// it, so the metrics `handle_cached` hit-tests against could disagree
    /// with what this exact `render()` call paints. Shared by the
    /// `PANEL_EXTENSIONS` arm and the `id if id.starts_with("ext:")` arm
    /// above, which were previously two verbatim copies of this same
    /// four-line snippet.
    fn refresh_ext_sidebar_metrics(
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        units: render::UnitProfile,
    ) {
        let ext_lh = backend.line_height();
        engine
            .ext_sidebar_system
            .borrow_mut()
            .set_backend_info(ext_lh, (units.sidebar_system_metrics)(ext_lh));
    }

    /// Compose the editor-anchored popups: completion menu, LSP hover, editor
    /// hover (rich markdown), diff peek and signature help.
    ///
    /// Extracted out of `render_content` (#766) for the same reason as
    /// [`Self::paint_sidebar_panel_rung`] — it was ~160 lines of geometry in
    /// the middle of what #766 makes a statement of the frame's *order*.
    /// Deliberately **not** a `FrameOp` rung: these are anchored to the active
    /// window's cursor rather than to a band, and both backends compose them at
    /// exactly this point (TUI through `render_impl.rs`'s `paint_editor_popups`),
    /// between the editor band and the bottom band.
    ///
    /// #1167: the build-adapter → `.layout()` → `backend.draw_*` → cache-
    /// output part (identical to TUI's `render_impl::paint_editor_popups`,
    /// modulo coordinate units) now lives once in `render::paint_editor_popups`.
    /// #1237 folded the anchor-point arithmetic itself into
    /// `render::editor_popup_anchors` too (see its doc — GTK's completion
    /// anchor, and all four non-completion anchors on *both* backends, used
    /// the raw character column as a tab-expanded display column, drifting
    /// left of the cursor on tab-indented lines). This method's own job
    /// shrank to exactly what stays genuinely per-backend: finding the
    /// active window, scaling into GTK's native pixel units (`lh`/`cw`),
    /// and picking each popup's clip viewport.
    ///
    /// `main` is `AppShellLayout::main_content_bounds` — the clip viewport
    /// every popup but the completion menu and editor-hover popup is placed
    /// inside; those two clamp to the active window's own rect instead (see
    /// `render::paint_editor_popups`'s doc for why).
    fn paint_editor_popups_rung(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        theme: &Theme,
        main: quadraui::Rect,
        lh: f64,
        cw: f64,
    ) {
        let active_win = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id);

        let win_viewport = active_win.map(|active_win| {
            quadraui::Rect::new(
                active_win.rect.x as f32,
                active_win.rect.y as f32,
                active_win.rect.width as f32,
                active_win.rect.height as f32,
            )
        });

        // #1237: the char→display column arithmetic (tab expansion, scroll
        // offset) for every one of the five anchors below now lives once in
        // `render::editor_popup_anchors`, shared with TUI's
        // `render_impl::paint_editor_popups`. GTK uses its raw sub-pixel
        // float window origin as-is (no whole-cell snapping — that's a
        // TUI-only concern).
        //
        // Note the precision drop: `win_origin`/`cw`/`lh` are cast to `f32`
        // *here*, before the anchor arithmetic runs, whereas pre-#1237 each
        // anchor's arithmetic ran entirely in `f64` and only cast to `f32`
        // at the very end for `PopupAnchor`. `editor_popup_anchors` is
        // shared with TUI, whose native units are already `f32` cell
        // columns, so it takes `f32` throughout — GTK pays for that sharing
        // with one extra rounding step. Immaterial at realistic screen-pixel
        // magnitudes (`f32` keeps ~7 significant decimal digits, far more
        // than any on-screen coordinate needs), and reviewed as acceptable
        // in #1237's follow-up round rather than threading `f64` generics
        // through a function TUI also calls.
        let win_origin = active_win.map(|w| (w.rect.x as f32, w.rect.y as f32));
        let anchor_points = render::editor_popup_anchors(screen, win_origin, cw as f32, lh as f32);

        // Completion popup anchor — cache the layout so the click handler
        // (B.5b Stage 5) can hit-test items and register the popup on the
        // modal stack.
        let completion = screen.completion.as_ref().and_then(|menu| {
            let (cursor_x, cursor_y) = anchor_points.completion?;
            // Longest candidate + 4 cells of padding/border, floored at 12
            // cells' worth of raw units (`cw`-scaled, not a bare pixel
            // constant — #1432: this function runs unmodified for both `gtk`
            // and TUI-via-`App`, and a flat `100.0` floor is a GTK pixel
            // width. On TUI `cw` is `1.0` (cell = 1 raw unit), so that same
            // constant used to demand a 100-*cell*-wide popup — wider than
            // most terminals — which made `Completions::layout`'s own
            // right-edge-overflow branch always fire and clamp `x` straight
            // back to `viewport.x`, discarding the correctly-computed cursor
            // anchor outright (observed: popup pinned at the window's left
            // edge regardless of cursor column). Both the `+4`/`.max(12.0)`
            // now match `tui_main::render_impl`'s independent
            // completion-width calc exactly (its own comment there names
            // the same reasoning) — this used to read `+2`, which happened
            // to never matter while `100.0` always dominated it, but once
            // scaled down to a real cell-sized floor a too-narrow candidate
            // term clipped the popup's own last character (observed:
            // `"ZQXWFOOBAR"` painted as `"ZQXWFOOBA"`, missing the border's
            // trailing padding cell). Scaling the shared `12.0` floor by
            // `cw` here keeps GTK's effective minimum close to its old
            // ~100px (12 * a typical ~8px cell) while giving TUI-via-`App`
            // the same sane 12-cell floor `render_impl` already ships.
            //
            // #420: `Completions::layout` clamps the popup's *position*
            // into `win_viewport` (`x.max(viewport.x)`) but never clamps
            // `popup_w` itself, so a long enough candidate label can still
            // render past the window's own right edge even once positioned
            // as far left as it can go. That's a gap in the shared
            // `quadraui::Completions::layout` primitive (it already clips
            // height the same way, via `clipped_h`), not something to patch
            // around per-backend — see the Platform-Neutrality Rule.
            // Tracked as a follow-up pending a quadraui-side fix rather than
            // duplicating a `.min(...)` clamp here and in
            // `tui_main::render_impl`.
            let popup_w = ((menu.max_width + 4) as f64 * cw).max(12.0 * cw);
            let max_popup_h = 10.0 * lh;
            Some((
                render::PopupAnchor {
                    x: cursor_x,
                    y: cursor_y,
                    viewport: win_viewport?,
                },
                popup_w as f32,
                max_popup_h as f32,
            ))
        });

        // Simple LSP hover popup anchor (plain text, non-interactive).
        let hover = anchor_points.hover.map(|(x, y)| render::PopupAnchor {
            x,
            y,
            viewport: main,
        });

        // Signature-help popup anchor (insert mode, cursor inside a call).
        let signature_help = anchor_points
            .signature_help
            .map(|(x, y)| render::PopupAnchor {
                x,
                y,
                viewport: main,
            });

        // Diff-peek popup anchor (inline git hunk preview).
        let diff_peek = anchor_points.diff_peek.map(|(x, y)| render::PopupAnchor {
            x,
            y,
            viewport: main,
        });

        // Editor hover popup anchor (rich markdown; `gh` key, diagnostic/
        // annotation/plugin hovers, or mouse dwell). Bounds/link rects/
        // scrollbar geometry are cached for the click + drag handlers
        // (#215), same as `draw.rs::draw_editor_hover_popup` did.
        let editor_hover = anchor_points.editor_hover.and_then(|(x, y)| {
            Some(render::PopupAnchor {
                x,
                y,
                viewport: win_viewport?,
            })
        });

        let mut completion_layout = self.completion_layout.borrow_mut();
        let mut editor_hover_link_rects = self.editor_hover_link_rects.borrow_mut();
        let mut editor_hover_popup_rect = self.editor_hover_popup_rect.get();
        let mut editor_hover_scrollbar = self.editor_hover_scrollbar.get();
        render::paint_editor_popups(
            backend,
            screen,
            theme,
            cw as f32,
            lh as f32,
            completion,
            hover,
            editor_hover,
            diff_peek,
            signature_help,
            &mut completion_layout,
            &mut editor_hover_link_rects,
            &mut editor_hover_popup_rect,
            &mut editor_hover_scrollbar,
        );
        self.editor_hover_popup_rect.set(editor_hover_popup_rect);
        self.editor_hover_scrollbar.set(editor_hover_scrollbar);
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
        // #1580: re-synced every frame, same reasoning as the theme sync
        // right above — a runtime `:set menu_style=...` (or its Settings-
        // sidebar equivalent) reaches quadraui's own `MenuStyle` on the
        // very next paint. `set_menu_style` only persists a value on the
        // backend struct (no popup, no blocking call), so calling it
        // every frame is as cheap as the theme/font syncs it sits next to.
        backend.set_menu_style(render::to_quadraui_menu_style(engine.settings.menu_style));
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
        sync_ui_font_size(&engine.settings, backend);
        // #705 item 3 / quadraui#624: push the same UI_FONT() family+size
        // onto the *paint* backend's `ui_font`, which `draw_status_bar`
        // (breadcrumbs, per-window/global status lines), `draw_tree`
        // (explorer), `draw_tab_bar_icons`, and `draw_menu_bar` now all
        // honour for both paint and their no-paint measurement twins
        // (quadraui#624). Before this call `ui_font` on the paint backend
        // was never touched, so it
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
        //
        // #1542: resolved through `resolve_editor_font` (same function
        // `shell_config`'s pre-seed above uses) instead of the raw fields,
        // so an un-customized setting keeps tracking this backend's
        // platform-native convention every frame, not just at startup.
        let (editor_family, editor_size_pt) = resolve_editor_font(&engine.settings, backend);
        backend.set_editor_font(&editor_family, editor_size_pt);

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

    /// Compose the **editor band** (#764, #735 slice 3; converged into one
    /// shared walk by #1251) and recover this frame's `FrameHitMap` from it.
    ///
    /// Walks `render::paint_editor_band_rungs`, the single loop both this
    /// method and the pre-#1434 TUI shell's `paint_editor_band` now call —
    /// see that function's own doc for exactly which rungs need `self` and
    /// why. Extracted out of `render_content` (#766) so that function reads
    /// as the frame's *order*.
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

        // Reset the pixel-accurate hit caches; repopulated by the `TabBars`
        // rung below so the click / hover hit-tests use the exact drawn
        // geometry (#515). Cleared here, before the walk, rather than from an
        // absent-rung branch: `compose_editor_band` returns only the live
        // rungs, so a frame with no tab bars has no arm to clear them from.
        self.cached_group_tab_bar_layouts.borrow_mut().clear();
        self.cached_tab_slots_abs.borrow_mut().clear();
        // #1165: reset alongside the hit caches above — this frame's
        // `TabBars` rung (if any) repopulates it, and `handle_poll_tick`
        // wants this frame's measurements, not an accumulation across
        // frames (mirrors TUI's `tab_visible_counts.borrow_mut().clear()`).
        self.tab_visible_counts.borrow_mut().clear();

        // `window_editors`/`hit_bars` stash what the `Windows`/`TabBars`
        // rungs painted, past the walk (#449), so the `FrameHitMap` built
        // below references the SAME objects just painted rather than a
        // second copy that could drift — see `render::paint_editor_band_rungs`'s
        // doc for why this bookkeeping can't move into the shared walk itself.
        //
        // #1128 (was #731): both scrollbars ARE painted for the editor on
        // GTK today. quadraui#968 taught `gtk::editor::draw_editor` (the
        // rasteriser `Surface::Editor` below calls into) to paint both
        // scrollbars itself, the way `tui::editor::draw_editor` already
        // did — geometry comes from `Editor::layout`, the same call
        // hit-testing uses, so paint and click agree by construction (see
        // that rasteriser's module doc, "Scrollbars" section). This
        // replaced the #731-era claim that GTK "deliberately skips
        // scrollbars and defers to the host" — that sentence described the
        // dead Relm4-era native `gtk4::Scrollbar` overlay path
        // (`sync_scrollbar`/`create_window_scrollbars`) #731 deleted, which
        // never ran under the ShellApp runner in the first place (nothing
        // assigns `self.overlay`/`self.drawing_area`).
        //
        // `app_support::editor_scrollbar_layout` builds the same
        // `Editor`/`EditorLayout` pair (minus the rendered text, which
        // scrollbar geometry never reads) so the axis-parameterised
        // `scrollbar_thumb_geometry`/`scrollbar_hit_test` (#1493, was one
        // hand-rolled pair per axis) below can never resolve a hover/drag
        // rect paint didn't actually draw — #1128 deleted the pre-#968
        // h-scrollbar geometry helper that independently guessed its own
        // track width and could disagree with what was actually painted.
        let mut window_editors = Vec::with_capacity(screen.windows.len());
        let mut hit_bars = Vec::new();
        let units = render::EditorBandUnits::px(lh, cw, tab_row_h);
        let composed_editor = render::paint_editor_band_rungs(
            backend,
            engine,
            screen,
            theme,
            band,
            units,
            tab_bar_h,
            self.tab_drag.is_dragging(),
            self,
            &mut window_editors,
            &mut hit_bars,
        );
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
                    // from key events (unlike TUI's the pre-#1434 TUI shell), so the
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
                    // #1690: backdrop fill, full window/terminal width (not
                    // `main.width` — main-content-column-only), painted
                    // *before* the real, window-bounded bar below — see
                    // `render::paint_status_backdrop`'s doc (the same
                    // helper `paint_editor_windows_rung`'s single-window
                    // case uses) for why this is a separate, content-free
                    // paint rather than widening `sb_rect` itself.
                    let real_bg = status
                        .left_segments
                        .first()
                        .or(status.right_segments.first())
                        .map(|s| quadraui::Color::rgb(s.bg.r, s.bg.g, s.bg.b));
                    let backdrop_rect = quadraui::Rect::new(
                        0.0,
                        separated_status_y as f32,
                        backend.viewport().width,
                        el.separated_status_h as f32,
                    );
                    render::paint_status_backdrop(
                        backend,
                        "status-backdrop:separated",
                        backdrop_rect,
                        real_bg,
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

    pub(crate) fn paint_tab_bars_rung<'a>(
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
        let char_width = backend.char_width();
        let mut group_layouts = self.cached_group_tab_bar_layouts.borrow_mut();
        let mut slots_abs = self.cached_tab_slots_abs.borrow_mut();
        let mut visible_counts = self.tab_visible_counts.borrow_mut();
        for bar in painted {
            // #1165/#1491: the engine-feedback "how many columns of tab
            // strip fit" number `Engine::set_tab_visible_count` budgets
            // against — see `click::tab_bar_available_cols`'s doc for why
            // this reads the paint's own rect/layout instead of the
            // deprecated `TabBarHits::available_cols`.
            visible_counts.push((
                bar.group_id,
                tab_bar_available_cols(bar.rect, &bar.layout, char_width),
            ));
            // Recover the exact pixel geometry the rasteriser just drew and
            // cache it for hit-testing and tab-drop geometry.
            //
            // #764: `bar.layout` is what the *paint* returned, not a separate
            // re-measure this used to make. The icon reservation widens
            // every decorated tab, so an icon-less or differently-fonted
            // twin reports slot and close bounds shifted left of the
            // painted glyphs — i.e. the close × of tab N lands inside tab
            // N+1's painted slot, and clicking it closes the wrong tab.
            // Exactly the measure/paint desync of #654; reading the paint's
            // own answer makes it unreachable rather than merely fixed
            // (#703).
            slots_abs.insert(
                bar.group_id.0,
                abs_slot_positions_from_layout(bar.rect, bar.bar, &bar.layout),
            );
            hit_bars.push((bar.group_id, bar.rect, bar.bar));
            group_layouts.insert(bar.group_id.0, (bar.rect, bar.layout));
        }
    }

    /// Recompute the per-group tab-drop geometry from this frame's screen
    /// layout and stash it in `cached_drop_ctx`.
    ///
    /// One source for two consumers that must never disagree: the drag
    /// hit-test (`handle_mouse_drag_msg` → `render::resolve_tab_drop_zone`)
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
    pub(crate) fn cache_tab_drop_geometry(
        &self,
        screen: &render::ScreenLayout,
        engine: &Engine,
        tab_bar_h: f64,
    ) {
        let bounds = render::screen_to_drop_group_bounds(screen);
        // Per-tab slot x-positions (absolute) are captured by the `TabBars`
        // rung while drawing. Feeding them here makes a drag inside a group's
        // own tab bar resolve to a `TabReorder` (insertion bar) instead of
        // falling through to a new-split/center overlay. (#515)
        let slots_abs = self.cached_tab_slots_abs.borrow();
        let ctx = render::build_tab_drop_ctx(&bounds, engine, tab_bar_h as f32, &slots_abs);
        drop(slots_abs);
        *self.cached_drop_ctx.borrow_mut() = ctx;
    }

    /// Re-resolve a tab-drag press point to `(group, tab index)`, or `None`
    /// when the press was not on a tab after all.
    ///
    /// GTK arms the drag for the whole tab-bar band (its tab geometry is
    /// proportional-font pixel bounds, resolved by `pixel_to_click_target`, not
    /// the exact cell hit TUI gets for free), so the confirmation that
    /// `render::TabDragMove::Crossed` asks for is a real second hit-test here.
    fn tab_drag_source_at(
        &self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
    ) -> Option<(core::window::GroupId, usize)> {
        let layout_ref = self.cached_screen_layout.borrow();
        let layout = layout_ref.as_ref()?;
        let mut engine = self.engine.borrow_mut();
        let drag_rc = backend.drag_state_handle();
        let target = pixel_to_click_target(
            &mut engine,
            backend,
            x,
            y,
            self.cached_line_height,
            self.cached_char_width,
            layout,
            &self.cached_group_tab_bar_layouts.borrow(),
            self.cached_frame_hit_map.borrow().as_ref(),
            &self.cached_tab_bar_zones.borrow(),
            true, // resolving the original tab-bar mouse-down; switching tabs is intended
            &mut drag_rc.borrow_mut(),
            false, // re-resolving a tab-bar press; the minimap rung cannot match here
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
                &(self.units.picker_rows)(lh),
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
                    &(self.units.find_replace_anchor),
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
    fn apply_picker_route(&mut self, backend: &dyn quadraui::Backend, route: render::PickerRoute) {
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
                &(self.units.picker_rows)(lh),
                &engine,
            )
        };
        // Keep the stack in step: the drag guard in `handle_mouse_drag_msg`
        // consults it to stop a gesture leaking to the editor behind the modal
        // (#192).
        backend
            .modal_stack_handle()
            .borrow_mut()
            .push(picker_id.clone(), geo.bounds);

        match route {
            render::PickerRoute::Row(idx) => {
                render::apply_picker_row_click(&mut self.engine.borrow_mut(), idx);
            }
            render::PickerRoute::ScrollbarThumb { grab_offset } => {
                backend
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
                // #1630 review: clicking away from a `vimcode.picker.open`
                // picker is a user-initiated cancel exactly like Escape —
                // use the same helper so `on_cancel` fires and the plugin's
                // registration is released instead of leaking forever.
                self.engine.borrow_mut().close_picker_cancelling_plugin();
                backend.modal_stack_handle().borrow_mut().pop(&picker_id);
            }
        }
    }

    /// Apply a context-menu verdict from [`render::route_modal_overlay_click`].
    ///
    /// Returns `true` when the event was consumed. The route itself — which
    /// item, hover vs. click, dismiss vs. keep-open — is decided once in
    /// `render.rs` and shared with TUI's `handle_mouse`; what stays here is
    /// GTK's own plumbing (modal-stack bookkeeping, file-tree refresh).
    fn apply_context_menu_route(
        &mut self,
        backend: &dyn quadraui::Backend,
        route: render::ContextMenuRoute,
    ) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        let pop_stack = || {
            backend.modal_stack_handle().borrow_mut().pop(&cm_id);
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
                pop_stack();
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
                pop_stack();
            }
            render::ContextMenuRoute::Fallthrough => return false,
        }
        self.draw_needed.set(true);
        true
    }

    /// Resolve a press against one editor scrollbar axis — thumb-drag vs.
    /// track page-jump — shared between the horizontal and vertical rungs
    /// of `handle_mouse_click_msg` (#1493). Returns `true` when the point
    /// landed on a scrollbar and the click was consumed (either scrolled
    /// immediately or armed a drag); the caller should `return` without
    /// falling through to the next rung.
    ///
    /// Window rects always come from [`Self::painted_editor_bounds`] — the
    /// same cached, already-painted geometry the divider rung below reads
    /// via `painted_divider_geometry` — never a `compute_editor_window_rects`
    /// recompute from the drawing area's raw `width`/`height`, which always
    /// assumes the editor area starts at `x = 0`. That only holds with the
    /// activity bar/sidebar at zero width; with either painted, the real
    /// left edge is `AppShellLayout::main_content_bounds.x`, so a rect
    /// rebuilt from `(0, 0, width, height)` lands columns off from what was
    /// actually drawn (this is what made the pre-#1493 horizontal-only copy
    /// of this method resolve against a phantom rect once the sidebar/
    /// activity bar reserved real width).
    fn editor_scrollbar_press(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
        axis: ScrollbarAxis,
    ) -> bool {
        let Some((content_bounds, tab_bar_h)) = self.painted_editor_bounds() else {
            return false;
        };
        let lh = self.cached_line_height;
        let cw = self.cached_char_width;
        let engine = self.engine.borrow();
        let (rects, _dividers) = engine.calculate_group_window_rects(content_bounds, tab_bar_h);
        let Some((win_id, scroll_at_click)) =
            scrollbar_hit_test(&engine, x, y, &rects, cw, lh, axis)
        else {
            return false;
        };
        let win_rect = rects.iter().find(|(id, _)| *id == win_id).map(|(_, r)| *r);
        let geom = win_rect
            .and_then(|rect| scrollbar_thumb_geometry(&engine, win_id, &rect, cw, lh, axis));
        drop(engine);
        let Some((track_x, track_y, track_w, track_h, thumb_pos, thumb_len, scroll_range, _)) =
            geom
        else {
            return false;
        };
        let max_scroll = scroll_range.round() as usize;
        // #1061: `resolve_editor_scrollbar_click` below is the shared
        // click-vs-drag resolver used by this (now axis-parameterised, #1493)
        // press handler — the standalone TUI `tui_main/mouse.rs` module this
        // comment used to reference was deleted by #1434 once #1433 flipped
        // TUI's `run` onto this same shared `App` dispatch; see that
        // function's doc for the full click/drag rationale.
        let (click_pos, track_visible, track_start, track_length) = match axis {
            ScrollbarAxis::Horizontal => (
                x as f32,
                (track_w / cw).floor() as usize,
                track_x as f32,
                track_w as f32,
            ),
            ScrollbarAxis::Vertical => (
                y as f32,
                (track_h / lh.max(1.0)).floor() as usize,
                track_y as f32,
                track_h as f32,
            ),
        };
        match render::resolve_editor_scrollbar_click(
            click_pos,
            thumb_pos as f32,
            (thumb_pos + thumb_len) as f32,
            track_visible,
            max_scroll,
            scroll_at_click,
        ) {
            render::EditorScrollbarClick::PageTo(new_scroll) => {
                let mut engine = self.engine.borrow_mut();
                match axis {
                    ScrollbarAxis::Horizontal => {
                        engine.set_scroll_left_for_window(win_id, new_scroll);
                    }
                    ScrollbarAxis::Vertical => {
                        engine.set_scroll_top_for_window(win_id, new_scroll);
                        engine.sync_scroll_binds();
                    }
                }
            }
            render::EditorScrollbarClick::BeginDrag { grab_offset } => {
                let drag_rc = backend.drag_state_handle();
                let widget_prefix = match axis {
                    ScrollbarAxis::Horizontal => "editor:h_sb",
                    ScrollbarAxis::Vertical => "editor:v_sb",
                };
                let widget = quadraui::WidgetId::new(format!("{widget_prefix}:{}", win_id.0));
                let target = match axis {
                    ScrollbarAxis::Horizontal => quadraui::DragTarget::ScrollbarX {
                        widget,
                        track_start,
                        track_length,
                        thumb_length: thumb_len as f32,
                        max_scroll,
                        grab_offset,
                        inverted: false,
                    },
                    ScrollbarAxis::Vertical => quadraui::DragTarget::ScrollbarY {
                        widget,
                        track_start,
                        track_length,
                        thumb_length: thumb_len as f32,
                        max_scroll,
                        grab_offset,
                        inverted: false,
                    },
                };
                drag_rc.borrow_mut().begin(target);
            }
        }
        self.draw_needed.set(true);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_mouse_click_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
        width: f64,
        alt: bool,
    ) {
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

        // ── Change-review surface mouse handling (#955, shared with #525) ─
        // Same "checked before every other rung, swallows every click"
        // policy as the folder picker above — see
        // `render::route_change_review_click`'s doc comment. TUI's
        // `mouse::handle_mouse` checks the identical shared helper.
        if self.engine.borrow().change_review.is_some() {
            self.route_and_apply_change_review_click(x, y);
            return;
        }

        self.reconcile_editor_hover_modal(backend);

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
                if self.apply_context_menu_route(backend, route) {
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
                self.apply_picker_route(backend, hit);
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
        if self.route_and_apply_editor_hover_popup(backend, x, y) {
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
            let modal = backend.modal_stack_handle().borrow().clone();
            let mut drag = backend.drag_state_handle().borrow().clone();
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
            *backend.drag_state_handle().borrow_mut() = drag;
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
                    terminal_max_rows: self.terminal_maximize_target_rows(&self.engine.borrow()),
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

                // ── H/V scrollbar hit-test (before divider) — #1026/#987/#1493 ──
                // If the click lands on either editor scrollbar:
                //   - on the thumb → start a DragTarget::ScrollbarX/Y drag.
                //   - on the empty track → page-jump toward the click.
                // Either way, consume the click *before* the divider hit-test
                // below gets a look. Without a vertical rung here,
                // `handle_mouse_click_msg` had no vertical-scrollbar hit-test
                // at all, so a click on a window's own scrollbar column (the
                // last `cell_width` before its edge, painted since
                // quadraui#968) fell straight through to `route_divider_grab`
                // — inert on any window, and silently resizing the split for
                // any window whose scrollbar-adjacent side happened to sit
                // inside the divider's own grab margin.
                //
                // Horizontal tried first (matches the pre-#1493 ordering);
                // shared axis-parameterised `App::editor_scrollbar_press`
                // (#1493) — was two hand-rolled ~85-line copies here, one per
                // axis, which had already drifted: the horizontal copy
                // rebuilt window rects via `compute_editor_window_rects`'s
                // `width`/`height` recompute, which always assumes the
                // editor area starts at `x = 0`, while the vertical copy had
                // already been fixed (this rung's own prior comment) to read
                // `self.painted_editor_bounds()` instead — the real left
                // edge, `AppShellLayout::main_content_bounds.x`, once the
                // activity bar/sidebar reserves real width. Both axes now go
                // through the same method, so that offset can never
                // re-diverge per axis again.
                if self.editor_scrollbar_press(backend, x, y, ScrollbarAxis::Horizontal) {
                    return;
                }
                if self.editor_scrollbar_press(backend, x, y, ScrollbarAxis::Vertical) {
                    return;
                }

                // ── Divider hit-test (#753 shared rung) ───────────────────────
                // Editor-group boundaries then `:split`/`:vsplit` boundaries
                // (#582), sequenced by `render::route_divider_grab`. This
                // backend's only contribution is its own painted geometry and
                // its own grab margin — `self.units.divider_metrics`, a
                // symmetric 6px around the thin drawn line on GTK
                // (`quantize: false`) vs TUI's cell metrics (`quantize:
                // true`, `group_horizontal` reaching across the tab bar's
                // row count) — the ordering does not differ.
                if let Some((group_dividers, window_dividers, on_tab_bar)) =
                    self.painted_divider_geometry(x, y)
                {
                    let breadcrumbs = self.engine.borrow().settings.breadcrumbs;
                    if let Some(grab) = render::route_divider_grab(
                        &render::DividerState {
                            group_dividers: &group_dividers,
                            window_dividers: &window_dividers,
                            metrics: (self.units.divider_metrics)(breadcrumbs),
                            on_tab_bar,
                        },
                        x,
                        y,
                    ) {
                        self.divider_grab = Some(grab);
                        return;
                    }
                }

                // ── Editor-tab-hosted plugin view click (#1627, #1631) ─────
                //
                // A `vimcode.ui.register_view` view opened as an editor-area
                // tab (`Engine::open_plugin_view_tab`) paints either a
                // `Form` or a body-kind primitive (`List`/`Tree`/`Table`/
                // `TextView`), not buffer text (`App::paint_editor_windows_
                // rung`) — a click inside its window rect has no
                // buffer-click meaning for `click::handle_mouse_click` to
                // resolve (no cursor to place, no file to reveal), so it's
                // routed through `render::route_plugin_view_body_event`
                // first (the same body-kind router the sidebar's `ExtPanel`
                // arm uses) and, when that reports the view is a
                // field-stack `Form` instead (`None`), through
                // `render::handle_plugin_view_tab_ui_event` — before falling
                // into the generic buffer-click block below.
                // `Self::plugin_view_tab_hit` walks the same `screen.windows`
                // rects `click.rs` itself resolves clicks against, so the
                // two can't disagree about which window a click landed in.
                if let Some((name, rect)) = self.plugin_view_tab_hit(x, y) {
                    let mut engine = self.engine.borrow_mut();
                    engine.clear_sidebar_focus();
                    let event = quadraui::UiEvent::MouseDown {
                        widget: None,
                        button: quadraui::MouseButton::Left,
                        position: quadraui::Point::new(x as f32, y as f32),
                        modifiers: quadraui::Modifiers::default(),
                    };
                    let backend_rc = self.backend.clone();
                    let mut b = backend_rc.borrow_mut();
                    let consumed = render::route_plugin_view_body_event(
                        &mut engine,
                        &name,
                        PluginViewHost::Tab,
                        &event,
                        rect,
                        &mut **b,
                    );
                    drop(b);
                    if consumed.is_none() {
                        render::handle_plugin_view_tab_ui_event(&mut engine, &name, &event, rect);
                    }
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }

                {
                    let mut engine = self.engine.borrow_mut();

                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    let (click_result, engine_action) = {
                        let layout_ref = self.cached_screen_layout.borrow();
                        if let Some(ref layout) = *layout_ref {
                            let drag_rc = backend.drag_state_handle();
                            let mut drag = drag_rc.borrow_mut();
                            handle_mouse_click(
                                &mut engine,
                                backend,
                                x,
                                y,
                                alt,
                                self.cached_line_height,
                                self.cached_char_width,
                                layout,
                                &self.cached_group_tab_bar_layouts.borrow(),
                                self.cached_frame_hit_map.borrow().as_ref(),
                                &self.cached_tab_bar_zones.borrow(),
                                &mut drag,
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
                            // #1421: `width` is this handler's own live panel
                            // width — prefer it over the cached one.
                            let cols = self.terminal_panel_cols(width);
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
    ///
    /// #1250: the `StatusBand` assembly itself — separated line, then each
    /// window's own line, then the global bar — is [`render::status_bands`],
    /// shared with TUI's `mouse::route_and_apply_chrome_click` now that TUI
    /// caches its own paint-time layouts the same way this backend always
    /// has, rather than each backend walking `screen.windows` and rebuilding
    /// the band rects independently.
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
        let global_status_zones = self.global_status_zones.borrow();

        // #1250: the separated/per-window/global assembly (in that
        // arbitration order — see `render::status_bands`'s own doc comment
        // for why) is now the one shared builder both backends call, rather
        // than this loop and TUI's near-identical twin in
        // `mouse::route_and_apply_chrome_click`.
        let global_rect = engine.global_status_rect.get();
        let bands = render::status_bands(
            &screen.windows,
            lh,
            &segment_map,
            self.separated_status_bar_rect
                .get()
                .map(|rect| (rect, screen.active_window_id)),
            (global_rect.width > 0.0 && global_rect.height > 0.0)
                .then(|| (global_rect, &*global_status_zones)),
        );

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
                        self.units.hit_tolerance,
                        self.units.hit_tolerance,
                        (self.units.divider_metrics)(engine.settings.breadcrumbs).quantize,
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
        drop(global_status_zones);
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
                let cols = self.terminal_panel_cols(self.painted_editor_content_width());
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
    /// `self.units.picker`'s own sizing, whereas both callers here pass the
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
            ..(self.units.picker)(1.0)
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
    fn handle_mouse_drag_msg(
        &mut self,
        backend: &dyn quadraui::Backend,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) {
        // Keep the picker's modal-stack entry fresh before anything hit-tests
        // the stack: the popup's size depends on `has_preview`, which can change
        // mid-picker.
        let picker_open = self.engine.borrow().picker_open;
        {
            let picker_id = quadraui::WidgetId::new("picker");
            let stack_rc = backend.modal_stack_handle();
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
        let drag_rc = backend.drag_state_handle();
        let stack_rc = backend.modal_stack_handle();
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
                // GTK has no canvas sidebar separator: it's a `gtk::Paned`.
                // Explorer drag-and-drop *is* shared now (#1429) — see
                // `explorer_dnd_active` below; this used to read `false`
                // unconditionally, the App-side gap that issue closes.
                //
                // Command-line selection *is* shared now (#816):
                // `engine.cmd_dragging` is armed by `handle_mouse_click_msg`'s
                // press rung the same way TUI's `mouse::handle_mouse` arms its
                // local `cmd_dragging` — quadraui#705's `CommandLineLayout`
                // closed the "no character hit test" gap the old comment here
                // recorded.
                sidebar_resizing: false,
                explorer_dnd_active: self.explorer_drag_src.is_some()
                    || self.explorer_drag_active.is_some(),
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
                        &(self.units.picker)(lh as f32),
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
                    // #1429: same `units.hover_popup_pad` the press rung
                    // (`route_and_apply_editor_hover_popup`) now reads —
                    // was hardcoded `4.0`/`4.0` here too, which is why a
                    // TUI-native `App` (`tui` harness arm) picked a
                    // different content cell mid-drag than the press had
                    // already landed on.
                    let (pad_x, pad_y) = self.units.hover_popup_pad;
                    let lh = self.cached_line_height.max(1.0);
                    let scroll = self
                        .engine
                        .borrow()
                        .editor_hover
                        .as_ref()
                        .map(|h| h.scroll_top)
                        .unwrap_or(0);
                    let rel_x = x - px - pad_x as f64;
                    let rel_y = y - py - pad_y as f64;
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
                        if let Some(source) = self.tab_drag.source() {
                            let ctx = self.cached_drop_ctx.borrow();
                            let zone =
                                render::resolve_tab_drop_zone(&ctx, source, x as f32, y as f32);
                            drop(ctx);
                            self.tab_drag.track(zone);
                        }
                    }
                    render::TabDragMove::Crossed { press_x, press_y } => {
                        // Unlike TUI, this backend's arm fires for the whole
                        // tab-bar band, so the press has to be re-resolved to
                        // confirm it was on a tab. If it was not, disarm and
                        // re-route the same event with the machine idle — the
                        // one rung that can decline after being asked.
                        if let Some(source) = self.tab_drag_source_at(backend, press_x, press_y) {
                            self.tab_drag.begin(source, x, y);
                        } else {
                            self.tab_drag.disarm();
                            self.draw_needed.set(true);
                            self.handle_mouse_drag_msg(backend, x, y, width, height);
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
                    let global_status_rows =
                        if render::global_status_bar_visible(&self.engine.borrow()) {
                            1.0
                        } else {
                            0.0
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
                // #1187: a real minimap drag now arms a `DragTarget::ScrollbarY`
                // on press (`click::pixel_to_click_target`'s minimap rung), so a
                // following move routes to `MouseDragRoute::ArmedTarget` above,
                // never here — re-running `apply_minimap_click` (an absolute
                // seek against the strip's own scroll-following window) on
                // every move was the crawl bug this issue fixes. See
                // `MouseDragRoute::Minimap`'s doc comment for when this arm can
                // still be reached at all.
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
                    let drag_rc = backend.drag_state_handle();
                    handle_mouse_drag(
                        &mut engine,
                        backend,
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
                        &self.cached_group_tab_bar_layouts.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                        &mut drag_rc.borrow_mut(),
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
            render::MouseDragRoute::ExplorerDnd => {
                // #1429: shared with TUI's `mouse::handle_mouse` — the row
                // under the pointer is resolved against `explorer_tree_rect`
                // (the rect this frame's `paint_sidebar_panel_rung` painted
                // the tree into) and the row's own pixel/cell pitch
                // (`units.explorer_row_h`, GTK: `quadraui::gtk::tree`'s
                // `item_height = (line_height * 1.4).round()`; TUI: one
                // whole cell) applied to `cached_explorer_metrics`'s
                // paint-time line-height (#540's re-apply, same value the
                // press rung already uses). Plain `cached_explorer_metrics`
                // alone under-counts every row past the first on GTK — its
                // row pitch is *not* the bare line height.
                let rect = self.engine.borrow().explorer_tree_rect.get();
                let row_height = (self.units.explorer_row_h)(self.cached_explorer_metrics.get().0);
                render::apply_explorer_drag_move(
                    &self.engine.borrow(),
                    rect,
                    row_height,
                    x,
                    y,
                    &mut self.explorer_drag_src,
                    &mut self.explorer_drag_active,
                );
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
    fn handle_mouse_up_msg(&mut self, backend: &dyn quadraui::Backend, width: f64) {
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
            let drag_rc = backend.drag_state_handle();
            let mut drag = drag_rc.borrow_mut();
            if drag.is_active() {
                let stack_rc = backend.modal_stack_handle();
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
        // Explorer drag-and-drop: execute the move on release (#1429 —
        // shared with TUI's identical `mouse.rs` release arm). Also clears
        // `sidebar_pointer_captured` — `try_route_sidebar_mouse_event` left
        // it set (it stopped resetting it once a DnD gesture bypassed that
        // function to reach here) and a future unrelated press must not
        // start out already "dragging".
        if self.explorer_drag_src.take().is_some() || self.explorer_drag_active.is_some() {
            self.sidebar_pointer_captured.set(false);
        }
        if let Some((src_row, target_row)) = self.explorer_drag_active.take() {
            render::apply_explorer_drop(&mut self.engine.borrow_mut(), src_row, target_row);
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
            // #1421: `width` is this handler's own live panel width.
            let cols = self.terminal_panel_cols(width);
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
            // Use the actual editor content width so the PTY matches the visible panel.
            let cols = self.terminal_panel_cols(self.painted_editor_content_width());
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
            terminal_cols: self.terminal_panel_cols(self.painted_editor_content_width()),
            terminal_max_rows: self.terminal_maximize_target_rows(&self.engine.borrow()),
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
    pub(crate) fn sync_sidebar_widgets(&mut self) {
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

    /// Resolve a press against the Explorer header's view-actions toolbar
    /// row (#1693) — mirrors `Self::route_debug_sidebar_event`'s
    /// chrome-band check for the Debug sidebar's own title/action bars.
    /// `render::explorer_toolbar_hit_at` does the engine-only hit-test
    /// (button index); this wrapper adds the one thing it can't do without
    /// a `Backend` handle — converting `pos` to the character-cell
    /// coordinates `Engine::open_explorer_overflow_menu` needs to anchor
    /// the "..." popup, using `self.cached_explorer_metrics` (the same
    /// paint-time metrics `explorer_ui_event` re-applies for its own
    /// hit-test, #540).
    ///
    /// Returns `false` (and does nothing) when `pos` doesn't land on a
    /// toolbar segment, so the caller falls through to the tree's own
    /// click routing.
    fn route_explorer_toolbar_click(&mut self, pos: quadraui::Point) -> bool {
        let idx = {
            let engine = self.engine.borrow();
            render::explorer_toolbar_hit_at(&engine, pos)
        };
        let Some(idx) = idx else {
            return false;
        };
        let mut engine = self.engine.borrow_mut();
        if idx == 4 {
            let (line_height, char_width) = self.cached_explorer_metrics.get();
            let col = (pos.x as f64 / char_width.max(1.0)) as u16;
            let row = (pos.y as f64 / line_height.max(1.0)) as u16;
            engine.open_explorer_overflow_menu(col, row, 1.0);
        } else {
            engine.explorer_activate_toolbar_action(idx);
        }
        // "Refresh" (idx 2) sets this immediately rather than waiting for
        // the next idle poll tick — same immediacy
        // `apply_context_menu_route`/`dispatch_context_menu_key` already
        // give a context-menu-driven explorer action.
        let needs_refresh = engine.explorer_needs_refresh;
        if needs_refresh {
            engine.explorer_needs_refresh = false;
        }
        drop(engine);
        if needs_refresh {
            self.refresh_file_tree();
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
        true
    }

    /// `UiEvent` (scroll, mouse) over the explorer panel — routed through
    /// `TreeController::handle` for scrollbar interaction.
    /// Sidebar routing for the Explorer panel (#540/#754).
    ///
    /// The `TreeController` widget dispatch itself — populate, re-apply the
    /// paint-time metrics, `handle()`, resolve a `ContextMenuRequested` —
    /// is [`render::route_explorer_tree_event`], shared with TUI's
    /// the pre-#1434 TUI shell's `handle_mouse_event` explorer intercept. What stays here
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
        quadraui::Backend::set_current_line_height(&mut **b, metrics.0 as f32);
        quadraui::Backend::set_current_char_width(&mut **b, metrics.1 as f32);
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
        // #1429: shared empty-space right-click fallback, mirroring
        // `tui_main::mouse`'s right-click arm — see
        // `route_tree_empty_space_context_menu`'s doc for the upstream
        // quadraui#1045 gap this stands in for and the deletion plan.
        if let quadraui::UiEvent::MouseDown {
            button: quadraui::MouseButton::Right,
            position,
            ..
        } = ev
        {
            render::route_tree_empty_space_context_menu(
                &mut self.engine.borrow_mut(),
                rect,
                metrics,
                position,
            );
        }
        if matches!(ev, quadraui::UiEvent::DoubleClick { .. }) {
            self.engine
                .borrow_mut()
                .dispatch_explorer_tree_event(tree_event);
        } else if matches!(ev, quadraui::UiEvent::MouseDown { .. }) {
            // #1429: record a potential drag-and-drop source — mirrors
            // TUI's identical arm in `mouse::handle_mouse` — only a genuine
            // row selection (not a chevron toggle or a scrollbar drag) arms
            // one.
            if let quadraui::TreeControllerEvent::RowSelected { ref path } = tree_event {
                if let Some(&row_idx) = path.first() {
                    self.explorer_drag_src = Some(row_idx as usize);
                }
            }
            self.engine
                .borrow_mut()
                .handle_explorer_mouse_event(tree_event);
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// Drop the server-side WM titlebar in favour of the drawn CSD row from
    /// `render_content`, the first time each run `Backend::window()` returns
    /// `Some` while this backend draws its own chrome. Called from both
    /// `setup()` (fast path, usually too early — the runner hasn't called
    /// `window.present()` yet, so `backend.window()` is still `None`) and
    /// `tick()` (reliable path — retried every frame via `csd_applied` until
    /// it succeeds). (#552)
    ///
    /// Gated on `!backend.backend_caps().native_menu` rather than
    /// `#[cfg(feature = "gui")]`: a backend with a real OS menu bar (macOS's
    /// `MacBackend`, #901) keeps its native titlebar too — the drawn CSD row
    /// only exists on backends that also draw their own menu bar (GTK,
    /// Win-GUI) — so this is the same portable capability check `setup()`
    /// already uses a few lines below to decide whether to install the
    /// drawn menu bar at all, not a second per-backend fork of the same
    /// decision.
    ///
    /// #1234 deleted this method's `gtk4::Window::list_toplevels()`
    /// discovery scan (the former `find_visible_window`) along with the
    /// `PlatformWindowHandle` seam it fed: `Backend::window()`
    /// (quadraui#950) is now backed on every windowed backend, and
    /// `GtkBackend` already tracks its own top-level window handle
    /// internally the moment the runner constructs it, so `app.rs` never had
    /// a genuine discovery gap here — only a missing portable accessor.
    fn capture_window_and_apply_csd(&mut self, backend: &mut dyn quadraui::Backend) {
        if self.csd_applied.get() || backend.backend_caps().native_menu {
            return;
        }
        if let Some(w) = backend.window() {
            if w.set_decorated(false).is_ok() {
                self.csd_applied.set(true);
            }
        }
    }

    /// Restore the window's saved size, position and maximized state
    /// (#1529), the first time each run `Backend::window()` returns `Some`.
    /// Called from both `setup()` (fast path, usually too early — the
    /// runner hasn't called `window.present()` yet, so `backend.window()`
    /// is still `None`) and `tick()` (reliable path — retried every frame
    /// via `window_geometry_restored` until it succeeds), mirroring
    /// `capture_window_and_apply_csd`'s own identical two-call-site shape
    /// and doc (#552).
    ///
    /// Order matters: size and (clamped) position are applied first, then
    /// maximized state last — `Backend::toggle_window_maximize` is a
    /// *toggle*, not a setter (`WindowControl` has no `maximize()`; see
    /// that trait's own doc), so it only flips from "restored" to
    /// "maximized" correctly if the restored geometry is already in place
    /// underneath it, the same way GTK's own `restore()` falls back to the
    /// pre-maximize size/position rather than a hardcoded one.
    ///
    /// `saved.x`/`y` are clamped against the live display list
    /// (`WindowGeometry::clamp_to_displays`) before use, so a monitor that
    /// was unplugged since the position was saved can never strand the
    /// restored window off-screen; an unclamped position (or no displays
    /// at all) falls back to `None`, leaving placement to the OS/window
    /// manager default. `set_size`/`set_bounds` failing (e.g. GTK's
    /// structural inability to reposition at all — see
    /// `WindowControl::set_bounds`'s own doc) is not an error here — every
    /// call is best-effort, exactly like `capture_window_and_apply_csd`'s
    /// own `set_decorated` call.
    fn restore_window_geometry(&mut self, backend: &mut dyn quadraui::Backend) {
        if self.window_geometry_restored.get() {
            return;
        }
        let saved = self.engine.borrow().session.window.clone();
        let displays = backend.services().displays().unwrap_or_default();
        let clamped = saved.clamp_to_displays(&displays);
        let already_maximized = {
            let Some(w) = backend.window() else {
                return;
            };
            let _ = w.set_size(clamped.width as f32, clamped.height as f32);
            if let Some((x, y)) = clamped.x.zip(clamped.y) {
                let _ = w.set_bounds(quadraui::Rect::new(
                    x as f32,
                    y as f32,
                    clamped.width as f32,
                    clamped.height as f32,
                ));
            }
            matches!(w.is_maximized(), Ok(true))
        };
        self.window_geometry_restored.set(true);
        if clamped.maximized && !already_maximized {
            backend.toggle_window_maximize();
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
        backend: &dyn quadraui::Backend,
        event: &quadraui::UiEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) -> bool {
        use quadraui::UiEvent;

        let Some(sb) = ctx.layout.sidebar_content_bounds else {
            self.sidebar_pointer_captured.set(false);
            return false;
        };
        let dragging = self.sidebar_pointer_captured.get();
        // #1429: once an explorer row has been picked up (`explorer_drag_src`)
        // or the drag is already active (`explorer_drag_active`), the *move*
        // and *release* that follow the initial press must reach
        // `handle_mouse_drag_msg`/`handle_mouse_up_msg` — the
        // `MouseDragRoute::ExplorerDnd` rung and `render::apply_explorer_drop`
        // — not loop back through here into `explorer_ui_event`, which would
        // hand a plain `MouseMoved` to `TreeController::handle` and get its
        // *scrollbar*-drag `drag_to`, never the row-under-pointer tracking a
        // DnD gesture needs. The press itself still claims capture as usual
        // (`explorer_ui_event` arms `explorer_drag_src` from that same press).
        let explorer_dnd_active =
            self.explorer_drag_src.is_some() || self.explorer_drag_active.is_some();
        let pos = match event {
            UiEvent::MouseDown { position, .. }
            | UiEvent::DoubleClick { position, .. }
            | UiEvent::Scroll { position, .. } => *position,
            // Follow-through only: never *start* an interaction from a move or
            // a release (see the doc comment above).
            UiEvent::MouseUp { position, .. } if dragging && !explorer_dnd_active => {
                self.sidebar_pointer_captured.set(false);
                *position
            }
            UiEvent::MouseMoved {
                position,
                buttons: quadraui::ButtonMask { left: true, .. },
            } if dragging && !explorer_dnd_active => *position,
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

        // #955 (ACP-4, review fix): same reasoning as the picker above, for
        // the change-review surface — full-viewport, so its diff pane sits
        // squarely on top of the sidebar body underneath. Without this, a
        // click on the diff's left pane (which happens to fall inside
        // `sidebar_content_bounds`, since the surface deliberately doesn't
        // resize/hide the sidebar it's painted over) drove the sidebar's own
        // `TreeController`/panel row hit-test instead of ever reaching
        // `handle_mouse_click_msg`'s change-review block further down —
        // `route_and_apply_change_review_click` was correct but unreachable
        // for any click whose *coordinates* happened to land in that band.
        if self.engine.borrow().change_review.is_some() {
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
                self.dispatch_context_menu_click(backend, pos.x as f64, pos.y as f64);
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
                // #1693: the view-actions toolbar row sits above the tree
                // (`paint_sidebar_panel_rung`'s `PANEL_EXPLORER` arm), so a
                // press is checked against it first — mirrors
                // `Self::route_debug_sidebar_event`'s chrome-band check.
                // Only a genuine press/release (`starts_interaction`) can
                // open the overflow menu or run an action; a drag
                // follow-through always falls to the tree.
                if !starts_interaction || !self.route_explorer_toolbar_click(pos) {
                    self.explorer_ui_event(event.clone());
                }
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
                //
                // #1343: `engine.settings_form_rect`, not the raw `sb` —
                // `paint_sidebar_panel_rung`'s `PANEL_SETTINGS` arm now
                // reserves one row above the form for the shared search row
                // (`SidebarPanelChrome::Search`, quadraui#1061, via
                // `render::search_only_chrome`, #1391), so `sb` (the whole
                // sidebar content rect the shell hands this frame) is one
                // row taller than what `FormController::render_and_cache`
                // actually painted into. `handle_settings_form_ui_event`'s
                // own doc says `rect` must be "the *same* rect the last
                // frame passed to `FormController::render_and_cache`" —
                // `sb` stopped being that the moment the search row was
                // added.
                let settings_rect = engine.settings_form_rect.get();
                render::handle_settings_form_ui_event(&mut engine, event, settings_rect);
                true
            }
            render::SidebarOwner::ExtPanel(_) => {
                // #1089: `render_content` now paints this panel through
                // `render::ext_panel_to_tree_view` — the same adapter TUI
                // uses — not `ext_sidebar_system` (the extension
                // marketplace), so routing goes through the shared
                // `render::route_ext_panel_click` router instead, built on
                // the `Backend::tree_layout` cached at paint time
                // (`Engine::ext_panel_tree_layout`) rather than a
                // hand-rolled row formula (this backend pitches a tree's
                // header rows shorter than its item rows — see that cache
                // field's own doc).
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_panel_has_focus = true;
                }
                // #146: a view-backed panel painted a `quadraui::Form`, so its
                // clicks resolve through `FormController` (and dispatch to the
                // plugin's `on_event`), not through the tree-row router.
                let is_view = engine
                    .ext_panel_active
                    .as_deref()
                    .is_some_and(|n| engine.is_plugin_view(n));
                // `handle_plugin_view_ui_event`'s own `bool` return is ignored
                // for the same reason the Settings arm ignores it: the position
                // is already known to be inside the sidebar, so even a click on
                // empty panel padding belongs here rather than leaking to the
                // editor underneath.
                match event {
                    // #1631: a `ViewBody`-kind view routes through its own
                    // matching primitive's click/scroll resolution instead
                    // of `FormController` — `render::route_plugin_view_body_
                    // event`, shared with the tab-hosted arm below. See the
                    // paint arm (`paint_sidebar_panel_rung`'s `ext:` case)
                    // for the matching kind branch.
                    _ if is_view => {
                        let name = engine.ext_panel_active.clone().unwrap_or_default();
                        let rect = engine.plugin_view_form_rect.get();
                        let backend_rc = self.backend.clone();
                        let mut b = backend_rc.borrow_mut();
                        let consumed = render::route_plugin_view_body_event(
                            &mut engine,
                            &name,
                            PluginViewHost::Sidebar,
                            event,
                            rect,
                            &mut **b,
                        );
                        drop(b);
                        if consumed.is_none() {
                            render::handle_plugin_view_ui_event(&mut engine, event, rect);
                        }
                    }
                    UiEvent::Scroll { delta, .. } => {
                        let flat_len = engine.ext_panel_flat_len();
                        let step = (delta.y.abs() * 3.0).round().max(1.0) as usize;
                        if delta.y > 0.0 {
                            // Positive y = up toward the top of the content
                            // (quadraui's convention for an already-resolved
                            // `UiEvent::Scroll`; see `handle_mouse_scroll_msg`'s
                            // #554 comment on the raw-vs-quadraui polarity
                            // split this event predates).
                            engine.ext_panel_scroll_top =
                                engine.ext_panel_scroll_top.saturating_sub(step);
                        } else {
                            engine.ext_panel_scroll_top = (engine.ext_panel_scroll_top + step)
                                .min(flat_len.saturating_sub(1));
                        }
                    }
                    UiEvent::DoubleClick { .. } => {
                        render::route_ext_panel_click(&mut engine, pos, true);
                    }
                    UiEvent::MouseDown {
                        button: quadraui::MouseButton::Left,
                        ..
                    } => {
                        render::route_ext_panel_click(&mut engine, pos, false);
                    }
                    // Right-click context menu and mid-drag follow-through
                    // (`MouseUp`/`MouseMoved`) aren't wired for this panel yet
                    // — out of #1089's scope (paint + left-click selection);
                    // still consumed below so neither leaks through to the
                    // editor underneath.
                    _ => {}
                }
                true
            }
            render::SidebarOwner::Ai => self.route_ai_sidebar_event(event, pos, starts_interaction),
            render::SidebarOwner::Board => {
                self.route_board_sidebar_event(event, pos, starts_interaction)
            }
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
    /// `render_content` paints the title + action-button chrome through
    /// `SidebarPanelBody::render_with`'s `StatusBars` variant and stashes its
    /// `status_bar_hit_regions` in `engine.dap_sidebar_action_hits`, already
    /// in `pos`'s own absolute pixel space — no per-backend translation step
    /// (#1392; before quadraui#1061 the hits were bar-relative and had to be
    /// translated via a separately cached `action_rect`, two independently
    /// derived values for the same geometry). `dap_sidebar_action_click_at`
    /// takes `pos` directly. Everything below the chrome goes to the shared
    /// `SidebarSystem` at the body rect it painted into
    /// ([`render::dispatch_dap_sidebar_body_event`]) — the same two shared
    /// functions TUI calls for this panel.
    fn route_debug_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
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
            render::dap_sidebar_action_click_at(&mut engine, pos);
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

    /// Sidebar routing for the Board panel (#521, right-click added #523).
    ///
    /// `render::route_board_click` resolves the press against the
    /// `quadraui::BoardLayout` `paint_sidebar_panel_rung`'s `PANEL_BOARD`
    /// arm cached at paint time — the same "paint caches, click reads"
    /// contract as `Engine::ext_panel_tree_layout`. Consumed
    /// unconditionally like every other panel arm here, per
    /// [`Self::route_sc_sidebar_event`]'s neighbouring doc.
    ///
    /// The right-click arm mirrors `handle_tab_right_click`/
    /// `handle_editor_right_click`'s own pixel→cell conversion —
    /// `Engine::open_board_context_menu`'s `x`/`y` are cell coordinates,
    /// same convention every other `open_*_context_menu` uses, so the
    /// conversion happens here rather than inside `render.rs` (which has no
    /// notion of GTK's pixel metrics).
    fn route_board_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let mut engine = self.engine.borrow_mut();
        if starts_interaction {
            engine.board_has_focus = true;
        }
        match event {
            quadraui::UiEvent::DoubleClick { .. } => {
                render::route_board_click(&mut engine, pos, true);
            }
            quadraui::UiEvent::MouseDown {
                button: quadraui::MouseButton::Left,
                ..
            } => {
                render::route_board_click(&mut engine, pos, false);
            }
            quadraui::UiEvent::MouseDown {
                button: quadraui::MouseButton::Right,
                ..
            } => {
                if let Some(card_id) = render::board_right_click_card(&engine, pos) {
                    let cw = self.cached_char_width.max(1.0);
                    let lh = self.cached_line_height.max(1.0);
                    let cx = (pos.x as f64 / cw) as u16;
                    let cy = (pos.y as f64 / lh) as u16;
                    engine.open_board_context_menu(card_id, cx, cy);
                }
            }
            _ => {}
        }
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
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let mut engine = self.engine.borrow_mut();
        let rect = engine.ai_chat_rect.get();
        let plan_rect = engine.ai_plan_rect.get();
        if rect.width <= 0.0 && plan_rect.width <= 0.0 {
            return false;
        }
        if starts_interaction {
            engine.ai_has_focus = true;
            // #1507 review: a mouse click into the panel isn't part of any
            // keyboard `<leader>ai` gesture, so it breaks one exactly like a
            // Named key would — discard rather than replay, since a click
            // (unlike a keystroke) has no natural place in the input to
            // insert buffered text.
            engine.ai_leader_toggle_pending.clear();
            // #1513: a press landing on the pinned plan block's band
            // toggles its collapse state instead of reaching `ChatController`
            // — checked first (before the metrics re-apply/`route_ai_chat_
            // event` call below) exactly like `route_board_sidebar_event`'s
            // right-click resolves against its own cached layout ahead of
            // falling through to generic nav.
            if plan_rect.width > 0.0
                && plan_rect.height > 0.0
                && pos.y >= plan_rect.y
                && pos.y < plan_rect.y + plan_rect.height
            {
                render::route_ai_plan_band_click(&mut engine, pos);
                return true;
            }
        }
        if rect.width <= 0.0 {
            return true;
        }
        let theme = render::Theme::from_name(&engine.settings.colorscheme);
        let backend_rc = self.backend.clone();
        // Re-apply the metrics `render()` painted the panel with — see
        // `cached_ai_chat_metrics`'s doc for why this can't be skipped.
        let metrics = self.cached_ai_chat_metrics.get();
        {
            let mut b = backend_rc.borrow_mut();
            quadraui::Backend::set_current_line_height(&mut **b, metrics.0 as f32);
            quadraui::Backend::set_current_char_width(&mut **b, metrics.1 as f32);
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
        match render::activity_bar_key_action(key_name, ctrl) {
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

    /// #734 slice 1: the single sink for the shared context-menu key rung
    /// (`render::ModalKeyRoute::ContextMenu`) — on every backend `App` hosts
    /// now (#1433 flips TUI onto this same method; it was written when
    /// `App` was still GTK-only, hence the "GTK-side" framing this doc used
    /// to carry).
    ///
    /// Replaces two hand-rolled copies — the block that opened
    /// `handle_key_press` and `handle_explorer_ctx_menu_key` (#426) on the
    /// explorer DA path — both of which reimplemented selection movement
    /// inline instead of calling `Engine::handle_context_menu_key`, and so
    /// disagreed with TUI on `l` (confirm), `q`/`h` (close) and disabled-item
    /// skipping. The engine owns all of that now; the only per-action part
    /// left is dispatching the confirmed action, since `new_file` /
    /// `open_terminal` / `find_in_folder` need backend plumbing —
    /// `find_in_folder` itself has no remaining backend fork to record:
    /// #1418 already converged TUI's explicit-target `"delete"`/
    /// `"move_file"` handling and GTK's mismatched `"find_in_folder"`
    /// (Search-panel-focus vs Grep-picker) onto the one shared
    /// `render::apply_explorer_context_action`, which is what this method
    /// calls below — #1433 re-checked this while auditing what became
    /// backend-shared vs stayed backend-specific once `App` started running
    /// on TUI in production, and confirmed there is nothing left to
    /// reconcile here.
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
        if let (Some(ref act), Some((ref path, is_dir))) = (action, target) {
            let engine_rc = self.engine.clone();
            // Was the `"open_terminal"` arm of the deleted
            // `App::dispatch_explorer_ctx_action`, which called the deleted
            // `App::open_terminal_at` — inlined here (rather than calling a
            // method) because that method reached for its own
            // `self.engine.borrow_mut()`, and this closure runs while
            // `engine_rc` is already borrowed mutably below; a second borrow
            // would panic (`RefCell` already mutably borrowed).
            render::apply_explorer_context_action(
                &mut engine_rc.borrow_mut(),
                act,
                path,
                is_dir,
                &mut |engine: &mut Engine, dir: std::path::PathBuf| {
                    let cols = self.terminal_panel_cols(self.painted_editor_content_width());
                    let rows = engine.session.terminal_panel_rows;
                    engine.terminal_new_tab_at(cols, rows, Some(&dir));
                    self.draw_needed.set(true);
                },
            );
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

    /// Minimize the application window (inline window-control button).
    ///
    /// Routed through `Backend::window()` (quadraui#950, #1124) rather than
    /// the old GTK-only `self.window`/`PlatformWindowHandle` seam (deleted
    /// #1234) — that seam was `None` on macOS/Win-GUI, so this used to be a
    /// silent no-op there; `WindowControl` is backed on every windowed
    /// backend.
    fn window_minimize(&mut self, backend: &mut dyn quadraui::Backend) {
        self.last_window_control_action
            .set(Some(render::WINDOW_MINIMIZE_ACTION));
        if let Some(w) = backend.window() {
            let _ = w.minimize();
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
        self.last_window_control_action
            .set(Some(render::WINDOW_MAXIMIZE_ACTION));
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
        self.last_window_control_action
            .set(Some(render::WINDOW_CLOSE_ACTION));
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

    /// #955 (ACP-4, shared with #525): the change-review surface's mouse
    /// handling, called from `handle_mouse_click_msg` — same shared
    /// `render::route_change_review_click` TUI's `mouse::handle_mouse`
    /// calls.
    fn route_and_apply_change_review_click(&mut self, x: f64, y: f64) {
        let diff_rect = self.engine.borrow().change_review_diff_rect.get();
        let view = self
            .engine
            .borrow()
            .change_review
            .as_ref()
            .and_then(|r| r.current_entry())
            .map(|e| e.view.clone());
        let Some(view) = view else {
            return;
        };
        let lh = self.cached_line_height.max(1.0) as f32;
        match render::route_change_review_click(diff_rect, &view, lh, x as f32, y as f32) {
            render::ChangeReviewClickRoute::Jump(hit) => {
                self.engine.borrow_mut().change_review_jump_to_hit(hit);
            }
            render::ChangeReviewClickRoute::Consume => {}
        }
        self.draw_needed.set(true);
    }

    /// Show a native "Save Workspace As" dialog.
    pub(crate) fn save_workspace_as_dialog(&mut self) {
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

    /// User clicked ✕ on a tab with unsaved changes — ask what to do.
    ///
    /// #823 item 4: was a byte-identical restatement of
    /// `Engine::show_close_tab_confirm` (`core/engine/panels.rs`) instead of
    /// calling it.
    fn show_close_tab_confirm(&mut self) {
        self.engine.borrow_mut().show_close_tab_confirm();
        self.draw_needed.set(true);
    }

    /// Editor content pixel width last painted by `render_content`
    /// (`cached_editor_bounds`) — the same width the bottom (terminal) panel
    /// spans, since `editor_bounds` is derived from `main_content_bounds`
    /// with the sidebar/activity bar already excluded (#582). Used by
    /// [`Self::terminal_panel_cols`] callers with no live pixel width of
    /// their own in scope (accelerator/menu/tick paths) — callers that DO
    /// have one (a click/drag handler's own `width` parameter) should pass
    /// that same-frame value to `terminal_panel_cols` directly instead, for
    /// the #1058 reason recorded on `terminal_panel_cols` itself.
    ///
    /// Falls back to the cached window width before the first frame has
    /// painted (`cached_editor_bounds` is still `None`).
    ///
    /// #1421: replaces the old hardcoded `terminal_cols() -> 80`.
    pub(crate) fn painted_editor_content_width(&self) -> f64 {
        self.cached_editor_bounds
            .get()
            .map(|(r, _)| r.width)
            .unwrap_or_else(|| self.cached_window_width.get() as f64)
    }

    /// Terminal panel pixel width reserved for the panel's own vertical
    /// scrollbar — the same strip `render_content` paints one into and
    /// `MouseDragRoute::TerminalSplitDivider` already clamps the divider
    /// drag against (`handle_mouse_drag_msg`).
    const TERMINAL_PANEL_SB_W: f64 = 6.0;

    /// Convert a *live* terminal-panel pixel width to a column count using
    /// the last-painted char advance (`cached_char_width`). #1058: this used
    /// to be a fallback of a hardcoded `terminal_cols() -> 80` (and, for the
    /// terminal-split finalize path specifically, a separate `da_w = 800.0`
    /// guess) rather than a real conversion of the live width, so any window
    /// that wasn't exactly 800px wide split the terminal into the wrong
    /// column counts. `cached_char_width` is seeded to a positive default in
    /// `App::new` and only ever grows from a real paint, so clamping it to a
    /// `1.0` floor (rather than branching on a `<= 0.0` fallback) is enough
    /// to avoid a divide-by-zero without a second hardcoded column count.
    pub(crate) fn terminal_panel_cols(&self, width: f64) -> u16 {
        let cw = self.cached_char_width.max(1.0);
        ((width - Self::TERMINAL_PANEL_SB_W).max(0.0) / cw) as u16
    }

    /// Terminal-maximize target row count for the next
    /// `terminal.toggle_maximize` dispatch — the exact
    /// `render::compute_editor_layout` call `render_content` makes every
    /// frame, replayed here against the content height/line height last
    /// painted (`cached_main_content_height`, `cached_line_height`) so
    /// accelerator/menu paths — which have no live pixel height of their own
    /// in scope — agree with what was actually painted. Same reasoning as
    /// [`Self::painted_editor_content_width`]'s fallback.
    ///
    /// #1421: replaces the old hardcoded `terminal_target_maximize_rows() ->
    /// 10`. Mirrors the TUI equivalent,
    /// `tui_main::terminal_target_maximize_rows_tui`.
    pub(crate) fn terminal_maximize_target_rows(&self, engine: &Engine) -> u16 {
        let h = self.cached_main_content_height.get();
        let lh = self.cached_line_height.max(1.0);
        render::compute_editor_layout(engine, h, lh, false).terminal_max_target_rows
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
                // `app_icon_image_for_paint` (this file) — see its doc
                // comment for why every backend can now share the same
                // [`crate::render::app_icon_image`] builder here (#1102).
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
            // #1234: read live via `WindowControl::is_maximized()` rather
            // than the deleted `self.window`/`PlatformWindowHandle` seam —
            // `backend` is already in hand here, so there is no "no backend
            // in scope" problem to cache around (contrast
            // `App::cached_window_width`'s doc).
            let maximized = backend
                .window()
                .and_then(|w| w.is_maximized().ok())
                .unwrap_or(false);
            let controls_bar = render::window_controls_status_bar(theme, maximized);
            let interaction = self.title_bar_interaction.borrow();
            let hits = backend.draw_status_bar_interactive(
                controls_rect,
                &controls_bar,
                &quadraui::InteractionState::from_parts(
                    interaction.hovered_id().cloned(),
                    interaction.pressed_id().cloned(),
                ),
            );
            interaction.set_layout(hits);
        }
    }
}

/// #1745: on the native macOS GUI, a real Cmd keypress reaches this crate as
/// `quadraui::Modifiers::cmd`, a bit `engine_key_from_ui`/`handle_key_press`/
/// every other `modifiers.ctrl` check in this file has never read — see
/// `tests/vscode_keybinding_parity.rs`'s corrected `macos_gui` column. So
/// before this fix Cmd+C/V/X/Z/S/P/… didn't merely *diverge* from VS Code's
/// Mac defaults, they did **nothing at all**: `ctrl` stayed `false` and the
/// plain, unmodified character fell through to whatever Insert-mode typing
/// does with it.
///
/// Folds `cmd` into `ctrl` for a `KeyPressed` event, but only when the live
/// backend's own [`quadraui::PlatformServices::platform_name`] — a runtime
/// capability query, not a `cfg!(target_os = "macos")` guess — reports
/// `"macos"`. That is true only for `quadraui::macos::MacBackend`: GTK's own
/// Cmd-reporting convention (Super/Meta -> `cmd`, quadraui's
/// `gtk/events.rs`) is left completely alone, so a GNOME user's Super key
/// does not suddenly start acting like Ctrl. This is the one shared
/// dispatcher both GTK and the macOS GUI already run through (`App::handle`
/// -> `handle_dispatch`), so the fix lives here once rather than in any
/// backend-specific file — see this file's own module doc and CLAUDE.md's
/// Platform-Neutrality Rule.
///
/// Mouse events are deliberately left untouched: the issue this fixes
/// (#1745) is scoped to VS Code mode's keyboard chords, and folding `cmd`
/// into the `MouseButton::Left if modifiers.ctrl` go-to-definition chord
/// would be a second, separate behaviour change with no test coverage here.
///
/// ## Arrow keys are handled separately, and only in VS Code mode
///
/// VS Code's real Mac defaults do **not** treat Cmd+Arrow as a plain
/// Ctrl-to-Cmd substitution the way every letter/symbol chord above does:
/// Option (`alt`) is the word-wise-navigation modifier on Mac
/// (`cursorWordLeft`/`cursorWordRight`, vimcode's existing Ctrl+Left/Right),
/// while Cmd+Left/Right is line start/end (`cursorHome`/`cursorEnd`, plain
/// `Home`/`End`) and Cmd+Up/Down is document start/end
/// (`cursorTop`/`cursorBottom`, Ctrl+Home/Ctrl+End) — see
/// tests/vscode_keybinding_parity.rs's `cursorWordEndRight / cursorWordLeft` and
/// `cursorTop / cursorBottom` rows. A blanket `cmd -> ctrl` fold would make
/// Cmd+Right *word-move* (vimcode's Ctrl+Right), which is not what either
/// VS Code or this fix wants, so arrows are excluded from the fold above
/// and translated here instead — a key-identity translation (not just a
/// modifier fold), gated on VS Code mode specifically because that's this
/// issue's scope and `route_alt_key`'s own Alt+Left/Right handling (VS
/// Code's Win/Linux `navigateBack`/`navigateForward` default) is itself
/// VS-Code-mode-gated.
///
/// `alt` is cleared on the Option+Left/Right arm so `route_alt_key` (called
/// later in `handle_dispatch`, unconditionally whenever `alt` is set) sees
/// `alt == false` and takes its own `Fallthrough` path instead of also
/// claiming the chord as `navigateBack`/`navigateForward` — Mac's own
/// default for that command is Ctrl+-/Ctrl+Shift+- instead (tracked as a
/// still-open gap; see `tests/vscode_keybinding_parity.rs`'s `KNOWN_GAPS`).
fn normalize_mac_cmd_as_ctrl(
    event: quadraui::UiEvent,
    backend: &dyn quadraui::Backend,
    vscode_mode: bool,
) -> quadraui::UiEvent {
    use quadraui::{Key, Modifiers, NamedKey, UiEvent};
    if backend.services().platform_name() != "macos" {
        return event;
    }
    match event {
        // Option+Left/Right (no Cmd, no physical Ctrl already held): word
        // move, matching vimcode's existing Ctrl+Left/Right.
        UiEvent::KeyPressed {
            key: key @ (Key::Named(NamedKey::Left) | Key::Named(NamedKey::Right)),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.alt && !modifiers.cmd && !modifiers.ctrl => {
            UiEvent::KeyPressed {
                key,
                modifiers: Modifiers {
                    ctrl: true,
                    alt: false,
                    ..modifiers
                },
                repeat,
            }
        }
        // Cmd+Left/Right (no Option): line start/end.
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Left),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Home),
            modifiers,
            repeat,
        },
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Right),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::End),
            modifiers,
            repeat,
        },
        // Cmd+Up/Down (no Option): document start/end — `Home`/`End` with
        // an extra `ctrl` bit, the same shape `engine_key_from_ui`'s
        // `NamedKey::Home`/`End` arms already use for Ctrl+Home/Ctrl+End.
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Up),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Home),
            modifiers: Modifiers {
                ctrl: true,
                ..modifiers
            },
            repeat,
        },
        UiEvent::KeyPressed {
            key: Key::Named(NamedKey::Down),
            modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd && !modifiers.alt => UiEvent::KeyPressed {
            key: Key::Named(NamedKey::End),
            modifiers: Modifiers {
                ctrl: true,
                ..modifiers
            },
            repeat,
        },
        // Every other chord: fold `cmd` into `ctrl` unchanged (the plain
        // Ctrl-to-Cmd substitution that covers every letter/symbol VS Code
        // Mac default this file's own doc enumerates). Gated on
        // `vscode_mode`, same as the arrow arms above — this issue is
        // scoped to VS Code mode on the macOS GUI (#1745), and without the
        // gate it would also change default Vim-mode behaviour on Mac:
        // Cmd+W would enter vim's window-command prefix, Cmd+V would enter
        // visual-block, Cmd+D would scroll a half page, etc. Vim mode's own
        // Mac Cmd semantics (if any are ever wanted) are a separate,
        // untested design decision and not part of this fix.
        UiEvent::KeyPressed {
            key,
            mut modifiers,
            repeat,
        } if vscode_mode && modifiers.cmd => {
            modifiers.ctrl = true;
            UiEvent::KeyPressed {
                key,
                modifiers,
                repeat,
            }
        }
        other => other,
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
        // #1745: must run before anything else reads `event`'s modifiers —
        // see `normalize_mac_cmd_as_ctrl`'s own doc. The `matches!` guard
        // is a cheap hardening (#1745 review), not a behaviour change:
        // `normalize_mac_cmd_as_ctrl`'s own `match` only has arms for
        // `UiEvent::KeyPressed`, falling through every other variant via
        // `other => other` regardless of `vscode_mode`'s value — so
        // skipping the borrow entirely for a non-`KeyPressed` event (mouse
        // moves/window events, the overwhelming majority of dispatches)
        // changes nothing it would have computed. Without the guard this
        // was an *unconditional* immutable borrow at the top of the one
        // shared dispatcher, on every event; `dispatch_engine_action`
        // holds a `borrow_mut()` across the whole `apply_engine_action`
        // call, and quadraui's macOS dialogs drive nested modal loops, so
        // any nested re-entry into `handle` that previously passed through
        // a non-engine-borrowing arm would have panicked on this borrow.
        let vscode_mode =
            matches!(event, UiEvent::KeyPressed { .. }) && self.engine.borrow().is_vscode_mode();
        let event = normalize_mac_cmd_as_ctrl(event, backend, vscode_mode);

        // ── #1427: shared menu-bar reveal/hide routing ───────────────────────
        // The #318 Alt+<letter> shim (only fires while the bar is hidden —
        // GTK's is always visible, see `ShellApp::setup`'s three-way branch)
        // and the #988/#1029 stale-hamburger-corner-click guard (only ever
        // armed by a hamburger panel click, which GTK/macOS/Win never
        // register — see `Self::shell_config`'s `cell`-profile branch). A
        // no-op on those backends; see `render::route_menu_bar_reveal`'s own
        // doc. Must run before the `MenuSystem` intercept just below, since
        // the shim's reveal has to take effect in the *same* dispatch that
        // intercept reads `menu_bar_visible` from.
        let menu_bar_reveal_reaction = {
            let mut engine = self.engine.borrow_mut();
            // Nothing in `handle_dispatch` returns before this point, so
            // consuming the one-shot guard here is equivalent to TUI's "at
            // the very top of `handle`, before any early exit" placement —
            // see `consume_hamburger_stale_click_guard`'s own doc.
            let stale_hamburger_corner_click =
                render::consume_hamburger_stale_click_guard(&mut engine, &event);
            render::route_menu_bar_reveal(
                &mut engine,
                &event,
                stale_hamburger_corner_click,
                backend,
                ctx,
            )
        };
        if let Some(reaction) = menu_bar_reveal_reaction {
            return reaction;
        }

        // ── Menu system intercept (#552) ─────────────────────────────────────
        // GTK's menu bar is always visible (see `ShellApp::setup`) and its
        // dropdown overlay must intercept keys/clicks before the sidebar or
        // editor sees them — same precedence TUI uses (mod.rs "MenuSystem
        // intercept" block) via the identical shared `menu_system.handle()`.
        //
        // #955 (ACP-4, review fix): also gated on the dropdown genuinely
        // being closed OR the change-review surface being closed. The menu
        // bar/CSD row occupies the window's full top band (`bar_rect.x`
        // starts right after the app icon, `bar_rect.width` spans the rest
        // of the window) — exactly where the change-review surface's first
        // diff rows paint, since that surface is genuinely full-viewport.
        // Without this, `menu_system.handle` treated a click on those rows
        // as landing on "File" (or whichever label happens to share that
        // band) and returned `StateChanged`/`Activated`, swallowing the
        // click into a menu open/highlight instead of ever reaching
        // `handle_mouse_click_msg`. An *already-open* dropdown still wins
        // regardless (`menu_system.borrow().is_open()`), matching every
        // other "topmost modal wins" precedent in this function — only the
        // idle bar itself yields.
        let (menu_bar_visible, menu_bar_toggleable, menu_system) = {
            let eng = self.engine.borrow();
            (
                eng.menu_bar_visible,
                eng.menu_bar_toggleable,
                eng.menu_system.clone(),
            )
        };
        let menu_open = menu_system.borrow().is_open();
        let change_review_open = self.engine.borrow().change_review.is_some();
        // #1764: a *closed* dropdown must not be opened by an Alt+<mnemonic>
        // `KeyPressed` while the engine is mid-text-entry (Insert/Replace) or
        // mid-command-line (Command/Search) — see
        // `render::alt_mnemonic_open_allowed`'s own doc for why (a bugbash
        // repro traced a real macOS pty's Escape-then-letter collapsing into
        // exactly this chord, and letting it open a menu here swallows the
        // keystroke that was supposed to be the Escape, leaving every
        // following key falling through as literal Insert-mode text).
        // `menu_open` (a dropdown that's *already* open) is untouched — this
        // only gates a fresh open.
        let alt_mnemonic_open_blocked = !menu_open
            && matches!(
                event,
                UiEvent::KeyPressed { modifiers, .. } if modifiers.alt
            )
            && !render::alt_mnemonic_open_allowed(self.engine.borrow().mode, menu_bar_toggleable);
        if !alt_mnemonic_open_blocked && (menu_open || (menu_bar_visible && !change_review_open)) {
            // `menu_items_rect`, not `menu_row_rect` (#720): the app icon
            // occupies a leading slot, so the items the last frame *painted*
            // start one slot right of the band's left edge. Hit-testing
            // against the full band would resolve a click on `File` to
            // whatever label now sits a slot to its left. `render_content`
            // writes this from the same `split_menu_row_for_app_icon` call
            // that positions the paint.
            //
            // #1427: on the toggleable-menu-bar profile, `route_menu_bar_
            // reveal`'s Alt+<letter> shim (just above) can reveal the bar in
            // this very dispatch, before any paint has refreshed
            // `menu_items_rect` — `render::menu_bar_intercept_rect` falls
            // back to a synthetic full-width row for exactly that frame; see
            // its own doc. Always `cached` unchanged on GTK/macOS/Win
            // (`menu_bar_toggleable` is never `true` there).
            let bar_rect = render::menu_bar_intercept_rect(
                menu_bar_toggleable,
                self.menu_items_rect.get(),
                backend.viewport().width,
            );
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
                // #1763: a dropdown that was genuinely open (`menu_open`,
                // not just the bar revealed) and a `KeyPressed` it doesn't
                // recognise (anything but Escape/arrows/Enter/a matching
                // Alt+<letter>, all handled above) falls all the way
                // through to `Ignored` — `quadraui::MenuSystem::handle` has
                // no type-ahead/dismiss-on-any-key behaviour of its own
                // (`quadraui/src/compose/menu_system.rs`'s own `handle`:
                // the match's final arm is a bare `_ => MenuEvent::Ignored`
                // for exactly this case). Left alone, the dropdown stays
                // `is_open()` and keeps painting every subsequent frame
                // (`render_content`'s unconditional `menu_system.render()`
                // call) while this same event keeps flowing to the Vim/
                // editor dispatch below and is applied there as normal —
                // so a key sequence that happens to open a menu (Alt+<its
                // mnemonic>, or — per #1763's own bugbash repro — a
                // terminal that collapses a fast Escape-then-letter into
                // an Alt+letter chord) leaves every following keystroke
                // editing the buffer correctly *underneath* a dropdown
                // that visually never goes away, exactly the "unexpected
                // 'Go' menu dropdown" symptom. Closing here mirrors the
                // private `handle_escape`'s own whole-menu close inside
                // `MenuSystem::handle` (same `close()` call) for the one
                // case that handler can't reach: the key wasn't Escape, so
                // `MenuSystem::handle` never ran that arm itself.
                //
                // This lives in *shared* `handle_dispatch`, not TUI-side
                // wiring, so the same dismiss-on-unrecognised-key behaviour
                // lands on GTK and macOS too, where `menu_bar_visible` is
                // always `true` and `menu_open` is reachable by a plain
                // mouse click on a bar label — see GTK's
                // `unrecognised_key_closes_a_stale_open_dropdown_on_gtk_1763`
                // (`src/gtk/testing.rs`) for that side's coverage.
                //
                // Deliberately does not also return early or consume the
                // key (no `engine.menu_bar_visible` touch, no early
                // return) — the event must still fall through to the rest
                // of this dispatch unchanged. That is a conscious choice,
                // not an oversight: a dropdown open from a genuine user
                // click, dismissed by a stray letter, now also lets that
                // letter reach and mutate the buffer underneath (a
                // pre-existing leak this does not introduce — the key
                // already flowed through before this arm existed, just
                // with a stale dropdown left on top of it). Swallowing the
                // key instead would avoid that, but would also silently
                // eat whatever the user actually meant to type, which is
                // the worse failure mode of the two. A plain Escape close
                // already leaves the toggleable bar row itself visible too
                // (TUI: `escape_closes_the_dropdown_but_leaves_the_
                // toggleable_bar_row_visible_1763` below), so an
                // unrecognised key closing only the dropdown — and letting
                // the key fall through — is the narrower, consistent fix.
                //
                // NOT fixed here, and still open: #1763's own repro traces
                // the spurious dropdown open back to a real macOS pty
                // apparently collapsing a fast keystroke into an `Alt+g`
                // chord — `TuiDriver` can't carry that modifier
                // synthetically (see `alt_g_dropdown_does_not_survive_a_
                // vim_dw_1763`'s own doc), so the mis-decode itself is
                // neither reproduced nor fixed by this arm, and the
                // mis-decoded keystroke is still swallowed by whichever
                // `handle_alt_char`/menu-open arm actually consumes it
                // upstream of this one. A follow-up issue for that
                // raw-terminal-decode question (vimcode or quadraui) still
                // needs to be filed — track it from there, not from this
                // comment, once it exists.
                //
                // A generic "dismiss an open menu on any unrecognised key"
                // is also arguably `quadraui::MenuSystem::handle`'s own
                // policy to own (every quadraui consumer would want it,
                // not just vimcode) rather than a vimcode-side patch — a
                // quadraui issue for type-ahead/dismiss-on-any-key would
                // let this arm be deleted later; also not yet filed.
                quadraui::MenuEvent::Ignored => {
                    if menu_open && matches!(event, UiEvent::KeyPressed { .. }) {
                        menu_system.borrow_mut().close(backend);
                        self.draw_needed.set(true);
                    }
                }
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
            // `command_center_hit_in_band`, not `CommandCenterLayout::
            // hit_test` directly (#1494 CI): the cached layout's `SearchBox`
            // rect overflows the band whenever the band is narrower than the
            // primitive's 344px content floor, and the overflow lands
            // squarely on the inline window-control buttons that start where
            // the Command Center band ends. See that function's doc.
            let cc_hit = self
                .engine
                .borrow()
                .command_center_layout
                .borrow()
                .as_ref()
                .and_then(|l| crate::render::command_center_hit_in_band(l, position.x, position.y));
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
                            render::WINDOW_MINIMIZE_ACTION => self.window_minimize(backend),
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
        // window border, default everywhere else. Falls through so the
        // editor/sidebar hover handling below still runs. Mirrors
        // `full_chrome_demo`'s `MouseMoved` arm. GTK-only; TUI `set_cursor`
        // is a documented no-op.
        //
        // #1528: `render::WINDOW_RESIZE_GRIP_PX` (a few px), not
        // `backend.line_height()` (a full title-bar/command-line row) — see
        // that constant's doc. With a thin grip there's no need to special-case
        // `ctx.in_title_bar` here the way this used to: the title bar is only
        // as wide as the whole row, and the grip only occupies its outermost
        // sliver, so a point inside the title bar but outside the grip already
        // resolves to `None` on its own. Checked on the SAME predicate the
        // press path below uses when no change-review surface is open — so
        // the cursor never promises a resize the press won't honor in that
        // (overwhelmingly common) case.
        //
        // Pre-existing quirk, NOT fixed by #1528 (review, non-blocking
        // finding #2): this `MouseMoved` arm does not check
        // `change_review_open` at all — it never has — while the press path
        // below does, for the top edge only
        // (`render::resize_edge_overlaps_change_review_band`). So while a
        // change-review surface is open, hovering near the top edge can
        // still show a resize cursor that a press in the same spot won't
        // honor (it falls through to the change-review click handling
        // instead). Narrowing this hint to match would need
        // `change_review_open` computed above the `MouseMoved` arm instead
        // of below it — out of scope for this fix; called out here so the
        // "share one predicate" guarantee above isn't read as unconditional.
        if let UiEvent::MouseMoved { position, .. } = &event {
            let shape = match ctx.window_edge(position.x, position.y, render::WINDOW_RESIZE_GRIP_PX)
            {
                Some(edge) => quadraui::PointerShape::Resize(edge),
                None => quadraui::PointerShape::Default,
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

        // ── Gutter hover — #1544 (`fold_controls = "mouseover"`) ───────────
        // Shared with TUI: both backends reach this same `MouseMoved` arm in
        // `App::handle_dispatch`, so a fold-control marker's visibility can
        // never diverge between them the way a per-backend hover tracker
        // would risk. `render::route_gutter_hover` resolves against
        // `cached_screen_layout` — the same last-painted geometry
        // `handle_mouse_click_msg` already hit-tests real clicks against —
        // so "hovering" and "clicking" the gutter always agree on where it
        // is.
        if let UiEvent::MouseMoved { position, .. } = &event {
            let layout_ref = self.cached_screen_layout.borrow();
            if let Some(layout) = layout_ref.as_ref() {
                let mut engine = self.engine.borrow_mut();
                let was = engine.gutter_hover_window;
                render::route_gutter_hover(
                    &mut engine,
                    layout,
                    position.x as f64,
                    position.y as f64,
                    backend.line_height() as f64,
                    backend.char_width() as f64,
                );
                if engine.gutter_hover_window != was {
                    drop(engine);
                    self.draw_needed.set(true);
                }
            }
        }

        // #955 (ACP-4, review fix): gates the title-bar drag/double-click
        // arms below on the change-review surface being closed. That surface
        // is genuinely full-viewport — its first diff row paints inside
        // `ctx.in_title_bar`'s band, underneath the (visually hidden but
        // still logically live) CSD title bar — so without this guard, a
        // click there was silently reinterpreted as "start dragging the
        // window" instead of reaching `handle_mouse_click_msg`'s
        // change-review branch further down. `ctx.in_title_bar` has no such
        // reach today for the folder picker (its popup is centred, never
        // touching row 0), which is why this wasn't already latent there in
        // a way any existing test could see.
        let change_review_open = self.engine.borrow().change_review.is_some();

        // ── Outer window border: edge-resize press (quadraui#406) ──────────
        // Checked BEFORE the CSD title-bar drag-to-move / double-click-
        // maximize arms and the #816 command-line click below (#1528, review
        // of #816/#1026/#987): `render::WINDOW_RESIZE_GRIP_PX` is a thin,
        // fixed pixel margin (see that constant's doc) — a few px, not a full
        // `line_height` row — so it sits *inside* the title bar and
        // command-line rows instead of spanning them. It has to win over
        // both to ever fire at all: a full-width CSD title bar otherwise owns
        // every pixel of the top row (dead North/NE/NW), and the command
        // line likewise owns every pixel of the bottom row (dead South/SE/
        // SW, the #816 bug this replaces the guard for). Everywhere else in
        // those rows — the overwhelming majority of both — `window_edge`
        // returns `None` and this falls through to the drag/click handling
        // below untouched, same as before #1528. The same thin margin also
        // keeps the East edge out of the vertical scrollbar's and the
        // minimap's own hit-test area with a single editor group (see the
        // constant's doc); no guard is needed for those the way the command
        // line needed one, because the margin no longer reaches them.
        //
        // #1026/#987 review: `begin_window_resize`'s own doc contract is
        // explicit — it returns `false` "when the backend owns no window
        // (TUI...)" and callers "should treat `false` as a no-op, not an
        // error". `ctx.window_edge`'s margin math is generic geometry, not
        // GTK-gated — it fires for any backend near the outer window bounds
        // — so only consume the event when the backend actually armed a
        // resize; otherwise fall through so a window's own rightmost column
        // (exactly where a vertical scrollbar or minimap gutter can sit) on
        // a backend with no window still reaches the editor's own hit-test.
        //
        // #1528 review (blocking finding #3): the base commit's guard here
        // was `!change_review_open` applied to EVERY edge — a regression
        // this PR introduced and the review caught, since the base-commit
        // pre-#1528 code had no `change_review_open` guard on edge-resize at
        // all. The change-review surface only ever overlaps the *top* of the
        // window (see the doc above), so only `North`/`NorthEast`/
        // `NorthWest` need to defer to it —
        // `render::resize_edge_overlaps_change_review_band` is the
        // (unit-tested) decision. Resizing from the bottom, left, right, or
        // the two southern corners must keep working while a change-review
        // surface is open; nothing in that surface ever paints there.
        if let UiEvent::MouseDown {
            button: MouseButton::Left,
            position,
            ..
        } = &event
        {
            if let Some(edge) =
                ctx.window_edge(position.x, position.y, render::WINDOW_RESIZE_GRIP_PX)
            {
                let blocked_by_change_review =
                    change_review_open && render::resize_edge_overlaps_change_review_band(edge);
                if !blocked_by_change_review && backend.begin_window_resize(edge) {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
            }
        }

        // ── CSD titlebar background: drag-to-move / double-click-maximize ──
        // (quadraui#400). Runs after the menu-item intercept, the
        // window-control-button check, and the edge-resize press above, so
        // all three take priority — only a press/double-click that lands in
        // the title bar band but misses every interactive segment (menu
        // item, min/max/close button, resize grip) reaches here, matching
        // `Backend::begin_window_drag`'s documented contract. Mirrors
        // quadraui's `full_chrome_demo` reference. TUI has no window, so
        // `begin_window_drag`/`begin_window_resize`/`toggle_window_maximize`
        // are all documented no-ops there; this path is GTK-only.
        match &event {
            UiEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } if !change_review_open && ctx.in_title_bar(position.x, position.y) => {
                backend.begin_window_drag();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::DoubleClick { position, .. }
                if !change_review_open && ctx.in_title_bar(position.x, position.y) =>
            {
                backend.toggle_window_maximize();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            _ => {}
        }

        // Pointer events over the sidebar content area are forwarded to the active
        // panel's controller before the editor click path sees them. In ShellApp
        // mode there is no per-panel DrawingArea, so without this the file explorer
        // never receives clicks. (#540 ShellApp port)
        if self.try_route_sidebar_mouse_event(&*backend, &event, ctx) {
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
                    // #1744: Ctrl+Shift+\ (VS Code's `editor.action.
                    // jumpToBracket`) needs to decode distinctly from plain
                    // Ctrl+\ (`open_editor_group`/split-editor). Most
                    // surfaces (GDK, and a kitty/CSI-u terminal with
                    // character-resolution keyboard enhancement) already
                    // deliver the resolved glyph `'|'` here, which matches
                    // `Engine::handle_vscode_key`'s own `"Shift_backslash" |
                    // "|"` arm unchanged below — no special case needed for
                    // that shape. A kitty/CSI-u terminal reporting the base
                    // key `'\\'` plus an explicit Shift *bit* instead (rather
                    // than the shifted glyph) needs this one extra arm to
                    // reach the same arm — `render::engine_key_from_ui` has
                    // the mirror logic for this chord too, but is never
                    // called from this `Key::Char` match (see that
                    // function's own module doc on why GTK/TUI's `Key::Char`
                    // decode stays independent of it), so it alone cannot
                    // make this shape reachable.
                    Key::Char('\\') if modifiers.ctrl && modifiers.shift => {
                        ("Shift_backslash".to_string(), Some('\\'))
                    }
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
                        // #1422: `key_name` now reaches every engine consumer
                        // exactly as produced here — `handle_key_press` and
                        // its callees (`dispatch_sidebar_panel_key`,
                        // `dispatch_focus_owner_residual`,
                        // `activity_bar_key_action`, …) used to round-trip it
                        // through a second, GTK-local decode layer
                        // (`map_gtk_key_name` / `map_gtk_key_with_unicode` /
                        // `gtk_key_name_to_quadraui`) that this shared
                        // decoder's own spelling already made redundant — see
                        // issue #1422 for the full audit of why every
                        // consumer already accepted this decoder's spelling
                        // directly.
                        // #1428: `self.keyboard_enhanced` (read once in
                        // `setup()` from the live `BackendCaps::
                        // kitty_keyboard`) rather than a hardcoded `true` —
                        // wrong on a TUI-via-`App` construction running on a
                        // terminal without the kitty protocol (#826);
                        // unaffected on GTK/macOS/Win-GUI since that
                        // capability is always `false` there anyway, which
                        // is exactly what a literal `true` used to paper
                        // over by disabling the terminal-only fallback arms
                        // unconditionally rather than because a real
                        // capability read said so.
                        let n = render::engine_key_from_ui(&key, modifiers, self.keyboard_enhanced)
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
                        backend,
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
                    backend,
                    ctx,
                );
            }
            UiEvent::Accelerator(id, _mods) => {
                if let Some(action) = render::dispatch_panel_accelerator(
                    id.as_str(),
                    &mut self.engine.borrow_mut(),
                    self,
                ) {
                    // `dispatch_panel_accelerator` already mutated `engine`
                    // directly for the nine pure-`Engine` actions, but every
                    // action still needs geometry recomputed before the next
                    // paint — matches the pre-#761 per-arm
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
                // #902/#1580: fired by a *native* right-click popup (macOS
                // `NSMenu` via `Backend::show_context_menu`, opened by
                // `Self::open_context_menu_now_if_native` when
                // `Backend::effective_menu_style()` resolved `Native`).
                // `id` is one of `context_menu_panel_to_quadraui_
                // context_menu`'s synthesised `"context:N"` ids — the exact
                // same ids `route_modal_overlay_click`'s in-window hit-test
                // (`ContextMenuHit::Item` → `context_menu_hit_to_idx`)
                // resolves, so routing the activation through
                // `apply_context_menu_route` reuses that one conversion
                // instead of duplicating it. Reached from `handle_dispatch`
                // itself, i.e. event-handler time — same as every other
                // `UiEvent` arm here, and (per #1580's root-cause fix)
                // never queued from inside a paint closure any more.
                let idx = crate::core::engine::context_menu_hit_to_idx(
                    &quadraui::ContextMenuHit::Item(id),
                );
                let route = match idx {
                    Some(idx) => render::ContextMenuRoute::Item(idx),
                    None => render::ContextMenuRoute::Dismiss,
                };
                self.apply_context_menu_route(&*backend, route);
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::ContextMenuDismissed => {
                // #902: the native popup was dismissed without a selection
                // (Escape, click-away). Same close path a `ContextMenuRoute
                // ::Dismiss` from the in-window hit-test already takes.
                self.apply_context_menu_route(&*backend, render::ContextMenuRoute::Dismiss);
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
                let w = main.width as f64;
                match button {
                    MouseButton::Left if modifiers.ctrl => {
                        self.handle_ctrl_mouse_click(
                            &*backend,
                            position.x as f64,
                            position.y as f64,
                        );
                    }
                    MouseButton::Left => {
                        self.handle_mouse_click_msg(
                            &*backend,
                            position.x as f64,
                            position.y as f64,
                            w,
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
                                        &self.cached_group_tab_bar_layouts.borrow(),
                                        self.cached_frame_hit_map.borrow().as_ref(),
                                        &self.cached_tab_bar_zones.borrow(),
                                    )
                                })
                            };
                            if let Some((group_id, tab_idx)) = tab_target {
                                self.handle_tab_right_click(group_id, tab_idx, rx, ry);
                            } else {
                                self.handle_editor_right_click(&*backend, rx, ry);
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
                self.handle_mouse_double_click_msg(&*backend, position.x as f64, position.y as f64);
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
                        self.apply_context_menu_route(&*backend, route);
                    }
                }
                if buttons.left {
                    let main = ctx.layout.main_content_bounds;
                    self.handle_mouse_drag_msg(
                        &*backend,
                        position.x as f64,
                        position.y as f64,
                        main.width as f64,
                        main.height as f64,
                    );
                }
            }
            UiEvent::MouseUp { .. } => {
                let main = ctx.layout.main_content_bounds;
                self.handle_mouse_up_msg(&*backend, main.width as f64);
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
                self.handle_mouse_scroll_msg(&*backend, delta.x as f64, -(delta.y as f64));
            }
            UiEvent::WindowResized { .. } => {
                // Runner sets new line_height/char_width after resize.
                self.cached_line_height = backend.line_height() as f64;
                self.cached_char_width = backend.char_width() as f64;
                self.line_height_cell.set(self.cached_line_height);
                self.char_width_cell.set(self.cached_char_width);
                self.handle_resize();
                // #1428: forward the resize to any open terminal PTY —
                // mirrors TUI's identical `WindowResized` rung
                // (`route_terminal_resize`), a latent gap on GTK (and any
                // future win-gui/macOS backend hosting a terminal panel)
                // rather than TUI-only behaviour: nothing resized the PTY
                // on a window resize before this, so a shell running
                // inside the terminal panel kept painting at its stale
                // column/row count (`$COLUMNS`/`$LINES`, and any full-
                // screen program reading the real ioctl) after the window
                // — and therefore the panel — changed size.
                //
                // `terminal_panel_cols` off `painted_editor_content_width`
                // (not the just-updated `cached_char_width` against a live
                // pixel width `WindowResized` doesn't carry) — the same
                // "no live pixel width in scope" fallback every other
                // accelerator/menu/tick call site of `terminal_panel_cols`
                // already uses; the *next* `render_content` repaints the
                // terminal panel at the corrected geometry regardless of
                // which frame's width this resize computed against.
                let cols = self.terminal_panel_cols(self.painted_editor_content_width());
                let rows = self.engine.borrow().session.terminal_panel_rows;
                render::route_terminal_resize(&mut self.engine.borrow_mut(), cols, rows);
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
        // ── Terminal chrome the runner doesn't own (#1428) ──────────────
        // Mirrors the pre-#1434 TUI shell's `tick`'s identical rung verbatim, including
        // the `self.live` gate — see `Self::live`'s own doc for why an
        // unconditional call would corrupt a `TuiBackend`-backed test
        // harness's real stdout. A no-op on GTK/macOS/Win-GUI either way
        // (`Backend::set_caret_shape`'s trait default), so gating this
        // costs nothing there.
        if self.live {
            let shape = {
                let engine = self.engine.borrow();
                render::caret_shape_for_mode(&engine, engine.sidebar_has_focus())
            };
            // #1634: only write when the shape actually changed since the
            // last tick — see `last_caret_shape`'s own doc (including its
            // "not confirmed as the flicker cause" caveat). Without this,
            // `TuiBackend::set_caret_shape` re-emitted the identical
            // DECSCUSR escape (`ratatui::crossterm::cursor::SetCursorStyle`)
            // to the real terminal on every idle poll cycle
            // (`quadraui::runtime::IDLE_POLL_CEILING`, ~250ms) for no reason
            // — real waste, independent of whether it's what the operator
            // saw flicker. The decision itself is `dedup_caret_shape`, a
            // free function so it's directly unit-tested without a
            // `Backend` — see that function's own doc for why a
            // `Backend`-call-count driver test isn't achievable here.
            if dedup_caret_shape(&mut self.last_caret_shape, shape) {
                backend.set_caret_shape(shape);
            }
        }

        // Keep cached metrics up to date.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);

        // Retry dropping the server-side titlebar until the runner's window
        // is mapped — see `capture_window_and_apply_csd` (#552). No-ops once
        // `csd_applied` is set.
        self.capture_window_and_apply_csd(backend);

        // Retry restoring the saved window size/position/maximized state
        // until the runner's window is mapped — see `restore_window_geometry`
        // (#1529). No-ops once `window_geometry_restored` is set.
        self.restore_window_geometry(backend);

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

        // Periodic background work: LSP, DAP, git, search, etc. — also
        // where the yank-highlight deadline armed by `run_post_key_epilogue`
        // (#813) and the platform-action drain (open URL / reveal in file
        // manager, #1134) are polled, since #1248 folded both into the
        // chore list `render::run_shared_tick_chores` shares with TUI.
        self.handle_poll_tick(backend);

        // #1428: the one-shot nerd-font startup nudge — mirrors
        // the pre-#1434 TUI shell's `tick`'s identical drain (`pending_startup_msg`),
        // unconditional on `Self::live`, unlike the caret-shape write
        // above: this only ever writes to `engine.message`, never touches
        // the real terminal, so there is nothing here a test harness needs
        // protecting from. Always `None` on a GUI-backend `App` (see
        // `Self::pending_startup_msg`'s own doc), so this is a no-op there.
        if let Some(msg) = self.pending_startup_msg.take() {
            self.engine.borrow_mut().message = msg;
            self.draw_needed.set(true);
        }

        // #1508: while any ACP session (or the direct-curl transport, which
        // also flips `ai_streaming`) has a turn in flight, keep re-arming
        // `tick` at the cadence `quadraui::runner::Reaction::RedrawAfter`'s
        // own doc recommends for exactly this case (its "thinking-spinner
        // countdown" example) — 100ms, i.e. ≥4 Hz — rather than trusting
        // the coarser 250ms `IDLE_POLL_CEILING` fallback every backend
        // keeps regardless. `Engine::tick_ai_spinner` (called from
        // `poll_idle` above, inside `handle_poll_tick`) already advanced
        // the frame this tick and set `draw_needed`, so the `Redraw` below
        // paints the new frame now; this re-arms the *next* wake.
        if self
            .engine
            .borrow()
            .acp_sessions
            .iter()
            .any(|s| s.ai_streaming)
        {
            backend.request_frame_in(std::time::Duration::from_millis(100));
        }

        // #1668: while any terminal pane has a live PTY session, keep
        // re-arming `tick` so `Engine::poll_terminal` keeps draining its
        // output — see `terminal_poll_rearm_delay`'s own doc for why this
        // matters specifically (and only) on Win-GUI, and why it's a
        // harmless no-op re-arm on GTK/TUI/macOS (they already tick
        // regardless, via their own `IDLE_POLL_CEILING` fallback).
        if let Some(delay) =
            terminal_poll_rearm_delay(!self.engine.borrow().terminal_panes.is_empty())
        {
            backend.request_frame_in(delay);
        }

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }
}

impl App {
    /// The logic behind [`Self::on_shell_event_ctx`], the real
    /// (non-deprecated, `ShellContext`-aware) hook the runner calls.
    ///
    /// #1491 pulled this out of what used to be `App`'s override of the
    /// deprecated ctx-less `ShellApp::on_shell_event` (issue #617's
    /// predecessor) and into this plain inherent method, so
    /// `on_shell_event_ctx` calls it directly instead of dispatching through
    /// the deprecated trait method — `App` no longer overrides
    /// `on_shell_event` at all (the trait's own no-op default applies),
    /// which was the last deprecation-lint suppression this file needed
    /// once the tab-bar hit-testing migration cleared the others.
    fn dispatch_shell_event(&mut self, event: &quadraui::AppShellEvent) {
        use quadraui::AppShellEvent;
        // #1062: the shadow-`engine.app_shell` sync, unconditionally and
        // first — see `render::sync_shell_event_shadow`'s rung comment for
        // why this call has to come before any of the id-specific branching
        // below rather than be repeated inside each arm.
        {
            let mut engine = self.engine.borrow_mut();
            render::sync_shell_event_shadow(event, &mut engine);
        }
        match event {
            AppShellEvent::PanelChanged { panel_id } => {
                // #1427: the hamburger panel only exists on the `cell`
                // profile (`Self::shell_config`) — reveal the menu bar
                // instead of switching to a nonexistent shadow panel.
                // Mirrors the pre-#1434 TUI shell's `on_shell_event`'s own
                // hamburger arm, now shared via
                // `render::route_hamburger_panel_changed`; see its own doc.
                // A no-op check on GTK/macOS/Win, which never register this
                // panel id in the first place.
                if render::route_hamburger_panel_changed(&mut self.engine.borrow_mut(), panel_id) {
                    self.last_shell_panel = Some(panel_id.clone());
                    return;
                }
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
                    // #1427: a real activity-bar click is exactly as much
                    // "the user moved on" as a keystroke — spend the guard
                    // (mirrors TUI's identical call in this arm). A no-op
                    // when never armed (GTK/macOS/Win).
                    render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                    self.switch_panel(panel_id.as_str().to_string());
                    return;
                }
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                // #1360: a built-in panel's activity-bar icon click must move
                // keyboard focus into that panel, exactly as TUI's own
                // `PanelChanged` arm does (`focus_sidebar_panel` +
                // `sidebar.has_focus = true` in the pre-#1434 TUI shell's `on_shell_event`)
                // — before this, GTK only redrew, so `render::route_focus_key`
                // (which every keystroke passes through, see
                // `Self::handle_key_press`'s "Shared focus-owner keyboard
                // rung") kept reading `sidebar_has_focus() == false` and sent
                // every subsequent key straight to the editor. `sidebar.
                // has_focus`/`ext_panel_name` have no GTK equivalent to set —
                // GTK's `route_focus_key` call passes
                // `engine.sidebar_has_focus()` itself as the "band" (see
                // that call site's own comment), so the one engine call
                // below is the whole fix, and it is the same
                // already-shared `Engine::focus_sidebar_panel` this method's
                // own `toggle_focus_search`/`toggle_focus_explorer` already
                // call for the keyboard-accelerator path.
                self.engine
                    .borrow_mut()
                    .focus_sidebar_panel(panel_id.as_str());
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarHidden => {
                // #1427: a real panel's own second click — the hamburger's
                // is intercepted by `Self::on_shell_event_ctx` before it
                // ever reaches this ctx-less method (see that method's own
                // doc) — so spend the guard here too (mirrors TUI).
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                // #557: this is also how a *second* click on an open
                // extension panel's icon arrives — `sync_shell_event_shadow`
                // already dropped the plugin panel's claim (its
                // `SidebarHidden` arm clears the same two fields
                // unconditionally). Re-opening still works:
                // `AppShell::handle_activity_click` reports a click on the
                // active panel as `PanelChanged`, not `SidebarHidden`, once
                // the sidebar is hidden.
                //
                // #1427: `Engine::clear_sidebar_focus()` — a real second
                // click must clear the sidebar's own keyboard focus the
                // same way the keyboard-driven `ActivityBarKeyAction::
                // Collapse` arm above already does (via the wider
                // `Engine::collapse_sidebar`, which also touches
                // `session.explorer_visible` — not appropriate here, since
                // this arm fires for *any* panel's close, not just
                // Explorer's), or a subsequent keystroke keeps routing to
                // whichever sidebar panel last held focus instead of the
                // editor. A pre-existing gap on every backend (this arm
                // never cleared focus before), invisible until #1427's own
                // `App`-on-TUI driver tests started clicking a real panel's
                // icon twice in a row (reveal, then collapse) — the
                // pre-#1427 "tui" arm never needed a first click at all,
                // since Explorer was already the runner's default-active
                // panel.
                self.engine.borrow_mut().clear_sidebar_focus();
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarResized { .. } => {
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
            }
            AppShellEvent::BottomItemClicked { id } => {
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                // The runner treats bottom activity-bar items as action
                // buttons (not sidebar panels), so it never toggles or
                // hides on its own — it only ever reports the click.
                // #1057: this used to unconditionally `show_panel`, so a
                // second click on an already-open bottom item (e.g.
                // "bottom:settings") re-showed it instead of collapsing the
                // sidebar like VS Code does for an active-tab click — while
                // the old, hand-rolled the pre-#1434 TUI shell's `on_shell_event` (same
                // arm) already ran the toggle. Route through `switch_panel`,
                // the same shared `render::apply_activity_panel_switch`
                // call site `PanelChanged`'s ext-panel arm above already
                // uses, so both backends make the identical toggle decision
                // from one place instead of drifting again. #1433: this is
                // no longer "two arms that happen to agree" — `App` is what
                // TUI dispatches through in production now too, so this one
                // arm *is* both backends' Settings-bottom-item-toggle
                // behaviour; there was no remaining fork to reconcile when
                // auditing this for the flip.
                self.switch_panel(id.as_str().to_string());
            }
            // #1427: every remaining `AppShellEvent` variant is likewise a
            // shell-consumed user interaction — spend the guard and let any
            // variant quadraui adds later default to the safe direction
            // (mirrors TUI's identical catch-all).
            _ => render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut()),
        }
    }
}

/// #1549: the macOS report ("command-line row below the status bar is
/// clipped to about half its height by the window's bottom edge")
/// reduces to one portable geometric invariant on the painted rect: it
/// must fit entirely inside the real window content area (`viewport`,
/// the exact portable equivalent of AppKit's `contentView.bounds`
/// on every backend — see `quadraui::Backend::viewport`) and be at
/// least one full line tall.
///
/// This should hold on *every* backend by construction: quadraui's
/// `compose::app_shell::compute_layout` derives `AppShellLayout::
/// main_content_bounds` from this exact `viewport` (title bar carved off
/// the top; nothing reserved below, since vimcode never opts into
/// quadraui's own status-bar/command-line bands — `render_content`'s own
/// `status_bar_h`/`cmd_y` locals lay both rows out entirely inside
/// `main_content_bounds` instead), and `cmd_y + lh` always resolves to
/// `main.y + main.height` (see the `FrameOp::CommandLine` arm below). A
/// live macOS run is the one environment this fleet cannot check that
/// construction against directly — no macOS cross-toolchain is
/// installed (`src/macos/mod.rs`'s "Verifying this file without a Mac").
/// `debug_assert!` (not a hard `assert!`) so a violation surfaces loudly
/// in a debug build's log/console — exactly the signal a future live-
/// macOS debug run needs to confirm or rule out this shape of bug —
/// instead of crashing a release build over a cosmetic clip.
fn debug_assert_command_line_fits_viewport(
    cmd_rect: quadraui::Rect,
    viewport: quadraui::Viewport,
    line_height: f64,
) {
    debug_assert!(
        (cmd_rect.y + cmd_rect.height) as f64 <= viewport.height as f64 + 0.5,
        "#1549: command-line row bottom ({}) exceeds the real window's \
         content height ({}) — the row will be clipped by the window edge",
        cmd_rect.y + cmd_rect.height,
        viewport.height,
    );
    debug_assert!(
        cmd_rect.height as f64 + 0.5 >= line_height,
        "#1549: command-line row height ({}) is shorter than one full \
         line ({})",
        cmd_rect.height,
        line_height,
    );
}

#[cfg(test)]
mod command_line_viewport_geometry_tests {
    //! #1549: coverage for `debug_assert_command_line_fits_viewport` in
    //! isolation — no live window (or even a constructed `App`) needed,
    //! since the check is a pure function of the three values the bug
    //! report's own "next step" asked to be logged. `cargo test`'s
    //! default debug profile keeps `debug_assert!` live, so these panics
    //! are real, observable failures, not silently-compiled-out no-ops.

    use super::*;

    fn viewport(height: f32) -> quadraui::Viewport {
        quadraui::Viewport::new(800.0, height, 1.0)
    }

    /// The exact shape `render_content`'s `FrameOp::CommandLine` arm
    /// produces on a healthy frame: the row ends precisely at the
    /// viewport's bottom edge. Must not panic.
    #[test]
    fn row_flush_with_viewport_bottom_does_not_panic() {
        let rect = quadraui::Rect::new(0.0, 576.0, 800.0, 24.0);
        debug_assert_command_line_fits_viewport(rect, viewport(600.0), 24.0);
    }

    /// RED-verified: with the assertion's first check removed, this test
    /// still passes (proving it *can* fail) — see the reasoning in the
    /// function's own doc for why this shape is exactly #1549's report.
    /// Mirrors a real window whose usable content height is ~12px
    /// shorter than the layout assumed (half of a 24px line).
    #[test]
    #[should_panic(expected = "#1549: command-line row bottom")]
    fn row_extending_past_viewport_bottom_panics() {
        let rect = quadraui::Rect::new(0.0, 576.0, 800.0, 24.0);
        debug_assert_command_line_fits_viewport(rect, viewport(588.0), 24.0);
    }

    /// The second, independent invariant: a row shorter than one full
    /// line (even if it does fit inside the viewport) still reproduces
    /// the reported symptom — only part of a line of glyphs has room to
    /// paint.
    #[test]
    #[should_panic(expected = "is shorter than one full")]
    fn row_shorter_than_line_height_panics() {
        let rect = quadraui::Rect::new(0.0, 576.0, 800.0, 12.0);
        debug_assert_command_line_fits_viewport(rect, viewport(600.0), 24.0);
    }
}

impl quadraui::ShellApp for App {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // #1428: read the live kitty-keyboard-protocol capability once, at
        // setup time — mirrors the pre-#1434 TUI shell's `setup`'s identical read
        // (`self.keyboard_enhanced = backend.backend_caps().kitty_keyboard`)
        // verbatim, except unconditional rather than gated behind TUI's own
        // `self.live` (that gate exists solely because a *second*,
        // TUI-only, direct crossterm round-trip used to live at this call
        // site before #1109 — see the pre-#1434 TUI shell's `setup`'s own doc; reading
        // `backend_caps()` itself is a plain field access on every backend,
        // never I/O, so there is nothing here for a "don't do this under a
        // test harness" gate to protect against). A GUI backend's
        // `BackendCaps::kitty_keyboard` is always `false` (GDK/AppKit/Win32
        // hand over an already-resolved keysym per physical key, never the
        // terminal-only ambiguity this flag exists to disambiguate — see
        // `render::engine_key_from_ui`'s own doc), so this is a no-op there;
        // TUI's own capability is unaffected by living on the shared `App`
        // instead of the pre-#1434 TUI shell — `backend.backend_caps()` reads the same
        // `TuiBackend` state either way.
        self.keyboard_enhanced = backend.backend_caps().kitty_keyboard;
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

        // Try to drop the server-side WM titlebar now, in favour of the
        // drawn CSD row; `setup()` runs before `run_with_shell`'s runner
        // calls `window.present()`, so `backend.window()` is very likely
        // still `None` here. `tick()` retries every frame until the window
        // is mapped, which is the reliable path (#552).
        self.capture_window_and_apply_csd(backend);

        // Same "very likely still None here, tick() retries" story as the
        // CSD drop above, for restoring the saved window size/position/
        // maximized state instead (#1529).
        self.restore_window_geometry(backend);

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
            // `install_menu_bar`'s macOS implementation (`MacBackend`)
            // asserts it is called on the real AppKit main thread and
            // panics otherwise — a documented quadraui limitation with no
            // portable pre-check exposed through the `Backend` trait.
            // Every real invocation of `ShellApp::setup` *is* on the main
            // thread (`quadraui::macos::shell_runner`'s only entry point),
            // so this never fires outside a test harness — but
            // `quadraui::macos::testing::driver_with_shell` (used by
            // `src/macos/mod.rs::mac_driver_tests`) necessarily calls
            // `setup` from a spawned test thread, same as every other
            // `#[test]` fn, per Rust's own test runner. Catching it here
            // keeps `setup()` — which every backend, including the ones
            // with no native menu, must be able to complete without
            // aborting the process — from taking the whole test process
            // down over a call this method doesn't otherwise depend on.
            // Filed upstream: `install_menu_bar` should degrade
            // gracefully off-main-thread the way its own sibling test
            // helpers already do (`menu_bar_install.rs`'s `let Some(mtm)
            // = MainThreadMarker::new() else { return }`), not hard
            // `.expect()`.
            //
            // #1618 found `WinBackend` (quadraui#1200) briefly also
            // declared `native_menu: true` and implemented
            // `install_menu_bar` (a real Win32 `HMENU` via `SetMenu`) —
            // quadraui#1228 (#1629) reverted that: `WinBackend` no longer
            // declares `native_menu`, `install_menu_bar` is back to the
            // trait's no-op default on that backend, and Windows now
            // takes the `window_chrome` arm below instead (a drawn menu
            // row, matching GTK). `MacBackend` remains the only in-tree
            // `native_menu` implementation this `catch_unwind` guards
            // against, so its panic-recovery rationale stays
            // macOS-specific again.
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
        } else if backend.backend_caps().window_chrome {
            // GTK's (and any future Win-GUI's) drawn menu bar doubles as the
            // client-side titlebar — pinned visible always, same as before
            // #1427 (this `else` used to be the only other arm).
            self.engine.borrow_mut().menu_bar_visible = true;
        } else {
            // #1427: no OS menu bar to hide behind (`native_menu`) and no
            // window chrome for the drawn row to double as (`window_chrome`)
            // — the `cell` profile (TUI-via-`App`) today. The bar is fully
            // hideable at runtime (mirrors the pre-#1434 TUI shell's `setup`,
            // `event_loop`'s `mod.rs:797`) and starts however
            // `Engine::new` already resolved `menu_bar_visible` (`true`
            // only in vscode-mode) — left untouched here, unlike the two
            // arms above, which both *force* a value regardless of what the
            // engine was constructed with.
            self.engine.borrow_mut().menu_bar_toggleable = true;
        }
        self.engine
            .borrow()
            .menu_system
            .borrow_mut()
            .set_menus(menu_defs);

        // #1498: initial CSS used to be applied here by hand (a GTK-only
        // `css_provider` theming the native file dialog). That provider is
        // gone — `GtkPlatformServices::set_theme` (JDonaghy/quadraui#1091)
        // now owns the equivalent stylesheet, and `render_content`'s first
        // frame already calls `sync_per_frame_backend_state`, which calls
        // `Backend::set_theme` unconditionally before anything paints — so
        // there is nothing left for `setup` to do here.

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

        // #1427: correct this frame's layout for the hamburger's own
        // phantom sidebar reservation — a no-op unless
        // `engine.menu_bar_toggleable` (the `cell` profile; always `false`
        // on GTK/macOS/Win). See `render::reclaim_hamburger_sidebar_
        // reservation`'s own doc; mirrors pre-#1427
        // the pre-#1434 TUI shell's `render_content`'s identical rebind.
        let corrected_layout = render::reclaim_hamburger_sidebar_reservation(&engine, layout);
        let layout = &corrected_layout;

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
        let tab_row_h = (self.units.tab_row_h)(lh);
        let tab_bar_h = (self.units.tab_bar_h)(lh, engine.settings.breadcrumbs);
        // Whether the *global* (non-per-window) status bar occupies its own
        // row — `global_status_bar_visible`, not `effective_window_status_line`
        // directly, since `'laststatus'` can hide the status line entirely
        // (0, or 1 with a single window) even while per-window status is off,
        // in which case there is still no separate global row to offset the
        // wildmenu past (#1235 follow-up).
        let global_status_visible = render::global_status_bar_visible(&engine);
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
        // #1421: the raw `h` this frame's `compute_editor_layout` call above
        // was given — see `cached_main_content_height`'s own doc for why
        // this is `h`, not `editor_area_h`/`editor_bounds`'s height.
        self.cached_main_content_height.set(h);
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
            (self.units.breadcrumb_row_h)(lh),
            backend.scrollbar_reserve() as f64,
            self.units.minimap,
        );

        // Cache for click handlers (move into RefCell, then borrow back for drawing).
        *self.cached_screen_layout.borrow_mut() = Some(screen);
        let screen_ref = self.cached_screen_layout.borrow();
        let screen = screen_ref.as_ref().unwrap();

        // #560 / #947 / #1104: mouse clicks resolve editor columns via the
        // per-glyph Pango inverse (`Backend::editor_col_at_x`), not a naive
        // uniform-cell division — see `pixel_to_click_target`'s call site for
        // the emoji/CJK drift #560 reported.
        //
        // Before #1104, click-time hit-testing ran against `self.backend`, a
        // SECOND `GtkBackend` vimcode constructed purely to mimic the real
        // one's font (`build_editor_click_context` + a `set_editor_font` echo
        // right here, every frame) — because the mouse-click handlers
        // (`handle_mouse_click_msg` and friends) never received the runner's
        // own live backend at all. #1104 threaded that live `backend`
        // (the exact instance `quadraui::gtk::run` paints every frame, already
        // carrying the stable `pango_ctx` its own `activate()` sets at widget
        // realize, and already up to date on `editor_font_*` via
        // `sync_per_frame_backend_state`'s `set_editor_font` call above)
        // through the whole click/drag/modal-stack call chain instead, so
        // there is no second backend left to keep in sync here — this frame's
        // `backend` *is* the click backend now.
        //
        // `self.backend` still exists for the handful of callers this refactor
        // could not reach: `explorer_ui_event`, `route_ai_sidebar_event` and
        // the DAP-sidebar key route all call
        // `quadraui::Backend::set_current_line_height`/`set_current_char_width`
        // (JDonaghy/quadraui#1086) to undo click drift, from deep inside the
        // keyboard/mouse dispatch tree where only `&mut self` (no live
        // `backend` reference) is available.

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
        //
        // #1432: `quadraui::native_dialog_options` is a pure content-shape
        // check (no table/no input) — it says nothing about whether *this*
        // backend actually has a native alert facility to show it with. Its
        // own doc says as much: "callers should consult [`BackendCaps::
        // native_dialogs`] before calling [`Backend::show_message_dialog`],
        // and fall back to `draw_dialog` when it returns `None`." Gating here
        // (rather than only inside `Some(opts) =>`) also keeps `presence.dialog`
        // below correct — an ungated `native_dialog_shown` would still have
        // suppressed the in-canvas rung on a backend with no native dialog at
        // all (TUI), leaving the dialog painted nowhere.
        match screen
            .dialog
            .as_ref()
            .map(render::dialog_panel_to_quadraui_dialog)
            .as_ref()
            .and_then(quadraui::native_dialog_options)
            .filter(|_| backend.backend_caps().native_dialogs)
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
            // #1234: see `paint_title_bar_band`'s identical read for why
            // this queries `WindowControl::is_maximized()` live instead of
            // caching.
            let maximized = backend
                .window()
                .and_then(|w| w.is_maximized().ok())
                .unwrap_or(false);
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

        // #955 review fix, the #1117 class: `presence.change_review` is the
        // exact gate `compose_frame` uses to decide whether
        // `FrameOp::ChangeReview` is even in this frame's op list, so that
        // rung's arm below can never run on the frame the surface *closes* —
        // an `else` inside it is dead code, and the full-viewport modal-stack
        // entry `paint_change_review_rung` pushes would stay registered
        // forever, routing every later click past `AppShell`'s chrome dispatch
        // (`ShellAdapter::handle` hit-tests the modal stack first). Reconciled
        // here, once, unconditionally, ungated by which rungs compose —
        // exactly how `reconcile_editor_hover_modal` is called.
        render::reconcile_change_review_modal_stack(
            backend,
            presence.change_review,
            popup_viewport,
        );

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
                        // Shares the command-line row whenever there is no
                        // separate global bar row to sit under — per-window
                        // status lines are on, or `'laststatus'` hides the
                        // status line entirely (#1235 follow-up).
                        let wm_y = if global_status_visible {
                            status_y + lh
                        } else {
                            status_y
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
                    // #1549: catch a "row clipped by the window edge"
                    // regression the instant it's introduced, on every
                    // backend, rather than only on a live macOS run.
                    debug_assert_command_line_fits_viewport(cmd_rect, backend.viewport(), lh);
                    // #816: publish the painted rect for
                    // `render::command_line_click_char_idx` — the exact twin
                    // of `global_status_rect` above, and TUI's identical
                    // cache in `shell_app.rs`'s own `FrameOp::CommandLine`
                    // arm.
                    self.engine.borrow().command_line_rect.set(cmd_rect);
                    // #1185: paint through `draw_command_line_selection`
                    // (quadraui#1001) instead of the selection-blind
                    // `draw_command_line` — `cmd_sel` is character indices
                    // into `screen.command.text`, converted to the byte
                    // offsets the primitive expects.
                    let sel_bytes =
                        self.engine.borrow().cmd_sel.get().map(|sel| {
                            render::command_line_selection_bytes(&screen.command.text, sel)
                        });
                    backend.draw_command_line_selection(cmd_rect, &cmd, sel_bytes);
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
                // trait method TUI's the pre-#1434 TUI shell's `render_content` calls) is
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
                // helpers the legacy path used (`PickerGeometry` + this
                // profile's own picker sizing), so no Pango/Cairo access is
                // needed here.
                render::FrameOp::UnifiedPicker => {
                    if let Some(ref picker) = screen.picker {
                        let rect = render::paint_picker_rung(
                            backend,
                            picker,
                            popup_viewport,
                            &(self.units.picker)(lh as f32),
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
                // the pre-#1434 TUI shell's `render_content` uses, through
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
                            &(self.units.tab_switcher)(lh as f32),
                        ) {
                            let list =
                                render::tab_switcher_to_quadraui_list_view(ts, geo.visible_rows);
                            backend.draw_list(geo.bounds, &list);
                            self.tab_switcher_popup_rect.set(Some(geo.bounds));
                            composed.push(render::FrameOp::TabSwitcher);
                        }
                    }
                }

                // ── Context menu (#546, #1580) ────────────────────────────────
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
                        // #1580: a native popup (`Backend::show_context_menu`)
                        // was already shown from the event handler that opened
                        // it (`App::handle`'s `open_context_menu_now_if_native`
                        // choke point) — never from here. This rung paints
                        // nothing in-window when the backend resolves `Native`
                        // (no layout to cache, no rung to record as painted),
                        // and must not call `show_context_menu` itself: doing
                        // so from inside `render_content` re-enters AppKit's
                        // modal popup loop from a paint closure that still
                        // holds the borrows it needs to finish its own frame.
                        if backend.effective_menu_style() == quadraui::ResolvedMenuStyle::Custom {
                            let layout = render::paint_context_menu_rung(
                                backend,
                                panel,
                                popup_viewport,
                                cw,
                                lh,
                                0.0,
                            );
                            *self.context_menu_layout.borrow_mut() = Some(layout);
                            composed.push(render::FrameOp::ContextMenu);
                        } else {
                            *self.context_menu_layout.borrow_mut() = None;
                        }
                    }
                }

                // ── Change-review surface (#955, shared with #525) ───────────
                // `render::paint_change_review_rung` is the whole body — no
                // GTK-specific diff rendering, matching every other
                // `quadraui::DiffView` consumer. Uses the same
                // `popup_viewport` every other overlay rung anchors to. The
                // surface's modal-stack entry is reconciled *before* the walk
                // (see there), not from an `else` here — this arm cannot run
                // on a frame where the surface is closed.
                render::FrameOp::ChangeReview => {
                    if let Some(review) = screen.change_review.as_ref() {
                        render::paint_change_review_rung(
                            backend,
                            &engine,
                            review,
                            popup_viewport,
                            &theme,
                        );
                        composed.push(render::FrameOp::ChangeReview);
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
        let had_context_menu_before = self.engine.borrow().context_menu.is_some();
        let reaction = self.handle_dispatch(event, backend, ctx);
        // #1580: open the native context-menu popup exactly once, from
        // this event-handler choke point — never from `render_content`'s
        // paint rung. Gated on the open *transition* (`None` -> `Some`),
        // not just `is_some()`, so a native menu that's still open (the
        // real runner blocks on AppKit's own modal loop for the whole
        // `show_context_menu` call, so this can't actually re-enter, but a
        // headless test driver that calls `handle` again while nothing
        // closed the menu must not re-show it either).
        if !had_context_menu_before {
            self.open_context_menu_now_if_native(backend);
        }
        // #1427: keep the runner's `AppShell` title-bar reservation in sync
        // with `engine.menu_bar_visible` — that flag can flip from any one
        // of several places inside `handle_dispatch` (the #1427 reveal/hide
        // routing at its top, a `:set menu`/`nomenu` ex-command, the
        // status-bar `[M]` toggle segment's `Engine::handle_status_action`,
        // ...), each with its own early `return`, so this single choke point
        // — run once, after every path through `handle_dispatch` has
        // already returned — is what makes the sync unconditional rather
        // than requiring one call per mutation site. See
        // `render::sync_menu_bar_title_row`'s own doc.
        //
        // Gated on `menu_bar_toggleable`, not unconditional: on GTK/macOS/
        // Win the title-bar *band* is a permanent, construction-time
        // reservation (`shell_config`'s `with_title_bar`) that survives
        // `menu_bar_visible` going `false` on its own — only the *drawn*
        // row/dropdown inside it are coupled to that flag (#939's own
        // regression pin, `gtk::testing::command_center_stays_live_when_
        // menu_bar_is_hidden`: a native-menu backend, or a test forcing the
        // flag false directly, must not lose the Command Center's anchor
        // band). Calling `set_title_bar_visible` there unconditionally
        // would collapse that reservation the moment anything sets the
        // flag false, which is wrong for every profile except the
        // toggleable one, where the band genuinely is meant to come and go
        // with the flag.
        if self.engine.borrow().menu_bar_toggleable {
            render::sync_menu_bar_title_row(ctx, &self.engine.borrow());
        }
        // #1427: keep the runner's `AppShell` sidebar *visibility* in sync
        // with the shadow's too, unconditionally — not just from the two
        // narrower call sites this method already had
        // (`run_post_key_epilogue`, `on_shell_event_ctx`'s `BottomItemClicked`
        // arm) — but *only* on the toggleable-menu-bar profile
        // (`menu_bar_toggleable`), where it's what corrects `AppShell::
        // new`'s "active + visible" construction-time default away from the
        // hamburger *before* the very first hamburger click ever lands, so
        // that click resolves as a reveal rather than `AppShell::
        // handle_activity_click`'s already-active toggle-hide branch — see
        // `render::sync_runner_sidebar_visibility`'s own doc.
        //
        // Deliberately gated, not unconditional like the title-row sync
        // above: GTK/macOS/Win's existing two call sites already keep their
        // runner/shadow sidebar visibility converged on every path that
        // needs it, and a *third*, blanket call on every dispatch surfaced
        // a pre-existing staleness in numerous test fixtures that mutate
        // `session.explorer_visible` post-construction without also calling
        // `Engine::sync_app_shell_sidebar_visibility`/`AppShell::show_panel`
        // (harmless before #1427, since nothing ever read the shadow's
        // `sidebar_visible()` this eagerly) — collapsing their sidebar
        // reservation the moment any dispatch ran. The toggleable profile
        // needs the correction regardless (that's the whole fix), but
        // GTK/macOS/Win do not, so this stays scoped to where #1427
        // actually requires it.
        if self.engine.borrow().menu_bar_toggleable {
            self.sync_runner_sidebar_visibility(ctx);
        }
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
    /// the pre-#1434 TUI shell's `take_requested_panel` already had this override (see
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

    /// #1057: the ctx-aware override TUI's the pre-#1434 TUI shell already had (its
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
        // #1427: a `SidebarHidden` for the hamburger — the runner's second
        // click on it while the menu is open — has to be special-cased
        // *here*, before delegating to the ctx-less `Self::on_shell_event`
        // below, because only `ctx` (the runner's own `AppShell`) can tell
        // the hamburger apart from a real panel's own second click; the
        // shadow `engine.app_shell` has no hamburger `PanelDefinition` at
        // all (see `render::route_hamburger_panel_changed`'s doc). Mirrors
        // the pre-#1434 TUI shell's `on_shell_event_ctx`'s identical check, now
        // shared via `render::route_hamburger_sidebar_hidden`. A no-op check
        // on GTK/macOS/Win, which never register this panel id.
        if matches!(event, quadraui::AppShellEvent::SidebarHidden)
            && ctx
                .shell()
                .active_panel_id()
                .map(quadraui::WidgetId::as_str)
                == Some(crate::core::engine::sidebar::HAMBURGER_PANEL_ID)
        {
            render::route_hamburger_sidebar_hidden(&mut self.engine.borrow_mut(), ctx);
            return;
        }
        self.dispatch_shell_event(event);
        // #1356 (quadraui bump for quadraui#1055): a bottom item's click
        // never moves the *real* `AppShell` on its own — the `BottomItemClicked`
        // arm above only toggles the engine-side shadow via `switch_panel`.
        // quadraui#1055 lets `show_panel` accept a bottom item's id (e.g.
        // "bottom:settings"), but an app has to opt in explicitly by calling
        // it from its own `BottomItemClicked` handler — exactly what
        // quadraui's own `AppShellDemo` does. Without this, the runner's own
        // `AppShell` (which is what `render` titles the sidebar header from)
        // never learns Settings now owns the sidebar, and the header stays
        // stuck on whatever top panel was open before.
        if let quadraui::AppShellEvent::BottomItemClicked { id } = event {
            if self.engine.borrow().app_shell.sidebar_visible() {
                ctx.shell_mut().show_panel(id);
            } else {
                ctx.shell_mut().hide_sidebar();
            }
        }
        self.sync_runner_sidebar_visibility(ctx);
        // #1427: `ShellAdapter::handle` consumes a `PanelChanged`/
        // `SidebarHidden` for a top-row panel itself and returns without
        // ever calling `Self::handle` — the one place `Self::handle`'s own
        // `render::sync_menu_bar_title_row` call can never reach. Mirrors
        // the pre-#1434 TUI shell's `on_shell_event_ctx`'s identical tail call
        // — see that function's own doc for why running this on every path
        // (not just the hamburger arm above) is what makes any *future* arm
        // that flips `menu_bar_visible` get the same same-frame guarantee
        // for free. Gated on `menu_bar_toggleable` for the same reason as
        // `Self::handle`'s own identical call — see that call site's doc.
        if self.engine.borrow().menu_bar_toggleable {
            render::sync_menu_bar_title_row(ctx, &self.engine.borrow());
        }
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

    /// #1124 routed title-sync (`handle_poll_tick`) and [`App::
    /// window_minimize`] through `Backend::window()` (quadraui#950)
    /// instead of the GTK-only `self.window`/`PlatformWindowHandle` seam
    /// — that seam was `None` on macOS and Win-GUI, so both were silent
    /// no-ops there before this fix. What that PR could not add is
    /// black-box coverage of the *positive* path (a title that actually
    /// reaches the OS window, a minimize that actually iconifies it) —
    /// here is why that gap is real, not an oversight, and what would
    /// close it:
    ///
    /// 1. `GtkBackend::window()` (quadraui `gtk/backend.rs:2012-2015`)
    ///    returns `Some` only once `self.window` has been set via
    ///    `GtkBackend::set_window`, which takes a real `gtk4::
    ///    ApplicationWindow` — constructing one needs GTK initialized
    ///    against a live display. This repo's whole headless-testing
    ///    strategy (`src/gtk/testing.rs`'s `GtkDriver` wrapper)
    ///    deliberately never calls `gtk::init()` (see that module's own
    ///    "Known gaps" doc), so there is no headless path to a non-`None`
    ///    `GtkBackend::window()`.
    /// 2. Routing through `GtkDriver` instead of a bare `GtkBackend`
    ///    doesn't work around (1) either: `GtkDriver::backend()`
    ///    (quadraui `gtk/testing.rs:341`, and the sibling
    ///    `handle_poll_tick_reloads_settings_changed_on_disk` test right
    ///    above already established this for the poll-tick pump) returns
    ///    `&GtkBackend`, not `&mut GtkBackend` — there is no
    ///    `backend_mut()` — but `Backend::window()` needs `&mut self`, so
    ///    even a driver that *had* a window attached couldn't reach it
    ///    from this crate's tests.
    /// 3. A hand-rolled fake `Backend` to test the wiring in isolation is
    ///    not an option either: `quadraui::Backend` is a sealed trait
    ///    (`pub(crate) sealed::Sealed` supertrait, `backend.rs:488-614`),
    ///    and the one publicly constructible non-live impl,
    ///    `quadraui::testing::RecordingBackend`, does not override
    ///    `window()` either, so it inherits the trait's default `None`
    ///    too (checked directly against the pinned rev: no `fn window(`
    ///    in `quadraui/src/testing/mod.rs`).
    ///
    /// This is the same shape of gap `src/macos/mod.rs`'s
    /// `control_inset_is_default_because_mac_driver_never_sets_a_window`
    /// test documents for `titlebar_control_inset` (#940) — a live
    /// windowed run is the only thing that exercises the `Some` branch,
    /// which is why manual title-sync/minimize verification is a
    /// `SMOKE_TESTS` item on #1124's PR rather than an automated test
    /// here. Closing this properly needs a quadraui-side testing hook
    /// (e.g. a `GtkDriver::backend_mut()`, or a way to attach a window
    /// without a live display) — a quadraui issue to file, not a
    /// vimcode workaround (`CLAUDE.md`'s Platform-Neutrality Rule: file
    /// upstream, wait, then implement).
    ///
    /// RED-verification note: nothing to make RED here, symmetric with
    /// the macOS test's own note — this pins what the pinned quadraui rev
    /// provably always returns for a `GtkBackend` nobody has called
    /// `set_window` on, not a vimcode behaviour that could regress. Its
    /// job is the opposite: it goes RED the day quadraui adds a headless
    /// way to attach a window (or a version bump otherwise changes this),
    /// which is exactly the signal that real black-box coverage of
    /// title-sync/minimize finally becomes possible.
    ///
    /// #1234 moved `capture_window_and_apply_csd` (CSD/`set_decorated`),
    /// the two `paint_title_bar_band`/`render_content` `is_maximized` reads,
    /// and `cached_window_width`/`cached_window_height`'s `bounds()` refresh
    /// onto this exact same `Backend::window()` accessor, deleting the
    /// `PlatformWindowHandle`/`gtk4::Window` seam that used to back them.
    /// They inherit the identical structural gap this test documents — this
    /// assertion covering all of them, not just title-sync/minimize, is why
    /// it was not split into one copy per call site. #1529's
    /// `App::restore_window_geometry` (the `set_size`/`set_bounds`/
    /// `toggle_window_maximize` restore) and `sync_window_title`'s new
    /// `cached_window_x`/`y`/`maximized` caching are the same story again:
    /// both gate on this identical `Some`/`None` split, so both are live-
    /// smoke-only for the same reason — see `restore_window_geometry_*`
    /// tests below for what *is* covered headlessly (the retry-until-mapped
    /// contract and the pure clamp/snapshot logic feeding it).
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

    /// #1165: `App::handle_poll_tick` never drained `App::tab_visible_counts`
    /// before this fix, so `Engine::post_draw_apply_widths` — the "single
    /// contract every UI backend must call after each completed paint" per
    /// its own doc comment — was never called on GTK at all. A tab scrolled
    /// out of the visible tab bar by a resize, a sidebar toggle, or simply
    /// opening enough tabs could stay off-screen forever on this backend,
    /// with nothing left to bring it back until some unrelated change
    /// happened to touch `tab_scroll_offset` (`goto_tab`/`close_tab`/etc.).
    /// TUI already self-corrected within two frames via the identical
    /// `tab_visible_counts` → `post_draw_apply_widths` drain in
    /// the pre-#1434 TUI shell's `tick`.
    ///
    /// Follows `handle_poll_tick_reloads_settings_changed_on_disk`'s
    /// established pattern of driving `App::handle_poll_tick` directly
    /// against a real `App` + `Engine` (see that test's doc for why: as of
    /// the pinned quadraui rev `GtkDriver` has no way to pump `tick()`
    /// headlessly, so a genuine painted-pixel assertion needs quadraui-side
    /// test infrastructure this repo doesn't have yet — a quadraui issue to
    /// file, not a vimcode workaround, per `CLAUDE.md`'s Platform-Neutrality
    /// Rule). What this test stands in for a real paint: `tab_visible_counts`
    /// is normally populated by `paint_tab_bars_rung` from
    /// `click::tab_bar_available_cols(bar.rect, &bar.layout, ..)`, off the
    /// exact geometry `render::paint_tab_bars` (shared with TUI, already
    /// covered by its own driver-tier tests) just painted — here it's pushed
    /// by hand to isolate
    /// the wiring bug this issue is about from that already-tested paint
    /// step.
    ///
    /// RED-verification: reverting this fix's `handle_poll_tick` block
    /// (the `tab_visible_counts` drain calling `post_draw_apply_widths`)
    /// while leaving the `tab_visible_counts` field itself in place turns
    /// this red — `tab_scroll_offset` stays `0` and the assertion below
    /// fails. Confirmed by hand before committing.
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

    /// #1360: the GTK mirror of the pre-#1434 TUI shell's own
    /// `take_requested_panel_echo_does_not_steal_focus` (`src/tui_main/
    /// shell_app.rs`) — a *reconciliation* `PanelChanged` (the one
    /// `Self::take_requested_panel` synthesizes to steer the runner's own
    /// `AppShell` back onto whatever the shadow `engine.app_shell` already
    /// believes, e.g. after a DAP reveal or a keyboard accelerator) must
    /// only update `Self::last_shell_panel`'s bookkeeping — never call
    /// `Engine::focus_sidebar_panel` the way a genuine activity-bar click
    /// does. Without the `suppress_shell_panel_echo` guard this issue's own
    /// fix added a call behind, every one of those app-initiated switches
    /// would also steal keyboard focus into the panel out from under
    /// whatever the app itself just focused (e.g. the editor, mid-DAP-launch).
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

    /// #1529: `App::restore_window_geometry` gates on `Backend::window()`
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

    /// #1529: `App::cached_window_geometry` is the save-side snapshot
    /// `App::quit_and_save_session`/`App::save_session_and_exit` (folded
    /// from three separate near-duplicate sites into one by #1499) funnel
    /// through before `Engine::save_session_state` persists it. Before
    /// this fix each of those three sites wrote only `width`/`height`
    /// into `engine.session.window` — `x`/`y`/`maximized` were never
    /// saved no matter what `sync_window_title` cached, the exact "save
    /// is partial" bug this issue reports. This drives the cells
    /// directly (`sync_window_title`'s own live-window dependency is the
    /// same structural gap `gtk_backend_window_is_none_...` documents,
    /// so it can't be exercised headlessly) and asserts the snapshot
    /// carries all five fields through — a data-plumbing assertion, not
    /// a painted one: these cells feed a session-file write, not a
    /// screen, so CLAUDE.md's "assert on rendered output" rule (aimed at
    /// paint paths that never consume the state they populate) has no
    /// paint path to apply to here.
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

    /// #1529 review: the black-box GTK coverage the acceptance criteria
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

    /// #1722: a tab-bar width correction that doesn't move the active
    /// tab's resolved scroll offset must not schedule a redraw — the
    /// shared-code half of "spurious full repaints: idle tick returns
    /// Redraw ~4×/s with several tabs open". Backend-neutral (`TuiBackend`,
    /// no `gui` feature needed — unlike
    /// `handle_poll_tick_scrolls_the_active_tab_back_into_view_on_gtk`
    /// above) because the bug lives in
    /// `render::run_shared_tick_chores`/`Engine::post_draw_apply_widths`,
    /// code both backends share equally; this additionally proves the fix
    /// reaches the real tick path a runner drives (`App::handle_poll_tick`
    /// → `run_shared_tick_chores` → `self.draw_needed`), the exact signal
    /// `tick_dispatch` turns into `Reaction::Redraw`/`Continue` — the same
    /// observable `app_on_tui_tests`'s `idle_stability_1583`/`_1650` assert
    /// on via `driver.tick()`.
    ///
    /// Stands in for a real paint by pushing straight into
    /// `App::tab_visible_counts`, the same technique
    /// `handle_poll_tick_scrolls_the_active_tab_back_into_view_on_gtk`
    /// above uses and documents the rationale for (no headless way to pump
    /// a real paint here yet) — on a pixel-measuring backend (GTK/Win-GUI/
    /// macOS) the measured width genuinely can wobble by a sub-pixel-
    /// rounding unit between otherwise-identical frames; this test
    /// reproduces that shape directly rather than depending on real GTK
    /// float jitter happening to land on a test machine.
    ///
    /// RED-verified: reverting `Engine::post_draw_apply_widths` to its
    /// pre-#1722 `width_bookkeeping_changed || scroll_changed` contract
    /// makes this fail — the second tick below (a one-column width wobble
    /// with no effect on the resolved scroll offset) sets `draw_needed`
    /// instead of leaving it clear. Confirmed by hand before committing.
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
