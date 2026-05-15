// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional
// TODO: Migrate to ListView/ColumnView in a future phase
#![allow(deprecated)]
// Relm4 view! macro generates #[name = "..."] bindings that trigger this lint
#![allow(unused_assignments)]

use gio::prelude::{FileExt, FileMonitorExt};
use gtk4::cairo::Context;
use gtk4::gdk;
use gtk4::pango::{self, AttrColor, AttrList, FontDescription};
use gtk4::prelude::*;
use pangocairo::functions as pangocairo;
use relm4::prelude::*;
use std::cell::{Cell, RefCell};
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

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants used in later phases
enum SidebarPanel {
    Explorer,
    Search,
    Debug,
    Git,
    Extensions,
    Settings,
    Ai,
    ExtPanel(String),
    None,
}

impl SidebarPanel {
    fn to_panel_id(&self) -> Option<&'static str> {
        use crate::core::engine::sidebar::*;
        match self {
            SidebarPanel::Explorer => Some(PANEL_EXPLORER),
            SidebarPanel::Search => Some(PANEL_SEARCH),
            SidebarPanel::Debug => Some(PANEL_DEBUG),
            SidebarPanel::Git => Some(PANEL_GIT),
            SidebarPanel::Extensions => Some(PANEL_EXTENSIONS),
            SidebarPanel::Ai => Some(PANEL_AI),
            SidebarPanel::Settings => Some(PANEL_SETTINGS),
            _ => None,
        }
    }

    fn from_panel_id(id: &str) -> SidebarPanel {
        use crate::core::engine::sidebar::*;
        match id {
            PANEL_EXPLORER => SidebarPanel::Explorer,
            PANEL_SEARCH => SidebarPanel::Search,
            PANEL_DEBUG => SidebarPanel::Debug,
            PANEL_GIT => SidebarPanel::Git,
            PANEL_EXTENSIONS => SidebarPanel::Extensions,
            PANEL_AI => SidebarPanel::Ai,
            PANEL_SETTINGS => SidebarPanel::Settings,
            _ => SidebarPanel::None,
        }
    }
}

type TabSlotMap = HashMap<usize, Vec<(f64, f64)>>;
type TabCloseMap = HashMap<usize, Vec<Option<(f64, f64)>>>;

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
    sender: &ComponentSender<App>,
    engine: &Rc<RefCell<Engine>>,
) -> bool {
    match id {
        ACC_OPEN_TERMINAL => {
            sender.input(Msg::ToggleTerminal);
            true
        }
        ACC_TOGGLE_SIDEBAR => {
            sender.input(Msg::ToggleSidebar);
            true
        }
        ACC_FOCUS_EXPLORER => {
            sender.input(Msg::ToggleFocusExplorer);
            true
        }
        ACC_FOCUS_SEARCH => {
            sender.input(Msg::ToggleFocusSearch);
            true
        }
        ACC_FUZZY_FINDER => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Files);
            sender.input(Msg::Resize);
            true
        }
        ACC_LIVE_GREP => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Grep);
            sender.input(Msg::Resize);
            true
        }
        ACC_COMMAND_PALETTE => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Commands);
            sender.input(Msg::Resize);
            true
        }
        ACC_TERMINAL_TOGGLE_MAX => {
            sender.input(Msg::ToggleTerminalMaximize);
            true
        }
        ACC_ADD_CURSOR => {
            engine.borrow_mut().add_cursor_at_next_match();
            sender.input(Msg::Resize);
            true
        }
        ACC_SELECT_ALL_MATCHES => {
            engine.borrow_mut().select_all_occurrences();
            sender.input(Msg::Resize);
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

struct App {
    engine: Rc<RefCell<Engine>>,
    /// Set to true in update() whenever a draw is needed; cleared by the #[watch] block.
    /// This prevents the 20/sec SearchPollTick timer from unconditionally calling queue_draw().
    draw_needed: Rc<Cell<bool>>,
    sidebar_visible: bool,
    active_panel: SidebarPanel,
    /// DrawingArea for the file explorer sidebar (Phase A.2b-2: native
    /// `gtk4::TreeView` replaced by a single DrawingArea rendering via
    /// `draw_explorer_panel`).
    explorer_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// A.6f: activity bar DA handle; used to queue redraws when panel
    /// state or extension registrations change.
    activity_bar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// A.6f: shared `active_panel` mirror — lets the draw func read the
    /// current panel without borrowing `&self`. Updated in `Msg::SwitchPanel`.

    /// Row height actually used by the most recent explorer draw call.
    /// The draw callback writes this each frame from the same Pango
    /// context it renders with, so click and scroll handlers hit-test with
    /// byte-exact row math.
    explorer_row_height_cell: Rc<Cell<f64>>,
    /// Fractional dy accumulator for the explorer scroll wheel. Small
    /// trackpad deltas are summed here until they exceed one row, so no
    /// scroll event is silently dropped.
    explorer_scroll_accum: Rc<Cell<f64>>,
    /// Most recent scrollbar rect in DA-local coords, published by
    /// `draw_explorer_panel` each frame: `Some((x, y, w, h))` when a
    /// scrollbar is visible, `None` otherwise. Used by the click/drag
    /// handlers to hit-test scrollbar interactions.
    #[allow(clippy::type_complexity)]
    explorer_scrollbar_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
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
    sender: relm4::Sender<Msg>,
    /// Last content written to system clipboard.
    /// Used to avoid redundant writes on every keystroke.
    last_clipboard_content: Option<String>,
    /// System clipboard context (copypasta-ext).  None if unavailable.
    // Box<dyn ClipboardProviderExt> is !Send; GTK App lives on main thread only.
    clipboard: Option<Box<dyn ClipboardProviderExt>>,
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
    /// Cached diff toolbar button pixel positions, populated during draw_tab_bar.
    diff_btn_map: Rc<RefCell<DiffBtnMap>>,
    split_btn_map: Rc<RefCell<SplitBtnMap>>,
    action_btn_map: Rc<RefCell<ActionBtnMap>>,
    /// Cached per-window status bar segment hit zones from draw_window_status_bar.
    status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Cached ScreenLayout from the last draw_editor paint pass. Click handlers
    /// read this instead of recomputing geometry from engine state (#344).
    cached_screen_layout: Rc<RefCell<Option<render::ScreenLayout>>>,
    /// Cached debug toolbar layout from `draw_debug_toolbar`.
    debug_toolbar_layout: Rc<RefCell<Option<quadraui::StatusBarLayout>>>,
    /// Pixel y-offset where the debug toolbar was last drawn.
    debug_toolbar_y_offset: Rc<Cell<f64>>,
    /// Pixel height of the debug toolbar (last draw).
    debug_toolbar_height: Rc<Cell<f64>>,
    /// Which debug toolbar button the cursor is over (hit-tested from cached regions).
    debug_toolbar_hovered_id: Rc<RefCell<Option<quadraui::WidgetId>>>,
    /// Which debug toolbar button is currently pressed (mouse-down, not yet released).
    debug_toolbar_pressed_id: Rc<RefCell<Option<quadraui::WidgetId>>>,
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
    /// Reference to the root GTK window used for minimize / maximize / close actions.
    window: gtk4::Window,
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
    /// Completion popup bounding rect (x, y, w, h) — set during draw,
    /// used for `ModalStack` registration in the click handler. None
    /// when the popup isn't visible. (B.5b Stage 5.)
    #[allow(clippy::type_complexity)]
    completion_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
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
    /// Active context-menu popover (explorer or tab). Kept alive so we can
    /// unparent it before creating a new one (avoids GTK CSS node assertions).
    active_ctx_popover: Rc<RefCell<Option<gtk4::PopoverMenu>>>,
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

/// Map GDK key names to the engine's expected key names.
///
/// This is the canonical superset mapping — callers that only care about a
/// subset simply ignore the extra translations (they're harmless).
/// A.6f: adapter — build the `quadraui::ActivityBar` primitive that the
/// GTK activity bar DrawingArea renders each frame.
///
/// Item order matches the pre-migration view! macro layout:
/// * Top: explorer · search · debug · git · extensions · AI
///   · dynamically-registered extension panels (sorted by name)
/// * Bottom: settings
///
/// GTK has no keyboard-focused highlight (mouse-driven UX), so every
/// `is_keyboard_selected` is false. Hover state is layered in by the
/// draw function via a separate `hovered_idx` parameter.
fn build_gtk_activity_bar_primitive(
    engine: &crate::core::engine::Engine,
    theme: &crate::render::Theme,
) -> quadraui::ActivityBar {
    use crate::core::engine::sidebar::*;
    let sb_visible = engine.app_shell.sidebar_visible();
    let has_ext = engine.ext_panel_active.is_some();
    let active_id = engine.app_shell.active_panel_id().map(|w| w.as_str());

    let fixed = [
        (
            PANEL_EXPLORER,
            icons::EXPLORER.nerd,
            "Explorer (Ctrl+Shift+E)",
            "activity:explorer",
        ),
        (
            PANEL_SEARCH,
            icons::SEARCH_COD.nerd,
            "Search (Ctrl+Shift+F)",
            "activity:search",
        ),
        (PANEL_DEBUG, icons::DEBUG.nerd, "Debug", "activity:debug"),
        (
            PANEL_GIT,
            icons::GIT_BRANCH.nerd,
            "Source Control",
            "activity:git",
        ),
        (
            PANEL_EXTENSIONS,
            icons::EXTENSIONS.nerd,
            "Extensions",
            "activity:extensions",
        ),
        (PANEL_AI, icons::AI_CHAT.nerd, "AI Assistant", "activity:ai"),
    ];

    let mut top: Vec<quadraui::ActivityItem> = fixed
        .iter()
        .map(
            |(panel_id, icon, tooltip, activity_id)| quadraui::ActivityItem {
                id: quadraui::WidgetId::new(*activity_id),
                icon: (*icon).to_string(),
                tooltip: (*tooltip).to_string(),
                is_active: sb_visible && !has_ext && active_id == Some(*panel_id),
                is_keyboard_selected: false,
            },
        )
        .collect();

    let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
    ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
    for panel in ext_panels {
        let is_active = sb_visible && engine.ext_panel_active.as_deref() == Some(&panel.name);
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(format!("activity:ext:{}", panel.name)),
            icon: panel.resolved_icon().to_string(),
            tooltip: panel.title.clone(),
            is_active,
            is_keyboard_selected: false,
        });
    }

    let bottom = vec![quadraui::ActivityItem {
        id: quadraui::WidgetId::new("activity:settings"),
        icon: icons::SETTINGS.nerd.to_string(),
        tooltip: "Settings".to_string(),
        is_active: sb_visible && !has_ext && active_id == Some(PANEL_SETTINGS),
        is_keyboard_selected: false,
    }];

    quadraui::ActivityBar {
        id: quadraui::WidgetId::new("activity-bar"),
        top_items: top,
        bottom_items: bottom,
        active_accent: Some(quadraui::Color::rgb(
            theme.cursor.r,
            theme.cursor.g,
            theme.cursor.b,
        )),
        selection_bg: None,
    }
}

/// A.6f: decode a `WidgetId` from `build_gtk_activity_bar_primitive` into
/// the engine-side `SidebarPanel` enum used by `Msg::SwitchPanel`.
fn activity_id_to_panel(id: &str) -> Option<SidebarPanel> {
    match id {
        "activity:explorer" => Some(SidebarPanel::Explorer),
        "activity:search" => Some(SidebarPanel::Search),
        "activity:debug" => Some(SidebarPanel::Debug),
        "activity:git" => Some(SidebarPanel::Git),
        "activity:extensions" => Some(SidebarPanel::Extensions),
        "activity:ai" => Some(SidebarPanel::Ai),
        "activity:settings" => Some(SidebarPanel::Settings),
        other => other
            .strip_prefix("activity:ext:")
            .map(|name| SidebarPanel::ExtPanel(name.to_string())),
    }
}

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
    SwitchPanel(SidebarPanel),
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
    /// Mouse-wheel on the explorer DrawingArea. Positive dy scrolls down.
    ExplorerScroll(f64),
    /// Prompt the user for a filename to rename `path` to. Dialog fallback
    /// used by GTK since inline TextInput editing on `draw_tree` rows is
    /// deferred until a future primitive stage.
    PromptRenameFile(PathBuf),
    /// Prompt the user for a filename for a new file under `parent_dir`.
    PromptNewFile(PathBuf),
    /// Prompt the user for a folder name under `parent_dir`.
    PromptNewFolder(PathBuf),
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

#[relm4::component]
impl SimpleComponent for App {
    type Init = Option<PathBuf>;
    type Input = Msg;
    type Output = ();

    view! {
        gtk4::Window {
            set_title: Some("VimCode"),
            set_default_size: (800, 600),
            set_icon_name: Some("vimcode"),

            // Intercept window close — check for unsaved changes before allowing quit.
            connect_close_request[sender] => move |_window| {
                sender.input(Msg::ShowQuitConfirm);
                gtk4::glib::Propagation::Stop
            },

            #[name = "window_overlay"]
            gtk4::Overlay {
            gtk4::Box {
                set_orientation: gtk4::Orientation::Vertical,

                // Menu bar row — set as custom titlebar imperatively in init().
                // CSD provides edge resize handles; WindowHandle enables drag-to-move.
                #[name = "menu_bar_row"]
                gtk4::Box {
                    set_orientation: gtk4::Orientation::Horizontal,
                    set_css_classes: &["custom-titlebar"],

                    gtk4::WindowHandle {
                        set_hexpand: true,

                        #[name = "menu_bar_da"]
                        gtk4::DrawingArea {
                            set_hexpand: true,
                            set_height_request: 24,
                        },
                    },

                    // Window control buttons — VSCode style
                    // Minimize: thin dash; Maximize: thin square; Close: thin ×
                    gtk4::Button {
                        set_label: "\u{2212}",
                        set_tooltip_text: Some("Minimize"),
                        set_css_classes: &["window-control"],
                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::WindowMinimize);
                        }
                    },
                    #[name = "maximize_button"]
                    gtk4::Button {
                        set_label: "\u{25a1}",
                        set_tooltip_text: Some("Maximize"),
                        set_css_classes: &["window-control"],
                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::WindowMaximize);
                        }
                    },
                    gtk4::Button {
                        set_label: "\u{2715}",
                        set_tooltip_text: Some("Close"),
                        set_css_classes: &["window-control"],
                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::WindowClose);
                        }
                    },
                },

                #[name = "main_hbox"]
                gtk4::Box {
                    set_orientation: gtk4::Orientation::Horizontal,
                    set_vexpand: true,

                // Activity Bar (48px, always visible).
                // A.6f: migrated from a `gtk4::Box` with native `gtk4::Button`
                // children to a single `DrawingArea` that renders via
                // `quadraui_gtk::draw_activity_bar`. Rendering + click +
                // hover + tooltip wiring is imperative (below this view!
                // macro) to match the A.2b-2 / A.3c-2 pattern.
                #[name = "activity_bar"]
                gtk4::DrawingArea {
                    set_width_request: 48,
                    set_vexpand: true,
                    set_css_classes: &["activity-bar"],
                    set_can_focus: true,
                    set_has_tooltip: true,
                },

                // Sidebar (collapsible with Revealer)
                #[name = "sidebar_revealer"]
                gtk4::Revealer {
                    set_transition_type: gtk4::RevealerTransitionType::SlideRight,
                    set_transition_duration: 200,

                    #[watch]
                    set_reveal_child: model.sidebar_visible,

                    // ScrolledWindow constrains children to the allocated width
                    // (hscrollbar Never prevents content from growing the sidebar).
                    #[name = "sidebar_inner_sw"]
                    gtk4::ScrolledWindow {
                        set_width_request: 260,
                        set_hexpand: false,
                        set_hscrollbar_policy: gtk4::PolicyType::Never,
                        set_vscrollbar_policy: gtk4::PolicyType::Never,

                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar-container"],

                        // Explorer panel (A.2b-2: DrawingArea + quadraui_gtk::draw_tree)
                        #[name = "explorer_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: model.active_panel == SidebarPanel::Explorer,

                            #[name = "explorer_da"]
                            gtk4::DrawingArea {
                                set_hexpand: true,
                                set_vexpand: true,
                                set_focusable: true,
                            },
                        },

                        // Settings panel — Phase A.3c-2: native widget tree replaced
                        // by a single DrawingArea that renders via `draw_settings_panel`
                        // (which calls `quadraui_gtk::draw_form`). Visibility
                        // managed imperatively via settings_panel_box.
                        #[name = "settings_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],
                            set_visible: false,  // hidden initially; toggled via settings_panel_box

                            #[name = "settings_da"]
                            gtk4::DrawingArea {
                                set_hexpand: true,
                                set_vexpand: true,
                            },
                        },

                        // Search panel (quadraui DrawingArea)
                        #[name = "search_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: {
                                if model.active_panel == SidebarPanel::Search {
                                    search_sidebar_da.queue_draw();
                                }
                                model.active_panel == SidebarPanel::Search
                            },

                            #[name = "search_sidebar_da"]
                            gtk4::DrawingArea {
                                set_vexpand: true,
                            },
                        },

                        // Debug sidebar panel
                        #[name = "debug_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: {
                                if model.active_panel == SidebarPanel::Debug {
                                    debug_sidebar_da.queue_draw();
                                }
                                model.active_panel == SidebarPanel::Debug
                            },

                            #[name = "debug_sidebar_da"]
                            gtk4::DrawingArea {
                                set_vexpand: true,
                            },
                        },

                        // Source Control (Git) sidebar panel
                        #[name = "git_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: {
                                if model.active_panel == SidebarPanel::Git {
                                    git_sidebar_da.queue_draw();
                                }
                                model.active_panel == SidebarPanel::Git
                            },

                            #[name = "git_sidebar_da"]
                            gtk4::DrawingArea {
                                set_vexpand: true,
                            },
                        },

                        // Extensions sidebar panel
                        #[name = "ext_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: {
                                if model.active_panel == SidebarPanel::Extensions {
                                    ext_sidebar_da.queue_draw();
                                }
                                model.active_panel == SidebarPanel::Extensions
                            },

                            #[name = "ext_sidebar_da"]
                            gtk4::DrawingArea {
                                set_vexpand: true,
                            },
                        },

                        // Extension-provided panel (e.g. git-insights GIT LOG)
                        #[name = "ext_dyn_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: {
                                if matches!(model.active_panel, SidebarPanel::ExtPanel(_)) {
                                    ext_dyn_panel_da.queue_draw();
                                }
                                matches!(model.active_panel, SidebarPanel::ExtPanel(_))
                            },

                            #[name = "ext_dyn_panel_da"]
                            gtk4::DrawingArea {
                                set_vexpand: true,
                            },
                        },

                        // AI assistant sidebar panel
                        #[name = "ai_panel_box"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: {
                                if model.active_panel == SidebarPanel::Ai {
                                    ai_sidebar_da.queue_draw();
                                }
                                model.active_panel == SidebarPanel::Ai
                            },

                            #[name = "ai_sidebar_da"]
                            gtk4::DrawingArea {
                                set_vexpand: true,
                                set_focusable: true,
                            },
                        },
                    },  // close inner Box
                    },  // close ScrolledWindow
                },  // close Revealer

                // Sidebar resize drag handle (6px wide, ew-resize cursor)
                #[name = "sidebar_resize_handle"]
                gtk4::Box {
                    set_width_request: 6,
                    set_vexpand: true,
                    set_css_classes: &["sidebar-resize-handle"],

                    #[watch]
                    set_visible: model.sidebar_visible,
                },

                // Editor area (DrawingArea wrapped in Overlay for scrollbars)
                gtk4::Box {
                    set_orientation: gtk4::Orientation::Vertical,
                    set_hexpand: true,

                    #[name = "editor_overlay"]
                    gtk4::Overlay {
                        #[name = "drawing_area"]
                        gtk4::DrawingArea {
                            set_hexpand: true,
                            set_vexpand: true,
                            set_focusable: true,
                            grab_focus: (),

                            add_controller = gtk4::EventControllerKey {
                                set_propagation_phase: gtk4::PropagationPhase::Capture,
                                connect_key_pressed[sender, engine, backend_events, backend, lh_cell = line_height_cell.clone()] => move |ctrl_ref, key, _, modifier| {
                                    // Phase B.5b Stage 1: dual-write the
                                    // translated UiEvent into the backend
                                    // queue. The drain timer consumes and
                                    // discards today; B5b.2 routes the
                                    // accelerator-shaped events back into
                                    // dispatch.
                                    let ui_event = events::gdk_key_to_uievent(key, modifier, false);
                                    if let Some(ref ev) = ui_event {
                                        backend_events.borrow_mut().push_back(ev.clone());
                                    }

                                    let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                                    let unicode = key.to_unicode().filter(|c| !c.is_control());
                                    let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                                    let shift = modifier.contains(gdk::ModifierType::SHIFT_MASK);
                                    let alt = modifier.contains(gdk::ModifierType::ALT_MASK);

                                    if util::is_modifier_only_key(&key_name) {
                                        return gtk4::glib::Propagation::Proceed;
                                    }

                                    let entry_has_focus = ctrl_ref
                                        .widget()
                                        .root()
                                        .and_then(|r| r.downcast::<gtk4::Window>().ok())
                                        .and_then(|w| gtk4::prelude::GtkWindowExt::focus(&w))
                                        .is_some_and(|f| {
                                            f.downcast_ref::<gtk4::Entry>().is_some()
                                                || f.downcast_ref::<gtk4::Text>().is_some()
                                        });
                                    if entry_has_focus {
                                        return gtk4::glib::Propagation::Proceed;
                                    }

                                    // MenuSystem intercept — handles Alt+letter, arrow keys, Enter, Escape.
                                    // Same pattern as TUI: one call handles all menu keyboard events.
                                    if let Some(ref ev) = ui_event {
                                        let bar_visible = engine.borrow().menu_bar_visible;
                                        if bar_visible {
                                            let lh = lh_cell.get() as f32;
                                            let win_w = ctrl_ref
                                                .widget()
                                                .root()
                                                .and_then(|r| r.downcast::<gtk4::Window>().ok())
                                                .map(|w| w.width() as f32)
                                                .unwrap_or(800.0);
                                            let bar_rect = quadraui::Rect::new(0.0, 0.0, win_w, lh);
                                            if lh > 1.0 {
                                                let menu_event = engine.borrow().menu_system.borrow_mut()
                                                    .handle(ev, &mut *backend.borrow_mut(), bar_rect);
                                                match menu_event {
                                                    quadraui::MenuEvent::Activated(id) => {
                                                        sender.input(Msg::HandleMenuAction(id.as_str().to_string()));
                                                        return gtk4::glib::Propagation::Stop;
                                                    }
                                                    quadraui::MenuEvent::StateChanged
                                                    | quadraui::MenuEvent::Consumed => {
                                                        sender.input(Msg::MenuRedraw);
                                                        return gtk4::glib::Propagation::Stop;
                                                    }
                                                    quadraui::MenuEvent::Ignored => {}
                                                }
                                            }
                                        }
                                    }

                                    // Ctrl+Tab / Ctrl+Shift+Tab: MRU tab switcher
                                    if ctrl && !alt && key_name == "Tab" {
                                        engine.borrow_mut().tab_switcher_cycle(true);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if ctrl && !alt && key_name == "ISO_Left_Tab" {
                                        engine.borrow_mut().tab_switcher_cycle(false);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Alt+t: MRU tab switcher (open or cycle forward)
                                    if alt && !ctrl && !shift && unicode == Some('t') {
                                        engine.borrow_mut().tab_switcher_cycle(true);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Alt-M: toggle Vim ↔ VSCode editing mode
                                    if alt && !ctrl && !shift && unicode == Some('m') {
                                        engine.borrow_mut().toggle_editor_mode();
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Alt+, / Alt+. — resize editor group split
                                    if alt && !ctrl && !shift {
                                        if unicode == Some(',') {
                                            engine.borrow_mut().group_resize(-0.05);
                                            sender.input(Msg::Resize);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if unicode == Some('.') {
                                            engine.borrow_mut().group_resize(0.05);
                                            sender.input(Msg::Resize);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }

                                    // Shift+Alt+F: LSP format document
                                    if alt && shift && !ctrl {
                                        let key_lower = key_name.to_ascii_lowercase();
                                        if key_lower == "f" {
                                            engine.borrow_mut().lsp_format_current();
                                            sender.input(Msg::Resize);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }

                                    // Ctrl-F without terminal focus: engine find/replace.
                                    // (Terminal-focused Ctrl+F is handled by handle_terminal_key.)
                                    if ctrl && !shift && unicode == Some('f')
                                        && !engine.borrow().terminal_has_focus
                                    {
                                        engine.borrow_mut().handle_key("f", Some('f'), true);
                                        sender.input(Msg::SearchPollTick);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Ctrl-Shift-V without terminal focus: paste to editor.
                                    // (Terminal-focused paste is handled by handle_terminal_key.)
                                    if ctrl && shift && (key_name == "v" || key_name == "V")
                                        && !engine.borrow().terminal_has_focus
                                    {
                                        sender.input(Msg::KeyPress {
                                            key_name: "PasteClipboard".to_string(),
                                            unicode: None,
                                            ctrl: false,
                                            alt: false,
                                        });
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Panel navigation — driven by panel_keys settings.
                                    //
                                    // Phase B.5b Stage 2: a single
                                    // registry lookup against `GtkBackend`'s
                                    // accelerator table replaces 13 inline
                                    // `if matches_gtk_key(&pk.X, ...)`
                                    // arms. The lookup runs once and the
                                    // result is dispatched in two windows:
                                    // — early (here) only for
                                    //   `ACC_OPEN_TERMINAL`, so Ctrl+T
                                    //   keeps working when the terminal has
                                    //   focus;
                                    // — late (after the terminal-focus
                                    //   block) for every other id.
                                    let matched_acc_id = events::gdk_key_to_quadraui_key(key)
                                        .and_then(|qkey| {
                                            let qmods = events::gdk_modifiers_to_quadraui(modifier);
                                            backend.borrow().match_keypress(&qkey, qmods)
                                        });
                                    if let Some(ref id) = matched_acc_id {
                                        if id.as_str() == ACC_OPEN_TERMINAL {
                                            sender.input(Msg::ToggleTerminal);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }
                                    // Phase B.2: engine-side accelerator
                                    // registry. Currently only carries
                                    // `terminal.toggle_maximize`. Distinct
                                    // from `GtkBackend`'s registry; both
                                    // exist so the engine can register
                                    // accelerators visible to all backends
                                    // while the GTK-only panel keys live on
                                    // the backend.
                                    {
                                        let eng = engine.borrow();
                                        if let Some(id) = eng.match_accelerator(
                                            modifier.contains(gdk::ModifierType::CONTROL_MASK),
                                            modifier.contains(gdk::ModifierType::SHIFT_MASK),
                                            modifier.contains(gdk::ModifierType::ALT_MASK),
                                            key.to_unicode(),
                                            key == gdk::Key::Tab
                                                || key == gdk::Key::ISO_Left_Tab,
                                            key.to_unicode() == Some(' '),
                                            key == gdk::Key::Escape,
                                        ) {
                                            drop(eng);
                                            if id.as_str() == "terminal.toggle_maximize" {
                                                sender.input(Msg::ToggleTerminalMaximize);
                                            }
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }
                                    // Terminal key routing (#351): engine decides
                                    // the action, backend executes clipboard I/O.
                                    if engine.borrow().terminal_has_focus {
                                        use core::engine::TerminalKeyAction;
                                        let action = engine.borrow_mut().handle_terminal_key(
                                            &key_name, unicode, ctrl, shift, alt,
                                        );
                                        match action {
                                            TerminalKeyAction::CopySelection => {
                                                sender.input(Msg::TerminalCopySelection);
                                            }
                                            TerminalKeyAction::PasteClipboard => {
                                                sender.input(Msg::TerminalPasteClipboard);
                                            }
                                            TerminalKeyAction::SendToPty(data) => {
                                                engine.borrow_mut().terminal_write(&data);
                                                sender.input(Msg::Resize);
                                            }
                                            TerminalKeyAction::Handled => {
                                                sender.input(Msg::Resize);
                                            }
                                            TerminalKeyAction::Ignore => {}
                                        }
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    // Phase B.5b Stage 2: late panel-key
                                    // dispatch. The lookup ran once before
                                    // the engine's accelerator block; if
                                    // it matched and wasn't the early
                                    // `ACC_OPEN_TERMINAL` shortcut, the
                                    // dispatcher routes the action here.
                                    // Replaces 12 inline `matches_gtk_key`
                                    // arms (toggle_sidebar / focus_explorer /
                                    // focus_search / fuzzy_finder / live_grep /
                                    // command_palette / add_cursor /
                                    // select_all_matches / split_editor_right /
                                    // split_editor_down / nav_back /
                                    // nav_forward).
                                    if let Some(id) = &matched_acc_id {
                                        if dispatch_gtk_panel_accelerator(id.as_str(), &sender, &engine) {
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }

                                    // Shift+F5 → stop, Shift+F11 → stepout (debug shortcuts)
                                    if shift && !ctrl && !alt {
                                        match key_name.as_str() {
                                            "F5" => {
                                                engine.borrow_mut().execute_command("stop");
                                                return gtk4::glib::Propagation::Stop;
                                            }
                                            "F11" => {
                                                engine.borrow_mut().execute_command("stepout");
                                                return gtk4::glib::Propagation::Stop;
                                            }
                                            _ => {}
                                        }
                                    }

                                    // Alt+] / Alt+[ — cycle AI ghost text alternatives.
                                    if alt && !ctrl && !shift {
                                        let in_insert = engine.borrow().mode == crate::core::Mode::Insert;
                                        if in_insert {
                                            if key_name == "bracketright" {
                                                engine.borrow_mut().ai_ghost_next_alt();
                                                sender.input(Msg::Resize);
                                                return gtk4::glib::Propagation::Stop;
                                            }
                                            if key_name == "bracketleft" {
                                                engine.borrow_mut().ai_ghost_prev_alt();
                                                sender.input(Msg::Resize);
                                                return gtk4::glib::Propagation::Stop;
                                            }
                                        }
                                    }

                                    // VSCode mode: Ctrl+] indent / Ctrl+[ outdent.
                                    // GDK may report bracket keys as "bracketright"/"bracketleft"
                                    // OR as control characters, so handle both.
                                    if engine.borrow().is_vscode_mode() && ctrl && !alt {
                                        let is_bracket_right = key_name == "bracketright"
                                            || key == gdk::Key::bracketright;
                                        let is_bracket_left = key_name == "bracketleft"
                                            || key == gdk::Key::bracketleft;
                                        // Shift+[ → braceleft/{, Shift+] → braceright/}
                                        let is_brace_left = key_name == "braceleft"
                                            || key_name == "{"
                                            || key == gdk::Key::braceleft;
                                        let is_brace_right = key_name == "braceright"
                                            || key_name == "}"
                                            || key == gdk::Key::braceright;
                                        // Ctrl+Shift+[ → fold, Ctrl+Shift+] → unfold
                                        if shift && (is_bracket_left || is_brace_left) {
                                            sender.input(Msg::KeyPress {
                                                key_name: "Shift_bracketleft".to_string(),
                                                unicode: None,
                                                ctrl: true,
                                                alt: false,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if shift && (is_bracket_right || is_brace_right) {
                                            sender.input(Msg::KeyPress {
                                                key_name: "Shift_bracketright".to_string(),
                                                unicode: None,
                                                ctrl: true,
                                                alt: false,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Ctrl+[ → outdent, Ctrl+] → indent (no shift)
                                        if is_bracket_right && !shift {
                                            sender.input(Msg::KeyPress {
                                                key_name: "bracketright".to_string(),
                                                unicode: None,
                                                ctrl: true,
                                                alt: false,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if is_bracket_left && !shift {
                                            sender.input(Msg::KeyPress {
                                                key_name: "bracketleft".to_string(),
                                                unicode: None,
                                                ctrl: true,
                                                alt: false,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }

                                    // In VSCode mode, encode Alt+key and Shift+key into
                                    // prefixed key names for the engine's vscode handler.
                                    let is_vscode = engine.borrow().is_vscode_mode();

                                    // Alt+key → "Alt_" encoded key for VSCode mode
                                    if is_vscode && alt && !ctrl {
                                        let alt_key_name = if shift {
                                            match key_name.as_str() {
                                                "Up"   => Some("Alt_Shift_Up"),
                                                "Down" => Some("Alt_Shift_Down"),
                                                _      => None,
                                            }
                                        } else {
                                            match key_name.as_str() {
                                                "Up"   => Some("Alt_Up"),
                                                "Down" => Some("Alt_Down"),
                                                "z"    => Some("Alt_z"),
                                                _      => None,
                                            }
                                        };
                                        if let Some(name) = alt_key_name {
                                            sender.input(Msg::KeyPress {
                                                key_name: name.to_string(),
                                                unicode: None,
                                                ctrl: false,
                                                alt: true,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                    }

                                    let effective_key = if is_vscode && shift {
                                        match key_name.as_str() {
                                            "Right"        => "Shift_Right".to_string(),
                                            "Left"         => "Shift_Left".to_string(),
                                            "Up"           => "Shift_Up".to_string(),
                                            "Down"         => "Shift_Down".to_string(),
                                            "Home"         => "Shift_Home".to_string(),
                                            "End"          => "Shift_End".to_string(),
                                            "Return" if ctrl => "Shift_Return".to_string(),
                                            "bracketleft" if ctrl  => "Shift_bracketleft".to_string(),
                                            "bracketright" if ctrl => "Shift_bracketright".to_string(),
                                            // Ctrl+Shift+letter: uppercase single-letter key names
                                            // so engine can distinguish Ctrl+L from Ctrl+Shift+L
                                            s if ctrl && s.len() == 1 => s.to_ascii_uppercase(),
                                            _              => key_name,
                                        }
                                    } else {
                                        key_name
                                    };

                                    sender.input(Msg::KeyPress { key_name: effective_key, unicode, ctrl, alt });
                                    gtk4::glib::Propagation::Stop
                                }
                            },

                            add_controller = gtk4::GestureClick {
                                set_button: 1,
                                connect_pressed[sender, drawing_area, backend_events] => move |gesture, n_press, x, y| {
                                    // Grab focus when clicking in editor
                                    drawing_area.grab_focus();

                                    let width = drawing_area.width() as f64;
                                    let height = drawing_area.height() as f64;
                                    let modifier = gesture.current_event_state();
                                    let alt = gesture
                                        .current_event()
                                        .map(|ev| ev.modifier_state().contains(gdk::ModifierType::ALT_MASK))
                                        .unwrap_or(false);

                                    // Phase B.5b Stage 1: dual-write
                                    // `UiEvent::MouseDown`. The trait
                                    // doesn't carry `n_press`, so consumers
                                    // detect double-clicks separately.
                                    backend_events.borrow_mut().push_back(
                                        events::gdk_button_to_mouse_down(1, x, y, modifier),
                                    );

                                    if modifier.contains(gdk::ModifierType::CONTROL_MASK) {
                                        sender.input(Msg::CtrlMouseClick { x, y, width, height });
                                    } else if n_press >= 2 {
                                        sender.input(Msg::MouseDoubleClick { x, y, width, height });
                                    } else {
                                        sender.input(Msg::MouseClick { x, y, width, height, alt });
                                    }
                                }
                            },

                            add_controller = gtk4::GestureDrag {
                                set_button: 1,
                                connect_drag_update[sender, drawing_area, backend_events] => move |gesture, dx, dy| {
                                    // Dead zone: ignore sub-4px movement to avoid
                                    // accidental visual mode on click jitter.
                                    if dx * dx + dy * dy < 16.0 {
                                        return;
                                    }
                                    if let Some((start_x, start_y)) = gesture.start_point() {
                                        let x = start_x + dx;
                                        let y = start_y + dy;
                                        let width = drawing_area.width() as f64;
                                        let height = drawing_area.height() as f64;

                                        // Phase B.5b Stage 1: drag updates
                                        // surface as `MouseMoved` with a
                                        // left-button-held mask. Buttons
                                        // mask matches the gesture's
                                        // configured button (1 = left).
                                        let buttons = quadraui::ButtonMask {
                                            left: true,
                                            right: false,
                                            middle: false,
                                        };
                                        backend_events.borrow_mut().push_back(
                                            events::gdk_motion_to_uievent(x, y, buttons),
                                        );

                                        sender.input(Msg::MouseDrag { x, y, width, height });
                                    }
                                },
                                connect_drag_end[sender, backend_events] => move |gesture, dx, dy| {
                                    // Phase B.5b Stage 1: dual-write
                                    // `UiEvent::MouseUp`. Reconstruct the
                                    // release coords from the gesture's
                                    // start + delta (the existing Msg
                                    // discards them but the trait carries
                                    // them through).
                                    let (rx, ry) = gesture
                                        .start_point()
                                        .map(|(sx, sy)| (sx + dx, sy + dy))
                                        .unwrap_or((0.0, 0.0));
                                    backend_events.borrow_mut().push_back(
                                        events::gdk_button_to_mouse_up(1, rx, ry),
                                    );
                                    sender.input(Msg::MouseUp);
                                },
                            },

                            add_controller = gtk4::EventControllerScroll {
                                set_flags: gtk4::EventControllerScrollFlags::VERTICAL
                                         | gtk4::EventControllerScrollFlags::HORIZONTAL,
                                connect_scroll[sender, backend_events, last_editor_pointer] => move |_, dx, dy| {
                                    // Phase B.5b Stage 1: dual-write
                                    // `UiEvent::Scroll`. Use the cached
                                    // editor-pointer position (#240) so
                                    // consumers can route the wheel event
                                    // to the window under the cursor.
                                    let (px, py) = last_editor_pointer
                                        .get()
                                        .unwrap_or((0.0, 0.0));
                                    backend_events.borrow_mut().push_back(
                                        events::gdk_scroll_to_uievent(dx, dy, px, py),
                                    );

                                    sender.input(Msg::MouseScroll { delta_x: dx, delta_y: dy });
                                    gtk4::glib::Propagation::Stop
                                },
                            },

                            // #240: track the editor pointer so the scroll
                            // handler can route wheel events to the window
                            // under the cursor (across editor groups), not
                            // just the active one.
                            add_controller = gtk4::EventControllerMotion {
                                connect_motion[last_editor_pointer] => move |_, x, y| {
                                    last_editor_pointer.set(Some((x, y)));
                                },
                                connect_leave[last_editor_pointer] => move |_| {
                                    last_editor_pointer.set(None);
                                },
                            },

                            #[watch]
                            set_css_classes: {
                                // Only queue a draw when explicitly requested by update().
                                // Using take() clears the flag atomically so it fires once per request.
                                if model.draw_needed.take() {
                                    drawing_area.queue_draw();
                                    menu_bar_da.queue_draw();
                                }
                                // Return static classes — no even/odd alternation — so GTK
                                // skips CSS re-resolution when classes haven't changed.
                                // This eliminates expensive CSS thrashing on every update().
                                &["vim-code"]
                            },
                        },

                        // Find/Replace is now engine-level (drawn by Cairo in draw.rs)
                    }
                }
                }  // close main_hbox
            }  // close outer gtk4::Box
            }  // close window_overlay (gtk4::Overlay)
        }
    }

    fn init(
        file_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Dark/light preference is set after engine init, once we know the colorscheme.

        // Ensure GTK finds our installed SVG icon by adding
        // ~/.local/share/icons to the icon theme search path.
        if let Some(home) = std::env::var_os("HOME") {
            let icon_dir = std::path::PathBuf::from(home).join(".local/share/icons");
            if let Some(display) = gdk::Display::default() {
                let icon_theme = gtk4::IconTheme::for_display(&display);
                icon_theme.add_search_path(&icon_dir);
            }
        }

        // Install bundled Nerd Font icon subset so UI glyphs render without
        // requiring the user to install a Nerd Font system-wide.
        install_bundled_icon_font();

        let engine = {
            let mut e = Engine::new();
            icons::set_nerd_fonts(e.settings.use_nerd_fonts);
            e.startup(file_path.as_deref());
            e
        };

        // Load CSS after engine so we can read the saved colorscheme setting.
        let initial_theme = Theme::from_name(&engine.settings.colorscheme);
        let css_provider = load_css(&initial_theme);
        let last_colorscheme = engine.settings.colorscheme.clone();

        // Set GTK dark/light preference based on the active colorscheme.
        if let Some(gtk_settings) = gtk4::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(!initial_theme.is_light());
        }

        // On X11 use x11_bin (xclip/xsel subprocesses) explicitly: try_context() picks
        // x11_fork first, whose get_contents() uses X11ClipboardContext directly and
        // competes with GTK's X11 event loop.  Subprocess reads open their own X11
        // connection per call and have no such conflict.
        let clipboard: Option<Box<dyn ClipboardProviderExt>> = {
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

        // Set window title based on file
        let title = match engine.file_path() {
            Some(p) => format!("VimCode - {}", p.display()),
            None => "VimCode - [No Name]".to_string(),
        };

        let engine = Rc::new(RefCell::new(engine));

        // Register engine pointer for emergency swap flush from the panic hook.
        // SAFETY: The Rc<RefCell<Engine>> lives for the GTK app's lifetime.
        // The pointer is only dereferenced during panic recovery on the main thread.
        unsafe {
            crate::core::swap::register_emergency_engine(
                engine.as_ptr() as *const crate::core::Engine
            );
        }

        let explorer_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let activity_bar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let activity_bar_hits: Rc<RefCell<Vec<quadraui::ActivityBarRowHit>>> =
            Rc::new(RefCell::new(Vec::new()));
        let activity_bar_hover: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

        let explorer_row_height_cell: Rc<Cell<f64>> = Rc::new(Cell::new(28.0));
        let explorer_scroll_accum: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        #[allow(clippy::type_complexity)]
        let explorer_scrollbar_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> =
            Rc::new(Cell::new(None));
        let active_ctx_popover_ref: Rc<RefCell<Option<gtk4::PopoverMenu>>> =
            Rc::new(RefCell::new(None));
        let drawing_area_ref = Rc::new(RefCell::new(None));
        // Editor pointer cache (#240): updated by EventControllerMotion on
        // the editor DA, read by the scroll handler to route wheel events
        // to the window under the cursor across editor groups.
        let last_editor_pointer: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
        let menu_bar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> = Rc::new(RefCell::new(None));
        let menu_dropdown_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let panel_hover_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        #[allow(clippy::type_complexity)]
        let panel_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String, bool)>>> =
            Rc::new(RefCell::new(Vec::new()));
        #[allow(clippy::type_complexity)]
        let panel_hover_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> =
            Rc::new(Cell::new(None));
        #[allow(clippy::type_complexity)]
        let editor_hover_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> =
            Rc::new(Cell::new(None));
        #[allow(clippy::type_complexity)]
        let completion_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> =
            Rc::new(Cell::new(None));
        #[allow(clippy::type_complexity)]
        let tab_switcher_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> =
            Rc::new(Cell::new(None));
        #[allow(clippy::type_complexity)]
        let dialog_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> = Rc::new(Cell::new(None));
        #[allow(clippy::type_complexity)]
        let editor_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let editor_hover_scrollbar: Rc<Cell<Option<render::PopupScrollbarHit>>> =
            Rc::new(Cell::new(None));
        let menu_dd_lh: Rc<Cell<f64>> = Rc::new(Cell::new(24.0));
        let debug_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let debug_sidebar_lh: Rc<Cell<f64>> = Rc::new(Cell::new(20.0));
        let git_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let overlay_ref = Rc::new(RefCell::new(None));
        let window_scrollbars_ref = Rc::new(RefCell::new(HashMap::new()));
        let line_height_cell: Rc<Cell<f64>> = Rc::new(Cell::new(24.0));
        let char_width_cell: Rc<Cell<f64>> = Rc::new(Cell::new(9.0));
        // Last font metrics sent via CacheFontMetrics — avoids sending on every draw.
        let last_metrics_cell: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
        // Current mouse position written directly from the motion callback — avoids routing
        // every motion event through the Relm4 message loop (which fires at 100-200 Hz).
        // (-1.0, -1.0) means the pointer is outside the drawing area.
        let mouse_pos_cell: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((-1.0, -1.0)));
        // Shared state for Cairo h scrollbar hover/drag — read by set_draw_func closure.
        let h_sb_hovered_cell: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let tab_close_hover_cell: Rc<Cell<Option<(usize, usize)>>> = Rc::new(Cell::new(None));
        let h_sb_drag_cell: Rc<Cell<Option<core::WindowId>>> = Rc::new(Cell::new(None));
        let tab_slot_positions_cell: Rc<RefCell<TabSlotMap>> =
            Rc::new(RefCell::new(HashMap::new()));
        let tab_close_bounds_cell: Rc<RefCell<TabCloseMap>> = Rc::new(RefCell::new(HashMap::new()));
        let diff_btn_map_cell: Rc<RefCell<DiffBtnMap>> = Rc::new(RefCell::new(HashMap::new()));
        let split_btn_map_cell: Rc<RefCell<SplitBtnMap>> = Rc::new(RefCell::new(HashMap::new()));
        let action_btn_map_cell: Rc<RefCell<ActionBtnMap>> = Rc::new(RefCell::new(HashMap::new()));
        let status_segment_map_cell: Rc<RefCell<StatusSegmentMap>> =
            Rc::new(RefCell::new(HashMap::new()));
        let tab_visible_counts_cell: Rc<
            RefCell<Vec<(crate::core::window::GroupId, usize, usize)>>,
        > = Rc::new(RefCell::new(Vec::new()));
        let command_center_layout_cell: Rc<RefCell<Option<quadraui::CommandCenterLayout>>> =
            Rc::new(RefCell::new(None));
        let sidebar_inner_sw_ref: Rc<RefCell<Option<gtk4::ScrolledWindow>>> =
            Rc::new(RefCell::new(None));
        let sidebar_revealer_ref: Rc<RefCell<Option<gtk4::Revealer>>> = Rc::new(RefCell::new(None));
        // Saves the sidebar width at the start of a drag so we can compute
        // initial_width + total_offset instead of accumulating delta per event.
        let sidebar_drag_start_w: Rc<Cell<i32>> = Rc::new(Cell::new(300));
        let explorer_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let search_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let debug_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let git_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let ext_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let ext_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let ext_dyn_panel_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let ext_dyn_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let settings_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let settings_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> = Rc::new(RefCell::new(None));
        let ai_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let ai_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> = Rc::new(RefCell::new(None));

        // Set up file watcher for settings.json
        let settings_path = std::env::var("HOME")
            .map(|h| format!("{}/.config/vimcode/settings.json", h))
            .unwrap_or_else(|_| ".config/vimcode/settings.json".to_string());

        let file = gio::File::for_path(&settings_path);
        let settings_monitor =
            match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
                Ok(monitor) => {
                    let sender_for_monitor = sender.input_sender().clone();
                    monitor.connect_changed(move |_, _, _, event| {
                        // ChangesDoneHint fires once after the file is fully written and
                        // closed (IN_CLOSE_WRITE on Linux/inotify).  This is the most
                        // reliable single event per save.  We do NOT also listen for
                        // Changed (IN_MODIFY) to avoid processing two events per VimCode
                        // save — the self-save guard in SettingsFileChanged handles any
                        // stray duplicates anyway.
                        if event == gio::FileMonitorEvent::ChangesDoneHint {
                            sender_for_monitor.send(Msg::SettingsFileChanged).ok();
                        }
                    });
                    Some(monitor)
                }
                Err(_) => None,
            };

        // Initialize sidebar visibility from session state or settings
        let sidebar_visible = {
            let eng = engine.borrow();
            eng.session.explorer_visible || eng.settings.explorer_visible_on_startup
        };

        // Phase B.5 Stage 1: build the `quadraui::Backend` impl now,
        // before constructing the App. Both the App's modal_stack /
        // drag_state alias fields and the App's `backend` field share
        // the same underlying `Rc<RefCell<>>`s — B.5b Stage 11 migrates
        // the alias call sites onto `backend.borrow().*_handle()` and
        // drops the duplicates.
        //
        // Panel-key accelerators are registered here (re-runs on each
        // settings reload via `register_panel_accelerators`). The
        // editor key handler dispatches matches through
        // `dispatch_gtk_panel_accelerator` — see B.5b Stage 2.
        let mut gtk_backend = backend::GtkBackend::new();
        register_panel_accelerators(&mut gtk_backend, &engine.borrow().settings.panel_keys);
        // #270 lift: GtkBackend no longer reads the `crate::icons`
        // global atomic or `UI_FONT()` macro internally (those are
        // vimcode-private). Sync the values onto the backend instead.
        // Re-synced per-frame in the `CacheFontMetrics` handler below
        // so runtime toggles (`:set nonerdfonts`, `:set guifont=...`)
        // propagate.
        {
            let e = engine.borrow();
            gtk_backend.set_nerd_fonts(e.settings.use_nerd_fonts);
            gtk_backend.set_ui_font(format!(
                "{} {}",
                UI_FONT_FAMILY,
                e.settings.ui_font_size.max(1)
            ));
        }
        // Phase B.5b Stage 1: shared event-queue handle. Producer-side
        // signal callbacks (key/mouse/scroll on the editor DA) push
        // translated `UiEvent`s into this `RefCell<VecDeque>`; the drain
        // hook installed below polls and discards. Dual-write today —
        // Relm4 `Msg` flow remains authoritative, the queue just proves
        // producers + consumer are wired so subsequent stages can route
        // dispatch off it.
        let backend_events = gtk_backend.events_handle();
        let backend = Rc::new(RefCell::new(gtk_backend));

        let model = App {
            engine: engine.clone(),
            sidebar_visible,
            window: root.clone(),
            active_panel: SidebarPanel::Explorer,
            explorer_sidebar_da_ref: explorer_sidebar_da_ref.clone(),
            activity_bar_da_ref: activity_bar_da_ref.clone(),
            explorer_row_height_cell: explorer_row_height_cell.clone(),
            explorer_scroll_accum: explorer_scroll_accum.clone(),
            explorer_scrollbar_rect: explorer_scrollbar_rect.clone(),
            drawing_area: drawing_area_ref.clone(),
            menu_bar_da: menu_bar_da_ref.clone(),
            debug_sidebar_da_ref: debug_sidebar_da_ref.clone(),
            debug_sidebar_lh: debug_sidebar_lh.clone(),
            git_sidebar_da_ref: git_sidebar_da_ref.clone(),
            ext_sidebar_da_ref: ext_sidebar_da_ref.clone(),
            ai_sidebar_da_ref: ai_sidebar_da_ref.clone(),
            window_scrollbars: window_scrollbars_ref.clone(),
            overlay: overlay_ref.clone(),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
            last_editor_pointer: last_editor_pointer.clone(),
            cached_ui_line_height: 20.0,
            dialog_btn_rects: Rc::new(RefCell::new(Vec::new())),
            line_height_cell: line_height_cell.clone(),
            char_width_cell: char_width_cell.clone(),
            draw_needed: Rc::new(Cell::new(false)),
            mouse_pos_cell: mouse_pos_cell.clone(),
            h_sb_hovered_cell: h_sb_hovered_cell.clone(),
            tab_close_hover_cell: tab_close_hover_cell.clone(),
            h_sb_drag_cell: h_sb_drag_cell.clone(),
            fr_input_dragging: false,
            settings_monitor,
            sender: sender.input_sender().clone(),
            sidebar_inner_sw: sidebar_inner_sw_ref.clone(),
            sidebar_revealer: sidebar_revealer_ref.clone(),
            explorer_panel_box: explorer_panel_box_ref.clone(),
            search_sidebar_da_ref: search_sidebar_da_ref.clone(),
            debug_panel_box: debug_panel_box_ref.clone(),
            git_panel_box: git_panel_box_ref.clone(),
            ext_panel_box: ext_panel_box_ref.clone(),
            ext_dyn_panel_da_ref: ext_dyn_panel_da_ref.clone(),
            ext_dyn_panel_box: ext_dyn_panel_box_ref.clone(),
            settings_panel_box: settings_panel_box_ref.clone(),
            settings_da_ref: settings_da_ref.clone(),
            ai_panel_box_ref: ai_panel_box_ref.clone(),
            last_clipboard_content: None,
            clipboard,
            h_sb_hovered: false,
            tab_close_hover: None,
            tab_slot_positions: tab_slot_positions_cell.clone(),
            tab_close_bounds: tab_close_bounds_cell.clone(),
            diff_btn_map: diff_btn_map_cell.clone(),
            split_btn_map: split_btn_map_cell.clone(),
            action_btn_map: action_btn_map_cell.clone(),
            status_segment_map: status_segment_map_cell.clone(),
            cached_screen_layout: Rc::new(RefCell::new(None)),
            debug_toolbar_layout: Rc::new(RefCell::new(None)),
            debug_toolbar_y_offset: Rc::new(Cell::new(0.0)),
            debug_toolbar_height: Rc::new(Cell::new(0.0)),
            debug_toolbar_hovered_id: Rc::new(RefCell::new(None)),
            debug_toolbar_pressed_id: Rc::new(RefCell::new(None)),
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            group_divider_dragging: None,
            tab_dragging: false,
            tab_drag_start: None,
            last_sc_refresh: std::time::Instant::now(),
            last_tree_indicator_update: std::time::Instant::now(),
            menu_dropdown_da: menu_dropdown_da_ref.clone(),
            panel_hover_da: panel_hover_da_ref.clone(),
            panel_hover_link_rects: panel_hover_link_rects.clone(),
            panel_hover_popup_rect: panel_hover_popup_rect.clone(),
            editor_hover_popup_rect: editor_hover_popup_rect.clone(),
            completion_popup_rect: completion_popup_rect.clone(),
            tab_switcher_popup_rect: tab_switcher_popup_rect.clone(),
            dialog_popup_rect: dialog_popup_rect.clone(),
            editor_hover_link_rects: editor_hover_link_rects.clone(),
            editor_hover_scrollbar: editor_hover_scrollbar.clone(),
            menu_dd_line_height: menu_dd_lh.clone(),
            css_provider,
            last_colorscheme,
            active_ctx_popover: active_ctx_popover_ref.clone(),
            backend: backend.clone(),
        };
        let widgets = view_output!();

        // Store widget references
        *explorer_sidebar_da_ref.borrow_mut() = Some(widgets.explorer_da.clone());
        *drawing_area_ref.borrow_mut() = Some(widgets.drawing_area.clone());
        *menu_bar_da_ref.borrow_mut() = Some(widgets.menu_bar_da.clone());
        *overlay_ref.borrow_mut() = Some(widgets.editor_overlay.clone());
        *sidebar_inner_sw_ref.borrow_mut() = Some(widgets.sidebar_inner_sw.clone());
        *sidebar_revealer_ref.borrow_mut() = Some(widgets.sidebar_revealer.clone());
        *explorer_panel_box_ref.borrow_mut() = Some(widgets.explorer_panel.clone());
        *search_sidebar_da_ref.borrow_mut() = Some(widgets.search_sidebar_da.clone());
        *debug_panel_box_ref.borrow_mut() = Some(widgets.debug_panel.clone());
        *git_panel_box_ref.borrow_mut() = Some(widgets.git_panel.clone());
        *ext_panel_box_ref.borrow_mut() = Some(widgets.ext_panel.clone());
        *ext_dyn_panel_box_ref.borrow_mut() = Some(widgets.ext_dyn_panel.clone());
        *settings_panel_box_ref.borrow_mut() = Some(widgets.settings_panel.clone());
        *ai_panel_box_ref.borrow_mut() = Some(widgets.ai_panel_box.clone());
        // ── Search sidebar DrawingArea setup ──────────────────────────────
        {
            let pango_ctx = widgets.search_sidebar_da.pango_context();
            let font_desc = pango::FontDescription::from_string(&draw::UI_FONT());
            pango_ctx.set_font_description(Some(&font_desc));
            let metrics = pango_ctx.metrics(Some(&font_desc), None);
            let lh = (metrics.ascent() + metrics.descent()) as f64 / pango::SCALE as f64;
            let cw = metrics.approximate_char_width() as f64 / pango::SCALE as f64;
            let mut b = backend.borrow_mut();
            b.set_pango_context(pango_ctx);
            b.set_current_line_height(lh);
            b.set_current_char_width(cw);
            {
                use quadraui::Backend;
                b.begin_frame(quadraui::Viewport::new(
                    root.width().max(800) as f32,
                    root.height().max(600) as f32,
                    1.0,
                ));
            }
        }
        {
            let engine = engine.clone();
            let backend_d = backend.clone();
            widgets
                .search_sidebar_da
                .set_draw_func(move |da, cr, _w, _h| {
                    let engine = engine.borrow();
                    let theme = Theme::from_name(&engine.settings.colorscheme);
                    let q_theme = crate::gtk::quadraui_gtk::q_theme(&theme);
                    render::populate_search_sidebar_system(&engine, &engine.cwd);
                    let w = da.width() as f64;
                    let h = da.height() as f64;
                    let area = quadraui::Rect::new(0.0, 0.0, w as f32, h as f32);
                    engine.search_sidebar_body_rect.set(area);
                    let pango_ctx = pangocairo::create_context(cr);
                    let font_desc = pango::FontDescription::from_string(&draw::UI_FONT());
                    let pango_layout = pango::Layout::new(&pango_ctx);
                    pango_layout.set_font_description(Some(&font_desc));
                    pango_layout.set_text("Xy");
                    let line_height = pango_layout.pixel_size().1 as f64;
                    pango_layout.set_text("M");
                    let char_width = pango_layout.pixel_size().0 as f64;
                    backend_d
                        .borrow_mut()
                        .enter_frame_scope(cr, &pango_layout, |b| {
                            b.set_current_theme(q_theme);
                            b.set_current_line_height(line_height);
                            b.set_current_char_width(char_width);
                            engine.search_sidebar_system.borrow().render(b, area);
                        });
                });
        }
        {
            let sender_ev = sender.input_sender().clone();
            quadraui::gtk::wire_da_events(&widgets.search_sidebar_da, move |ev| {
                sender_ev.send(Msg::SearchSidebarEvent(ev)).ok();
            });
        }

        // ── Settings sidebar (Phase A.3c-2: native widgets → DrawingArea) ──────
        {
            let engine_d = engine.clone();
            let backend_d = backend.clone();
            widgets.settings_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine_d.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(&UI_FONT());
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_settings_panel(
                    cr,
                    &layout,
                    &engine,
                    &theme,
                    &backend_d,
                    0.0,
                    0.0,
                    w,
                    h,
                    line_height,
                );
            });
        }
        {
            let sender_set = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let unicode = key.to_unicode().filter(|c| !c.is_control());
                let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                sender_set
                    .send(Msg::SettingsKey(key_name, ctrl, unicode))
                    .ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.settings_da.set_focusable(true);
            widgets.settings_da.add_controller(key_ctrl);
        }
        {
            let sender_set = sender.input_sender().clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, n_press, x, y| {
                sender_set.send(Msg::SettingsClick(x, y, n_press)).ok();
            });
            widgets.settings_da.add_controller(gesture);
        }
        {
            let sender_set = sender.input_sender().clone();
            let scroll_ctrl =
                gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
            scroll_ctrl.connect_scroll(move |_, _dx, dy| {
                sender_set.send(Msg::SettingsScroll(dy)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.settings_da.add_controller(scroll_ctrl);
        }
        {
            let engine_drag = engine.clone();
            let sender_drag = sender.input_sender().clone();
            let settings_da_drag = model.settings_da_ref.clone();
            let gesture = gtk4::GestureDrag::new();
            gesture.set_button(1);
            gesture.connect_drag_update(move |g, dx, dy| {
                let (sx, sy) = g.start_point().unwrap_or((0.0, 0.0));
                let pt = quadraui::Point::new((sx + dx) as f32, (sy + dy) as f32);
                let da_h = settings_da_drag
                    .borrow()
                    .as_ref()
                    .map(|da| da.height() as f32)
                    .unwrap_or(0.0);
                let event = quadraui::UiEvent::MouseMoved {
                    position: pt,
                    buttons: quadraui::ButtonMask {
                        left: true,
                        middle: false,
                        right: false,
                    },
                };
                let q_rect = quadraui::Rect::new(0.0, 0.0, 1.0, da_h);
                let eng = engine_drag.borrow();
                render::populate_settings_form_controller(&eng);
                let result = eng
                    .settings_form_controller
                    .borrow_mut()
                    .handle_cached(&event, q_rect);
                if matches!(
                    result,
                    quadraui::FormControllerEvent::ScrollChanged
                        | quadraui::FormControllerEvent::Consumed
                ) {
                    let new_offset = eng.settings_form_controller.borrow().scroll_offset();
                    drop(eng);
                    engine_drag.borrow_mut().settings_scroll_top = new_offset;
                    sender_drag.send(Msg::SettingsScroll(0.0)).ok();
                }
            });
            widgets.settings_da.add_controller(gesture);
        }
        *settings_da_ref.borrow_mut() = Some(widgets.settings_da.clone());

        // ── Explorer sidebar — TreeController render ─────────────────────────
        {
            let engine_d = engine.clone();
            let row_h_cell = explorer_row_height_cell.clone();
            let sb_rect_cell = explorer_scrollbar_rect.clone();
            let backend_d = backend.clone();
            widgets.explorer_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine_d.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(&UI_FONT());
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let row_h = (line_height * 1.4).round().max(1.0);
                row_h_cell.set(row_h);
                let w = da.width() as f64;
                let h = da.height() as f64;

                let item_height = row_h;
                let visible_rows = if item_height > 0.0 {
                    (h / item_height).floor() as usize
                } else {
                    0
                };
                engine.explorer_viewport_rows.set(visible_rows);
                let q_rect = quadraui::Rect::new(0.0, 0.0, w as f32, h as f32);
                engine.explorer_tree_rect.set(q_rect);

                crate::render::populate_explorer_tree_controller(&engine, &theme);

                let total = engine.explorer_rows.len();
                let need_sb = visible_rows > 0 && total > visible_rows;
                let sb_w_px = if need_sb { 8.0 } else { 0.0 };
                let tree_w = (w - sb_w_px).max(0.0);
                let tree_rect = quadraui::Rect::new(0.0, 0.0, tree_w as f32, h as f32);

                backend_d.borrow_mut().enter_frame_scope(cr, &layout, |b| {
                    b.set_current_theme(crate::gtk::quadraui_gtk::q_theme(&theme));
                    b.set_current_line_height(line_height);
                    engine.explorer_tree.borrow().render(b, tree_rect);
                });

                if need_sb {
                    let sb_x = tree_w;
                    let scroll_top = engine.explorer_tree.borrow().scroll_offset();
                    let track_len = h;
                    let thumb_len = (track_len * visible_rows as f64 / total as f64).max(8.0);
                    let max_scroll = total.saturating_sub(visible_rows) as f64;
                    let scroll_ratio = if max_scroll > 0.0 {
                        scroll_top as f64 / max_scroll
                    } else {
                        0.0
                    };
                    let thumb_y = scroll_ratio * (track_len - thumb_len);
                    let (bg_r, bg_g, bg_b) = theme.tab_bar_bg.to_cairo();
                    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
                    cr.set_source_rgb(bg_r, bg_g, bg_b);
                    cr.rectangle(sb_x, 0.0, sb_w_px, track_len);
                    cr.fill().ok();
                    cr.set_source_rgb(dim_r, dim_g, dim_b);
                    cr.rectangle(sb_x + 2.0, thumb_y, sb_w_px - 4.0, thumb_len);
                    cr.fill().ok();
                    sb_rect_cell.set(Some((sb_x, 0.0, sb_w_px, track_len)));
                } else {
                    sb_rect_cell.set(None);
                }
            });
        }
        {
            let sender_ex = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let unicode = key.to_unicode().filter(|c| !c.is_control());
                let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                sender_ex
                    .send(Msg::ExplorerKey {
                        key_name,
                        unicode,
                        ctrl,
                    })
                    .ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.explorer_da.add_controller(key_ctrl);
        }
        {
            let sender_ex = sender.input_sender().clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, n_press, x, y| {
                sender_ex.send(Msg::ExplorerClick { x, y, n_press }).ok();
            });
            widgets.explorer_da.add_controller(gesture);
        }
        {
            let sender_ex = sender.input_sender().clone();
            let right_click = gtk4::GestureClick::new();
            right_click.set_button(3);
            right_click.connect_pressed(move |_, _n_press, x, y| {
                sender_ex.send(Msg::ExplorerRightClick { x, y }).ok();
            });
            widgets.explorer_da.add_controller(right_click);
        }
        {
            let sender_ex = sender.input_sender().clone();
            let scroll_ctrl =
                gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
            scroll_ctrl.connect_scroll(move |_, _dx, dy| {
                sender_ex.send(Msg::ExplorerScroll(dy)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.explorer_da.add_controller(scroll_ctrl);
        }
        // Scrollbar thumb drag: a dedicated `GestureDrag` on the explorer
        // DA watches for drags that start inside the scrollbar track and
        // translates vertical motion into `scroll_top` updates. Claiming
        // the gesture on begin prevents the ancestor sidebar-resize
        // `GestureDrag` on `main_hbox` from interpreting a thumb drag as
        // a panel-width resize (the scrollbar lives right at the sidebar
        // edge, so the ambiguity is real).
        //
        // Drag math goes through `quadraui::dispatch_mouse_drag` (same code
        // path TUI uses) — this gives correct thumb-length compensation
        // (#204) so the thumb tracks 1:1 with the mouse instead of feeling
        // sluggish, and makes the picker / sidebar / palette scrollbars all
        // share one math implementation. Click-on-track jump-scroll happens
        // here in `connect_drag_begin` rather than in the click handler so
        // it fires reliably even when GTK claims the drag sequence and
        // suppresses the click (#199).
        {
            let sb_rect_begin = explorer_scrollbar_rect.clone();
            let engine_begin = engine.clone();
            let row_h_begin = explorer_row_height_cell.clone();
            let da_begin = widgets.explorer_da.clone();
            let drag_state_begin = model.backend.borrow().drag_state_handle();

            let engine_update = engine.clone();
            let da_update = widgets.explorer_da.clone();
            let drag_state_update = model.backend.borrow().drag_state_handle();

            let drag_state_end = model.backend.borrow().drag_state_handle();
            let da_end = widgets.explorer_da.clone();

            let gesture = gtk4::GestureDrag::new();
            gesture.set_button(1);
            gesture.connect_drag_begin(move |g, x_start, y_start| {
                let Some((sb_x, sb_y, sb_w, sb_h)) = sb_rect_begin.get() else {
                    return;
                };
                if x_start < sb_x || x_start > sb_x + sb_w {
                    return;
                }
                g.set_state(gtk4::EventSequenceState::Claimed);

                let eng = engine_begin.borrow();
                let total = eng.explorer_rows.len();
                let item_h = row_h_begin.get().max(1.0);
                let viewport = (da_begin.height() as f64 / item_h).floor().max(0.0) as usize;
                let max_scroll = total.saturating_sub(viewport);
                if max_scroll == 0 || sb_h <= 0.0 {
                    return;
                }

                let thumb_ratio = (viewport as f32 / total as f32).min(1.0);
                let thumb_length = (sb_h as f32 * thumb_ratio).max(1.0);
                let effective_track = (sb_h as f32 - thumb_length).max(1.0);
                let scroll_top = eng.explorer_tree.borrow().scroll_offset();
                let scroll_ratio = if max_scroll == 0 {
                    0.0
                } else {
                    (scroll_top as f32 / max_scroll as f32).clamp(0.0, 1.0)
                };
                let thumb_top = sb_y as f32 + scroll_ratio * effective_track;
                let dy = y_start as f32 - thumb_top;
                let grab_offset = if dy >= 0.0 && dy < thumb_length {
                    dy
                } else {
                    0.0
                };
                drop(eng);

                drag_state_begin
                    .borrow_mut()
                    .begin(quadraui::DragTarget::ScrollbarY {
                        widget: quadraui::WidgetId::new("explorer:sb"),
                        track_start: sb_y as f32,
                        track_length: sb_h as f32,
                        thumb_length: (sb_h as f32 * viewport as f32 / total.max(1) as f32)
                            .max(1.0),
                        max_scroll: total.saturating_sub(viewport),
                        grab_offset,
                        inverted: false,
                    });

                let events = quadraui::dispatch_mouse_drag(
                    &drag_state_begin.borrow(),
                    quadraui::Point {
                        x: x_start as f32,
                        y: y_start as f32,
                    },
                    Default::default(),
                );
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { new_offset, .. } = ev {
                        engine_begin
                            .borrow()
                            .explorer_tree
                            .borrow_mut()
                            .set_scroll_offset(*new_offset);
                    }
                }

                da_begin.queue_draw();
            });
            gesture.connect_drag_update(move |g, dx, dy| {
                let Some((start_x, start_y)) = g.start_point() else {
                    return;
                };
                let drag = drag_state_update.borrow();
                if !drag.is_active() {
                    return;
                }
                let events = quadraui::dispatch_mouse_drag(
                    &drag,
                    quadraui::Point {
                        x: (start_x + dx) as f32,
                        y: (start_y + dy) as f32,
                    },
                    Default::default(),
                );
                drop(drag);
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                        if widget.as_str() == "explorer:sb" {
                            engine_update
                                .borrow()
                                .explorer_tree
                                .borrow_mut()
                                .set_scroll_offset(*new_offset);
                            da_update.queue_draw();
                        }
                    }
                }
            });
            gesture.connect_drag_end(move |_, _, _| {
                drag_state_end.borrow_mut().end();
                da_end.queue_draw();
            });
            widgets.explorer_da.add_controller(gesture);
        }

        // Drag-and-drop from the explorer was part of the native
        // `gtk4::TreeView` setup. DnD is deferred — tracked as
        // https://github.com/JDonaghy/vimcode/issues/149.

        // ── Sidebar resize drag handle ─────────────────────────────────────────
        // Attach the GestureDrag to main_hbox (which never moves during a sidebar
        // resize) rather than to the 6-px handle strip itself.  When the handle
        // strip is a child of a reflowing layout, GTK4 may cancel the gesture as
        // soon as the widget allocation changes (premature drag-end / jitter).
        // We gate on the x-position in drag_begin so that only clicks near the
        // sidebar/editor boundary are treated as a sidebar resize.
        {
            let is_sb_drag: Rc<Cell<bool>> = Rc::new(Cell::new(false));
            let is_sb_drag_begin = is_sb_drag.clone();
            let is_sb_drag_update = is_sb_drag.clone();
            let is_sb_drag_end = is_sb_drag.clone();

            let gesture = gtk4::GestureDrag::new();

            let sb_ref = sidebar_inner_sw_ref.clone();
            let sw = sidebar_drag_start_w.clone();
            gesture.connect_drag_begin(move |_, x, _| {
                let Some(ref sb) = *sb_ref.borrow() else {
                    is_sb_drag_begin.set(false);
                    return;
                };
                if !sb.is_visible() {
                    is_sb_drag_begin.set(false);
                    return;
                }
                // The resize handle strip sits immediately to the right of
                // the sidebar. Accept clicks only from the sidebar's right
                // edge outward, so drags that start inside the sidebar
                // (including on the explorer scrollbar which is flush with
                // the right edge) aren't stolen as panel-resize drags.
                const ACTIVITY_W: f64 = 48.0;
                let aw = sb.allocated_width();
                let sidebar_right = ACTIVITY_W + aw as f64;
                if x >= sidebar_right && x <= sidebar_right + 10.0 {
                    is_sb_drag_begin.set(true);
                    sw.set(sb.width_request());
                } else {
                    is_sb_drag_begin.set(false);
                }
            });

            let sb_ref2 = sidebar_inner_sw_ref.clone();
            let sw2 = sidebar_drag_start_w.clone();
            gesture.connect_drag_update(move |_, dx, _| {
                if !is_sb_drag_update.get() {
                    return;
                }
                let new_w = (sw2.get() as f64 + dx).round() as i32;
                if let Some(ref sb) = *sb_ref2.borrow() {
                    sb.set_width_request(new_w.clamp(80, 600));
                }
            });

            let sender_resize = sender.input_sender().clone();
            gesture.connect_drag_end(move |_, _, _| {
                if !is_sb_drag_end.get() {
                    return;
                }
                is_sb_drag_end.set(false);
                sender_resize.send(Msg::SidebarResized).ok();
            });

            widgets.main_hbox.add_controller(gesture);
        }

        // Shared bar_rect — set by the menu bar DA's draw_func each frame,
        // read by all menu click/motion/key handlers for MenuSystem::handle().
        let menu_bar_rect_cell: Rc<Cell<quadraui::Rect>> =
            Rc::new(Cell::new(quadraui::Rect::new(0.0, 0.0, 800.0, 24.0)));

        // ── Menu dropdown overlay — quadraui::gtk::MenuOverlay ────────────────
        {
            let menu_overlay = quadraui::gtk::MenuOverlay::new();
            let menu_system_rc = engine.borrow().menu_system.clone();
            menu_overlay.connect(
                menu_system_rc,
                backend.clone(),
                menu_bar_rect_cell.clone(),
                &UI_FONT(),
                {
                    let sender = sender.input_sender().clone();
                    move |ev| match ev {
                        quadraui::MenuEvent::Activated(id) => {
                            sender
                                .send(Msg::HandleMenuAction(id.as_str().to_string()))
                                .ok();
                        }
                        quadraui::MenuEvent::Ignored => {}
                        _ => {
                            sender.send(Msg::MenuRedraw).ok();
                        }
                    }
                },
            );
            widgets
                .window_overlay
                .add_overlay(menu_overlay.drawing_area());
            *menu_dropdown_da_ref.borrow_mut() = Some(menu_overlay.drawing_area().clone());
        }

        // ── Panel hover popup overlay DrawingArea ────────────────────────────
        // A full-window transparent overlay that draws the panel hover popup
        // to the right of the sidebar (extending into the editor area).
        {
            let hover_da = gtk4::DrawingArea::new();
            hover_da.set_hexpand(true);
            hover_da.set_vexpand(true);
            hover_da.set_can_target(false); // pass-through until popup has links

            {
                let engine = engine.clone();
                let lh = menu_dd_lh.clone();
                let link_rects = panel_hover_link_rects.clone();
                let popup_rect = panel_hover_popup_rect.clone();
                hover_da.set_draw_func(move |da, cr, _w, _h| {
                    link_rects.borrow_mut().clear();
                    popup_rect.set(None);
                    let engine = engine.borrow();
                    if engine.panel_hover.is_none() {
                        return;
                    }
                    let theme = Theme::from_name(&engine.settings.colorscheme);
                    let font_desc = FontDescription::from_string(&UI_FONT());
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    lh.set(line_height);
                    let char_width = {
                        layout.set_text("0");
                        layout.pixel_size().0 as f64
                    };
                    let screen =
                        build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                    let window_w = da.width() as f64;
                    let window_h = da.height() as f64;
                    let sidebar_right = 48.0 + engine.session.sidebar_width as f64;
                    let is_native = engine
                        .panel_hover
                        .as_ref()
                        .map(|ph| ph.is_native())
                        .unwrap_or(false);
                    let (rects, bounds) = draw_panel_hover_popup(
                        cr,
                        &layout,
                        &screen,
                        &theme,
                        sidebar_right,
                        0.0,
                        window_w,
                        window_h,
                        line_height,
                        is_native,
                    );
                    *link_rects.borrow_mut() = rects;
                    popup_rect.set(bounds);
                });
            }

            widgets.window_overlay.add_overlay(&hover_da);
            *panel_hover_da_ref.borrow_mut() = Some(hover_da);

            // Capture-phase click on the window overlay: intercept clicks on
            // popup links before they reach child widgets.
            {
                let sender_hover = sender.input_sender().clone();
                let popup_rect_click = panel_hover_popup_rect.clone();
                let gesture = gtk4::GestureClick::new();
                gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
                gesture.connect_pressed(move |gesture, _n_press, x, y| {
                    if let Some((px, py, pw, ph)) = popup_rect_click.get() {
                        if x >= px && x <= px + pw && y >= py && y <= py + ph {
                            sender_hover.send(Msg::PanelHoverClick(x, y)).ok();
                            gesture.set_state(gtk4::EventSequenceState::Claimed);
                        }
                    }
                });
                widgets.window_overlay.add_controller(gesture);
            }

            // Capture-phase motion on the window overlay: cancel dismiss when
            // the mouse is over the popup area.
            {
                let engine_motion = engine.clone();
                let popup_rect_motion = panel_hover_popup_rect.clone();
                let motion = gtk4::EventControllerMotion::new();
                motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
                motion.connect_motion(move |_, x, y| {
                    if let Some((px, py, pw, ph)) = popup_rect_motion.get() {
                        if x >= px && x <= px + pw && y >= py && y <= py + ph {
                            engine_motion.borrow_mut().cancel_panel_hover_dismiss();
                        }
                    }
                });
                widgets.window_overlay.add_controller(motion);
            }
        }

        // ── Menu bar DrawingArea setup ─────────────────────────────────────────
        // Draw: menu bar labels via Backend + command center adjacent.
        {
            let engine = engine.clone();
            let backend_d = backend.clone();
            let cc_layout_draw = command_center_layout_cell.clone();
            let bar_rect_update = menu_bar_rect_cell.clone();
            widgets.menu_bar_da.set_draw_func(move |da, cr, _w, _h| {
                let eng = engine.borrow();
                let theme = Theme::from_name(&eng.settings.colorscheme);
                let q_theme = quadraui_gtk::q_theme(&theme);
                let font_desc = FontDescription::from_string(&UI_FONT());
                let pango_ctx = pangocairo::create_context(cr);
                let pango_layout = pango::Layout::new(&pango_ctx);
                pango_layout.set_font_description(Some(&font_desc));
                pango_layout.set_text("Xy");
                let lh = pango_layout.pixel_size().1 as f64;
                pango_layout.set_text("M");
                let cw = pango_layout.pixel_size().0 as f64;
                let w = da.width() as f64;
                let h = da.height() as f64;

                use quadraui::Backend;
                let bar = eng.menu_system.borrow().menu_bar();
                let bar_rect = quadraui::Rect::new(0.0, 0.0, w as f32, h as f32);
                bar_rect_update.set(bar_rect);
                let mb_layout = backend_d
                    .borrow_mut()
                    .enter_frame_scope(cr, &pango_layout, |b| {
                        b.set_current_theme(q_theme);
                        b.set_current_line_height(lh);
                        b.set_current_char_width(cw);
                        b.draw_menu_bar(bar_rect, &bar)
                    });

                let menu_end = mb_layout
                    .visible_items
                    .last()
                    .map(|vi| (vi.bounds.x + vi.bounds.width) as f64)
                    .unwrap_or(0.0);
                let title = eng
                    .cwd
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "VimCode".to_string());
                let cc = render::build_command_center_view(
                    eng.tab_nav_can_go_back(),
                    eng.tab_nav_can_go_forward(),
                    &title,
                );
                let cc_layout = quadraui::gtk::draw_command_center(
                    cr,
                    &pango_layout,
                    menu_end,
                    0.0,
                    (w - menu_end).max(0.0),
                    h,
                    &cc,
                    &quadraui_gtk::q_theme(&theme),
                    lh,
                );
                *cc_layout_draw.borrow_mut() = Some(cc_layout);
            });
        }
        // Click: menu bar clicks → MenuSystem, command center clicks handled separately.
        {
            let sender_menu = sender.input_sender().clone();
            let engine_menu = engine.clone();
            let backend_click = backend.clone();
            let cc_layout_click = command_center_layout_cell.clone();
            let bar_rect_click = menu_bar_rect_cell.clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |gest, _, x, _y| {
                // Try command center first (not part of MenuSystem).
                let cc_hit = cc_layout_click
                    .borrow()
                    .as_ref()
                    .map(|l| l.hit_test(x as f32, 0.5));
                match cc_hit {
                    Some(quadraui::CommandCenterHit::Back) => {
                        gest.set_state(gtk4::EventSequenceState::Claimed);
                        sender_menu.send(Msg::MruNavBack).ok();
                        return;
                    }
                    Some(quadraui::CommandCenterHit::Forward) => {
                        gest.set_state(gtk4::EventSequenceState::Claimed);
                        sender_menu.send(Msg::MruNavForward).ok();
                        return;
                    }
                    Some(quadraui::CommandCenterHit::SearchBox) => {
                        gest.set_state(gtk4::EventSequenceState::Claimed);
                        sender_menu.send(Msg::OpenCommandCenter).ok();
                        return;
                    }
                    _ => {}
                }
                // Delegate to MenuSystem for menu bar label clicks.
                let bar_rect = bar_rect_click.get();
                let ev = quadraui::UiEvent::MouseDown {
                    widget: None,
                    button: quadraui::MouseButton::Left,
                    position: quadraui::Point {
                        x: x as f32,
                        y: 0.5,
                    },
                    modifiers: quadraui::Modifiers::default(),
                };
                let menu_event = engine_menu.borrow().menu_system.borrow_mut().handle(
                    &ev,
                    &mut *backend_click.borrow_mut(),
                    bar_rect,
                );
                match menu_event {
                    quadraui::MenuEvent::Activated(id) => {
                        sender_menu
                            .send(Msg::HandleMenuAction(id.as_str().to_string()))
                            .ok();
                    }
                    quadraui::MenuEvent::Ignored => {}
                    _ => {
                        sender_menu.send(Msg::MenuRedraw).ok();
                    }
                }
            });
            widgets.menu_bar_da.add_controller(gesture);
        }
        // Hover: delegate to MenuSystem for hover-to-switch.
        {
            let sender_hover = sender.input_sender().clone();
            let engine_hover = engine.clone();
            let backend_hover = backend.clone();
            let bar_rect_hover = menu_bar_rect_cell.clone();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_, x, _y| {
                let bar_rect = bar_rect_hover.get();
                let ev = quadraui::UiEvent::MouseMoved {
                    position: quadraui::Point {
                        x: x as f32,
                        y: 0.5,
                    },
                    buttons: quadraui::ButtonMask::default(),
                };
                let menu_event = engine_hover.borrow().menu_system.borrow_mut().handle(
                    &ev,
                    &mut *backend_hover.borrow_mut(),
                    bar_rect,
                );
                match menu_event {
                    quadraui::MenuEvent::Ignored => {}
                    _ => {
                        sender_hover.send(Msg::MenuRedraw).ok();
                    }
                }
            });
            widgets.menu_bar_da.add_controller(motion);
        }
        // ── Debug sidebar DrawingArea setup ───────────────────────────────────
        {
            let engine = engine.clone();
            let backend_d = backend.clone();
            let lh_cell = debug_sidebar_lh.clone();
            widgets
                .debug_sidebar_da
                .set_draw_func(move |da, cr, _w, _h| {
                    let engine = engine.borrow();
                    let theme = Theme::from_name(&engine.settings.colorscheme);
                    let font_desc = FontDescription::from_string(&UI_FONT());
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    let char_width = {
                        layout.set_text("0");
                        layout.pixel_size().0 as f64
                    };
                    // Publish line_height for the click / scroll / key
                    // handlers — they can't recompute it themselves
                    // (no cairo context available outside the draw
                    // callback) and `cached_ui_line_height` (computed
                    // from a different DA's pango_context()) drifts on
                    // HiDPI displays. #281 smoke surfaced a 4:3 ratio
                    // off-by-N when these diverged.
                    lh_cell.set(line_height);
                    let screen =
                        build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                    let w = da.width() as f64;
                    let h = da.height() as f64;
                    render::populate_dap_sidebar_system(&engine);
                    let action_hits = draw_debug_sidebar(
                        cr,
                        &layout,
                        &screen,
                        &theme,
                        0.0,
                        0.0,
                        w,
                        h,
                        line_height,
                        &backend_d,
                        &engine,
                    );
                    engine.dap_sidebar_action_hits.replace(Some(action_hits));
                });
        }
        // ── Debug sidebar click handler ────────────────────────────────────────
        {
            let sender_dbg = sender.input_sender().clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, _, x, y| {
                sender_dbg.send(Msg::DebugSidebarClick(x, y)).ok();
            });
            widgets.debug_sidebar_da.add_controller(gesture);
        }
        // ── Debug sidebar drag handler (scrollbar thumb) ─────────────────────
        {
            let sender_drag = sender.input_sender().clone();
            let sender_drag_end = sender.input_sender().clone();
            let gesture = gtk4::GestureDrag::new();
            gesture.set_button(1);
            gesture.connect_drag_update(move |g, off_x, off_y| {
                if let Some((sx, sy)) = g.start_point() {
                    sender_drag
                        .send(Msg::DebugSidebarDrag(sx + off_x, sy + off_y))
                        .ok();
                }
            });
            gesture.connect_drag_end(move |g, off_x, off_y| {
                if let Some((sx, sy)) = g.start_point() {
                    sender_drag_end
                        .send(Msg::DebugSidebarDragEnd(sx + off_x, sy + off_y))
                        .ok();
                }
            });
            widgets.debug_sidebar_da.add_controller(gesture);
        }
        // ── Debug sidebar keyboard handler ───────────────────────────────────
        {
            let sender_dbg_key = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                sender_dbg_key
                    .send(Msg::DebugSidebarKey(key_name, ctrl))
                    .ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.debug_sidebar_da.set_focusable(true);
            widgets.debug_sidebar_da.add_controller(key_ctrl);
        }
        // ── Debug sidebar scroll handler ──────────────────────────────────────
        {
            let sender_dbg_scroll = sender.input_sender().clone();
            let scroll_ctrl =
                gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
            scroll_ctrl.connect_scroll(move |_, _dx, dy| {
                sender_dbg_scroll.send(Msg::DebugSidebarScroll(dy)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.debug_sidebar_da.add_controller(scroll_ctrl);
        }
        // Store a reference so update() can explicitly queue_draw when DAP events arrive.
        *debug_sidebar_da_ref.borrow_mut() = Some(widgets.debug_sidebar_da.clone());

        // ── Source Control sidebar draw + key setup ────────────────────────────
        {
            let engine = engine.clone();
            let backend_d = backend.clone();
            widgets.git_sidebar_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(&UI_FONT());
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let char_width = {
                    layout.set_text("0");
                    layout.pixel_size().0 as f64
                };
                let screen =
                    build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_source_control_panel(
                    cr,
                    &layout,
                    &screen,
                    &theme,
                    0.0,
                    0.0,
                    w,
                    h,
                    line_height,
                    &backend_d,
                    &engine,
                );
            });
        }
        {
            let sender_sc = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                sender_sc.send(Msg::ScKey(key_name, ctrl)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.git_sidebar_da.set_focusable(true);
            widgets.git_sidebar_da.add_controller(key_ctrl);
        }
        {
            let sender_sc = sender.input_sender().clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, n_press, x, y| {
                sender_sc.send(Msg::ScSidebarClick(x, y, n_press)).ok();
            });
            widgets.git_sidebar_da.add_controller(gesture);
        }
        {
            let sender_sc = sender.input_sender().clone();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_, x, y| {
                sender_sc.send(Msg::ScSidebarMotion(x, y)).ok();
            });
            let sender_leave = sender.input_sender().clone();
            motion.connect_leave(move |_| {
                sender_leave.send(Msg::ScSidebarMotion(-1.0, -1.0)).ok();
            });
            widgets.git_sidebar_da.add_controller(motion);
        }
        {
            let sender_sc = sender.input_sender().clone();
            quadraui::gtk::wire_da_events(&widgets.git_sidebar_da, move |ev| {
                sender_sc.send(Msg::ScSidebarEvent(ev)).ok();
            });
        }
        *git_sidebar_da_ref.borrow_mut() = Some(widgets.git_sidebar_da.clone());

        // ── Extensions sidebar draw + key setup ───────────────────────────────
        {
            let engine = engine.clone();
            let backend_d = backend.clone();
            widgets.ext_sidebar_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(&UI_FONT());
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let char_width = {
                    layout.set_text("0");
                    layout.pixel_size().0 as f64
                };
                let screen =
                    build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_ext_sidebar(
                    cr,
                    &layout,
                    &screen,
                    &theme,
                    0.0,
                    0.0,
                    w,
                    h,
                    line_height,
                    &backend_d,
                    &engine,
                );
            });
        }
        {
            let sender_ext = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, _modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let unicode = key.to_unicode().filter(|c| !c.is_control());
                sender_ext.send(Msg::ExtSidebarKey(key_name, unicode)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.ext_sidebar_da.set_focusable(true);
            widgets.ext_sidebar_da.add_controller(key_ctrl);
        }
        {
            let sender_ext = sender.input_sender().clone();
            quadraui::gtk::wire_da_events(&widgets.ext_sidebar_da, move |ev| {
                sender_ext.send(Msg::ExtSidebarEvent(ev)).ok();
            });
        }
        *ext_sidebar_da_ref.borrow_mut() = Some(widgets.ext_sidebar_da.clone());

        // ── Extension-provided panel (e.g. git-insights) draw + key + click ──
        {
            let engine = engine.clone();
            widgets
                .ext_dyn_panel_da
                .set_draw_func(move |da, cr, _w, _h| {
                    let engine = engine.borrow();
                    let theme = Theme::from_name(&engine.settings.colorscheme);
                    let font_desc = FontDescription::from_string(&UI_FONT());
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    let char_width = {
                        layout.set_text("0");
                        layout.pixel_size().0 as f64
                    };
                    let screen =
                        build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                    let w = da.width() as f64;
                    let h = da.height() as f64;
                    draw_ext_dyn_panel(cr, &layout, &screen, &theme, 0.0, 0.0, w, h, line_height);
                });
        }
        {
            let sender_ep = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, _modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let unicode = key.to_unicode().filter(|c| !c.is_control());
                sender_ep.send(Msg::ExtPanelKey(key_name, unicode)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.ext_dyn_panel_da.set_focusable(true);
            widgets.ext_dyn_panel_da.add_controller(key_ctrl);
        }
        {
            let sender_ep = sender.input_sender().clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, n_press, x, y| {
                sender_ep.send(Msg::ExtPanelClick(x, y, n_press)).ok();
            });
            widgets.ext_dyn_panel_da.add_controller(gesture);
        }
        {
            let sender_ep_rc = sender.input_sender().clone();
            let gesture_rc = gtk4::GestureClick::new();
            gesture_rc.set_button(3);
            gesture_rc.connect_pressed(move |_, _n_press, x, y| {
                sender_ep_rc.send(Msg::ExtPanelRightClick(x, y)).ok();
            });
            widgets.ext_dyn_panel_da.add_controller(gesture_rc);
        }
        {
            let sender_motion = sender.input_sender().clone();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_, x, y| {
                sender_motion.send(Msg::ExtPanelMouseMove(x, y)).ok();
            });
            widgets.ext_dyn_panel_da.add_controller(motion);
        }
        {
            let sender_scroll = sender.input_sender().clone();
            let scroll_ctrl =
                gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
            scroll_ctrl.connect_scroll(move |_, _dx, dy| {
                sender_scroll.send(Msg::ExtPanelScroll(dy)).ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.ext_dyn_panel_da.add_controller(scroll_ctrl);
        }
        // Scrollbar drag: when dragging on the scrollbar area, proportionally scroll.
        {
            let engine_drag = engine.clone();
            let da_ref_drag = ext_dyn_panel_da_ref.clone();
            let draw_needed = model.draw_needed.clone();
            let gesture = gtk4::GestureDrag::new();
            // Claim the gesture when the drag starts in the scrollbar area so that
            // parent gestures (sidebar resize) cannot steal the sequence.
            let da_ref_begin = ext_dyn_panel_da_ref.clone();
            gesture.connect_drag_begin(move |g, x, _y| {
                let da_w = if let Some(ref da) = *da_ref_begin.borrow() {
                    da.width() as f64
                } else {
                    return;
                };
                if x >= da_w - 8.0 {
                    g.set_state(gtk4::EventSequenceState::Claimed);
                }
            });
            gesture.connect_drag_update(move |g, _dx, dy| {
                let Some((start_x, start_y)) = g.start_point() else {
                    return;
                };
                let da_w = if let Some(ref da) = *da_ref_drag.borrow() {
                    da.width() as f64
                } else {
                    return;
                };
                // Only handle scrollbar drag (rightmost 8px)
                if start_x < da_w - 8.0 {
                    return;
                }
                let da_h = if let Some(ref da) = *da_ref_drag.borrow() {
                    da.height() as f64
                } else {
                    return;
                };
                let y = start_y + dy;
                let mut engine = engine_drag.borrow_mut();
                let flat_len = engine.ext_panel_flat_len();
                if flat_len == 0 || da_h <= 0.0 {
                    return;
                }
                let ratio = (y / da_h).clamp(0.0, 1.0);
                engine.ext_panel_scroll_top = (ratio * flat_len as f64) as usize;
                engine.ext_panel_scroll_top =
                    engine.ext_panel_scroll_top.min(flat_len.saturating_sub(1));
                drop(engine);
                if let Some(ref da) = *da_ref_drag.borrow() {
                    da.queue_draw();
                }
                draw_needed.set(true);
            });
            widgets.ext_dyn_panel_da.add_controller(gesture);
        }
        *ext_dyn_panel_da_ref.borrow_mut() = Some(widgets.ext_dyn_panel_da.clone());

        // ── Activity bar (A.6f: native Button chain → DrawingArea) ────────────
        {
            let engine_d = engine.clone();
            let hits_d = activity_bar_hits.clone();
            let hover_d = activity_bar_hover.clone();
            widgets.activity_bar.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine_d.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                let bar = build_gtk_activity_bar_primitive(&engine, &theme);
                let hovered = hover_d.get();
                let hits = quadraui::gtk::draw_activity_bar(
                    cr,
                    &layout,
                    da.width() as f64,
                    da.height() as f64,
                    &bar,
                    &crate::gtk::quadraui_gtk::q_theme(&theme),
                    hovered,
                );
                *hits_d.borrow_mut() = hits;
            });
        }
        // Left-click: resolve row → SidebarPanel → Msg::SwitchPanel.
        {
            let sender_c = sender.input_sender().clone();
            let hits_c = activity_bar_hits.clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, _n, _x, y| {
                let hits = hits_c.borrow();
                for hit in hits.iter() {
                    if y >= hit.y_start && y < hit.y_end {
                        if let Some(panel) = activity_id_to_panel(hit.id.as_str()) {
                            let _ = sender_c.send(Msg::SwitchPanel(panel));
                        }
                        return;
                    }
                }
            });
            widgets.activity_bar.add_controller(gesture);
        }
        // Hover tracking — updates the cell used by the draw func and queues a redraw.
        {
            let hits_m = activity_bar_hits.clone();
            let hover_m = activity_bar_hover.clone();
            let da_weak = widgets.activity_bar.downgrade();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_, _x, y| {
                let hits = hits_m.borrow();
                let mut new_hover: Option<usize> = None;
                for (i, hit) in hits.iter().enumerate() {
                    if y >= hit.y_start && y < hit.y_end {
                        new_hover = Some(i);
                        break;
                    }
                }
                if hover_m.get() != new_hover {
                    hover_m.set(new_hover);
                    if let Some(da) = da_weak.upgrade() {
                        da.queue_draw();
                    }
                }
            });
            let hover_leave = activity_bar_hover.clone();
            let da_weak_leave = widgets.activity_bar.downgrade();
            motion.connect_leave(move |_| {
                if hover_leave.get().is_some() {
                    hover_leave.set(None);
                    if let Some(da) = da_weak_leave.upgrade() {
                        da.queue_draw();
                    }
                }
            });
            widgets.activity_bar.add_controller(motion);
        }
        // Per-row tooltip via the query-tooltip signal.
        {
            let hits_t = activity_bar_hits.clone();
            widgets
                .activity_bar
                .connect_query_tooltip(move |_, _x, y, _kbd, tooltip| {
                    let hits = hits_t.borrow();
                    for hit in hits.iter() {
                        if (y as f64) >= hit.y_start && (y as f64) < hit.y_end {
                            if !hit.tooltip.is_empty() {
                                tooltip.set_text(Some(&hit.tooltip));
                                return true;
                            }
                            return false;
                        }
                    }
                    false
                });
        }
        *activity_bar_da_ref.borrow_mut() = Some(widgets.activity_bar.clone());

        // AI sidebar DrawingArea: draw function + key controller + click gesture
        {
            let engine = engine.clone();
            widgets.ai_sidebar_da.set_draw_func(move |da, cr, _, _| {
                let engine = engine.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_size = engine.settings.font_size as f64;
                let font_family = engine.settings.font_family.clone();
                let font_desc =
                    pango::FontDescription::from_string(&format!("{} {}", font_family, font_size));
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let char_width = {
                    layout.set_text("0");
                    layout.pixel_size().0 as f64
                };
                let screen =
                    build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_ai_sidebar(cr, &layout, &screen, &theme, 0.0, 0.0, w, h, line_height);
            });
        }
        {
            let sender_ai = sender.input_sender().clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, modifier| {
                let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                let unicode = key.to_unicode().filter(|c| !c.is_control());
                sender_ai
                    .send(Msg::AiSidebarKey(key_name, ctrl, unicode))
                    .ok();
                gtk4::glib::Propagation::Stop
            });
            widgets.ai_sidebar_da.add_controller(key_ctrl);
        }
        {
            let sender_ai = sender.input_sender().clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, _, x, y| {
                sender_ai.send(Msg::AiSidebarClick(x, y)).ok();
            });
            widgets.ai_sidebar_da.add_controller(gesture);
        }
        *ai_sidebar_da_ref.borrow_mut() = Some(widgets.ai_sidebar_da.clone());

        // Move the menu bar row out of the content Box and set it as the window's
        // custom titlebar.  This gives us CSD edge resize handles while keeping
        // our dark custom title strip with WindowHandle for drag-to-move.
        {
            let menu_row = &widgets.menu_bar_row;
            if let Some(parent) = menu_row.parent() {
                if let Some(parent_box) = parent.downcast_ref::<gtk4::Box>() {
                    parent_box.remove(menu_row);
                }
            }
            root.set_titlebar(Some(menu_row));
        }

        // Restore saved sidebar width (clamp to reasonable range)
        {
            let saved_width = engine.borrow().session.sidebar_width.clamp(80, 600);
            widgets.sidebar_inner_sw.set_width_request(saved_width);
        }

        // Set ew-resize cursor on drag handle
        widgets
            .sidebar_resize_handle
            .set_cursor_from_name(Some("ew-resize"));

        // Apply saved window geometry from session state
        {
            let eng = engine.borrow();
            let geom = &eng.session.window;
            root.set_default_size(geom.width, geom.height);
        }

        // Update maximize button icon and tooltip when window maximized state changes.
        // □ = maximize; ❐ (U+2750 HEAVY RIGHT ARROW) not ideal; use ⧉ (TWO JOINED SQUARES).
        {
            let btn = widgets.maximize_button.clone();
            root.connect_notify_local(Some("maximized"), move |win, _| {
                if win.is_maximized() {
                    btn.set_label("\u{29c9}"); // ⧉ two joined squares = restore
                    btn.set_tooltip_text(Some("Restore Down"));
                } else {
                    btn.set_label("\u{25a1}"); // □ = maximize
                    btn.set_tooltip_text(Some("Maximize"));
                }
            });
        }

        // Set the actual title after widget creation
        root.set_title(Some(&title));

        // Menu bar is always visible in GTK (it acts as the title bar).
        engine.borrow_mut().menu_bar_visible = true;
        engine
            .borrow()
            .menu_system
            .borrow_mut()
            .set_menus(render::build_menu_defs(engine.borrow().is_vscode_mode()));

        // Create initial scrollbars for the first window
        {
            let initial_window_id = engine.borrow().active_window_id();
            let ws = model.create_window_scrollbars(
                &widgets.editor_overlay,
                initial_window_id,
                sender.input_sender(),
            );
            model
                .window_scrollbars
                .borrow_mut()
                .insert(initial_window_id, ws);
        }

        // ── Capture-phase gesture on the editor overlay ───────────────────
        // This intercepts drag events *before* the scrollbar widgets receive
        // them, so the group divider can be grabbed even when a scrollbar
        // overlaps the divider area.  The full drag cycle (press → motion →
        // release) is handled here; the DrawingArea's divider hit-test is
        // kept as a fallback but won't fire when the overlay claims the event.
        {
            let engine_div = engine.clone();
            let lh_div = line_height_cell.clone();
            let _sender_div = sender.input_sender().clone();
            let div_active: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
            let div_active_pressed = div_active.clone();
            let div_active_motion = div_active.clone();
            let div_active_end = div_active.clone();
            let engine_motion = engine.clone();
            let lh_motion = line_height_cell.clone();
            let sender_motion = sender.input_sender().clone();
            let gesture = gtk4::GestureDrag::new();
            gesture.set_button(1);
            gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
            gesture.connect_drag_begin(move |g, x, y| {
                let engine = engine_div.borrow();
                if engine.group_layout.is_single_group() {
                    return; // let event propagate to scrollbar
                }
                let lh = lh_div.get().max(1.0);
                let widget = g.widget();
                let width = widget.width() as f64;
                let height = widget.height() as f64;
                let editor_bottom = gtk_editor_bottom(&engine, width, height, lh);
                let tab_row_h = (lh * 1.6).ceil();
                let tab_bar_h = if engine.settings.breadcrumbs {
                    tab_row_h + lh
                } else {
                    tab_row_h
                };
                let content_bounds = core::window::WindowRect::new(0.0, 0.0, width, editor_bottom);
                let dividers = engine.group_layout.dividers(content_bounds, &mut 0);
                // Check if click is in a scrollbar zone (rightmost 10px of any
                // window rect). If so, skip divider claim to let the scrollbar
                // handle the click instead.
                let (window_rects, _) =
                    engine.calculate_group_window_rects(content_bounds, tab_bar_h);
                let in_scrollbar = window_rects.iter().any(|(_, r)| {
                    let sb_zone = 10.0; // scrollbar width + margin
                    x >= r.x + r.width - sb_zone
                        && x <= r.x + r.width
                        && y >= r.y
                        && y < r.y + r.height
                });
                // Check if click is in any group's tab bar region.
                let group_rects = engine
                    .group_layout
                    .calculate_group_rects(content_bounds, tab_bar_h);
                let in_tab_bar = group_rects.iter().any(|(gid, grect)| {
                    if engine.is_tab_bar_hidden(*gid) {
                        return false;
                    }
                    let ty = grect.y - tab_bar_h;
                    y >= ty && y < ty + tab_bar_h && x >= grect.x && x < grect.x + grect.width
                });
                if !in_scrollbar && !in_tab_bar {
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
                            div_active_pressed.set(Some(div.split_index));
                            g.set_state(gtk4::EventSequenceState::Claimed);
                            return;
                        }
                    }
                }
                // Not on a divider (or in scrollbar zone) — don't claim, let scrollbar handle it
            });
            gesture.connect_drag_update(move |g, offset_x, offset_y| {
                if let Some(split_index) = div_active_motion.get() {
                    let (start_x, start_y) = g.start_point().unwrap_or((0.0, 0.0));
                    let x = start_x + offset_x;
                    let y = start_y + offset_y;
                    let engine = engine_motion.borrow();
                    let lh = lh_motion.get().max(1.0);
                    let widget = g.widget();
                    let width = widget.width() as f64;
                    let height = widget.height() as f64;
                    let editor_bottom = gtk_editor_bottom(&engine, width, height, lh);
                    let content_bounds =
                        core::window::WindowRect::new(0.0, 0.0, width, editor_bottom);
                    let dividers = engine.group_layout.dividers(content_bounds, &mut 0);
                    drop(engine);
                    if let Some(div) = dividers.iter().find(|d| d.split_index == split_index) {
                        let mouse_pos = match div.direction {
                            core::window::SplitDirection::Vertical => x,
                            core::window::SplitDirection::Horizontal => y,
                        };
                        let new_ratio =
                            ((mouse_pos - div.axis_start) / div.axis_size).clamp(0.1, 0.9);
                        engine_motion
                            .borrow_mut()
                            .group_layout
                            .set_ratio_at_index(split_index, new_ratio);
                        sender_motion.send(Msg::Resize).ok();
                    }
                }
            });
            gesture.connect_drag_end(move |_, _, _| {
                div_active_end.set(None);
            });
            widgets.editor_overlay.add_controller(gesture);
        }

        // Track resize to update viewport_lines and viewport_cols
        let sender_clone = sender.clone();
        let engine_for_resize = engine.clone();
        let cw_cell_resize = char_width_cell.clone();
        let lh_cell_resize = line_height_cell.clone();
        widgets
            .drawing_area
            .connect_resize(move |_, width, height| {
                // Use actual measured font metrics when available; fall back to
                // reasonable defaults before the first draw (Pango not yet measured).
                let line_height = lh_cell_resize.get().max(1.0);
                let char_width = cw_cell_resize.get().max(1.0);

                let total_lines = (height as f64 / line_height).floor() as usize;
                // Subtract status bar (1) + command line (1) + tab bar (1) +
                // breadcrumbs (1 if enabled).  The per-window values from
                // draw are more accurate; this is just the fallback estimate.
                let chrome_rows = {
                    let e = engine_for_resize.borrow();
                    let mut rows = 3usize; // status + cmd + tab bar
                    if e.settings.breadcrumbs {
                        rows += 1;
                    }
                    if e.settings.hide_single_tab && e.active_group().tabs.len() <= 1 {
                        rows -= 1; // tab bar hidden
                    }
                    rows
                };
                let viewport_lines = total_lines.saturating_sub(chrome_rows);

                // viewport_cols here is a rough estimate used by ensure_cursor_visible.
                // The accurate wrap column is computed in build_rendered_window from
                // the precise rect + char_width, so a small error here only affects
                // cursor scroll clamping, not wrap rendering.
                let total_cols = (width as f64 / char_width).floor() as usize;
                let viewport_cols = total_cols.saturating_sub(5); // Account for gutter

                {
                    let mut e = engine_for_resize.borrow_mut();
                    e.set_viewport_lines(viewport_lines.max(1));
                    e.set_viewport_cols(viewport_cols.max(40));
                }
                // NB: we intentionally do NOT call `terminal_resize` here when
                // `terminal_maximized` is true. During drag-resize GTK fires
                // this handler many times per second, and each `terminal_resize`
                // sends SIGWINCH + re-lays out the VT100 grid. Combined with
                // Relm4's `Msg::Resize` going through an idle queue that's
                // starved under continuous events (see PLAN.md lesson
                // "idle_add_local_once"), the panel ends up drawing at
                // NEW dimensions while the VT100 is still catching up —
                // which shows as stale cells / phantom prompts. The panel's
                // *visual* size does still track the window via
                // `effective_terminal_panel_rows` on every frame; the PTY
                // simply stays at its toggle-time size until the user
                // un-maximizes (which re-syncs via the toggle handlers).
                sender_clone.input(Msg::Resize);
            });

        // Second connect_resize: synchronously reposition scrollbar widgets so
        // they track the new size in the *same* frame as the editor redraw.
        // This avoids the 1-frame lag that occurs when going through Relm4's
        // message queue (Msg::Resize → sync_scrollbar).
        {
            let engine_for_sb = engine.clone();
            let scrollbars_for_sb = window_scrollbars_ref.clone();
            let lh_cell = line_height_cell.clone();
            let cw_cell = char_width_cell.clone();
            widgets
                .drawing_area
                .connect_resize(move |_, width, height| {
                    let engine = engine_for_sb.borrow();
                    let scrollbars = scrollbars_for_sb.borrow();
                    sync_scrollbar_positions(
                        width as f64,
                        height as f64,
                        lh_cell.get(),
                        cw_cell.get(),
                        &engine,
                        &scrollbars,
                    );
                });
        }

        let engine_clone = engine.clone();
        let sender_for_draw = sender.input_sender().clone();
        let h_sb_hovered_for_draw = h_sb_hovered_cell.clone();
        let tab_close_hover_for_draw = tab_close_hover_cell.clone();
        let h_sb_drag_for_draw = h_sb_drag_cell.clone();
        let last_metrics_for_draw = last_metrics_cell.clone();
        let tab_slots_for_draw = tab_slot_positions_cell.clone();
        let tab_close_bounds_for_draw = tab_close_bounds_cell.clone();
        let diff_btn_for_draw = diff_btn_map_cell.clone();
        let split_btn_for_draw = split_btn_map_cell.clone();
        let action_btn_for_draw = action_btn_map_cell.clone();
        let dialog_btn_for_draw = model.dialog_btn_rects.clone();
        let dialog_popup_for_draw = model.dialog_popup_rect.clone();
        let editor_hover_rect_for_draw = model.editor_hover_popup_rect.clone();
        let completion_rect_for_draw = model.completion_popup_rect.clone();
        let tab_switcher_rect_for_draw = model.tab_switcher_popup_rect.clone();
        let editor_hover_links_for_draw = model.editor_hover_link_rects.clone();
        let editor_hover_sb_for_draw = model.editor_hover_scrollbar.clone();
        let mouse_pos_for_draw = mouse_pos_cell.clone();
        let tab_vis_for_draw = tab_visible_counts_cell.clone();
        let status_seg_for_draw = model.status_segment_map.clone();
        let screen_layout_for_draw = model.cached_screen_layout.clone();
        let dbg_layout_for_draw = model.debug_toolbar_layout.clone();
        let dbg_y_for_draw = model.debug_toolbar_y_offset.clone();
        let dbg_h_for_draw = model.debug_toolbar_height.clone();
        let dbg_hovered_for_draw = model.debug_toolbar_hovered_id.clone();
        let dbg_pressed_for_draw = model.debug_toolbar_pressed_id.clone();
        let backend_for_draw = model.backend.clone();
        widgets
            .drawing_area
            .set_draw_func(move |_, cr, width, height| {
                // Wrap in catch_unwind to prevent GTK abort on panic in extern "C" callback.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Closure for one paint pass — borrows engine immutably,
                    // calls draw_editor, drops the borrow.
                    let do_paint = || {
                        let engine = engine_clone.borrow();
                        draw_editor(
                            cr,
                            &engine,
                            width,
                            height,
                            &sender_for_draw,
                            h_sb_hovered_for_draw.get(),
                            tab_close_hover_for_draw.get(),
                            h_sb_drag_for_draw.get(),
                            &last_metrics_for_draw,
                            &tab_slots_for_draw,
                            &tab_close_bounds_for_draw,
                            &diff_btn_for_draw,
                            &split_btn_for_draw,
                            &action_btn_for_draw,
                            &dialog_btn_for_draw,
                            &dialog_popup_for_draw,
                            &editor_hover_rect_for_draw,
                            &completion_rect_for_draw,
                            &tab_switcher_rect_for_draw,
                            &editor_hover_links_for_draw,
                            &editor_hover_sb_for_draw,
                            mouse_pos_for_draw.get(),
                            &tab_vis_for_draw,
                            &status_seg_for_draw,
                            &screen_layout_for_draw,
                            &dbg_layout_for_draw,
                            &dbg_y_for_draw,
                            &dbg_h_for_draw,
                            &backend_for_draw,
                            dbg_hovered_for_draw.borrow().as_ref(),
                            dbg_pressed_for_draw.borrow().as_ref(),
                        );
                    };

                    // ── Pass 1: paint with current engine state ──────────────
                    do_paint();

                    // ── Apply pixel-correct scroll offsets per group ─────────
                    // Each tuple is (group_id, available_cols, correct_offset).
                    // available_cols is reported but unused for GTK because
                    // the engine's char-based ensure_active_tab_visible
                    // algorithm under-estimates GTK's per-tab pixel width
                    // (label + tab_pad*2 + inner_gap + close + outer_gap)
                    // by ~4 chars, which causes the active tab to land
                    // off-screen. Instead the GTK draw_tab_bar computes the
                    // correct offset using actual Pango pixel measurements
                    // via quadraui::TabBar::fit_active_scroll_offset, and we
                    // write it directly to the engine here.
                    //
                    // TUI/Win-GUI keep using post_draw_apply_widths because
                    // their measurements use the same units as the engine.
                    let reports: Vec<(crate::core::window::GroupId, usize, usize)> =
                        tab_vis_for_draw.borrow_mut().drain(..).collect();
                    let mut changed = false;
                    {
                        let mut engine = engine_clone.borrow_mut();
                        for (gid, _available_cols, correct_offset) in &reports {
                            if engine.set_tab_scroll_offset(*gid, *correct_offset) {
                                changed = true;
                            }
                        }
                    }

                    // ── Pass 2: if state changed, repaint with fresh
                    // scroll_offset — overdraws pass 1 in the same Cairo
                    // context. Eliminates the one-frame lag. Converges
                    // within this single callback: pass 2 measures the
                    // same widths and computes the same correct_offset,
                    // which now matches the engine state, so set returns
                    // false and we don't loop.
                    if changed {
                        tab_vis_for_draw.borrow_mut().clear();
                        do_paint();
                        // Drain pass 2's reports so the queue is empty for
                        // the next paint (avoids stale widths sitting around).
                        let reports2: Vec<(_, _, _)> =
                            tab_vis_for_draw.borrow_mut().drain(..).collect();
                        let mut engine = engine_clone.borrow_mut();
                        for (gid, _available_cols, correct_offset) in &reports2 {
                            engine.set_tab_scroll_offset(*gid, *correct_offset);
                        }
                    }
                }));
                if let Err(e) = result {
                    eprintln!("draw_editor panic: {:?}", e);
                }
            });

        // Motion controller: write mouse position directly into a shared cell.
        // This avoids routing every motion event (100-200 Hz on Linux) through the Relm4
        // message loop. The hover state is computed in SearchPollTick (20 Hz) instead.
        {
            let pos_cell = mouse_pos_cell.clone();
            let pos_cell_leave = mouse_pos_cell.clone();
            let engine_motion = engine.clone();
            let lh_motion = line_height_cell.clone();
            let cw_motion = char_width_cell.clone();
            let da_motion = widgets.drawing_area.clone();
            let mc = gtk4::EventControllerMotion::new();
            mc.connect_motion(move |_, x, y| {
                pos_cell.set((x, y));
                // Update context menu hover: persist selected index so it
                // sticks when the mouse leaves. try_borrow_mut fails during
                // draw (engine immutably borrowed) — that's fine, the draw
                // function computes hover from mouse_pos directly.
                if let Ok(mut eng) = engine_motion.try_borrow_mut() {
                    if eng.context_menu.is_some() {
                        let lh = lh_motion.get();
                        let cw = cw_motion.get();
                        if lh >= 1.0 && cw >= 1.0 {
                            let col = (x / cw) as u16;
                            let row = (y / lh) as u16;
                            let tw = (da_motion.width() as f64 / cw) as u16;
                            let th = (da_motion.height() as f64 / lh) as u16;
                            let cm = eng.context_menu.as_ref().unwrap();
                            if let crate::core::engine::ContextMenuClickResult::Item(idx) =
                                crate::core::engine::resolve_context_menu_click(
                                    &cm.items,
                                    cm.screen_x,
                                    cm.screen_y,
                                    tw,
                                    th,
                                    col,
                                    row,
                                )
                            {
                                eng.context_menu.as_mut().unwrap().selected = idx;
                            }
                        }
                        drop(eng);
                        da_motion.queue_draw();
                    }
                }
            });
            mc.connect_leave(move |_| {
                pos_cell_leave.set((-1.0, -1.0));
            });
            widgets.drawing_area.add_controller(mc);
        }

        // Right-click on drawing area (tab bar or editor context menu).
        {
            let engine_rc = engine.clone();
            let sender_rc = sender.input_sender().clone();
            let lh_rc = line_height_cell.clone();
            let cw_rc = char_width_cell.clone();
            let da_rc = model.drawing_area.clone();
            let tab_slots_rc = tab_slot_positions_cell.clone();
            let diff_btn_rc = diff_btn_map_cell.clone();
            let split_btn_rc = split_btn_map_cell.clone();
            let action_btn_rc = action_btn_map_cell.clone();
            let status_seg_rc = status_segment_map_cell.clone();
            let screen_layout_rc = model.cached_screen_layout.clone();
            let rc_gesture = gtk4::GestureClick::new();
            rc_gesture.set_button(3);
            rc_gesture.connect_pressed(move |gesture, _n_press, x, y| {
                let _widget = gesture.widget();
                let lh = lh_rc.get().max(1.0);
                let cw = cw_rc.get().max(1.0);
                let layout_ref = screen_layout_rc.borrow();
                let Some(ref layout) = *layout_ref else {
                    return;
                };
                let mut engine = engine_rc.borrow_mut();
                let editor_pl = {
                    let da_ref = da_rc.borrow();
                    let ctx = da_ref.as_ref().expect("drawing area").pango_context();
                    let pl = pango::Layout::new(&ctx);
                    let fd = FontDescription::from_string(&format!(
                        "{} {}",
                        engine.settings.font_family, engine.settings.font_size
                    ));
                    pl.set_font_description(Some(&fd));
                    pl
                };
                let target = pixel_to_click_target(
                    &mut engine,
                    x,
                    y,
                    lh,
                    cw,
                    &editor_pl,
                    layout,
                    &tab_slots_rc.borrow(),
                    &diff_btn_rc.borrow(),
                    &split_btn_rc.borrow(),
                    &action_btn_rc.borrow(),
                    &status_seg_rc.borrow(),
                );
                match target {
                    ClickTarget::TabBar => {
                        let group_id = engine.active_group;
                        let tab_idx = engine
                            .editor_groups
                            .get(&group_id)
                            .map(|g| g.active_tab)
                            .unwrap_or(0);
                        drop(engine);
                        let _ = sender_rc.send(Msg::TabRightClick {
                            group_id,
                            tab_idx,
                            x,
                            y,
                        });
                    }
                    ClickTarget::BufferPos(..) | ClickTarget::Gutter => {
                        drop(engine);
                        let _ = sender_rc.send(Msg::EditorRightClick { x, y });
                    }
                    _ => {}
                }
            });
            widgets.drawing_area.add_controller(rc_gesture);
        }

        // Tab switcher auto-confirm: poll modifier state every 50ms while open.
        // When neither Ctrl nor Alt is held, confirm immediately.
        {
            let engine_ref = engine.clone();
            let da = widgets.drawing_area.clone();
            let root_ref = root.clone();
            gtk4::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let open = engine_ref
                    .try_borrow()
                    .map(|e| e.tab_switcher_open)
                    .unwrap_or(false);
                if !open {
                    return gtk4::glib::ControlFlow::Continue;
                }
                // Query the current keyboard modifier state from GDK
                {
                    let display = gtk4::prelude::WidgetExt::display(&root_ref);
                    if let Some(seat) = display.default_seat() {
                        if let Some(keyboard) = seat.keyboard() {
                            let mods: gdk::ModifierType = keyboard.modifier_state();
                            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
                            let alt = mods.contains(gdk::ModifierType::ALT_MASK);
                            if !ctrl && !alt {
                                if let Ok(mut e) = engine_ref.try_borrow_mut() {
                                    e.tab_switcher_confirm();
                                    drop(e);
                                    da.queue_draw();
                                }
                            }
                        }
                    }
                }
                gtk4::glib::ControlFlow::Continue
            });
        }

        // Ensure drawing area has keyboard focus on startup.
        // grab_focus() during init runs before the window is mapped, so some
        // window managers (e.g. Cinnamon/Mutter) ignore it.  Present the window
        // and defer the grab until the first frame is drawn.
        root.present();
        {
            let da = widgets.drawing_area.clone();
            gtk4::glib::idle_add_local_once(move || {
                da.grab_focus();
            });
        }

        // Poll for background search results every 50 ms.
        let sender_for_poll = sender.input_sender().clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            sender_for_poll.send(Msg::SearchPollTick).ok();
            gtk4::glib::ControlFlow::Continue
        });

        // Phase B.5b Stage 1: drain the backend's `UiEvent` queue.
        // Producers — the editor DA's key/mouse/scroll signal
        // callbacks — push translated events into
        // `GtkBackend::events_handle()`. This consumer drains them
        // periodically so the queue can't grow unbounded. Today the
        // events are simply discarded (Relm4 `Msg` flow stays
        // authoritative); subsequent B.5b stages route specific
        // event shapes back through `dispatch_*` helpers as each
        // surface migrates onto the trait. Tick at ~60 Hz so a real
        // dispatcher introduced later sees no perceptible latency.
        {
            use quadraui::Backend;
            let backend_for_drain = model.backend.clone();
            gtk4::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                let _ = backend_for_drain.borrow_mut().poll_events();
                gtk4::glib::ControlFlow::Continue
            });
        }

        // ── Disable GTK mnemonic Alt interception ─────────────────────────────
        // GTK4 has a built-in ShortcutController on the window that intercepts
        // Alt key events for mnemonic activation *during* the capture phase,
        // before any user-added EventControllerKey can see them.  We don't use
        // mnemonics, so reassign the trigger to HYPER_MASK (never pressed) so
        // Alt keys reach our regular key handler for VSCode-mode shortcuts.
        {
            use gtk4::prelude::*;
            let controllers = root.observe_controllers();
            for i in 0..controllers.n_items() {
                if let Some(obj) = controllers.item(i) {
                    if let Ok(sc) = obj.downcast::<gtk4::ShortcutController>() {
                        if sc
                            .mnemonics_modifiers()
                            .contains(gdk::ModifierType::ALT_MASK)
                        {
                            sc.set_mnemonics_modifiers(gdk::ModifierType::HYPER_MASK);
                        }
                    }
                }
            }
        }

        // Intercept F10 at the window level before GTK's built-in
        // menubar activation shortcut can swallow it.
        {
            let sender_fkey = sender.input_sender().clone();
            let fkey_ctrl = gtk4::EventControllerKey::new();
            fkey_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
            fkey_ctrl.connect_key_pressed(move |_, key, _, modifier| {
                let name = key.name().map(|s| s.to_string()).unwrap_or_default();
                let dominated = modifier.contains(gdk::ModifierType::CONTROL_MASK)
                    || modifier.contains(gdk::ModifierType::ALT_MASK);
                if !dominated && matches!(name.as_str(), "F5" | "F9" | "F10" | "F11") {
                    sender_fkey
                        .send(Msg::KeyPress {
                            key_name: name,
                            unicode: None,
                            ctrl: false,
                            alt: false,
                        })
                        .ok();
                    return gtk4::glib::Propagation::Stop;
                }
                if modifier.contains(gdk::ModifierType::SHIFT_MASK)
                    && !dominated
                    && matches!(name.as_str(), "F5" | "F11")
                {
                    sender_fkey
                        .send(Msg::KeyPress {
                            key_name: name,
                            unicode: None,
                            ctrl: false,
                            alt: false,
                        })
                        .ok();
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
            });
            root.add_controller(fkey_ctrl);
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
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
                self.handle_key_press(key_name, unicode, ctrl, alt, &sender);
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
                self.handle_mouse_click_msg(x, y, width, height, alt, &sender);
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
                self.handle_sidebar_panel_msg(msg, &sender);
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
                self.handle_explorer_msg(msg, &sender);
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
                                    let step = (delta.y.abs() * 3.0).ceil() as usize;
                                    if delta.y > 0.0 {
                                        engine.terminal_scroll_down(step);
                                    } else {
                                        engine.terminal_scroll_up(step);
                                    }
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
                    sender.input(Msg::RefreshFileTree);
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
                    sender.input(Msg::RefreshFileTree);
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
                self.handle_poll_tick(&sender);
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
                self.handle_file_ops_msg(msg, &sender);
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
                self.handle_menu_msg(msg, &sender);
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
            | Msg::PromptRenameFile(_)
            | Msg::PromptNewFile(_)
            | Msg::PromptNewFolder(_) => {
                self.handle_explorer_msg(msg, &sender);
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
                self.handle_dialog_msg(msg, &sender);
            }
        }

        // Sync scrollbar position to match engine state (except when scrollbar itself changed)
        if !is_scrollbar_msg {
            self.sync_scrollbar();
        }
    }
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

impl App {
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
        engine.session.window.width = self.window.default_width();
        engine.session.window.height = self.window.default_height();
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
    fn dispatch_engine_action(
        &mut self,
        action: EngineAction,
        sender: &ComponentSender<Self>,
        is_macro: bool,
    ) {
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
                    sender.input(Msg::ToggleTerminal);
                } else {
                    sender.input(Msg::NewTerminalTab);
                }
            }
            EngineAction::ToggleTerminalMaximize => {
                sender.input(Msg::ToggleTerminalMaximize);
            }
            EngineAction::RunInTerminal(cmd) => {
                sender.input(Msg::RunCommandInTerminal(cmd));
            }
            EngineAction::OpenFolderDialog => {
                if !is_macro {
                    sender.input(Msg::OpenFolderDialog);
                }
            }
            EngineAction::OpenWorkspaceDialog => {
                if !is_macro {
                    sender.input(Msg::OpenWorkspaceDialog);
                }
            }
            EngineAction::SaveWorkspaceAsDialog => {
                if !is_macro {
                    sender.input(Msg::SaveWorkspaceAsDialog);
                }
            }
            EngineAction::OpenRecentDialog => {
                if !is_macro {
                    sender.input(Msg::OpenRecentDialog);
                }
            }
            EngineAction::QuitWithUnsaved => {
                sender.input(Msg::ShowQuitConfirm);
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
        drop(engine);

        if new_content != self.last_clipboard_content {
            if let (Some(ref content), Some(ref mut ctx)) = (&new_content, &mut self.clipboard) {
                let _ = ctx.set_contents(content.clone());
            }
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
        sender: &relm4::Sender<Msg>,
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
    fn handle_key_press(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        alt: bool,
        sender: &ComponentSender<Self>,
    ) {
        // Handle Ctrl-Shift-V paste (sent as synthetic "PasteClipboard" key):
        // do async GDK clipboard read → ClipboardPasteToInput
        if key_name == "PasteClipboard" {
            if let Some(display) = gdk::Display::default() {
                let sender = sender.clone();
                display
                    .clipboard()
                    .read_text_async(gtk4::gio::Cancellable::NONE, move |result| {
                        let text = result
                            .ok()
                            .flatten()
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        sender.input(Msg::ClipboardPasteToInput { text });
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
                        sender.input(Msg::RefreshFileTree);
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

        // In VSCode mode, Ctrl-V reads clipboard into register '+' before
        // calling handle_key (which will read it via get_register_content).
        if ctrl && key_name == "v" && self.engine.borrow().is_vscode_mode() {
            if let Some(ref mut ctx) = self.clipboard {
                let text = ctx.get_contents().unwrap_or_default();
                let mut engine = self.engine.borrow_mut();
                engine.registers.insert('+', (text.clone(), false));
                engine.registers.insert('"', (text, false));
            }
            // Fall through to handle_key which calls vscode_paste().
        }

        // Intercept p/P to read from the system clipboard first
        // (clipboard=unnamedplus semantics: plain p/P and "+p/"*p all read
        // from system clipboard).  Skip for explicit named registers like "ap.
        if !ctrl && (key_name == "p" || key_name == "P") {
            let use_clipboard = {
                let engine = self.engine.borrow();
                matches!(
                    engine.selected_register,
                    None | Some('"') | Some('+') | Some('*')
                )
            };
            if use_clipboard {
                if let Some(ref mut ctx) = self.clipboard {
                    let text = ctx.get_contents().unwrap_or_default();
                    if !text.is_empty() {
                        let mut engine = self.engine.borrow_mut();
                        self.last_clipboard_content = Some(text.clone());
                        engine.load_clipboard_for_paste(text);
                    }
                }
                // Fall through — handle_key() will execute the paste.
            }
        }

        // Debug F-keys must reach the engine regardless of which panel
        // has focus — F5 (continue), F9 (breakpoint), F10 (step over),
        // F11 (step in) are global debugger commands.
        if !ctrl && !alt {
            match key_name.as_str() {
                "F5" | "F9" | "F10" | "F11" => {
                    let mapped = map_gtk_key_name(&key_name);
                    let action = self.engine.borrow_mut().handle_key(mapped, None, false);
                    self.dispatch_engine_action(action, sender, false);
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
            self.handle_explorer_da_key(key_mapped, unicode, ctrl, sender);
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
                self.focus_editor_if_needed(still_focused && !has_dialog);
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
                self.focus_editor_if_needed(still_focused && !has_dialog);
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
                self.focus_editor_if_needed(still_focused && !has_dialog);
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
                        let sender = sender.clone();
                        display.clipboard().read_text_async(
                            gtk4::gio::Cancellable::NONE,
                            move |result| {
                                let text = result
                                    .ok()
                                    .flatten()
                                    .map(|s| s.to_string())
                                    .unwrap_or_default();
                                sender.input(Msg::ClipboardPasteToInput { text });
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
                self.focus_editor_if_needed(still_focused);
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
                self.focus_editor_if_needed(still_focused);
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
                self.focus_editor_if_needed(still_focused);
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
                self.focus_editor_if_needed(still_focused);
                if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
                return;
            }
        }

        // Hover popup copy: intercept y/Ctrl-C when hover is focused
        // because GTK doesn't set clipboard_write on the engine.
        {
            let engine = self.engine.borrow();
            let is_hover_copy = engine.editor_hover_has_focus
                && (key_name == "y" || key_name == "Y" || (ctrl && key_name == "c"));
            if is_hover_copy {
                if let Some(text) = engine.hover_selection_text() {
                    drop(engine);
                    if let Some(ref mut ctx) = self.clipboard {
                        let _ = ctx.set_contents(text);
                    }
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

        self.dispatch_engine_action(action, sender, false);
        self.draw_needed.set(true);

        // Process macro playback queue if active
        loop {
            let (has_more, action) = {
                let mut engine = self.engine.borrow_mut();
                engine.advance_macro_playback()
            };

            self.dispatch_engine_action(action, sender, true);

            if !has_more {
                break;
            }
        }

        // Ctrl-W h/l overflow: show sidebar and focus the active panel.
        {
            let overflow = self.engine.borrow_mut().window_nav_overflow.take();
            if let Some(false) = overflow {
                let panel_id = self
                    .active_panel
                    .to_panel_id()
                    .unwrap_or(crate::core::engine::sidebar::PANEL_EXPLORER);
                self.engine.borrow_mut().focus_sidebar_panel(panel_id);
                self.sync_sidebar_from_engine();
            }
        }

        // Sync the unnamed register to the system clipboard if it changed.
        // The comparison is O(1); actual write is deferred to the background thread.
        self.sync_plus_register_to_clipboard();

        // If a yank just happened, schedule a 200 ms one-shot to clear the highlight.
        if self.engine.borrow().yank_highlight.is_some() {
            let s = sender.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                s.input(Msg::ClearYankHighlight);
            });
        }

        self.draw_needed.set(true);
    }

    fn handle_poll_tick(&mut self, sender: &ComponentSender<Self>) {
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

                // Debug toolbar hover detection — same pattern as tab_close_hover above.
                {
                    let dbg_y = self.debug_toolbar_y_offset.get();
                    let dbg_h = self.debug_toolbar_height.get();
                    let new_hover = if dbg_h > 0.0 && my >= dbg_y && my < dbg_y + dbg_h {
                        let guard = self.debug_toolbar_layout.borrow();
                        guard.as_ref().and_then(|l| {
                            match l.hit_test(mx as f32, (my - dbg_y) as f32) {
                                quadraui::StatusBarHit::Segment(id) => Some(id),
                                quadraui::StatusBarHit::Empty => None,
                            }
                        })
                    } else {
                        None
                    };
                    if new_hover != *self.debug_toolbar_hovered_id.borrow() {
                        *self.debug_toolbar_hovered_id.borrow_mut() = new_hover;
                        self.draw_needed.set(true);
                    }
                }

                // Sync per-window viewport dimensions from the paint-time
                // ScreenLayout so ensure_cursor_visible uses exact geometry.
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
        // Run all periodic background work (LSP, DAP, terminal, search, etc.)
        // poll_idle() consumes dap_wants_sidebar internally.
        let idle_dirty = self.engine.borrow_mut().poll_idle();
        if idle_dirty {
            self.sync_sidebar_from_engine();
        }
        // Format-on-save + :wq/:x deferred quit
        if self.engine.borrow().format_save_quit_ready {
            self.engine.borrow_mut().format_save_quit_ready = false;
            sender.input(Msg::QuitConfirmed);
        }
        // Run pending terminal commands (needs backend-supplied terminal size).
        if self.engine.borrow().pending_terminal_command.is_some() {
            let cmd = self
                .engine
                .borrow_mut()
                .pending_terminal_command
                .take()
                .unwrap();
            sender.input(Msg::RunCommandInTerminal(cmd));
        }
        // Explicitly redraw the debug sidebar if it's active so the
        // Run/Stop button text and section data stay in sync.
        if self.active_panel == SidebarPanel::Debug {
            if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                da.queue_draw();
            }
        }
        // Explorer refresh after confirmed file move.
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            sender.input(Msg::RefreshFileTree);
        }
        // Auto-refresh SC panel periodically (gated on sidebar visibility).
        if self.sidebar_visible
            && (self.active_panel == SidebarPanel::Git
                || self.active_panel == SidebarPanel::Explorer)
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
                engine.ext_panel_active = Some(panel_name.clone());
            }
            self.active_panel = SidebarPanel::ExtPanel(panel_name);
            self.sidebar_visible = true;
            self.sync_sidebar_widgets();
        }
        // GTK-specific: queue redraws on individual sidebar DAs whose
        // content may have changed from the polls above.
        if self.active_panel == SidebarPanel::Explorer {
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
            if self.active_panel == SidebarPanel::Explorer {
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
        self.window.set_title(Some(&win_title));
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
        let ctx = {
            let da_ref = self.drawing_area.borrow();
            da_ref.as_ref().expect("drawing area").pango_context()
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
    fn handle_mouse_click_msg(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        alt: bool,
        sender: &ComponentSender<Self>,
    ) {
        self.reconcile_editor_hover_modal();

        // ── Scroll-surface click dispatch (scrollbar thumb-drag + track-page). ──
        {
            let surfaces = self.engine.borrow().scroll_surfaces.borrow().clone();
            let modal = self.backend.borrow().modal_stack_handle().borrow().clone();
            let mut drag = self.backend.borrow().drag_state_handle().borrow().clone();
            let click_events = quadraui::dispatch_click(
                &modal,
                &surfaces,
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
        //
        // The popup is keyboard-driven (Tab to cycle, Enter to commit)
        // so this stage doesn't add row-level click-to-select; the
        // bounds registration on `ModalStack` exists so future
        // `is_modal_open()` consumers (e.g. B5b.6 hover-trigger gate)
        // see the popup correctly.
        if self.engine.borrow().completion_idx.is_some() {
            let completion_id = quadraui::WidgetId::new("completion");
            let inside = if let Some((px, py, pw, ph)) = self.completion_popup_rect.get() {
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .push(
                        completion_id.clone(),
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
                            if *id == completion_id
                    )
                })
            } else {
                false
            };

            // Either way, dismiss the popup — it's transient.
            self.engine.borrow_mut().dismiss_completion();
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&completion_id);

            if inside {
                self.draw_needed.set(true);
                return;
            }
            // Fall through so the editor's click (cursor move) proceeds.
        } else {
            // Defensive cleanup: completion may have been dismissed
            // without us seeing a click. Pop any stale entry.
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&quadraui::WidgetId::new("completion"));
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
            let cw = self.cached_char_width.max(1.0);
            let lh = self.cached_line_height.max(1.0);
            let cm_id = quadraui::WidgetId::new("context_menu");

            // Build a `quadraui::ContextMenu` + `ContextMenuLayout`
            // identical to what the renderer computes. Mirrors
            // `draw.rs::draw_context_menu_popup` so any future
            // sizing/positioning change there propagates here without
            // a parallel update.
            let menu_layout = {
                let engine = self.engine.borrow();
                let cm = engine.context_menu.as_ref().unwrap();
                if cm.items.is_empty() {
                    None
                } else {
                    let panel = crate::render::ContextMenuPanel {
                        items: cm
                            .items
                            .iter()
                            .map(|item| crate::render::ContextMenuRenderItem {
                                label: item.label.clone(),
                                shortcut: item.shortcut.clone(),
                                separator_after: item.separator_after,
                                enabled: item.enabled,
                            })
                            .collect(),
                        selected_idx: cm.selected,
                        screen_col: cm.screen_x,
                        screen_row: cm.screen_y,
                    };
                    let menu = crate::render::context_menu_panel_to_quadraui_context_menu(&panel);
                    let item_height = |_i: usize| quadraui::ContextMenuItemMeasure::new(lh as f32);
                    let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
                    let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
                    let content_cols = (max_label + max_sc + 6).clamp(20, 50);
                    let menu_w = content_cols as f64 * cw;
                    let anchor_x = cm.screen_x as f64 * cw;
                    let anchor_y = cm.screen_y as f64 * lh;
                    let viewport = quadraui::Rect::new(0.0, 0.0, width as f32, height as f32);
                    Some(menu.layout(
                        anchor_x as f32,
                        anchor_y as f32,
                        viewport,
                        menu_w as f32,
                        item_height,
                    ))
                }
            };

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
                                sender.input(Msg::RefreshFileTree);
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

        // Debug toolbar click: resolve via cached StatusBarLayout.
        // Each region's `id` is `debug:btn:N` — translated to the
        // action via `render::debug_toolbar_action_index`.
        {
            let dbg_y = self.debug_toolbar_y_offset.get();
            let dbg_h = self.debug_toolbar_height.get();
            if dbg_h > 0.0 && y >= dbg_y && y < dbg_y + dbg_h {
                *self.debug_toolbar_pressed_id.borrow_mut() =
                    self.debug_toolbar_hovered_id.borrow().clone();
                self.draw_needed.set(true);
                let guard = self.debug_toolbar_layout.borrow();
                if let Some(ref bar_layout) = *guard {
                    let local_x = x as f32;
                    let local_y = (y - dbg_y) as f32;
                    if let quadraui::StatusBarHit::Segment(ref id) =
                        bar_layout.hit_test(local_x, local_y)
                    {
                        if let Some(idx) = render::debug_toolbar_action_index(id) {
                            if let Some(btn) = render::DEBUG_BUTTONS.get(idx) {
                                drop(guard);
                                let _ = self.engine.borrow_mut().execute_command(btn.action);
                                return;
                            }
                        }
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
                    sender.input(Msg::RefreshFileTree);
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
            let in_terminal = if self.cached_line_height > 0.0 {
                let engine = self.engine.borrow();
                if engine.terminal_open || engine.bottom_panel_open {
                    // Must use the *effective* rows so hit-testing lines up
                    // with where the panel is actually drawn when maximized.
                    let target =
                        gtk_terminal_target_maximize_rows(&engine, height, self.cached_line_height);
                    let effective_rows = engine.effective_terminal_panel_rows(target);
                    let term_px = (effective_rows as f64 + 2.0) * self.cached_line_height;
                    let global_status_rows = if engine.settings.window_status_line {
                        0.0
                    } else {
                        1.0
                    };
                    let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                    let toolbar_px = if engine.debug_toolbar_visible {
                        self.cached_line_height
                    } else {
                        0.0
                    };
                    let term_y = height - status_h - toolbar_px - term_px;
                    if y >= term_y {
                        // 0 = tab bar, 1 = toolbar, 2 = content
                        let zone = if y >= term_y + 2.0 * self.cached_line_height {
                            2
                        } else if y >= term_y + self.cached_line_height {
                            1
                        } else {
                            0
                        };
                        Some((term_y, zone))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((term_y, zone)) = in_terminal {
                if zone == 0 {
                    self.engine.borrow_mut().handle_bottom_tab_bar_click(x);
                    sender.input(Msg::Resize);
                    return;
                }
                self.engine.borrow_mut().terminal_has_focus = true;
                if zone == 2 {
                    const SB_W: f64 = 6.0;
                    // In split mode: detect a click on the divider (start drag)
                    // or set keyboard focus to the appropriate pane.
                    let on_divider = if self.engine.borrow().terminal_split
                        && self.engine.borrow().terminal_panes.len() >= 2
                    {
                        let left_cols = {
                            let engine = self.engine.borrow();
                            if engine.terminal_split_left_cols > 0 {
                                engine.terminal_split_left_cols
                            } else {
                                engine.terminal_panes[0].cols
                            }
                        };
                        let div_x = left_cols as f64 * self.cached_char_width;
                        if x < width - SB_W && (x - div_x).abs() < 4.0 {
                            self.terminal_split_dragging = true;
                            true
                        } else {
                            let mut engine = self.engine.borrow_mut();
                            engine.terminal_active = if x < div_x { 0 } else { 1 };
                            false
                        }
                    } else {
                        false
                    };
                    if !on_divider {
                        self.terminal_resize_dragging = false;
                        let row = ((y - term_y - 2.0 * self.cached_line_height)
                            / self.cached_line_height) as u16;
                        let col = (x / self.cached_char_width.max(1.0)) as u16;
                        self.engine.borrow_mut().terminal_scroll_reset();
                        if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                            term.selection = Some(crate::core::terminal::TermSelection {
                                start_row: row,
                                start_col: col,
                                end_row: row,
                                end_col: col,
                            });
                        }
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

                    // ── Debug toolbar hit-test ────────────────────────────────
                    let mut toolbar_handled = false;
                    if engine.debug_toolbar_visible && self.cached_line_height > 0.0 {
                        // Toolbar is the single row above status(1)+cmd(1).
                        // It is always at a fixed position; terminal/quickfix/DAP
                        // panels stack above it, not below it.
                        let toolbar_y =
                            height - 2.0 * self.cached_line_height - self.cached_line_height;
                        if y >= toolbar_y && y < toolbar_y + self.cached_line_height {
                            let mut cursor_x = 8.0_f64;
                            for (idx, btn) in render::DEBUG_BUTTONS.iter().enumerate() {
                                if idx == 4 {
                                    cursor_x += 16.0; // visual separator gap
                                }
                                let text_len =
                                    btn.icon.chars().count() + btn.key_hint.chars().count() + 4; // " (hint) "
                                let btn_w = text_len as f64 * self.cached_char_width;
                                if x >= cursor_x && x < cursor_x + btn_w {
                                    let _ = engine.execute_command(btn.action);
                                    toolbar_handled = true;
                                    break;
                                }
                                cursor_x += btn_w;
                            }
                            if !toolbar_handled {
                                toolbar_handled = true; // click in toolbar row, consume event
                            }
                        }
                    }

                    if !toolbar_handled {
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
                                sender.input(Msg::ShowCloseTabConfirm);
                                self.draw_needed.set(true);
                                return;
                            }
                            Some(false) => {
                                // Buffer click — fire hooks and reveal file
                            }
                            None => {
                                // Check if the click opened an editor action menu.
                                if engine.context_menu.as_ref().is_some_and(|cm| {
                                    matches!(
                                        cm.target,
                                        core::engine::ContextMenuTarget::EditorActionMenu { .. }
                                    )
                                }) {
                                    let group_id =
                                        match &engine.context_menu.as_ref().unwrap().target {
                                            core::engine::ContextMenuTarget::EditorActionMenu {
                                                group_id,
                                            } => *group_id,
                                            _ => unreachable!(),
                                        };
                                    drop(engine);
                                    self.show_action_menu_popover(group_id, x, y, sender);
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
            // Update drop zone while dragging.
            let mut engine = self.engine.borrow_mut();
            engine.tab_drag_mouse = Some((x, y));
            let zone = compute_tab_drop_zone(
                &engine,
                x,
                y,
                width,
                height,
                self.cached_line_height,
                self.cached_char_width,
                &self.tab_slot_positions.borrow(),
            );
            engine.tab_drop_zone = zone;
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
                    engine.tab_drag_begin(gid, tidx);
                    engine.tab_drag_mouse = Some((x, y));
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
            // Check if drag is in the terminal content area (text selection).
            let in_terminal = if self.cached_line_height > 0.0 {
                let engine = self.engine.borrow();
                if engine.terminal_open || engine.bottom_panel_open {
                    // Must use the *effective* rows so hit-testing lines up
                    // with where the panel is actually drawn when maximized.
                    let target =
                        gtk_terminal_target_maximize_rows(&engine, height, self.cached_line_height);
                    let effective_rows = engine.effective_terminal_panel_rows(target);
                    let term_px = (effective_rows as f64 + 2.0) * self.cached_line_height;
                    let global_status_rows = if engine.settings.window_status_line {
                        0.0
                    } else {
                        1.0
                    };
                    let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                    let toolbar_px = if engine.debug_toolbar_visible {
                        self.cached_line_height
                    } else {
                        0.0
                    };
                    let term_y = height - status_h - toolbar_px - term_px;
                    if y >= term_y + 2.0 * self.cached_line_height {
                        Some(term_y)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(term_y) = in_terminal {
                let row =
                    ((y - term_y - 2.0 * self.cached_line_height) / self.cached_line_height) as u16;
                let col = (x / self.cached_char_width.max(1.0)) as u16;
                if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                    if let Some(ref mut sel) = term.selection {
                        sel.end_row = row;
                        sel.end_col = col;
                    }
                }
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
        if self.debug_toolbar_pressed_id.borrow().is_some() {
            *self.debug_toolbar_pressed_id.borrow_mut() = None;
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
            let mut engine = self.engine.borrow_mut();
            let zone = engine.tab_drop_zone;
            engine.tab_drag_drop(zone);
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
                    engine.terminal_panes[0].cols
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
        let mut engine = self.engine.borrow_mut();
        engine.mouse_drag_active = false;
        engine.mouse_drag_origin_window = None;
        self.draw_needed.set(true);
    }

    fn show_action_menu_popover(
        &mut self,
        group_id: core::window::GroupId,
        x: f64,
        y: f64,
        _sender: &ComponentSender<Self>,
    ) {
        let da = match self.drawing_area.borrow().as_ref() {
            Some(da) => da.clone(),
            None => return,
        };

        // Extract the items from the engine context menu (already populated).
        let items: Vec<core::engine::ContextMenuItem> = {
            let engine = self.engine.borrow();
            engine
                .context_menu
                .as_ref()
                .map(|cm| cm.items.clone())
                .unwrap_or_default()
        };
        // Close the engine-side context menu; GTK handles it natively.
        self.engine.borrow_mut().close_context_menu();

        let menu = build_gio_menu_from_engine_items(&items, "actmenu");

        let enabled_map: std::collections::HashMap<String, bool> = items
            .iter()
            .map(|it| (it.action.clone(), it.enabled))
            .collect();

        let actions = gtk4::gio::SimpleActionGroup::new();

        // Register an action for each menu item that delegates to engine.
        for item in &items {
            let action_name = item.action.clone();
            let engine_ref = self.engine.clone();
            let draw_ref = self.draw_needed.clone();
            let gid = group_id;
            let a = gtk4::gio::SimpleAction::new(&action_name, None);
            let act = action_name.clone();
            a.connect_activate(move |_, _| {
                let mut e = engine_ref.borrow_mut();
                e.active_group = gid;
                // Re-open the context menu so confirm() can find items.
                e.open_editor_action_menu(gid, 0, 0);
                // Find and select the matching item.
                if let Some(ref mut cm) = e.context_menu {
                    if let Some(idx) = cm.items.iter().position(|i| i.action == act) {
                        cm.selected = idx;
                    }
                }
                e.context_menu_confirm();
                draw_ref.set(true);
            });
            if enabled_map.get(&action_name) == Some(&false) {
                a.set_enabled(false);
            }
            actions.add_action(&a);
        }

        da.insert_action_group("actmenu", Some(&actions));

        let n_rows = menu_row_count(&menu);
        swap_ctx_popover(&self.active_ctx_popover, {
            let popover = gtk4::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&da);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.set_has_arrow(false);
            popover.set_position(gtk4::PositionType::Bottom);
            popover.set_size_request(-1, n_rows * 22 + 14);
            popover
        });
        if let Some(ref p) = *self.active_ctx_popover.borrow() {
            p.popup();
        }
    }

    #[allow(dead_code)]
    fn handle_tab_right_click(
        &mut self,
        group_id: core::window::GroupId,
        tab_idx: usize,
        x: f64,
        y: f64,
        _sender: &ComponentSender<Self>,
    ) {
        let da = match self.drawing_area.borrow().as_ref() {
            Some(da) => da.clone(),
            None => return,
        };

        // Build gio::Menu from engine-generated items (single source of truth).
        let items: Vec<core::engine::ContextMenuItem> = {
            let mut engine = self.engine.borrow_mut();
            engine.open_tab_context_menu(group_id, tab_idx, 0, 0);
            let items = engine
                .context_menu
                .as_ref()
                .map(|cm| cm.items.clone())
                .unwrap_or_default();
            engine.close_context_menu();
            items
        };

        let menu = build_gio_menu_from_engine_items(&items, "tabctx");

        // Collect enabled state from engine items keyed by action string.
        let enabled_map: std::collections::HashMap<String, bool> = items
            .iter()
            .map(|it| (it.action.clone(), it.enabled))
            .collect();

        // Build action group
        let actions = gtk4::gio::SimpleActionGroup::new();

        macro_rules! tab_action {
            ($name:expr, $engine:expr, $draw:expr, $body:expr) => {{
                let engine_ref = $engine.clone();
                let draw_ref = $draw.clone();
                let a = gtk4::gio::SimpleAction::new($name, None);
                a.connect_activate(move |_, _| {
                    $body(&engine_ref, &draw_ref);
                });
                if enabled_map.get($name) == Some(&false) {
                    a.set_enabled(false);
                }
                actions.add_action(&a);
            }};
        }

        {
            let engine_ref = self.engine.clone();
            let draw_ref = self.draw_needed.clone();
            let sender = self.sender.clone();
            let a = gtk4::gio::SimpleAction::new("close", None);
            a.connect_activate(move |_, _| {
                let mut e = engine_ref.borrow_mut();
                e.active_group = group_id;
                if let Some(g) = e.editor_groups.get_mut(&group_id) {
                    g.active_tab = tab_idx;
                }
                if e.dirty() {
                    drop(e);
                    let _ = sender.send(Msg::ShowCloseTabConfirm);
                } else {
                    e.close_tab();
                    draw_ref.set(true);
                }
            });
            actions.add_action(&a);
        }

        tab_action!(
            "close_others",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                let mut e = engine_ref.borrow_mut();
                e.active_group = group_id;
                if let Some(g) = e.editor_groups.get_mut(&group_id) {
                    g.active_tab = tab_idx;
                }
                e.close_other_tabs();
                draw_ref.set(true);
            }
        );
        tab_action!(
            "close_right",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                let mut e = engine_ref.borrow_mut();
                e.active_group = group_id;
                if let Some(g) = e.editor_groups.get_mut(&group_id) {
                    g.active_tab = tab_idx;
                }
                e.close_tabs_to_right();
                draw_ref.set(true);
            }
        );
        tab_action!(
            "close_saved",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                let mut e = engine_ref.borrow_mut();
                e.active_group = group_id;
                if let Some(g) = e.editor_groups.get_mut(&group_id) {
                    g.active_tab = tab_idx;
                }
                e.close_saved_tabs();
                draw_ref.set(true);
            }
        );
        tab_action!(
            "copy_path",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                let e = engine_ref.borrow();
                if let Some(path) = e.tab_file_path(group_id, tab_idx) {
                    let text = path.to_string_lossy().to_string();
                    if let Some(ref cb) = e.clipboard_write {
                        let _ = cb(&text);
                    }
                    drop(e);
                    engine_ref.borrow_mut().message = format!("Copied: {text}");
                }
                draw_ref.set(true);
            }
        );
        tab_action!(
            "copy_relative_path",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                let e = engine_ref.borrow();
                if let Some(path) = e.tab_file_path(group_id, tab_idx) {
                    let rel = e.copy_relative_path(&path);
                    if let Some(ref cb) = e.clipboard_write {
                        let _ = cb(&rel);
                    }
                    drop(e);
                    engine_ref.borrow_mut().message = format!("Copied: {rel}");
                }
                draw_ref.set(true);
            }
        );
        tab_action!(
            "reveal",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, _draw_ref: &Rc<Cell<bool>>| {
                let e = engine_ref.borrow();
                if let Some(path) = e.tab_file_path(group_id, tab_idx) {
                    drop(e);
                    engine_ref.borrow().reveal_in_file_manager(&path);
                }
            }
        );
        tab_action!(
            "split_right",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                engine_ref
                    .borrow_mut()
                    .split_window(core::window::SplitDirection::Vertical, None);
                draw_ref.set(true);
            }
        );
        tab_action!(
            "split_down",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                engine_ref
                    .borrow_mut()
                    .split_window(core::window::SplitDirection::Horizontal, None);
                draw_ref.set(true);
            }
        );
        tab_action!(
            "group_split_right",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                engine_ref
                    .borrow_mut()
                    .open_editor_group(core::window::SplitDirection::Vertical);
                draw_ref.set(true);
            }
        );
        tab_action!(
            "group_split_down",
            self.engine,
            self.draw_needed,
            |engine_ref: &Rc<RefCell<Engine>>, draw_ref: &Rc<Cell<bool>>| {
                engine_ref
                    .borrow_mut()
                    .open_editor_group(core::window::SplitDirection::Horizontal);
                draw_ref.set(true);
            }
        );

        da.insert_action_group("tabctx", Some(&actions));

        let n_rows = menu_row_count(&menu);
        swap_ctx_popover(&self.active_ctx_popover, {
            let popover = gtk4::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&da);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.set_has_arrow(false);
            popover.set_position(gtk4::PositionType::Right);
            popover.set_size_request(-1, n_rows * 22 + 14);
            popover
        });
        if let Some(ref p) = *self.active_ctx_popover.borrow() {
            p.popup();
        }
    }

    #[allow(dead_code)]
    fn handle_editor_right_click(&mut self, x: f64, y: f64) {
        let da = match self.drawing_area.borrow().as_ref() {
            Some(da) => da.clone(),
            None => return,
        };

        // Build gio::Menu from engine-generated items (single source of truth).
        let items: Vec<core::engine::ContextMenuItem> = {
            let mut engine = self.engine.borrow_mut();
            engine.open_editor_context_menu(0, 0);
            let items = engine
                .context_menu
                .as_ref()
                .map(|cm| cm.items.clone())
                .unwrap_or_default();
            engine.close_context_menu();
            items
        };

        let menu = build_gio_menu_from_engine_items(&items, "edctx");

        let enabled_map: std::collections::HashMap<String, bool> = items
            .iter()
            .map(|it| (it.action.clone(), it.enabled))
            .collect();

        let actions = gtk4::gio::SimpleActionGroup::new();

        // Helper macro to reduce boilerplate for engine-driven actions.
        macro_rules! add_editor_ctx_action {
            ($name:expr, $engine_rc:expr, $draw_rc:expr, $body:expr) => {{
                let engine_ref = $engine_rc.clone();
                let draw_ref = $draw_rc.clone();
                let a = gtk4::gio::SimpleAction::new($name, None);
                a.connect_activate(move |_, _| {
                    ($body)(&engine_ref, &draw_ref);
                });
                if enabled_map.get($name) == Some(&false) {
                    a.set_enabled(false);
                }
                actions.add_action(&a);
            }};
        }

        add_editor_ctx_action!(
            "goto_definition",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                eng.borrow_mut().lsp_request_definition();
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "goto_references",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                eng.borrow_mut().lsp_request_references();
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "rename_symbol",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                let mut e = eng.borrow_mut();
                e.mode = core::Mode::Command;
                e.command_buffer = "Rename ".to_string();
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "open_changes",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                eng.borrow_mut().open_diff_peek();
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "cut",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                let mut e = eng.borrow_mut();
                if matches!(
                    e.mode,
                    core::Mode::Visual | core::Mode::VisualLine | core::Mode::VisualBlock
                ) {
                    e.yank_visual_selection();
                    if let Some((ref text, _)) = e.registers.get(&'"') {
                        let text = text.clone();
                        if let Some(ref cb) = e.clipboard_write {
                            let _ = cb(&text);
                        }
                    }
                    let mut changed = false;
                    e.delete_visual_selection(&mut changed);
                }
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "copy",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                let mut e = eng.borrow_mut();
                if matches!(
                    e.mode,
                    core::Mode::Visual | core::Mode::VisualLine | core::Mode::VisualBlock
                ) {
                    e.yank_visual_selection();
                    if let Some((ref text, _)) = e.registers.get(&'"') {
                        let text = text.clone();
                        if let Some(ref cb) = e.clipboard_write {
                            let _ = cb(&text);
                        }
                    }
                    e.mode = core::Mode::Normal;
                }
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "paste",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                let mut e = eng.borrow_mut();
                if let Some(ref cb_read) = e.clipboard_read {
                    if let Ok(text) = cb_read() {
                        if !text.is_empty() {
                            e.registers.insert('"', (text, false));
                            let mut changed = false;
                            e.paste_after(&mut changed);
                        }
                    }
                }
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "open_side_vsplit",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                let mut e = eng.borrow_mut();
                if let Some(path) = e.file_path().map(|p| p.to_path_buf()) {
                    e.split_window(core::window::SplitDirection::Vertical, None);
                    let _ = e.open_file_with_mode(&path, core::OpenMode::Permanent);
                }
                dr.set(true);
            }
        );

        add_editor_ctx_action!(
            "command_palette",
            self.engine,
            self.draw_needed,
            |eng: &std::cell::RefCell<core::Engine>, dr: &std::cell::Cell<bool>| {
                eng.borrow_mut()
                    .open_picker(core::engine::PickerSource::Commands);
                dr.set(true);
            }
        );

        da.insert_action_group("edctx", Some(&actions));

        let n_rows = menu_row_count(&menu);
        swap_ctx_popover(&self.active_ctx_popover, {
            let popover = gtk4::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&da);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.set_has_arrow(false);
            popover.set_position(gtk4::PositionType::Right);
            popover.set_size_request(-1, n_rows * 22 + 14);
            popover
        });
        if let Some(ref p) = *self.active_ctx_popover.borrow() {
            p.popup();
        }
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
                if let Some(text) = self.engine.borrow_mut().terminal_copy_selection() {
                    if let Some(ref mut ctx) = self.clipboard {
                        let _ = ctx.set_contents(text);
                    }
                }
            }
            Msg::TerminalPasteClipboard => {
                let text = if let Some(ref mut ctx) = self.clipboard {
                    ctx.get_contents().ok()
                } else {
                    None
                };
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

    fn handle_menu_msg(&mut self, msg: Msg, sender: &ComponentSender<Self>) {
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
                        sender.input(Msg::OpenFileDialog);
                    }
                    "open_folder_dialog" => {
                        sender.input(Msg::OpenFolderDialog);
                    }
                    "open_workspace_dialog" => {
                        self.engine.borrow_mut().open_workspace_from_file();
                        sender.input(Msg::RefreshFileTree);
                    }
                    "save_workspace_as_dialog" => {
                        sender.input(Msg::SaveWorkspaceAsDialog);
                    }
                    "openrecent" => {
                        sender.input(Msg::OpenRecentDialog);
                    }
                    "find" => {
                        self.engine.borrow_mut().open_find_replace();
                        self.draw_needed.set(true);
                    }
                    "quit_menu" => {
                        if self.engine.borrow().has_any_unsaved() {
                            sender.input(Msg::ShowQuitConfirm);
                        } else {
                            self.save_session_and_exit();
                        }
                    }
                    _ => {
                        let engine_action = self.engine.borrow_mut().dispatch_menu_action(&action);
                        match engine_action {
                            EngineAction::Quit | EngineAction::SaveQuit => {
                                sender.input(Msg::QuitConfirmed);
                            }
                            EngineAction::QuitWithUnsaved => {
                                sender.input(Msg::ShowQuitConfirm);
                            }
                            EngineAction::ToggleSidebar => {
                                self.sync_sidebar_from_engine();
                            }
                            EngineAction::OpenTerminal => {
                                sender.input(Msg::NewTerminalTab);
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
                    engine.handle_key(&key_name, key_name.chars().next(), ctrl);
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
                self.focus_editor_if_needed(still_focused);
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
                    let gap = (lh * 0.3).round();
                    let commit_rows = engine.sc_commit_message.split('\n').count().max(1);
                    let commit_h = commit_rows as f64 * lh;
                    let header_end = lh;
                    let commit_top = header_end + gap;
                    let commit_bottom = commit_top + commit_h;
                    let btn_top = commit_bottom + gap;
                    let btn_bottom = btn_top + lh;
                    let section_top = btn_bottom + gap;

                    if y < header_end {
                        engine.sc_commit_input_active = false;
                    } else if y >= commit_top && y < commit_bottom {
                        engine.sc_commit_input_active = true;
                        engine.sc_commit_cursor = engine.sc_commit_message.len();
                    } else if y >= btn_top && y < btn_bottom {
                        engine.sc_commit_input_active = false;
                        if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                            let da_w = da.width() as f64;
                            let margin = 4.0;
                            let btn_w = da_w - margin * 2.0;
                            let rel_x = x_click - margin;
                            if let Some(idx) = Engine::sc_button_hit_test(rel_x, btn_w) {
                                engine.sc_activate_button(idx);
                            }
                        }
                    } else if y >= section_top {
                        engine.sc_commit_input_active = false;
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
                    } else {
                        engine.sc_commit_input_active = false;
                    }
                }
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ScSidebarMotion(mx, my) => {
                // Determine which button (if any) the mouse is over.
                let lh = self.cached_ui_line_height;
                let mut engine = self.engine.borrow_mut();
                let gap = (lh * 0.3).round();
                let commit_rows = engine.sc_commit_message.split('\n').count().max(1);
                let commit_h = commit_rows as f64 * lh;
                // Button row Y range: after header + gap + commit + gap
                let btn_top = lh + gap + commit_h + gap;
                let btn_bottom = btn_top + lh;
                let old = engine.sc_button_hovered;
                if mx < 0.0 || my < btn_top || my >= btn_bottom {
                    engine.sc_button_hovered = None;
                } else if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    let da_w = da.width() as f64;
                    let margin = 4.0;
                    let btn_w = da_w - margin * 2.0;
                    let rel_x = mx - margin;
                    engine.sc_button_hovered = Engine::sc_button_hit_test(rel_x, btn_w);
                } else {
                    engine.sc_button_hovered = None;
                }
                if engine.sc_button_hovered != old {
                    drop(engine);
                    if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                } else {
                    // Panel hover dwell tracking for SC items.
                    let item_height = (lh * 1.4).round();
                    let btn_pad = gap;
                    let section_top = btn_bottom + btn_pad;
                    if mx >= 0.0 && my >= section_top {
                        // Accumulator walk matching draw_source_control_panel layout.
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

                        let mut y_off = section_top;
                        let mut flat_idx = 0usize;
                        let mut hit_flat: Option<usize> = None;

                        // Walk each section: header(lh) + items(item_height) if expanded
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
                            // Header
                            if my >= y_off && my < y_off + lh {
                                hit_flat = Some(flat_idx);
                                break;
                            }
                            y_off += lh;
                            flat_idx += 1;
                            if sec.expanded {
                                for _ in 0..sec.count {
                                    if my >= y_off && my < y_off + item_height {
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
                            // Worktrees header
                            if my >= y_off && my < y_off + lh {
                                hit_flat = Some(flat_idx);
                            }
                            y_off += lh;
                            flat_idx += 1;
                            if hit_flat.is_none() && engine.sc_sections_expanded[2] {
                                for _ in 0..wt_count {
                                    if my >= y_off && my < y_off + item_height {
                                        hit_flat = Some(flat_idx);
                                        break;
                                    }
                                    y_off += item_height;
                                    flat_idx += 1;
                                }
                            }
                        }
                        if hit_flat.is_none() {
                            // Log header
                            if my >= y_off && my < y_off + lh {
                                hit_flat = Some(flat_idx);
                            }
                            y_off += lh;
                            flat_idx += 1;
                            if hit_flat.is_none() && engine.sc_sections_expanded[3] {
                                for _ in 0..log_count {
                                    if my >= y_off && my < y_off + item_height {
                                        hit_flat = Some(flat_idx);
                                        break;
                                    }
                                    y_off += item_height;
                                    flat_idx += 1;
                                }
                            }
                        }

                        if let Some(fi) = hit_flat {
                            if engine.panel_hover_mouse_move("source_control", "", fi) {
                                drop(engine);
                                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                                    da.queue_draw();
                                }
                            }
                        } else {
                            engine.dismiss_panel_hover();
                        }
                    } else if mx < 0.0 {
                        // Mouse left the panel. Use delayed dismiss so the overlay's
                        // motion controller can cancel it if the mouse enters the popup.
                        engine.dismiss_panel_hover();
                    }
                }
            }
            Msg::ScKey(key_name, ctrl) => {
                let mut engine = self.engine.borrow_mut();
                if engine.dialog.is_some() {
                    engine.handle_key(&key_name, key_name.chars().next(), ctrl);
                    drop(engine);
                    self.focus_editor_if_needed(false);
                    self.draw_needed.set(true);
                    return;
                }
                let (mapped, unicode) = map_gtk_key_with_unicode(key_name.as_str());
                engine.dispatch_sc_sidebar_key_unified(mapped, ctrl, unicode);
                let still_focused = engine.sc_has_focus;
                drop(engine);
                self.focus_editor_if_needed(still_focused);
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
                    engine.handle_key(mapped, unicode, false);
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
                self.focus_editor_if_needed(still_focused && !has_dialog);
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
                    engine.handle_key(mapped, unicode, ctrl);
                } else {
                    engine.handle_settings_key(mapped, ctrl, unicode);
                }
                let still_focused = engine.settings_has_focus;
                drop(engine);
                self.focus_editor_if_needed(still_focused);
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
                    engine.handle_key(mapped, unicode, false);
                    drop(engine);
                    self.focus_editor_if_needed(false);
                } else if engine.ext_panel_input_active {
                    engine.handle_ext_panel_input_key(mapped, false, unicode);
                    drop(engine);
                } else {
                    engine.handle_ext_panel_key(mapped, false, unicode);
                    let still_focused = engine.ext_panel_has_focus;
                    drop(engine);
                    self.focus_editor_if_needed(still_focused);
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
                let panel_name = if let SidebarPanel::ExtPanel(ref name) = self.active_panel {
                    name.clone()
                } else {
                    return;
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
                    let mut engine = self.engine.borrow_mut();
                    engine.handle_key(&key_name, key_name.chars().next(), ctrl);
                    drop(engine);
                    self.focus_editor_if_needed(false);
                    self.draw_needed.set(true);
                    return;
                }
                // Ctrl-V: paste from system clipboard into AI input.
                if ctrl && key_name == "v" {
                    if let Some(ref mut ctx) = self.clipboard {
                        let text = ctx.get_contents().unwrap_or_default();
                        if !text.is_empty() {
                            self.engine.borrow_mut().ai_insert_text(&text);
                        }
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
                self.focus_editor_if_needed(still_focused);
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

    /// Sync local sidebar cache fields from engine AppShell state, then
    /// update GTK widgets.
    fn sync_sidebar_from_engine(&mut self) {
        let engine = self.engine.borrow();
        self.sidebar_visible = engine.app_shell.sidebar_visible();
        // Extension panels bypass AppShell (no dynamic registration).
        // If an ext panel is active, preserve it instead of falling back
        // to AppShell's panel ID (which would default to Explorer).
        if let Some(ref name) = engine.ext_panel_active {
            self.active_panel = SidebarPanel::ExtPanel(name.clone());
        } else {
            self.active_panel = engine
                .app_shell
                .active_panel_id()
                .map(|id| SidebarPanel::from_panel_id(id.as_str()))
                .unwrap_or(SidebarPanel::Explorer);
        }
        drop(engine);
        self.sync_sidebar_widgets();
    }

    /// Update GTK widget visibility (revealer + panel boxes), grab focus on
    /// the active panel DA, and mirror to the activity bar draw callback.
    /// Reads from `self.sidebar_visible` and `self.active_panel` (local cache).
    fn sync_sidebar_widgets(&mut self) {
        let show = self.sidebar_visible;
        let p = self.active_panel.clone();

        if let Some(ref r) = *self.sidebar_revealer.borrow() {
            r.set_reveal_child(show);
        }
        for (which, panel_ref) in [
            (SidebarPanel::Explorer, &self.explorer_panel_box),
            (SidebarPanel::Debug, &self.debug_panel_box),
            (SidebarPanel::Git, &self.git_panel_box),
            (SidebarPanel::Extensions, &self.ext_panel_box),
            (SidebarPanel::Settings, &self.settings_panel_box),
            (SidebarPanel::Ai, &self.ai_panel_box_ref),
        ] {
            if let Some(ref b) = *panel_ref.borrow() {
                b.set_visible(show && p == which);
            }
        }
        if let Some(ref b) = *self.ext_dyn_panel_box.borrow() {
            b.set_visible(show && matches!(p, SidebarPanel::ExtPanel(_)));
        }
        if show {
            match p {
                SidebarPanel::Git => {
                    if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                SidebarPanel::Extensions => {
                    if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                SidebarPanel::Debug => {
                    if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                SidebarPanel::Ai => {
                    if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                SidebarPanel::ExtPanel(_) => {
                    if let Some(ref da) = *self.ext_dyn_panel_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                SidebarPanel::Settings => {
                    if let Some(ref da) = *self.settings_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                SidebarPanel::Explorer => {
                    if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                        da.grab_focus();
                    }
                }
                _ => {}
            }
        }
        if let Some(ref da) = *self.activity_bar_da_ref.borrow() {
            da.queue_draw();
        }
        self.draw_needed.set(true);
    }

    fn handle_sidebar_panel_msg(&mut self, msg: Msg, _sender: &ComponentSender<Self>) {
        match msg {
            Msg::ToggleSidebar => {
                self.engine.borrow_mut().toggle_sidebar();
                self.sync_sidebar_from_engine();
            }
            Msg::SwitchPanel(panel) => {
                if let SidebarPanel::ExtPanel(ref name) = panel {
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
                        engine.ext_panel_active = Some(name.clone());
                        if !already {
                            engine.ext_panel_selected = 0;
                            engine.plugin_event("panel_focus", name);
                        }
                    }
                    engine.session.explorer_visible = engine.app_shell.sidebar_visible();
                    let _ = engine.session.save();
                    drop(engine);
                    // Sync local cache — active_panel stays as ExtPanel for widget visibility.
                    // Do NOT call sync_sidebar_from_engine() — it would overwrite active_panel
                    // with the engine's fixed-panel ID, losing the ExtPanel variant.
                    self.sidebar_visible = self.engine.borrow().app_shell.sidebar_visible();
                    self.active_panel = panel;
                    self.sync_sidebar_widgets();
                } else if let Some(panel_id) = panel.to_panel_id() {
                    // Clear ext panel state when switching to a built-in panel.
                    {
                        let mut engine = self.engine.borrow_mut();
                        engine.ext_panel_has_focus = false;
                        engine.ext_panel_active = None;
                        engine.toggle_sidebar_panel(panel_id);
                    }
                    self.sync_sidebar_from_engine();
                }
            }
            _ => unreachable!(),
        }
    }

    fn handle_explorer_msg(&mut self, msg: Msg, sender: &ComponentSender<Self>) {
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
                        sender.input(Msg::RefreshFileTree);

                        // Open the new file
                        sender.input(Msg::OpenFileFromSidebar(file_path));
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
                        sender.input(Msg::RefreshFileTree);
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
                sender.input(Msg::ExplorerAction("new_file".to_string()));
            }
            Msg::StartInlineNewFolder(_) => {
                sender.input(Msg::ExplorerAction("new_folder".to_string()));
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
                self.sidebar_visible = true;
                self.active_panel = SidebarPanel::Explorer;
                self.engine.borrow_mut().explorer_has_focus = true;
                if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                    da.grab_focus();
                    // `draw_needed` only queues the editor DA / menu bar.
                    // The explorer DA needs its own `queue_draw` to re-run
                    // the draw callback so the selection highlight
                    // appears now that `explorer_has_focus = true`.
                    da.queue_draw();
                }
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
                    self.sidebar_visible = true;
                    self.active_panel = SidebarPanel::Explorer;
                    self.engine.borrow_mut().explorer_has_focus = true;
                    if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                        da.grab_focus();
                        da.queue_draw();
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::ToggleFocusSearch => {
                if self.active_panel == SidebarPanel::Search && self.sidebar_visible {
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                } else {
                    self.active_panel = SidebarPanel::Search;
                    self.sidebar_visible = true;
                    self.engine.borrow_mut().search_set_focus(true);
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
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
                self.handle_explorer_da_key(key_name, unicode, ctrl, sender);
                self.queue_explorer_draw();
                self.draw_needed.set(true);
            }
            Msg::ExplorerClick { x, y, n_press } => {
                self.handle_explorer_da_click(x, y, n_press, sender);
            }
            Msg::ExplorerRightClick { x, y } => {
                self.handle_explorer_da_right_click(x, y, sender);
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
            Msg::PromptRenameFile(_) | Msg::PromptNewFile(_) | Msg::PromptNewFolder(_) => {
                // Explorer CRUD now uses inline editing via TreeController.
                // These dialog paths are kept as Msg variants for backwards
                // compatibility but nothing sends them.
            }
            _ => unreachable!(),
        }
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

    fn handle_explorer_da_key(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        sender: &ComponentSender<Self>,
    ) {
        // When an engine dialog is active (delete confirmation), route
        // keys to the dialog handler, not the explorer dispatch.
        if self.engine.borrow().dialog.is_some() {
            let mapped = map_gtk_key_name(&key_name);
            self.engine.borrow_mut().handle_key(mapped, unicode, false);
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
            sender.input(Msg::ToggleSidebar);
            return;
        }
        if printable == pk_explorer {
            sender.input(Msg::ToggleFocusExplorer);
            return;
        }
        if printable == pk_search {
            sender.input(Msg::ToggleFocusSearch);
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
                // GTK doesn't have a toolbar focus concept — treat as no-op
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

    fn explorer_jump_scroll(&self, click_y: f64, sb_y: f64, sb_h: f64) {
        let engine = self.engine.borrow();
        let total = engine.explorer_rows.len();
        let viewport = engine.explorer_viewport_rows.get();
        let max_scroll = total.saturating_sub(viewport);
        if max_scroll == 0 || sb_h <= 0.0 {
            return;
        }
        let thumb_ratio = (viewport as f64 / total as f64).min(1.0);
        let thumb_h = (sb_h * thumb_ratio).max(1.0);
        let effective_track = (sb_h - thumb_h).max(1.0);
        let scroll_top = engine.explorer_tree.borrow().scroll_offset();
        let scroll_ratio = (scroll_top as f64 / max_scroll.max(1) as f64).clamp(0.0, 1.0);
        let thumb_top = sb_y + scroll_ratio * effective_track;
        if click_y >= thumb_top && click_y < thumb_top + thumb_h {
            return;
        }
        let rel = ((click_y - sb_y) / effective_track).clamp(0.0, 1.0);
        let new_top = (rel * max_scroll as f64).round() as usize;
        engine
            .explorer_tree
            .borrow_mut()
            .set_scroll_offset(new_top.min(max_scroll));
    }

    fn handle_explorer_da_click(
        &mut self,
        x: f64,
        y: f64,
        n_press: i32,
        sender: &ComponentSender<Self>,
    ) {
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.grab_focus();
        }
        self.engine.borrow_mut().explorer_has_focus = true;

        if let Some((sb_x, sb_y, sb_w, sb_h)) = self.explorer_scrollbar_rect.get() {
            if x >= sb_x && x <= sb_x + sb_w && y >= sb_y && y <= sb_y + sb_h {
                self.explorer_jump_scroll(y, sb_y, sb_h);
                self.queue_explorer_draw();
                return;
            }
        }

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
            sender.input(Msg::OpenFileFromSidebar(path));
        } else {
            sender.input(Msg::PreviewFileFromSidebar(path));
        }
    }

    fn handle_explorer_da_right_click(&mut self, x: f64, y: f64, sender: &ComponentSender<Self>) {
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
        self.queue_explorer_draw();
        self.show_explorer_context_menu(x, y, target, is_dir, sender);
    }

    fn show_explorer_context_menu(
        &self,
        x: f64,
        y: f64,
        target: PathBuf,
        is_dir: bool,
        sender: &ComponentSender<Self>,
    ) {
        let da: gtk4::DrawingArea = match self.explorer_sidebar_da_ref.borrow().as_ref() {
            Some(da) => da.clone(),
            None => return,
        };
        // Build the engine-driven context menu items (for enabled state).
        self.engine
            .borrow_mut()
            .open_explorer_context_menu(target.clone(), is_dir, 0, 0);
        let items: Vec<core::engine::ContextMenuItem> = self
            .engine
            .borrow()
            .context_menu
            .as_ref()
            .map(|cm| cm.items.clone())
            .unwrap_or_default();
        self.engine.borrow_mut().close_context_menu();
        let menu = build_gio_menu_from_engine_items(&items, "ctx");
        let ctx_enabled: std::collections::HashMap<String, bool> = items
            .iter()
            .map(|it| (it.action.clone(), it.enabled))
            .collect();

        let actions = gtk4::gio::SimpleActionGroup::new();
        let add_action = |actions: &gtk4::gio::SimpleActionGroup, a: &gtk4::gio::SimpleAction| {
            if ctx_enabled.get(a.name().as_str()) == Some(&false) {
                a.set_enabled(false);
            }
            actions.add_action(a);
        };

        {
            let s = sender.input_sender().clone();
            let a = gtk4::gio::SimpleAction::new("new_file", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::ExplorerAction("new_file".to_string())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let a = gtk4::gio::SimpleAction::new("new_folder", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::ExplorerAction("new_folder".to_string())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let a = gtk4::gio::SimpleAction::new("rename", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::ExplorerAction("rename".to_string())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let a = gtk4::gio::SimpleAction::new("delete", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::ExplorerAction("delete".to_string())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("copy_path", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::CopyPath(t.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("copy_relative_path", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::CopyRelativePath(t.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("reveal", None);
            a.connect_activate(move |_, _| {
                let dir = if t.is_dir() {
                    t.clone()
                } else {
                    t.parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                };
                let _ = std::process::Command::new("xdg-open")
                    .arg(&dir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("select_for_diff", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::SelectForDiff(t.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("diff_with_selected", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::DiffWithSelected(t.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("open_side", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::OpenSide(t.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let eng = self.engine.clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("open_side_vsplit", None);
            a.connect_activate(move |_, _| {
                let mut e = eng.borrow_mut();
                e.split_window(crate::core::window::SplitDirection::Vertical, None);
                let _ = e.open_file_with_mode(&t, crate::core::OpenMode::Permanent);
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("open_terminal", None);
            a.connect_activate(move |_, _| {
                let dir = if t.is_dir() {
                    t.clone()
                } else {
                    t.parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                };
                s.send(Msg::OpenTerminalAt(dir)).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let a = gtk4::gio::SimpleAction::new("find_in_folder", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::ToggleFocusSearch).ok();
            });
            add_action(&actions, &a);
        }

        let n_rows = menu_row_count(&menu);
        let popover_parent: gtk4::Widget = da.clone().upcast();
        popover_parent.insert_action_group("ctx", Some(&actions));
        swap_ctx_popover(&self.active_ctx_popover, {
            let popover = gtk4::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&popover_parent);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.set_has_arrow(false);
            popover.set_position(gtk4::PositionType::Right);
            popover.set_size_request(-1, n_rows * 22 + 14);
            popover
        });
        if let Some(ref p) = *self.active_ctx_popover.borrow() {
            p.popup();
        }
    }

    /// Show a simple modal dialog with a text entry for rename /
    /// new-file / new-folder flows. Phase A.2b-2 replaced the native
    /// `gtk4::TreeView` inline cell editor with this fallback. On OK the
    /// closure fires `on_confirm(name)`. Empty names close the dialog
    /// silently.
    #[allow(dead_code)]
    fn prompt_for_name(
        &self,
        title: &str,
        prompt: &str,
        initial: &str,
        on_confirm: Box<dyn Fn(String)>,
    ) {
        let dialog = gtk4::Dialog::with_buttons(
            Some(title),
            Some(&self.window),
            gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("Cancel", gtk4::ResponseType::Cancel),
                ("OK", gtk4::ResponseType::Ok),
            ],
        );
        dialog.set_default_response(gtk4::ResponseType::Ok);
        let content = dialog.content_area();
        content.set_margin_top(8);
        content.set_margin_bottom(8);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.set_spacing(6);
        let label = gtk4::Label::new(Some(prompt));
        label.set_halign(gtk4::Align::Start);
        content.append(&label);
        let entry = gtk4::Entry::new();
        entry.set_text(initial);
        entry.set_activates_default(true);
        // Pre-select the stem (up to the last dot) so the user can type
        // a new name while keeping the extension.
        if !initial.is_empty() {
            let stem_end = initial
                .rfind('.')
                .filter(|&i| i > 0)
                .unwrap_or(initial.len()) as i32;
            let entry_for_select = entry.clone();
            gtk4::glib::idle_add_local_once(move || {
                entry_for_select.select_region(0, stem_end);
            });
        }
        content.append(&entry);
        let entry_for_response = entry.clone();
        dialog.connect_response(move |d, resp| {
            if resp == gtk4::ResponseType::Ok {
                let name = entry_for_response.text().trim().to_string();
                if !name.is_empty() {
                    on_confirm(name);
                }
            }
            d.close();
        });
        dialog.show();
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

    fn handle_file_ops_msg(&mut self, msg: Msg, sender: &ComponentSender<Self>) {
        match msg {
            Msg::RenameFile(old_path, new_name) => {
                let result = self.engine.borrow_mut().rename_file(&old_path, &new_name);
                match result {
                    Ok(()) => {
                        self.engine.borrow_mut().message = format!("Renamed to '{}'", new_name);
                        sender.input(Msg::RefreshFileTree);
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
                engine.session.explorer_visible = self.sidebar_visible;
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

    fn handle_dialog_msg(&mut self, msg: Msg, sender: &ComponentSender<Self>) {
        match msg {
            Msg::WindowMinimize => {
                self.window.minimize();
            }
            Msg::WindowMaximize => {
                if self.window.is_maximized() {
                    self.window.unmaximize();
                } else {
                    self.window.maximize();
                }
            }
            Msg::WindowClose => {
                self.window.close();
            }
            Msg::OpenFileDialog => {
                let engine = self.engine.clone();
                let sender2 = sender.input_sender().clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Open File");
                let win = self.window.clone();
                dialog.open(Some(&win), gtk4::gio::Cancellable::NONE, move |result| {
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
                let sender2 = sender.input_sender().clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Open Folder");
                dialog.set_accept_label(Some("Open Folder"));
                let win = self.window.clone();
                dialog.select_folder(Some(&win), gtk4::gio::Cancellable::NONE, move |result| {
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
                sender.input(Msg::RefreshFileTree);
                self.draw_needed.set(true);
            }
            Msg::SaveWorkspaceAsDialog => {
                let engine = self.engine.clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Save Workspace As");
                dialog.set_initial_name(Some(".vimcode-workspace"));
                let win = self.window.clone();
                dialog.save(Some(&win), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = gtk4::prelude::FileExt::path(&file) {
                            engine.borrow_mut().save_workspace_as(&path);
                        }
                    }
                });
                self.draw_needed.set(true);
            }
            Msg::OpenRecentDialog => {
                let paths: Vec<std::path::PathBuf> = self
                    .engine
                    .borrow()
                    .session
                    .recent_workspaces
                    .iter()
                    .rev()
                    .cloned()
                    .collect();
                if paths.is_empty() {
                    self.engine.borrow_mut().message = "No recent workspaces".to_string();
                } else {
                    let engine = self.engine.clone();
                    let sender2 = sender.input_sender().clone();
                    let dialog = gtk4::Dialog::with_buttons(
                        Some("Open Recent Workspace"),
                        Some(&self.window),
                        gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
                        &[("Cancel", gtk4::ResponseType::Cancel)],
                    );
                    let content = dialog.content_area();
                    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
                    content.append(&vbox);
                    for (idx, path) in paths.iter().enumerate() {
                        let label = path.to_string_lossy().into_owned();
                        let btn = gtk4::Button::with_label(&label);
                        let dialog_clone = dialog.clone();
                        let engine_clone = engine.clone();
                        let sender_clone = sender2.clone();
                        let path_clone = path.clone();
                        btn.connect_clicked(move |_| {
                            let _ = idx; // suppress unused var warning
                            engine_clone.borrow_mut().open_folder(&path_clone);
                            sender_clone.send(Msg::RefreshFileTree).ok();
                            dialog_clone.close();
                        });
                        vbox.append(&btn);
                    }
                    dialog.show();
                }
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
            gtk_terminal_target_maximize_rows(&self.engine.borrow(), da.height() as f64, lh)
        } else {
            10
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
/// and `gtk_editor_bottom`, so window resize automatically re-derives the
/// maximized panel size.
pub(super) fn gtk_terminal_target_maximize_rows(
    engine: &Engine,
    da_height: f64,
    line_height: f64,
) -> u16 {
    // Convert GTK's pixel-based chrome into row units, then hand off to the
    // shared `PanelChromeDesc::max_panel_content_rows`. The editor tab row is
    // `1.6 * lh`, which rounds up to 2 row-units when reserved — the
    // clamping in `max_panel_content_rows` absorbs that ≤0.4 lh of slack.
    let lh = line_height.max(1.0);
    let viewport_rows = (da_height / lh).floor() as u16;
    let per_window = engine.settings.window_status_line;
    let has_separated = per_window && !engine.settings.status_line_above_terminal;
    crate::core::engine::PanelChromeDesc {
        viewport_rows,
        menu_rows: 0, // GTK menu bar lives outside the DrawingArea.
        quickfix_rows: if engine.quickfix_open && !engine.quickfix_items.is_empty() {
            6
        } else {
            0
        },
        debug_toolbar_rows: if engine.debug_toolbar_visible { 1 } else { 0 },
        wildmenu_rows: if engine.wildmenu_items.is_empty() {
            0
        } else {
            1
        },
        // 1.6 lh for tab row → reserve 2 row-units (ceiling).
        tab_bar_rows: 2,
        separated_status_rows: if has_separated { 1 } else { 0 },
        // per-window: cmd(1); otherwise: status + cmd (2).
        status_cmd_rows: if per_window { 1 } else { 2 },
        panel_chrome_rows: 2,
        min_content_rows: 5,
    }
    .max_panel_content_rows()
}

fn gtk_editor_bottom(engine: &Engine, _da_width: f64, da_height: f64, line_height: f64) -> f64 {
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let bp_open = engine.terminal_open || engine.bottom_panel_open;
    let has_separated = engine.settings.window_status_line
        && !engine.settings.status_line_above_terminal
        && bp_open;
    let global_status_rows = if engine.settings.window_status_line {
        1.0
    } else {
        2.0
    };
    let status_bar_height = line_height * global_status_rows + wildmenu_px;
    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        6.0 * line_height
    } else {
        0.0
    };
    let term_px = if bp_open {
        let target = gtk_terminal_target_maximize_rows(engine, da_height, line_height);
        (engine.effective_terminal_panel_rows(target) as f64 + 2.0) * line_height
    } else {
        0.0
    };
    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };
    let separated_status_px = if has_separated {
        line_height // status row below terminal (cmd already in status_bar_height)
    } else {
        0.0
    };
    da_height - status_bar_height - debug_toolbar_px - qf_px - term_px - separated_status_px
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
        gtk4::glib::ffi::g_log_set_handler(
            c"Gtk".as_ptr(),
            gtk4::glib::ffi::G_LOG_LEVEL_CRITICAL,
            Some(suppress_css_node_warning),
            std::ptr::null_mut(),
        );
    }
    let gtk_app = gtk4::Application::builder()
        .application_id("com.vimcode.VimCode")
        .flags(
            gtk4::gio::ApplicationFlags::NON_UNIQUE
                | gtk4::gio::ApplicationFlags::HANDLES_COMMAND_LINE,
        )
        .build();
    gtk_app.connect_command_line(|app, _| {
        // GTK4 default is to warp the slider to the click position on
        // a trough left-click — that means clicking near the bottom of
        // the editor scrollbar in a long file jumps thousands of lines
        // away from the cursor. Disabling makes left-click page by
        // `page_increment` (one viewport, since we set that per-frame
        // alongside `page_size`); middle-click / shift-click retain
        // the warp behaviour for users who want it.
        if let Some(settings) = gtk4::Settings::default() {
            settings.set_gtk_primary_button_warps_slider(false);
        }
        app.activate();
        0
    });
    // Unbind F10 from GTK's built-in "activate-menubar" action so it
    // reaches our key controller (used for DAP step-over).
    gtk_app.set_accels_for_action("win.show-help-overlay", &[]);
    gtk_app.set_accels_for_action("win.activate-menubar", &[]);
    let app = RelmApp::from_app(gtk_app);
    app.run::<App>(file_path);
}
