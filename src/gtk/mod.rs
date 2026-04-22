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
use core::lsp::DiagnosticSeverity;
use core::settings::LineNumberMode;
use core::{Engine, GitLineStatus, OpenMode, WindowRect};
use render::{
    build_screen_layout, CommandLineData, CursorShape, RenderedWindow, SelectionKind,
    SelectionRange, StyledSpan, TabInfo, Theme,
};

use copypasta_ext::ClipboardProviderExt;
use std::collections::HashMap;

mod click;
mod css;
mod draw;
mod explorer;
mod quadraui_gtk;
mod util;

use click::*;
use css::*;
use draw::*;
use explorer::ExplorerState;
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

type TabSlotMap = HashMap<usize, Vec<(f64, f64)>>;

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

/// Return type of draw_tab_bar: (tab_slot_positions, diff_btn_positions,
/// split_btn_widths, visible_tab_count, action_btn, correct_scroll_offset).
/// `correct_scroll_offset` is the offset that would make the active tab
/// visible given THIS frame's pixel measurements; the caller compares to
/// the engine's stored value and triggers a repaint if they differ.
type TabBarDrawResult = (
    Vec<(f64, f64)>,
    Option<(f64, f64, f64, f64, f64, f64)>,
    Option<(f64, f64)>,
    usize,
    Option<(f64, f64)>, // action menu button (start_x, end_x)
    usize,              // correct_scroll_offset (per-group, in pixels-aware units)
);

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
    activity_bar_active_panel: Rc<RefCell<Option<Rc<RefCell<SidebarPanel>>>>>,
    /// Flat row model + expand / selection state backing the explorer DA.
    explorer_state: Rc<RefCell<ExplorerState>>,
    /// Row height actually used by the most recent `draw_explorer_panel`
    /// call. The draw callback writes this each frame from the same Pango
    /// context it renders with, so click and scroll handlers hit-test with
    /// byte-exact row math (Cairo-backed Pango contexts can report
    /// line-heights that drift from `cached_ui_line_height` on HiDPI).
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
    search_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
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
    /// Status text shown below the project search input ("N matches in M files").
    project_search_status: String,
    /// Ref to the search results ListBox so we can rebuild it after each search.
    search_results_list: Rc<RefCell<Option<gtk4::ListBox>>>,
    /// Last content written to system clipboard.
    /// Used to avoid redundant writes on every keystroke.
    last_clipboard_content: Option<String>,
    /// System clipboard context (copypasta-ext).  None if unavailable.
    // Box<dyn ClipboardProviderExt> is !Send; GTK App lives on main thread only.
    clipboard: Option<Box<dyn ClipboardProviderExt>>,
    /// Drag state while the user drags a Cairo horizontal scrollbar thumb.
    h_sb_dragging: Option<HScrollDragState>,
    /// True while the mouse cursor is over any horizontal scrollbar track.
    h_sb_hovered: bool,
    /// Which tab close button (×) the mouse is over: (group_id.0, tab_idx).
    tab_close_hover: Option<(usize, usize)>,
    /// Cached tab slot widths per group, populated during draw_tab_bar for click hit-testing.
    /// Key = group_id.0 (or usize::MAX for single-group mode), Value = cumulative x positions.
    tab_slot_positions: Rc<RefCell<TabSlotMap>>,
    /// Cached diff toolbar button pixel positions, populated during draw_tab_bar.
    diff_btn_map: Rc<RefCell<DiffBtnMap>>,
    split_btn_map: Rc<RefCell<SplitBtnMap>>,
    action_btn_map: Rc<RefCell<ActionBtnMap>>,
    /// Cached per-window status bar segment hit zones from draw_window_status_bar.
    status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Cached nav arrow pixel hit rects from draw_menu_bar: (back_x, back_end, fwd_x, fwd_end, unit_end).
    #[allow(dead_code, clippy::type_complexity)]
    nav_arrow_rects: Rc<RefCell<(f64, f64, f64, f64, f64)>>,
    /// True while the user is dragging the terminal panel's scrollbar thumb.
    terminal_sb_dragging: bool,
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
    /// Last time check_file_changes() was called for auto-reload detection.
    last_file_check: std::time::Instant,
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
    /// Link hit rects populated during editor hover popup draw: (x, y, w, h, url).
    #[allow(clippy::type_complexity)]
    editor_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>>,
    /// Cached line height shared with menu_dropdown_da draw/click closures.
    menu_dd_line_height: Rc<Cell<f64>>,
    /// CSS provider registered with the GTK display — updated when colorscheme changes.
    css_provider: gtk4::CssProvider,
    /// Colorscheme name at the time the CSS was last applied.
    last_colorscheme: String,
    /// Set to true when VimCode writes settings.json itself (via SettingChanged or :set).
    /// SettingsFileChanged skips the reload if this flag is true (we already have the
    /// correct in-memory state) and clears the flag.  Prevents the GIO file watcher from
    /// redundantly reloading settings that VimCode just saved.
    settings_self_save: bool,
    /// Active context-menu popover (explorer or tab). Kept alive so we can
    /// unparent it before creating a new one (avoids GTK CSS node assertions).
    active_ctx_popover: Rc<RefCell<Option<gtk4::PopoverMenu>>>,
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
    active: &SidebarPanel,
    theme: &crate::render::Theme,
) -> quadraui::ActivityBar {
    let fixed = [
        (
            SidebarPanel::Explorer,
            icons::EXPLORER.nerd,
            "Explorer (Ctrl+Shift+E)",
            "activity:explorer",
        ),
        (
            SidebarPanel::Search,
            icons::SEARCH_COD.nerd,
            "Search (Ctrl+Shift+F)",
            "activity:search",
        ),
        (
            SidebarPanel::Debug,
            icons::DEBUG.nerd,
            "Debug",
            "activity:debug",
        ),
        (
            SidebarPanel::Git,
            icons::GIT_BRANCH.nerd,
            "Source Control",
            "activity:git",
        ),
        (
            SidebarPanel::Extensions,
            icons::EXTENSIONS.nerd,
            "Extensions",
            "activity:extensions",
        ),
        (
            SidebarPanel::Ai,
            icons::AI_CHAT.nerd,
            "AI Assistant",
            "activity:ai",
        ),
    ];

    let mut top: Vec<quadraui::ActivityItem> = fixed
        .iter()
        .map(|(panel, icon, tooltip, id)| quadraui::ActivityItem {
            id: quadraui::WidgetId::new(*id),
            icon: (*icon).to_string(),
            tooltip: (*tooltip).to_string(),
            is_active: active == panel,
            is_keyboard_selected: false,
        })
        .collect();

    // Dynamic extension panels (sorted by name).
    let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
    ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
    for panel in ext_panels {
        let is_active = matches!(active, SidebarPanel::ExtPanel(n) if n == &panel.name);
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
        is_active: matches!(active, SidebarPanel::Settings),
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
        "Tab" | "ISO_Left_Tab" => "Tab",
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

/// Drag state for a Cairo-drawn horizontal scrollbar.
struct HScrollDragState {
    window_id: core::WindowId,
    drag_start_x: f64,
    scroll_left_at_start: usize,
    /// pixels per one column unit: `(track_w - thumb_w) / scroll_range`
    px_per_col: f64,
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
    /// Open a specific top-level menu dropdown by index.
    OpenMenu(usize),
    /// Close the open menu dropdown.
    CloseMenu,
    /// Navigate back in MRU tab history.
    MruNavBack,
    /// Navigate forward in MRU tab history.
    MruNavForward,
    /// Open the Command Center picker (search box click).
    OpenCommandCenter,
    /// Activate a menu item: (menu_idx, item_idx, action_str).
    MenuActivateItem(usize, usize, String),
    /// Highlight a menu dropdown item by index (mouse hover).
    MenuHighlight(Option<usize>),
    /// Click in the debug sidebar DrawingArea (x, y coordinates in pixels).
    DebugSidebarClick(f64, f64),
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
    /// Key press in the Extensions sidebar DrawingArea (key_name, unicode).
    ExtSidebarKey(String, Option<char>),
    /// Click in the Extensions sidebar DrawingArea (x, y, n_press).
    ExtSidebarClick(f64, f64, i32),
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

                        // Search panel
                        #[name = "search_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: model.active_panel == SidebarPanel::Search,

                            // Header
                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Horizontal,
                                set_css_classes: &["sidebar-header"],
                                gtk4::Label {
                                    set_text: " SEARCH",
                                    set_halign: gtk4::Align::Start,
                                    set_hexpand: true,
                                    set_css_classes: &["sidebar-title"],
                                },
                            },

                            // Search input row
                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Horizontal,
                                set_margin_top: 6,
                                set_margin_bottom: 4,
                                set_margin_start: 6,
                                set_margin_end: 6,

                                #[name = "project_search_entry"]
                                gtk4::Entry {
                                    set_hexpand: true,
                                    set_width_chars: 1,
                                    set_placeholder_text: Some("Search files…"),

                                    connect_changed[sender] => move |entry| {
                                        sender.input(Msg::ProjectSearchQueryChanged(
                                            entry.text().to_string(),
                                        ));
                                    },

                                    connect_activate[sender] => move |_| {
                                        sender.input(Msg::ProjectSearchSubmit);
                                    },
                                },
                            },

                            // Toggle buttons row (Aa / Ab| / .*)
                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Horizontal,
                                set_margin_start: 6,
                                set_margin_end: 6,
                                set_margin_bottom: 4,
                                set_spacing: 4,

                                gtk4::ToggleButton {
                                    set_label: "Aa",
                                    set_tooltip_text: Some("Match Case"),
                                    set_css_classes: &["search-toggle-btn"],

                                    #[watch]
                                    set_active: model.engine.borrow().project_search_options.case_sensitive,

                                    connect_clicked[sender] => move |_| {
                                        sender.input(Msg::ProjectSearchToggleCase);
                                    },
                                },

                                gtk4::ToggleButton {
                                    set_label: "Ab|",
                                    set_tooltip_text: Some("Match Whole Word"),
                                    set_css_classes: &["search-toggle-btn"],

                                    #[watch]
                                    set_active: model.engine.borrow().project_search_options.whole_word,

                                    connect_clicked[sender] => move |_| {
                                        sender.input(Msg::ProjectSearchToggleWholeWord);
                                    },
                                },

                                gtk4::ToggleButton {
                                    set_label: ".*",
                                    set_tooltip_text: Some("Use Regular Expression"),
                                    set_css_classes: &["search-toggle-btn"],

                                    #[watch]
                                    set_active: model.engine.borrow().project_search_options.use_regex,

                                    connect_clicked[sender] => move |_| {
                                        sender.input(Msg::ProjectSearchToggleRegex);
                                    },
                                },
                            },

                            // Replace input row
                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Horizontal,
                                set_margin_top: 2,
                                set_margin_bottom: 4,
                                set_margin_start: 6,
                                set_margin_end: 6,
                                set_spacing: 4,

                                gtk4::Entry {
                                    set_hexpand: true,
                                    set_width_chars: 1,
                                    set_placeholder_text: Some("Replace…"),

                                    connect_changed[sender] => move |entry| {
                                        sender.input(Msg::ProjectReplaceTextChanged(
                                            entry.text().to_string(),
                                        ));
                                    },

                                    connect_activate[sender] => move |_| {
                                        sender.input(Msg::ProjectReplaceAll);
                                    },
                                },

                                gtk4::Button {
                                    set_label: "All",
                                    set_tooltip_text: Some("Replace all matches in project"),
                                    set_css_classes: &["search-toggle-btn"],

                                    connect_clicked[sender] => move |_| {
                                        sender.input(Msg::ProjectReplaceAll);
                                    },
                                },
                            },

                            // Status label ("N results in M files" / empty)
                            gtk4::Label {
                                set_margin_start: 8,
                                set_margin_bottom: 4,
                                set_halign: gtk4::Align::Start,
                                set_css_classes: &["dim-label"],

                                #[watch]
                                set_text: &model.project_search_status,
                            },

                            // Results list
                            gtk4::ScrolledWindow {
                                set_vexpand: true,
                                set_hscrollbar_policy: gtk4::PolicyType::Never,
                                set_vscrollbar_policy: gtk4::PolicyType::Automatic,
                                set_overlay_scrolling: false,
                                set_css_classes: &["search-results-scroll"],

                                #[name = "search_results_list"]
                                gtk4::ListBox {
                                    set_selection_mode: gtk4::SelectionMode::Single,
                                    set_css_classes: &["search-results-list"],
                                },
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
                                connect_key_pressed[sender, engine] => move |ctrl_ref, key, _, modifier| {
                                    let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                                    let unicode = key.to_unicode().filter(|c| !c.is_control());
                                    let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                                    let shift = modifier.contains(gdk::ModifierType::SHIFT_MASK);
                                    let alt = modifier.contains(gdk::ModifierType::ALT_MASK);

                                    // When a GTK Entry widget has focus (find dialog, search panel),
                                    // let most keys propagate to it. Only intercept Escape and
                                    // global shortcuts (Ctrl-F, Ctrl-Tab, etc.).
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
                                        // Let all other keys reach the Entry widget
                                        // (e.g. search panel input, terminal find input).
                                        return gtk4::glib::Propagation::Proceed;
                                    }

                                    // Alt+letter: open menu (when menu bar visible)
                                    if alt && !ctrl && !shift {
                                        if let Some(ch) = unicode {
                                            let ch_lower = ch.to_ascii_lowercase();
                                            use crate::render::MENU_STRUCTURE;
                                            let menu_idx = MENU_STRUCTURE
                                                .iter()
                                                .position(|(_, alt_key, _)| *alt_key == ch_lower);
                                            if let Some(idx) = menu_idx {
                                                if engine.borrow().menu_bar_visible {
                                                    if engine.borrow().menu_open_idx == Some(idx) {
                                                        sender.input(Msg::CloseMenu);
                                                    } else {
                                                        sender.input(Msg::OpenMenu(idx));
                                                    }
                                                    return gtk4::glib::Propagation::Stop;
                                                }
                                            }
                                        }
                                    }

                                    // Ctrl+Tab / Ctrl+Shift+Tab: MRU tab switcher
                                    if ctrl && !alt && key_name == "Tab" {
                                        let mut eng = engine.borrow_mut();
                                        if eng.tab_switcher_open {
                                            let len = eng.tab_mru.len();
                                            if len > 0 {
                                                eng.tab_switcher_selected =
                                                    (eng.tab_switcher_selected + 1) % len;
                                            }
                                        } else {
                                            eng.open_tab_switcher();
                                        }
                                        drop(eng);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if ctrl && !alt && key_name == "ISO_Left_Tab" {
                                        let mut eng = engine.borrow_mut();
                                        if eng.tab_switcher_open {
                                            let len = eng.tab_mru.len();
                                            if len > 0 {
                                                eng.tab_switcher_selected =
                                                    if eng.tab_switcher_selected == 0 {
                                                        len - 1
                                                    } else {
                                                        eng.tab_switcher_selected - 1
                                                    };
                                            }
                                        } else {
                                            eng.open_tab_switcher();
                                            let len = eng.tab_mru.len();
                                            if len > 0 {
                                                eng.tab_switcher_selected = len - 1;
                                            }
                                        }
                                        drop(eng);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Alt+t: MRU tab switcher (open or cycle forward)
                                    if alt && !ctrl && !shift && unicode == Some('t') {
                                        let mut eng = engine.borrow_mut();
                                        if eng.tab_switcher_open {
                                            let len = eng.tab_mru.len();
                                            if len > 0 {
                                                eng.tab_switcher_selected =
                                                    (eng.tab_switcher_selected + 1) % len;
                                            }
                                        } else {
                                            eng.open_tab_switcher();
                                        }
                                        drop(eng);
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

                                    // Ctrl-F: terminal find when terminal focused, else engine find/replace
                                    if ctrl && !shift && unicode == Some('f') {
                                        if engine.borrow().terminal_has_focus {
                                            if engine.borrow().terminal_find_active {
                                                sender.input(Msg::TerminalFindClose);
                                            } else {
                                                sender.input(Msg::TerminalFindOpen);
                                            }
                                        } else {
                                            // Pass Ctrl+F to engine (opens find/replace overlay
                                            // or does page-down based on ctrl_f_action setting)
                                            engine.borrow_mut().handle_key("f", Some('f'), true);
                                            sender.input(Msg::SearchPollTick);
                                        }
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Ctrl-Shift-V: paste from system clipboard (editor or terminal)
                                    if ctrl && shift && (key_name == "v" || key_name == "V") {
                                        if engine.borrow().terminal_has_focus {
                                            sender.input(Msg::TerminalPasteClipboard);
                                        } else {
                                            sender.input(Msg::KeyPress {
                                                key_name: "PasteClipboard".to_string(),
                                                unicode: None,
                                                ctrl: false,
                                            });
                                        }
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Panel navigation — driven by panel_keys settings
                                    let pk = engine.borrow().settings.panel_keys.clone();
                                    // Ctrl+T: toggle terminal (checked first so it works even when terminal has focus)
                                    if matches_gtk_key(&pk.open_terminal, key, modifier) {
                                        sender.input(Msg::ToggleTerminal);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    // Ctrl+Shift+T: toggle terminal maximize (panel fills editor area)
                                    if matches_gtk_key(&pk.toggle_terminal_maximize, key, modifier) {
                                        sender.input(Msg::ToggleTerminalMaximize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    // Terminal key routing: when terminal has focus, all keys
                                    // are forwarded as PTY bytes without going to the engine.
                                    if engine.borrow().terminal_has_focus {
                                        // Alt+1–9: switch terminal tab.
                                        if alt && !ctrl && !shift {
                                            if let Some(ch) = unicode {
                                                if ch.is_ascii_digit() && ch != '0' {
                                                    let idx = (ch as u8 - b'1') as usize;
                                                    sender.input(Msg::TerminalSwitchTab(idx));
                                                    return gtk4::glib::Propagation::Stop;
                                                }
                                            }
                                        }
                                        // Ctrl+Y / Ctrl+Shift+C: copy terminal selection to clipboard.
                                        if ctrl && !shift && (key_name == "y" || key_name == "Y") {
                                            sender.input(Msg::TerminalCopySelection);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if ctrl && shift && (key_name == "c" || key_name == "C") {
                                            sender.input(Msg::TerminalCopySelection);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Terminal find bar key routing.
                                        if engine.borrow().terminal_find_active {
                                            match key_name.as_str() {
                                                "Escape" => sender.input(Msg::TerminalFindClose),
                                                "Return" if !shift => sender.input(Msg::TerminalFindNext),
                                                "Return" => sender.input(Msg::TerminalFindPrev),
                                                "BackSpace" => sender.input(Msg::TerminalFindBackspace),
                                                _ => {
                                                    if !ctrl && !alt {
                                                        if let Some(ch) = unicode {
                                                            sender.input(Msg::TerminalFindChar(ch));
                                                        }
                                                    }
                                                }
                                            }
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Ctrl-W in split mode: switch focus between panes.
                                        if ctrl && !shift && !alt
                                            && (key_name == "w" || key_name == "W")
                                            && engine.borrow().terminal_split
                                        {
                                            engine.borrow_mut().terminal_split_switch_focus();
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Ctrl+V (without shift): paste clipboard to PTY (VS Code behavior).
                                        if ctrl && !shift && (key_name == "v" || key_name == "V") {
                                            sender.input(Msg::TerminalPasteClipboard);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        let data = gtk_key_to_pty_bytes(&key_name, unicode, ctrl);
                                        if !data.is_empty() {
                                            engine.borrow_mut().terminal_write(&data);
                                        }
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.toggle_sidebar, key, modifier) {
                                        sender.input(Msg::ToggleSidebar);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.focus_explorer, key, modifier) {
                                        // Toggle: if tree already focused, go back to editor
                                        sender.input(Msg::ToggleFocusExplorer);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.focus_search, key, modifier) {
                                        sender.input(Msg::ToggleFocusSearch);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.fuzzy_finder, key, modifier) {
                                        engine.borrow_mut().open_picker(core::engine::PickerSource::Files);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.live_grep, key, modifier) {
                                        engine.borrow_mut().open_picker(core::engine::PickerSource::Grep);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.command_palette, key, modifier) {
                                        engine.borrow_mut().open_picker(core::engine::PickerSource::Commands);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.add_cursor, key, modifier) {
                                        engine.borrow_mut().add_cursor_at_next_match();
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.select_all_matches, key, modifier) {
                                        let mut eng = engine.borrow_mut();
                                        if eng.is_vscode_mode() {
                                            eng.vscode_select_all_occurrences();
                                        } else {
                                            eng.select_all_word_occurrences();
                                        }
                                        drop(eng);
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    if !pk.split_editor_right.is_empty()
                                        && matches_gtk_key(&pk.split_editor_right, key, modifier)
                                    {
                                        engine.borrow_mut().open_editor_group(
                                            crate::core::window::SplitDirection::Vertical,
                                        );
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    if !pk.split_editor_down.is_empty()
                                        && matches_gtk_key(&pk.split_editor_down, key, modifier)
                                    {
                                        engine.borrow_mut().open_editor_group(
                                            crate::core::window::SplitDirection::Horizontal,
                                        );
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    if matches_gtk_key(&pk.nav_back, key, modifier) {
                                        engine.borrow_mut().tab_nav_back();
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.nav_forward, key, modifier) {
                                        engine.borrow_mut().tab_nav_forward();
                                        return gtk4::glib::Propagation::Stop;
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
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if shift && (is_bracket_right || is_brace_right) {
                                            sender.input(Msg::KeyPress {
                                                key_name: "Shift_bracketright".to_string(),
                                                unicode: None,
                                                ctrl: true,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Ctrl+[ → outdent, Ctrl+] → indent (no shift)
                                        if is_bracket_right && !shift {
                                            sender.input(Msg::KeyPress {
                                                key_name: "bracketright".to_string(),
                                                unicode: None,
                                                ctrl: true,
                                            });
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if is_bracket_left && !shift {
                                            sender.input(Msg::KeyPress {
                                                key_name: "bracketleft".to_string(),
                                                unicode: None,
                                                ctrl: true,
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

                                    sender.input(Msg::KeyPress { key_name: effective_key, unicode, ctrl });
                                    gtk4::glib::Propagation::Stop
                                }
                            },

                            add_controller = gtk4::GestureClick {
                                set_button: 1,
                                connect_pressed[sender, drawing_area] => move |gesture, n_press, x, y| {
                                    // Grab focus when clicking in editor
                                    drawing_area.grab_focus();

                                    let width = drawing_area.width() as f64;
                                    let height = drawing_area.height() as f64;
                                    let modifier = gesture.current_event_state();
                                    let alt = gesture
                                        .current_event()
                                        .map(|ev| ev.modifier_state().contains(gdk::ModifierType::ALT_MASK))
                                        .unwrap_or(false);
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
                                connect_drag_update[sender, drawing_area] => move |gesture, dx, dy| {
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
                                        sender.input(Msg::MouseDrag { x, y, width, height });
                                    }
                                },
                                connect_drag_end[sender] => move |_, _, _| {
                                    sender.input(Msg::MouseUp);
                                },
                            },

                            add_controller = gtk4::EventControllerScroll {
                                set_flags: gtk4::EventControllerScrollFlags::VERTICAL
                                         | gtk4::EventControllerScrollFlags::HORIZONTAL,
                                connect_scroll[sender] => move |_, dx, dy| {
                                    sender.input(Msg::MouseScroll { delta_x: dx, delta_y: dy });
                                    gtk4::glib::Propagation::Stop
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
            e.plugin_init();
            // Fetch fresh extension registry in background (updates ignore_error_sources, etc.)
            e.ext_refresh();
            if let Some(ref path) = file_path {
                // CLI argument: open only the specified file/directory, skip session restore
                if path.is_dir() {
                    e.open_folder(path);
                } else {
                    // Load file into the initial window (reuses the scratch buffer's tab).
                    let _ = e.open_file_with_mode(path, crate::core::engine::OpenMode::Permanent);
                }
            } else {
                e.restore_session_files();
            }
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

        // Explorer sidebar state (Phase A.2b-2): flat-row model backing
        // the DrawingArea. Initialised from the engine's cwd so the root
        // folder starts expanded, matching the TUI's default.
        let explorer_state: Rc<RefCell<ExplorerState>> = {
            let root = engine.borrow().cwd.clone();
            Rc::new(RefCell::new(ExplorerState::new(&root)))
        };
        let explorer_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let activity_bar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let activity_bar_hits: Rc<RefCell<Vec<crate::gtk::quadraui_gtk::ActivityBarHit>>> =
            Rc::new(RefCell::new(Vec::new()));
        let activity_bar_hover: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let activity_bar_active_panel: Rc<RefCell<Option<Rc<RefCell<SidebarPanel>>>>> =
            Rc::new(RefCell::new(None));
        let explorer_row_height_cell: Rc<Cell<f64>> = Rc::new(Cell::new(28.0));
        let explorer_scroll_accum: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        #[allow(clippy::type_complexity)]
        let explorer_scrollbar_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>> =
            Rc::new(Cell::new(None));
        let explorer_scrollbar_drag_from: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let active_ctx_popover_ref: Rc<RefCell<Option<gtk4::PopoverMenu>>> =
            Rc::new(RefCell::new(None));
        let drawing_area_ref = Rc::new(RefCell::new(None));
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
        let editor_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let menu_dd_lh: Rc<Cell<f64>> = Rc::new(Cell::new(24.0));
        let debug_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
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
        let diff_btn_map_cell: Rc<RefCell<DiffBtnMap>> = Rc::new(RefCell::new(HashMap::new()));
        let split_btn_map_cell: Rc<RefCell<SplitBtnMap>> = Rc::new(RefCell::new(HashMap::new()));
        let action_btn_map_cell: Rc<RefCell<ActionBtnMap>> = Rc::new(RefCell::new(HashMap::new()));
        let status_segment_map_cell: Rc<RefCell<StatusSegmentMap>> =
            Rc::new(RefCell::new(HashMap::new()));
        let tab_visible_counts_cell: Rc<
            RefCell<Vec<(crate::core::window::GroupId, usize, usize)>>,
        > = Rc::new(RefCell::new(Vec::new()));
        #[allow(clippy::type_complexity)]
        let nav_arrow_rects_cell: Rc<RefCell<(f64, f64, f64, f64, f64)>> =
            Rc::new(RefCell::new((0.0, 0.0, 0.0, 0.0, 0.0)));
        let sidebar_inner_sw_ref: Rc<RefCell<Option<gtk4::ScrolledWindow>>> =
            Rc::new(RefCell::new(None));
        let sidebar_revealer_ref: Rc<RefCell<Option<gtk4::Revealer>>> = Rc::new(RefCell::new(None));
        // Saves the sidebar width at the start of a drag so we can compute
        // initial_width + total_offset instead of accumulating delta per event.
        let sidebar_drag_start_w: Rc<Cell<i32>> = Rc::new(Cell::new(300));
        let explorer_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let search_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
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
        let search_results_list_ref: Rc<RefCell<Option<gtk4::ListBox>>> =
            Rc::new(RefCell::new(None));

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

        let model = App {
            engine: engine.clone(),
            sidebar_visible,
            window: root.clone(),
            active_panel: SidebarPanel::Explorer,
            explorer_sidebar_da_ref: explorer_sidebar_da_ref.clone(),
            activity_bar_da_ref: activity_bar_da_ref.clone(),
            activity_bar_active_panel: activity_bar_active_panel.clone(),
            explorer_state: explorer_state.clone(),
            explorer_row_height_cell: explorer_row_height_cell.clone(),
            explorer_scroll_accum: explorer_scroll_accum.clone(),
            explorer_scrollbar_rect: explorer_scrollbar_rect.clone(),
            drawing_area: drawing_area_ref.clone(),
            menu_bar_da: menu_bar_da_ref.clone(),
            debug_sidebar_da_ref: debug_sidebar_da_ref.clone(),
            git_sidebar_da_ref: git_sidebar_da_ref.clone(),
            ext_sidebar_da_ref: ext_sidebar_da_ref.clone(),
            ai_sidebar_da_ref: ai_sidebar_da_ref.clone(),
            window_scrollbars: window_scrollbars_ref.clone(),
            overlay: overlay_ref.clone(),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
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
            search_panel_box: search_panel_box_ref.clone(),
            debug_panel_box: debug_panel_box_ref.clone(),
            git_panel_box: git_panel_box_ref.clone(),
            ext_panel_box: ext_panel_box_ref.clone(),
            ext_dyn_panel_da_ref: ext_dyn_panel_da_ref.clone(),
            ext_dyn_panel_box: ext_dyn_panel_box_ref.clone(),
            settings_panel_box: settings_panel_box_ref.clone(),
            settings_da_ref: settings_da_ref.clone(),
            ai_panel_box_ref: ai_panel_box_ref.clone(),
            project_search_status: String::new(),
            search_results_list: search_results_list_ref.clone(),
            last_clipboard_content: None,
            clipboard,
            h_sb_dragging: None,
            h_sb_hovered: false,
            tab_close_hover: None,
            tab_slot_positions: tab_slot_positions_cell.clone(),
            diff_btn_map: diff_btn_map_cell.clone(),
            split_btn_map: split_btn_map_cell.clone(),
            action_btn_map: action_btn_map_cell.clone(),
            status_segment_map: status_segment_map_cell.clone(),
            nav_arrow_rects: nav_arrow_rects_cell.clone(),
            terminal_sb_dragging: false,
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            group_divider_dragging: None,
            tab_dragging: false,
            tab_drag_start: None,
            last_sc_refresh: std::time::Instant::now(),
            last_file_check: std::time::Instant::now(),
            last_tree_indicator_update: std::time::Instant::now(),
            menu_dropdown_da: menu_dropdown_da_ref.clone(),
            panel_hover_da: panel_hover_da_ref.clone(),
            panel_hover_link_rects: panel_hover_link_rects.clone(),
            panel_hover_popup_rect: panel_hover_popup_rect.clone(),
            editor_hover_popup_rect: editor_hover_popup_rect.clone(),
            editor_hover_link_rects: editor_hover_link_rects.clone(),
            menu_dd_line_height: menu_dd_lh.clone(),
            css_provider,
            last_colorscheme,
            settings_self_save: false,
            active_ctx_popover: active_ctx_popover_ref.clone(),
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
        *search_panel_box_ref.borrow_mut() = Some(widgets.search_panel.clone());
        *debug_panel_box_ref.borrow_mut() = Some(widgets.debug_panel.clone());
        *git_panel_box_ref.borrow_mut() = Some(widgets.git_panel.clone());
        *ext_panel_box_ref.borrow_mut() = Some(widgets.ext_panel.clone());
        *ext_dyn_panel_box_ref.borrow_mut() = Some(widgets.ext_dyn_panel.clone());
        *settings_panel_box_ref.borrow_mut() = Some(widgets.settings_panel.clone());
        *ai_panel_box_ref.borrow_mut() = Some(widgets.ai_panel_box.clone());
        *search_results_list_ref.borrow_mut() = Some(widgets.search_results_list.clone());

        // ── Settings sidebar (Phase A.3c-2: native widgets → DrawingArea) ──────
        {
            let engine_d = engine.clone();
            widgets.settings_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine_d.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(UI_FONT);
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_settings_panel(cr, &layout, &engine, &theme, 0.0, 0.0, w, h, line_height);
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
        *settings_da_ref.borrow_mut() = Some(widgets.settings_da.clone());

        // ── Explorer sidebar (Phase A.2b-2: native TreeView → DrawingArea) ─────
        {
            let engine_d = engine.clone();
            let state_d = explorer_state.clone();
            let row_h_cell = explorer_row_height_cell.clone();
            let sb_rect_cell = explorer_scrollbar_rect.clone();
            widgets.explorer_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine_d.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(UI_FONT);
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
                let state = state_d.borrow();
                let has_focus = engine.explorer_has_focus;
                let tree = crate::render::explorer_to_tree_view(
                    &state.rows,
                    state.scroll_top,
                    state.selected,
                    has_focus,
                    &engine,
                );
                let sb_rect =
                    draw_explorer_panel(cr, &layout, &tree, &theme, 0.0, 0.0, w, h, line_height);
                sb_rect_cell.set(sb_rect);
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
        {
            let sb_rect_cell = explorer_scrollbar_rect.clone();
            let drag_from = explorer_scrollbar_drag_from.clone();
            let state_d = explorer_state.clone();
            let row_h_cell = explorer_row_height_cell.clone();
            let da_for_draw = widgets.explorer_da.clone();
            let da_for_update = widgets.explorer_da.clone();
            let drag_from_update = drag_from.clone();
            let sb_rect_update = sb_rect_cell.clone();
            let state_update = state_d.clone();
            let row_h_update = row_h_cell.clone();
            let drag_from_end = drag_from.clone();
            let gesture = gtk4::GestureDrag::new();
            gesture.set_button(1);
            gesture.connect_drag_begin(move |g, x_start, _y_start| {
                let Some((sb_x, _sb_y, sb_w, _sb_h)) = sb_rect_cell.get() else {
                    drag_from.set(None);
                    return;
                };
                if x_start < sb_x || x_start > sb_x + sb_w {
                    drag_from.set(None);
                    return;
                }
                // Started on scrollbar — claim so sibling/parent drags
                // (sidebar resize) don't also fire.
                g.set_state(gtk4::EventSequenceState::Claimed);
                let st = state_d.borrow();
                drag_from.set(Some(st.scroll_top));
                // A press on the track has already jump-scrolled via the
                // click gesture; the drag-begin fires AFTER click-press,
                // so `st.scroll_top` already reflects the jump. Subsequent
                // drag_update deltas fine-tune from there.
                da_for_draw.queue_draw();
            });
            gesture.connect_drag_update(move |_, _dx, dy| {
                let Some(start_top) = drag_from_update.get() else {
                    return;
                };
                let Some((_sb_x, _sb_y, _sb_w, sb_h)) = sb_rect_update.get() else {
                    return;
                };
                let total = state_update.borrow().rows.len();
                let item_h = row_h_update.get().max(1.0);
                let viewport = (da_for_update.height() as f64 / item_h).floor().max(0.0) as usize;
                let max_scroll = total.saturating_sub(viewport);
                if max_scroll == 0 || sb_h <= 0.0 {
                    return;
                }
                // `dy` is pixels on the track; convert to a row offset
                // using the same ratio the draw uses to place the thumb.
                let delta_rows = (dy / sb_h * max_scroll as f64).round() as isize;
                let new_top = (start_top as isize + delta_rows).max(0) as usize;
                state_update.borrow_mut().scroll_top = new_top.min(max_scroll);
                da_for_update.queue_draw();
            });
            gesture.connect_drag_end(move |_, _, _| {
                drag_from_end.set(None);
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

        // ── Menu dropdown overlay DrawingArea ─────────────────────────────────
        // A full-window transparent overlay that draws the dropdown in window
        // coordinates (x=0 at window left edge).  can_target is toggled on/off
        // with menu open/close so that normal editor clicks pass through.
        {
            let menu_dd_da = gtk4::DrawingArea::new();
            menu_dd_da.set_hexpand(true);
            menu_dd_da.set_vexpand(true);
            menu_dd_da.set_can_target(false); // pass-through until menu opens

            // Draw function — only renders when a menu is open.
            {
                let engine = engine.clone();
                let lh = menu_dd_lh.clone();
                menu_dd_da.set_draw_func(move |_, cr, _, _| {
                    let engine = engine.borrow();
                    let Some(midx) = engine.menu_open_idx else {
                        return;
                    };
                    let theme = Theme::from_name(&engine.settings.colorscheme);
                    let open_items: Vec<render::MenuItemData> = render::MENU_STRUCTURE
                        .get(midx)
                        .map(|(_, _, items)| items.to_vec())
                        .unwrap_or_default();
                    let mut open_menu_col: u16 = 0;
                    for i in 0..midx {
                        if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                            open_menu_col += name.len() as u16 + 2;
                        }
                    }
                    let title = engine
                        .cwd
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "VimCode".to_string());
                    let data = render::MenuBarData {
                        open_menu_idx: engine.menu_open_idx,
                        open_items,
                        open_menu_col,
                        highlighted_item_idx: engine.menu_highlighted_item,
                        title,
                        show_window_controls: true,
                        is_vscode_mode: engine.is_vscode_mode(),
                        nav_back_enabled: engine.tab_nav_can_go_back(),
                        nav_forward_enabled: engine.tab_nav_can_go_forward(),
                    };
                    let line_height = lh.get();
                    // Draw the dropdown at window-level coordinates.
                    // anchor_y = 0: menu bar is now the titlebar, so the overlay
                    // content starts right below it.
                    draw_menu_dropdown(cr, &data, &theme, 0.0, 0.0, 0.0, 0.0, line_height);
                });
            }

            // Click handler — routes clicks to menu items or closes the menu.
            {
                let sender_dd = sender.input_sender().clone();
                let engine_dd = engine.clone();
                let lh_dd = menu_dd_lh.clone();
                let gesture = gtk4::GestureClick::new();
                gesture.set_button(1);
                gesture.connect_pressed(move |_, _, x, y| {
                    let engine = engine_dd.borrow();
                    let line_height = lh_dd.get();
                    let Some(open_idx) = engine.menu_open_idx else {
                        return;
                    };
                    // Menu bar is now the titlebar — the overlay starts below it.
                    // The dropdown draws at y=0 in the overlay.
                    if let Some((_, _, items)) = render::MENU_STRUCTURE.get(open_idx) {
                        // Click in the dropdown area.
                        // Use ~7px per char as a rough approximation matching draw_menu_bar.
                        let mut popup_x = 8.0_f64;
                        for i in 0..open_idx {
                            if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                                popup_x += name.len() as f64 * 7.0 + 10.0;
                            }
                        }
                        let popup_w = 220.0_f64;
                        let popup_y = 0.0;
                        let menu_pad = 4.0;
                        let popup_h = items.len() as f64 * line_height + menu_pad * 2.0;
                        if y >= popup_y
                            && y < popup_y + popup_h
                            && x >= popup_x
                            && x < popup_x + popup_w
                        {
                            let item_idx =
                                ((y - popup_y - menu_pad) / line_height).floor() as usize;
                            if item_idx < items.len() && !items[item_idx].separator {
                                let action = items[item_idx].action.to_string();
                                drop(engine);
                                sender_dd
                                    .send(Msg::MenuActivateItem(open_idx, item_idx, action))
                                    .ok();
                            } else {
                                drop(engine);
                                sender_dd.send(Msg::CloseMenu).ok();
                            }
                        } else {
                            // Click outside the dropdown → close it.
                            drop(engine);
                            sender_dd.send(Msg::CloseMenu).ok();
                        }
                    } else {
                        drop(engine);
                        sender_dd.send(Msg::CloseMenu).ok();
                    }
                });
                menu_dd_da.add_controller(gesture);
            }

            // Motion handler — highlight menu items on hover.
            {
                let sender_motion = sender.input_sender().clone();
                let engine_motion = engine.clone();
                let lh_motion = menu_dd_lh.clone();
                let motion = gtk4::EventControllerMotion::new();
                motion.connect_motion(move |_, x, y| {
                    let engine = engine_motion.borrow();
                    let line_height = lh_motion.get();
                    let Some(open_idx) = engine.menu_open_idx else {
                        return;
                    };
                    if let Some((_, _, items)) = render::MENU_STRUCTURE.get(open_idx) {
                        let mut popup_x = 8.0_f64;
                        for i in 0..open_idx {
                            if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                                popup_x += name.len() as f64 * 7.0 + 10.0;
                            }
                        }
                        let popup_w = 220.0_f64;
                        let popup_y = 0.0;
                        let menu_pad = 4.0;
                        let popup_h = items.len() as f64 * line_height + menu_pad * 2.0;
                        if y >= popup_y
                            && y < popup_y + popup_h
                            && x >= popup_x
                            && x < popup_x + popup_w
                        {
                            let item_idx =
                                ((y - popup_y - menu_pad) / line_height).floor() as usize;
                            if item_idx < items.len() && !items[item_idx].separator {
                                if engine.menu_highlighted_item != Some(item_idx) {
                                    drop(engine);
                                    sender_motion.send(Msg::MenuHighlight(Some(item_idx))).ok();
                                }
                            } else if engine.menu_highlighted_item.is_some() {
                                drop(engine);
                                sender_motion.send(Msg::MenuHighlight(None)).ok();
                            }
                        } else if engine.menu_highlighted_item.is_some() {
                            drop(engine);
                            sender_motion.send(Msg::MenuHighlight(None)).ok();
                        }
                    }
                });
                menu_dd_da.add_controller(motion);
            }

            widgets.window_overlay.add_overlay(&menu_dd_da);
            *menu_dropdown_da_ref.borrow_mut() = Some(menu_dd_da);
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
                    let font_desc = FontDescription::from_string(UI_FONT);
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    lh.set(line_height);
                    let char_width =
                        font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
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
        // Draw function: renders menu labels using the same Cairo helper.
        {
            let engine = engine.clone();
            let nav_rects = nav_arrow_rects_cell.clone();
            widgets.menu_bar_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine.borrow();
                // Menu bar is always visible in GTK (acts as the window title bar).
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let open_items: Vec<render::MenuItemData> = if let Some(midx) = engine.menu_open_idx
                {
                    render::MENU_STRUCTURE
                        .get(midx)
                        .map(|(_, _, items)| items.to_vec())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let open_menu_col: u16 = if let Some(midx) = engine.menu_open_idx {
                    let mut col: u16 = 0;
                    for i in 0..midx {
                        if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                            col += name.len() as u16 + 2;
                        }
                    }
                    col
                } else {
                    0
                };
                let title = engine
                    .cwd
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "VimCode".to_string());
                let data = render::MenuBarData {
                    open_menu_idx: engine.menu_open_idx,
                    open_items,
                    open_menu_col,
                    highlighted_item_idx: engine.menu_highlighted_item,
                    title,
                    show_window_controls: true,
                    is_vscode_mode: engine.is_vscode_mode(),
                    nav_back_enabled: engine.tab_nav_can_go_back(),
                    nav_forward_enabled: engine.tab_nav_can_go_forward(),
                };
                let w = da.width() as f64;
                let h = da.height() as f64;
                let rects = draw_menu_bar(cr, &data, &theme, 0.0, 0.0, w, h);
                *nav_rects.borrow_mut() = rects;
            });
        }
        // Click gesture: open/close individual menus (no hamburger zone here).
        {
            let sender_menu = sender.input_sender().clone();
            let engine_menu = engine.clone();
            let nav_rects_click = nav_arrow_rects_cell.clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |gest, _, x, _y| {
                let engine = engine_menu.borrow();
                // Scan menu labels from left edge (no hamburger on this widget).
                // Use ~7px/char + 10px padding as approximation for UI font metrics.
                let mut cursor_x = 8.0_f64;
                for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
                    let item_w = name.len() as f64 * 7.0 + 10.0;
                    if x >= cursor_x && x < cursor_x + item_w {
                        if engine.menu_open_idx == Some(idx) {
                            sender_menu.send(Msg::CloseMenu).ok();
                        } else {
                            sender_menu.send(Msg::OpenMenu(idx)).ok();
                        }
                        return;
                    }
                    cursor_x += item_w;
                }
                // Use cached arrow pixel positions from draw_menu_bar.
                let (back_x, back_end, fwd_x, fwd_end, unit_end) = *nav_rects_click.borrow();
                if x >= back_x && x < back_end {
                    // Claim the gesture so WindowHandle doesn't maximize on double-click.
                    gest.set_state(gtk4::EventSequenceState::Claimed);
                    sender_menu.send(Msg::MruNavBack).ok();
                    return;
                }
                if x >= fwd_x && x < fwd_end {
                    gest.set_state(gtk4::EventSequenceState::Claimed);
                    sender_menu.send(Msg::MruNavForward).ok();
                    return;
                }
                // Click on the search box area → open Command Center.
                if x >= fwd_end && x < unit_end {
                    gest.set_state(gtk4::EventSequenceState::Claimed);
                    sender_menu.send(Msg::OpenCommandCenter).ok();
                    return;
                }
                // Claim clicks within the nav+search box area to prevent
                // WindowHandle double-click-to-maximize on the search box.
                if x >= back_x && x < unit_end {
                    gest.set_state(gtk4::EventSequenceState::Claimed);
                }
                // Click in empty part of bar → close any open dropdown
                if engine.menu_open_idx.is_some() {
                    sender_menu.send(Msg::CloseMenu).ok();
                }
            });
            widgets.menu_bar_da.add_controller(gesture);
        }
        // Hover motion: switch dropdown when moving between menu labels while open.
        {
            let sender_hover = sender.input_sender().clone();
            let engine_hover = engine.clone();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_, x, _y| {
                let engine = engine_hover.borrow();
                // Only switch if a menu is already open.
                let Some(current) = engine.menu_open_idx else {
                    return;
                };
                let mut cursor_x = 8.0_f64;
                for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
                    let item_w = name.len() as f64 * 7.0 + 10.0;
                    if x >= cursor_x && x < cursor_x + item_w {
                        if idx != current {
                            sender_hover.send(Msg::OpenMenu(idx)).ok();
                        }
                        return;
                    }
                    cursor_x += item_w;
                }
            });
            widgets.menu_bar_da.add_controller(motion);
        }
        // ── Debug sidebar DrawingArea setup ───────────────────────────────────
        {
            let engine = engine.clone();
            widgets
                .debug_sidebar_da
                .set_draw_func(move |da, cr, _w, _h| {
                    let engine = engine.borrow();
                    let theme = Theme::from_name(&engine.settings.colorscheme);
                    let font_desc = FontDescription::from_string(UI_FONT);
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    let char_width =
                        font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
                    let screen =
                        build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                    let w = da.width() as f64;
                    let h = da.height() as f64;
                    draw_debug_sidebar(cr, &layout, &screen, &theme, 0.0, 0.0, w, h, line_height);
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
            widgets.git_sidebar_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(UI_FONT);
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
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
        *git_sidebar_da_ref.borrow_mut() = Some(widgets.git_sidebar_da.clone());

        // ── Extensions sidebar draw + key setup ───────────────────────────────
        {
            let engine = engine.clone();
            widgets.ext_sidebar_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let font_desc = FontDescription::from_string(UI_FONT);
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
                let screen =
                    build_screen_layout(&engine, &theme, &[], line_height, char_width, false);
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_ext_sidebar(cr, &layout, &screen, &theme, 0.0, 0.0, w, h, line_height);
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
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, n_press, x, y| {
                sender_ext.send(Msg::ExtSidebarClick(x, y, n_press)).ok();
            });
            widgets.ext_sidebar_da.add_controller(gesture);
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
                    let font_desc = FontDescription::from_string(UI_FONT);
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    let char_width =
                        font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
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
            let active_panel_d = Rc::new(RefCell::new(SidebarPanel::Explorer));
            let active_panel_write = active_panel_d.clone();
            // Mirror the current `active_panel` into the refcell via an update-time hook.
            // Done below in `update()` — this refcell is published here so the draw func
            // can read it without borrowing `self`.
            *activity_bar_active_panel.borrow_mut() = Some(active_panel_d);
            widgets.activity_bar.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine_d.borrow();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                let active = active_panel_write.borrow().clone();
                let bar = build_gtk_activity_bar_primitive(&engine, &active, &theme);
                let hovered = hover_d.get();
                let hits = crate::gtk::quadraui_gtk::draw_activity_bar(
                    cr,
                    &layout,
                    da.width() as f64,
                    da.height() as f64,
                    &bar,
                    &theme,
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
                let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
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
                // If the terminal is maximized, re-sync the PTY rows to the
                // new window size so the shell reflows immediately rather
                // than on the next user toggle.
                {
                    let e = engine_for_resize.borrow();
                    if e.terminal_maximized && !e.terminal_panes.is_empty() {
                        let target = gtk_terminal_target_maximize_rows(
                            &e,
                            height as f64,
                            line_height,
                        );
                        let effective = e.effective_terminal_panel_rows(target);
                        let cols = (width as f64 / char_width).floor() as u16;
                        drop(e);
                        engine_for_resize
                            .borrow_mut()
                            .terminal_resize(cols.max(40), effective);
                    }
                }
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
        let diff_btn_for_draw = diff_btn_map_cell.clone();
        let split_btn_for_draw = split_btn_map_cell.clone();
        let action_btn_for_draw = action_btn_map_cell.clone();
        let dialog_btn_for_draw = model.dialog_btn_rects.clone();
        let editor_hover_rect_for_draw = model.editor_hover_popup_rect.clone();
        let editor_hover_links_for_draw = model.editor_hover_link_rects.clone();
        let mouse_pos_for_draw = mouse_pos_cell.clone();
        let tab_vis_for_draw = tab_visible_counts_cell.clone();
        let status_seg_for_draw = model.status_segment_map.clone();
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
                            &diff_btn_for_draw,
                            &split_btn_for_draw,
                            &action_btn_for_draw,
                            &dialog_btn_for_draw,
                            &editor_hover_rect_for_draw,
                            &editor_hover_links_for_draw,
                            mouse_pos_for_draw.get(),
                            &tab_vis_for_draw,
                            &status_seg_for_draw,
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
            let tab_slots_rc = tab_slot_positions_cell.clone();
            let diff_btn_rc = diff_btn_map_cell.clone();
            let split_btn_rc = split_btn_map_cell.clone();
            let action_btn_rc = action_btn_map_cell.clone();
            let status_seg_rc = status_segment_map_cell.clone();
            let rc_gesture = gtk4::GestureClick::new();
            rc_gesture.set_button(3);
            rc_gesture.connect_pressed(move |gesture, _n_press, x, y| {
                let widget = gesture.widget();
                let width = widget.width() as f64;
                let height = widget.height() as f64;
                let lh = lh_rc.get().max(1.0);
                let mut engine = engine_rc.borrow_mut();
                let target = pixel_to_click_target(
                    &mut engine,
                    x,
                    y,
                    width,
                    height,
                    lh,
                    0.0, // char_width not needed for tab bar detection
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
                if !engine_ref.borrow().tab_switcher_open {
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
                                engine_ref.borrow_mut().tab_switcher_confirm();
                                da.queue_draw();
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
            } => {
                self.handle_key_press(key_name, unicode, ctrl, &sender);
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
                let cw = self.cached_char_width.max(1.0);
                let lh = self.cached_line_height.max(1.0);
                let cx = (x / cw) as u16;
                let cy = (y / lh) as u16;
                self.engine.borrow_mut().open_editor_context_menu(cx, cy);
                self.draw_needed.set(true);
            }
            Msg::Resize => {
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
                width,
                height,
            } => {
                let mut engine = self.engine.borrow_mut();
                if !engine.picker_open {
                    if let ClickTarget::BufferPos(_, line, col) = pixel_to_click_target(
                        &mut engine,
                        x,
                        y,
                        width,
                        height,
                        self.cached_line_height,
                        self.cached_char_width,
                        &self.tab_slot_positions.borrow(),
                        &self.diff_btn_map.borrow(),
                        &self.split_btn_map.borrow(),
                        &self.action_btn_map.borrow(),
                        &self.status_segment_map.borrow(),
                    ) {
                        engine.add_cursor_at_pos(line, col);
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::MouseDoubleClick {
                x,
                y,
                width,
                height,
            } => {
                let mut engine = self.engine.borrow_mut();
                if engine.picker_open {
                    // Double-click on picker: toggle expand for tree items, or confirm
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
                    // Check breadcrumb double-click before falling through
                    let mut bc_handled = false;
                    if engine.settings.breadcrumbs {
                        let lh = self.cached_line_height.max(1.0);
                        let cw = self.cached_char_width.max(1.0);
                        if y >= lh && y < lh * 2.0 {
                            let segments =
                                crate::render::build_breadcrumbs_for_active_group(&engine);
                            let sep_w = " › ".chars().count() as f64 * cw;
                            let mut seg_x = cw; // left padding
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
                        handle_mouse_double_click(
                            &mut engine,
                            x,
                            y,
                            width,
                            height,
                            self.cached_line_height,
                            self.cached_char_width,
                            &self.tab_slot_positions.borrow(),
                            &self.diff_btn_map.borrow(),
                            &self.split_btn_map.borrow(),
                            &self.action_btn_map.borrow(),
                            &self.status_segment_map.borrow(),
                        );
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
                // Picker open: scroll the picker results
                if engine.picker_open && delta_y.abs() > 0.01 {
                    let step = (delta_y * 3.0).round().abs() as usize;
                    let max = engine.picker_items.len().saturating_sub(1);
                    if delta_y > 0.0 {
                        engine.picker_selected = (engine.picker_selected + step).min(max);
                    } else {
                        engine.picker_selected = engine.picker_selected.saturating_sub(step);
                    }
                    let visible = 20usize;
                    if engine.picker_selected >= engine.picker_scroll_top + visible {
                        engine.picker_scroll_top = engine.picker_selected + 1 - visible;
                    }
                    if engine.picker_selected < engine.picker_scroll_top {
                        engine.picker_scroll_top = engine.picker_selected;
                    }
                    engine.picker_load_preview();
                    drop(engine);
                    self.draw_needed.set(true);
                    return;
                }
                // If editor hover popup is visible, scroll it instead of the editor
                if engine.editor_hover.is_some() && delta_y.abs() > 0.01 {
                    let delta = (delta_y * 3.0).round() as i32;
                    if engine.editor_hover_scroll(delta) {
                        drop(engine);
                        self.draw_needed.set(true);
                        return;
                    }
                }
                if delta_y.abs() > 0.01 {
                    let lines = engine.buffer().len_lines().saturating_sub(1);
                    let scroll_count = (delta_y * 3.0).round().abs() as usize;
                    if delta_y > 0.0 {
                        engine.scroll_down_visible(scroll_count);
                    } else {
                        engine.scroll_up_visible(scroll_count);
                    }
                    // Move cursor into viewport instead of snapping scroll back.
                    let scrolloff = engine.settings.scrolloff;
                    let vp = engine.view().viewport_lines.max(1);
                    let cur = engine.view().cursor.line;
                    let new_top = engine.view().scroll_top;
                    if cur < new_top + scrolloff {
                        engine.view_mut().cursor.line = (new_top + scrolloff).min(lines);
                        engine.clamp_cursor_col();
                    } else if cur >= new_top + vp.saturating_sub(scrolloff) {
                        engine.view_mut().cursor.line =
                            (new_top + vp.saturating_sub(scrolloff + 1)).min(lines);
                        engine.clamp_cursor_col();
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
                // Compute UI font line height for sidebar click handlers.
                if let Some(ref da) = *self.drawing_area.borrow() {
                    let font_desc = FontDescription::from_string(UI_FONT);
                    let pango_ctx = da.pango_context();
                    let fm = pango_ctx.metrics(Some(&font_desc), None);
                    self.cached_ui_line_height =
                        (fm.ascent() + fm.descent()) as f64 / pango::SCALE as f64;
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
                // If VimCode itself just saved the file, skip the reload — we already
                // have the correct in-memory state and the file contains exactly what
                // we wrote.  This prevents the GIO file watcher from firing an extra
                // (redundant) Settings::load_with_validation() after every SettingChanged.
                if self.settings_self_save {
                    self.settings_self_save = false;
                    return;
                }

                // External edit: reload from disk.
                // Use load_with_validation (not load) to avoid writing back to the file,
                // which would trigger the watcher again and cause an infinite reload loop.
                // Silently ignore errors — the file may be mid-write.
                if let Ok(new_settings) = core::settings::Settings::load_with_validation() {
                    let mut engine = self.engine.borrow_mut();
                    engine.settings = new_settings;
                    engine.ensure_spell_checker();
                    engine.message = "Settings reloaded from disk".to_string();
                    drop(engine);

                    // Force redraw to apply new font/line number settings
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
                    match engine.settings.save() {
                        Ok(()) => {
                            // Mark that WE wrote the file so SettingsFileChanged can skip
                            // the redundant reload (we already have the correct in-memory state).
                            self.settings_self_save = true;
                        }
                        Err(e) => {
                            engine.message = format!("Warning: setting changed but not saved: {e}");
                        }
                    }
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
                let status = self.engine.borrow().message.clone();
                self.project_search_status = status;
                self.draw_needed.set(true);
            }
            Msg::ProjectSearchSubmit => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                self.engine.borrow_mut().start_project_search(cwd);
                let status = self.engine.borrow().message.clone();
                self.project_search_status = status;
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
            | Msg::OpenMenu(_)
            | Msg::CloseMenu
            | Msg::MruNavBack
            | Msg::MruNavForward
            | Msg::OpenCommandCenter
            | Msg::MenuActivateItem(_, _, _)
            | Msg::MenuHighlight(_) => {
                self.handle_menu_msg(msg, &sender);
            }
            Msg::DebugSidebarClick(_, _)
            | Msg::DebugSidebarKey(_, _)
            | Msg::DebugSidebarScroll(_) => {
                self.handle_debug_sidebar_msg(msg);
            }
            Msg::ScSidebarClick(_, _, _) | Msg::ScSidebarMotion(_, _) | Msg::ScKey(_, _) => {
                self.handle_sc_sidebar_msg(msg);
            }
            Msg::ExtSidebarKey(_, _) | Msg::ExtSidebarClick(_, _, _) => {
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
    let modal_open = engine.picker_open || engine.tab_switcher_open;
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
        let engine = self.engine.borrow();
        let root = engine.cwd.clone();
        let show_hidden = engine.settings.show_hidden_files;
        let case_insensitive = engine.settings.explorer_sort_case_insensitive;
        drop(engine);
        let viewport_rows = self
            .explorer_sidebar_da_ref
            .borrow()
            .as_ref()
            .map(|da| {
                let h = da.height() as f64;
                let item_h = self.explorer_row_height_cell.get().max(1.0);
                (h / item_h).floor().max(0.0) as usize
            })
            .unwrap_or(20);
        self.explorer_state.borrow_mut().reveal_path(
            target,
            &root,
            viewport_rows,
            show_hidden,
            case_insensitive,
        );
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.queue_draw();
        }
    }

    /// Rebuild the explorer flat-row list from disk (used after file
    /// create/rename/delete/move or when `cwd` changes) and redraw the DA.
    fn refresh_explorer(&self) {
        let engine = self.engine.borrow();
        let root = engine.cwd.clone();
        let show_hidden = engine.settings.show_hidden_files;
        let case_insensitive = engine.settings.explorer_sort_case_insensitive;
        drop(engine);
        self.explorer_state
            .borrow_mut()
            .rebuild(&root, show_hidden, case_insensitive);
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.queue_draw();
        }
    }

    /// Save the current session state and exit the process immediately.
    /// This is the canonical quit path — called when there are no unsaved changes.
    fn save_session_and_exit(&self) -> ! {
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
        std::process::exit(0);
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
                        self.reveal_path_in_explorer(&path);
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
                sender.input(Msg::ToggleSidebar);
            }
            EngineAction::QuitWithError => {
                let mut engine = self.engine.borrow_mut();
                engine.cleanup_all_swaps();
                engine.lsp_shutdown();
                drop(engine);
                std::process::exit(1);
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

    /// Rebuild the search results ListBox from current engine state.
    fn rebuild_search_results(&self, sender: &relm4::Sender<Msg>) {
        let list = match self.search_results_list.borrow().as_ref() {
            Some(l) => l.clone(),
            None => return,
        };

        // Remove all existing rows
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        let engine = self.engine.borrow();
        let results = &engine.project_search_results;
        if results.is_empty() {
            return;
        }

        let theme = Theme::from_name(&engine.settings.colorscheme);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut last_file: Option<PathBuf> = None;

        for (idx, m) in results.iter().enumerate() {
            // Add a file header row when the file changes
            if last_file.as_deref() != Some(&m.file) {
                last_file = Some(m.file.clone());
                let rel = m.file.strip_prefix(&cwd).unwrap_or(&m.file);
                let file_label = gtk4::Label::new(None);
                let header_markup = format!(
                    "<b><span foreground='{}'>{}</span></b>",
                    theme.function.to_hex(),
                    gtk4::glib::markup_escape_text(&rel.display().to_string())
                );
                file_label.set_markup(&header_markup);
                file_label.set_halign(gtk4::Align::Start);
                file_label.set_margin_top(4);
                file_label.set_margin_start(4);
                let header_row = gtk4::ListBoxRow::new();
                header_row.set_selectable(false);
                header_row.set_child(Some(&file_label));
                list.append(&header_row);
            }

            // Result row
            let snippet = format!("  {}: {}", m.line + 1, m.line_text.trim());
            let row_label = gtk4::Label::new(None);
            let result_markup = format!(
                "<span foreground='{}'>{}</span>",
                theme.foreground.to_hex(),
                gtk4::glib::markup_escape_text(&snippet)
            );
            row_label.set_markup(&result_markup);
            row_label.set_halign(gtk4::Align::Start);
            row_label.set_ellipsize(pango::EllipsizeMode::End);
            row_label.set_margin_start(4);
            let result_row = gtk4::ListBoxRow::new();
            result_row.set_selectable(true);

            // Tag the row with its result index via the widget name
            result_row.set_widget_name(&idx.to_string());
            result_row.set_child(Some(&row_label));
            list.append(&result_row);
        }

        let sender_clone = sender.clone();
        list.connect_row_activated(move |_, row| {
            if let Ok(idx) = row.widget_name().parse::<usize>() {
                sender_clone.send(Msg::ProjectSearchOpenResult(idx)).ok();
            }
        });
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
        let modal_open = engine.picker_open || engine.tab_switcher_open;
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

        // Route keys to sidebar handlers when a sidebar has focus.
        // GTK focus on sidebar DrawingAreas is unreliable, so we check
        // the engine focus flags here (same approach as TUI backend).

        // Explorer sidebar: CRUD keys + navigation
        if self.engine.borrow().explorer_has_focus {
            let key_mapped = map_gtk_key_name(key_name.as_str());
            if key_mapped == "Escape" {
                self.engine.borrow_mut().explorer_has_focus = false;
                // tree_has_focus removed (A.2b-2); engine.explorer_has_focus is authoritative
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                self.draw_needed.set(true);
                return;
            }
            // Explorer CRUD keys
            if !ctrl {
                if let Some(ch) = unicode {
                    let is_crud = self
                        .engine
                        .borrow()
                        .settings
                        .explorer_keys
                        .resolve(ch)
                        .is_some();
                    if is_crud {
                        // Defer to avoid borrow conflicts with start_inline_new_entry
                        let s = sender.clone();
                        gtk4::glib::idle_add_local_once(move || {
                            s.input(Msg::ExplorerAction(ch.to_string()));
                        });
                        self.draw_needed.set(true);
                        return;
                    }
                }
            }
            // Let j/k/Up/Down through to TreeView for navigation
            if matches!(key_mapped, "j" | "k" | "Up" | "Down") {
                return; // don't consume — let GTK TreeView handle navigation
            }
            // Other keys while explorer focused — ignore (don't pass to editor)
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
                    engine.handle_ext_sidebar_key(mapped, false, unicode);
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
            if engine.sc_has_focus {
                let (mapped, sc_unicode) = map_gtk_key_with_unicode(key_name.as_str());
                if engine.dialog.is_some() {
                    engine.handle_key(mapped, sc_unicode, ctrl);
                } else {
                    engine.handle_sc_key(mapped, ctrl, sc_unicode);
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
                    engine.handle_debug_sidebar_key(&key_name, ctrl);
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

        let (action, prev_tab) = {
            let mut engine = self.engine.borrow_mut();
            let prev = engine.active_group().active_tab;
            let a = engine.handle_key(&key_name, unicode, ctrl);
            // After any key press in insert mode, reset the AI completion
            // debounce timer so a new suggestion fires after idle.
            if engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
                engine.ai_completion_reset_timer();
            }
            (a, prev)
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

        // Reveal the active file in the sidebar when tab changed (gt/gT/:tabn/:tabp)
        {
            let engine = self.engine.borrow();
            if engine.active_group().active_tab != prev_tab {
                let file_path = engine.file_path().cloned();
                drop(engine);
                if let Some(path) = file_path {
                    self.reveal_path_in_explorer(&path);
                }
            }
        }

        // Ctrl-W h/l overflow: move focus to explorer sidebar
        {
            let overflow = self.engine.borrow_mut().window_nav_overflow.take();
            if let Some(false) = overflow {
                // Left overflow → focus explorer
                sender.input(Msg::FocusExplorer);
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
                let tab_hover = if mx >= 0.0 && lh > 0.0 {
                    tab_close_hit_test(&engine, mx, my, da_w, da_h, lh, cw)
                } else {
                    None
                };
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

                // Sync per-window viewport dimensions from actual window rects
                // so ensure_cursor_visible uses accurate heights (not the rough
                // DrawingArea-based estimate from connect_resize).
                {
                    let mut engine = self.engine.borrow_mut();
                    for (wid, rect) in &rects {
                        let pane_lines = (rect.height / lh).floor() as usize;
                        let pane_cols = (rect.width / cw).floor() as usize;
                        engine.set_viewport_for_window(
                            *wid,
                            pane_lines.max(1),
                            pane_cols.saturating_sub(5).max(1),
                        );
                    }
                }

                // Editor hover: convert mouse pixel position to editor (line, col)
                // and feed into dwell detection for auto-hover popups.
                if mx >= 0.0 {
                    let mut engine = self.engine.borrow_mut();
                    if engine.settings.hover_delay > 0
                        && !engine.editor_hover_has_focus
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
        if self.engine.borrow_mut().poll_project_search() {
            let status = self.engine.borrow().message.clone();
            self.project_search_status = status;
            let s = self.sender.clone();
            self.rebuild_search_results(&s);
            self.draw_needed.set(true);
        }
        if self.engine.borrow_mut().poll_project_replace() {
            let status = self.engine.borrow().message.clone();
            self.project_search_status = status;
            let s = self.sender.clone();
            self.rebuild_search_results(&s);
            self.draw_needed.set(true);
        }
        // LSP: flush debounced didChange notifications and poll for events
        {
            let mut engine = self.engine.borrow_mut();
            // Flush debounced cursor_move hook (plugin events + code action requests).
            if engine.flush_cursor_move_hook() {
                self.draw_needed.set(true);
            }
            engine.lsp_flush_changes();
            if engine.poll_lsp() {
                self.draw_needed.set(true);
            }
            // Format-on-save + :wq/:x deferred quit
            if engine.format_save_quit_ready {
                engine.format_save_quit_ready = false;
                drop(engine);
                sender.input(Msg::QuitConfirmed);
            }
        }
        // Terminal: drain PTY output and refresh display if needed
        if self.engine.borrow_mut().poll_terminal() {
            self.draw_needed.set(true);
        }
        // Run pending terminal commands (e.g. extension installs).
        if self.engine.borrow().pending_terminal_command.is_some() {
            let cmd = self
                .engine
                .borrow_mut()
                .pending_terminal_command
                .take()
                .unwrap();
            sender.input(Msg::RunCommandInTerminal(cmd));
        }
        // DAP: drain adapter events (breakpoint hits, stops, output)
        {
            let mut engine = self.engine.borrow_mut();
            if engine.poll_dap() {
                self.draw_needed.set(true);
            }
            // Auto-switch to Debug sidebar when a session starts.
            if engine.dap_wants_sidebar {
                engine.dap_wants_sidebar = false;
                self.active_panel = SidebarPanel::Debug;
                self.sidebar_visible = true;
                self.draw_needed.set(true);
            }
        }
        // Explicitly redraw the debug sidebar if it's active so the
        // Run/Stop button text and section data stay in sync.
        if self.active_panel == SidebarPanel::Debug {
            if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                da.queue_draw();
            }
        }
        // Explorer refresh after confirmed file move. Phase A.2b-2 removed
        // the inline cell-editor, so there's no widget-destruction race to
        // defer around any more — refresh whenever the engine asks for it.
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            sender.input(Msg::RefreshFileTree);
        }
        // Auto-refresh SC panel periodically to pick up external git
        // changes. Runs four `git` subprocesses (~1 s on a non-trivial
        // workspace) on a background thread — blocking the main thread
        // on this used to peg CPU at ~100 % (see #153). Only spawn when
        // a panel actually needs the data; drain the receiver every
        // tick so the snapshot arrives on the next draw.
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
        // Auto-reload buffers whose files changed on disk.
        if self.last_file_check.elapsed() >= std::time::Duration::from_secs(2) {
            self.last_file_check = std::time::Instant::now();
            if self.engine.borrow_mut().check_file_changes() {
                self.draw_needed.set(true);
            }
        }
        // Poll for completed extension registry fetch.
        {
            let mut engine = self.engine.borrow_mut();
            if engine.poll_ext_registry() {
                drop(engine);
                if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
        }
        // Poll for completed SC diff background request.
        {
            if self.engine.borrow_mut().poll_sc_diff() {
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
        }
        // Poll for completed async shell tasks (plugin background commands).
        {
            if self.engine.borrow_mut().poll_async_shells() {
                self.draw_needed.set(true);
            }
        }
        // Check for panel reveal request from plugins.
        {
            let engine = self.engine.borrow_mut();
            if let Some(panel_name) = engine.ext_panel_focus_pending.clone() {
                drop(engine);
                // Switch directly — don't go through SwitchPanel which
                // would toggle visibility or reset selection set by reveal.
                self.active_panel = SidebarPanel::ExtPanel(panel_name);
                self.sidebar_visible = true;
                self.engine.borrow_mut().ext_panel_focus_pending = None;
                self.draw_needed.set(true);
            }
        }
        // Poll for completed AI response.
        {
            let mut engine = self.engine.borrow_mut();
            if engine.poll_ai() {
                drop(engine);
                if let Some(ref da) = *self.ai_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
        }
        // Tick AI inline completions debounce counter.
        {
            if self.engine.borrow_mut().tick_ai_completion() {
                self.draw_needed.set(true);
            }
        }
        // Poll for panel hover popup (dwell detection).
        {
            let had_hover = self.engine.borrow().panel_hover.is_some();
            let changed = self.engine.borrow_mut().poll_panel_hover();
            let has_hover = self.engine.borrow().panel_hover.is_some();
            if changed || (had_hover && !has_hover) {
                if let Some(ref da) = *self.panel_hover_da.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
        }
        // Poll for editor hover popup (dwell detection / delayed dismiss).
        {
            let changed = self.engine.borrow_mut().poll_editor_hover();
            if changed {
                self.draw_needed.set(true);
            }
        }
        // Poll async blame results.
        {
            let changed = self.engine.borrow_mut().poll_blame();
            if changed {
                self.draw_needed.set(true);
            }
        }
        // Debounced syntax refresh during insert mode — after 150ms of no
        // keystrokes, re-parse + re-extract highlights so stale byte offsets
        // don't cause wrong colors near edited regions.
        if self.engine.borrow_mut().tick_syntax_debounce() {
            self.draw_needed.set(true);
        }
        // Tick swap file writes (only does work when updatetime elapsed).
        self.engine.borrow_mut().tick_swap_files();
        // Poll for external git branch changes (rate-limited to once per 2s inside).
        if self.engine.borrow_mut().tick_git_branch() {
            self.draw_needed.set(true);
        }
        // Auto-dismiss completed notifications after timeout; force redraw for spinner animation.
        {
            let mut engine = self.engine.borrow_mut();
            if engine.has_active_notifications() {
                self.draw_needed.set(true);
            }
            let had_notifs = !engine.notifications.is_empty();
            engine.tick_notifications();
            if had_notifs && engine.notifications.is_empty() {
                self.draw_needed.set(true);
            }
        }
        // Explorer tree indicators (modified/diagnostics) are now pulled by
        // the DrawingArea's draw callback from `engine.explorer_indicators()`
        // via the `explorer_to_tree_view` adapter, so we just trigger a
        // redraw on a 1 Hz cadence to pick up background changes.
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
        // ── Context menu click handling (engine-drawn) ──
        if self.engine.borrow().context_menu.is_some() {
            let cw = self.cached_char_width.max(1.0);
            let lh = self.cached_line_height.max(1.0);
            let click_col = (x / cw) as u16;
            let click_row = (y / lh) as u16;
            let term_w = (width / cw) as u16;
            let term_h = (height / lh) as u16;

            let result = {
                let engine = self.engine.borrow();
                let cm = engine.context_menu.as_ref().unwrap();
                crate::core::engine::resolve_context_menu_click(
                    &cm.items,
                    cm.screen_x,
                    cm.screen_y,
                    term_w,
                    term_h,
                    click_col,
                    click_row,
                )
            };

            use crate::core::engine::ContextMenuClickResult;
            match result {
                ContextMenuClickResult::Item(idx) => {
                    let mut engine = self.engine.borrow_mut();
                    engine.context_menu.as_mut().unwrap().selected = idx;
                    // context_menu_confirm() handles the action internally and
                    // consumes the menu.
                    let _act = engine.context_menu_confirm();
                    let needs_tree_refresh = engine.explorer_needs_refresh;
                    if needs_tree_refresh {
                        engine.explorer_needs_refresh = false;
                    }
                    drop(engine);
                    if needs_tree_refresh {
                        sender.input(Msg::RefreshFileTree);
                    }
                }
                ContextMenuClickResult::InsidePopup => {
                    // Click inside but not on an item — ignore
                }
                ContextMenuClickResult::Outside => {
                    self.engine.borrow_mut().close_context_menu();
                }
            }
            self.draw_needed.set(true);
            return;
        }

        // ── Find/replace overlay click handling (using shared hit regions) ──
        if self.engine.borrow().find_replace_open {
            let cw = self.cached_char_width.max(1.0);
            let lh = self.cached_line_height.max(1.0);

            let (hit_regions, on_panel, rel_col, rel_row) = {
                let engine = self.engine.borrow();

                // Build match_info (same logic as build_screen_layout)
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

                let panel_w = render::FR_PANEL_WIDTH;
                let (hit_regions, _) = render::compute_find_replace_hit_regions(
                    panel_w,
                    engine.find_replace_show_replace,
                    &match_info,
                );

                // Replicate draw.rs pixel layout for panel bounding box
                let pad = 6.0;
                let input_w_px = 200.0;
                let btn_s = lh;
                let chevron_w = 16.0;
                let toggles_w = 3.0 * (btn_s + 4.0);
                let info_w = 80.0;
                let nav_w = 4.0 * (btn_s + 2.0);
                let popup_w = chevron_w + input_w_px + pad + toggles_w + info_w + nav_w + pad;
                let row_count_f = if engine.find_replace_show_replace {
                    2.0
                } else {
                    1.0
                };
                let popup_h = lh * row_count_f + pad * (row_count_f + 1.0);
                let popup_x = (width - popup_w - 10.0).max(0.0);
                let popup_y = lh * 2.5 + 2.0; // approximate position

                let on_panel =
                    x >= popup_x && x < popup_x + popup_w && y >= popup_y && y < popup_y + popup_h;

                // Translate pixel to panel-relative row + char column
                let row_y = popup_y + pad;
                let rel_row = if y >= row_y && y < row_y + lh {
                    0u16
                } else if y >= row_y + lh + pad && y < row_y + lh + pad + lh {
                    1u16
                } else {
                    u16::MAX
                };
                let content_px = popup_x + chevron_w; // content starts after chevron
                let rel_col = ((x - content_px) / cw).max(0.0) as u16;

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

        // Picker popup: intercept all clicks when picker is open
        {
            let engine = self.engine.borrow();
            if engine.picker_open {
                let has_preview = engine.picker_preview.is_some();
                let popup_w = if has_preview {
                    (width * 0.8).max(600.0)
                } else {
                    (width * 0.55).max(500.0)
                };
                let popup_h = if has_preview {
                    (height * 0.65).max(400.0)
                } else {
                    (height * 0.60).max(350.0)
                };
                let popup_x = (width - popup_w) / 2.0;
                let popup_y = (height - popup_h) / 2.0;
                let lh = self.cached_line_height.max(1.0);
                // Results start below separator: popup_y + 2*lh + 1px padding
                let results_top = popup_y + lh * 2.0 + 1.0;
                let results_bottom = popup_y + popup_h;

                let on_popup =
                    x >= popup_x && x < popup_x + popup_w && y >= popup_y && y < popup_y + popup_h;
                let on_results = on_popup && y >= results_top && y < results_bottom;

                drop(engine);
                if on_results {
                    let mut engine = self.engine.borrow_mut();
                    let clicked_idx = engine.picker_scroll_top + ((y - results_top) / lh) as usize;
                    if clicked_idx < engine.picker_items.len() {
                        engine.picker_selected = clicked_idx;
                        engine.picker_load_preview();
                    }
                } else if !on_popup {
                    self.engine.borrow_mut().close_picker();
                }
                // Consume click — don't fall through to editor
                return;
            }
        }

        // Breadcrumb click: the breadcrumb row sits at y = line_height (below tab bar).
        // Use char_width to approximate segment positions.
        {
            let engine = self.engine.borrow();
            if engine.settings.breadcrumbs {
                let lh = self.cached_line_height.max(1.0);
                let cw = self.cached_char_width.max(1.0);
                // Breadcrumb row spans y ∈ [lh, 2*lh)
                if y >= lh && y < lh * 2.0 {
                    // Build segments to find what was clicked.
                    // Also rebuild engine-side segments so scoped filtering works.
                    let segments = crate::render::build_breadcrumbs_for_active_group(&engine);
                    drop(engine);
                    self.engine.borrow_mut().rebuild_breadcrumb_segments();
                    let sep_w = " › ".chars().count() as f64 * cw;
                    let pad = cw; // left padding
                    let mut seg_x = pad;
                    for (i, seg) in segments.iter().enumerate() {
                        let label_w = seg.label.chars().count() as f64 * cw;
                        if x >= seg_x && x < seg_x + label_w {
                            let mut engine = self.engine.borrow_mut();
                            engine.breadcrumb_selected = i;
                            engine.breadcrumb_open_scoped();
                            return;
                        }
                        seg_x += label_w + sep_w;
                    }
                    return; // clicked on breadcrumb row but not a segment
                }
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
                        let cw = self.cached_char_width.max(1.0);
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
                            let content_col = (rel_x / cw).max(0.0) as usize;
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
        if self.engine.borrow().dialog.is_some() {
            let lh = self.cached_line_height.max(1.0);
            let btn_rects = self.dialog_btn_rects.borrow().clone();

            // Use actual button rects from the last draw_dialog_popup call.
            let mut clicked_btn: Option<usize> = None;
            for (idx, &(bx, by, bw, bh)) in btn_rects.iter().enumerate() {
                if x >= bx && x < bx + bw && y >= by && y < by + bh {
                    clicked_btn = Some(idx);
                    break;
                }
            }

            // Compute popup bounds for outside-click detection.
            let engine = self.engine.borrow();
            let dialog = engine.dialog.as_ref().unwrap();
            let popup_h = ((3.0 + dialog.body.len() as f64 + 2.0) * lh).min(height - 40.0);
            // Approximate popup width from button rects span.
            let (popup_x, popup_w) =
                if let (Some(first), Some(last)) = (btn_rects.first(), btn_rects.last()) {
                    // Buttons start at popup_x + 12, so popup_x = first.0 - 12
                    let px = first.0 - 12.0;
                    // popup_w is at least wide enough to contain all buttons + padding
                    let pw = (last.0 + last.2 - px + 12.0).max(350.0);
                    (px, pw)
                } else {
                    ((width - 350.0) / 2.0, 350.0)
                };
            let popup_y_pos = (height - popup_h) / 2.0;
            let outside = x < popup_x
                || x >= popup_x + popup_w
                || y < popup_y_pos
                || y >= popup_y_pos + popup_h;
            drop(engine);

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
            } else if outside {
                self.engine.borrow_mut().dialog = None;
                self.engine.borrow_mut().pending_move = None;
            }
            self.draw_needed.set(true);
        } else {
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

            // Snapshot the active file path before processing the click so we
            // can detect tab switches (and only then highlight in the tree).
            let file_before_click = self.engine.borrow().file_path().cloned();
            // Clicking in the editor clears debug sidebar focus.
            self.engine.borrow_mut().dap_sidebar_has_focus = false;
            // Check if click lands in the terminal panel before general handling.
            // Layout (bottom to top): status | toolbar | terminal | quickfix | DAP | editor
            let in_terminal = if self.cached_line_height > 0.0 {
                let engine = self.engine.borrow();
                if engine.terminal_open || engine.bottom_panel_open {
                    let term_px =
                        (engine.session.terminal_panel_rows as f64 + 2.0) * self.cached_line_height;
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
                    // Click on the tab bar row: switch active bottom panel tab.
                    // Sans-serif chars are ~60% of monospace width; use that estimate.
                    let cw = self.cached_char_width.max(1.0) * 0.6;
                    let padding = 12.0;
                    // Close button (×) at right edge
                    if x >= width - padding - 10.0 {
                        let mut engine = self.engine.borrow_mut();
                        engine.bottom_panel_open = false;
                        engine.close_terminal();
                        drop(engine);
                        sender.input(Msg::Resize);
                        return;
                    }
                    let terminal_label = "TERMINAL";
                    let debug_label = "DEBUG CONSOLE";
                    let terminal_w = padding + terminal_label.len() as f64 * cw + padding;
                    let tab_x = x - padding; // offset matches cursor_x start
                    let new_kind = if tab_x < terminal_w {
                        render::BottomPanelKind::Terminal
                    } else if tab_x < terminal_w + debug_label.len() as f64 * cw + padding * 2.0 {
                        render::BottomPanelKind::DebugOutput
                    } else {
                        self.engine.borrow().bottom_panel_kind.clone()
                    };
                    self.engine.borrow_mut().bottom_panel_kind = new_kind;
                    sender.input(Msg::Resize); // triggers redraw
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
                        // 6px scrollbar strip on the right edge — start a scrollbar drag.
                        if x >= width - SB_W {
                            self.terminal_sb_dragging = true;
                        } else {
                            self.terminal_sb_dragging = false;
                            self.terminal_resize_dragging = false;
                            let row = ((y - term_y - 2.0 * self.cached_line_height)
                                / self.cached_line_height)
                                as u16;
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
                    }
                } else {
                    // Header row click — tab switch, toolbar buttons, or resize drag.
                    const TERMINAL_TAB_COLS: usize = 4;
                    let tab_count = self.engine.borrow().terminal_panes.len();
                    let tab_area_px =
                        tab_count as f64 * TERMINAL_TAB_COLS as f64 * self.cached_char_width;
                    // Right-aligned buttons (2 cols each): + ⊞ □ ×
                    let close_x = width - self.cached_char_width * 2.0;
                    let max_x = width - self.cached_char_width * 4.0;
                    let split_x = width - self.cached_char_width * 6.0;
                    let add_x = width - self.cached_char_width * 8.0;
                    if x < tab_area_px && self.cached_char_width > 0.0 {
                        let idx =
                            (x / (TERMINAL_TAB_COLS as f64 * self.cached_char_width)) as usize;
                        sender.input(Msg::TerminalSwitchTab(idx));
                    } else if x >= close_x {
                        sender.input(Msg::TerminalCloseActiveTab);
                    } else if x >= max_x {
                        sender.input(Msg::ToggleTerminalMaximize);
                    } else if x >= split_x {
                        sender.input(Msg::TerminalToggleSplit);
                    } else if x >= add_x {
                        sender.input(Msg::NewTerminalTab);
                    } else {
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
                // If the click lands on a Cairo h scrollbar, start a drag and
                // don't pass the click through to the editor.
                {
                    let lh = self.cached_line_height;
                    let cw = self.cached_char_width;
                    let engine = self.engine.borrow();
                    let rects = compute_editor_window_rects(&engine, width, height, lh);
                    if let Some((win_id, px_per_col, scroll_left)) =
                        h_scrollbar_hit_test(&engine, x, y, &rects, cw, lh)
                    {
                        drop(engine);
                        self.h_sb_dragging = Some(HScrollDragState {
                            window_id: win_id,
                            drag_start_x: x,
                            scroll_left_at_start: scroll_left,
                            px_per_col,
                        });
                        self.h_sb_drag_cell.set(Some(win_id));
                        self.draw_needed.set(true);
                        return; // consume click; don't send it to the editor
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
                        // Clear selection on click in VSCode mode.
                        if engine.is_vscode_mode() {
                            engine.vscode_clear_selection();
                        }
                        let (click_result, engine_action) = handle_mouse_click(
                            &mut engine,
                            x,
                            y,
                            width,
                            height,
                            alt,
                            self.cached_line_height,
                            self.cached_char_width,
                            &self.tab_slot_positions.borrow(),
                            &self.diff_btn_map.borrow(),
                            &self.split_btn_map.borrow(),
                            &self.action_btn_map.borrow(),
                            &self.status_segment_map.borrow(),
                        );
                        match engine_action {
                            Some(core::engine::EngineAction::ToggleSidebar) => {
                                drop(engine);
                                sender.input(Msg::ToggleSidebar);
                                self.draw_needed.set(true);
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
                                // Defer sidebar tree highlight so tab switch renders instantly.
                                let new_file_path = engine.file_path().cloned();
                                drop(engine);
                                if new_file_path != file_before_click {
                                    if let Some(path) = new_file_path {
                                        self.reveal_path_in_explorer(&path);
                                    }
                                }
                                self.draw_needed.set(true);
                                return;
                            }
                        }
                    }

                    // Fire cursor_move hook so plugins (e.g. git-insights blame)
                    // see the new cursor position after a mouse click.
                    engine.fire_cursor_move_hook();

                    // Reveal the active file in the sidebar tree only when the
                    // active file actually changed (e.g. tab click), NOT on every
                    // editor click.
                    let new_file_path = engine.file_path().cloned();
                    drop(engine);
                    if new_file_path != file_before_click {
                        if let Some(path) = new_file_path {
                            self.reveal_path_in_explorer(&path);
                        }
                    }
                    self.draw_needed.set(true);
                }
            }
        } // close else (dialog not open)
    }

    fn handle_mouse_drag_msg(&mut self, x: f64, y: f64, width: f64, height: f64) {
        // Editor hover popup text selection drag
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
                    let cw = self.cached_char_width.max(1.0);
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
                    let content_col = (rel_x / cw).max(0.0) as usize;
                    self.engine
                        .borrow_mut()
                        .editor_hover_extend_selection(content_line, content_col);
                    self.draw_needed.set(true);
                    return;
                }
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
                // Determine which tab was clicked using pixel_to_click_target.
                let mut engine = self.engine.borrow_mut();
                let target = pixel_to_click_target(
                    &mut engine,
                    sx,
                    sy,
                    width,
                    height,
                    self.cached_line_height,
                    self.cached_char_width,
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
        // H scrollbar thumb drag — convert pointer delta to scroll_left.
        if let Some(ref state) = self.h_sb_dragging {
            if state.px_per_col > 0.0 {
                let delta_cols = ((x - state.drag_start_x) / state.px_per_col).round() as isize;
                let new_left = (state.scroll_left_at_start as isize + delta_cols).max(0) as usize;
                self.engine
                    .borrow_mut()
                    .set_scroll_left_for_window(state.window_id, new_left);
                self.draw_needed.set(true);
            }
            return;
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
        } else if self.terminal_sb_dragging {
            let (term_rows, scrollback_rows) = {
                let engine = self.engine.borrow();
                if let Some(term) = engine.active_terminal() {
                    (term.rows, term.history.len())
                } else {
                    (0, 0)
                }
            };
            if term_rows > 0 {
                let term_px = (self.engine.borrow().session.terminal_panel_rows as f64 + 2.0)
                    * self.cached_line_height;
                let global_status_rows = if self.engine.borrow().settings.window_status_line {
                    0.0
                } else {
                    1.0
                };
                let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                let toolbar_px = if self.engine.borrow().debug_toolbar_visible {
                    self.cached_line_height
                } else {
                    0.0
                };
                let term_y = height - status_h - toolbar_px - term_px;
                let content_y = term_y + 2.0 * self.cached_line_height;
                let content_h = term_px - 2.0 * self.cached_line_height;
                if scrollback_rows > 0 && content_h > 0.0 {
                    let y_rel = (y - content_y).clamp(0.0, content_h);
                    let frac = y_rel / content_h;
                    // frac=0 (top) → max scroll; frac=1 (bottom) → live view
                    let new_offset = ((1.0 - frac) * scrollback_rows as f64) as usize;
                    if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                        term.set_scroll_offset(new_offset.min(scrollback_rows));
                    }
                }
            }
            self.draw_needed.set(true);
        } else {
            // Check if drag is in the terminal content area (text selection).
            let in_terminal = if self.cached_line_height > 0.0 {
                let engine = self.engine.borrow();
                if engine.terminal_open || engine.bottom_panel_open {
                    let term_px =
                        (engine.session.terminal_panel_rows as f64 + 2.0) * self.cached_line_height;
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
                let mut engine = self.engine.borrow_mut();
                handle_mouse_drag(
                    &mut engine,
                    x,
                    y,
                    width,
                    height,
                    self.cached_line_height,
                    self.cached_char_width,
                    &self.tab_slot_positions.borrow(),
                    &self.diff_btn_map.borrow(),
                    &self.split_btn_map.borrow(),
                    &self.action_btn_map.borrow(),
                    &self.status_segment_map.borrow(),
                );
                self.draw_needed.set(true);
            }
        }
    }

    fn handle_mouse_up_msg(&mut self) {
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
        self.terminal_sb_dragging = false;
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
        self.h_sb_dragging = None;
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
                let target = self.terminal_target_maximize_rows();
                let cols = self.terminal_cols();
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.toggle_terminal_maximize();
                }
                // Create a pane if none exists; otherwise resize the existing
                // PTY to the new effective content rows.
                let needs_new_tab = self.engine.borrow().terminal_panes.is_empty();
                let effective = self
                    .engine
                    .borrow()
                    .effective_terminal_panel_rows(target);
                if needs_new_tab {
                    self.engine.borrow_mut().terminal_new_tab(cols, effective);
                } else {
                    self.engine
                        .borrow_mut()
                        .terminal_resize(cols, effective);
                }
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

    fn handle_menu_msg(&mut self, msg: Msg, sender: &ComponentSender<Self>) {
        match msg {
            Msg::ToggleMenuBar => {
                // In GTK the menu bar is always on (it's our title bar).
                // Just queue a redraw so the menu labels re-render.
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::OpenMenu(idx) => {
                self.engine.borrow_mut().open_menu(idx);
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    da.queue_draw();
                }
                // Enable the full-window overlay so it captures dropdown clicks.
                if let Some(ref da) = *self.menu_dropdown_da.borrow() {
                    da.set_can_target(true);
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::CloseMenu => {
                self.engine.borrow_mut().close_menu();
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    da.queue_draw();
                }
                // Disable overlay so normal clicks pass through again.
                if let Some(ref da) = *self.menu_dropdown_da.borrow() {
                    da.set_can_target(false);
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
            Msg::MenuActivateItem(menu_idx, item_idx, action) => {
                // Close the menu engine-side for every action.
                self.engine.borrow_mut().close_menu();
                // Intercept dialog actions that need GTK-side handling
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
                        let engine_action = self
                            .engine
                            .borrow_mut()
                            .menu_activate_item(menu_idx, item_idx, &action);
                        match engine_action {
                            EngineAction::Quit | EngineAction::SaveQuit => {
                                sender.input(Msg::QuitConfirmed);
                            }
                            EngineAction::QuitWithUnsaved => {
                                sender.input(Msg::ShowQuitConfirm);
                            }
                            EngineAction::ToggleSidebar => {
                                sender.input(Msg::ToggleSidebar);
                            }
                            EngineAction::OpenTerminal => {
                                sender.input(Msg::NewTerminalTab);
                            }
                            _ => {}
                        }
                    }
                }
                // Disable overlay so clicks pass through again after item selection.
                if let Some(ref da) = *self.menu_dropdown_da.borrow() {
                    da.set_can_target(false);
                    da.queue_draw();
                }
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::MenuHighlight(idx) => {
                self.engine.borrow_mut().menu_highlighted_item = idx;
                if let Some(ref da) = *self.menu_dropdown_da.borrow() {
                    da.queue_draw();
                }
            }
            _ => unreachable!(),
        }
    }

    fn handle_debug_sidebar_msg(&mut self, msg: Msg) {
        match msg {
            Msg::DebugSidebarClick(click_x, y) => {
                use crate::core::engine::DebugSidebarSection;
                let lh = self.cached_line_height;
                let row_idx = (y / lh) as u16;
                let mut engine = self.engine.borrow_mut();

                // Compute section heights for click mapping.
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    if lh > 0.0 {
                        let da_h = da.height() as f64;
                        let content_px = (da_h - 6.0 * lh).max(0.0);
                        let per_sec = (content_px / 4.0 / lh).floor() as u16;
                        engine.dap_sidebar_section_heights = [per_sec; 4];
                    }
                }

                // Give focus to the debug sidebar
                engine.dap_sidebar_has_focus = true;
                // tree_has_focus removed (A.2b-2); engine.explorer_has_focus is authoritative

                if row_idx == 0 {
                    // Header — no-op
                } else if row_idx == 1 {
                    // Run/Stop button
                    if engine.dap_session_active && engine.dap_stopped_thread.is_some() {
                        engine.dap_continue();
                    } else if engine.dap_session_active {
                        engine.execute_command("stop");
                    } else {
                        engine.execute_command("debug");
                    }
                } else {
                    // Walk sections using fixed-allocation layout:
                    // row 2+ = [section_header(1) + content(height)]×4
                    let sections = [
                        (DebugSidebarSection::Variables, 0usize),
                        (DebugSidebarSection::Watch, 1),
                        (DebugSidebarSection::CallStack, 2),
                        (DebugSidebarSection::Breakpoints, 3),
                    ];
                    let mut cur_row: u16 = 2;
                    for (section, sec_idx) in &sections {
                        let sec_height = engine.dap_sidebar_section_heights[*sec_idx];
                        let section_header_row = cur_row;
                        let items_start = cur_row + 1;
                        let items_end = items_start + sec_height;

                        if row_idx == section_header_row {
                            engine.dap_sidebar_section = *section;
                            engine.dap_sidebar_selected = 0;
                            break;
                        } else if row_idx >= items_start && row_idx < items_end {
                            let item_count = engine.dap_sidebar_section_item_count(*section);
                            let height = sec_height as usize;
                            // Scrollbar click: rightmost 6px when items overflow.
                            let da_w = self
                                .debug_sidebar_da_ref
                                .borrow()
                                .as_ref()
                                .map(|da| da.width() as f64)
                                .unwrap_or(200.0);
                            if click_x >= da_w - 6.0 && item_count > height && height > 0 {
                                let rel_row = (row_idx - items_start) as usize;
                                let ratio = rel_row as f64 / height as f64;
                                let max_scroll = item_count.saturating_sub(height);
                                engine.dap_sidebar_scroll[*sec_idx] =
                                    (ratio * max_scroll as f64) as usize;
                                engine.dap_sidebar_section = *section;
                            } else {
                                let scroll_off = engine.dap_sidebar_scroll[*sec_idx];
                                let row_offset = (row_idx - items_start) as usize;
                                let item_idx = scroll_off + row_offset;
                                if item_count > 0 && item_idx < item_count {
                                    engine.dap_sidebar_section = *section;
                                    engine.dap_sidebar_selected = item_idx;
                                    engine.handle_debug_sidebar_key("Return", false);
                                }
                            }
                            break;
                        }
                        cur_row = items_end;
                    }
                }
                drop(engine);

                // Grab focus on the debug sidebar DA
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
                // Compute section heights for ensure_visible.
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    let lh = self.cached_line_height;
                    if lh > 0.0 {
                        let da_h = da.height() as f64;
                        let content_px = (da_h - 6.0 * lh).max(0.0);
                        let per_sec = (content_px / 4.0 / lh).floor() as u16;
                        engine.dap_sidebar_section_heights = [per_sec; 4];
                    }
                }
                let mapped = map_gtk_key_name(key_name.as_str());
                engine.handle_debug_sidebar_key(mapped, ctrl);
                let still_focused = engine.dap_sidebar_has_focus;
                drop(engine);
                self.focus_editor_if_needed(still_focused);
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::DebugSidebarScroll(dy) => {
                let mut engine = self.engine.borrow_mut();
                // Compute section heights.
                let lh = self.cached_line_height;
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    if lh > 0.0 {
                        let da_h = da.height() as f64;
                        let content_px = (da_h - 6.0 * lh).max(0.0);
                        let per_sec = (content_px / 4.0 / lh).floor() as u16;
                        engine.dap_sidebar_section_heights = [per_sec; 4];
                    }
                }
                // Scroll the active section.
                let scroll_amount = (dy.abs() * 3.0).ceil() as usize;
                let sec = engine.dap_sidebar_section;
                let idx = Engine::dap_sidebar_section_index(sec);
                let item_count = engine.dap_sidebar_section_item_count(sec);
                let height = engine.dap_sidebar_section_heights[idx] as usize;
                let max_scroll = item_count.saturating_sub(height);
                if dy > 0.0 {
                    engine.dap_sidebar_scroll[idx] =
                        (engine.dap_sidebar_scroll[idx] + scroll_amount).min(max_scroll);
                } else {
                    engine.dap_sidebar_scroll[idx] =
                        engine.dap_sidebar_scroll[idx].saturating_sub(scroll_amount);
                }
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
                // Update selection/focus in one borrow scope.
                // Returns: Some("Return") = open file, Some("Tab") = toggle expand, None = no-op
                let action: Option<&'static str> = {
                    let mut engine = self.engine.borrow_mut();
                    engine.sc_has_focus = true;
                    // Pixel-based hit zones matching draw_source_control_panel layout:
                    //   header:  0 .. lh
                    //   gap:     lh .. lh+gap
                    //   commit:  lh+gap .. lh+gap+commit_h
                    //   gap:     .. + gap
                    //   buttons: .. + lh
                    //   gap:     .. + gap
                    //   sections: item_height rows
                    let gap = (lh * 0.3).round();
                    let commit_rows = engine.sc_commit_message.split('\n').count().max(1);
                    let commit_h = commit_rows as f64 * lh;
                    let header_end = lh;
                    let commit_top = header_end + gap;
                    let commit_bottom = commit_top + commit_h;
                    let btn_top = commit_bottom + gap;
                    let btn_bottom = btn_top + lh;
                    let section_top = btn_bottom + gap;
                    let item_height = (lh * 1.4).round();

                    if y < header_end {
                        // Panel header — no-op
                        engine.sc_commit_input_active = false;
                        None
                    } else if y >= commit_top && y < commit_bottom {
                        // Commit input row(s)
                        engine.sc_commit_input_active = true;
                        engine.sc_commit_cursor = engine.sc_commit_message.len();
                        None
                    } else if y >= btn_top && y < btn_bottom {
                        engine.sc_commit_input_active = false;
                        // Button row: Commit (~50%), Push/Pull/Sync (~17% each, icon-only).
                        if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                            let da_w = da.width() as f64;
                            let margin = 4.0;
                            let btn_w = da_w - margin * 2.0;
                            let rel_x = x_click - margin;
                            if btn_w > 0.0 && rel_x >= 0.0 && rel_x < btn_w {
                                let commit_w = btn_w / 2.0;
                                let btn_idx = if rel_x < commit_w {
                                    0
                                } else {
                                    let icon_w = (btn_w - commit_w) / 3.0;
                                    ((1.0 + (rel_x - commit_w) / icon_w) as usize).min(3)
                                };
                                engine.sc_activate_button(btn_idx);
                            }
                        }
                        None
                    } else if y >= section_top {
                        engine.sc_commit_input_active = false;
                        // Accumulator walk matching draw_source_control_panel layout:
                        // headers use line_height, items use item_height (1.4×).
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
                        let expanded = engine.sc_sections_expanded;

                        // Build section descriptors: (item_count, is_shown, expanded)
                        let sections: [(usize, bool, bool); 4] = [
                            (staged_count, true, expanded[0]),
                            (unstaged_count, true, expanded[1]),
                            (wt_count, show_worktrees, expanded[2]),
                            (log_count, true, expanded[3]),
                        ];

                        let mut ry = section_top;
                        let mut flat: usize = 0;
                        let mut result: Option<(usize, bool)> = None;

                        'walk: for &(count, shown, exp) in &sections {
                            if !shown {
                                continue;
                            }
                            // Section header (line_height)
                            if y >= ry && y < ry + lh {
                                result = Some((flat, true));
                                break 'walk;
                            }
                            ry += lh;
                            let header_flat = flat;
                            flat += 1;
                            if exp {
                                for i in 0..count {
                                    if y >= ry && y < ry + item_height {
                                        result = Some((header_flat + 1 + i, false));
                                        break 'walk;
                                    }
                                    ry += item_height;
                                }
                                flat += count;
                            }
                        }

                        match result {
                            Some((flat_idx, is_header)) => {
                                engine.sc_selected = flat_idx;
                                if is_header {
                                    Some("Tab")
                                } else if n_press >= 2 {
                                    Some("Return")
                                } else {
                                    None // single-click: just select
                                }
                            }
                            None => None,
                        }
                    } else {
                        // Gap/padding area — no-op
                        engine.sc_commit_input_active = false;
                        None
                    }
                };
                if let Some(key) = action {
                    if key == "Return" {
                        // Defer all heavy work (file open + git show) so
                        // the sidebar repaints the selection highlight first.
                        let engine_rc = self.engine.clone();
                        let git_da = self.git_sidebar_da_ref.clone();
                        let drawing = self.drawing_area.clone();
                        let draw_needed = self.draw_needed.clone();
                        gtk4::glib::idle_add_local_once(move || {
                            let done = engine_rc.borrow_mut().sc_open_selected_async();
                            if done {
                                let still_focused = engine_rc.borrow().sc_has_focus;
                                if !still_focused {
                                    if let Some(ref da) = *drawing.borrow() {
                                        da.grab_focus();
                                    }
                                }
                            }
                            if let Some(ref da) = *git_da.borrow() {
                                da.queue_draw();
                            }
                            draw_needed.set(true);
                        });
                    } else {
                        self.engine.borrow_mut().handle_sc_key(key, false, None);
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
                    if rel_x < 0.0 || rel_x >= btn_w {
                        engine.sc_button_hovered = None;
                    } else {
                        let commit_w = btn_w / 2.0;
                        engine.sc_button_hovered = Some(if rel_x < commit_w {
                            0
                        } else {
                            let icon_w = (btn_w - commit_w) / 3.0;
                            ((1.0 + (rel_x - commit_w) / icon_w) as usize).min(3)
                        });
                    }
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
                if engine.sc_commit_input_active
                    || engine.sc_branch_picker_open
                    || engine.sc_branch_create_mode
                    || engine.sc_help_open
                {
                    // In input/popup mode, pass everything through.
                    let (mapped_key, unicode) = map_gtk_key_with_unicode(key_name.as_str());
                    engine.handle_sc_key(mapped_key, ctrl, unicode);
                } else {
                    // Normal navigation: map known keys only.
                    let mapped = match key_name.as_str() {
                        "Return" | "KP_Enter" => "Return",
                        "Escape" => "Escape",
                        "Tab" => "Tab",
                        "Down" => "j",
                        "Up" => "k",
                        "Left" => "h",
                        "Right" => "l",
                        "BackSpace" => "BackSpace",
                        "j" => "j",
                        "k" => "k",
                        "h" => "h",
                        "l" => "l",
                        "s" => "s",
                        "S" => "S",
                        "d" => "d",
                        "D" => "D",
                        "r" => "r",
                        "q" => "q",
                        "c" => "c",
                        "p" => "p",
                        "P" => "P",
                        "f" => "f",
                        "b" => "b",
                        "B" => "B",
                        "question" | "?" => "?",
                        _ => "",
                    };
                    if !mapped.is_empty() {
                        engine.handle_sc_key(mapped, ctrl, None);
                    }
                }
                let still_focused = engine.sc_has_focus;
                drop(engine);
                self.focus_editor_if_needed(still_focused);
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            _ => unreachable!(),
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
                engine.handle_ext_sidebar_key(mapped, false, unicode);
                let still_focused = engine.ext_sidebar_has_focus;
                let has_dialog = engine.dialog.is_some();
                drop(engine);
                self.focus_editor_if_needed(still_focused && !has_dialog);
                if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
            }
            Msg::ExtSidebarClick(x_click, y_click, n_press) => {
                let mut engine = self.engine.borrow_mut();
                // Compute line_height from UI font (not editor font) to match drawing.
                let line_height = self.cached_ui_line_height.max(1.0);
                // Item rows use 1.4× line_height for spacing.
                let item_height = (line_height * 1.4_f64).ceil();
                // Walk the layout to determine which row was clicked.
                // Headers use line_height, items use item_height.
                let mut ry: f64 = 0.0;
                engine.ext_sidebar_has_focus = true;

                // Row 0: panel header (line_height)
                if y_click < ry + line_height {
                    // no action on header click
                } else {
                    ry += line_height;
                }
                // Row 1: search box (line_height)
                if y_click >= ry && y_click < ry + line_height {
                    engine.ext_sidebar_input_active = true;
                } else if y_click >= ry + line_height {
                    ry += line_height;
                    // INSTALLED section header (line_height)
                    let installed = engine.ext_installed_items().len();
                    let inst_expanded = engine.ext_sidebar_sections_expanded[0];
                    if y_click >= ry && y_click < ry + line_height {
                        engine.ext_sidebar_sections_expanded[0] = !inst_expanded;
                    } else {
                        ry += line_height;
                        if inst_expanded {
                            let inst_len = if installed == 0 { 1 } else { installed };
                            let items_h = inst_len as f64 * item_height;
                            if installed > 0 && y_click >= ry && y_click < ry + items_h {
                                let idx = ((y_click - ry) / item_height) as usize;
                                engine.ext_sidebar_selected = idx.min(installed.saturating_sub(1));
                            }
                            ry += items_h;
                        }
                        // AVAILABLE section header (line_height)
                        let avail_expanded = engine.ext_sidebar_sections_expanded[1];
                        if y_click >= ry && y_click < ry + line_height {
                            engine.ext_sidebar_sections_expanded[1] = !avail_expanded;
                        } else {
                            ry += line_height;
                            if avail_expanded {
                                let available = engine.ext_available_items().len();
                                if available > 0 && y_click >= ry {
                                    let idx = ((y_click - ry) / item_height) as usize;
                                    engine.ext_sidebar_selected =
                                        installed + idx.min(available.saturating_sub(1));
                                }
                            }
                        }
                    }
                }
                let _ = x_click;
                // Double-click opens the README
                if n_press >= 2 {
                    engine.ext_open_selected_readme();
                    let still_focused = engine.ext_sidebar_has_focus;
                    drop(engine);
                    self.focus_editor_if_needed(still_focused);
                } else {
                    drop(engine);
                }
                if let Some(ref da) = *self.ext_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
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
                } else if need_sb && x_click >= form_right {
                    // Scrollbar track — jump-scroll so the click position maps
                    // to the centre of the thumb (same behaviour as TUI).
                    let track_len = body_h;
                    let max_scroll = total.saturating_sub(visible_rows);
                    let rel = (y_click - body_top).clamp(0.0, track_len);
                    let ratio = if track_len > 0.0 {
                        rel / track_len
                    } else {
                        0.0
                    };
                    engine.settings_scroll_top = (ratio * max_scroll as f64).round() as usize;
                } else if row_h > 0.0 {
                    // Body row.
                    let local = ((y_click - body_top) / row_h) as usize;
                    let scroll = engine.settings_scroll_top;
                    let flat_idx = scroll + local;
                    if flat_idx < total {
                        engine.settings_selected = flat_idx;
                        if n_press >= 2 {
                            // Double-click = act on the row (toggle, expand,
                            // open editor for Integer/StringVal — same as Enter
                            // in keyboard nav).
                            let row = engine.settings_flat_list()[flat_idx].clone();
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

    fn handle_sidebar_panel_msg(&mut self, msg: Msg, _sender: &ComponentSender<Self>) {
        match msg {
            Msg::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.draw_needed.set(true);

                // Directly control the revealer and panel visibility.
                let show = self.sidebar_visible;
                let p = self.active_panel.clone();
                if let Some(ref r) = *self.sidebar_revealer.borrow() {
                    r.set_reveal_child(show);
                }
                for (which, panel_ref) in [
                    (SidebarPanel::Explorer, &self.explorer_panel_box),
                    (SidebarPanel::Search, &self.search_panel_box),
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
                // Save sidebar visibility to session state
                let mut engine = self.engine.borrow_mut();
                engine.session.explorer_visible = self.sidebar_visible;
                let _ = engine.session.save();
            }
            Msg::SwitchPanel(panel) => {
                if self.active_panel == panel {
                    // Same panel - toggle visibility
                    self.sidebar_visible = !self.sidebar_visible;
                    // Set engine focus flags when toggling back to visible
                    if self.sidebar_visible {
                        match panel {
                            SidebarPanel::Git => {
                                self.engine.borrow_mut().sc_has_focus = true;
                            }
                            SidebarPanel::Extensions => {
                                self.engine.borrow_mut().ext_sidebar_has_focus = true;
                            }
                            SidebarPanel::Ai => {
                                self.engine.borrow_mut().ai_has_focus = true;
                            }
                            SidebarPanel::Debug => {
                                self.engine.borrow_mut().dap_sidebar_has_focus = true;
                            }
                            SidebarPanel::ExtPanel(ref name) => {
                                let mut engine = self.engine.borrow_mut();
                                engine.ext_panel_has_focus = true;
                                engine.ext_panel_active = Some(name.clone());
                            }
                            SidebarPanel::Settings => {
                                self.engine.borrow_mut().settings_has_focus = true;
                            }
                            _ => {}
                        }
                    }
                } else {
                    // Different panel - switch and ensure visible
                    // Clear ext panel focus when switching away
                    if matches!(self.active_panel, SidebarPanel::ExtPanel(_)) {
                        let mut engine = self.engine.borrow_mut();
                        engine.ext_panel_has_focus = false;
                        engine.ext_panel_active = None;
                    }
                    self.active_panel = panel;
                    self.sidebar_visible = true;
                    // Refresh SC data when switching to the Git panel
                    if self.active_panel == SidebarPanel::Git {
                        let mut engine = self.engine.borrow_mut();
                        engine.sc_refresh();
                        engine.sc_has_focus = true;
                    }
                    // Focus + refresh when switching to Extensions panel
                    if self.active_panel == SidebarPanel::Extensions {
                        let mut engine = self.engine.borrow_mut();
                        engine.ext_sidebar_has_focus = true;
                        // Auto-fetch registry if not already done
                        if engine.ext_registry.is_none() && !engine.ext_registry_fetching {
                            engine.ext_refresh();
                        }
                    }
                    // Focus when switching to AI panel
                    if self.active_panel == SidebarPanel::Ai {
                        self.engine.borrow_mut().ai_has_focus = true;
                    }
                    // Focus + fire panel_focus event for ext panels
                    if let SidebarPanel::ExtPanel(ref name) = self.active_panel {
                        let mut engine = self.engine.borrow_mut();
                        let already_active = engine.ext_panel_active.as_deref() == Some(name);
                        engine.ext_panel_has_focus = true;
                        engine.ext_panel_active = Some(name.clone());
                        if !already_active {
                            engine.ext_panel_selected = 0;
                            engine.plugin_event("panel_focus", name);
                        }
                    }
                    // Phase A.3c-2: Settings panel is a stateless DrawingArea; just
                    // give it focus so key events route to handle_settings_key.
                    if self.active_panel == SidebarPanel::Settings {
                        self.engine.borrow_mut().settings_has_focus = true;
                        if let Some(ref da) = *self.settings_da_ref.borrow() {
                            da.queue_draw();
                        }
                    }
                }
                // Directly set visibility on the revealer and each panel box.
                let p = self.active_panel.clone();
                let show_sidebar = self.sidebar_visible;
                if let Some(ref r) = *self.sidebar_revealer.borrow() {
                    r.set_reveal_child(show_sidebar);
                }
                for (which, panel_ref) in [
                    (SidebarPanel::Explorer, &self.explorer_panel_box),
                    (SidebarPanel::Search, &self.search_panel_box),
                    (SidebarPanel::Debug, &self.debug_panel_box),
                    (SidebarPanel::Git, &self.git_panel_box),
                    (SidebarPanel::Extensions, &self.ext_panel_box),
                    (SidebarPanel::Settings, &self.settings_panel_box),
                    (SidebarPanel::Ai, &self.ai_panel_box_ref),
                ] {
                    if let Some(ref b) = *panel_ref.borrow() {
                        b.set_visible(show_sidebar && p == which);
                    }
                }
                // Extension-provided panel box: visible when any ExtPanel variant is active
                if let Some(ref b) = *self.ext_dyn_panel_box.borrow() {
                    b.set_visible(show_sidebar && matches!(p, SidebarPanel::ExtPanel(_)));
                }
                // Grab focus on sidebar DA AFTER visibility is set (hidden widgets can't accept focus).
                if show_sidebar {
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
                // A.6f: mirror the active panel into the shared cell read by the
                // activity-bar draw callback, and queue a redraw so the accent
                // bar updates.
                if let Some(mirror) = self.activity_bar_active_panel.borrow().as_ref() {
                    *mirror.borrow_mut() = self.active_panel.clone();
                }
                if let Some(ref da) = *self.activity_bar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.draw_needed.set(true);
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
                self.reveal_path_in_explorer(&path);
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
                self.reveal_path_in_explorer(&path);
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
            Msg::StartInlineNewFile(parent_dir) => {
                sender.input(Msg::PromptNewFile(parent_dir));
            }
            Msg::StartInlineNewFolder(parent_dir) => {
                sender.input(Msg::PromptNewFolder(parent_dir));
            }
            Msg::ExplorerActivateSelected => {
                let state = self.explorer_state.borrow();
                if state.selected < state.rows.len() {
                    let row = &state.rows[state.selected];
                    let path = row.path.clone();
                    let is_dir = row.is_dir;
                    drop(state);
                    if is_dir {
                        let root = self.engine.borrow().cwd.clone();
                        let show_hidden = self.engine.borrow().settings.show_hidden_files;
                        let case_insensitive =
                            self.engine.borrow().settings.explorer_sort_case_insensitive;
                        let idx = self.explorer_state.borrow().selected;
                        self.explorer_state.borrow_mut().toggle_dir(
                            idx,
                            &root,
                            show_hidden,
                            case_insensitive,
                        );
                        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                            da.queue_draw();
                        }
                    } else {
                        sender.input(Msg::OpenFileFromSidebar(path));
                    }
                }
            }
            Msg::ExplorerAction(key_str) => {
                use crate::core::settings::ExplorerAction;
                let action = key_str
                    .chars()
                    .next()
                    .and_then(|ch| self.engine.borrow().settings.explorer_keys.resolve(ch));
                if let Some(action) = action {
                    let selected_path: Option<PathBuf> = {
                        let state = self.explorer_state.borrow();
                        if state.selected < state.rows.len() {
                            Some(state.rows[state.selected].path.clone())
                        } else {
                            None
                        }
                    };
                    let parent_dir = match selected_path.as_ref() {
                        Some(p) if p.is_dir() => p.clone(),
                        Some(p) => p
                            .parent()
                            .map(|pp| pp.to_path_buf())
                            .unwrap_or_else(|| self.engine.borrow().cwd.clone()),
                        None => self.engine.borrow().cwd.clone(),
                    };
                    match action {
                        ExplorerAction::NewFile => {
                            sender.input(Msg::PromptNewFile(parent_dir));
                        }
                        ExplorerAction::NewFolder => {
                            sender.input(Msg::PromptNewFolder(parent_dir));
                        }
                        ExplorerAction::Delete => {
                            if let Some(path) = selected_path {
                                sender.input(Msg::ConfirmDeletePath(path));
                            }
                        }
                        ExplorerAction::Rename => {
                            if let Some(path) = selected_path {
                                sender.input(Msg::PromptRenameFile(path));
                            }
                        }
                        ExplorerAction::MoveFile => {
                            // Move not yet supported via keyboard in GTK
                            // (uses status-line prompt in TUI)
                        }
                    }
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
                // Toggle between showing the search panel and returning to the editor.
                // When "exiting" we keep the sidebar visible (don't touch sidebar_visible)
                // to avoid a white-area artifact from the Revealer animation — Ctrl+B
                // closes the sidebar entirely.
                // tree_has_focus removed (A.2b-2); engine.explorer_has_focus is authoritative
                if self.active_panel == SidebarPanel::Search && self.sidebar_visible {
                    // Already showing search — return keyboard focus to editor, keep panel open.
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                } else {
                    // Show search panel and return focus to editor (Entry widgets are mouse-driven).
                    self.active_panel = SidebarPanel::Search;
                    self.sidebar_visible = true;
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
            }
            Msg::ExplorerClick { x, y, n_press } => {
                self.handle_explorer_da_click(x, y, n_press, sender);
            }
            Msg::ExplorerRightClick { x, y } => {
                self.handle_explorer_da_right_click(x, y, sender);
            }
            Msg::ExplorerScroll(dy) => {
                let total = self.explorer_state.borrow().rows.len();
                let viewport = self
                    .explorer_sidebar_da_ref
                    .borrow()
                    .as_ref()
                    .map(|da| {
                        let item_h = self.explorer_row_height_cell.get().max(1.0);
                        (da.height() as f64 / item_h).floor().max(0.0) as usize
                    })
                    .unwrap_or(0);
                let max_scroll = total.saturating_sub(viewport);
                // GTK scroll events can arrive with fractional `dy`
                // (trackpads, smooth scrolling) as well as integer steps
                // (mouse wheel notches). We accumulate the scaled delta
                // so small trackpad deltas aren't silently dropped and
                // large wheel notches still move a noticeable amount.
                let scaled = dy * 3.0;
                let accum = self.explorer_scroll_accum.get() + scaled;
                let step = accum.trunc() as isize;
                self.explorer_scroll_accum.set(accum - step as f64);
                if step == 0 {
                    return;
                }
                let mut st = self.explorer_state.borrow_mut();
                let new_top = (st.scroll_top as isize + step).max(0) as usize;
                st.scroll_top = new_top.min(max_scroll);
                drop(st);
                if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
            }
            Msg::PromptRenameFile(path) => {
                let sender_clone = sender.input_sender().clone();
                let initial = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let path_for_close = path.clone();
                self.prompt_for_name(
                    "Rename",
                    &format!("Rename '{}' to:", initial),
                    &initial,
                    Box::new(move |name| {
                        sender_clone
                            .send(Msg::RenameFile(path_for_close.clone(), name))
                            .ok();
                    }),
                );
            }
            Msg::PromptNewFile(parent_dir) => {
                let sender_clone = sender.input_sender().clone();
                self.prompt_for_name(
                    "New File",
                    &format!(
                        "Create file under {}:",
                        parent_dir
                            .file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    ),
                    "",
                    Box::new(move |name| {
                        sender_clone
                            .send(Msg::CreateFile(parent_dir.clone(), name))
                            .ok();
                    }),
                );
            }
            Msg::PromptNewFolder(parent_dir) => {
                let sender_clone = sender.input_sender().clone();
                self.prompt_for_name(
                    "New Folder",
                    &format!(
                        "Create folder under {}:",
                        parent_dir
                            .file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    ),
                    "",
                    Box::new(move |name| {
                        sender_clone
                            .send(Msg::CreateFolder(parent_dir.clone(), name))
                            .ok();
                    }),
                );
            }
            _ => unreachable!(),
        }
    }

    /// Visible-row capacity of the explorer DrawingArea in the current
    /// allocation. Returns 0 when the DA hasn't been measured yet.
    fn explorer_viewport_rows(&self) -> usize {
        self.explorer_sidebar_da_ref
            .borrow()
            .as_ref()
            .map(|da| {
                let item_h = self.explorer_row_height_cell.get().max(1.0);
                (da.height() as f64 / item_h).floor().max(0.0) as usize
            })
            .unwrap_or(0)
    }

    /// Pixel y → flat row index. Returns None if out of bounds or
    /// scroll_offset overshoots the row count.
    fn explorer_row_at(&self, y: f64) -> Option<usize> {
        let state = self.explorer_state.borrow();
        let total = state.rows.len();
        let scroll_top = state.scroll_top;
        drop(state);
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
        // Escape returns focus to editor.
        if key_name == "Escape" {
            sender.input(Msg::FocusEditor);
            return;
        }
        // Panel-nav shortcuts (Ctrl-B / Ctrl-Shift-E / Ctrl-Shift-F).
        // `matches_gtk_key` takes a `gtk4::gdk::Key` — we lost the original
        // here (it was consumed in the controller callback to build the
        // String), so dispatch by name for the common cases.
        let pk_toggle_sidebar = self
            .engine
            .borrow()
            .settings
            .panel_keys
            .toggle_sidebar
            .clone();
        let pk_focus_explorer = self
            .engine
            .borrow()
            .settings
            .panel_keys
            .focus_explorer
            .clone();
        let pk_focus_search = self
            .engine
            .borrow()
            .settings
            .panel_keys
            .focus_search
            .clone();
        // Build a printable form of the current key for the settings
        // string comparison (e.g. "Ctrl-B", "Ctrl-Shift-E"). The settings
        // format is defined in `util::matches_gtk_key`; we approximate
        // here and only match exact strings — good enough for the common
        // defaults.
        let printable = match (ctrl, unicode) {
            (true, Some(c)) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            (false, Some(c)) => c.to_string(),
            _ => key_name.clone(),
        };
        if printable == pk_toggle_sidebar {
            sender.input(Msg::ToggleSidebar);
            return;
        }
        if printable == pk_focus_explorer {
            sender.input(Msg::ToggleFocusExplorer);
            return;
        }
        if printable == pk_focus_search {
            sender.input(Msg::ToggleFocusSearch);
            return;
        }

        // Vim-style single-char navigation keys (j/k/h/l) take priority
        // over the generic explorer_keys shortcuts so the user can always
        // navigate the tree regardless of how explorer_keys is configured.
        if !ctrl {
            if let Some(ch) = unicode {
                match ch {
                    'j' => {
                        self.explorer_move_selection(1);
                        return;
                    }
                    'k' => {
                        self.explorer_move_selection(-1);
                        return;
                    }
                    'l' => {
                        sender.input(Msg::ExplorerActivateSelected);
                        return;
                    }
                    'h' => {
                        self.explorer_collapse_or_parent();
                        return;
                    }
                    _ => {}
                }
            }
        }

        match key_name.as_str() {
            "Return" | "KP_Enter" => {
                sender.input(Msg::ExplorerActivateSelected);
            }
            "Down" => {
                self.explorer_move_selection(1);
            }
            "Up" => {
                self.explorer_move_selection(-1);
            }
            "Page_Down" | "KP_Page_Down" => {
                let v = self.explorer_viewport_rows().max(1) as isize;
                self.explorer_move_selection(v);
            }
            "Page_Up" | "KP_Page_Up" => {
                let v = self.explorer_viewport_rows().max(1) as isize;
                self.explorer_move_selection(-v);
            }
            "Home" => {
                let mut st = self.explorer_state.borrow_mut();
                st.selected = 0;
                st.scroll_top = 0;
                drop(st);
                self.queue_explorer_draw();
            }
            "End" => {
                let mut st = self.explorer_state.borrow_mut();
                if !st.rows.is_empty() {
                    st.selected = st.rows.len() - 1;
                }
                let v = self.explorer_viewport_rows();
                st.ensure_visible(v);
                drop(st);
                self.queue_explorer_draw();
            }
            "Right" => {
                // Right — activate (open or expand).
                sender.input(Msg::ExplorerActivateSelected);
            }
            "Left" => {
                self.explorer_collapse_or_parent();
            }
            _ => {
                // Single-char explorer shortcuts (a/A/D/r/M etc.).
                if !ctrl {
                    if let Some(ch) = unicode {
                        let ch_str = ch.to_string();
                        let is_explorer_key = {
                            let ek = &self.engine.borrow().settings.explorer_keys;
                            ch_str == ek.new_file
                                || ch_str == ek.new_folder
                                || ch_str == ek.delete
                                || ch_str == ek.rename
                                || ch_str == ek.move_file
                        };
                        if is_explorer_key {
                            sender.input(Msg::ExplorerAction(ch_str));
                        }
                    }
                }
            }
        }
    }

    fn explorer_move_selection(&self, delta: isize) {
        let mut st = self.explorer_state.borrow_mut();
        let total = st.rows.len();
        if total == 0 {
            return;
        }
        let new_sel = (st.selected as isize + delta)
            .max(0)
            .min(total as isize - 1) as usize;
        st.selected = new_sel;
        let viewport_rows = self
            .explorer_sidebar_da_ref
            .borrow()
            .as_ref()
            .map(|da| {
                let item_h = self.explorer_row_height_cell.get().max(1.0);
                (da.height() as f64 / item_h).floor().max(0.0) as usize
            })
            .unwrap_or(0);
        st.ensure_visible(viewport_rows);
        drop(st);
        self.queue_explorer_draw();
    }

    fn queue_explorer_draw(&self) {
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.queue_draw();
        }
    }

    /// h / Left behaviour: collapse the current dir if it is expanded,
    /// otherwise move selection to the parent-depth row above.
    fn explorer_collapse_or_parent(&self) {
        let (idx, is_dir, is_expanded, depth) = {
            let st = self.explorer_state.borrow();
            if st.selected >= st.rows.len() {
                return;
            }
            let r = &st.rows[st.selected];
            (st.selected, r.is_dir, r.is_expanded, r.depth)
        };
        if is_dir && is_expanded {
            let root = self.engine.borrow().cwd.clone();
            let show_hidden = self.engine.borrow().settings.show_hidden_files;
            let case_insensitive = self.engine.borrow().settings.explorer_sort_case_insensitive;
            self.explorer_state
                .borrow_mut()
                .toggle_dir(idx, &root, show_hidden, case_insensitive);
            self.queue_explorer_draw();
        } else if depth > 0 {
            let new_selected = {
                let st = self.explorer_state.borrow();
                (0..idx)
                    .rev()
                    .find(|&i| st.rows[i].depth < depth)
                    .unwrap_or(0)
            };
            let mut st = self.explorer_state.borrow_mut();
            st.selected = new_selected;
            let v = self.explorer_viewport_rows();
            st.ensure_visible(v);
            drop(st);
            self.queue_explorer_draw();
        }
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

        // Scrollbar click: jump-scroll so the click y becomes the thumb
        // centre. Return early so the click doesn't also hit-test a row.
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
            let mut st = self.explorer_state.borrow_mut();
            st.selected = idx;
            let row = &st.rows[idx];
            (row.path.clone(), row.is_dir)
        };
        self.queue_explorer_draw();
        if is_dir {
            // Single or double click on a dir toggles expansion — matches
            // typical file-tree UX.
            let root = self.engine.borrow().cwd.clone();
            let show_hidden = self.engine.borrow().settings.show_hidden_files;
            let case_insensitive = self.engine.borrow().settings.explorer_sort_case_insensitive;
            self.explorer_state
                .borrow_mut()
                .toggle_dir(idx, &root, show_hidden, case_insensitive);
            self.queue_explorer_draw();
        } else if n_press >= 2 {
            sender.input(Msg::OpenFileFromSidebar(path));
        } else {
            sender.input(Msg::PreviewFileFromSidebar(path));
        }
    }

    /// Update `scroll_top` so the clicked y lands at the thumb position.
    /// `sb_y` / `sb_h` are the scrollbar track bounds.
    fn explorer_jump_scroll(&self, click_y: f64, sb_y: f64, sb_h: f64) {
        let total = self.explorer_state.borrow().rows.len();
        let viewport = self.explorer_viewport_rows();
        let max_scroll = total.saturating_sub(viewport);
        if max_scroll == 0 || sb_h <= 0.0 {
            return;
        }
        let ratio = ((click_y - sb_y) / sb_h).clamp(0.0, 1.0);
        let new_top = (ratio * max_scroll as f64).round() as usize;
        self.explorer_state.borrow_mut().scroll_top = new_top.min(max_scroll);
    }

    fn handle_explorer_da_right_click(&mut self, x: f64, y: f64, sender: &ComponentSender<Self>) {
        if let Some(ref da) = *self.explorer_sidebar_da_ref.borrow() {
            da.grab_focus();
        }
        self.engine.borrow_mut().explorer_has_focus = true;
        // If click lands on a row, select it before opening the menu.
        // If below the last row, fall back to the workspace root.
        let (target, is_dir) = if let Some(idx) = self.explorer_row_at(y) {
            let mut st = self.explorer_state.borrow_mut();
            st.selected = idx;
            let row = &st.rows[idx];
            (row.path.clone(), row.is_dir)
        } else {
            let root = self.engine.borrow().cwd.clone();
            (root, true)
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

        let parent_dir = if target.is_dir() {
            target.clone()
        } else {
            target
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        };

        let actions = gtk4::gio::SimpleActionGroup::new();
        let add_action = |actions: &gtk4::gio::SimpleActionGroup, a: &gtk4::gio::SimpleAction| {
            if ctx_enabled.get(a.name().as_str()) == Some(&false) {
                a.set_enabled(false);
            }
            actions.add_action(a);
        };

        {
            let s = sender.input_sender().clone();
            let pd = parent_dir.clone();
            let a = gtk4::gio::SimpleAction::new("new_file", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::PromptNewFile(pd.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let pd = parent_dir.clone();
            let a = gtk4::gio::SimpleAction::new("new_folder", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::PromptNewFolder(pd.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("rename", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::PromptRenameFile(t.clone())).ok();
            });
            add_action(&actions, &a);
        }
        {
            let s = sender.input_sender().clone();
            let t = target.clone();
            let a = gtk4::gio::SimpleAction::new("delete", None);
            a.connect_activate(move |_, _| {
                s.send(Msg::ConfirmDeletePath(t.clone())).ok();
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
                // GDK clipboard text arrived for Ctrl-Shift-V paste.
                use core::Mode;
                let mut engine = self.engine.borrow_mut();
                match engine.mode {
                    Mode::Command | Mode::Search => {
                        engine.paste_text_to_input(&text);
                    }
                    Mode::Insert | Mode::Replace => {
                        engine.paste_in_insert_mode(&text);
                    }
                    Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                        if !text.is_empty() {
                            engine.load_clipboard_for_paste(text);
                            engine.handle_key("", Some('p'), false);
                        }
                    }
                }
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
    let lh = line_height.max(1.0);
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        lh
    };
    let per_window_status = engine.settings.window_status_line;
    let global_status_rows = if per_window_status { 1.0 } else { 2.0 };
    let status_bar_height = lh * global_status_rows + wildmenu_px;
    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        6.0 * lh
    } else {
        0.0
    };
    let debug_toolbar_px = if engine.debug_toolbar_visible { lh } else { 0.0 };
    let has_separated =
        per_window_status && !engine.settings.status_line_above_terminal;
    let separated_status_px = if has_separated { lh } else { 0.0 };
    let tab_row_height = (lh * 1.6).ceil();
    let chrome =
        status_bar_height + qf_px + debug_toolbar_px + separated_status_px + tab_row_height;
    let available = (da_height - chrome).max(lh * 7.0);
    let term_rows = (available / lh).floor() as u16;
    term_rows.saturating_sub(2).max(5)
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
    let track_y = rect.y + rect.height - sb_height;
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

/// Hit-test a point against all h scrollbars. Returns `(window_id, px_per_col,
/// scroll_left_at_click)` when the point is on any h scrollbar track (not only
/// the thumb), so the caller can decide between thumb-drag and track-click.
fn h_scrollbar_hit_test(
    engine: &Engine,
    x: f64,
    y: f64,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
) -> Option<(core::WindowId, f64, usize)> {
    for (window_id, rect) in window_rects {
        if let Some((
            track_x,
            track_y,
            track_w,
            sb_height,
            _thumb_x,
            _thumb_w,
            _range,
            px_per_col,
        )) = h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        {
            if x >= track_x && x <= track_x + track_w && y >= track_y && y <= track_y + sb_height {
                let scroll_left = engine
                    .windows
                    .get(window_id)
                    .map(|w| w.view.scroll_left)
                    .unwrap_or(0);
                return Some((*window_id, px_per_col, scroll_left));
            }
        }
    }
    None
}

/// Hit-test tab close buttons. Returns `Some((group_id.0, tab_idx))` if the
/// mouse is over a tab's × button, matching the same geometry as the click handler.
fn tab_close_hit_test(
    engine: &Engine,
    mx: f64,
    my: f64,
    da_w: f64,
    da_h: f64,
    line_height: f64,
    char_width: f64,
) -> Option<(usize, usize)> {
    let tab_row_height = (line_height * 1.6).ceil();
    let tab_bar_height = if engine.settings.breadcrumbs {
        tab_row_height + line_height
    } else {
        tab_row_height
    };
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let status_bar_height = line_height * 2.0 + wildmenu_px;
    let editor_bottom = da_h - status_bar_height;
    let content_bounds = core::WindowRect::new(0.0, 0.0, da_w, editor_bottom);
    let mut group_rects = engine
        .group_layout
        .calculate_group_rects(content_bounds, tab_bar_height);
    engine.adjust_group_rects_for_hidden_tabs(&mut group_rects, tab_bar_height);

    let close_w = char_width;
    let tab_pad = 14.0_f64;
    let tab_inner_gap = 10.0_f64;
    let tab_outer_gap = 1.0_f64;
    let close_pad = char_width;

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
                let name = if let Some(window) = engine.windows.get(&wid) {
                    if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                        let dirty = if state.dirty { "*" } else { "" };
                        format!(" {}: {}{} ", i + 1, state.display_name(), dirty)
                    } else {
                        format!(" {}: [No Name] ", i + 1)
                    }
                } else {
                    format!(" {}: [No Name] ", i + 1)
                };
                let tab_w = name.chars().count() as f64 * char_width;
                let tab_content_w = tab_pad + tab_w + tab_inner_gap + close_w + tab_pad;
                let slot_w = tab_content_w + tab_outer_gap;
                if local_x >= tab_x && local_x < tab_x + slot_w {
                    let close_x_start = tab_x + tab_pad + tab_w + tab_inner_gap - close_pad;
                    let close_x_end = tab_x + tab_content_w;
                    if local_x >= close_x_start && local_x < close_x_end {
                        return Some((gid.0, i));
                    }
                    return None; // In this tab, but not on the close button.
                }
                tab_x += slot_w;
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
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let status_bar_height = line_height * 2.0 + wildmenu_px;
    let editor_bottom = da_h - status_bar_height;
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
        app.activate();
        0
    });
    let app = RelmApp::from_app(gtk_app);
    app.run::<App>(file_path);
}
