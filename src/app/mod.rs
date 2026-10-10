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
