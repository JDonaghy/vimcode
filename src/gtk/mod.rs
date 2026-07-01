// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional
// TODO: Migrate to ListView/ColumnView in a future phase
#![allow(deprecated)]

use gio::prelude::{FileExt, FileMonitorExt};
use gtk4::cairo::Context;
use gtk4::gdk;
use gtk4::pango::{self, AttrColor, AttrList, FontDescription};
use gtk4::prelude::*;
use pangocairo::functions as pangocairo;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::core;
use crate::icons;
use crate::render;

use core::engine::EngineAction;
use core::settings::LineNumberMode;
use core::{Engine, OpenMode, WindowRect};
use render::{build_screen_layout, CommandLineData, RenderedWindow, Theme};

use copypasta_ext::ClipboardProviderExt;
use std::collections::HashMap;

mod backend;
mod click;
mod css;
mod draw;
mod events;
mod explorer;
mod quadraui_gtk;
mod services;
mod util;

use click::*;
use css::*;
use draw::*;
use util::*;

use crate::core::engine::sidebar::*;

fn is_ext_panel_id(id: &str) -> bool {
    id.starts_with("ext:")
}

fn ext_panel_name(id: &str) -> Option<&str> {
    id.strip_prefix("ext:")
}

type TabSlotMap = HashMap<usize, Vec<(f64, f64)>>;
type TabCloseMap = HashMap<usize, Vec<Option<(f64, f64)>>>;

/// Per-group pixel-accurate tab-bar hit geometry recovered from
/// [`quadraui::Backend::tab_bar_layout`] during the ShellApp `render_content`
/// pass. All x-ranges are **relative to the group's tab-bar left edge** — the
/// same space as `render::screen_zone_hit_test`'s `local_x`.
///
/// This replaces the char-cell `hit_regions` approximation for GTK tab clicks.
/// GTK draws tabs with proportional-font Pango widths + fixed pixel padding
/// (`tab_pad`, `inner_gap`, close-glyph width), so a `name.chars() * char_width`
/// estimate under-measures every tab — shifting the tab/close boundaries and
/// making mid-tab clicks land on the close button and right-edge clicks land on
/// the next tab (#515 regression). The rasteriser reports the exact drawn
/// geometry, so we hit-test against that. (`hit_regions` stays authoritative for
/// the monospace TUI backend, whose char-cell layout matches its draw.)
#[derive(Default, Clone)]
pub(super) struct TabBarPixelHits {
    /// `(start_x, end_x)` per tab index; `(0.0, 0.0)` for scrolled-off tabs.
    pub slots: Vec<(f64, f64)>,
    /// `Some((start_x, end_x))` close-button zone per tab, or `None`.
    pub close: Vec<Option<(f64, f64)>>,
    /// Right-segment hit zones (split / diff / action buttons) as
    /// `(start_x, end_x, target)`, disjoint from the tab slots.
    pub segments: Vec<(f64, f64, crate::core::engine::TabBarClickTarget)>,
}

/// Key = `group_id.0` (single-group mode keys under the active group's id, which
/// is what `screen_zone_hit_test` reports for it).
type TabPixelHitMap = HashMap<usize, TabBarPixelHits>;

/// Convert a rasteriser [`quadraui::TabBarHits`] (absolute pixel x, from
/// `Backend::tab_bar_layout`) plus its source [`quadraui::TabBar`] into a
/// [`TabBarPixelHits`] with every x-range shifted to be **relative to
/// `bar_left_x`** (the group tab bar's left edge). Right-segment ids are mapped
/// to their `TabBarClickTarget` using the same `"tab:*"` ids that
/// `build_tab_bar_primitive` emits (mirrors `draw::draw_tab_bar`).
fn tab_hits_to_pixel_hits(
    hits: &quadraui::TabBarHits,
    bar: &quadraui::TabBar,
    bar_left_x: f64,
) -> TabBarPixelHits {
    use crate::core::engine::TabBarClickTarget as T;
    let rel = |a: f64, b: f64| (a - bar_left_x, b - bar_left_x);
    let slots = hits
        .slot_positions
        .iter()
        .map(|&(a, b)| {
            if (a, b) == (0.0, 0.0) {
                (0.0, 0.0) // scrolled-off sentinel — leave as zero-width
            } else {
                rel(a, b)
            }
        })
        .collect();
    let close = hits
        .close_bounds
        .iter()
        .map(|c| c.map(|(a, b)| rel(a, b)))
        .collect();
    let mut segments = Vec::new();
    for (i, seg) in bar.right_segments.iter().enumerate() {
        let Some((a, b)) = hits.right_segment_bounds.get(i).copied() else {
            continue;
        };
        let Some(ref id) = seg.id else { continue };
        let target = match id.as_str() {
            "tab:split_right" => Some(T::SplitRight),
            "tab:split_down" => Some(T::SplitDown),
            "tab:diff_prev" => Some(T::DiffPrev),
            "tab:diff_next" => Some(T::DiffNext),
            "tab:diff_toggle" => Some(T::DiffToggle),
            "tab:action_menu" => Some(T::ActionMenu),
            _ => None,
        };
        if let Some(t) = target {
            let (s, e) = rel(a, b);
            segments.push((s, e, t));
        }
    }
    TabBarPixelHits {
        slots,
        close,
        segments,
    }
}

/// Cached diff toolbar button positions per group: group_id -> (prev_start, prev_end, next_start, next_end, fold_start, fold_end).
/// Populated during draw_tab_bar, used for click hit-testing.
type DiffBtnMap = HashMap<usize, (f64, f64, f64, f64, f64, f64)>;

/// Cached split button pixel widths per group: group_id -> (both_btns_px, btn_right_px).
/// Only populated when split buttons are visible (active group in multi-group, or single-group mode).
type SplitBtnMap = HashMap<usize, (f64, f64)>;

/// Cached action menu button pixel range per group: group_id -> (start_x, end_x).
type ActionBtnMap = HashMap<usize, (f64, f64)>;

/// Cached dialog button hit rects: Vec<(x, y, w, h)> populated by draw_dialog_popup.
type DialogBtnRects = Vec<(f64, f64, f64, f64)>;

/// Cached per-window status segment hit zones: window_id -> Vec<(start_x, end_x, action)>.
/// Populated by draw_window_status_bar, consumed by click hit-testing.
type StatusSegmentMap = HashMap<usize, Vec<(f64, f64, crate::core::engine::StatusAction)>>;

/// Return type of draw_tab_bar: (tab_slot_positions, close_bounds,
/// diff_btn_positions, split_btn_widths, visible_tab_count, action_btn,
/// correct_scroll_offset).
/// `correct_scroll_offset` is the offset that would make the active tab
/// visible given THIS frame's pixel measurements; the caller compares to
/// the engine's stored value and triggers a repaint if they differ.
type TabBarDrawResult = (
    Vec<(f64, f64)>,
    Vec<Option<(f64, f64)>>, // per-tab close-button bounds (None for sentinels)
    Option<(f64, f64, f64, f64, f64, f64)>,
    Option<(f64, f64)>,
    usize,
    Option<(f64, f64)>, // action menu button (start_x, end_x)
    usize,              // correct_scroll_offset (per-group, in pixels-aware units)
);

// ─── Panel-key accelerator registry ─────────────────────────────────────────
//
// Stable accelerator IDs for the `panel_keys` settings, registered on
// `GtkBackend` at App startup. Mirrors the TUI's `tui.panel.*`
// registry. As of Phase B.5b Stage 2 the editor key handler runs a
// single `match_keypress` lookup against this registry and routes
// matches through `dispatch_gtk_panel_accelerator` — replacing 13
// inline `matches_gtk_key` arms that used to scan the bindings linearly.

pub(super) const ACC_TOGGLE_SIDEBAR: &str = "gtk.panel.toggle_sidebar";
pub(super) const ACC_FOCUS_EXPLORER: &str = "gtk.panel.focus_explorer";
pub(super) const ACC_FOCUS_SEARCH: &str = "gtk.panel.focus_search";
pub(super) const ACC_FUZZY_FINDER: &str = "gtk.panel.fuzzy_finder";
pub(super) const ACC_LIVE_GREP: &str = "gtk.panel.live_grep";
pub(super) const ACC_COMMAND_PALETTE: &str = "gtk.panel.command_palette";
pub(super) const ACC_OPEN_TERMINAL: &str = "gtk.panel.open_terminal";
pub(super) const ACC_TERMINAL_TOGGLE_MAX: &str = "terminal.toggle_maximize";
pub(super) const ACC_ADD_CURSOR: &str = "gtk.panel.add_cursor";
pub(super) const ACC_SELECT_ALL_MATCHES: &str = "gtk.panel.select_all_matches";
pub(super) const ACC_SPLIT_EDITOR_RIGHT: &str = "gtk.panel.split_editor_right";
pub(super) const ACC_SPLIT_EDITOR_DOWN: &str = "gtk.panel.split_editor_down";
pub(super) const ACC_NAV_BACK: &str = "gtk.panel.nav_back";
pub(super) const ACC_NAV_FORWARD: &str = "gtk.panel.nav_forward";

/// Register the panel-keys accelerator set on the backend. Re-runs on each
/// settings reload so live rebinding takes effect.
/// Dead in ShellApp mode until accelerator re-binding is re-wired (#448-C follow-on).
#[allow(dead_code)]
fn register_panel_accelerators(
    backend: &mut backend::GtkBackend,
    pk: &crate::core::settings::PanelKeys,
) {
    use quadraui::Backend;
    let entries: [(&str, &str); 14] = [
        (ACC_TOGGLE_SIDEBAR, &pk.toggle_sidebar),
        (ACC_FOCUS_EXPLORER, &pk.focus_explorer),
        (ACC_FOCUS_SEARCH, &pk.focus_search),
        (ACC_FUZZY_FINDER, &pk.fuzzy_finder),
        (ACC_LIVE_GREP, &pk.live_grep),
        (ACC_COMMAND_PALETTE, &pk.command_palette),
        (ACC_OPEN_TERMINAL, &pk.open_terminal),
        (ACC_TERMINAL_TOGGLE_MAX, &pk.toggle_terminal_maximize),
        (ACC_ADD_CURSOR, &pk.add_cursor),
        (ACC_SELECT_ALL_MATCHES, &pk.select_all_matches),
        (ACC_SPLIT_EDITOR_RIGHT, &pk.split_editor_right),
        (ACC_SPLIT_EDITOR_DOWN, &pk.split_editor_down),
        (ACC_NAV_BACK, &pk.nav_back),
        (ACC_NAV_FORWARD, &pk.nav_forward),
    ];
    for (id, binding) in entries {
        let acc_id = quadraui::AcceleratorId::new(id);
        if binding.is_empty() {
            backend.unregister_accelerator(&acc_id);
            continue;
        }
        backend.register_accelerator(&quadraui::Accelerator {
            id: acc_id,
            binding: quadraui::KeyBinding::Literal(binding.to_string()),
            scope: quadraui::AcceleratorScope::Global,
            label: None,
        });
    }
}

// ─── Phase B.5b Stage 2: panel-key accelerator dispatcher ───────────────────
//
// Mirrors `tui_main::dispatch_panel_accelerator`. Replaces 13 inline
// `if matches_gtk_key(&pk.X, ...)` arms in the editor key handler with
// a single registry lookup → match-on-id dispatcher. The action set
// matches what the legacy arms did (Msg dispatch where the existing
// update() handler runs the side effect; direct engine mutation where
// the legacy arms also called engine directly).
//
// Returns `true` if the id was handled — caller should `return Stop`.
// Returns `false` for unknown ids so the caller can fall through.
//
// `ACC_TERMINAL_TOGGLE_MAX` is included for completeness; the engine's
// own `match_accelerator` block handles the same key first and
// returns Stop, so this arm is only reachable if the engine's
// registration is removed.
fn dispatch_gtk_panel_accelerator(
    id: &str,
    sender: &MsgSender,
    engine: &Rc<RefCell<Engine>>,
) -> bool {
    match id {
        ACC_OPEN_TERMINAL => {
            sender.send(Msg::ToggleTerminal).ok();
            true
        }
        ACC_TOGGLE_SIDEBAR => {
            sender.send(Msg::ToggleSidebar).ok();
            true
        }
        ACC_FOCUS_EXPLORER => {
            sender.send(Msg::ToggleFocusExplorer).ok();
            true
        }
        ACC_FOCUS_SEARCH => {
            sender.send(Msg::ToggleFocusSearch).ok();
            true
        }
        ACC_FUZZY_FINDER => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Files);
            sender.send(Msg::Resize).ok();
            true
        }
        ACC_LIVE_GREP => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Grep);
            sender.send(Msg::Resize).ok();
            true
        }
        ACC_COMMAND_PALETTE => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Commands);
            sender.send(Msg::Resize).ok();
            true
        }
        ACC_TERMINAL_TOGGLE_MAX => {
            sender.send(Msg::ToggleTerminalMaximize).ok();
            true
        }
        ACC_ADD_CURSOR => {
            engine.borrow_mut().add_cursor_at_next_match();
            sender.send(Msg::Resize).ok();
            true
        }
        ACC_SELECT_ALL_MATCHES => {
            engine.borrow_mut().select_all_occurrences();
            sender.send(Msg::Resize).ok();
            true
        }
        ACC_SPLIT_EDITOR_RIGHT => {
            engine
                .borrow_mut()
                .open_editor_group(crate::core::window::SplitDirection::Vertical);
            true
        }
        ACC_SPLIT_EDITOR_DOWN => {
            engine
                .borrow_mut()
                .open_editor_group(crate::core::window::SplitDirection::Horizontal);
            true
        }
        ACC_NAV_BACK => {
            engine.borrow_mut().tab_nav_back();
            true
        }
        ACC_NAV_FORWARD => {
            engine.borrow_mut().tab_nav_forward();
            true
        }
        _ => false,
    }
}

/// Drop-in replacement for the removed `relm4::Sender<Msg>`.
///
/// Async GTK callbacks (clipboard reads, file dialogs, timers) clone this
/// sender and push messages to the shared VecDeque. `ShellApp::tick` drains
/// the queue on every frame to process them synchronously.
#[derive(Clone)]
struct MsgSender(Rc<RefCell<VecDeque<Msg>>>);

impl MsgSender {
    fn new() -> Self {
        MsgSender(Rc::new(RefCell::new(VecDeque::new())))
    }

    /// Enqueue a message for processing in the next `tick()` call.
    fn send(&self, msg: Msg) -> Result<(), ()> {
        self.0.borrow_mut().push_back(msg);
        Ok(())
    }

    /// Take all pending messages, leaving the queue empty.
    fn drain(&self) -> Vec<Msg> {
        let mut q = self.0.borrow_mut();
        q.drain(..).collect()
    }
}

struct App {
    engine: Rc<RefCell<Engine>>,
    /// Set to true in update() whenever a draw is needed; cleared by the #[watch] block.
    /// This prevents the 20/sec SearchPollTick timer from unconditionally calling queue_draw().
    draw_needed: Rc<Cell<bool>>,
    /// DrawingArea for the file explorer sidebar (Phase A.2b-2: native
    /// `gtk4::TreeView` replaced by a single DrawingArea rendering via
    /// `draw_explorer_panel`).
    explorer_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// A.6f: activity bar DA handle; used to queue redraws when panel
    /// state or extension registrations change.
    activity_bar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,

    /// Row height actually used by the most recent explorer draw call.
    /// The draw callback writes this each frame from the same Pango
    /// context it renders with, so click and scroll handlers hit-test with
    /// byte-exact row math.
    explorer_row_height_cell: Rc<Cell<f64>>,
    /// Explorer DA's UI-font line_height + char_width in pixels — cached
    /// for the engine-drawn ctx menu (#426). The right-click handler
    /// converts pixel coords to engine cells using these, and the
    /// explorer-DA-side ctx menu render multiplies them back out for the
    /// anchor pixel.
    explorer_line_height_cell: Rc<Cell<f64>>,
    explorer_char_width_cell: Rc<Cell<f64>>,
    /// Cached ContextMenuLayout from the last explorer-ctx-menu paint
    /// on the window-overlay DA (#426). Capture-phase click + motion
    /// handlers hit-test against this.
    explorer_ctx_menu_layout: Rc<RefCell<Option<quadraui::ContextMenuLayout>>>,
    /// Window-level overlay DA dedicated to the explorer ctx menu (#426)
    /// — kept here so `Msg::ExplorerRightClick` / `Esc` / item-confirm
    /// can `queue_draw()` it.
    ctx_menu_overlay_da: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// Fractional dy accumulator for the explorer scroll wheel. Small
    /// trackpad deltas are summed here until they exceed one row, so no
    /// scroll event is silently dropped.
    explorer_scroll_accum: Rc<Cell<f64>>,
    /// Most recent scrollbar rect in DA-local coords, published by
    drawing_area: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    menu_bar_da: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    debug_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// Line height the debug-sidebar draw closure last computed via
    /// `pangocairo::create_context(cr).metrics(...)`. Click / scroll /
    /// key handlers read this cell so their row math agrees with what
    /// was painted, even when the widget's `pango_context()` reports a
    /// different scale than the cairo-derived context (HiDPI). #281
    /// smoke surfaced a 4:3 drift between the two paths.
    debug_sidebar_lh: Rc<Cell<f64>>,
    git_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    ext_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// DrawingArea for extension-provided panels (e.g. git-insights GIT LOG).
    ext_dyn_panel_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// Outer Box for the extension-provided panel sidebar.
    ext_dyn_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    ai_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    sidebar_inner_sw: Rc<RefCell<Option<gtk4::ScrolledWindow>>>,
    /// Direct ref to the sidebar Revealer for programmatic open/close.
    sidebar_revealer: Rc<RefCell<Option<gtk4::Revealer>>>,
    /// Direct refs to each panel's outer Box for programmatic show/hide.
    explorer_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    search_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    debug_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    git_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    ext_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    settings_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    /// DrawingArea inside the Settings panel (Phase A.3c-2: native widget
    /// tree replaced by a single DrawingArea that calls `draw_settings_panel`).
    settings_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    ai_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>>,
    // Per-window scrollbars and indicators
    window_scrollbars: Rc<RefCell<HashMap<core::WindowId, WindowScrollbars>>>,
    overlay: Rc<RefCell<Option<gtk4::Overlay>>>,
    cached_line_height: f64,
    cached_char_width: f64,
    /// Last seen pointer position over the editor `DrawingArea`, in
    /// DA-local pixels. Updated by `EventControllerMotion`; cleared on
    /// `leave`. Read by the scroll handler to route wheel events to
    /// the window under the cursor (#240) — matches TUI behaviour.
    last_editor_pointer: Rc<Cell<Option<(f64, f64)>>>,
    /// Cached line height for the UI font (sidebars, panels).
    /// Computed alongside `cached_line_height` in `CacheFontMetrics`.
    cached_ui_line_height: f64,
    /// Cached dialog button hit rects: Vec<(x, y, w, h)> populated by draw_dialog_popup.
    dialog_btn_rects: Rc<RefCell<DialogBtnRects>>,
    /// Shared with the drawing-area resize callback so scrollbars can be
    /// repositioned synchronously (before each frame) without going through
    /// Relm4's async message queue.
    line_height_cell: Rc<Cell<f64>>,
    char_width_cell: Rc<Cell<f64>>,
    /// Current mouse position, updated directly from the motion callback (no Relm4 message).
    mouse_pos_cell: Rc<Cell<(f64, f64)>>,
    /// Shared with draw closure: hovered state for Cairo h scrollbars.
    h_sb_hovered_cell: Rc<Cell<bool>>,
    /// Shared with draw closure: which tab close button (×) is hovered: (group_id.0, tab_idx).
    tab_close_hover_cell: Rc<Cell<Option<(usize, usize)>>>,
    /// Shared with draw closure: which window (if any) has an active h scrollbar drag.
    h_sb_drag_cell: Rc<Cell<Option<core::WindowId>>>,
    /// True while user is drag-selecting text inside a find/replace input field.
    fr_input_dragging: bool,
    #[allow(dead_code)] // Kept alive to continue monitoring settings.json
    settings_monitor: Option<gio::FileMonitor>,
    sender: MsgSender,
    /// Last content written to system clipboard.
    /// Used to avoid redundant writes on every keystroke.
    last_clipboard_content: Option<String>,
    /// True while the mouse cursor is over any horizontal scrollbar track.
    h_sb_hovered: bool,
    /// Which tab close button (×) the mouse is over: (group_id.0, tab_idx).
    tab_close_hover: Option<(usize, usize)>,
    /// Cached tab slot widths per group, populated during draw_tab_bar for click hit-testing.
    /// Key = group_id.0 (or usize::MAX for single-group mode), Value = cumulative x positions.
    tab_slot_positions: Rc<RefCell<TabSlotMap>>,
    /// Cached close-button bounds per tab per group, populated during
    /// draw_tab_bar. Used by `tab_close_hit_test` for hover detection.
    tab_close_bounds: Rc<RefCell<TabCloseMap>>,
    /// Pixel-accurate per-group tab-bar hit geometry from the ShellApp
    /// `render_content` pass (via `Backend::tab_bar_layout`). Consumed by the
    /// GTK tab-bar click hit-test instead of the char-cell `hit_regions`, which
    /// don't match GTK's proportional-font tab layout. (#515)
    cached_tab_pixel_hits: Rc<RefCell<TabPixelHitMap>>,
    /// Cached diff toolbar button pixel positions, populated during draw_tab_bar.
    diff_btn_map: Rc<RefCell<DiffBtnMap>>,
    split_btn_map: Rc<RefCell<SplitBtnMap>>,
    action_btn_map: Rc<RefCell<ActionBtnMap>>,
    /// Cached per-window status bar segment hit zones from draw_window_status_bar.
    status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Cached ScreenLayout from the last draw_editor paint pass. Click handlers
    /// read this instead of recomputing geometry from engine state (#344).
    cached_screen_layout: Rc<RefCell<Option<render::ScreenLayout>>>,
    /// Per-group tab-drop geometry (absolute pixel bounds) computed each frame in
    /// `render_content`. Both the drag overlay (same frame) and the drag hit-test
    /// in `handle_mouse_drag_msg` (next mouse-move) read this, so the drop-zone
    /// detection and the highlight always use one identical bounds source. (#515)
    cached_drop_groups: Rc<RefCell<Vec<render::TabDropGroup>>>,
    /// Effective tab-bar height (px) paired with `cached_drop_groups`.
    cached_drop_tbh: Rc<Cell<f32>>,
    /// Backend (line_height, char_width) captured at the instant the file
    /// explorer tree was rendered. The backend's `current_line_height` is mutable
    /// per-frame state and may differ by click time, which made the explorer
    /// hit-test resolve the wrong row (it ran `tree_layout` at a different line
    /// height than `draw_tree` used). Re-applied before hit-testing so draw and
    /// hit agree. (#540 ShellApp port)
    cached_explorer_metrics: Rc<Cell<(f64, f64)>>,
    /// Pixel y-offset where the debug toolbar was last drawn.
    debug_toolbar_y_offset: Rc<Cell<f64>>,
    /// Pixel height of the debug toolbar (last draw).
    debug_toolbar_height: Rc<Cell<f64>>,
    /// Cached menu-dropdown hit regions from the last draw of the
    /// dropdown overlay. Each entry is `(x, y, w, h, action_id)`
    /// where `action_id` is e.g. `menu:7`. Click + motion handlers
    /// walk this list to map (x, y) → engine-side
    /// `MENU_STRUCTURE.items` index instead of computing row indices
    /// True while the user drags the terminal header row to resize the panel.
    terminal_resize_dragging: bool,
    /// True while the user drags the terminal split divider left/right.
    terminal_split_dragging: bool,
    /// Split index being dragged (None if not dragging a group divider).
    group_divider_dragging: Option<usize>,
    /// True while the user is dragging a tab between groups.
    tab_dragging: bool,
    /// Start position of a potential tab drag (set on MouseClick in tab bar).
    tab_drag_start: Option<(f64, f64)>,
    /// Source of the active tab drag: (group_id, tab_index).  Set when drag starts.
    tab_drag_source: Option<(core::window::GroupId, usize)>,
    /// Most recently computed drop zone during an active tab drag.
    tab_drag_drop_zone: core::window::DropZone,
    /// GTK window handle — set in `ShellApp::setup` once the runner creates the window.
    window: Option<gtk4::Window>,
    /// Last time sc_refresh() was called for the Git sidebar auto-refresh.
    last_sc_refresh: std::time::Instant,
    /// Last time explorer tree indicators (modified/diagnostics) were refreshed.
    last_tree_indicator_update: std::time::Instant,
    /// Full-window overlay DrawingArea that draws the menu dropdown.
    /// Can-target toggles true/false with menu open/close.
    menu_dropdown_da: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// Full-window overlay DrawingArea for panel hover popups.
    panel_hover_da: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// Link hit rects populated during hover popup draw: (x, y, w, h, url, is_native).
    #[allow(clippy::type_complexity)]
    panel_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String, bool)>>>,
    /// Popup bounding rect (x, y, w, h) — set during draw, used for motion hit-testing.
    #[allow(dead_code, clippy::type_complexity)]
    panel_hover_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Editor hover popup bounding rect (x, y, w, h) — set during draw, used for click hit-testing.
    #[allow(clippy::type_complexity)]
    editor_hover_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Completion popup layout — set during draw, used for hit-test in
    /// the click handler. None when the popup isn't visible.
    completion_layout: Rc<RefCell<Option<quadraui::CompletionsLayout>>>,
    /// Context menu layout — set during draw, used for hit-test in
    /// both click and motion handlers. None when no menu is visible.
    context_menu_layout: Rc<RefCell<Option<quadraui::ContextMenuLayout>>>,
    /// Tab-switcher popup bounding rect (x, y, w, h) — set during
    /// draw, used for `ModalStack` registration in the click
    /// handler. (B.5b Stage 7.)
    #[allow(clippy::type_complexity)]
    tab_switcher_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Dialog popup bounding rect (x, y, w, h) — set during draw
    /// from the resolved `quadraui::DialogLayout::bounds`. Used for
    /// `ModalStack` registration in the click handler. The pre-fix
    /// `dialog_btn_rects`-derived inline calc overshot the actual
    /// popup width on small dialogs (`:about`), causing
    /// click-outside-to-dismiss to fail.
    #[allow(clippy::type_complexity)]
    dialog_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Link hit rects populated during editor hover popup draw: (x, y, w, h, url).
    #[allow(clippy::type_complexity)]
    editor_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>>,
    /// Editor hover popup scrollbar geometry (#215). Populated by
    /// `draw_editor_hover_popup`; consumed by click + drag handlers
    /// in this file.
    editor_hover_scrollbar: Rc<Cell<Option<render::PopupScrollbarHit>>>,
    /// Cached line height shared with menu_dropdown_da draw/click closures.
    menu_dd_line_height: Rc<Cell<f64>>,
    /// CSS provider registered with the GTK display — updated when colorscheme changes.
    css_provider: gtk4::CssProvider,
    /// Colorscheme name at the time the CSS was last applied.
    last_colorscheme: String,
    /// Cross-backend modal-overlay tracking. Pushed to when a palette /
    /// `quadraui::Backend`-impl handle. Owns the canonical
    /// accelerators / event-queue / viewport / services / modal-stack /
    /// drag-state. Call sites reach modal-stack and drag-state via
    /// `self.backend.borrow().modal_stack_handle()` and
    /// `drag_state_handle()` (B.5b Stage 11 dropped the alias `Rc`
    /// clones that previously lived at `App.modal_stack` /
    /// `App.drag_state`). The `init` drain timer holds a clone and
    /// pumps `poll_events()` every 16 ms.
    backend: Rc<RefCell<backend::GtkBackend>>,
}

/// Decode an activity bar widget ID into a panel ID for `Msg::SwitchPanel`.
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
        "Tab" | "ISO_Left_Tab" => ("Tab", None),
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
                .or_else(copypasta_ext::try_context)
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

/// Scrollbars and indicators for a single window.
/// The horizontal scrollbar is drawn in Cairo (draw_editor) so it can be
/// pixel-exact in height — GTK's Scrollbar widget enforces theme minimum
/// heights that can't be overridden with CSS.
struct WindowScrollbars {
    vertical: gtk4::Scrollbar,
    cursor_indicator: gtk4::DrawingArea,
}

#[derive(Debug)]
#[allow(dead_code)] // Variants used in later phases
enum Msg {
    /// Carries the key name (e.g. "Escape", "Return", "Left") and the
    /// Unicode character the key maps to (if any), plus modifier state.
    KeyPress {
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        alt: bool,
    },
    /// Notify that a resize happened (triggers redraw).
    Resize,
    /// Mouse click at (x, y) coordinates in drawing area.
    MouseClick {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        /// True if the Alt modifier was held when the mouse button was pressed.
        alt: bool,
    },
    /// Toggle sidebar visibility.
    ToggleSidebar,
    /// Switch to a different sidebar panel.
    SwitchPanel(String),
    /// Open file from sidebar tree view (switches to existing tab or opens new permanent tab).
    /// Used for double-click.
    OpenFileFromSidebar(PathBuf),
    /// Open file in a new split group to the side.
    OpenSide(PathBuf),
    /// Preview file from sidebar tree view (single-click, replaces current preview tab).
    PreviewFileFromSidebar(PathBuf),
    /// Create a new file: (parent_dir, name).
    CreateFile(PathBuf, String),
    /// Create a new folder: (parent_dir, name).
    CreateFolder(PathBuf, String),
    /// Start inline new-file creation in the explorer tree under the given dir.
    StartInlineNewFile(PathBuf),
    /// Start inline new-folder creation in the explorer tree under the given dir.
    StartInlineNewFolder(PathBuf),
    /// Explorer CRUD action triggered by keyboard shortcut (the char string).
    ExplorerAction(String),
    ExplorerActivateSelected,
    /// Key press routed to the explorer DrawingArea (Phase A.2b-2).
    ExplorerKey {
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
    },
    /// Left-click at (x, y) on the explorer DrawingArea. `n_press` is 1 for
    /// single-click (preview), 2+ for double-click (open permanent / toggle dir).
    ExplorerClick {
        x: f64,
        y: f64,
        n_press: i32,
    },
    /// Right-click at (x, y) on the explorer DrawingArea — opens the context menu.
    ExplorerRightClick {
        x: f64,
        y: f64,
    },
    /// #426: click on the ctx-menu overlay DA (window coords). Routed
    /// through the overlay's gesture so the menu can extend past the
    /// explorer's right edge into the editor area.
    ExplorerCtxMenuClick(f64, f64),
    /// #426: mouse motion on the ctx-menu overlay DA (window coords).
    /// Updates the engine's `context_menu.selected` from the cached layout.
    ExplorerCtxMenuMotion(f64, f64),
    /// Mouse-wheel on the explorer DrawingArea. Positive dy scrolls down.
    ExplorerScroll(f64),
    /// UiEvent (scroll, mouse) on the explorer DrawingArea — routed
    /// through TreeController.handle() for scrollbar interaction.
    ExplorerUiEvent(quadraui::UiEvent),
    /// Show confirmation dialog before deleting.
    ConfirmDeletePath(PathBuf),
    /// Refresh the file tree from current working directory.
    RefreshFileTree,
    /// Focus the explorer panel (Ctrl-Shift-E).
    FocusExplorer,
    /// Toggle focus between explorer and editor.
    ToggleFocusExplorer,
    /// Toggle focus between search panel and editor.
    ToggleFocusSearch,
    /// Focus the editor (Escape from tree).
    FocusEditor,
    /// Vertical scrollbar value changed.
    VerticalScrollbarChanged {
        window_id: core::WindowId,
        value: f64,
    },
    /// Horizontal scrollbar value changed.
    HorizontalScrollbarChanged {
        window_id: core::WindowId,
        value: f64,
    },
    /// Cache font metrics (line_height, char_width) from draw_editor.
    CacheFontMetrics(f64, f64),
    /// Open settings.json in editor.
    OpenSettingsFile,
    /// Settings file changed on disk.
    SettingsFileChanged,
    /// Window size changed.
    WindowResized {
        width: i32,
        height: i32,
    },
    /// Window closing (save session state).
    WindowClosing {
        width: i32,
        height: i32,
    },
    /// Sidebar was resized via drag handle — save new width.
    SidebarResized,
    /// Project search input text changed (query update, no search yet).
    ProjectSearchQueryChanged(String),
    /// User pressed Enter in the project search box — run the search.
    ProjectSearchSubmit,
    /// User clicked/activated a search result by index — open the file.
    ProjectSearchOpenResult(usize),
    /// Periodic tick to poll for background search results.
    SearchPollTick,
    /// Toggle case-sensitive project search.
    ProjectSearchToggleCase,
    /// Toggle whole-word project search.
    ProjectSearchToggleWholeWord,
    /// Toggle regex project search.
    ProjectSearchToggleRegex,
    /// Project replace input text changed.
    ProjectReplaceTextChanged(String),
    /// User clicked "Replace All" button — run replace across files.
    ProjectReplaceAll,
    SearchPanelClick(f64, f64),
    SearchPanelKey(String, Option<char>),
    /// Mouse scroll wheel on editor drawing area.
    MouseScroll {
        delta_x: f64,
        delta_y: f64,
    },
    /// Ctrl+Click — plant a secondary cursor at the clicked buffer position.
    CtrlMouseClick {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Mouse double-click at (x, y) coordinates in drawing area.
    MouseDoubleClick {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Mouse drag to (x, y) coordinates in drawing area.
    MouseDrag {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Mouse button released in editor.
    MouseUp,
    /// Rename a file: (old_path, new_name_without_dir)
    RenameFile(PathBuf, String),
    /// Move a file to a different directory: (src, dest_dir)
    MoveFile(PathBuf, PathBuf),
    /// Copy the file path to the clipboard.
    CopyPath(PathBuf),
    /// Copy the relative file path to the clipboard.
    CopyRelativePath(PathBuf),
    /// Remember this file as the "left side" for a two-way diff.
    SelectForDiff(PathBuf),
    /// Open a vsplit diff: current file is right side, stored path is left.
    DiffWithSelected(PathBuf),
    /// GDK clipboard text arrived for pasting into command/search/insert input.
    ClipboardPasteToInput {
        text: String,
    },
    /// Toggle the integrated terminal panel open/closed.
    ToggleTerminal,
    /// Toggle the "terminal maximized" state (panel fills editor area).
    ToggleTerminalMaximize,
    /// Open a new terminal tab at a specific directory.
    OpenTerminalAt(PathBuf),
    /// Open a new terminal tab.
    NewTerminalTab,
    /// Run a command in a visible terminal pane (for installs).
    RunCommandInTerminal(String),
    /// Switch to a specific terminal tab by index.
    TerminalSwitchTab(usize),
    /// Close the active terminal tab (closes panel if last tab).
    TerminalCloseActiveTab,
    /// Kill the terminal process and close the panel.
    TerminalKill,
    /// Toggle horizontal split view (two panes side-by-side).
    TerminalToggleSplit,
    /// Set keyboard focus to a specific split pane (0=left, 1=right).
    TerminalSplitFocus(usize),
    /// Copy terminal selection to clipboard.
    TerminalCopySelection,
    /// Paste from system clipboard into the terminal PTY.
    TerminalPasteClipboard,
    /// Mouse pressed at terminal cell (row, col).
    TerminalMouseDown {
        row: u16,
        col: u16,
    },
    /// Mouse dragged to terminal cell (row, col).
    TerminalMouseDrag {
        row: u16,
        col: u16,
    },
    /// Mouse released over terminal.
    TerminalMouseUp,
    /// Open the terminal inline find bar.
    TerminalFindOpen,
    /// Close the terminal inline find bar.
    TerminalFindClose,
    /// Type a character into the terminal find bar.
    TerminalFindChar(char),
    /// Delete the last character from the terminal find bar.
    TerminalFindBackspace,
    /// Navigate to the next find match.
    TerminalFindNext,
    /// Navigate to the previous find match.
    TerminalFindPrev,
    /// Toggle the VSCode-style menu bar on/off.
    ToggleMenuBar,
    /// Dispatch a menu action by command string (from MenuSystem::Activated).
    HandleMenuAction(String),
    /// MenuSystem state changed — sync overlay visibility and redraw.
    MenuRedraw,
    /// Navigate back in MRU tab history.
    MruNavBack,
    /// Navigate forward in MRU tab history.
    MruNavForward,
    /// Open the Command Center picker (search box click).
    OpenCommandCenter,
    /// Click in the debug sidebar DrawingArea (x, y coordinates in pixels).
    DebugSidebarClick(f64, f64),
    /// Drag motion in the debug sidebar (absolute x, y from GestureDrag).
    DebugSidebarDrag(f64, f64),
    /// Drag end in the debug sidebar (absolute x, y).
    DebugSidebarDragEnd(f64, f64),
    /// Key press in the debug sidebar DrawingArea.
    DebugSidebarKey(String, bool),
    /// Scroll in the debug sidebar DrawingArea (dy value from EventControllerScroll).
    DebugSidebarScroll(f64),
    /// Click in the Source Control sidebar DrawingArea (x, y coordinates in pixels).
    ScSidebarClick(f64, f64, i32),
    /// Mouse motion in the Source Control sidebar DrawingArea (x, y).
    ScSidebarMotion(f64, f64),
    /// Key press in the Source Control sidebar DrawingArea.
    ScKey(String, bool),
    /// UiEvent (scroll, mouse) in the SC sidebar DrawingArea.
    ScSidebarEvent(quadraui::UiEvent),
    SearchSidebarEvent(quadraui::UiEvent),
    /// Key press in the Extensions sidebar DrawingArea (key_name, unicode).
    ExtSidebarKey(String, Option<char>),
    ExtSidebarEvent(quadraui::UiEvent),
    /// Key press in the Settings sidebar DrawingArea (key_name, ctrl, unicode).
    SettingsKey(String, bool, Option<char>),
    /// Click in the Settings sidebar DrawingArea (x, y, n_press).
    SettingsClick(f64, f64, i32),
    /// Scroll wheel in the Settings sidebar DrawingArea (dy).
    SettingsScroll(f64),
    /// Key press in an extension-provided panel DrawingArea (e.g. git-insights).
    ExtPanelKey(String, Option<char>),
    /// Click in an extension-provided panel DrawingArea (x, y, n_press).
    ExtPanelClick(f64, f64, i32),
    /// Right-click in an extension-provided panel DrawingArea (x, y).
    ExtPanelRightClick(f64, f64),
    /// Mouse motion in an extension-provided panel DrawingArea (x, y).
    ExtPanelMouseMove(f64, f64),
    /// Scroll in an extension-provided panel DrawingArea (dy).
    ExtPanelScroll(f64),
    /// Click on the panel hover popup overlay (x, y in window coords).
    PanelHoverClick(f64, f64),
    /// Key press in the AI sidebar DrawingArea.
    AiSidebarKey(String, bool, Option<char>),
    /// Click in the AI sidebar DrawingArea (x, y).
    AiSidebarClick(f64, f64),
    /// Minimize the application window.
    WindowMinimize,
    /// Maximize or restore the application window.
    WindowMaximize,
    /// Close the application window.
    WindowClose,
    /// Show a native "Open File" dialog.
    OpenFileDialog,
    /// Show a native "Open Folder" dialog.
    OpenFolderDialog,
    /// Show a native "Open Workspace" dialog.
    OpenWorkspaceDialog,
    /// Show a native "Save Workspace As" dialog.
    SaveWorkspaceAsDialog,
    /// Show a "Open Recent" picker.
    OpenRecentDialog,
    /// User triggered quit from menu/close-button; check for unsaved changes.
    ShowQuitConfirm,
    /// User confirmed quit (after saving or choosing to discard changes).
    QuitConfirmed,
    /// Clear the yank highlight after the flash duration has elapsed.
    ClearYankHighlight,
    /// User clicked ✕ on a tab with unsaved changes — ask what to do.
    ShowCloseTabConfirm,
    /// User responded to the close-tab unsaved-changes dialog.
    CloseTabConfirmed {
        save: bool,
    },
    /// A setting was changed via the Settings sidebar form widget.
    SettingChanged {
        key: String,
        value: String,
    },
    /// Open a buffer editor for the named setting key (e.g. "keymaps", "extension_registries").
    OpenBufferEditor(String),
    /// Alt key released — confirm tab switcher if open.
    TabSwitcherRelease,
    /// Right-click on a tab in the tab bar: (group_id, tab_idx, pixel x, pixel y).
    TabRightClick {
        group_id: core::window::GroupId,
        tab_idx: usize,
        x: f64,
        y: f64,
    },
    /// Right-click on the editor area (buffer text).
    EditorRightClick {
        x: f64,
        y: f64,
    },
}

/// Reposition existing scrollbar widgets for the given drawing-area size.
///
/// This is a free function so it can be called both from `sync_scrollbar` (via
/// Relm4's message queue) AND from a `connect_resize` callback that runs
/// synchronously during GTK's layout pass — before each frame is rendered.
/// Calling it synchronously eliminates the 1-frame lag where the editor draws
/// at the new size while scrollbars are still at the old position.
///
/// It only updates widget geometry; it does NOT create/remove scrollbars or
/// update adjustment values (that is `sync_scrollbar`'s job).
#[allow(clippy::too_many_arguments)]
fn sync_scrollbar_positions(
    da_width: f64,
    da_height: f64,
    line_height: f64,
    _char_width: f64,
    engine: &core::Engine,
    scrollbars: &HashMap<core::WindowId, WindowScrollbars>,
) {
    if da_width < 20.0 || da_height < 20.0 || line_height < 1.0 {
        return;
    }
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };
    let editor_bounds = core::WindowRect::new(
        0.0,
        0.0,
        da_width,
        gtk_editor_bottom(engine, da_width, da_height, line_height),
    );
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(editor_bounds, tab_bar_height);

    // Hide scrollbars for windows not in the current visible set
    // (e.g. windows in non-active tabs), or when a modal popup is
    // open. Native gtk4::Scrollbar widgets render above the
    // DrawingArea, so they would otherwise poke through the
    // palette / picker / tab-switcher overlays.
    let visible_ids: std::collections::HashSet<core::WindowId> =
        window_rects.iter().map(|(wid, _)| *wid).collect();
    let modal_open = engine.is_blocking_modal_open();
    for (wid, ws) in scrollbars.iter() {
        let show = visible_ids.contains(wid) && !modal_open;
        ws.vertical.set_visible(show);
        ws.cursor_indicator.set_visible(show);
    }

    for (window_id, rect) in &window_rects {
        let ws = match scrollbars.get(window_id) {
            Some(ws) => ws,
            None => continue,
        };
        let window = match engine.windows.get(window_id) {
            Some(w) => w,
            None => continue,
        };
        if engine.buffer_manager.get(window.buffer_id).is_none() {
            continue;
        }

        // — Vertical scrollbar —
        // Query the actual allocated width so we position correctly even if
        // GTK's theme enforces a minimum wider than our CSS min-width.
        // Inset 2px from the right edge so the scrollbar doesn't visually
        // overlap the group divider or the adjacent group's space.
        let sb_actual_w = ws.vertical.width().max(4) as f64;
        ws.vertical.set_halign(gtk4::Align::Start);
        ws.vertical.set_valign(gtk4::Align::Start);
        ws.vertical
            .set_margin_start(rect.x as i32 + (rect.width - sb_actual_w) as i32 - 2);
        ws.vertical.set_margin_top(rect.y as i32);
        ws.vertical
            .set_height_request((rect.height as i32 - 4).max(0));

        // Horizontal scrollbar is drawn in Cairo by draw_editor — nothing to do here.
    }
}

/// Create a new `App` instance.
///
/// All widget-dependent setup (window handle, CSS) is deferred to
/// `ShellApp::setup()`, called by the runner once the window exists.
impl App {
    fn new(file_path: Option<PathBuf>) -> Self {
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
            icons::set_nerd_fonts(e.settings.use_nerd_fonts);
            e.startup(file_path.as_deref());
            e
        };
        setup_gtk_clipboard(&mut engine);

        let initial_theme = Theme::from_name(&engine.settings.colorscheme);
        let css_provider = load_css(&initial_theme);
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

        let sender = MsgSender::new();

        // File watcher for settings.json hot-reload.
        let settings_path = std::env::var("HOME")
            .map(|h| format!("{}/.config/vimcode/settings.json", h))
            .unwrap_or_else(|_| ".config/vimcode/settings.json".to_string());
        let file = gio::File::for_path(&settings_path);
        let settings_monitor =
            match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
                Ok(monitor) => {
                    let sender_for_monitor = sender.clone();
                    monitor.connect_changed(move |_, _, _, event| {
                        if event == gio::FileMonitorEvent::ChangesDoneHint {
                            sender_for_monitor.send(Msg::SettingsFileChanged).ok();
                        }
                    });
                    Some(monitor)
                }
                Err(_) => None,
            };

        let backend = Rc::new(RefCell::new(backend::GtkBackend::new()));

        App {
            engine,
            draw_needed: Rc::new(Cell::new(false)),
            explorer_sidebar_da_ref: Rc::new(RefCell::new(None)),
            activity_bar_da_ref: Rc::new(RefCell::new(None)),
            explorer_row_height_cell: Rc::new(Cell::new(28.0)),
            explorer_line_height_cell: Rc::new(Cell::new(20.0)),
            explorer_char_width_cell: Rc::new(Cell::new(8.0)),
            explorer_ctx_menu_layout: Rc::new(RefCell::new(None)),
            ctx_menu_overlay_da: Rc::new(RefCell::new(None)),
            explorer_scroll_accum: Rc::new(Cell::new(0.0)),
            drawing_area: Rc::new(RefCell::new(None)),
            menu_bar_da: Rc::new(RefCell::new(None)),
            debug_sidebar_da_ref: Rc::new(RefCell::new(None)),
            debug_sidebar_lh: Rc::new(Cell::new(20.0)),
            git_sidebar_da_ref: Rc::new(RefCell::new(None)),
            ext_sidebar_da_ref: Rc::new(RefCell::new(None)),
            ext_dyn_panel_da_ref: Rc::new(RefCell::new(None)),
            ext_dyn_panel_box: Rc::new(RefCell::new(None)),
            ai_sidebar_da_ref: Rc::new(RefCell::new(None)),
            sidebar_inner_sw: Rc::new(RefCell::new(None)),
            sidebar_revealer: Rc::new(RefCell::new(None)),
            explorer_panel_box: Rc::new(RefCell::new(None)),
            search_sidebar_da_ref: Rc::new(RefCell::new(None)),
            debug_panel_box: Rc::new(RefCell::new(None)),
            git_panel_box: Rc::new(RefCell::new(None)),
            ext_panel_box: Rc::new(RefCell::new(None)),
            settings_panel_box: Rc::new(RefCell::new(None)),
            settings_da_ref: Rc::new(RefCell::new(None)),
            ai_panel_box_ref: Rc::new(RefCell::new(None)),
            window_scrollbars: Rc::new(RefCell::new(HashMap::new())),
            overlay: Rc::new(RefCell::new(None)),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
            last_editor_pointer: Rc::new(Cell::new(None)),
            cached_ui_line_height: 20.0,
            dialog_btn_rects: Rc::new(RefCell::new(Vec::new())),
            line_height_cell: Rc::new(Cell::new(24.0)),
            char_width_cell: Rc::new(Cell::new(9.0)),
            mouse_pos_cell: Rc::new(Cell::new((-1.0, -1.0))),
            h_sb_hovered_cell: Rc::new(Cell::new(false)),
            tab_close_hover_cell: Rc::new(Cell::new(None)),
            h_sb_drag_cell: Rc::new(Cell::new(None)),
            fr_input_dragging: false,
            settings_monitor,
            sender,
            last_clipboard_content: None,
            h_sb_hovered: false,
            tab_close_hover: None,
            tab_slot_positions: Rc::new(RefCell::new(HashMap::new())),
            tab_close_bounds: Rc::new(RefCell::new(HashMap::new())),
            cached_tab_pixel_hits: Rc::new(RefCell::new(HashMap::new())),
            diff_btn_map: Rc::new(RefCell::new(HashMap::new())),
            split_btn_map: Rc::new(RefCell::new(HashMap::new())),
            action_btn_map: Rc::new(RefCell::new(HashMap::new())),
            status_segment_map: Rc::new(RefCell::new(HashMap::new())),
            cached_screen_layout: Rc::new(RefCell::new(None)),
            cached_drop_groups: Rc::new(RefCell::new(Vec::new())),
            cached_drop_tbh: Rc::new(Cell::new(0.0)),
            cached_explorer_metrics: Rc::new(Cell::new((16.0, 8.0))),
            debug_toolbar_y_offset: Rc::new(Cell::new(0.0)),
            debug_toolbar_height: Rc::new(Cell::new(0.0)),
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            group_divider_dragging: None,
            tab_dragging: false,
            tab_drag_start: None,
            tab_drag_source: None,
            tab_drag_drop_zone: core::window::DropZone::None,
            window: None,
            last_sc_refresh: std::time::Instant::now(),
            last_tree_indicator_update: std::time::Instant::now(),
            menu_dropdown_da: Rc::new(RefCell::new(None)),
            panel_hover_da: Rc::new(RefCell::new(None)),
            panel_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            panel_hover_popup_rect: Rc::new(Cell::new(None)),
            editor_hover_popup_rect: Rc::new(Cell::new(None)),
            completion_layout: Rc::new(RefCell::new(None)),
            context_menu_layout: Rc::new(RefCell::new(None)),
            tab_switcher_popup_rect: Rc::new(Cell::new(None)),
            dialog_popup_rect: Rc::new(Cell::new(None)),
            editor_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            editor_hover_scrollbar: Rc::new(Cell::new(None)),
            menu_dd_line_height: Rc::new(Cell::new(24.0)),
            css_provider,
            last_colorscheme,
            backend,
        }
    }
}

impl App {
    fn dispatch(&mut self, msg: Msg) {
        // Track if this is a scrollbar change to avoid syncing feedback loop
        let is_scrollbar_msg = matches!(
            &msg,
            Msg::VerticalScrollbarChanged { .. } | Msg::HorizontalScrollbarChanged { .. }
        );

        match msg {
            Msg::KeyPress {
                key_name,
                unicode,
                ctrl,
                alt,
            } => {
                self.handle_key_press(key_name, unicode, ctrl, alt);
            }
            Msg::ClearYankHighlight => {
                self.engine.borrow_mut().clear_yank_highlight();
                self.draw_needed.set(true);
            }
            Msg::TabRightClick {
                group_id,
                tab_idx,
                x,
                y,
            } => {
                let cw = self.cached_char_width.max(1.0);
                let lh = self.cached_line_height.max(1.0);
                let cx = (x / cw) as u16;
                let cy = (y / lh) as u16;
                self.engine
                    .borrow_mut()
                    .open_tab_context_menu(group_id, tab_idx, cx, cy);
                self.draw_needed.set(true);
            }
            Msg::TabSwitcherRelease => {
                // Handled directly by the root EventControllerKey release handler.
                // Kept as a no-op for exhaustive match.
            }
            Msg::EditorRightClick { x, y } => {
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
            Msg::Resize => {
                // Update backend viewport for MenuSystem::handle() calls.
                if let Some(ref overlay) = *self.overlay.borrow() {
                    use quadraui::Backend;
                    self.backend
                        .borrow_mut()
                        .begin_frame(quadraui::Viewport::new(
                            overlay.width().max(1) as f32,
                            overlay.height().max(1) as f32,
                            1.0,
                        ));
                }
                // Propagate window resize to open terminal panes.
                if !self.engine.borrow().terminal_panes.is_empty() {
                    if let Some(da) = self.drawing_area.borrow().as_ref() {
                        let cols = ((da.width() as f64 / self.cached_char_width) as u16).max(40);
                        let rows = self.engine.borrow().session.terminal_panel_rows;
                        self.engine.borrow_mut().terminal_resize(cols, rows);
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::MouseClick {
                x,
                y,
                width,
                height,
                alt,
            } => {
                self.handle_mouse_click_msg(x, y, width, height, alt);
            }
            Msg::CtrlMouseClick {
                x,
                y,
                width: _,
                height: _,
            } => {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    if !engine.picker_open {
                        let editor_pl = self.editor_pango_layout(&engine);
                        if let ClickTarget::BufferPos(_, line, col) = pixel_to_click_target(
                            &mut engine,
                            x,
                            y,
                            self.cached_line_height,
                            self.cached_char_width,
                            &editor_pl,
                            layout,
                            &self.cached_tab_pixel_hits.borrow(),
                            &self.tab_slot_positions.borrow(),
                            &self.diff_btn_map.borrow(),
                            &self.split_btn_map.borrow(),
                            &self.action_btn_map.borrow(),
                            &self.status_segment_map.borrow(),
                        ) {
                            engine.add_cursor_at_pos(line, col);
                        }
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::MouseDoubleClick {
                x,
                y,
                width: _,
                height: _,
            } => {
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
                    let mut bc_handled = false;
                    if engine.settings.breadcrumbs {
                        let lh = self.cached_line_height.max(1.0);
                        let cw = self.cached_char_width.max(1.0);
                        if y >= lh && y < lh * 2.0 {
                            let segments =
                                crate::render::build_breadcrumbs_for_active_group(&engine);
                            let sep_w = " › ".chars().count() as f64 * cw;
                            let mut seg_x = cw;
                            for seg in &segments {
                                let label_w = seg.label.chars().count() as f64 * cw;
                                if x >= seg_x && x < seg_x + label_w {
                                    engine.breadcrumb_double_click(
                                        seg.is_symbol,
                                        seg.path_prefix.as_deref(),
                                        seg.symbol_line,
                                    );
                                    bc_handled = true;
                                    break;
                                }
                                seg_x += label_w + sep_w;
                            }
                        }
                    }
                    if !bc_handled {
                        let editor_pl = self.editor_pango_layout(&engine);
                        let layout_ref = self.cached_screen_layout.borrow();
                        if let Some(ref layout) = *layout_ref {
                            handle_mouse_double_click(
                                &mut engine,
                                x,
                                y,
                                self.cached_line_height,
                                self.cached_char_width,
                                &editor_pl,
                                layout,
                                &self.cached_tab_pixel_hits.borrow(),
                                &self.tab_slot_positions.borrow(),
                                &self.diff_btn_map.borrow(),
                                &self.split_btn_map.borrow(),
                                &self.action_btn_map.borrow(),
                                &self.status_segment_map.borrow(),
                            );
                        }
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::MouseDrag {
                x,
                y,
                width,
                height,
            } => {
                self.handle_mouse_drag_msg(x, y, width, height);
            }
            Msg::MouseUp => {
                self.handle_mouse_up_msg();
            }
            Msg::ToggleSidebar | Msg::SwitchPanel(_) => {
                self.handle_sidebar_panel_msg(msg);
            }
            Msg::OpenFileFromSidebar(_)
            | Msg::OpenSide(_)
            | Msg::PreviewFileFromSidebar(_)
            | Msg::CreateFile(_, _)
            | Msg::CreateFolder(_, _)
            | Msg::StartInlineNewFile(_)
            | Msg::StartInlineNewFolder(_)
            | Msg::ExplorerAction(_)
            | Msg::ExplorerActivateSelected
            | Msg::ConfirmDeletePath(_)
            | Msg::RefreshFileTree
            | Msg::FocusExplorer
            | Msg::ToggleFocusExplorer
            | Msg::ToggleFocusSearch
            | Msg::FocusEditor => {
                self.handle_explorer_msg(msg);
            }
            Msg::VerticalScrollbarChanged { window_id, value } => {
                // Update specific window's scroll_top based on scrollbar value
                let mut engine = self.engine.borrow_mut();
                // For now, only scroll if it's the active window
                if engine.active_window_id() == window_id {
                    engine.set_scroll_top(value.round() as usize);
                    engine.sync_scroll_binds();
                }
                drop(engine);
                self.draw_needed.set(true);
            }
            Msg::HorizontalScrollbarChanged { window_id, value } => {
                let mut engine = self.engine.borrow_mut();
                engine.set_scroll_left_for_window(window_id, value.round() as usize);
                drop(engine);
                self.draw_needed.set(true);
            }
            Msg::MouseScroll { delta_x, delta_y } => {
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
                                    if let Some(da) = self.drawing_area.borrow().as_ref() {
                                        da.queue_draw();
                                    }
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
                let hovered_window_id = self.last_editor_pointer.get().and_then(|(x, y)| {
                    if let Some(da) = self.drawing_area.borrow().as_ref() {
                        let width = da.width() as f64;
                        let height = da.height() as f64;
                        let line_height = self.cached_line_height.max(1.0);
                        let editor_bottom = gtk_editor_bottom(&engine, width, height, line_height);
                        let tab_bar_height =
                            render::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
                        let editor_bounds = core::WindowRect::new(0.0, 0.0, width, editor_bottom);
                        let (rects, _) =
                            engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
                        rects
                            .iter()
                            .find(|(_, r)| {
                                x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
                            })
                            .map(|(id, _)| *id)
                    } else {
                        None
                    }
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
            Msg::CacheFontMetrics(line_height, char_width) => {
                let old_char_width = self.cached_char_width;
                self.cached_line_height = line_height;
                self.cached_char_width = char_width;
                // #270 lift: keep the lifted `GtkBackend`'s settings-driven
                // fields in sync with current settings. Cheap (bool +
                // small String) and runtime toggles (`:set nonerdfonts`,
                // `:set guifont=…`) propagate without a restart.
                {
                    use quadraui::Backend;
                    let e = self.engine.borrow();
                    let mut b = self.backend.borrow_mut();
                    b.set_nerd_fonts(e.settings.use_nerd_fonts);
                    b.set_ui_font(format!(
                        "{} {}",
                        UI_FONT_FAMILY,
                        e.settings.ui_font_size.max(1)
                    ));
                }
                // Compute UI font line height for sidebar click handlers.
                if let Some(ref da) = *self.drawing_area.borrow() {
                    let font_desc = FontDescription::from_string(&UI_FONT());
                    let pango_ctx = da.pango_context();
                    let fm = pango_ctx.metrics(Some(&font_desc), None);
                    self.cached_ui_line_height =
                        (fm.ascent() + fm.descent()) as f64 / pango::SCALE as f64;
                    let lh = self.cached_ui_line_height as f32;
                    let metrics = quadraui::MsvLayoutMetrics {
                        header_size: (lh * 1.2).round(),
                        divider_size: 0.0,
                        scrollbar_size: 8.0,
                        cell_quantum: 0.0,
                    };
                    self.engine
                        .borrow()
                        .ext_sidebar_system
                        .borrow_mut()
                        .set_backend_info(lh, metrics);
                    self.engine
                        .borrow()
                        .sc_sidebar_system
                        .borrow_mut()
                        .set_backend_info(lh, metrics);
                    self.engine
                        .borrow()
                        .search_sidebar_system
                        .borrow_mut()
                        .set_backend_info(lh, metrics);
                }
                // Keep shared cells in sync so the resize callback can use accurate values.
                self.line_height_cell.set(line_height);
                self.char_width_cell.set(char_width);
                // Keep menu dropdown overlay in sync with current line height.
                self.menu_dd_line_height.set(line_height);
                // Sync menu bar height to font metrics
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    if self.engine.borrow().menu_bar_visible {
                        da.set_height_request(line_height as i32);
                    }
                }
                // If cached_char_width changed significantly (e.g. on first draw after startup
                // when the initial default of 9.0 differed from the actual font metric),
                // resize any open terminal panes so their PTY col count matches the display.
                if (old_char_width - char_width).abs() > 0.5
                    && !self.engine.borrow().terminal_panes.is_empty()
                {
                    if let Some(da) = self.drawing_area.borrow().as_ref() {
                        let cols = ((da.width() as f64 / char_width) as u16).max(40);
                        let rows = self.engine.borrow().session.terminal_panel_rows;
                        self.engine.borrow_mut().terminal_resize(cols, rows);
                    }
                }
                // Sync per-window viewport_cols from paint-time geometry
                // so ensure_cursor_visible (run during key handling) uses
                // exact column counts, not the resize handler's estimate.
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
            }
            Msg::OpenSettingsFile => {
                let settings_path = std::env::var("HOME")
                    .map(|h| format!("{}/.config/vimcode/settings.json", h))
                    .unwrap_or_else(|_| ".config/vimcode/settings.json".to_string());

                let mut engine = self.engine.borrow_mut();
                // Open settings in a new tab
                engine.new_tab(Some(Path::new(&settings_path)));
                drop(engine);
                self.draw_needed.set(true);
            }
            Msg::SettingsFileChanged => {
                if self.engine.borrow_mut().check_settings_reload() {
                    if let Some(drawing_area) = self.drawing_area.borrow().as_ref() {
                        drawing_area.queue_draw();
                    }
                    self.dispatch(Msg::RefreshFileTree);
                    self.draw_needed.set(true);
                }
            }
            Msg::SettingChanged { key, value } => {
                let mut engine = self.engine.borrow_mut();
                if engine.settings.set_value_str(&key, &value).is_ok() {
                    if let Err(e) = engine.settings.save() {
                        engine.message = format!("Warning: setting changed but not saved: {e}");
                    }
                    // No flag to set — `Settings::save` bumps the global save
                    // revision; SettingsFileChanged consults it directly.
                }
                drop(engine);
                if key == "show_hidden_files" {
                    self.dispatch(Msg::RefreshFileTree);
                }
                self.draw_needed.set(true);
            }
            Msg::OpenBufferEditor(key) => {
                let mut engine = self.engine.borrow_mut();
                match key.as_str() {
                    "keymaps" => engine.open_keymaps_editor(),
                    "extension_registries" => engine.open_registries_editor(),
                    _ => {}
                }
                drop(engine);
                self.draw_needed.set(true);
            }
            Msg::WindowResized { .. } | Msg::SidebarResized => {
                self.handle_find_replace_msg(msg);
            }
            Msg::ProjectSearchQueryChanged(q) => {
                self.engine.borrow_mut().project_search_query = q;
            }
            Msg::ProjectSearchToggleCase => {
                self.engine.borrow_mut().toggle_project_search_case();
                self.draw_needed.set(true);
            }
            Msg::ProjectSearchToggleWholeWord => {
                self.engine.borrow_mut().toggle_project_search_whole_word();
                self.draw_needed.set(true);
            }
            Msg::ProjectSearchToggleRegex => {
                self.engine.borrow_mut().toggle_project_search_regex();
                self.draw_needed.set(true);
            }
            Msg::ProjectReplaceTextChanged(t) => {
                self.engine.borrow_mut().project_replace_text = t;
            }
            Msg::ProjectReplaceAll => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                self.engine.borrow_mut().start_project_replace(cwd);
                self.draw_needed.set(true);
            }
            Msg::ProjectSearchSubmit => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                self.engine.borrow_mut().start_project_search(cwd);
                self.draw_needed.set(true);
            }
            Msg::SearchPollTick => {
                self.handle_poll_tick();
            }
            Msg::ProjectSearchOpenResult(idx) => {
                let result = self
                    .engine
                    .borrow()
                    .project_search_results
                    .get(idx)
                    .map(|m| (m.file.clone(), m.line));
                if let Some((file, line)) = result {
                    self.engine.borrow_mut().open_file_in_tab(&file);
                    // Jump cursor to the matched line
                    let win_id = self.engine.borrow().active_window_id();
                    self.engine
                        .borrow_mut()
                        .set_cursor_for_window(win_id, line, 0);
                    self.engine.borrow_mut().ensure_cursor_visible();
                }
                self.draw_needed.set(true);
            }
            Msg::SearchPanelClick(_, _) | Msg::SearchPanelKey(_, _) => {
                // Superseded by SearchSidebarEvent via wire_da_events
            }
            Msg::SearchSidebarEvent(ev) => {
                self.engine.borrow_mut().handle_search_sidebar_ui_event(ev);
                self.draw_needed.set(true);
                if let Some(ref da) = *self.search_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
            }
            Msg::RenameFile(_, _)
            | Msg::MoveFile(_, _)
            | Msg::CopyPath(_)
            | Msg::CopyRelativePath(_)
            | Msg::SelectForDiff(_)
            | Msg::DiffWithSelected(_)
            | Msg::ClipboardPasteToInput { .. }
            | Msg::WindowClosing { .. } => {
                self.handle_file_ops_msg(msg);
            }
            Msg::ToggleTerminal
            | Msg::ToggleTerminalMaximize
            | Msg::OpenTerminalAt(_)
            | Msg::NewTerminalTab
            | Msg::RunCommandInTerminal(_)
            | Msg::TerminalSwitchTab(_)
            | Msg::TerminalCloseActiveTab
            | Msg::TerminalKill
            | Msg::TerminalToggleSplit
            | Msg::TerminalSplitFocus(_)
            | Msg::TerminalCopySelection
            | Msg::TerminalPasteClipboard
            | Msg::TerminalMouseDown { .. }
            | Msg::TerminalMouseDrag { .. }
            | Msg::TerminalMouseUp
            | Msg::TerminalFindOpen
            | Msg::TerminalFindClose
            | Msg::TerminalFindChar(_)
            | Msg::TerminalFindBackspace
            | Msg::TerminalFindNext
            | Msg::TerminalFindPrev => {
                self.handle_terminal_msg(msg);
            }
            Msg::ToggleMenuBar
            | Msg::HandleMenuAction(_)
            | Msg::MenuRedraw
            | Msg::MruNavBack
            | Msg::MruNavForward
            | Msg::OpenCommandCenter => {
                self.handle_menu_msg(msg);
            }
            Msg::DebugSidebarClick(_, _)
            | Msg::DebugSidebarDrag(_, _)
            | Msg::DebugSidebarDragEnd(_, _)
            | Msg::DebugSidebarKey(_, _)
            | Msg::DebugSidebarScroll(_) => {
                self.handle_debug_sidebar_msg(msg);
            }
            Msg::ScSidebarClick(_, _, _)
            | Msg::ScSidebarMotion(_, _)
            | Msg::ScKey(_, _)
            | Msg::ScSidebarEvent(_) => {
                self.handle_sc_sidebar_msg(msg);
            }
            Msg::ExtSidebarKey(_, _) | Msg::ExtSidebarEvent(_) => {
                self.handle_ext_sidebar_msg(msg);
            }
            Msg::SettingsKey(_, _, _) | Msg::SettingsClick(_, _, _) | Msg::SettingsScroll(_) => {
                self.handle_settings_msg(msg);
            }
            Msg::ExplorerKey { .. }
            | Msg::ExplorerClick { .. }
            | Msg::ExplorerRightClick { .. }
            | Msg::ExplorerScroll(_)
            | Msg::ExplorerUiEvent(_)
            | Msg::ExplorerCtxMenuClick(..)
            | Msg::ExplorerCtxMenuMotion(..) => {
                self.handle_explorer_msg(msg);
            }
            Msg::ExtPanelKey(_, _)
            | Msg::ExtPanelClick(_, _, _)
            | Msg::ExtPanelRightClick(_, _)
            | Msg::ExtPanelMouseMove(_, _)
            | Msg::ExtPanelScroll(_)
            | Msg::PanelHoverClick(_, _) => {
                self.handle_ext_panel_msg(msg);
            }
            Msg::AiSidebarKey(_, _, _) | Msg::AiSidebarClick(_, _) => {
                self.handle_ai_sidebar_msg(msg);
            }
            Msg::WindowMinimize
            | Msg::WindowMaximize
            | Msg::WindowClose
            | Msg::OpenFileDialog
            | Msg::OpenFolderDialog
            | Msg::OpenWorkspaceDialog
            | Msg::SaveWorkspaceAsDialog
            | Msg::OpenRecentDialog
            | Msg::ShowQuitConfirm
            | Msg::QuitConfirmed
            | Msg::ShowCloseTabConfirm
            | Msg::CloseTabConfirmed { .. } => {
                self.handle_dialog_msg(msg);
            }
        }

        // Sync scrollbar position to match engine state (except when scrollbar itself changed)
        if !is_scrollbar_msg {
            self.sync_scrollbar();
        }

        // #435: engine-drawn ctx menu keys (j/k/Enter/Esc) are dispatched on
        // the editor DA's key controller. If the trigger click landed on a
        // sibling DA (sidebar, ext panel) the DA never claimed focus, so keys
        // are dead until the user clicks inside the menu. Grab focus whenever
        // a ctx menu is open — grab_focus is idempotent if already focused.
        //
        // #426: do NOT do this for explorer-targeted ctx menus — those render
        // on the explorer DA and have their own key handler. Stealing focus
        // back to the editor DA would break keyboard nav of the explorer menu.
        {
            use core::engine::ContextMenuTarget;
            let on_explorer = matches!(
                self.engine
                    .borrow()
                    .context_menu
                    .as_ref()
                    .map(|cm| &cm.target),
                Some(
                    ContextMenuTarget::ExplorerFile { .. } | ContextMenuTarget::ExplorerDir { .. }
                )
            );
            if !on_explorer && self.engine.borrow().context_menu.is_some() {
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
            }
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

    /// Save the current session state and schedule process exit via idle callback.
    /// Uses `idle_add_local_once` so `process::exit` runs outside any GTK signal
    /// emission chain, avoiding UB from unwinding through extern "C" trampolines.
    fn save_session_and_exit(&self) {
        let mut engine = self.engine.borrow_mut();
        let buffer_id = engine.active_buffer_id();
        if let Some(path) = engine
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.as_deref())
            .map(|p| p.to_path_buf())
        {
            let view = engine.active_window().view.clone();
            engine.session.save_file_position(
                &path,
                view.cursor.line,
                view.cursor.col,
                view.scroll_top,
            );
        }
        engine.session.window.width = self
            .window
            .as_ref()
            .map(|w| w.default_width())
            .unwrap_or(800);
        engine.session.window.height = self
            .window
            .as_ref()
            .map(|w| w.default_height())
            .unwrap_or(600);
        engine.collect_session_open_files();
        if let Some(ref root) = engine.workspace_root.clone() {
            engine.save_session_for_workspace(root);
        }
        let _ = engine.session.save();
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        drop(engine);
        gtk4::glib::idle_add_local_once(|| std::process::exit(0));
    }

    /// Dispatch an `EngineAction` produced by `handle_key` or macro playback.
    ///
    /// `is_macro`: when true, `OpenTerminal` toggles instead of creating a new
    /// tab, and dialog-open actions are suppressed (macros can't drive dialogs).
    fn dispatch_engine_action(&mut self, action: EngineAction, is_macro: bool) {
        match action {
            EngineAction::Quit | EngineAction::SaveQuit => {
                self.save_session_and_exit();
            }
            EngineAction::OpenFile(path) => {
                let mut engine = self.engine.borrow_mut();
                match engine.open_file_with_mode(&path, OpenMode::Permanent) {
                    Ok(()) => {
                        drop(engine);
                        if let Some(ref drawing) = *self.drawing_area.borrow() {
                            drawing.grab_focus();
                        }
                    }
                    Err(e) => {
                        engine.message = e;
                    }
                }
            }
            EngineAction::OpenTerminal => {
                if is_macro {
                    self.dispatch(Msg::ToggleTerminal);
                } else {
                    self.dispatch(Msg::NewTerminalTab);
                }
            }
            EngineAction::ToggleTerminalMaximize => {
                self.dispatch(Msg::ToggleTerminalMaximize);
            }
            EngineAction::RunInTerminal(cmd) => {
                self.dispatch(Msg::RunCommandInTerminal(cmd));
            }
            EngineAction::OpenFolderDialog => {
                if !is_macro {
                    self.dispatch(Msg::OpenFolderDialog);
                }
            }
            EngineAction::OpenWorkspaceDialog => {
                if !is_macro {
                    self.dispatch(Msg::OpenWorkspaceDialog);
                }
            }
            EngineAction::SaveWorkspaceAsDialog => {
                if !is_macro {
                    self.dispatch(Msg::SaveWorkspaceAsDialog);
                }
            }
            EngineAction::OpenRecentDialog => {
                if !is_macro {
                    self.dispatch(Msg::OpenRecentDialog);
                }
            }
            EngineAction::QuitWithUnsaved => {
                self.dispatch(Msg::ShowQuitConfirm);
            }
            EngineAction::ToggleSidebar => {
                // Engine handles this internally; sync local cache.
                self.sync_sidebar_from_engine();
            }
            EngineAction::QuitWithError => {
                let mut engine = self.engine.borrow_mut();
                engine.cleanup_all_swaps();
                engine.lsp_shutdown();
                drop(engine);
                gtk4::glib::idle_add_local_once(|| std::process::exit(1));
            }
            EngineAction::OpenUrl(url) => {
                open_url(&url);
            }
            EngineAction::None | EngineAction::Error => {}
        }
    }

    /// Return focus to the main editor drawing area when a sidebar loses focus.
    fn focus_editor_if_needed(&self, still_focused: bool) {
        if !still_focused {
            if let Some(ref drawing) = *self.drawing_area.borrow() {
                drawing.grab_focus();
            }
        }
    }

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

    /// Rebuild and sync scrollbars for all windows
    fn sync_scrollbar(&self) {
        // Also run the fast positional sync so callers that go through the
        // Relm4 message queue still converge to the right layout.
        if let Some(da) = self.drawing_area.borrow().as_ref() {
            let scrollbars = self.window_scrollbars.borrow();
            let engine = self.engine.borrow();
            sync_scrollbar_positions(
                da.width() as f64,
                da.height() as f64,
                self.cached_line_height,
                self.cached_char_width,
                &engine,
                &scrollbars,
            );
        }
        let overlay = match self.overlay.borrow().as_ref() {
            Some(o) => o.clone(),
            None => return,
        };

        let drawing_area = match self.drawing_area.borrow().as_ref() {
            Some(da) => da.clone(),
            None => return,
        };

        let engine = self.engine.borrow();
        let mut scrollbars = self.window_scrollbars.borrow_mut();

        // Calculate window rects (same logic as draw_editor)
        let da_width = drawing_area.width() as f64;
        let da_height = drawing_area.height() as f64;

        // Skip if the drawing area hasn't been laid out yet (startup / minimised)
        if da_width < 20.0 || da_height < 20.0 {
            return;
        }

        let line_height = self.cached_line_height;
        let tab_row_height = (line_height * 1.6).ceil();
        let tab_bar_height = if engine.settings.breadcrumbs {
            tab_row_height + line_height
        } else {
            tab_row_height
        };
        let editor_bounds = WindowRect::new(
            0.0,
            0.0,
            da_width,
            gtk_editor_bottom(&engine, da_width, da_height, line_height),
        );
        let (window_rects, _dividers) =
            engine.calculate_group_window_rects(editor_bounds, tab_bar_height);

        // Remove scrollbars for windows that no longer exist.
        // Must explicitly remove GTK widgets from the overlay before dropping them,
        // otherwise the widgets remain visible even after the window is gone.
        let dead_ids: Vec<core::WindowId> = scrollbars
            .keys()
            .filter(|wid| !engine.windows.contains_key(*wid))
            .copied()
            .collect();
        for wid in dead_ids {
            if let Some(ws) = scrollbars.remove(&wid) {
                overlay.remove_overlay(&ws.vertical);
                overlay.remove_overlay(&ws.cursor_indicator);
            }
        }

        // Hide scrollbars for windows that exist but aren't visible
        // (e.g. windows in non-active tabs), or when a modal popup is
        // open. Native gtk4::Scrollbar widgets render above the
        // DrawingArea, so they would otherwise poke through the
        // palette / picker / tab-switcher overlays.
        let visible_ids: std::collections::HashSet<core::WindowId> =
            window_rects.iter().map(|(wid, _)| *wid).collect();
        // Native gtk4::Scrollbar widgets render above the DrawingArea
        // (they're real GTK widgets, not Cairo paint), so they'd
        // otherwise poke through every modal popup. Hide them when
        // any popup is up (#252). The single source of truth lives
        // in `Engine::is_blocking_modal_open()`.
        let modal_open = engine.is_blocking_modal_open();
        for (wid, ws) in scrollbars.iter() {
            let show = visible_ids.contains(wid) && !modal_open;
            ws.vertical.set_visible(show);
            ws.cursor_indicator.set_visible(show);
        }

        // Create/update scrollbars for each window
        for (window_id, rect) in &window_rects {
            let window = match engine.windows.get(window_id) {
                Some(w) => w,
                None => continue,
            };

            let buffer_state = match engine.buffer_manager.get(window.buffer_id) {
                Some(s) => s,
                None => continue,
            };

            // Create new scrollbars if needed
            if !scrollbars.contains_key(window_id) {
                let ws = self.create_window_scrollbars(&overlay, *window_id, &self.sender);
                scrollbars.insert(*window_id, ws);
            }

            // Get scrollbars for this window
            let ws = match scrollbars.get(window_id) {
                Some(ws) => ws,
                None => continue,
            };

            // Position and sync vertical scrollbar
            // Use absolute positioning with Start alignment
            ws.vertical.set_halign(gtk4::Align::Start);
            ws.vertical.set_valign(gtk4::Align::Start);

            let scrollbar_x = rect.x as i32 + (rect.width - 10.0) as i32;
            ws.vertical.set_margin_start(scrollbar_x);
            ws.vertical.set_margin_top(rect.y as i32);
            ws.vertical
                .set_height_request(((rect.height - 10.0) as i32).max(0));

            let total_lines = buffer_state.buffer.content.len_lines();
            let v_adj = ws.vertical.adjustment();
            v_adj.set_upper(total_lines as f64);
            v_adj.set_page_size(window.view.viewport_lines as f64);
            // Page-step (trough click) scrolls by one viewport instead
            // of the constructor's hardcoded 10. Without this, clicking
            // the trough always pages by 10 lines regardless of how
            // tall the window is.
            v_adj.set_page_increment(window.view.viewport_lines.max(1) as f64);
            v_adj.set_value(window.view.scroll_top as f64);

            // Position cursor indicator (fix: ensure height stays constant at 4px)
            let cursor_line = window.view.cursor.line;
            if total_lines > 0 {
                let ratio = cursor_line as f64 / total_lines as f64;

                // Calculate Y position within the scrollbar's visible area
                // Use the vertical scrollbar's actual height
                let scrollbar_height = ws.vertical.height() as f64;
                let indicator_y = rect.y + (ratio * scrollbar_height);

                let sb_w = ws.vertical.width().max(4) as f64;
                let indicator_x = rect.x as i32 + (rect.width - sb_w) as i32;
                ws.cursor_indicator.set_margin_start(indicator_x);
                ws.cursor_indicator.set_margin_top(indicator_y as i32);

                // Ensure size stays fixed (defensive coding)
                ws.cursor_indicator.set_width_request(sb_w as i32);
                ws.cursor_indicator.set_height_request(4);
            }
        }
        // Horizontal scrollbar is drawn in Cairo by draw_h_scrollbars() in draw_editor().

        // Remove overlay widgets for deleted windows
        // (GTK will automatically remove them when we drop the references)
    }

    /// Create scrollbars and indicator for a window
    fn create_window_scrollbars(
        &self,
        overlay: &gtk4::Overlay,
        window_id: core::WindowId,
        sender: &MsgSender,
    ) -> WindowScrollbars {
        // Vertical scrollbar — interactive for click-to-jump and drag.
        let v_adj = gtk4::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 20.0);
        let vertical = gtk4::Scrollbar::new(gtk4::Orientation::Vertical, Some(&v_adj));
        vertical.set_width_request(4);
        vertical.set_hexpand(false);
        vertical.set_vexpand(false);
        vertical.set_overflow(gtk4::Overflow::Hidden);

        // Cursor indicator
        let cursor_indicator = gtk4::DrawingArea::new();
        cursor_indicator.set_width_request(4);
        cursor_indicator.set_height_request(4);
        cursor_indicator.set_can_target(false);
        cursor_indicator.set_halign(gtk4::Align::Start);
        cursor_indicator.set_valign(gtk4::Align::Start);
        cursor_indicator.set_hexpand(false);
        cursor_indicator.set_vexpand(false);
        let thumb_color = {
            let engine = self.engine.borrow();
            Theme::from_name(&engine.settings.colorscheme).scrollbar_thumb
        };
        cursor_indicator.set_draw_func(move |_, cr, w, h| {
            let (r, g, b) = thumb_color.to_cairo();
            cr.set_source_rgba(r, g, b, 0.8);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
        });

        // Add to overlay
        overlay.add_overlay(&vertical);
        overlay.add_overlay(&cursor_indicator);

        vertical.show();
        cursor_indicator.show();

        // Connect vertical scrollbar signal
        let sender_v = sender.clone();
        v_adj.connect_value_changed(move |adj| {
            sender_v
                .send(Msg::VerticalScrollbarChanged {
                    window_id,
                    value: adj.value(),
                })
                .ok();
        });

        WindowScrollbars {
            vertical,
            cursor_indicator,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_key_press(&mut self, key_name: String, unicode: Option<char>, ctrl: bool, alt: bool) {
        // Handle Ctrl-Shift-V paste (sent as synthetic "PasteClipboard" key):
        // do async GDK clipboard read → ClipboardPasteToInput
        if key_name == "PasteClipboard" {
            if let Some(display) = gdk::Display::default() {
                let sender = self.sender.clone();
                display
                    .clipboard()
                    .read_text_async(gtk4::gio::Cancellable::NONE, move |result| {
                        let text = result
                            .ok()
                            .flatten()
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        sender.send(Msg::ClipboardPasteToInput { text }).ok();
                    });
            }
            return;
        }

        // Dismiss context menu on any key press (Escape, or j/k for nav, Enter to confirm).
        if self.engine.borrow().context_menu.is_some() {
            let mut engine = self.engine.borrow_mut();
            match key_name.as_str() {
                "Escape" => {
                    engine.close_context_menu();
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
                "Return" => {
                    let _act = engine.context_menu_confirm();
                    let needs_refresh = engine.explorer_needs_refresh;
                    if needs_refresh {
                        engine.explorer_needs_refresh = false;
                    }
                    drop(engine);
                    if needs_refresh {
                        self.dispatch(Msg::RefreshFileTree);
                    }
                    self.draw_needed.set(true);
                    return;
                }
                "j" | "Down" => {
                    if let Some(ref mut cm) = engine.context_menu {
                        let len = cm.items.len();
                        if len > 0 {
                            cm.selected = (cm.selected + 1) % len;
                        }
                    }
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
                "k" | "Up" => {
                    if let Some(ref mut cm) = engine.context_menu {
                        let len = cm.items.len();
                        if len > 0 {
                            cm.selected = if cm.selected > 0 {
                                cm.selected - 1
                            } else {
                                len - 1
                            };
                        }
                    }
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
                _ => {
                    engine.close_context_menu();
                    drop(engine);
                    self.draw_needed.set(true);
                    // Fall through to normal key handling
                }
            }
        }

        // Dismiss any panel hover popup on key press.
        self.engine.borrow_mut().dismiss_panel_hover_now();
        if let Some(ref da) = *self.panel_hover_da.borrow() {
            da.queue_draw();
        }

        // Pre-load system clipboard into engine registers for paste keys
        // (p/P in normal/visual, Ctrl+V in VSCode mode). Detection and
        // register loading are shared via engine methods (#381).
        if self
            .engine
            .borrow()
            .needs_clipboard_for_paste(&key_name, unicode, ctrl)
        {
            let text = self
                .engine
                .borrow()
                .clipboard_read
                .as_ref()
                .and_then(|cb| cb().ok());
            self.engine.borrow_mut().prepare_paste_clipboard(text);
        }

        // Activity bar keyboard navigation: j/k move cursor, l/Enter activate,
        // h/Esc return focus to the editor.
        if self.engine.borrow().activity_bar_focused && !self.engine.borrow().picker_open {
            self.handle_activity_bar_key(&key_name, ctrl);
            self.draw_needed.set(true);
            return;
        }

        // Debug F-keys must reach the engine regardless of which panel
        // has focus — F5 (continue), F9 (breakpoint), F10 (step over),
        // F11 (step in) are global debugger commands.
        if !ctrl && !alt {
            match key_name.as_str() {
                "F5" | "F9" | "F10" | "F11" => {
                    let mapped = map_gtk_key_name(&key_name);
                    let action = self.engine.borrow_mut().handle_key(mapped, None, false);
                    self.dispatch_engine_action(action, false);
                    self.draw_needed.set(true);
                    return;
                }
                _ => {}
            }
        }

        // Route keys to sidebar handlers when a sidebar has focus.
        // GTK focus on sidebar DrawingAreas is unreliable, so we check
        // the engine focus flags here (same approach as TUI backend).

        // Explorer keys are routed through Msg::ExplorerKey when the DA
        // has focus. This fallback catches keys when the DA lacks GTK
        // widget focus (grab_focus is unreliable for DrawingAreas).
        if self.engine.borrow().explorer_has_focus {
            let key_mapped = map_gtk_key_name(key_name.as_str()).to_string();
            self.handle_explorer_da_key(key_mapped, unicode, ctrl);
            self.draw_needed.set(true);
            return;
        }

        {
            let mut engine = self.engine.borrow_mut();
            if engine.ext_panel_has_focus {
                let mapped = map_gtk_key_name(key_name.as_str());
                if engine.dialog.is_some() {
                    engine.handle_key(mapped, unicode, false);
                } else if engine.ext_panel_input_active {
                    engine.handle_ext_panel_input_key(mapped, false, unicode);
                } else {
                    engine.handle_ext_panel_key(mapped, false, unicode);
                }
                let still_focused = engine.ext_panel_has_focus;
                let has_dialog = engine.dialog.is_some();
                drop(engine);
                // h/Left moves focus to the activity bar; other exits go to the editor.
                self.focus_after_sidebar_key(still_focused && !has_dialog);
                if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.queue_draw();
                }
                self.sync_plus_register_to_clipboard();
                self.draw_needed.set(true);
                return;
            }
            if engine.ext_sidebar_has_focus {
                let mapped = map_gtk_key_name(key_name.as_str());
                if engine.dialog.is_some() {
                    engine.handle_key(mapped, unicode, false);
                } else {
                    engine.dispatch_ext_sidebar_key_unified(mapped, unicode);
                }
                let still_focused = engine.ext_sidebar_has_focus;
                let has_dialog = engine.dialog.is_some();
                drop(engine);
                self.focus_after_sidebar_key(still_focused && !has_dialog);
                if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
            if engine.settings_has_focus {
                let mapped = map_gtk_key_name(key_name.as_str());
                if engine.dialog.is_some() {
                    engine.handle_key(mapped, unicode, ctrl);
                } else {
                    engine.handle_settings_key(mapped, ctrl, unicode);
                }
                let still_focused = engine.settings_has_focus;
                let has_dialog = engine.dialog.is_some();
                drop(engine);
                self.focus_after_sidebar_key(still_focused && !has_dialog);
                if let Some(ref da) = *self.settings_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
            if engine.search_has_focus {
                let mapped = map_gtk_key_name(key_name.as_str());
                if engine.dialog.is_some() {
                    engine.handle_key(mapped, unicode, ctrl);
                } else if ctrl && mapped == "v" {
                    drop(engine);
                    if let Some(display) = gdk::Display::default() {
                        let sender = self.sender.clone();
                        display.clipboard().read_text_async(
                            gtk4::gio::Cancellable::NONE,
                            move |result| {
                                let text = result
                                    .ok()
                                    .flatten()
                                    .map(|s| s.to_string())
                                    .unwrap_or_default();
                                sender.send(Msg::ClipboardPasteToInput { text }).ok();
                            },
                        );
                    }
                    self.draw_needed.set(true);
                    return;
                } else {
                    engine.dispatch_search_sidebar_key_unified(mapped, ctrl, alt, unicode);
                }
                let still_focused = engine.search_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.search_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
            if engine.sc_has_focus {
                let (mapped, sc_unicode) = map_gtk_key_with_unicode(key_name.as_str());
                if engine.dialog.is_some() {
                    engine.handle_key(mapped, sc_unicode, ctrl);
                } else {
                    engine.dispatch_sc_sidebar_key_unified(mapped, ctrl, sc_unicode);
                }
                let still_focused = engine.sc_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
            if engine.dap_sidebar_has_focus {
                if engine.dialog.is_some() {
                    engine.handle_key(&key_name, unicode, ctrl);
                } else {
                    let mapped = map_gtk_key_name(key_name.as_str());
                    let rect = engine.dap_sidebar_body_rect.get();
                    render::populate_dap_sidebar_system(&engine);
                    let consumed = if let Some(ui_event) = gtk_key_name_to_quadraui(mapped, ctrl) {
                        let backend_rc = self.backend.clone();
                        let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                            &ui_event,
                            &mut *backend_rc.borrow_mut(),
                            rect,
                        );
                        engine.dispatch_dap_sidebar_event(sidebar_event)
                    } else {
                        false
                    };
                    if !consumed {
                        engine.dispatch_dap_sidebar_action_key(mapped);
                    }
                }
                let still_focused = engine.dap_sidebar_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
            if engine.ai_has_focus {
                if engine.dialog.is_some() {
                    engine.handle_key(&key_name, unicode, ctrl);
                } else {
                    engine.handle_ai_panel_key(&key_name, ctrl, unicode);
                }
                let still_focused = engine.ai_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
        }

        // Hover popup copy: intercept y/Ctrl-C when hover is focused so the
        // engine's clipboard_write callback is invoked with the hover selection.
        {
            let engine = self.engine.borrow();
            let is_hover_copy = engine.editor_hover_has_focus
                && (key_name == "y" || key_name == "Y" || (ctrl && key_name == "c"));
            if is_hover_copy {
                if let Some(text) = engine.hover_selection_text() {
                    if let Some(ref cb) = engine.clipboard_write {
                        let _ = cb(text.as_str());
                    }
                    drop(engine);
                    let mut engine = self.engine.borrow_mut();
                    engine.message = "Hover text copied".to_string();
                    self.draw_needed.set(true);
                    return;
                }
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

        // Process macro playback queue if active
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

        // Ctrl-W h/l overflow: show sidebar and focus the active panel.
        {
            let overflow = self.engine.borrow_mut().window_nav_overflow.take();
            if let Some(false) = overflow {
                let current = self.current_active_panel_id();
                let panel_id = if is_ext_panel_id(&current) {
                    PANEL_EXPLORER.to_string()
                } else {
                    current
                };
                self.engine.borrow_mut().focus_sidebar_panel(&panel_id);
                self.sync_sidebar_from_engine();
            }
        }

        // Sync the unnamed register to the system clipboard if it changed.
        // The comparison is O(1); actual write is deferred to the background thread.
        self.sync_plus_register_to_clipboard();

        // If a yank just happened, schedule a 200 ms one-shot to clear the highlight.
        if self.engine.borrow().yank_highlight.is_some() {
            let s = self.sender.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                s.send(Msg::ClearYankHighlight).ok();
            });
        }

        self.draw_needed.set(true);
    }

    fn handle_poll_tick(&mut self) {
        // Reload CSS if the colorscheme changed (e.g. via :colorscheme command).
        {
            let current = self.engine.borrow().settings.colorscheme.clone();
            if current != self.last_colorscheme {
                let theme = Theme::from_name(&current);
                let combined = format!("{STATIC_CSS}\n{}", make_theme_css(&theme));
                self.css_provider.load_from_data(&combined);
                // Update GTK dark/light preference for native widgets & menus.
                if let Some(gtk_settings) = gtk4::Settings::default() {
                    gtk_settings.set_gtk_application_prefer_dark_theme(!theme.is_light());
                }
                self.last_colorscheme = current;
                self.draw_needed.set(true);
            }
        }

        // Check h scrollbar hover state from the shared mouse position cell.
        // This replaces per-motion-event Relm4 messages with a 20 Hz poll.
        {
            let (mx, my) = self.mouse_pos_cell.get();
            let lh = self.cached_line_height;
            let cw = self.cached_char_width;
            let da_size = self
                .drawing_area
                .borrow()
                .as_ref()
                .map(|da| (da.width() as f64, da.height() as f64));
            if let Some((da_w, da_h)) = da_size {
                let engine = self.engine.borrow();
                let rects = compute_editor_window_rects(&engine, da_w, da_h, lh);
                let now_hovered =
                    mx >= 0.0 && h_scrollbar_hit_test(&engine, mx, my, &rects, cw, lh).is_some();
                drop(engine);
                if now_hovered != self.h_sb_hovered {
                    self.h_sb_hovered = now_hovered;
                    self.h_sb_hovered_cell.set(now_hovered);
                    self.draw_needed.set(true);
                }

                // Tab close button hover detection + tab tooltip.
                let engine = self.engine.borrow();
                let close_bounds_map = self.tab_close_bounds.borrow();
                let tab_hover = if mx >= 0.0 && lh > 0.0 {
                    tab_close_hit_test(&engine, &close_bounds_map, mx, my, da_w, da_h, lh)
                } else {
                    None
                };
                drop(close_bounds_map);
                let tooltip = if mx >= 0.0 && lh > 0.0 {
                    tab_tooltip_hit_test(&engine, mx, my, da_w, da_h, lh, cw)
                } else {
                    None
                };
                drop(engine);
                if tab_hover != self.tab_close_hover {
                    self.tab_close_hover = tab_hover;
                    self.tab_close_hover_cell.set(tab_hover);
                    self.draw_needed.set(true);
                }
                {
                    let mut engine = self.engine.borrow_mut();
                    if tooltip != engine.tab_hover_tooltip {
                        engine.tab_hover_tooltip = tooltip;
                        self.draw_needed.set(true);
                    }
                }

                // Debug toolbar hover detection (#510) — use cached ToolbarLayout
                // on the engine rather than a model-local StatusBarLayout.
                {
                    let dbg_y = self.debug_toolbar_y_offset.get();
                    let dbg_h = self.debug_toolbar_height.get();
                    let new_hover = if dbg_h > 0.0 && my >= dbg_y && my < dbg_y + dbg_h {
                        let engine = self.engine.borrow();
                        engine.debug_button_hit(mx as f32, my as f32)
                    } else {
                        None
                    };
                    let old_hover = self.engine.borrow().debug_button_hovered;
                    if new_hover != old_hover {
                        self.engine.borrow_mut().debug_button_hovered = new_hover;
                        self.draw_needed.set(true);
                    }
                }

                // Editor hover: convert mouse pixel position to editor (line, col)
                // and feed into dwell detection for auto-hover popups.
                if mx >= 0.0 {
                    let mut engine = self.engine.borrow_mut();
                    // Phase B.5b Stage 6: gate the hover trigger when any
                    // blocking modal is open. Without this, mousing over
                    // an LSP-hoverable identifier under (e.g.) an open
                    // palette would still fire the hover request and
                    // pop the hover popup behind the palette (#247).
                    // The single source of truth lives in
                    // `Engine::is_blocking_modal_open()` — hover itself
                    // is a passive popup that doesn't count.
                    let blocking_modal_open = engine.is_blocking_modal_open();
                    if engine.settings.hover_delay > 0
                        && !engine.editor_hover_has_focus
                        && !blocking_modal_open
                        && (matches!(engine.mode, core::Mode::Normal | core::Mode::Visual)
                            || engine.is_vscode_mode())
                    {
                        let active_wid = engine.active_window_id();
                        if let Some((_wid, rect)) = rects.iter().find(|(w, _)| *w == active_wid) {
                            if mx >= rect.x
                                && mx < rect.x + rect.width
                                && my >= rect.y
                                && my < rect.y + rect.height
                            {
                                let total_lines = engine.buffer().len_lines();
                                // Approximate gutter width — exact value doesn't need
                                // to be pixel-perfect for hover dwell detection.
                                let gutter = render::calculate_gutter_cols(
                                    engine.settings.line_numbers,
                                    total_lines,
                                    cw,
                                    true, // assume git column present
                                    false,
                                );
                                let gutter_px = gutter as f64 * cw;
                                let text_x = rect.x + gutter_px;
                                let scroll_top = engine.view().scroll_top;
                                let scroll_left = engine.view().scroll_left;
                                if mx >= text_x {
                                    // Check if mouse is over the editor hover popup
                                    let mouse_on_popup = engine.editor_hover.is_some()
                                        && self.editor_hover_popup_rect.get().is_some_and(
                                            |(px, py, pw, ph)| {
                                                mx >= px && mx < px + pw && my >= py && my < py + ph
                                            },
                                        );
                                    if !mouse_on_popup {
                                        let rel_y = my - rect.y;
                                        let rel_x = mx - text_x;
                                        let vis_line = (rel_y / lh).floor() as usize;
                                        let line = scroll_top + vis_line;
                                        let col = scroll_left + (rel_x / cw).floor() as usize;
                                        engine.editor_hover_mouse_move(line, col, false);
                                    }
                                }
                            } else if engine.editor_hover.is_some()
                                && !engine.editor_hover_has_focus
                            {
                                // Mouse outside editor area — dismiss hover
                                engine.dismiss_editor_hover();
                            }
                        }
                    }
                }
            }
        }
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
            self.dispatch(Msg::QuitConfirmed);
        }
        // Run pending terminal commands (needs backend-supplied terminal size).
        if self.engine.borrow().pending_terminal_command.is_some() {
            let cmd = self
                .engine
                .borrow_mut()
                .pending_terminal_command
                .take()
                .unwrap();
            self.dispatch(Msg::RunCommandInTerminal(cmd));
        }
        // Explicitly redraw the debug sidebar if it's active so the
        // Run/Stop button text and section data stay in sync.
        let active_panel = self.current_active_panel_id();
        if active_panel == PANEL_DEBUG {
            if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                da.queue_draw();
            }
        }
        // Explorer refresh after confirmed file move.
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.dispatch(Msg::RefreshFileTree);
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
            if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                da.queue_draw();
            }
            if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                da.queue_draw();
            }
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
        // GTK-specific: queue redraws on individual sidebar DAs whose
        // content may have changed from the polls above.
        if active_panel == PANEL_EXPLORER {
            if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                da.queue_draw();
            }
        }
        // Panel hover overlay redraw.
        {
            if let Some(ref da) = *self.panel_hover_da.borrow() {
                da.queue_draw();
            }
        }
        // Explorer tree indicators (modified/diagnostics) are pulled by
        // the draw callback via `populate_explorer_tree_controller`, so we
        // trigger a redraw on a 1 Hz cadence to pick up background changes.
        if self.last_tree_indicator_update.elapsed() >= std::time::Duration::from_secs(1) {
            self.last_tree_indicator_update = std::time::Instant::now();
            if active_panel == PANEL_EXPLORER {
                if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
            }
        }
        // Sync the OS window title with the active buffer name (taskbar/pager).
        let win_title = self
            .engine
            .borrow()
            .active_buffer_name()
            .map(|n| format!("VimCode \u{2014} {}", n))
            .unwrap_or_else(|| "VimCode".to_string());
        if let Some(ref w) = self.window {
            w.set_title(Some(&win_title));
        }
    }

    /// Map a pixel x-offset within the editor hover popup's content
    /// area to a character column on `content_line`, using Pango to
    /// measure proportional UI-font widths (#218). The legacy code
    /// did `(rel_x / cached_char_width)` which drifts as the column
    /// index grows because UI_FONT is proportional. Heading rows
    /// (font scale > 1.0) need the scale applied to the layout so
    /// `xy_to_index` returns the right position.
    fn pixel_to_editor_hover_col(&self, rel_x: f64, content_line: usize) -> usize {
        let da = match self.drawing_area.borrow().as_ref() {
            Some(da) => da.clone(),
            None => return rel_x.max(0.0) as usize,
        };
        let engine = self.engine.borrow();
        let Some(eh) = engine.editor_hover.as_ref() else {
            return 0;
        };
        let Some(line_text) = eh.rendered.lines.get(content_line).cloned() else {
            return 0;
        };
        let heading_level = eh
            .rendered
            .spans
            .get(content_line)
            .and_then(|spans| {
                spans.iter().find_map(|s| match s.style {
                    core::markdown::MdStyle::Heading(n) => Some(n),
                    _ => None,
                })
            })
            .unwrap_or(0);
        let scale = match heading_level {
            1 => 1.4,
            2 => 1.2,
            3..=6 => 1.1,
            _ => 1.0,
        };
        drop(engine);

        let pango_ctx = da.pango_context();
        let layout = pango::Layout::new(&pango_ctx);
        let font_desc = FontDescription::from_string(&UI_FONT());
        layout.set_font_description(Some(&font_desc));
        layout.set_text(&line_text);
        if (scale - 1.0_f64).abs() > 0.01 {
            let attrs = pango::AttrList::new();
            let mut a = pango::AttrFloat::new_scale(scale);
            a.set_start_index(0);
            a.set_end_index(line_text.len() as u32);
            attrs.insert(a);
            layout.set_attributes(Some(&attrs));
        }

        let x_pango = (rel_x.max(0.0) * pango::SCALE as f64) as i32;
        // y=0 → first (and only) line of the layout. xy_to_index returns
        // (inside, byte_index, trailing). When the click is past the
        // line's last char `inside` is false but `byte_index + trailing`
        // points at the trailing edge, which is what we want.
        let (_inside, byte_index, trailing) = layout.xy_to_index(x_pango, 0);
        let byte_pos = (byte_index as usize).saturating_add(trailing as usize);
        let clamped = byte_pos.min(line_text.len());
        line_text[..clamped].chars().count()
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
            (true, Some((px, py, pw, ph))) => {
                stack.push(
                    editor_hover_id,
                    quadraui::Rect {
                        x: px as f32,
                        y: py as f32,
                        width: pw as f32,
                        height: ph as f32,
                    },
                );
            }
            _ => {
                stack.pop(&editor_hover_id);
            }
        }
    }

    fn editor_pango_layout(&self, engine: &Engine) -> pango::Layout {
        // The old Relm4 path stored a per-App DrawingArea so we could always
        // get a PangoContext from it.  Under the quadraui ShellApp runner the
        // single DrawingArea is owned by the runner and `self.drawing_area` is
        // never populated, so fall back to the runner-created Window (grabbed
        // in `setup()`) or, as a last resort, the default Pango/Cairo font map.
        // `pangocairo` is aliased to `pangocairo::functions` at the top of this
        // file, so use the fully-qualified path `::pangocairo::FontMap` to reach
        // the `FontMap` type from the crate root.
        let ctx = if let Some(ref da) = *self.drawing_area.borrow() {
            da.pango_context()
        } else if let Some(ref win) = self.window {
            win.pango_context()
        } else {
            // Last resort: GTK must be initialized at this point (enforced in
            // run()) so the default PangoCairo font map is available.
            ::pangocairo::FontMap::new().create_context()
        };
        let layout = pango::Layout::new(&ctx);
        let font_desc = FontDescription::from_string(&format!(
            "{} {}",
            engine.settings.font_family, engine.settings.font_size
        ));
        layout.set_font_description(Some(&font_desc));
        layout
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_mouse_click_msg(&mut self, x: f64, y: f64, width: f64, height: f64, alt: bool) {
        self.reconcile_editor_hover_modal();

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
                    quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset }
                        if widget.as_str() == "debug_output" =>
                    {
                        let mut engine = self.engine.borrow_mut();
                        engine.debug_output_scroll = *new_offset;
                        engine.debug_output_auto_scroll = false;
                        drop(engine);
                        self.draw_needed.set(true);
                        return;
                    }
                    quadraui::UiEvent::MouseDown {
                        widget: Some(id), ..
                    } if id.as_str() == "debug_output" => {
                        return;
                    }
                    quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset }
                        if widget.as_str() == "terminal_scrollback" =>
                    {
                        if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                            term.set_scroll_offset(*new_offset);
                        }
                        self.draw_needed.set(true);
                        return;
                    }
                    _ => {}
                }
            }
        }

        // ── Tab switcher modal arbitration (B.5b Stage 7) ──────────────
        //
        // Keyboard-driven popup (Ctrl+Tab cycles, Ctrl release commits,
        // Esc dismisses). Click anywhere dismisses — inside the popup
        // also consumes (no editor cursor-move underneath); outside
        // dismisses + propagates so the editor receives the click.
        if self.engine.borrow().tab_switcher_open {
            let switcher_id = quadraui::WidgetId::new("tab_switcher");
            let inside = if let Some((px, py, pw, ph)) = self.tab_switcher_popup_rect.get() {
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .push(
                        switcher_id.clone(),
                        quadraui::Rect {
                            x: px as f32,
                            y: py as f32,
                            width: pw as f32,
                            height: ph as f32,
                        },
                    );
                let stack_rc = self.backend.borrow().modal_stack_handle();
                let stack = stack_rc.borrow();
                let events = quadraui::dispatch_mouse_down(
                    &stack,
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    quadraui::MouseButton::Left,
                    quadraui::Modifiers::default(),
                );
                events.iter().any(|ev| {
                    matches!(
                        ev,
                        quadraui::UiEvent::MouseDown { widget: Some(id), .. }
                            if *id == switcher_id
                    )
                })
            } else {
                false
            };

            self.engine.borrow_mut().tab_switcher_open = false;
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&switcher_id);

            if inside {
                self.draw_needed.set(true);
                return;
            }
            // Outside: fall through so editor click proceeds.
        } else {
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&quadraui::WidgetId::new("tab_switcher"));
        }

        // ── Completion popup modal arbitration (B.5b Stage 5) ──────────
        //
        // The popup auto-dismisses on any click. If the click landed
        // INSIDE the popup we also consume it (return early) so the
        // editor underneath doesn't pick up a cursor move at the
        // candidate-row pixel — clicking on the popup shouldn't move
        // the cursor through it. Click-OUTSIDE simply dismisses and
        // falls through; the editor click then proceeds normally.
        if self.engine.borrow().completion_idx.is_some() {
            let hit = self
                .completion_layout
                .borrow()
                .as_ref()
                .map(|cl| cl.hit_test(x as f32, y as f32))
                .unwrap_or(quadraui::CompletionsHit::Empty);
            let consumed = self.engine.borrow_mut().handle_completion_click(hit);
            self.draw_needed.set(true);
            if consumed {
                return;
            }
        }

        // ── Context menu click handling (engine-drawn) ──
        //
        // Phase B.5b Stage 4: routed through `ModalStack` +
        // `quadraui::dispatch_mouse_down` for outside arbitration, and
        // through `quadraui::ContextMenuLayout::hit_test` for inner
        // row-level refinement — the SAME hit-test the renderer
        // (`draw.rs::draw_context_menu_popup`) uses for hover. The
        // legacy `resolve_context_menu_click` was off-by-one for items
        // below a separator (#251) because the renderer was migrated
        // to `quadraui::ContextMenu::layout` (no top/bottom border
        // padding) but the click hit-test still assumed the old
        // "+1 row top border" layout. Driving both off the same
        // `ContextMenuLayout` eliminates drift by construction.
        if self.engine.borrow().context_menu.is_some() {
            let cm_id = quadraui::WidgetId::new("context_menu");

            let menu_layout = self.context_menu_layout.borrow().clone();

            let Some(menu_layout) = menu_layout else {
                // Empty items list — close defensively.
                self.engine.borrow_mut().close_context_menu();
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&cm_id);
                self.draw_needed.set(true);
                return;
            };

            // Push the menu's resolved bounds to the modal stack so
            // any other modal that might be open (picker, dialog) is
            // arbitrated against the menu by `dispatch_mouse_down`.
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .push(cm_id.clone(), menu_layout.bounds);

            let stack_events = {
                let stack_rc = self.backend.borrow().modal_stack_handle();
                let stack = stack_rc.borrow();
                quadraui::dispatch_mouse_down(
                    &stack,
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    quadraui::MouseButton::Left,
                    quadraui::Modifiers::default(),
                )
            };
            let dismissed = stack_events.iter().any(|ev| {
                matches!(
                    ev,
                    quadraui::UiEvent::Palette(id, _) if *id == cm_id
                )
            });

            if dismissed {
                self.engine.borrow_mut().close_context_menu();
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&cm_id);
            } else {
                // Inner hit. `hit_test` returns Item(id) for clickable
                // rows, Inert for separators / disabled rows, Empty
                // for outside (unreachable here since the dispatcher
                // already routed that case to `dismissed`).
                match menu_layout.hit_test(x as f32, y as f32) {
                    quadraui::ContextMenuHit::Item(id) => {
                        // Item ids are synthesised as `"context:N"`
                        // where N is the engine-side item index. Parse
                        // back to the engine index so
                        // `context_menu_confirm` fires the right action.
                        let engine_idx = id
                            .as_str()
                            .strip_prefix("context:")
                            .and_then(|s| s.parse::<usize>().ok());
                        if let Some(idx) = engine_idx {
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
                            self.backend
                                .borrow()
                                .modal_stack_handle()
                                .borrow_mut()
                                .pop(&cm_id);
                            if needs_tree_refresh {
                                self.dispatch(Msg::RefreshFileTree);
                            }
                        }
                    }
                    quadraui::ContextMenuHit::Inert => {
                        // Separator or disabled item — keep menu open.
                    }
                    quadraui::ContextMenuHit::Empty => {
                        // Defensive: dispatcher should have caught this.
                        self.engine.borrow_mut().close_context_menu();
                        self.backend
                            .borrow()
                            .modal_stack_handle()
                            .borrow_mut()
                            .pop(&cm_id);
                    }
                }
            }
            self.draw_needed.set(true);
            return;
        }
        // Defensive cleanup: context menu may have closed via Esc/Enter
        // while no click was seen by us. Pop any stale entry.
        self.backend
            .borrow()
            .modal_stack_handle()
            .borrow_mut()
            .pop(&quadraui::WidgetId::new("context_menu"));

        // ── Find/replace overlay click handling (using shared hit regions) ──
        //
        // #196: must use the SAME cell-unit layout as the renderer in
        // `draw.rs::draw_find_replace_popup`. Pixel → cell conversion
        // uses `char_width` + `line_height`; the popup origin is
        // derived the same way the renderer computes it from the
        // active editor group's bounds.
        if self.engine.borrow().find_replace_open {
            let cw = self.cached_char_width.max(1.0);
            let lh = self.cached_line_height.max(1.0);

            let (hit_regions, on_panel, rel_col, rel_row) = {
                let engine = self.engine.borrow();

                // Build match_info (same logic as build_screen_layout).
                let match_info = if engine.search_matches.is_empty() {
                    if engine.find_replace_query.is_empty() {
                        String::new()
                    } else {
                        "No results".to_string()
                    }
                } else {
                    match engine.search_index {
                        Some(idx) => {
                            format!("{} of {}", idx + 1, engine.search_matches.len())
                        }
                        None => format!("{} matches", engine.search_matches.len()),
                    }
                };

                let panel_w_cells = render::FR_PANEL_WIDTH;
                let (hit_regions, _) = render::compute_find_replace_hit_regions(
                    panel_w_cells,
                    engine.find_replace_show_replace,
                    &match_info,
                );

                // Popup bounds — mirror exactly what `draw_find_replace_popup`
                // computes. panel_w is in cells; scale via `cw`.
                // +2 rows in height for top/bottom border.
                let popup_w = panel_w_cells as f64 * cw;
                let row_count_f = if engine.find_replace_show_replace {
                    2.0
                } else {
                    1.0
                };
                let popup_h = (row_count_f + 2.0) * lh;
                // Renderer uses the active group's bounds to compute
                // popup_x; we don't have that here, so approximate from
                // the DA width the same way the renderer does in the
                // typical single-group case.
                let popup_x = (width - popup_w - 10.0).max(0.0);
                let popup_y = lh * 2.5 + 2.0;

                let on_panel =
                    x >= popup_x && x < popup_x + popup_w && y >= popup_y && y < popup_y + popup_h;

                // Content origin — 1 cell inside the borders, same as
                // `draw_find_replace_popup`. Pixel → cell.
                let content_x = popup_x + cw;
                let content_y = popup_y + lh;
                let rel_col = if x >= content_x {
                    ((x - content_x) / cw) as u16
                } else {
                    u16::MAX
                };
                let rel_row = if y >= content_y {
                    ((y - content_y) / lh) as u16
                } else {
                    u16::MAX
                };

                (hit_regions, on_panel, rel_col, rel_row)
            };

            if on_panel {
                let mut matched_target = None;
                for (region, target) in &hit_regions {
                    if region.row == rel_row
                        && rel_col >= region.col
                        && rel_col < region.col + region.width
                    {
                        matched_target = Some((*target, region.col));
                        break;
                    }
                }

                if let Some((target, region_col)) = matched_target {
                    use core::engine::FindReplaceClickTarget::*;

                    let target = match target {
                        FindInput(_) => FindInput(rel_col.saturating_sub(region_col) as usize),
                        ReplaceInput(_) => {
                            ReplaceInput(rel_col.saturating_sub(region_col) as usize)
                        }
                        other => other,
                    };

                    if matches!(target, FindInput(_) | ReplaceInput(_)) {
                        self.fr_input_dragging = true;
                    }

                    self.engine.borrow_mut().handle_find_replace_click(target);
                }

                self.draw_needed.set(true);
                return;
            }
        }

        // Picker popup: route the click through quadraui's modal-stack
        // dispatcher (Phase B.4 pilot). Before this refactor, the click
        // was gated by an inline popup-bounds check; the *drag* gesture
        // on the same DrawingArea had no equivalent check and leaked
        // through to the editor behind the modal (#192). The drag guard
        // now lives in `handle_mouse_drag_msg` and consults the same
        // modal stack this branch pushes to.
        //
        // Inner hit refinement (which result row) still lives here
        // because the palette primitive's result-row hit math hasn't
        // been lifted into quadraui yet — that's a separate follow-up
        // once we generalise beyond the pilot.
        {
            let engine = self.engine.borrow();
            if engine.picker_open {
                drop(engine);
                // Keep the modal stack in sync with engine state. Safe
                // to call repeatedly — push() dedupes on id.
                let (popup_x, popup_y, popup_w, popup_h) =
                    self.compute_picker_popup_bounds(width, height);
                let picker_id = quadraui::WidgetId::new("picker");
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .push(
                        picker_id.clone(),
                        quadraui::Rect {
                            x: popup_x as f32,
                            y: popup_y as f32,
                            width: popup_w as f32,
                            height: popup_h as f32,
                        },
                    );

                let stack_rc = self.backend.borrow().modal_stack_handle();
                let stack = stack_rc.borrow();
                let events = quadraui::dispatch_mouse_down(
                    &stack,
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    quadraui::MouseButton::Left,
                    quadraui::Modifiers::default(),
                );
                drop(stack);

                // Inspect the dispatcher's verdict.
                let mut hit_modal = false;
                let mut dismiss_modal = false;
                for ev in &events {
                    match ev {
                        quadraui::UiEvent::MouseDown {
                            widget: Some(id), ..
                        } if *id == picker_id => {
                            hit_modal = true;
                        }
                        quadraui::UiEvent::Palette(_, _) => {
                            dismiss_modal = true;
                        }
                        _ => {}
                    }
                }

                if hit_modal {
                    let lh = self.cached_line_height.max(1.0);
                    let has_preview = self.engine.borrow().picker_preview.is_some();
                    let list_w = if has_preview {
                        (popup_w * 0.4_f64).round()
                    } else {
                        popup_w
                    };
                    let results_top = popup_y + lh * 2.0 + 1.0;
                    let results_bottom = popup_y + popup_h;
                    const BOTTOM_INSET: f64 = 4.0;
                    let rows_h_raw = (results_bottom - results_top - BOTTOM_INSET).max(0.0);
                    let visible_rows = (rows_h_raw / lh) as usize;
                    let rows_h = visible_rows as f64 * lh;
                    let (total, scroll_top, selected) = {
                        let engine = self.engine.borrow();
                        (
                            engine.picker_items.len(),
                            engine.picker_scroll_top,
                            engine.picker_selected,
                        )
                    };
                    let has_scrollbar = total > visible_rows;

                    let max_offset = total.saturating_sub(visible_rows);
                    let effective_offset = if visible_rows == 0 {
                        0
                    } else if selected < scroll_top {
                        selected
                    } else if selected >= scroll_top + visible_rows {
                        selected + 1 - visible_rows
                    } else {
                        scroll_top
                    }
                    .min(max_offset);

                    const SB_W: f64 = 6.0;
                    let sb_x = popup_x + list_w - SB_W;
                    let on_scrollbar = has_scrollbar
                        && visible_rows > 0
                        && x >= sb_x
                        && x < popup_x + list_w
                        && y >= results_top
                        && y < results_top + rows_h;

                    if on_scrollbar {
                        let rel = ((y - results_top) / rows_h).clamp(0.0, 1.0);
                        let max_scroll = total.saturating_sub(visible_rows);
                        let new_offset = (rel * max_scroll as f64).round() as usize;
                        {
                            let mut engine = self.engine.borrow_mut();
                            engine.picker_scroll_top = new_offset;
                            if engine.picker_selected < new_offset {
                                engine.picker_selected = new_offset;
                            } else if engine.picker_selected >= new_offset + visible_rows {
                                engine.picker_selected = new_offset + visible_rows - 1;
                            }
                            engine.picker_load_preview();
                        }
                        self.backend
                            .borrow()
                            .drag_state_handle()
                            .borrow_mut()
                            .begin(quadraui::DragTarget::ScrollbarY {
                                widget: picker_id.clone(),
                                track_start: results_top as f32,
                                track_length: rows_h as f32,
                                thumb_length: (rows_h as f32 * visible_rows as f32
                                    / total.max(1) as f32)
                                    .max(1.0),
                                max_scroll: total.saturating_sub(visible_rows),
                                grab_offset: 0.0,
                                inverted: false,
                            });
                    } else if y >= results_top && y < results_bottom {
                        let mut engine = self.engine.borrow_mut();
                        let clicked_idx = effective_offset + ((y - results_top) / lh) as usize;
                        if clicked_idx < engine.picker_items.len() {
                            engine.picker_selected = clicked_idx;
                            engine.picker_load_preview();
                        }
                    }
                }
                if dismiss_modal {
                    self.engine.borrow_mut().close_picker();
                    self.backend
                        .borrow()
                        .modal_stack_handle()
                        .borrow_mut()
                        .pop(&picker_id);
                }
                // Consume click — don't fall through to editor.
                return;
            } else {
                // Picker isn't open but the stack might hold a stale
                // entry (engine closed it via Esc or Enter while the
                // stack still has the id). Keep them consistent.
                let picker_id = quadraui::WidgetId::new("picker");
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&picker_id);
            }
        }

        // Breadcrumb click: shared resolution via cached StatusBarLayout.
        {
            let engine = self.engine.borrow();
            if engine.settings.breadcrumbs {
                let lh = self.cached_line_height.max(1.0);
                if let Some(ref screen) = *self.cached_screen_layout.borrow() {
                    match render::resolve_breadcrumb_click(&screen.breadcrumbs, x, y, lh) {
                        render::BreadcrumbClickResult::Hit(idx) => {
                            drop(engine);
                            self.engine.borrow_mut().handle_breadcrumb_click(idx);
                            return;
                        }
                        render::BreadcrumbClickResult::OnBar => return,
                        render::BreadcrumbClickResult::Miss => {}
                    }
                }
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

        // Editor hover: click on the popup focuses it; click elsewhere dismisses it
        {
            let engine = self.engine.borrow();
            if engine.editor_hover.is_some() {
                let rect = self.editor_hover_popup_rect.get();
                let on_popup = if let Some((px, py, pw, ph)) = rect {
                    x >= px && x < px + pw && y >= py && y < py + ph
                } else {
                    false
                };
                let has_focus = engine.editor_hover_has_focus;
                drop(engine);
                if on_popup {
                    // Scrollbar hit-test (#215). Track click jumps to that
                    // offset and arms a drag so mouse-move updates the
                    // offset live; thumb click just begins the drag.
                    if let Some(sb_hit) = self.editor_hover_scrollbar.get() {
                        let cx = x as f32;
                        let cy = y as f32;
                        let on_thumb = cx >= sb_hit.thumb.x
                            && cx < sb_hit.thumb.x + sb_hit.thumb.width
                            && cy >= sb_hit.thumb.y
                            && cy < sb_hit.thumb.y + sb_hit.thumb.height;
                        let on_track = !on_thumb
                            && cx >= sb_hit.track.x
                            && cx < sb_hit.track.x + sb_hit.track.width
                            && cy >= sb_hit.track.y
                            && cy < sb_hit.track.y + sb_hit.track.height;
                        if on_track || on_thumb {
                            if on_track {
                                let max_scroll = sb_hit.total.saturating_sub(sb_hit.visible_rows);
                                let rel = ((cy - sb_hit.track.y) / sb_hit.track.height.max(1.0))
                                    .clamp(0.0, 1.0);
                                let new_offset = (rel * max_scroll as f32).round() as usize;
                                self.engine.borrow_mut().editor_hover_set_scroll(new_offset);
                            }
                            self.backend
                                .borrow()
                                .drag_state_handle()
                                .borrow_mut()
                                .begin(quadraui::DragTarget::ScrollbarY {
                                    widget: quadraui::WidgetId::new("editor_hover"),
                                    track_start: sb_hit.track.y,
                                    track_length: sb_hit.track.height,
                                    thumb_length: sb_hit.thumb.height,
                                    max_scroll: sb_hit.total.saturating_sub(sb_hit.visible_rows),
                                    grab_offset: 0.0,
                                    inverted: false,
                                });
                            self.draw_needed.set(true);
                            return;
                        }
                    }
                    // Check if click hit a link rect.
                    let link_hit = self
                        .editor_hover_link_rects
                        .borrow()
                        .iter()
                        .find(|(lx, ly, lw, lh, _)| {
                            x >= *lx && x <= lx + lw && y >= *ly && y <= ly + lh
                        })
                        .cloned();
                    if let Some((_, _, _, _, url)) = link_hit {
                        if url.starts_with("command:") {
                            self.engine.borrow_mut().execute_command_uri(&url);
                        } else {
                            open_url(&url);
                        }
                        self.engine.borrow_mut().dismiss_editor_hover();
                    } else if !has_focus {
                        self.engine.borrow_mut().editor_hover_has_focus = true;
                    } else {
                        // Focused, no link hit — start text selection
                        let lh = self.cached_line_height.max(1.0);
                        if let Some((px, py, _pw, _ph)) = rect {
                            let padding = 4.0;
                            let rel_x = x - px - padding;
                            let rel_y = y - py - padding;
                            let engine_ref = self.engine.borrow();
                            let scroll = engine_ref
                                .editor_hover
                                .as_ref()
                                .map(|h| h.scroll_top)
                                .unwrap_or(0);
                            drop(engine_ref);
                            let content_line = (rel_y / lh).max(0.0) as usize + scroll;
                            let content_col = self.pixel_to_editor_hover_col(rel_x, content_line);
                            self.engine
                                .borrow_mut()
                                .editor_hover_start_selection(content_line, content_col);
                        }
                    }
                    // Consume click — don't process as editor click
                    self.draw_needed.set(true);
                    return;
                } else if !has_focus {
                    self.engine.borrow_mut().dismiss_editor_hover();
                } else {
                    // Focused popup — click outside dismisses
                    self.engine.borrow_mut().dismiss_editor_hover();
                }
            }
        }
        // Dialog button click — highest z-order element.
        //
        // Phase B.5b Stage 3: routed through `ModalStack` + the shared
        // `quadraui::dispatch_mouse_down` arbiter, mirroring the
        // picker pattern above. The dialog is pushed onto the stack
        // (idempotently — push() dedupes by id) every time the click
        // handler runs while the dialog is open, popped when the
        // dialog closes (here, after a button click or outside-click
        // dismiss; or in the `else` branch below if the engine
        // closed it via Esc/Enter without the click handler seeing).
        // Inner button hit-testing stays per-backend (uses GTK
        // pixel-level `dialog_btn_rects` from the last draw).
        if self.engine.borrow().dialog.is_some() {
            let btn_rects = self.dialog_btn_rects.borrow().clone();

            // Use actual button rects from the last draw_dialog_popup call.
            let mut clicked_btn: Option<usize> = None;
            for (idx, &(bx, by, bw, bh)) in btn_rects.iter().enumerate() {
                if x >= bx && x < bx + bw && y >= by && y < by + bh {
                    clicked_btn = Some(idx);
                    break;
                }
            }

            // Pull the resolved popup bounds from the last draw_dialog_popup
            // call (cached in `dialog_popup_rect`). Earlier the bounds were
            // derived from `btn_rects` with a 350px-min fudge that overshot
            // the actual popup width on small dialogs (`:about`), so
            // `dispatch_mouse_down` would mis-classify outside clicks as
            // inside and the dismiss path never fired.
            let dialog_id = quadraui::WidgetId::new("dialog");
            if let Some((px, py, pw, ph)) = self.dialog_popup_rect.get() {
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .push(
                        dialog_id.clone(),
                        quadraui::Rect {
                            x: px as f32,
                            y: py as f32,
                            width: pw as f32,
                            height: ph as f32,
                        },
                    );
            }

            // Run the shared dispatch to learn whether this click
            // landed inside the dialog or in the backdrop. We don't
            // strictly need the inside verdict (button hit-test
            // already drove that) but the outside verdict is what
            // replaces the inline `outside = x < popup_x || ...`
            // computation.
            let outside = {
                let stack_rc = self.backend.borrow().modal_stack_handle();
                let stack = stack_rc.borrow();
                let events = quadraui::dispatch_mouse_down(
                    &stack,
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    quadraui::MouseButton::Left,
                    quadraui::Modifiers::default(),
                );
                events.iter().any(|ev| {
                    matches!(
                        ev,
                        quadraui::UiEvent::Palette(id, _) if *id == dialog_id
                    )
                })
            };

            if let Some(idx) = clicked_btn {
                let action = self.engine.borrow_mut().dialog_click_button(idx);
                if self.engine.borrow().explorer_needs_refresh {
                    self.engine.borrow_mut().explorer_needs_refresh = false;
                    self.dispatch(Msg::RefreshFileTree);
                }
                match action {
                    EngineAction::Quit | EngineAction::SaveQuit => {
                        self.save_session_and_exit();
                    }
                    _ => {}
                }
                // dialog_click_button may have closed the dialog; sync
                // the stack so the next frame doesn't see a stale entry.
                if self.engine.borrow().dialog.is_none() {
                    self.backend
                        .borrow()
                        .modal_stack_handle()
                        .borrow_mut()
                        .pop(&dialog_id);
                }
            } else if outside {
                self.engine.borrow_mut().dialog = None;
                self.engine.borrow_mut().pending_move = None;
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&dialog_id);
            }
            self.draw_needed.set(true);
        } else {
            // Dialog closed (possibly via Esc/Enter while no click was
            // seen by us). Pop any stale entry so the stack stays in
            // sync with engine state — same defensive cleanup the
            // picker block does above.
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&quadraui::WidgetId::new("dialog"));
            // ── Status bar branch click — open branch picker ─────────────
            // (only when per-window status is off — global bar exists)
            if self.cached_line_height > 0.0 {
                let lh = self.cached_line_height;
                let engine = self.engine.borrow();
                let per_window_status = engine.settings.window_status_line;
                let wildmenu_px = if engine.wildmenu_items.is_empty() {
                    0.0
                } else {
                    lh
                };
                let global_status_rows = if per_window_status { 1.0 } else { 2.0 };
                let status_bar_height = lh * global_status_rows + wildmenu_px;
                let status_y = height - status_bar_height;
                if y >= status_y && y < status_y + lh && engine.git_branch.is_some() {
                    // Reconstruct branch column range (matching build_status_line logic)
                    let mode_str = engine.mode_str();
                    let filename = match engine.file_path() {
                        Some(p) => p
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.display().to_string()),
                        None => "[No Name]".to_string(),
                    };
                    let dirty = if engine.dirty() { " [+]" } else { "" };
                    let recording = if let Some(reg) = engine.macro_recording {
                        format!(" [recording @{}]", reg)
                    } else {
                        String::new()
                    };
                    let prefix = format!(" -- {}{} -- {}{}", mode_str, recording, filename, dirty);
                    let b = engine.git_branch.as_deref().unwrap();
                    let mut branch_text = b.to_string();
                    if engine.sc_ahead > 0 || engine.sc_behind > 0 {
                        let mut parts = Vec::new();
                        if engine.sc_ahead > 0 {
                            parts.push(format!("↑{}", engine.sc_ahead));
                        }
                        if engine.sc_behind > 0 {
                            parts.push(format!("↓{}", engine.sc_behind));
                        }
                        branch_text = format!("{} {}", branch_text, parts.join(" "));
                    }
                    let branch_str = format!(" [{}]", branch_text);
                    let start = prefix.len();
                    let end = start + branch_str.len();
                    let cw = self.cached_char_width.max(1.0);
                    let click_col = (x / cw) as usize;
                    drop(engine);
                    if click_col >= start && click_col < end {
                        self.engine
                            .borrow_mut()
                            .open_picker(crate::core::engine::PickerSource::GitBranches);
                        self.draw_needed.set(true);
                        return;
                    }
                } else {
                    drop(engine);
                }
            }

            // Clicking in the editor clears every sidebar's keyboard focus.
            // Without this, focus stays on whichever sidebar grabbed it last
            // (Source Control, Extensions, Settings, AI, DAP, …) and the
            // editor key handler keeps routing keys to that sidebar's
            // handler — so the editor "can't be interacted with" until the
            // user explicitly Escapes out of the sidebar. The DAP-only
            // version of this clear was incomplete; tracked all fields via
            // `clear_sidebar_focus()` instead.
            self.engine.borrow_mut().clear_sidebar_focus();
            // Check if click lands in the terminal panel before general handling.
            // Layout (bottom to top): status | toolbar | terminal | quickfix | DAP | editor
            // Geometry is cached at paint time on engine.bottom_panel_geometry (#418).
            let zone = self.engine.borrow().resolve_bottom_panel_zone(y);
            if let Some(zone) = zone {
                use crate::core::engine::BottomPanelZone;
                if matches!(zone, BottomPanelZone::TabBar) {
                    self.engine.borrow_mut().handle_bottom_tab_bar_click(x);
                    self.dispatch(Msg::Resize);
                    return;
                }
                self.engine.borrow_mut().terminal_has_focus = true;
                if let BottomPanelZone::Content { .. } = zone {
                    let split_layout = *self.engine.borrow().terminal_split_layout.borrow();
                    if let Some(ref sl) = split_layout {
                        let hit = sl.hit_test(x as f32, y as f32);
                        // #533: pass button/mods so the engine can
                        // forward_mouse(Press) to the child when it has mouse
                        // reporting enabled.
                        if self.engine.borrow_mut().handle_terminal_split_click(
                            hit,
                            quadraui::MouseButton::Left,
                            quadraui::Modifiers::default(),
                        ) {
                            self.terminal_split_dragging = true;
                        }
                    } else {
                        self.terminal_resize_dragging = false;
                        let col = (x / self.cached_char_width.max(1.0)) as u16;
                        let row_offset = match zone {
                            BottomPanelZone::Content { row_offset } => row_offset,
                            _ => 0,
                        };
                        // #533: shared press handler — tries forward_mouse(Press)
                        // when the child has mouse reporting, falls back to
                        // terminal_scroll_reset + local selection start.
                        self.engine.borrow_mut().handle_terminal_pane_press(
                            col,
                            row_offset,
                            quadraui::MouseButton::Left,
                            quadraui::Modifiers::default(),
                        );
                    }
                } else {
                    // Header row — dispatch through cached toolbar hit regions.
                    let action = self.engine.borrow().resolve_terminal_toolbar_click(x);
                    let ctx = crate::core::engine::UiEventContext {
                        terminal_cols: self.terminal_cols(),
                        terminal_max_rows: self.terminal_target_maximize_rows(),
                    };
                    if !self
                        .engine
                        .borrow_mut()
                        .execute_terminal_toolbar_action(action, ctx)
                        && matches!(
                            action,
                            crate::core::engine::TerminalToolbarAction::StartResize
                        )
                    {
                        self.terminal_resize_dragging = true;
                    }
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
                            if x < thumb_x {
                                let mut engine = self.engine.borrow_mut();
                                let new_left = scroll_left.saturating_sub(page_cols);
                                engine.set_scroll_left_for_window(win_id, new_left);
                                self.draw_needed.set(true);
                                return;
                            } else if x >= thumb_x + thumb_w {
                                let mut engine = self.engine.borrow_mut();
                                let new_left = (scroll_left + page_cols).min(max_scroll);
                                engine.set_scroll_left_for_window(win_id, new_left);
                                self.draw_needed.set(true);
                                return;
                            }
                            let grab_offset = (x - thumb_x) as f32;
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
                            self.h_sb_drag_cell.set(Some(win_id));
                            self.draw_needed.set(true);
                            return;
                        }
                    }
                }

                // ── Editor group divider hit-test ─────────────────────────────
                {
                    let engine = self.engine.borrow();
                    if !engine.group_layout.is_single_group() {
                        let lh = self.cached_line_height;
                        let tab_row_h = (lh * 1.6).ceil();
                        let tab_bar_h = if engine.settings.breadcrumbs {
                            tab_row_h + lh
                        } else {
                            tab_row_h
                        };
                        let editor_bottom = gtk_editor_bottom(&engine, width, height, lh);
                        let content_bounds =
                            core::window::WindowRect::new(0.0, 0.0, width, editor_bottom);

                        // Compute tab bar regions so we can exclude them from
                        // divider drag — tab bar clicks should go to tab handlers.
                        let group_rects = engine
                            .group_layout
                            .calculate_group_rects(content_bounds, tab_bar_h);
                        let in_tab_bar = group_rects.iter().any(|(gid, grect)| {
                            if engine.is_tab_bar_hidden(*gid) {
                                return false;
                            }
                            let ty = grect.y - tab_bar_h;
                            y >= ty
                                && y < ty + tab_bar_h
                                && x >= grect.x
                                && x < grect.x + grect.width
                        });

                        if !in_tab_bar {
                            let dividers = engine.group_layout.dividers(content_bounds, &mut 0);
                            for div in &dividers {
                                let hit = match div.direction {
                                    core::window::SplitDirection::Vertical => {
                                        (x - div.position).abs() < 6.0
                                            && y >= div.cross_start
                                            && y < div.cross_start + div.cross_size
                                    }
                                    core::window::SplitDirection::Horizontal => {
                                        (y - div.position).abs() < 6.0
                                            && x >= div.cross_start
                                            && x < div.cross_start + div.cross_size
                                    }
                                };
                                if hit {
                                    let si = div.split_index;
                                    drop(engine);
                                    self.group_divider_dragging = Some(si);
                                    return;
                                }
                            }
                        }
                    }
                }

                {
                    let mut engine = self.engine.borrow_mut();

                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    let editor_pl = self.editor_pango_layout(&engine);
                    let (click_result, engine_action) = {
                        let layout_ref = self.cached_screen_layout.borrow();
                        if let Some(ref layout) = *layout_ref {
                            handle_mouse_click(
                                &mut engine,
                                x,
                                y,
                                alt,
                                self.cached_line_height,
                                self.cached_char_width,
                                &editor_pl,
                                layout,
                                &self.cached_tab_pixel_hits.borrow(),
                                &self.tab_slot_positions.borrow(),
                                &self.diff_btn_map.borrow(),
                                &self.split_btn_map.borrow(),
                                &self.action_btn_map.borrow(),
                                &self.status_segment_map.borrow(),
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
                            // async Msg::ToggleTerminal) so the panel
                            // appears on this same draw cycle.
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
                            self.dispatch(Msg::ShowCloseTabConfirm);
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
                            self.tab_drag_start = Some((x, y));
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

    /// Compute the picker popup's bounds in DA-local pixels. Shared by
    /// the click handler (to push into the modal stack) and the drag
    /// guard (to decide if a drag started inside the popup).
    fn compute_picker_popup_bounds(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
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
        (
            geo.popup_x as f64,
            geo.popup_y as f64,
            geo.popup_w as f64,
            geo.popup_h as f64,
        )
    }

    fn handle_mouse_drag_msg(&mut self, x: f64, y: f64, width: f64, height: f64) {
        // Phase B.4 drag dispatch: feed the move through quadraui's
        // dispatcher so an active drag (scrollbar, handle, etc.)
        // translates into primitive-specific events, then guard
        // against drag-events-inside-modal leaking through to the
        // base layer (#192).
        //
        // Keep the stack fresh: if the picker is open, ensure its
        // current bounds are recorded (popup size depends on
        // has_preview which can change mid-picker).
        {
            let engine = self.engine.borrow();
            let picker_open = engine.picker_open;
            drop(engine);
            let picker_id = quadraui::WidgetId::new("picker");
            if picker_open {
                let (px, py, pw, ph) = self.compute_picker_popup_bounds(width, height);
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .push(
                        picker_id.clone(),
                        quadraui::Rect {
                            x: px as f32,
                            y: py as f32,
                            width: pw as f32,
                            height: ph as f32,
                        },
                    );
            } else {
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&picker_id);
            }

            let drag_rc = self.backend.borrow().drag_state_handle();
            let drag = drag_rc.borrow();
            let drag_active = drag.is_active();
            if drag_active {
                // Run dispatch_mouse_drag: emits MouseMoved + any
                // primitive-specific drag-update events.
                let events = quadraui::dispatch_mouse_drag(
                    &drag,
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    Default::default(),
                );
                drop(drag);
                // Apply each scroll event by widget id.
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                        match widget.as_str() {
                            "picker" => {
                                let lh = self.cached_line_height.max(1.0);
                                let has_preview = self.engine.borrow().picker_preview.is_some();
                                let geo = render::PickerGeometry::compute(
                                    width as f32,
                                    height as f32,
                                    has_preview,
                                    &render::gtk_picker_sizing(lh as f32),
                                );
                                let vis = geo.visible_rows;
                                let mut engine = self.engine.borrow_mut();
                                engine.picker_scroll_top = *new_offset;
                                if engine.picker_selected < *new_offset {
                                    engine.picker_selected = *new_offset;
                                } else if vis > 0 && engine.picker_selected >= *new_offset + vis {
                                    engine.picker_selected = *new_offset + vis - 1;
                                }
                                engine.picker_load_preview();
                                self.draw_needed.set(true);
                            }
                            "editor_hover" => {
                                self.engine
                                    .borrow_mut()
                                    .editor_hover_set_scroll(*new_offset);
                                self.draw_needed.set(true);
                            }
                            "terminal_scrollback" => {
                                if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                                    term.set_scroll_offset(*new_offset);
                                }
                                self.draw_needed.set(true);
                            }
                            "debug_output" => {
                                let mut engine = self.engine.borrow_mut();
                                engine.debug_output_scroll = *new_offset;
                                engine.debug_output_auto_scroll = false;
                                self.draw_needed.set(true);
                            }
                            w if w.starts_with("editor:h_sb:") => {
                                if let Some(id_str) = w.strip_prefix("editor:h_sb:") {
                                    if let Ok(id) = id_str.parse::<usize>() {
                                        let win_id = core::WindowId(id);
                                        self.engine
                                            .borrow_mut()
                                            .set_scroll_left_for_window(win_id, *new_offset);
                                        self.draw_needed.set(true);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                return;
            }
            drop(drag);

            // Editor hover popup text selection drag (#216). Must run
            // BEFORE the modal-stack-swallow guard below so an
            // in-progress selection drag inside the popup (which is
            // now on the modal stack) doesn't get short-circuited.
            {
                let engine = self.engine.borrow();
                if engine.editor_hover_has_focus
                    && engine
                        .editor_hover
                        .as_ref()
                        .is_some_and(|h| h.selection.is_some())
                {
                    if let Some((px, py, _pw, _ph)) = self.editor_hover_popup_rect.get() {
                        let padding = 4.0;
                        let lh = self.cached_line_height.max(1.0);
                        let scroll = engine
                            .editor_hover
                            .as_ref()
                            .map(|h| h.scroll_top)
                            .unwrap_or(0);
                        drop(engine);
                        let rel_x = x - px - padding;
                        let rel_y = y - py - padding;
                        let content_line = (rel_y / lh).max(0.0) as usize + scroll;
                        let content_col = self.pixel_to_editor_hover_col(rel_x, content_line);
                        self.engine
                            .borrow_mut()
                            .editor_hover_extend_selection(content_line, content_col);
                        self.draw_needed.set(true);
                        return;
                    }
                }
            }

            let stack_rc = self.backend.borrow().modal_stack_handle();
            let stack = stack_rc.borrow();
            let hit_point = quadraui::Point {
                x: x as f32,
                y: y as f32,
            };
            if stack.hit_test(hit_point).is_some() {
                // Drag landed inside an open modal but there's no
                // active drag target — swallow so it doesn't leak to
                // the editor (#192). Active modal drags have already
                // been handled above.
                return;
            }
        }
        // Tab drag-and-drop handling.
        if self.tab_dragging {
            // Update drop zone while dragging, using the per-group bounds cached by
            // render_content. Cursor (x, y) and those bounds are both in absolute
            // surface coordinates, so the hit-test matches what the overlay draws.
            // (#515 — previously used relative 0-based bounds vs an absolute cursor,
            // which misclassified the zone after a split.)
            let groups = self.cached_drop_groups.borrow();
            let zone = render::compute_tab_drop_zone(
                x as f32,
                y as f32,
                &groups,
                self.cached_drop_tbh.get(),
            );
            drop(groups);
            self.tab_drag_drop_zone = zone;
            self.draw_needed.set(true);
            return;
        }
        // Check if a tab drag should start (mouse moved far enough from tab click).
        if let Some((sx, sy)) = self.tab_drag_start {
            let dx = x - sx;
            let dy = y - sy;
            if dx * dx + dy * dy > 64.0 {
                let layout_ref = self.cached_screen_layout.borrow();
                let Some(ref layout) = *layout_ref else {
                    self.tab_drag_start = None;
                    return;
                };
                let mut engine = self.engine.borrow_mut();
                let editor_pl = self.editor_pango_layout(&engine);
                let target = pixel_to_click_target(
                    &mut engine,
                    sx,
                    sy,
                    self.cached_line_height,
                    self.cached_char_width,
                    &editor_pl,
                    layout,
                    &self.cached_tab_pixel_hits.borrow(),
                    &self.tab_slot_positions.borrow(),
                    &self.diff_btn_map.borrow(),
                    &self.split_btn_map.borrow(),
                    &self.action_btn_map.borrow(),
                    &self.status_segment_map.borrow(),
                );
                if let ClickTarget::TabBar = target {
                    // The tab was already switched by pixel_to_click_target.
                    // Use the active group + active tab as the drag source.
                    let gid = engine.active_group;
                    let tidx = engine
                        .editor_groups
                        .get(&gid)
                        .map(|g| g.active_tab)
                        .unwrap_or(0);
                    self.tab_drag_source = Some((gid, tidx));
                    self.tab_drag_drop_zone = core::window::DropZone::None;
                    self.tab_dragging = true;
                    self.tab_drag_start = None;
                    self.draw_needed.set(true);
                    return;
                }
                // Not a tab — clear drag start and fall through.
                self.tab_drag_start = None;
            } else {
                // Haven't moved enough yet, don't start any drag.
                return;
            }
        }
        // Editor group divider drag — adjust split ratio.
        if let Some(split_index) = self.group_divider_dragging {
            let engine = self.engine.borrow();
            let lh = self.cached_line_height;
            let editor_bottom = gtk_editor_bottom(&engine, width, height, lh);
            drop(engine);
            let content_bounds = core::window::WindowRect::new(0.0, 0.0, width, editor_bottom);
            let dividers = self
                .engine
                .borrow()
                .group_layout
                .dividers(content_bounds, &mut 0);
            if let Some(div) = dividers.iter().find(|d| d.split_index == split_index) {
                let mouse_pos = match div.direction {
                    core::window::SplitDirection::Vertical => x,
                    core::window::SplitDirection::Horizontal => y,
                };
                let new_ratio = (mouse_pos - div.axis_start) / div.axis_size;
                self.engine
                    .borrow_mut()
                    .group_layout
                    .set_ratio_at_index(split_index, new_ratio);
            }
            self.draw_needed.set(true);
            return;
        }
        // Terminal split divider drag — update visual position (no PTY resize yet).
        if self.terminal_split_dragging {
            if self.cached_char_width > 0.0 {
                const SB_W: f64 = 6.0;
                let min_x = self.cached_char_width * 5.0;
                let max_x = (width - SB_W - self.cached_char_width * 5.0).max(min_x);
                let clamped_x = x.clamp(min_x, max_x);
                let left_cols = (clamped_x / self.cached_char_width) as u16;
                self.engine
                    .borrow_mut()
                    .terminal_split_set_drag_cols(left_cols);
                self.draw_needed.set(true);
            }
        // Terminal panel resize drag.
        } else if self.terminal_resize_dragging {
            if self.cached_line_height > 0.0 {
                let global_status_rows = if self.engine.borrow().settings.window_status_line {
                    0.0
                } else {
                    1.0
                };
                let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                let available = (height - y - status_h).max(0.0);
                // Leave at least 4 editor lines visible (+ tab bar chrome)
                let min_editor_lines = 4.0 + 1.0; // 4 lines + tab bar
                let max_rows = ((height - status_h - min_editor_lines * self.cached_line_height)
                    / self.cached_line_height) as u16;
                let max_rows = max_rows.saturating_sub(2).max(5);
                let new_rows = ((available / self.cached_line_height) as u16)
                    .saturating_sub(2)
                    .clamp(5, max_rows);
                self.engine.borrow_mut().session.terminal_panel_rows = new_rows;
                self.draw_needed.set(true);
            }
        } else {
            // Drag in the terminal content area (text selection). Geometry is
            // cached at paint time on engine.bottom_panel_geometry (#418).
            let content_row = match self.engine.borrow().resolve_bottom_panel_zone(y) {
                Some(crate::core::engine::BottomPanelZone::Content { row_offset }) => {
                    Some(row_offset)
                }
                _ => None,
            };
            if let Some(row) = content_row {
                let col = (x / self.cached_char_width.max(1.0)) as u16;
                // #533: shared drag handler — tries forward_mouse(Move)
                // when the child has mouse reporting, falls back to local
                // selection update.
                self.engine.borrow_mut().handle_terminal_pane_drag(col, row);
                self.draw_needed.set(true);
            } else {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    let editor_pl = self.editor_pango_layout(&engine);
                    handle_mouse_drag(
                        &mut engine,
                        x,
                        y,
                        self.cached_line_height,
                        self.cached_char_width,
                        &editor_pl,
                        layout,
                        &self.cached_tab_pixel_hits.borrow(),
                        &self.tab_slot_positions.borrow(),
                        &self.diff_btn_map.borrow(),
                        &self.split_btn_map.borrow(),
                        &self.action_btn_map.borrow(),
                        &self.status_segment_map.borrow(),
                    );
                }
                self.draw_needed.set(true);
            }
        }
    }

    fn handle_mouse_up_msg(&mut self) {
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

        // Tab drag drop.
        if self.tab_dragging {
            self.tab_dragging = false;
            if let Some((src_gid, src_tab_idx)) = self.tab_drag_source.take() {
                let zone = self.tab_drag_drop_zone;
                self.tab_drag_drop_zone = core::window::DropZone::None;
                self.engine
                    .borrow_mut()
                    .apply_tab_drop_zone(src_gid, src_tab_idx, zone);
            }
            self.draw_needed.set(true);
        }
        self.tab_drag_start = None;
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
                    let da_w = if let Some(da) = self.drawing_area.borrow().as_ref() {
                        da.width() as f64
                    } else {
                        800.0
                    };
                    const SB_W: f64 = 6.0;
                    let total_cols = ((da_w - SB_W) / self.cached_char_width) as u16;
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
            let cols = if let Some(da) = self.drawing_area.borrow().as_ref() {
                if self.cached_char_width > 0.0 {
                    (da.width() as f64 / self.cached_char_width) as u16
                } else {
                    80
                }
            } else {
                80
            }
            .max(40);
            self.engine.borrow_mut().terminal_resize(cols, rows);
            let _ = self.engine.borrow().session.save();
        }
        self.h_sb_drag_cell.set(None);
        self.group_divider_dragging = None;
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

    fn handle_terminal_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ToggleTerminal => {
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
            Msg::ToggleTerminalMaximize => {
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
            Msg::OpenTerminalAt(dir) => {
                let cols = self.terminal_cols();
                let rows = self.engine.borrow().session.terminal_panel_rows;
                self.engine
                    .borrow_mut()
                    .terminal_new_tab_at(cols, rows, Some(&dir));
                self.draw_needed.set(true);
            }
            Msg::NewTerminalTab => {
                let cols = self.terminal_cols();
                let rows = self.engine.borrow().session.terminal_panel_rows;
                self.engine.borrow_mut().terminal_new_tab(cols, rows);
                self.draw_needed.set(true);
            }
            Msg::RunCommandInTerminal(cmd) => {
                let cols = self.terminal_cols();
                let rows = self.engine.borrow().session.terminal_panel_rows;
                self.engine
                    .borrow_mut()
                    .terminal_run_command(&cmd, cols, rows);
                self.draw_needed.set(true);
            }
            Msg::TerminalSwitchTab(idx) => {
                self.engine.borrow_mut().terminal_switch_tab(idx);
                self.draw_needed.set(true);
            }
            Msg::TerminalCloseActiveTab => {
                self.engine.borrow_mut().terminal_close_active_tab();
                self.draw_needed.set(true);
            }
            Msg::TerminalKill => {
                self.engine.borrow_mut().terminal_close_active_tab();
                self.draw_needed.set(true);
            }
            Msg::TerminalToggleSplit => {
                let (full_cols, rows) = {
                    let da_w = if let Some(da) = self.drawing_area.borrow().as_ref() {
                        da.width() as f64
                    } else {
                        0.0
                    };
                    let cols = if self.cached_char_width > 0.0 {
                        (da_w / self.cached_char_width) as u16
                    } else {
                        80
                    };
                    let rows = self.engine.borrow().session.terminal_panel_rows;
                    (cols, rows)
                };
                self.engine
                    .borrow_mut()
                    .terminal_toggle_split(full_cols, rows);
                self.draw_needed.set(true);
            }
            Msg::TerminalSplitFocus(idx) => {
                let mut engine = self.engine.borrow_mut();
                if engine.terminal_split && idx < engine.terminal_panes.len() {
                    engine.terminal_active = idx;
                }
                self.draw_needed.set(true);
            }
            Msg::TerminalCopySelection => {
                let text = self.engine.borrow_mut().terminal_copy_selection();
                if let Some(text) = text {
                    let engine = self.engine.borrow();
                    if let Some(ref cb) = engine.clipboard_write {
                        let _ = cb(text.as_str());
                    }
                }
            }
            Msg::TerminalPasteClipboard => {
                let text = self
                    .engine
                    .borrow()
                    .clipboard_read
                    .as_ref()
                    .and_then(|cb| cb().ok());
                if let Some(text) = text {
                    self.engine.borrow_mut().terminal_write(text.as_bytes());
                }
                self.draw_needed.set(true);
            }
            Msg::TerminalMouseDown { row, col } => {
                if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                    term.selection = Some(crate::core::terminal::TermSelection {
                        start_row: row,
                        start_col: col,
                        end_row: row,
                        end_col: col,
                    });
                }
                self.draw_needed.set(true);
            }
            Msg::TerminalMouseDrag { row, col } => {
                if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                    if let Some(ref mut sel) = term.selection {
                        sel.end_row = row;
                        sel.end_col = col;
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::TerminalMouseUp => {
                // Selection stays in place; user can now copy
                self.draw_needed.set(true);
            }
            Msg::TerminalFindOpen => {
                self.engine.borrow_mut().terminal_find_open();
                self.draw_needed.set(true);
            }
            Msg::TerminalFindClose => {
                self.engine.borrow_mut().terminal_find_close();
                self.draw_needed.set(true);
            }
            Msg::TerminalFindChar(ch) => {
                self.engine.borrow_mut().terminal_find_char(ch);
                self.draw_needed.set(true);
            }
            Msg::TerminalFindBackspace => {
                self.engine.borrow_mut().terminal_find_backspace();
                self.draw_needed.set(true);
            }
            Msg::TerminalFindNext => {
                self.engine.borrow_mut().terminal_find_next();
                self.draw_needed.set(true);
            }
            Msg::TerminalFindPrev => {
                self.engine.borrow_mut().terminal_find_prev();
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    fn sync_menu_overlay(&self) {
        let is_open = self.engine.borrow().menu_system.borrow().is_open();
        if let Some(ref da) = *self.menu_dropdown_da.borrow() {
            da.set_can_target(is_open);
            da.queue_draw();
        }
        if let Some(ref da) = *self.menu_bar_da.borrow() {
            da.queue_draw();
        }
    }

    fn handle_menu_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ToggleMenuBar => {
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::MruNavBack => {
                self.engine.borrow_mut().tab_nav_back();
                self.draw_needed.set(true);
            }
            Msg::OpenCommandCenter => {
                self.engine.borrow_mut().open_command_center();
                self.draw_needed.set(true);
            }
            Msg::MruNavForward => {
                self.engine.borrow_mut().tab_nav_forward();
                self.draw_needed.set(true);
            }
            Msg::MenuRedraw => {
                self.sync_menu_overlay();
                self.draw_needed.set(true);
            }
            Msg::HandleMenuAction(action) => {
                match action.as_str() {
                    "open_file_dialog" => {
                        self.dispatch(Msg::OpenFileDialog);
                    }
                    "open_folder_dialog" => {
                        self.dispatch(Msg::OpenFolderDialog);
                    }
                    "open_workspace_dialog" => {
                        self.engine.borrow_mut().open_workspace_from_file();
                        self.dispatch(Msg::RefreshFileTree);
                    }
                    "save_workspace_as_dialog" => {
                        self.dispatch(Msg::SaveWorkspaceAsDialog);
                    }
                    "openrecent" => {
                        self.dispatch(Msg::OpenRecentDialog);
                    }
                    "find" => {
                        self.engine.borrow_mut().open_find_replace();
                        self.draw_needed.set(true);
                    }
                    "quit_menu" => {
                        if self.engine.borrow().has_any_unsaved() {
                            self.dispatch(Msg::ShowQuitConfirm);
                        } else {
                            self.save_session_and_exit();
                        }
                    }
                    _ => {
                        let engine_action = self.engine.borrow_mut().dispatch_menu_action(&action);
                        match engine_action {
                            EngineAction::Quit | EngineAction::SaveQuit => {
                                self.dispatch(Msg::QuitConfirmed);
                            }
                            EngineAction::QuitWithUnsaved => {
                                self.dispatch(Msg::ShowQuitConfirm);
                            }
                            EngineAction::ToggleSidebar => {
                                self.sync_sidebar_from_engine();
                            }
                            EngineAction::OpenTerminal => {
                                self.dispatch(Msg::NewTerminalTab);
                            }
                            _ => {}
                        }
                    }
                }
                self.sync_menu_overlay();
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    fn handle_debug_sidebar_msg(&mut self, msg: Msg) {
        match msg {
            Msg::DebugSidebarClick(click_x, y) => {
                let lh = self.debug_sidebar_lh.get().max(1.0);
                let mut engine = self.engine.borrow_mut();
                engine.dap_sidebar_has_focus = true;

                if y < 2.0 * lh {
                    let guard = engine.dap_sidebar_action_hits.borrow();
                    let matched = guard
                        .as_ref()
                        .map(|l| {
                            matches!(
                                l.hit_test(click_x as f32, 0.0),
                                quadraui::StatusBarHit::Segment(_)
                            )
                        })
                        .unwrap_or(false);
                    drop(guard);
                    if matched {
                        engine.handle_dap_sidebar_action_click();
                    }
                } else {
                    let rect = engine.dap_sidebar_body_rect.get();
                    render::populate_dap_sidebar_system(&engine);
                    let click_event = quadraui::UiEvent::MouseDown {
                        widget: None,
                        button: quadraui::MouseButton::Left,
                        position: quadraui::Point::new(click_x as f32, y as f32),
                        modifiers: quadraui::Modifiers::default(),
                    };
                    let backend_rc = self.backend.clone();
                    let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                        &click_event,
                        &mut *backend_rc.borrow_mut(),
                        rect,
                    );
                    engine.dispatch_dap_sidebar_event(sidebar_event);
                }
                drop(engine);

                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.grab_focus();
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::DebugSidebarKey(key_name, ctrl) => {
                let mut engine = self.engine.borrow_mut();
                if engine.dialog.is_some() {
                    if !util::is_modifier_only_key(&key_name) {
                        engine.handle_key(&key_name, key_name.chars().next(), ctrl);
                    }
                    drop(engine);
                    self.focus_editor_if_needed(false);
                    self.draw_needed.set(true);
                    return;
                }
                let rect = engine.dap_sidebar_body_rect.get();
                render::populate_dap_sidebar_system(&engine);
                let mapped = map_gtk_key_name(key_name.as_str());
                let key = gtk_key_name_to_quadraui(mapped, ctrl);
                let consumed = if let Some(ui_event) = key {
                    let backend_rc = self.backend.clone();
                    let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                        &ui_event,
                        &mut *backend_rc.borrow_mut(),
                        rect,
                    );
                    engine.dispatch_dap_sidebar_event(sidebar_event)
                } else {
                    false
                };
                if !consumed {
                    engine.dispatch_dap_sidebar_action_key(mapped);
                }
                let still_focused = engine.dap_sidebar_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::DebugSidebarScroll(dy) => {
                let engine = self.engine.borrow_mut();
                let rect = engine.dap_sidebar_body_rect.get();
                render::populate_dap_sidebar_system(&engine);
                let scroll_event = quadraui::UiEvent::Scroll {
                    widget: None,
                    delta: quadraui::ScrollDelta::new(0.0, -dy as f32),
                    position: quadraui::Point::new(rect.x + 1.0, rect.y + 1.0),
                };
                let backend_rc = self.backend.clone();
                engine.dap_sidebar_system.borrow_mut().handle(
                    &scroll_event,
                    &mut *backend_rc.borrow_mut(),
                    rect,
                );
                drop(engine);
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::DebugSidebarDrag(x, y) => {
                let engine = self.engine.borrow();
                let rect = engine.dap_sidebar_body_rect.get();
                let event = quadraui::UiEvent::MouseMoved {
                    position: quadraui::Point::new(x as f32, y as f32),
                    buttons: quadraui::ButtonMask {
                        left: true,
                        ..Default::default()
                    },
                };
                let backend_rc = self.backend.clone();
                engine.dap_sidebar_system.borrow_mut().handle(
                    &event,
                    &mut *backend_rc.borrow_mut(),
                    rect,
                );
                drop(engine);
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::DebugSidebarDragEnd(x, y) => {
                let engine = self.engine.borrow();
                let rect = engine.dap_sidebar_body_rect.get();
                let event = quadraui::UiEvent::MouseUp {
                    widget: None,
                    button: quadraui::MouseButton::Left,
                    position: quadraui::Point::new(x as f32, y as f32),
                };
                let backend_rc = self.backend.clone();
                engine.dap_sidebar_system.borrow_mut().handle(
                    &event,
                    &mut *backend_rc.borrow_mut(),
                    rect,
                );
                drop(engine);
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    fn handle_sc_sidebar_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ScSidebarClick(x_click, y, n_press) => {
                let lh = self.cached_ui_line_height;
                if lh <= 0.0 {
                    return;
                }
                // tree_has_focus removed (A.2b-2); engine.explorer_has_focus is authoritative
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.grab_focus();
                }
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.sc_set_focus(true);
                    // Route via cached SidebarPanelLayout (#509).
                    // Header / commit-input area: identified by panel-local y
                    // (the panel starts at 0,0 in the drawing area).
                    let gap = (lh * 0.3).round();
                    let commit_rows = engine.sc_commit_message.split('\n').count().max(1);
                    let commit_h = commit_rows as f64 * lh;
                    let header_end = lh;
                    let commit_top = header_end + gap;
                    let commit_bottom = commit_top + commit_h;

                    if y < header_end {
                        engine.sc_commit_input_active = false;
                    } else if y >= commit_top && y < commit_bottom {
                        engine.sc_commit_input_active = true;
                        engine.sc_commit_cursor = engine.sc_commit_message.len();
                    } else {
                        // Bottom slab: delegate to SidebarPanelLayout hit_test.
                        engine.sc_commit_input_active = false;
                        let hit = {
                            let layout = engine.sc_panel_layout.borrow();
                            layout
                                .as_ref()
                                .map(|l| l.hit_test(x_click as f32, y as f32))
                        };
                        match hit {
                            Some(quadraui::SidebarPanelHit::ToolbarButton(_)) => {
                                if let Some(idx) = engine.sc_button_hit(x_click as f32, y as f32) {
                                    engine.sc_activate_button(idx);
                                }
                            }
                            Some(quadraui::SidebarPanelHit::Content { .. }) => {
                                let click_ev = quadraui::UiEvent::MouseDown {
                                    widget: None,
                                    button: quadraui::MouseButton::Left,
                                    position: quadraui::Point::new(x_click as f32, y as f32),
                                    modifiers: quadraui::Modifiers::default(),
                                };
                                engine.handle_sc_sidebar_ui_event(click_ev);
                                if n_press >= 2 {
                                    let double_ev = quadraui::UiEvent::DoubleClick {
                                        widget: None,
                                        position: quadraui::Point::new(x_click as f32, y as f32),
                                    };
                                    engine.handle_sc_sidebar_ui_event(double_ev);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ScSidebarMotion(mx, my) => {
                // Route via cached SidebarPanelLayout (#509) — no per-frame
                // button-row or section-top arithmetic.
                let lh = self.cached_ui_line_height;
                let mut engine = self.engine.borrow_mut();

                let hit = if mx < 0.0 {
                    // Mouse left the panel (leave event sends -1,-1).
                    None
                } else {
                    let layout = engine.sc_panel_layout.borrow();
                    layout.as_ref().map(|l| l.hit_test(mx as f32, my as f32))
                };

                let old = engine.sc_button_hovered;
                match hit {
                    Some(quadraui::SidebarPanelHit::ToolbarButton(_))
                    | Some(quadraui::SidebarPanelHit::ToolbarEmpty) => {
                        engine.sc_button_hovered = engine.sc_button_hit(mx as f32, my as f32);
                        engine.dismiss_panel_hover();
                    }
                    Some(quadraui::SidebarPanelHit::Content { y: content_y, .. }) => {
                        engine.sc_button_hovered = None;
                        // Walk sections within the content area using content-local y.
                        // content_y is in pixels relative to content_bounds.y.
                        let item_height = (lh * 1.4).round();
                        let staged_count = engine
                            .sc_file_statuses
                            .iter()
                            .filter(|f| f.staged.is_some())
                            .count();
                        let unstaged_count = engine
                            .sc_file_statuses
                            .iter()
                            .filter(|f| f.unstaged.is_some())
                            .count();
                        let show_worktrees = engine.sc_worktrees.len() > 1;
                        let wt_count = engine.sc_worktrees.len();
                        let log_count = engine.sc_log.len();

                        let my_local = content_y as f64;
                        let mut y_off = 0.0f64;
                        let mut flat_idx = 0usize;
                        let mut hit_flat: Option<usize> = None;

                        // Walk each section: header(lh) + items(item_height) if expanded.
                        struct Section {
                            count: usize,
                            expanded: bool,
                        }
                        let sections = [
                            Section {
                                count: staged_count,
                                expanded: engine.sc_sections_expanded[0],
                            },
                            Section {
                                count: unstaged_count,
                                expanded: engine.sc_sections_expanded[1],
                            },
                        ];
                        for sec in &sections {
                            if my_local >= y_off && my_local < y_off + lh {
                                hit_flat = Some(flat_idx);
                                break;
                            }
                            y_off += lh;
                            flat_idx += 1;
                            if sec.expanded {
                                for _ in 0..sec.count {
                                    if my_local >= y_off && my_local < y_off + item_height {
                                        hit_flat = Some(flat_idx);
                                        break;
                                    }
                                    y_off += item_height;
                                    flat_idx += 1;
                                }
                                if hit_flat.is_some() {
                                    break;
                                }
                            }
                        }
                        if hit_flat.is_none() && show_worktrees {
                            if my_local >= y_off && my_local < y_off + lh {
                                hit_flat = Some(flat_idx);
                            }
                            y_off += lh;
                            flat_idx += 1;
                            if hit_flat.is_none() && engine.sc_sections_expanded[2] {
                                for _ in 0..wt_count {
                                    if my_local >= y_off && my_local < y_off + item_height {
                                        hit_flat = Some(flat_idx);
                                        break;
                                    }
                                    y_off += item_height;
                                    flat_idx += 1;
                                }
                            }
                        }
                        if hit_flat.is_none() {
                            if my_local >= y_off && my_local < y_off + lh {
                                hit_flat = Some(flat_idx);
                            }
                            y_off += lh;
                            flat_idx += 1;
                            if hit_flat.is_none() && engine.sc_sections_expanded[3] {
                                for _ in 0..log_count {
                                    if my_local >= y_off && my_local < y_off + item_height {
                                        hit_flat = Some(flat_idx);
                                        break;
                                    }
                                    y_off += item_height;
                                    flat_idx += 1;
                                }
                            }
                        }
                        let _ = (y_off, flat_idx); // loop accumulators only

                        if let Some(fi) = hit_flat {
                            if engine.panel_hover_mouse_move("source_control", "", fi) {
                                drop(engine);
                                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                                    da.queue_draw();
                                }
                                return;
                            }
                        } else {
                            engine.dismiss_panel_hover();
                        }
                    }
                    _ => {
                        // Outside panel or mouse left — clear button hover and
                        // dwell tracking.
                        engine.sc_button_hovered = None;
                        engine.dismiss_panel_hover();
                    }
                }

                if engine.sc_button_hovered != old {
                    drop(engine);
                    if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                }
            }
            Msg::ScKey(key_name, ctrl) => {
                let mut engine = self.engine.borrow_mut();
                if engine.dialog.is_some() {
                    if !util::is_modifier_only_key(&key_name) {
                        engine.handle_key(&key_name, key_name.chars().next(), ctrl);
                    }
                    drop(engine);
                    self.focus_editor_if_needed(false);
                    self.draw_needed.set(true);
                    return;
                }
                let (mapped, unicode) = map_gtk_key_with_unicode(key_name.as_str());
                engine.dispatch_sc_sidebar_key_unified(mapped, ctrl, unicode);
                let still_focused = engine.sc_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ScSidebarEvent(ev) => {
                let mut engine = self.engine.borrow_mut();
                engine.handle_sc_sidebar_ui_event(ev);
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    fn offset_ext_sidebar_event(
        &self,
        ev: &quadraui::UiEvent,
        chrome_h: f32,
        body_y: f32,
    ) -> quadraui::UiEvent {
        let offset = |p: quadraui::Point| quadraui::Point::new(p.x, p.y - chrome_h + body_y);
        match ev {
            quadraui::UiEvent::MouseDown {
                widget,
                button,
                position,
                modifiers,
            } => quadraui::UiEvent::MouseDown {
                widget: widget.clone(),
                button: *button,
                position: offset(*position),
                modifiers: *modifiers,
            },
            quadraui::UiEvent::MouseUp {
                widget,
                button,
                position,
            } => quadraui::UiEvent::MouseUp {
                widget: widget.clone(),
                button: *button,
                position: offset(*position),
            },
            quadraui::UiEvent::MouseMoved { position, buttons } => quadraui::UiEvent::MouseMoved {
                position: offset(*position),
                buttons: *buttons,
            },
            quadraui::UiEvent::Scroll {
                widget,
                delta,
                position,
            } => quadraui::UiEvent::Scroll {
                widget: widget.clone(),
                delta: *delta,
                position: offset(*position),
            },
            quadraui::UiEvent::DoubleClick { widget, position } => quadraui::UiEvent::DoubleClick {
                widget: widget.clone(),
                position: offset(*position),
            },
            other => other.clone(),
        }
    }

    fn handle_ext_sidebar_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ExtSidebarKey(key_name, unicode) => {
                let mapped = map_gtk_key_name(key_name.as_str());
                let mut engine = self.engine.borrow_mut();
                if engine.dialog.is_some() {
                    if !util::is_modifier_only_key(&key_name) {
                        engine.handle_key(mapped, unicode, false);
                    }
                    drop(engine);
                    self.focus_editor_if_needed(false);
                    self.draw_needed.set(true);
                    if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                    return;
                }
                engine.dispatch_ext_sidebar_key_unified(mapped, unicode);
                let still_focused = engine.ext_sidebar_has_focus;
                let has_dialog = engine.dialog.is_some();
                drop(engine);
                self.focus_after_sidebar_key(still_focused && !has_dialog);
                if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ExtSidebarEvent(ev) => {
                let mut engine = self.engine.borrow_mut();
                let line_height = self.cached_ui_line_height.max(1.0);
                let chrome_h = 2.0 * line_height;
                let rect = engine.ext_sidebar_body_rect.get();
                let is_click = matches!(ev, quadraui::UiEvent::MouseDown { .. });
                let is_double = matches!(ev, quadraui::UiEvent::DoubleClick { .. });
                let click_y = match &ev {
                    quadraui::UiEvent::MouseDown { position, .. }
                    | quadraui::UiEvent::DoubleClick { position, .. } => Some(position.y as f64),
                    _ => None,
                };
                if let Some(y) = click_y {
                    engine.ext_sidebar_has_focus = true;
                    if y < line_height {
                        // Panel header — no-op.
                    } else if y < chrome_h {
                        engine.ext_sidebar_input_active = true;
                    } else {
                        let adjusted = self.offset_ext_sidebar_event(&ev, chrome_h as f32, rect.y);
                        engine.handle_ext_sidebar_ui_event(adjusted);
                    }
                    if is_double {
                        engine.ext_open_selected_readme();
                    }
                } else {
                    let adjusted = self.offset_ext_sidebar_event(&ev, chrome_h as f32, rect.y);
                    engine.handle_ext_sidebar_ui_event(adjusted);
                }
                let still_focused = engine.ext_sidebar_has_focus;
                let has_dialog = engine.dialog.is_some();
                drop(engine);
                if is_click || is_double {
                    self.focus_editor_if_needed(still_focused && !has_dialog);
                }
                self.draw_needed.set(true);
                if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
            }
            _ => unreachable!(),
        }
    }

    /// Phase A.3c-2: handle key/click/scroll messages routed from the
    /// Settings sidebar DrawingArea. Geometry must mirror
    /// `draw_settings_panel` in `src/gtk/draw.rs`:
    ///   row 0 = header, row 1 = search, body = form rows of `row_h`,
    ///   bottom row = "Open settings.json" footer.
    fn handle_settings_msg(&mut self, msg: Msg) {
        match msg {
            Msg::SettingsKey(key_name, ctrl, unicode) => {
                let mapped = map_gtk_key_name(key_name.as_str());
                let mut engine = self.engine.borrow_mut();
                if engine.dialog.is_some() {
                    if !util::is_modifier_only_key(&key_name) {
                        engine.handle_key(mapped, unicode, ctrl);
                    }
                } else {
                    engine.handle_settings_key(mapped, ctrl, unicode);
                }
                let still_focused = engine.settings_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.settings_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::SettingsClick(x_click, y_click, n_press) => {
                use crate::core::engine::SettingsRow;
                use crate::core::settings::{SettingType, SETTING_DEFS};

                let line_height = self.cached_ui_line_height.max(1.0);
                let row_h = (line_height * 1.4_f64).round();
                let body_top = line_height * 2.0; // header + search
                let panel_w = self
                    .settings_da_ref
                    .borrow()
                    .as_ref()
                    .map(|da| da.width() as f64)
                    .unwrap_or(0.0);
                let panel_h = self
                    .settings_da_ref
                    .borrow()
                    .as_ref()
                    .map(|da| da.height() as f64)
                    .unwrap_or(0.0);
                let footer_top = (panel_h - line_height).max(body_top);
                let body_h = (footer_top - body_top).max(0.0);

                // Grab focus so subsequent keys reach this panel's controller
                // (the activity-bar button keeps focus by default after click).
                if let Some(ref da) = *self.settings_da_ref.borrow() {
                    da.grab_focus();
                }

                let mut engine = self.engine.borrow_mut();
                engine.settings_has_focus = true;

                let total = engine.settings_flat_list().len();
                let visible_rows = if row_h > 0.0 {
                    (body_h / row_h).floor() as usize
                } else {
                    0
                };
                let need_sb = visible_rows > 0 && total > visible_rows;
                let sb_w = if need_sb { 8.0 } else { 0.0 };
                let form_right = (panel_w - sb_w).max(0.0);

                // Route scrollbar clicks through FormController.
                if need_sb && x_click >= form_right {
                    let q_rect =
                        quadraui::Rect::new(0.0, body_top as f32, panel_w as f32, body_h as f32);
                    render::populate_settings_form_controller(&engine);
                    let event = quadraui::UiEvent::MouseDown {
                        button: quadraui::MouseButton::Left,
                        position: quadraui::Point::new(x_click as f32, y_click as f32),
                        modifiers: Default::default(),
                        widget: None,
                    };
                    let result = engine
                        .settings_form_controller
                        .borrow_mut()
                        .handle_cached(&event, q_rect);
                    if matches!(
                        result,
                        quadraui::FormControllerEvent::ScrollChanged
                            | quadraui::FormControllerEvent::Consumed
                    ) {
                        let new_offset = engine.settings_form_controller.borrow().scroll_offset();
                        engine.settings_scroll_top = new_offset;
                    }
                    drop(engine);
                    self.draw_needed.set(true);
                    if let Some(ref da) = *self.settings_da_ref.borrow() {
                        da.queue_draw();
                    }
                    return;
                }

                if y_click < line_height {
                    // Header row — no-op.
                } else if y_click < body_top {
                    // Search row — activate search input.
                    engine.settings_input_active = true;
                } else if y_click >= footer_top {
                    // Footer row — open settings.json.
                    drop(engine);
                    let settings_path = std::env::var("HOME")
                        .map(|h| format!("{}/.config/vimcode/settings.json", h))
                        .unwrap_or_else(|_| ".config/vimcode/settings.json".to_string());
                    self.engine
                        .borrow_mut()
                        .new_tab(Some(Path::new(&settings_path)));
                    self.draw_needed.set(true);
                    return;
                } else if row_h > 0.0 {
                    // Body row.
                    let local = ((y_click - body_top) / row_h) as usize;
                    let scroll = engine.settings_scroll_top;
                    let flat_idx = scroll + local;
                    if flat_idx < total {
                        engine.settings_selected = flat_idx;
                        let row = engine.settings_flat_list()[flat_idx].clone();
                        let is_category = matches!(
                            row,
                            SettingsRow::CoreCategory(_) | SettingsRow::ExtCategory(_)
                        );
                        // Section headers activate on single-click (matches
                        // explorer folders + SC section headers). Settings
                        // require double-click to avoid surprise edits.
                        if is_category || n_press >= 2 {
                            match row {
                                SettingsRow::CoreSetting(idx) => {
                                    let def = &SETTING_DEFS[idx];
                                    if matches!(
                                        def.setting_type,
                                        SettingType::Integer { .. } | SettingType::StringVal
                                    ) {
                                        engine.settings_editing = Some(idx);
                                        engine.settings_edit_buf =
                                            engine.settings.get_value_str(def.key);
                                    } else {
                                        engine.handle_settings_key("Return", false, None);
                                    }
                                }
                                _ => {
                                    engine.handle_settings_key("Return", false, None);
                                }
                            }
                        }
                    }
                }

                drop(engine);
                if let Some(ref da) = *self.settings_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::SettingsScroll(dy) => {
                let mut engine = self.engine.borrow_mut();
                let total = engine.settings_flat_list().len();
                let line_height = self.cached_ui_line_height.max(1.0);
                let row_h = (line_height * 1.4_f64).round();
                let body_top = line_height * 2.0;
                let panel_h = self
                    .settings_da_ref
                    .borrow()
                    .as_ref()
                    .map(|da| da.height() as f64)
                    .unwrap_or(0.0);
                let body_h = (panel_h - body_top - line_height).max(0.0);
                let visible_rows = (body_h / row_h).floor() as usize;
                let max_scroll = total.saturating_sub(visible_rows);
                // dy is normally ±1 per wheel notch; multiply for a 3-row jump.
                let step = if dy > 0.0 { 3 } else { -3 };
                let new_scroll = (engine.settings_scroll_top as isize + step as isize)
                    .clamp(0, max_scroll as isize) as usize;
                engine.settings_scroll_top = new_scroll;
                drop(engine);
                if let Some(ref da) = *self.settings_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    fn handle_ext_panel_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ExtPanelKey(key_name, unicode) => {
                let mapped = map_gtk_key_name(key_name.as_str());
                let mut engine = self.engine.borrow_mut();
                if engine.dialog.is_some() {
                    if !util::is_modifier_only_key(&key_name) {
                        engine.handle_key(mapped, unicode, false);
                    }
                    drop(engine);
                    self.focus_editor_if_needed(false);
                } else if engine.ext_panel_input_active {
                    engine.handle_ext_panel_input_key(mapped, false, unicode);
                    drop(engine);
                } else {
                    engine.handle_ext_panel_key(mapped, false, unicode);
                    let still_focused = engine.ext_panel_has_focus;
                    drop(engine);
                    self.focus_after_sidebar_key(still_focused);
                }
                self.sync_plus_register_to_clipboard();
                if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ExtPanelClick(x_click, y_click, n_press) => {
                // Dismiss any hover popup (links are handled by the overlay DA).
                {
                    let had_hover = self.engine.borrow().panel_hover.is_some();
                    if had_hover {
                        self.engine.borrow_mut().dismiss_panel_hover_now();
                        if let Some(ref da) = *self.panel_hover_da.borrow() {
                            da.queue_draw();
                        }
                    }
                }
                let mut engine = self.engine.borrow_mut();
                let line_height = self.cached_ui_line_height.max(1.0);

                engine.ext_panel_has_focus = true;

                // Row 0 is the header; optional input row follows when active/has text.
                let has_input_row = engine.ext_panel_input_active
                    || engine
                        .ext_panel_active
                        .as_ref()
                        .and_then(|n| engine.ext_panel_input_text.get(n))
                        .map(|t| !t.is_empty())
                        .unwrap_or(false);
                let content_top = line_height * if has_input_row { 2.0 } else { 1.0 };

                // Scrollbar click — proportional jump scroll
                let da_w = if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.width() as f64
                } else {
                    200.0
                };
                let flat_len = engine.ext_panel_flat_len();
                if x_click >= da_w - 8.0 && y_click >= content_top && flat_len > 0 {
                    let da_h = if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                        da.height() as f64
                    } else {
                        400.0
                    };
                    let content_h = da_h - content_top;
                    if content_h > 0.0 {
                        let ratio = (y_click - content_top) / content_h;
                        let new_top = (ratio * flat_len as f64) as usize;
                        engine.ext_panel_scroll_top = new_top.min(flat_len.saturating_sub(1));
                        drop(engine);
                        if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                            da.queue_draw();
                        }
                        self.draw_needed.set(true);
                        return;
                    }
                }

                let mut clicked_valid = false;
                if y_click >= content_top {
                    // Content rows: each row is line_height tall.
                    let row_idx = ((y_click - content_top) / line_height) as usize;
                    let flat_idx = engine.ext_panel_scroll_top + row_idx;
                    if flat_idx < flat_len {
                        engine.ext_panel_selected = flat_idx;
                        clicked_valid = true;
                    }
                }
                if n_press >= 2 {
                    // Double-click fires panel_double_click event + confirms selection.
                    engine.handle_ext_panel_double_click();
                    engine.handle_ext_panel_key("Return", false, None);
                    let still_focused = engine.ext_panel_has_focus;
                    drop(engine);
                    self.focus_editor_if_needed(still_focused);
                } else if clicked_valid {
                    // Single-click: toggle section headers and expandable items
                    engine.handle_ext_panel_key("Return", false, None);
                    drop(engine);
                } else {
                    drop(engine);
                }
                if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ExtPanelRightClick(x_click, y_click) => {
                let mut engine = self.engine.borrow_mut();
                let line_height = self.cached_line_height.max(1.0);
                engine.ext_panel_has_focus = true;
                // Map click to flat index (same as left-click).
                if y_click >= line_height {
                    let row_idx = ((y_click - line_height) / line_height) as usize;
                    let flat_idx = engine.ext_panel_scroll_top + row_idx;
                    let flat_len = engine.ext_panel_flat_len();
                    if flat_idx < flat_len {
                        engine.ext_panel_selected = flat_idx;
                    }
                }
                engine.open_ext_panel_context_menu(x_click as u16, y_click as u16);
                drop(engine);
                if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ExtPanelMouseMove(x_move, y_move) => {
                // Determine which flat item the mouse is over (row 0 is the header).
                let line_height = self.cached_line_height.max(1.0);
                let active = self.current_active_panel_id();
                let panel_name = match ext_panel_name(&active) {
                    Some(name) => name.to_string(),
                    None => return,
                };
                // Header row occupies row 0; content rows start at line_height.
                if y_move < line_height {
                    self.engine.borrow_mut().dismiss_panel_hover();
                    if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                        da.queue_draw();
                    }
                    return;
                }
                let scroll_top = self.engine.borrow().ext_panel_scroll_top;
                let row_idx = ((y_move - line_height) / line_height) as usize;
                let flat_idx = scroll_top + row_idx;
                let _ = x_move;
                let changed =
                    self.engine
                        .borrow_mut()
                        .panel_hover_mouse_move(&panel_name, "", flat_idx);
                if changed {
                    if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                        da.queue_draw();
                    }
                }
            }
            Msg::ExtPanelScroll(dy) => {
                let scroll_amount = (dy.abs() * 3.0).ceil() as usize;
                let mut engine = self.engine.borrow_mut();
                let flat_len = engine.ext_panel_flat_len();
                if dy > 0.0 {
                    engine.ext_panel_scroll_top = (engine.ext_panel_scroll_top + scroll_amount)
                        .min(flat_len.saturating_sub(1));
                } else {
                    engine.ext_panel_scroll_top =
                        engine.ext_panel_scroll_top.saturating_sub(scroll_amount);
                }
                drop(engine);
                if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.queue_draw();
                }
            }
            Msg::PanelHoverClick(click_x, click_y) => {
                // Check if click hit a link rect in the hover popup.
                let rects = self.panel_hover_link_rects.borrow();
                let hit = rects
                    .iter()
                    .find(|(rx, ry, rw, rh, _, _)| {
                        click_x >= *rx && click_x <= rx + rw && click_y >= *ry && click_y <= ry + rh
                    })
                    .cloned();
                drop(rects);
                if let Some((_rx, _ry, _rw, _rh, url, is_native)) = hit {
                    use crate::core::engine::DialogButton;
                    if url.starts_with("command:") {
                        // Command URI — dispatch to engine.
                        self.engine.borrow_mut().execute_command_uri(&url);
                    } else if is_native {
                        // Trusted link from native panel — open directly.
                        open_url(&url);
                    } else {
                        // Extension-provided link — show confirmation dialog.
                        let tag = format!("open_ext_url:{}", url);
                        self.engine.borrow_mut().show_dialog(
                            &tag,
                            "Open URL?",
                            vec![url],
                            vec![
                                DialogButton {
                                    label: "Cancel".to_string(),
                                    hotkey: 'c',
                                    action: "cancel".to_string(),
                                },
                                DialogButton {
                                    label: "Open".to_string(),
                                    hotkey: 'o',
                                    action: "open".to_string(),
                                },
                            ],
                        );
                        self.draw_needed.set(true);
                    }
                }
                // Dismiss popup after click.
                self.engine.borrow_mut().dismiss_panel_hover_now();
                if let Some(ref da) = *self.panel_hover_da.borrow() {
                    da.queue_draw();
                }
            }
            _ => unreachable!(),
        }
    }

    fn handle_ai_sidebar_msg(&mut self, msg: Msg) {
        match msg {
            Msg::AiSidebarKey(key_name, ctrl, unicode) => {
                if self.engine.borrow().dialog.is_some() {
                    if !util::is_modifier_only_key(&key_name) {
                        let mut engine = self.engine.borrow_mut();
                        engine.handle_key(&key_name, key_name.chars().next(), ctrl);
                        drop(engine);
                    }
                    self.focus_editor_if_needed(false);
                    self.draw_needed.set(true);
                    return;
                }
                // Ctrl-V: paste from system clipboard into AI input.
                if ctrl && key_name == "v" {
                    let text = self
                        .engine
                        .borrow()
                        .clipboard_read
                        .as_ref()
                        .and_then(|cb| cb().ok())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        self.engine.borrow_mut().ai_insert_text(&text);
                    }
                    if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                    return;
                }
                let mut engine = self.engine.borrow_mut();
                engine.handle_ai_panel_key(&key_name, ctrl, unicode);
                let still_focused = engine.ai_has_focus;
                drop(engine);
                self.focus_after_sidebar_key(still_focused);
                if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::AiSidebarClick(x_click, y_click) => {
                let mut engine = self.engine.borrow_mut();
                let line_height = self.cached_line_height.max(1.0);
                let row = (y_click / line_height) as usize;
                // Last row = input box
                let msg_count = engine.ai_messages.len();
                let input_row = msg_count + 2; // header + messages
                if row >= input_row {
                    engine.ai_input_active = true;
                }
                engine.ai_has_focus = true;
                let _ = x_click;
                drop(engine);
                if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    /// Effective sidebar visibility — reads directly from
    /// `engine.app_shell` (owned by quadraui per #385). Replaces the
    /// former `App.sidebar_visible` local cache so GTK and engine state
    /// can never drift.
    fn current_sidebar_visible(&self) -> bool {
        self.engine.borrow().app_shell.sidebar_visible()
    }

    /// Effective active panel id, accounting for ext-panel synthetic IDs.
    /// Extension panels bypass AppShell (no dynamic registration on the
    /// quadraui side), so if `engine.ext_panel_active` is set we synthesise
    /// `ext:{name}`; otherwise we read AppShell's active panel.
    fn current_active_panel_id(&self) -> String {
        let engine = self.engine.borrow();
        if let Some(ref name) = engine.ext_panel_active {
            return format!("ext:{name}");
        }
        engine
            .app_shell
            .active_panel_id()
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(|| PANEL_EXPLORER.to_string())
    }

    /// Re-sync GTK widget tree from engine sidebar state. Was previously
    /// `sync_sidebar_from_engine` which copied into local cache fields;
    /// the cache is gone (engine.app_shell is the single source of truth)
    /// so this is now just a redraw trigger.
    fn sync_sidebar_from_engine(&mut self) {
        self.sync_sidebar_widgets();
    }

    /// Update GTK widget visibility (revealer + panel boxes), grab focus on
    /// the active panel DA, and queue an activity bar redraw. Reads
    /// effective state from `engine.app_shell` via the `current_*` helpers.
    fn sync_sidebar_widgets(&mut self) {
        let show = self.current_sidebar_visible();
        let id = self.current_active_panel_id();
        let is_ext = is_ext_panel_id(&id);

        if let Some(ref r) = *self.sidebar_revealer.borrow() {
            r.set_reveal_child(show);
        }
        let panel_boxes: [(&str, &Rc<RefCell<Option<gtk4::Box>>>); 6] = [
            (PANEL_EXPLORER, &self.explorer_panel_box),
            (PANEL_DEBUG, &self.debug_panel_box),
            (PANEL_GIT, &self.git_panel_box),
            (PANEL_EXTENSIONS, &self.ext_panel_box),
            (PANEL_SETTINGS, &self.settings_panel_box),
            (PANEL_AI, &self.ai_panel_box_ref),
        ];
        for (panel_id, box_ref) in &panel_boxes {
            if let Some(ref b) = *box_ref.borrow() {
                b.set_visible(show && !is_ext && id.as_str() == *panel_id);
            }
        }
        if let Some(ref b) = *self.ext_dyn_panel_box.borrow() {
            b.set_visible(show && is_ext);
        }
        if show && self.engine.borrow().sidebar_has_focus() {
            let da_refs: [(&str, &Rc<RefCell<Option<gtk4::DrawingArea>>>); 7] = [
                (PANEL_EXPLORER, &self.explorer_sidebar_da_ref),
                (PANEL_SEARCH, &self.search_sidebar_da_ref),
                (PANEL_DEBUG, &self.debug_sidebar_da_ref),
                (PANEL_GIT, &self.git_sidebar_da_ref),
                (PANEL_EXTENSIONS, &self.ext_sidebar_da_ref),
                (PANEL_SETTINGS, &self.settings_da_ref),
                (PANEL_AI, &self.ai_sidebar_da_ref),
            ];
            let target = if is_ext { "ext:" } else { id.as_str() };
            for (panel_id, da_ref) in &da_refs {
                if *panel_id == target {
                    if let Some(ref da) = *da_ref.borrow() {
                        da.grab_focus();
                    }
                    break;
                }
            }
            if is_ext {
                if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                    da.grab_focus();
                }
            }
        }
        if let Some(ref da) = *self.activity_bar_da_ref.borrow() {
            da.queue_draw();
        }
        self.draw_needed.set(true);
    }

    fn handle_sidebar_panel_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ToggleSidebar => {
                self.engine.borrow_mut().toggle_sidebar();
                self.sync_sidebar_from_engine();
            }
            Msg::SwitchPanel(panel_id) => {
                if let Some(name) = ext_panel_name(&panel_id) {
                    // Extension panels bypass AppShell (no dynamic registration).
                    let mut engine = self.engine.borrow_mut();
                    let same = engine.ext_panel_active.as_deref() == Some(name)
                        && engine.app_shell.sidebar_visible();
                    if same {
                        engine.app_shell.hide_sidebar();
                        engine.ext_panel_has_focus = false;
                        engine.ext_panel_active = None;
                    } else {
                        if !engine.app_shell.sidebar_visible() {
                            engine.app_shell.toggle_sidebar();
                        }
                        let already = engine.ext_panel_active.as_deref() == Some(name);
                        engine.ext_panel_has_focus = true;
                        engine.ext_panel_active = Some(name.to_string());
                        if !already {
                            engine.ext_panel_selected = 0;
                            engine.plugin_event("panel_focus", name);
                        }
                    }
                    engine.session.explorer_visible = engine.app_shell.sidebar_visible();
                    let _ = engine.session.save();
                    drop(engine);
                    let _ = panel_id; // engine.ext_panel_active drives `current_active_panel_id()`
                    self.sync_sidebar_widgets();
                } else {
                    {
                        let mut engine = self.engine.borrow_mut();
                        engine.ext_panel_has_focus = false;
                        engine.ext_panel_active = None;
                        engine.toggle_sidebar_panel(&panel_id);
                    }
                    self.sync_sidebar_from_engine();
                }
            }
            _ => unreachable!(),
        }
    }

    fn handle_explorer_msg(&mut self, msg: Msg) {
        match msg {
            Msg::OpenFileFromSidebar(path) => {
                {
                    let mut engine = self.engine.borrow_mut();
                    // Open in a new tab, or switch to the existing tab that shows this file.
                    engine.open_file_in_tab(&path);
                    engine.explorer_has_focus = false;
                }
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                self.draw_needed.set(true);
            }
            Msg::OpenSide(path) => {
                let mut engine = self.engine.borrow_mut();
                engine.open_editor_group(core::window::SplitDirection::Vertical);
                // Replace the cloned buffer in the new group with the target file.
                engine.execute_command(&format!("e {}", path.display()));
                drop(engine);
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                // tree_has_focus removed (A.2b-2); engine.explorer_has_focus is authoritative
                self.draw_needed.set(true);
            }
            Msg::PreviewFileFromSidebar(path) => {
                let mut engine = self.engine.borrow_mut();
                // Single-click: open as a preview tab (replaceable by next single-click).
                engine.open_file_preview(&path);
                drop(engine);
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                self.draw_needed.set(true);
            }
            Msg::CreateFile(parent_dir, name) => {
                // Validate name
                if let Err(msg) = validate_name(&name) {
                    self.engine.borrow_mut().message = msg;
                    self.draw_needed.set(true);
                    return;
                }

                let file_path = parent_dir.join(&name);

                // Check if already exists
                if file_path.exists() {
                    self.engine.borrow_mut().message = format!("'{}' already exists", name);
                    self.draw_needed.set(true);
                    return;
                }

                // Create file
                match std::fs::File::create(&file_path) {
                    Ok(_) => {
                        self.engine.borrow_mut().message = format!("Created: {}", name);

                        // Trigger tree refresh
                        self.dispatch(Msg::RefreshFileTree);

                        // Open the new file
                        self.dispatch(Msg::OpenFileFromSidebar(file_path));
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message =
                            format!("Error creating '{}': {}", name, e);
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::CreateFolder(parent_dir, name) => {
                // Validate name
                if let Err(msg) = validate_name(&name) {
                    self.engine.borrow_mut().message = msg;
                    self.draw_needed.set(true);
                    return;
                }

                let folder_path = parent_dir.join(&name);

                // Check if already exists
                if folder_path.exists() {
                    self.engine.borrow_mut().message = format!("'{}' already exists", name);
                    self.draw_needed.set(true);
                    return;
                }

                // Create folder
                match std::fs::create_dir(&folder_path) {
                    Ok(_) => {
                        self.engine.borrow_mut().message = format!("Created folder: {}", name);
                        self.dispatch(Msg::RefreshFileTree);
                        self.reveal_path_in_explorer(&folder_path);
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message =
                            format!("Error creating folder '{}': {}", name, e);
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::StartInlineNewFile(_) => {
                self.dispatch(Msg::ExplorerAction("new_file".to_string()));
            }
            Msg::StartInlineNewFolder(_) => {
                self.dispatch(Msg::ExplorerAction("new_folder".to_string()));
            }
            Msg::ExplorerActivateSelected => {
                self.engine.borrow_mut().explorer_activate_selected();
                let still_focused = self.engine.borrow().explorer_has_focus;
                if !still_focused {
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                }
                self.queue_explorer_draw();
                self.draw_needed.set(true);
            }
            Msg::ExplorerAction(action_str) => {
                use crate::core::settings::ExplorerAction;
                let action = match action_str.as_str() {
                    "new_file" => Some(ExplorerAction::NewFile),
                    "new_folder" => Some(ExplorerAction::NewFolder),
                    "rename" => Some(ExplorerAction::Rename),
                    "delete" => Some(ExplorerAction::Delete),
                    "move_file" => Some(ExplorerAction::MoveFile),
                    _ => None,
                };
                if let Some(action) = action {
                    self.engine.borrow_mut().dispatch_explorer_crud(action);
                    self.queue_explorer_draw();
                    self.draw_needed.set(true);
                }
            }
            Msg::ConfirmDeletePath(path) => {
                self.engine.borrow_mut().confirm_delete_file(&path);
                self.draw_needed.set(true);
            }
            Msg::RefreshFileTree => {
                self.refresh_explorer();
                if let Some(path) = self.engine.borrow().file_path().cloned() {
                    self.reveal_path_in_explorer(&path);
                }
                self.draw_needed.set(true);
            }
            Msg::FocusExplorer => {
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.ext_panel_active = None;
                    engine.focus_sidebar_panel(PANEL_EXPLORER);
                }
                if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                    da.grab_focus();
                    // `draw_needed` only queues the editor DA / menu bar.
                    // The explorer DA needs its own `queue_draw` to re-run
                    // the draw callback so the selection highlight
                    // appears now that `explorer_has_focus = true`.
                    da.queue_draw();
                }
                self.sync_sidebar_widgets();
                self.draw_needed.set(true);
            }
            Msg::ToggleFocusExplorer => {
                if self.engine.borrow().explorer_has_focus {
                    self.engine.borrow_mut().explorer_has_focus = false;
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                    if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                } else {
                    {
                        let mut engine = self.engine.borrow_mut();
                        engine.ext_panel_active = None;
                        engine.focus_sidebar_panel(PANEL_EXPLORER);
                    }
                    if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                        da.grab_focus();
                        da.queue_draw();
                    }
                    self.sync_sidebar_widgets();
                }
                self.draw_needed.set(true);
            }
            Msg::ToggleFocusSearch => {
                if self.current_active_panel_id() == PANEL_SEARCH && self.current_sidebar_visible()
                {
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                } else {
                    {
                        let mut engine = self.engine.borrow_mut();
                        engine.ext_panel_active = None;
                        engine.focus_sidebar_panel(PANEL_SEARCH);
                    }
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                    self.sync_sidebar_widgets();
                }
                self.draw_needed.set(true);
            }
            Msg::FocusEditor => {
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.explorer_has_focus = false;
                    engine.dap_sidebar_has_focus = false;
                }

                // Grab focus on drawing area
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                // Redraw explorer so its selection highlight fades.
                if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }

                self.draw_needed.set(true);
            }
            Msg::ExplorerKey {
                key_name,
                unicode,
                ctrl,
            } => {
                self.handle_explorer_da_key(key_name, unicode, ctrl);
                self.queue_explorer_draw();
                self.draw_needed.set(true);
            }
            Msg::ExplorerClick { x, y, n_press } => {
                self.handle_explorer_da_click(x, y, n_press);
            }
            Msg::ExplorerRightClick { x, y } => {
                self.handle_explorer_da_right_click(x, y);
            }
            Msg::ExplorerScroll(dy) => {
                let scaled = dy * 3.0;
                let accum = self.explorer_scroll_accum.get() + scaled;
                let step = accum.trunc() as isize;
                self.explorer_scroll_accum.set(accum - step as f64);
                if step == 0 {
                    return;
                }
                self.engine.borrow_mut().explorer_scroll(step);
                self.queue_explorer_draw();
            }
            Msg::ExplorerCtxMenuClick(x, y) => {
                self.handle_explorer_ctx_menu_overlay_click(x, y);
            }
            Msg::ExplorerCtxMenuMotion(x, y) => {
                self.handle_explorer_ctx_menu_overlay_motion(x, y);
            }
            Msg::ExplorerUiEvent(ev) => {
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
                if dominated {
                    let rect = self.engine.borrow().explorer_tree_rect.get();
                    if rect.width > 0.0 {
                        let theme = {
                            let eng = self.engine.borrow();
                            render::Theme::from_name(&eng.settings.colorscheme)
                        };
                        crate::render::populate_explorer_tree_controller(
                            &self.engine.borrow(),
                            &theme,
                        );
                        let tree_event = {
                            let mut b = self.backend.borrow_mut();
                            // Re-apply the metrics the tree was drawn with so the
                            // hit-test row math matches the rendered rows. (#540)
                            let (lh, cw) = self.cached_explorer_metrics.get();
                            b.set_current_line_height(lh);
                            b.set_current_char_width(cw);
                            self.engine
                                .borrow()
                                .explorer_tree
                                .borrow_mut()
                                .handle(&ev, &mut *b, rect)
                        };
                        let is_scrollbar =
                            matches!(tree_event, quadraui::TreeControllerEvent::ScrollChanged);
                        if matches!(ev, quadraui::UiEvent::DoubleClick { .. }) {
                            self.engine
                                .borrow_mut()
                                .dispatch_explorer_tree_event(tree_event);
                        } else if matches!(ev, quadraui::UiEvent::MouseDown { .. }) {
                            self.engine
                                .borrow_mut()
                                .handle_explorer_mouse_event(tree_event);
                        }
                        // Scrollbar interaction should not steal keyboard
                        // focus from the editor.
                        if is_scrollbar {
                            if let Some(ref da) = *self.drawing_area.borrow() {
                                da.grab_focus();
                            }
                        }
                        self.queue_explorer_draw();
                        self.draw_needed.set(true);
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    /// Forward a pointer event over the sidebar content area to the active panel's
    /// controller. In ShellApp mode the sidebar has no dedicated per-panel
    /// `DrawingArea`, so events the Relm4 build delivered straight to the explorer
    /// DA must be routed here instead. Currently wires the file explorer through
    /// its shared `quadraui::TreeController` (via `Msg::ExplorerUiEvent`); other
    /// panels have their own routing or are keyboard-driven. Returns `true` when
    /// the event was consumed. (#540 ShellApp port)
    fn try_route_sidebar_mouse_event(
        &mut self,
        event: &quadraui::UiEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) -> bool {
        use quadraui::UiEvent;

        let Some(sb) = ctx.layout.sidebar_content_bounds else {
            return false;
        };
        // Intercept only interaction-starting events (press / double-click) and
        // wheel scroll. MouseMoved/MouseUp are deliberately NOT intercepted so an
        // editor text-drag that happens to cross into the sidebar still finalizes
        // through the editor's own mouse-up path.
        let pos = match event {
            UiEvent::MouseDown { position, .. }
            | UiEvent::DoubleClick { position, .. }
            | UiEvent::Scroll { position, .. } => *position,
            _ => return false,
        };
        if pos.x < sb.x || pos.x >= sb.x + sb.width || pos.y < sb.y || pos.y >= sb.y + sb.height {
            return false;
        }

        // Only the file explorer panel is wired through here. When no panel id is
        // set the explorer is the default (mirrors render_content).
        let explorer_active = {
            let engine = self.engine.borrow();
            engine.ext_panel_active.is_none()
                && engine
                    .app_shell
                    .active_panel_id()
                    .map(|id| id.as_str() == PANEL_EXPLORER)
                    .unwrap_or(true)
        };
        if !explorer_active {
            return false;
        }

        self.dispatch(Msg::ExplorerUiEvent(event.clone()));
        self.draw_needed.set(true);
        true
    }

    fn explorer_row_at(&self, y: f64) -> Option<usize> {
        let engine = self.engine.borrow();
        let total = engine.explorer_rows.len();
        let scroll_top = engine.explorer_tree.borrow().scroll_offset();
        drop(engine);
        let item_h = self.explorer_row_height_cell.get().max(1.0);
        let local = (y / item_h).floor().max(0.0) as usize;
        let idx = scroll_top + local;
        if idx < total {
            Some(idx)
        } else {
            None
        }
    }

    fn handle_explorer_da_key(&mut self, key_name: String, unicode: Option<char>, ctrl: bool) {
        // #426: when an explorer ctx menu is open, dispatch j/k/Esc/Enter
        // to the engine ctx menu handler. On Enter, forward the returned
        // action via the shared dispatcher so backend-only flows
        // (new_file, open_terminal, etc.) fire.
        if self.handle_explorer_ctx_menu_key(&key_name) {
            self.queue_explorer_draw();
            self.draw_needed.set(true);
            return;
        }

        // When an engine dialog is active (delete confirmation), route
        // keys to the dialog handler, not the explorer dispatch.
        if self.engine.borrow().dialog.is_some() {
            if !util::is_modifier_only_key(&key_name) {
                let mapped = map_gtk_key_name(&key_name);
                self.engine.borrow_mut().handle_key(mapped, unicode, false);
            }
            self.draw_needed.set(true);
            return;
        }

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
            self.dispatch(Msg::ToggleSidebar);
            return;
        }
        if printable == pk_explorer {
            self.dispatch(Msg::ToggleFocusExplorer);
            return;
        }
        if printable == pk_search {
            self.dispatch(Msg::ToggleFocusSearch);
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
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
            }
            ExplorerKeyResult::FocusToolbar => {
                // engine.activity_bar_focus_in_at(1) was already called inside
                // dispatch_explorer_key. Redraw the activity bar for the
                // selection highlight; key events route through the editor DA
                // whose handle_key_press checks activity_bar_focused and
                // dispatches to handle_activity_bar_key. The activity bar DA
                // has no EventControllerKey, so grab_focus on it drops keys.
                self.engine.borrow_mut().explorer_has_focus = false;
                if let Some(ref da) = *self.activity_bar_da_ref.borrow() {
                    da.queue_draw();
                }
                if let Some(ref da) = *self.drawing_area.borrow() {
                    da.grab_focus();
                }
            }
            _ => {}
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    fn queue_explorer_draw(&self) {
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.queue_draw();
        }
    }

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
            // Activity bar has logical focus — redraw it for the keyboard-
            // selection highlight. Key routing flows through the editor DA.
            if let Some(ref da) = *self.activity_bar_da_ref.borrow() {
                da.queue_draw();
            }
            self.focus_editor_if_needed(false);
        } else {
            self.focus_editor_if_needed(fallback_focused);
        }
    }

    /// Handle a key press while the activity bar has keyboard focus.
    /// j/k move the cursor, l/Enter activates, h/Esc returns to the editor.
    fn handle_activity_bar_key(&mut self, key_name: &str, ctrl: bool) {
        let mapped = map_gtk_key_name(key_name);
        match mapped {
            "j" | "Down" => {
                self.engine.borrow_mut().activity_bar_move_down();
            }
            "k" | "Up" => {
                self.engine.borrow_mut().activity_bar_move_up();
            }
            "l" | "Right" | "Return" if !ctrl => {
                use crate::core::engine::sidebar::ActivityBarActivation;
                let activation = self.engine.borrow_mut().activity_bar_activate();
                match activation {
                    ActivityBarActivation::MenuToggled => {
                        // Re-draw menu bar overlay.
                        if let Some(ref da) = *self.menu_bar_da.borrow() {
                            da.queue_draw();
                        }
                    }
                    ActivityBarActivation::PanelFocused => {
                        self.sync_sidebar_from_engine();
                    }
                    ActivityBarActivation::ExtPanelFocused(_) => {
                        self.sync_sidebar_from_engine();
                        if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                            da.grab_focus();
                        }
                    }
                    ActivityBarActivation::NoOp => {}
                }
            }
            "h" | "Left" | "Escape" if !ctrl => {
                self.engine.borrow_mut().activity_bar_focus_out();
                // Return keyboard focus to the editor drawing area.
                if let Some(ref da) = *self.drawing_area.borrow() {
                    da.grab_focus();
                }
            }
            "q" => {
                let mut engine = self.engine.borrow_mut();
                engine.activity_bar_focus_out();
                engine.app_shell.hide_sidebar();
                engine.clear_sidebar_focus();
                engine.session.explorer_visible = false;
                let _ = engine.session.save();
                drop(engine);
                if let Some(ref da) = *self.drawing_area.borrow() {
                    da.grab_focus();
                }
            }
            _ => {}
        }
        // Always redraw the activity bar to update the selection highlight.
        if let Some(ref da) = *self.activity_bar_da_ref.borrow() {
            da.queue_draw();
        }
        // Suppress the default engine key handler — key is consumed.
    }

    fn handle_explorer_da_click(&mut self, _x: f64, y: f64, n_press: i32) {
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.grab_focus();
        }
        self.engine.borrow_mut().explorer_has_focus = true;

        let Some(idx) = self.explorer_row_at(y) else {
            return;
        };
        let (path, is_dir) = {
            let eng = self.engine.borrow();
            if idx >= eng.explorer_rows.len() {
                return;
            }
            let row = &eng.explorer_rows[idx];
            (row.path.clone(), row.is_dir)
        };
        self.engine
            .borrow()
            .explorer_tree
            .borrow_mut()
            .set_selected_path(Some(vec![idx as u16]));
        self.queue_explorer_draw();
        if is_dir {
            self.engine.borrow_mut().explorer_toggle_dir(idx);
            self.queue_explorer_draw();
        } else if n_press >= 2 {
            self.dispatch(Msg::OpenFileFromSidebar(path));
        } else {
            self.dispatch(Msg::PreviewFileFromSidebar(path));
        }
    }

    /// #426: Intercept j/k/Enter/Esc on the explorer DA when an
    /// engine-drawn explorer ctx menu is open. Returns true if consumed.
    fn handle_explorer_ctx_menu_key(&mut self, key_name: &str) -> bool {
        use core::engine::ContextMenuTarget;
        {
            let eng = self.engine.borrow();
            match eng.context_menu.as_ref().map(|cm| &cm.target) {
                Some(
                    ContextMenuTarget::ExplorerFile { .. } | ContextMenuTarget::ExplorerDir { .. },
                ) => {}
                _ => return false,
            }
        }
        match key_name {
            "Escape" => {
                self.engine.borrow_mut().close_context_menu();
                self.dismiss_ctx_menu_overlay();
                true
            }
            "Return" => {
                let target_path =
                    self.engine
                        .borrow()
                        .context_menu
                        .as_ref()
                        .and_then(|cm| match &cm.target {
                            ContextMenuTarget::ExplorerFile { path }
                            | ContextMenuTarget::ExplorerDir { path } => Some(path.clone()),
                            _ => None,
                        });
                let action = self.engine.borrow_mut().context_menu_confirm();
                if let (Some(action), Some(target)) = (action, target_path) {
                    self.dispatch_explorer_ctx_action(&action, &target);
                }
                self.dismiss_ctx_menu_overlay();
                true
            }
            "j" | "Down" => {
                {
                    let mut eng = self.engine.borrow_mut();
                    if let Some(ref mut cm) = eng.context_menu {
                        let len = cm.items.len();
                        if len > 0 {
                            cm.selected = (cm.selected + 1) % len;
                        }
                    }
                }
                if let Some(ref overlay) = *self.ctx_menu_overlay_da.borrow() {
                    overlay.queue_draw();
                }
                true
            }
            "k" | "Up" => {
                {
                    let mut eng = self.engine.borrow_mut();
                    if let Some(ref mut cm) = eng.context_menu {
                        let len = cm.items.len();
                        if len > 0 {
                            cm.selected = if cm.selected > 0 {
                                cm.selected - 1
                            } else {
                                len - 1
                            };
                        }
                    }
                }
                if let Some(ref overlay) = *self.ctx_menu_overlay_da.borrow() {
                    overlay.queue_draw();
                }
                true
            }
            _ => {
                // Any other key dismisses + falls through to normal explorer
                // handling so the user can keep navigating.
                self.engine.borrow_mut().close_context_menu();
                self.dismiss_ctx_menu_overlay();
                false
            }
        }
    }

    /// #426: Intercept UI events on the explorer DA when an engine-drawn
    /// explorer ctx menu is open. Returns true if consumed.
    /// #426: Mouse-move on the ctx-menu overlay DA — update hover idx
    /// from the cached layout (window coords).
    fn handle_explorer_ctx_menu_overlay_motion(&mut self, x: f64, y: f64) {
        let layout = match self.explorer_ctx_menu_layout.borrow().clone() {
            Some(l) => l,
            None => return,
        };
        let hit = layout.hit_test(x as f32, y as f32);
        if let Some(idx) = core::engine::context_menu_hit_to_idx(&hit) {
            let mut eng = self.engine.borrow_mut();
            if let Some(ref mut cm) = eng.context_menu {
                cm.selected = idx;
            }
        }
        if let Some(ref overlay) = *self.ctx_menu_overlay_da.borrow() {
            overlay.queue_draw();
        }
    }

    /// #426: Click on the ctx-menu overlay DA — hit-test cached layout
    /// and confirm or dismiss. On confirm, dispatch backend-only actions.
    fn handle_explorer_ctx_menu_overlay_click(&mut self, x: f64, y: f64) {
        use core::engine::ContextMenuTarget;
        let layout = match self.explorer_ctx_menu_layout.borrow().clone() {
            Some(l) => l,
            None => {
                self.engine.borrow_mut().close_context_menu();
                self.dismiss_ctx_menu_overlay();
                return;
            }
        };
        let hit = layout.hit_test(x as f32, y as f32);
        let idx = core::engine::context_menu_hit_to_idx(&hit);
        if let Some(idx) = idx {
            let target_path =
                self.engine
                    .borrow()
                    .context_menu
                    .as_ref()
                    .and_then(|cm| match &cm.target {
                        ContextMenuTarget::ExplorerFile { path }
                        | ContextMenuTarget::ExplorerDir { path } => Some(path.clone()),
                        _ => None,
                    });
            let action = {
                let mut eng = self.engine.borrow_mut();
                if let Some(ref mut cm) = eng.context_menu {
                    cm.selected = idx;
                }
                eng.context_menu_confirm()
            };
            if let (Some(action), Some(target)) = (action, target_path) {
                self.dispatch_explorer_ctx_action(&action, &target);
            }
        } else {
            // Click outside any item → dismiss.
            self.engine.borrow_mut().close_context_menu();
        }
        self.dismiss_ctx_menu_overlay();
    }

    /// #426: Stop intercepting events on the ctx-menu overlay DA and
    /// queue a redraw so the menu paint clears.
    fn dismiss_ctx_menu_overlay(&self) {
        if let Some(ref overlay) = *self.ctx_menu_overlay_da.borrow() {
            overlay.set_can_target(false);
            overlay.queue_draw();
        }
        *self.explorer_ctx_menu_layout.borrow_mut() = None;
    }

    /// #426: Map the action string returned by `context_menu_confirm` for
    /// an explorer ctx menu to the appropriate backend Msg. Engine-side
    /// actions (copy_path, reveal, select_for_diff, etc.) were already
    /// handled inside `context_menu_confirm`; this only covers actions
    /// that require GTK plumbing.
    fn dispatch_explorer_ctx_action(&mut self, action: &str, target: &std::path::Path) {
        match action {
            "new_file" | "new_folder" | "rename" | "delete" | "move_file" => {
                self.dispatch(Msg::ExplorerAction(action.to_string()));
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
                self.dispatch(Msg::OpenTerminalAt(dir));
            }
            "find_in_folder" => {
                self.dispatch(Msg::ToggleFocusSearch);
            }
            _ => {} // engine-handled actions (copy_path, reveal, etc.)
        }
    }

    fn handle_explorer_da_right_click(&mut self, x: f64, y: f64) {
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.grab_focus();
        }
        self.engine.borrow_mut().explorer_has_focus = true;
        let (target, is_dir) = if let Some(idx) = self.explorer_row_at(y) {
            let eng = self.engine.borrow();
            if idx < eng.explorer_rows.len() {
                eng.explorer_tree
                    .borrow_mut()
                    .set_selected_path(Some(vec![idx as u16]));
                let row = &eng.explorer_rows[idx];
                (row.path.clone(), row.is_dir)
            } else {
                (self.engine.borrow().cwd.clone(), true)
            }
        } else {
            (self.engine.borrow().cwd.clone(), true)
        };
        // #426: engine-driven ctx menu. The menu renders on a window-
        // level overlay DA so it can extend past the narrow explorer DA
        // into the editor area. Translate explorer-DA-local (x, y) to
        // window coords via `compute_point`, then divide by UI-font
        // metrics for the engine cell storage.
        let (win_x, win_y) = if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            if let Some(ref overlay) = *self.ctx_menu_overlay_da.borrow() {
                da.compute_point(overlay, &gtk4::graphene::Point::new(x as f32, y as f32))
                    .map(|p| (p.x() as f64, p.y() as f64))
                    .unwrap_or((x, y))
            } else {
                (x, y)
            }
        } else {
            (x, y)
        };
        let cw = self.explorer_char_width_cell.get().max(1.0);
        let lh = self.explorer_line_height_cell.get().max(1.0);
        let cx = (win_x / cw) as u16;
        let cy = (win_y / lh) as u16;
        self.engine
            .borrow_mut()
            .open_explorer_context_menu(target, is_dir, cx, cy);
        self.queue_explorer_draw();
        if let Some(ref overlay) = *self.ctx_menu_overlay_da.borrow() {
            overlay.set_can_target(true);
            overlay.queue_draw();
        }
    }

    fn handle_find_replace_msg(&mut self, msg: Msg) {
        match msg {
            Msg::WindowResized { width, height } => {
                // Update session state with new window geometry (debounced save)
                let mut engine = self.engine.borrow_mut();
                engine.session.window.width = width;
                engine.session.window.height = height;
                // Note: We don't save on every resize event (too frequent)
                // Window geometry is saved on close instead
            }
            Msg::SidebarResized => {
                if let Some(ref sb) = *self.sidebar_inner_sw.borrow() {
                    let w = sb.width_request();
                    self.engine.borrow_mut().session.sidebar_width = w;
                    let _ = self.engine.borrow().session.save();
                }
            }
            _ => unreachable!(),
        }
    }

    fn handle_file_ops_msg(&mut self, msg: Msg) {
        match msg {
            Msg::RenameFile(old_path, new_name) => {
                let result = self.engine.borrow_mut().rename_file(&old_path, &new_name);
                match result {
                    Ok(()) => {
                        self.engine.borrow_mut().message = format!("Renamed to '{}'", new_name);
                        self.dispatch(Msg::RefreshFileTree);
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message = e;
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::MoveFile(src, dest_dir) => {
                self.engine.borrow_mut().confirm_move_file(&src, &dest_dir);
                self.draw_needed.set(true);
            }
            Msg::CopyPath(path) => {
                let path_str = path.to_string_lossy().to_string();
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&path_str);
                    self.engine.borrow_mut().message = format!("Copied: {}", path_str);
                }
                self.draw_needed.set(true);
            }
            Msg::CopyRelativePath(path) => {
                let rel = self.engine.borrow().copy_relative_path(&path);
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&rel);
                    self.engine.borrow_mut().message = format!("Copied: {}", rel);
                }
                self.draw_needed.set(true);
            }
            Msg::SelectForDiff(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                self.engine.borrow_mut().diff_selected_file = Some(path);
                self.engine.borrow_mut().message =
                    format!("Selected '{name}' for compare. Right-click another file to compare.");
                self.draw_needed.set(true);
            }
            Msg::DiffWithSelected(right_path) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(left_path) = engine.diff_selected_file.take() {
                    engine.open_file_in_tab(&left_path);
                    engine.cmd_diffthis();
                    engine.cmd_diffsplit(&right_path);
                } else {
                    engine.message =
                        "No file selected for compare. Use 'Select for Compare' first.".to_string();
                }
                drop(engine);
                self.draw_needed.set(true);
            }
            Msg::ClipboardPasteToInput { text } => {
                self.engine.borrow_mut().route_paste(&text);
                self.draw_needed.set(true);
            }
            Msg::WindowClosing { width, height } => {
                let mut engine = self.engine.borrow_mut();
                engine.session.window.width = width;
                engine.session.window.height = height;
                engine.session.explorer_visible = engine.app_shell.sidebar_visible();
                // Save sidebar width on close too
                if let Some(ref sb) = *self.sidebar_inner_sw.borrow() {
                    engine.session.sidebar_width = sb.width_request();
                }

                // Save cursor/scroll position for the active file
                let buffer_id = engine.active_buffer_id();
                if let Some(path) = engine
                    .buffer_manager
                    .get(buffer_id)
                    .and_then(|s| s.file_path.as_deref())
                    .map(|p| p.to_path_buf())
                {
                    let view = engine.active_window().view.clone();
                    engine.session.save_file_position(
                        &path,
                        view.cursor.line,
                        view.cursor.col,
                        view.scroll_top,
                    );
                }

                engine.collect_session_open_files();
                let _ = engine.session.save();
            }
            _ => unreachable!(),
        }
    }

    fn handle_dialog_msg(&mut self, msg: Msg) {
        match msg {
            Msg::WindowMinimize => {
                if let Some(ref w) = self.window {
                    w.minimize();
                }
            }
            Msg::WindowMaximize => {
                if self.window.as_ref().is_some_and(|w| w.is_maximized()) {
                    if let Some(ref w) = self.window {
                        w.unmaximize();
                    }
                } else {
                    if let Some(ref w) = self.window {
                        w.maximize();
                    }
                }
            }
            Msg::WindowClose => {
                if let Some(ref w) = self.window {
                    w.close();
                }
            }
            Msg::OpenFileDialog => {
                let engine = self.engine.clone();
                let sender2 = self.sender.clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Open File");
                let win = self.window.clone();
                dialog.open(win.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = gtk4::prelude::FileExt::path(&file) {
                            let _ = engine.borrow_mut().open_file_with_mode(
                                &path,
                                crate::core::engine::OpenMode::Permanent,
                            );
                            sender2.send(Msg::RefreshFileTree).ok();
                        }
                    }
                });
                self.draw_needed.set(true);
            }
            Msg::OpenFolderDialog => {
                let engine = self.engine.clone();
                let sender2 = self.sender.clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Open Folder");
                dialog.set_accept_label(Some("Open Folder"));
                let win = self.window.clone();
                dialog.select_folder(win.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        // Use UFCS to call gtk4's FileExt::path (avoids gio version conflict)
                        if let Some(path) = gtk4::prelude::FileExt::path(&file) {
                            engine.borrow_mut().open_folder(&path);
                            sender2.send(Msg::RefreshFileTree).ok();
                        }
                    }
                });
                self.draw_needed.set(true);
            }
            Msg::OpenWorkspaceDialog => {
                // open_workspace_from_file() already ran in the engine;
                // just refresh the file tree.
                self.dispatch(Msg::RefreshFileTree);
                self.draw_needed.set(true);
            }
            Msg::SaveWorkspaceAsDialog => {
                let engine = self.engine.clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Save Workspace As");
                dialog.set_initial_name(Some(".vimcode-workspace"));
                let win = self.window.clone();
                dialog.save(win.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = gtk4::prelude::FileExt::path(&file) {
                            engine.borrow_mut().save_workspace_as(&path);
                        }
                    }
                });
                self.draw_needed.set(true);
            }
            Msg::OpenRecentDialog => {
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

            Msg::ShowQuitConfirm => {
                if !self.engine.borrow().has_any_unsaved() {
                    self.save_session_and_exit();
                    return;
                }
                use crate::core::engine::DialogButton;
                self.engine.borrow_mut().show_dialog(
                    "quit_unsaved",
                    "Unsaved Changes",
                    vec![
                        "You have unsaved changes.".to_string(),
                        "Do you want to save before quitting?".to_string(),
                    ],
                    vec![
                        DialogButton {
                            label: "Save All & Quit".into(),
                            hotkey: 's',
                            action: "save_quit".into(),
                        },
                        DialogButton {
                            label: "Quit Without Saving".into(),
                            hotkey: 'q',
                            action: "discard_quit".into(),
                        },
                        DialogButton {
                            label: "Cancel".into(),
                            hotkey: '\0',
                            action: "cancel".into(),
                        },
                    ],
                );
                self.draw_needed.set(true);
            }

            Msg::QuitConfirmed => {
                // Save session state then exit the process.
                self.save_session_and_exit();
            }

            Msg::ShowCloseTabConfirm => {
                use crate::core::engine::DialogButton;
                self.engine.borrow_mut().show_dialog(
                    "close_tab_confirm",
                    "Unsaved Changes",
                    vec!["This file has unsaved changes.".to_string()],
                    vec![
                        DialogButton {
                            label: "Save & Close".into(),
                            hotkey: 's',
                            action: "save_close".into(),
                        },
                        DialogButton {
                            label: "Discard & Close".into(),
                            hotkey: 'd',
                            action: "discard".into(),
                        },
                        DialogButton {
                            label: "Cancel".into(),
                            hotkey: '\0',
                            action: "cancel".into(),
                        },
                    ],
                );
                self.draw_needed.set(true);
            }

            Msg::CloseTabConfirmed { save } => {
                let mut engine = self.engine.borrow_mut();
                engine.escape_to_normal();
                if save {
                    let _ = engine.save();
                }
                engine.close_tab();
                drop(engine);
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    #[allow(dead_code)]
    fn terminal_cols(&self) -> u16 {
        if let Some(da) = self.drawing_area.borrow().as_ref() {
            if self.cached_char_width > 0.0 {
                (da.width() as f64 / self.cached_char_width) as u16
            } else {
                80
            }
        } else {
            80
        }
        .max(40)
    }

    fn terminal_target_maximize_rows(&self) -> u16 {
        let lh = self.cached_line_height.max(1.0);
        if let Some(da) = self.drawing_area.borrow().as_ref() {
            render::compute_editor_layout(&self.engine.borrow(), da.height() as f64, lh, false)
                .terminal_max_target_rows
        } else {
            10
        }
    }
}

// ── Dormant ShellApp impl (#448-B) ──────────────────────────────────────────
// This impl compiles alongside the Relm4 path but is NOT wired up.
impl quadraui::ShellApp for App {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // Seed cached metrics from runner defaults.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.cached_ui_line_height = self.cached_line_height;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);

        // Grab the runner-created GTK window so minimize/maximize/close work.
        let window = gtk4::Window::list_toplevels()
            .into_iter()
            .filter_map(|obj| obj.downcast::<gtk4::Window>().ok())
            .find(|w| w.is_visible());
        self.window = window;

        // Apply initial CSS.
        let theme = Theme::from_name(&self.engine.borrow().settings.colorscheme);
        let combined = format!("{STATIC_CSS}\n{}", make_theme_css(&theme));
        self.css_provider.load_from_data(&combined);
    }

    fn render_content(
        &self,
        backend: &mut dyn quadraui::Backend,
        layout: &quadraui::AppShellLayout,
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};

        let engine = self.engine.borrow();
        let theme = Theme::from_name(&engine.settings.colorscheme);
        backend.set_theme(render::to_quadraui_theme(&theme));

        let lh = self.cached_line_height.max(backend.line_height() as f64);
        let cw = self.cached_char_width.max(backend.char_width() as f64);

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
        let tab_row_h = (lh * 1.6).ceil();
        let tab_bar_h = render::tab_bar_height_px(lh, engine.settings.breadcrumbs);
        let per_window_status = engine.settings.window_status_line;
        let wildmenu_px = if engine.wildmenu_items.is_empty() {
            0.0
        } else {
            lh
        };
        let status_rows = if per_window_status { 1.0 } else { 2.0 };
        let status_bar_h = lh * status_rows + wildmenu_px;
        let el = render::compute_editor_layout(&engine, h, lh, false);
        let editor_area_h =
            (h - el.terminal_h - el.debug_toolbar_h - el.separated_status_h - status_bar_h)
                .max(0.0);

        let editor_bounds = WindowRect::new(x, y, w, editor_area_h);
        let (window_rects, _dividers) =
            engine.calculate_group_window_rects(editor_bounds, tab_bar_h);

        let screen = build_screen_layout(&engine, &theme, &window_rects, lh, cw, false);

        // Cache for click handlers (move into RefCell, then borrow back for drawing).
        *self.cached_screen_layout.borrow_mut() = Some(screen);
        let screen_ref = self.cached_screen_layout.borrow();
        let screen = screen_ref.as_ref().unwrap();

        // ── Draw editor windows ───────────────────────────────────────────────
        for rw in &screen.windows {
            let editor = render::to_q_editor(rw);
            let rect = editor.rect;
            let mut frame = QSL::new();
            frame.push(Surface::Editor {
                rect,
                editor: &editor,
            });
            frame.draw(backend);

            // Per-window status bar (when window_status_line=true, which is
            // the default; global_status_bar is None in that mode).
            if let Some(ref status) = rw.status_line {
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
                let mut frame = QSL::new();
                frame.push(Surface::StatusBar {
                    rect: sb_rect,
                    bar: &win_bar,
                    hovered: None,
                    pressed: None,
                });
                frame.draw(backend);
            }
        }

        // ── Draw tab bar(s) — one per editor group ────────────────────────────
        // Multi-group (post-split) layouts have a tab bar per group, each drawn
        // at the top edge of its own bounds. Single-group draws one full-width
        // bar at the editor top. Previously only the single-group primitive was
        // drawn, so split groups rendered with no tab bar at all. (#515)
        // Reset the pixel-accurate hit caches; repopulated per tab bar below so
        // the click / hover hit-tests use the exact drawn geometry (#515).
        let mut pixel_hits = self.cached_tab_pixel_hits.borrow_mut();
        let mut close_bounds = self.tab_close_bounds.borrow_mut();
        pixel_hits.clear();
        close_bounds.clear();
        if let Some(ref split) = screen.editor_group_split {
            for gtb in &split.group_tab_bars {
                if engine.is_tab_bar_hidden(gtb.group_id) {
                    continue;
                }
                let tb_rect = quadraui::Rect::new(
                    gtb.bounds.x as f32,
                    (gtb.bounds.y - tab_bar_h) as f32,
                    gtb.bounds.width as f32,
                    tab_row_h as f32,
                );
                let hover = self
                    .tab_close_hover
                    .and_then(|(gid, i)| (gid == gtb.group_id.0).then_some(i));
                let mut frame = QSL::new();
                frame.push(Surface::TabBar {
                    rect: tb_rect,
                    bar: &gtb.bar,
                    hovered_close: hover,
                });
                frame.draw(backend);
                // Recover the exact pixel geometry the rasteriser just drew and
                // cache it (relative to the bar's left edge) for hit-testing.
                let hits = backend.tab_bar_layout(tb_rect, &gtb.bar);
                let ph = tab_hits_to_pixel_hits(&hits, &gtb.bar, tb_rect.x as f64);
                close_bounds.insert(gtb.group_id.0, ph.close.clone());
                pixel_hits.insert(gtb.group_id.0, ph);
            }
        } else if !engine.is_tab_bar_hidden(engine.active_group) {
            let tb_rect = quadraui::Rect::new(x as f32, y as f32, w as f32, tab_row_h as f32);
            let hover = self.tab_close_hover.map(|(_, i)| i);
            let mut frame = QSL::new();
            frame.push(Surface::TabBar {
                rect: tb_rect,
                bar: &screen.tab_bar_primitive,
                hovered_close: hover,
            });
            frame.draw(backend);
            let hits = backend.tab_bar_layout(tb_rect, &screen.tab_bar_primitive);
            let ph = tab_hits_to_pixel_hits(&hits, &screen.tab_bar_primitive, tb_rect.x as f64);
            close_bounds.insert(engine.active_group.0, ph.close.clone());
            pixel_hits.insert(engine.active_group.0, ph);
        }
        drop(close_bounds);
        drop(pixel_hits);

        // ── Draw global status bar / wildmenu ─────────────────────────────────
        let status_y =
            y + h - el.terminal_h - el.debug_toolbar_h - el.separated_status_h - status_bar_h;
        if let Some(ref bar) = screen.global_status_bar {
            let sb_rect = quadraui::Rect::new(x as f32, status_y as f32, w as f32, lh as f32);
            let mut frame = QSL::new();
            frame.push(Surface::StatusBar {
                rect: sb_rect,
                bar,
                hovered: None,
                pressed: None,
            });
            frame.draw(backend);
        }
        if let Some(ref wm) = screen.wildmenu {
            let wm_bar = render::wildmenu_to_status_bar(wm, &theme);
            let wm_y = if per_window_status {
                status_y
            } else {
                status_y + lh
            };
            let wm_rect = quadraui::Rect::new(x as f32, wm_y as f32, w as f32, lh as f32);
            let mut frame = QSL::new();
            frame.push(Surface::StatusBar {
                rect: wm_rect,
                bar: &wm_bar,
                hovered: None,
                pressed: None,
            });
            frame.draw(backend);
        }

        // ── Draw command line ─────────────────────────────────────────────────
        let cmd_y = status_y + (status_bar_h - lh);
        let cmd = quadraui::CommandLine {
            id: "cmd".into(),
            text: screen.command.text.clone(),
            cursor_offset: if screen.command.show_cursor {
                Some(screen.command.cursor_anchor_text.len())
            } else {
                None
            },
            right_align: screen.command.right_align,
        };
        let cmd_rect = quadraui::Rect::new(x as f32, cmd_y as f32, w as f32, lh as f32);
        let mut frame = QSL::new();
        frame.push(Surface::CommandLine {
            rect: cmd_rect,
            cmd: &cmd,
        });
        frame.draw(backend);

        // ── Draw sidebar panel content ─────────────────────────────────────────
        // The quadraui AppShell chrome (activity bar + sidebar header) is rendered
        // by the runner; we fill only the content area it exposes.
        if let Some(q_sb) = layout.sidebar_content_bounds {
            // Which panel is active?  Extension panels bypass AppShell.
            let active_id: String = if let Some(ref name) = engine.ext_panel_active {
                format!("ext:{name}")
            } else {
                engine
                    .app_shell
                    .active_panel_id()
                    .map(|id| id.as_str().to_string())
                    .unwrap_or_else(|| PANEL_EXPLORER.to_string())
            };

            match active_id.as_str() {
                PANEL_EXPLORER => {
                    render::populate_explorer_tree_controller(&engine, &theme);
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
                    render::populate_search_sidebar_system(&engine, &engine.cwd);
                    engine.search_sidebar_body_rect.set(q_sb);
                    engine.search_sidebar_system.borrow().render(backend, q_sb);
                }
                PANEL_DEBUG => {
                    let (title_bar, action_bar) =
                        render::debug_sidebar_chrome_to_status_bars(&screen.debug_sidebar, &theme);
                    let title_rect = quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, lh as f32);
                    let action_rect =
                        quadraui::Rect::new(q_sb.x, q_sb.y + lh as f32, q_sb.width, lh as f32);
                    let body_y = q_sb.y + 2.0 * lh as f32;
                    let body_h = (q_sb.height - 2.0 * lh as f32).max(0.0);
                    let body_rect = quadraui::Rect::new(q_sb.x, body_y, q_sb.width, body_h);
                    let _ = backend.draw_status_bar(title_rect, &title_bar, None, None);
                    let hits = backend.draw_status_bar(action_rect, &action_bar, None, None);
                    engine.dap_sidebar_action_hits.replace(Some(hits));
                    engine.dap_sidebar_body_rect.set(body_rect);
                    render::populate_dap_sidebar_system(&engine);
                    engine
                        .dap_sidebar_system
                        .borrow()
                        .render(backend, body_rect);
                }
                PANEL_GIT => {
                    if let Some(ref sc) = screen.source_control {
                        // Render the toolbar-slab + section list; the header row
                        // (branch name) and commit-input chrome are deferred to a
                        // follow-up migration once a Backend primitive for them lands.
                        render::draw_sc_sidebar_panel(backend, &engine, sc, q_sb);
                        let body_rect = engine
                            .sc_panel_layout
                            .borrow()
                            .as_ref()
                            .map(|l| l.content_bounds)
                            .unwrap_or(q_sb);
                        engine.sc_sidebar_body_rect.set(body_rect);
                        render::populate_sc_sidebar_system(&engine, &theme);
                        engine.sc_sidebar_system.borrow().render(backend, body_rect);
                    }
                }
                PANEL_EXTENSIONS => {
                    render::populate_ext_sidebar_system(&engine);
                    engine.ext_sidebar_body_rect.set(q_sb);
                    engine.ext_sidebar_system.borrow().render(backend, q_sb);
                }
                PANEL_SETTINGS => {
                    render::populate_settings_form_controller(&engine);
                    engine
                        .settings_form_controller
                        .borrow_mut()
                        .render_and_cache(backend, q_sb);
                }
                id if id.starts_with("ext:") => {
                    // Extension panel — render via ext_sidebar_system.
                    render::populate_ext_sidebar_system(&engine);
                    engine.ext_sidebar_body_rect.set(q_sb);
                    engine.ext_sidebar_system.borrow().render(backend, q_sb);
                }
                _ => {
                    // PANEL_AI and unknowns: not yet migrated to Backend primitives.
                }
            }
        }

        // ── Cache per-group tab-drop geometry ─────────────────────────────────
        // Compute the absolute drop-group bounds from the shared screen layout and
        // stash them so the drag hit-test (handle_mouse_drag_msg) and the overlay
        // below use one identical source.
        //
        // Origin convention: in multi-group mode `gtb.bounds` are already absolute
        // (built from absolute window rects), so the origin offset must be (0,0) —
        // adding (x,y) again would double-count it and shift the highlight off the
        // group (the prior "covers half the group" bug). Single-group mode returns
        // (origin, size) directly, so it needs the real editor origin (x,y). (#515)
        {
            let drop_origin = if screen.editor_group_split.is_some() {
                (0.0, 0.0)
            } else {
                (x as f32, y as f32)
            };
            let bounds = render::screen_to_drop_group_bounds(
                screen,
                &engine,
                drop_origin,
                (w as f32, editor_area_h as f32),
            );
            // Tab-reorder insertion bars need per-tab slot x-positions; those are a
            // follow-up (empty map ⇒ center/split drop zones work, reorder bar
            // falls back). Center/split highlighting — the reported regression — is
            // bounds-only and works without slots.
            let empty_slots = std::collections::HashMap::<usize, Vec<(f32, f32)>>::new();
            let (groups, eff_tbh) =
                render::build_tab_drop_groups(&bounds, &engine, tab_bar_h as f32, &empty_slots);
            *self.cached_drop_groups.borrow_mut() = groups;
            self.cached_drop_tbh.set(eff_tbh);
        }

        // ── Draw tab drag overlay ─────────────────────────────────────────────
        // When a tab drag is in progress, paint the drop-zone highlight + insertion
        // bar on top of all other content, using the geometry cached just above.
        if self.tab_dragging {
            let groups = self.cached_drop_groups.borrow();
            let eff_tbh = self.cached_drop_tbh.get();
            let (mx, my) = self.mouse_pos_cell.get();
            if let Some(ov) = render::compute_tab_drop_overlay(
                &self.tab_drag_drop_zone,
                &groups,
                (mx as f32, my as f32),
                eff_tbh,
                2.0,
                lh as f32,
            ) {
                backend.draw_drop_overlay(&quadraui::DropOverlay {
                    highlight: ov.highlight,
                    insertion_bar: ov.insertion_bar,
                    ghost_position: Some(ov.ghost_position),
                });
            }
        }
    }

    fn handle(
        &mut self,
        event: quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) -> quadraui::Reaction {
        use quadraui::{Key, MouseButton, NamedKey, UiEvent};

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
            UiEvent::KeyPressed { key, modifiers, .. } => {
                let (key_name, unicode) = match key {
                    Key::Char(c) => (c.to_string(), Some(c)),
                    Key::Named(ref named) => {
                        let n: &str = match named {
                            NamedKey::Escape => "Escape",
                            NamedKey::Tab => "Tab",
                            NamedKey::BackTab => "BackTab",
                            NamedKey::Enter => "Return",
                            NamedKey::Backspace => "BackSpace",
                            NamedKey::Delete => "Delete",
                            NamedKey::Insert => "Insert",
                            NamedKey::Home => "Home",
                            NamedKey::End => "End",
                            NamedKey::PageUp => "PageUp",
                            NamedKey::PageDown => "PageDown",
                            NamedKey::Up => "Up",
                            NamedKey::Down => "Down",
                            NamedKey::Left => "Left",
                            NamedKey::Right => "Right",
                            NamedKey::F(1) => "F1",
                            NamedKey::F(2) => "F2",
                            NamedKey::F(3) => "F3",
                            NamedKey::F(4) => "F4",
                            NamedKey::F(5) => "F5",
                            NamedKey::F(6) => "F6",
                            NamedKey::F(7) => "F7",
                            NamedKey::F(8) => "F8",
                            NamedKey::F(9) => "F9",
                            NamedKey::F(10) => "F10",
                            NamedKey::F(11) => "F11",
                            NamedKey::F(12) => "F12",
                            _ => "",
                        };
                        (n.to_string(), None)
                    }
                };
                if !key_name.is_empty() || unicode.is_some() {
                    self.handle_key_press(key_name, unicode, modifiers.ctrl, modifiers.alt);
                }
            }
            UiEvent::CharTyped(c) => {
                // Ctrl-modified characters arrive via KeyPressed; CharTyped is
                // for IME-composed printable characters only.
                self.handle_key_press(c.to_string(), Some(c), false, false);
            }
            UiEvent::Accelerator(id, _mods) => {
                let id_str = id.as_str().to_string();
                dispatch_gtk_panel_accelerator(&id_str, &self.sender, &self.engine);
                self.draw_needed.set(true);
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
                        self.dispatch(Msg::CtrlMouseClick {
                            x: position.x as f64,
                            y: position.y as f64,
                            width: w,
                            height: h,
                        });
                    }
                    MouseButton::Left => {
                        self.dispatch(Msg::MouseClick {
                            x: position.x as f64,
                            y: position.y as f64,
                            width: w,
                            height: h,
                            alt: modifiers.alt,
                        });
                    }
                    MouseButton::Right => {
                        self.dispatch(Msg::EditorRightClick {
                            x: position.x as f64,
                            y: position.y as f64,
                        });
                    }
                    _ => {}
                }
                // Mouse clicks always require a redraw (cursor movement, selection,
                // focus change). draw_needed may already be set by dispatch(), but
                // set it unconditionally so handle() returns Reaction::Redraw even
                // when dispatch() takes an early-return path in ShellApp mode.
                self.draw_needed.set(true);
            }
            UiEvent::DoubleClick { position, .. } => {
                let main = ctx.layout.main_content_bounds;
                self.dispatch(Msg::MouseDoubleClick {
                    x: position.x as f64,
                    y: position.y as f64,
                    width: main.width as f64,
                    height: main.height as f64,
                });
                self.draw_needed.set(true);
            }
            UiEvent::MouseMoved { position, buttons } => {
                self.mouse_pos_cell
                    .set((position.x as f64, position.y as f64));
                if buttons.left {
                    let main = ctx.layout.main_content_bounds;
                    self.dispatch(Msg::MouseDrag {
                        x: position.x as f64,
                        y: position.y as f64,
                        width: main.width as f64,
                        height: main.height as f64,
                    });
                }
            }
            UiEvent::MouseUp { .. } => {
                self.dispatch(Msg::MouseUp);
            }
            UiEvent::Scroll { delta, .. } => {
                self.dispatch(Msg::MouseScroll {
                    delta_x: delta.x as f64,
                    delta_y: delta.y as f64,
                });
            }
            UiEvent::WindowResized { .. } => {
                // Runner sets new line_height/char_width after resize.
                self.cached_line_height = backend.line_height() as f64;
                self.cached_char_width = backend.char_width() as f64;
                self.line_height_cell.set(self.cached_line_height);
                self.char_width_cell.set(self.cached_char_width);
                self.dispatch(Msg::Resize);
            }
            UiEvent::WindowClose => {
                self.dispatch(Msg::ShowQuitConfirm);
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

    fn tick(&mut self, backend: &mut dyn quadraui::Backend) -> quadraui::Reaction {
        // Keep cached metrics up to date.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);

        // Drain messages queued by async GTK callbacks.
        let msgs = self.sender.drain();
        for msg in msgs {
            self.dispatch(msg);
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

    fn on_shell_event(&mut self, event: &quadraui::AppShellEvent) {
        use quadraui::AppShellEvent;
        match event {
            AppShellEvent::PanelChanged { panel_id } => {
                // Sync the runner's active panel into the engine's AppShell so
                // render_content() draws the correct sidebar panel content.
                self.engine.borrow_mut().app_shell.show_panel(panel_id);
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarHidden => {
                self.engine.borrow_mut().app_shell.hide_sidebar();
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarResized { new_width } => {
                self.engine
                    .borrow_mut()
                    .app_shell
                    .set_sidebar_width(*new_width);
            }
            AppShellEvent::BottomItemClicked { id } => {
                // The runner treats bottom activity-bar items as action buttons
                // (not sidebar panels), so it does not change its own sidebar
                // visibility.  For vimcode, bottom items like "bottom:settings"
                // represent sidebar panels stored in the engine's AppShell.
                // Sync the active panel so render_content() draws the correct
                // content the next time the sidebar is visible.
                self.engine.borrow_mut().app_shell.show_panel(id);
                self.draw_needed.set(true);
            }
            _ => {}
        }
    }
}

// view_row_to_buf_line and view_row_to_buf_pos_wrap are now shared functions
// in render.rs — use render::view_row_to_buf_line / render::view_row_to_buf_pos_wrap.

/// Calculate gutter width in pixels based on line number mode and buffer size
#[allow(dead_code)]
fn calculate_gutter_width(mode: LineNumberMode, total_lines: usize, char_width: f64) -> f64 {
    match mode {
        LineNumberMode::None => 0.0,
        LineNumberMode::Absolute => {
            // Width = number of digits + 2 chars padding (1 on each side)
            let digits = total_lines.to_string().len().max(1);
            (digits + 2) as f64 * char_width
        }
        LineNumberMode::Relative | LineNumberMode::Hybrid => {
            // Relative numbers can be large for long files, use at least 3 digits + 2 padding
            let max_relative = total_lines.saturating_sub(1);
            let digits = max_relative.to_string().len().max(3);
            (digits + 2) as f64 * char_width
        }
    }
}

/// Compute the editor area bottom Y coordinate.  Must match draw_editor (draw.rs)
/// so that group rects and divider positions are consistent across draw and click.
/// Compute the target `terminal_panel_rows` when maximizing the GTK panel.
///
/// The rendered terminal panel takes `(terminal_panel_rows + 2) * lh` pixels
/// (2 chrome rows = bottom-panel tab bar + terminal toolbar). Editor tab bar
/// stays visible (1 row reserved); breadcrumbs are suppressed elsewhere so
/// we don't reserve a row for them here. Called every frame from `draw_frame`
fn gtk_editor_bottom(engine: &Engine, _da_width: f64, da_height: f64, line_height: f64) -> f64 {
    render::compute_editor_layout(engine, da_height, line_height, false).editor_bottom
}

/// Compute editor window rects with the same formula used by draw_editor and
/// sync_scrollbar, so event handlers can do hit-testing without duplicating the
/// layout logic.
fn compute_editor_window_rects(
    engine: &Engine,
    da_width: f64,
    da_height: f64,
    line_height: f64,
) -> Vec<(core::WindowId, core::WindowRect)> {
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };
    let editor_bounds = core::WindowRect::new(
        0.0,
        0.0,
        da_width,
        gtk_editor_bottom(engine, da_width, da_height, line_height),
    );
    let (rects, _dividers) = engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
    rects
}

/// Compute the thumb geometry for one window's h scrollbar.
/// Returns `(track_x, track_y, track_w, sb_height, thumb_x, thumb_w, scroll_range, px_per_col)`.
/// Returns `None` when no scrollbar is needed (content fits).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn h_scrollbar_geometry(
    engine: &Engine,
    window_id: core::WindowId,
    rect: &core::WindowRect,
    char_width: f64,
    line_height: f64,
) -> Option<(f64, f64, f64, f64, f64, f64, f64, f64)> {
    let window = engine.windows.get(&window_id)?;
    let buffer_state = engine.buffer_manager.get(window.buffer_id)?;

    // max_col is pre-computed and cached in BufferState on every edit — O(1) vs O(N_lines).
    let max_line_length = buffer_state.max_col as f64;

    let v_scrollbar_px = 8.0_f64;
    let track_w = (rect.width - v_scrollbar_px).max(1.0);
    let visible_cols = (track_w / char_width).floor().max(1.0);

    if max_line_length <= visible_cols {
        return None;
    }

    let sb_height = (line_height * 0.35).round().max(4.0);
    let track_x = rect.x;
    // Per-window status line lives at `rect.y + rect.height -
    // line_height` and paints after the scrollbars, so anchor the
    // h-scrollbar above it when the status line is on. Otherwise the
    // status bar overdraws the entire scrollbar (it's `line_height`
    // tall vs the scrollbar's ~5px).
    let status_offset = if engine.settings.window_status_line && !engine.terminal_maximized {
        line_height
    } else {
        0.0
    };
    let track_y = rect.y + rect.height - sb_height - status_offset;
    let scroll_range = (max_line_length - visible_cols).max(1.0);
    let thumb_frac = visible_cols / max_line_length;
    let thumb_w = (thumb_frac * track_w).max(20.0).min(track_w);
    let px_per_col = (track_w - thumb_w) / scroll_range;
    let scroll_left = window.view.scroll_left as f64;
    let thumb_x = track_x + (scroll_left / scroll_range) * (track_w - thumb_w);

    Some((
        track_x,
        track_y,
        track_w,
        sb_height,
        thumb_x,
        thumb_w,
        scroll_range,
        px_per_col,
    ))
}

/// Hit-test a point against all h scrollbars. Returns `(window_id,
/// scroll_left_at_click)` when the point is on any h scrollbar track (not only
/// the thumb), so the caller can decide between thumb-drag and track-click.
fn h_scrollbar_hit_test(
    engine: &Engine,
    x: f64,
    y: f64,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
) -> Option<(core::WindowId, usize)> {
    for (window_id, rect) in window_rects {
        if let Some((track_x, track_y, track_w, sb_height, _, _, _, _)) =
            h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        {
            if x >= track_x && x <= track_x + track_w && y >= track_y && y <= track_y + sb_height {
                let scroll_left = engine
                    .windows
                    .get(window_id)
                    .map(|w| w.view.scroll_left)
                    .unwrap_or(0);
                return Some((*window_id, scroll_left));
            }
        }
    }
    None
}

/// Hit-test tab close buttons. Returns `Some((group_id.0, tab_idx))` if the
/// mouse is over a tab's × button, matching the same geometry as the click handler.
/// Tab-close hover hit-test driven by the rasteriser's cached
/// `close_bounds`. Each frame the GTK rasteriser publishes the exact
/// per-tab close-button rectangle (Pango pixel widths, not estimates)
/// to `App.tab_close_bounds`; this function consults those bounds
/// rather than re-deriving geometry from `name.chars() * char_width`,
/// which under-estimates Pango widths and shifts the close zone.
fn tab_close_hit_test(
    engine: &Engine,
    close_bounds_map: &TabCloseMap,
    mx: f64,
    my: f64,
    da_w: f64,
    da_h: f64,
    line_height: f64,
) -> Option<(usize, usize)> {
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };
    let editor_bottom = gtk_editor_bottom(engine, da_w, da_h, line_height);
    let content_bounds = core::WindowRect::new(0.0, 0.0, da_w, editor_bottom);
    let mut group_rects = engine
        .group_layout
        .calculate_group_rects(content_bounds, tab_bar_height);
    engine.adjust_group_rects_for_hidden_tabs(&mut group_rects, tab_bar_height);

    for (gid, grect) in &group_rects {
        if engine.is_tab_bar_hidden(*gid) {
            continue;
        }
        let tab_y = grect.y - tab_bar_height;
        if my < tab_y || my >= tab_y + tab_row_height || mx < grect.x || mx >= grect.x + grect.width
        {
            continue;
        }
        let local_x = mx - grect.x;
        let Some(close_bounds) = close_bounds_map.get(&gid.0) else {
            continue;
        };
        for (i, cb) in close_bounds.iter().enumerate() {
            if let Some((cx_start, cx_end)) = cb {
                if local_x >= *cx_start && local_x < *cx_end {
                    return Some((gid.0, i));
                }
            }
        }
    }
    None
}

/// Returns a shortened display path for the tab under the cursor, or `None` if
/// the cursor is not over a tab or the tab has no file path.
fn tab_tooltip_hit_test(
    engine: &Engine,
    mx: f64,
    my: f64,
    da_w: f64,
    da_h: f64,
    line_height: f64,
    char_width: f64,
) -> Option<String> {
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };
    let editor_bottom = gtk_editor_bottom(engine, da_w, da_h, line_height);
    let content_bounds = core::WindowRect::new(0.0, 0.0, da_w, editor_bottom);
    let mut group_rects = engine
        .group_layout
        .calculate_group_rects(content_bounds, tab_bar_height);
    engine.adjust_group_rects_for_hidden_tabs(&mut group_rects, tab_bar_height);

    let close_w = char_width;
    let tab_pad = 14.0_f64;
    let tab_inner_gap = 10.0_f64;
    let tab_outer_gap = 1.0_f64;

    for (gid, grect) in &group_rects {
        if engine.is_tab_bar_hidden(*gid) {
            continue;
        }
        let tab_y = grect.y - tab_bar_height;
        if my < tab_y || my >= tab_y + tab_row_height || mx < grect.x || mx >= grect.x + grect.width
        {
            continue;
        }
        let local_x = mx - grect.x;
        if let Some(group) = engine.editor_groups.get(gid) {
            let mut tab_x = 0.0;
            for (i, tab) in group.tabs.iter().enumerate() {
                let wid = tab.active_window;
                let (name, file_path) = if let Some(window) = engine.windows.get(&wid) {
                    if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                        let dirty = if state.dirty { "*" } else { "" };
                        (
                            format!(" {}: {}{} ", i + 1, state.display_name(), dirty),
                            state.file_path.clone(),
                        )
                    } else {
                        (format!(" {}: [No Name] ", i + 1), None)
                    }
                } else {
                    (format!(" {}: [No Name] ", i + 1), None)
                };
                let tab_w = name.chars().count() as f64 * char_width;
                let slot_w = tab_pad + tab_w + tab_inner_gap + close_w + tab_pad + tab_outer_gap;
                if local_x >= tab_x && local_x < tab_x + slot_w {
                    return file_path.map(|p| shorten_path(&p));
                }
                tab_x += slot_w;
            }
        }
    }
    None
}

/// Shorten a path for display: replace the user's home directory with `~`.
fn shorten_path(path: &std::path::Path) -> String {
    let home = core::paths::home_dir();
    if let Ok(rest) = path.strip_prefix(&home) {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// Entry point for GTK mode.
pub(crate) fn run(file_path: Option<PathBuf>) {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        std::env::set_var("DISPLAY", ":0");
    }

    // Install panic hook that flushes swap files + writes crash log.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Emergency: flush swap files for all dirty buffers.
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                eprintln!("VimCode crashed. Details written to {}", path.display());
                eprintln!("Unsaved buffers written to swap files for recovery.");
                eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
            }
            prev_hook(info);
        }));
    }

    install_icon_and_desktop();
    unsafe {
        gtk4::glib::ffi::g_log_set_writer_func(Some(gtk_log_writer), std::ptr::null_mut(), None);
    }
    // Initialize GTK before App::new() so that CssProvider, Display,
    // and Settings calls inside App::new() find an initialized toolkit.
    // Under the old Relm4 path this happened inside RelmApp::create_and_run();
    // with the ShellApp runner it happens inside gapp.run() which is called
    // by run_with_shell() — too late for App::new().
    gtk4::init().expect("Failed to initialize GTK");
    // Create the App and run via the quadraui ShellApp runner.
    // The runner creates its own GTK Application + window; vimcode's engine
    // and event handling are wired in via impl ShellApp for App above.
    let vimcode_app = App::new(file_path);
    // Mirror the engine's AppShell panel list into the ShellConfig so the
    // quadraui runner renders the activity bar icons.  The engine stores all
    // panels (including "bottom:settings") in a single `panels()` slice;
    // ShellConfig wants top-pinned panels in its first arg and bottom-pinned
    // items via `with_bottom_items()`, so split on the "bottom:" ID prefix.
    // Fill in activity-bar icons before building ShellConfig.  The engine's
    // AppShell initialises all PanelDefinition.icon fields to "" because the
    // engine itself is backend-agnostic; the GTK runner is responsible for
    // mapping each panel ID to the correct Nerd-Font / fallback glyph.
    let panels_with_icons: Vec<_> = vimcode_app
        .engine
        .borrow()
        .app_shell
        .panels()
        .iter()
        .cloned()
        .map(|mut p| {
            p.icon = match p.id.as_str() {
                "panel:explorer" => crate::icons::EXPLORER.s().to_string(),
                "panel:search" => crate::icons::SEARCH_COD.s().to_string(),
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
    let (top_panels, bottom_items): (Vec<_>, Vec<_>) = panels_with_icons
        .into_iter()
        .partition(|p| !p.id.as_str().starts_with("bottom:"));
    let config = quadraui::ShellConfig::new("VimCode", top_panels).with_bottom_items(bottom_items);
    quadraui::gtk::shell_runner::run_with_shell(vimcode_app, config);
}
