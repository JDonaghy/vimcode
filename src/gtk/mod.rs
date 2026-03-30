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
mod tree;
mod util;

use click::*;
use css::*;
use draw::*;
use tree::*;
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

/// Cached dialog button hit rects: Vec<(x, y, w, h)> populated by draw_dialog_popup.
type DialogBtnRects = Vec<(f64, f64, f64, f64)>;

/// Return type of draw_tab_bar: (tab_slot_positions, diff_btn_positions, split_btn_widths, visible_tab_count).
type TabBarDrawResult = (
    Vec<(f64, f64)>,
    Option<(f64, f64, f64, f64, f64, f64)>,
    Option<(f64, f64)>,
    usize,
);

struct App {
    engine: Rc<RefCell<Engine>>,
    /// Set to true in update() whenever a draw is needed; cleared by the #[watch] block.
    /// This prevents the 20/sec SearchPollTick timer from unconditionally calling queue_draw().
    draw_needed: Rc<Cell<bool>>,
    sidebar_visible: bool,
    active_panel: SidebarPanel,
    tree_store: Option<gtk4::TreeStore>,
    tree_has_focus: bool,
    file_tree_view: Rc<RefCell<Option<gtk4::TreeView>>>,
    /// Cell renderer for filenames in the explorer tree (for triggering inline editing).
    name_cell: Rc<RefCell<Option<gtk4::CellRendererText>>>,
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
    /// The scrollable list box inside the Settings panel.
    /// Cleared and rebuilt each time the panel is opened so widgets always
    /// reflect the current engine.settings (e.g. after :set in the editor).
    settings_list_box: Rc<RefCell<Option<gtk4::Box>>>,
    /// Current search-filter sections for the Settings panel, shared with the
    /// SearchEntry callback. Replaced (in place) on each panel rebuild.
    #[allow(clippy::type_complexity)]
    settings_sections: Rc<RefCell<Vec<(gtk4::Label, Vec<(String, gtk4::Box)>)>>>,
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
    #[allow(dead_code)] // Kept alive to continue monitoring settings.json
    settings_monitor: Option<gio::FileMonitor>,
    sender: relm4::Sender<Msg>,
    // Find/Replace dialog state
    find_dialog_visible: bool,
    find_text: String,
    replace_text: String,
    #[allow(dead_code)] // For future case-sensitive search feature
    find_case_sensitive: bool,
    #[allow(dead_code)] // For future whole word search feature
    find_whole_word: bool,
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
    /// Cached nav arrow pixel hit rects from draw_menu_bar: (back_x, back_end, fwd_x, fwd_end, unit_end).
    #[allow(dead_code, clippy::type_complexity)]
    nav_arrow_rects: Rc<RefCell<(f64, f64, f64, f64, f64)>>,
    /// Tab visible counts reported by draw callback, applied to engine in tick handler.
    tab_visible_counts: Rc<RefCell<Vec<(crate::core::window::GroupId, usize)>>>,
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
    /// Toggle find dialog visibility.
    ToggleFindDialog,
    /// Find text input changed.
    FindTextChanged(String),
    /// Replace text input changed.
    ReplaceTextChanged(String),
    /// Find next match.
    FindNext,
    /// Find previous match.
    FindPrevious,
    /// Replace current match and find next.
    ReplaceNext,
    /// Replace all matches.
    ReplaceAll,
    /// Close find dialog.
    CloseFindDialog,
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

                // Activity Bar (48px, always visible)
                #[name = "activity_bar"]
                gtk4::Box {
                    set_orientation: gtk4::Orientation::Vertical,
                    set_width_request: 48,
                    set_css_classes: &["activity-bar"],

                    #[name = "explorer_button"]
                    gtk4::Button {
                        set_label: "\u{f07c}",
                        set_tooltip_text: Some("Explorer (Ctrl+Shift+E)"),
                        set_width_request: 48,
                        set_height_request: 48,

                        #[watch]
                        set_css_classes: if model.active_panel == SidebarPanel::Explorer && model.sidebar_visible {
                            &["activity-button", "active"]
                        } else {
                            &["activity-button"]
                        },

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Explorer));
                        }
                    },

                    #[name = "search_button"]
                    gtk4::Button {
                        set_label: "\u{ea6d}",  // nf-cod-search
                        set_tooltip_text: Some("Search (Ctrl+Shift+F)"),
                        set_width_request: 48,
                        set_height_request: 48,

                        #[watch]
                        set_css_classes: if model.active_panel == SidebarPanel::Search && model.sidebar_visible {
                            &["activity-button", "active"]
                        } else {
                            &["activity-button"]
                        },

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Search));
                        }
                    },

                    #[name = "debug_button"]
                    gtk4::Button {
                        set_label: "\u{f188}",  // nf-fa-bug
                        set_tooltip_text: Some("Debug"),
                        set_width_request: 48,
                        set_height_request: 48,

                        #[watch]
                        set_css_classes: if model.active_panel == SidebarPanel::Debug && model.sidebar_visible {
                            &["activity-button", "active"]
                        } else {
                            &["activity-button"]
                        },

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Debug));
                        }
                    },

                    gtk4::Button {
                        set_label: "\u{e702}",
                        set_tooltip_text: Some("Source Control"),
                        set_width_request: 48,
                        set_height_request: 48,
                        set_css_classes: &["activity-button"],
                        set_sensitive: true,

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Git));
                        }
                    },

                    gtk4::Button {
                        set_label: "\u{eae6}",
                        set_tooltip_text: Some("Extensions"),
                        set_width_request: 48,
                        set_height_request: 48,
                        set_css_classes: &["activity-button"],
                        set_sensitive: true,

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Extensions));
                        }
                    },

                    gtk4::Button {
                        set_label: "\u{f0e5}",
                        set_tooltip_text: Some("AI Assistant"),
                        set_width_request: 48,
                        set_height_request: 48,
                        set_css_classes: &["activity-button"],
                        set_sensitive: true,

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Ai));
                        }
                    },

                    gtk4::Separator {
                        set_vexpand: true, // Pushes settings to bottom
                    },

                    gtk4::Button {
                        set_label: "\u{f013}",
                        set_tooltip_text: Some("Settings"),
                        set_width_request: 48,
                        set_height_request: 48,
                        set_css_classes: &["activity-button"],
                        set_sensitive: true,

                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::SwitchPanel(SidebarPanel::Settings));
                        }
                    },
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

                        // Explorer panel
                        #[name = "explorer_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: model.active_panel == SidebarPanel::Explorer,

                        // Toolbar with file operation buttons
                        #[name = "explorer_toolbar"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Horizontal,
                            set_margin_all: 5,
                            set_spacing: 5,
                            set_css_classes: &["explorer-toolbar"],

                            gtk4::Button {
                                set_label: "\u{f15b}",
                                set_tooltip_text: Some("New File"),
                                set_width_request: 32,
                                set_height_request: 32,
                                connect_clicked[sender, file_tree_view] => move |_| {
                                    let parent_dir = selected_parent_dir(&file_tree_view);
                                    sender.input(Msg::StartInlineNewFile(parent_dir));
                                }
                            },

                            gtk4::Button {
                                set_label: "\u{f07b}",
                                set_tooltip_text: Some("New Folder"),
                                set_width_request: 32,
                                set_height_request: 32,
                                connect_clicked[sender, file_tree_view] => move |_| {
                                    let parent_dir = selected_parent_dir(&file_tree_view);
                                    sender.input(Msg::StartInlineNewFolder(parent_dir));
                                }
                            },

                            gtk4::Button {
                                set_label: "\u{f1f8}",
                                set_tooltip_text: Some("Delete"),
                                set_width_request: 32,
                                set_height_request: 32,
                                connect_clicked[sender, file_tree_view] => move |_| {
                                    // Get selected row
                                    if let Some(selection) = file_tree_view.selection().selected() {
                                        let (model, iter) = selection;
                                        // Column 2 contains the full path
                                        let path_str: String = model.get_value(&iter, 2).get().unwrap_or_default();
                                        if !path_str.is_empty() {
                                            let path = PathBuf::from(path_str);
                                            sender.input(Msg::ConfirmDeletePath(path));
                                        }
                                    }
                                }
                            },

                        },

                        // Scrollable tree view
                        #[name = "file_tree_scroll"]
                        gtk4::ScrolledWindow {
                            set_vexpand: true,
                            set_hscrollbar_policy: gtk4::PolicyType::Automatic,
                            set_vscrollbar_policy: gtk4::PolicyType::Automatic,

                            #[name = "file_tree_view"]
                            gtk4::TreeView {
                                set_headers_visible: false,
                                set_enable_tree_lines: false,
                                set_show_expanders: true,
                                set_level_indentation: 0,
                                set_focusable: true,
                                set_enable_search: false,

                                add_controller = gtk4::EventControllerKey {
                                    connect_key_pressed[sender, engine] => move |_, key, _, modifier| {
                                        let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();

                                        // Escape returns focus to editor
                                        if key_name == "Escape" {
                                            sender.input(Msg::FocusEditor);
                                            return gtk4::glib::Propagation::Stop;
                                        }

                                        // Panel navigation — same shortcuts work from within tree view
                                        let pk = engine.borrow().settings.panel_keys.clone();
                                        if matches_gtk_key(&pk.toggle_sidebar, key, modifier) {
                                            sender.input(Msg::ToggleSidebar);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if matches_gtk_key(&pk.focus_explorer, key, modifier) {
                                            sender.input(Msg::ToggleFocusExplorer);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        if matches_gtk_key(&pk.focus_search, key, modifier) {
                                            sender.input(Msg::ToggleFocusSearch);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Arrow keys + Enter: let TreeView handle natively
                                        // (Enter fires row_activated which handles dirs+files)
                                        if matches!(key_name.as_str(), "Up" | "Down" | "Left" | "Right" | "Return" | "KP_Enter" | "space") {
                                            return gtk4::glib::Propagation::Proceed;
                                        }

                                        // Explorer CRUD keys (a/A/D/r/M etc.)
                                        if !modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                                            if let Some(ch) = key.to_unicode() {
                                                let ch_str = ch.to_string();
                                                // Resolve action with a short-lived borrow
                                                let is_explorer_key = {
                                                    let ek = &engine.borrow().settings.explorer_keys;
                                                    ch_str == ek.new_file || ch_str == ek.new_folder
                                                        || ch_str == ek.delete || ch_str == ek.rename
                                                        || ch_str == ek.move_file
                                                };
                                                if is_explorer_key {
                                                    // Defer via idle to avoid any borrow conflicts
                                                    let s = sender.clone();
                                                    gtk4::glib::idle_add_local_once(move || {
                                                        s.input(Msg::ExplorerAction(ch_str));
                                                    });
                                                    return gtk4::glib::Propagation::Stop;
                                                }
                                            }
                                        }

                                        // Stop all other keys from triggering TreeView search
                                        gtk4::glib::Propagation::Stop
                                    }
                                },
                            },
                        },
                        },

                        // Settings panel — visibility managed imperatively via settings_panel_box
                        #[name = "settings_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],
                            set_visible: false,  // hidden initially; toggled via settings_panel_box
                            // Content built imperatively in init() after view_output!()
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
                                        // Escape: close the find dialog and return focus to editor.
                                        if key_name == "Escape" {
                                            sender.input(Msg::CloseFindDialog);
                                            sender.input(Msg::Resize);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Ctrl-F: toggle find dialog.
                                        if ctrl && !shift && unicode == Some('f') {
                                            sender.input(Msg::ToggleFindDialog);
                                            return gtk4::glib::Propagation::Stop;
                                        }
                                        // Let all other keys reach the Entry widget.
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

                                    // Ctrl-F: terminal find when terminal focused, else editor find dialog
                                    if ctrl && !shift && unicode == Some('f') {
                                        if engine.borrow().terminal_has_focus {
                                            if engine.borrow().terminal_find_active {
                                                sender.input(Msg::TerminalFindClose);
                                            } else {
                                                sender.input(Msg::TerminalFindOpen);
                                            }
                                        } else {
                                            sender.input(Msg::ToggleFindDialog);
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
                                        // Ctrl+Y: copy terminal selection to clipboard.
                                        if ctrl && !shift && (key_name == "y" || key_name == "Y") {
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

                        // Find/Replace Dialog (overlay at top-right)
                        add_overlay = &gtk4::Revealer {
                            set_transition_type: gtk4::RevealerTransitionType::SlideDown,
                            set_transition_duration: 200,
                            set_halign: gtk4::Align::End,
                            set_valign: gtk4::Align::Start,
                            set_margin_top: 10,
                            set_margin_end: 10,

                            #[watch]
                            set_reveal_child: model.find_dialog_visible,

                            gtk4::Box {
                                set_orientation: gtk4::Orientation::Vertical,
                                set_spacing: 8,
                                set_css_classes: &["find-dialog"],
                                set_width_request: 400,

                                // Find input row
                                gtk4::Box {
                                    set_orientation: gtk4::Orientation::Horizontal,
                                    set_spacing: 4,

                                    gtk4::Label {
                                        set_text: "Find:",
                                        set_width_request: 60,
                                    },

                                    #[name = "find_entry"]
                                    gtk4::Entry {
                                        set_placeholder_text: Some("Find in buffer"),
                                        set_hexpand: true,

                                        connect_changed[sender] => move |entry| {
                                            let text = entry.text().to_string();
                                            sender.input(Msg::FindTextChanged(text));
                                        },

                                        connect_activate[sender] => move |_| {
                                            sender.input(Msg::FindNext);
                                        },
                                    },

                                    gtk4::Button {
                                        set_label: "↑",
                                        set_tooltip_text: Some("Previous (Shift+Enter)"),
                                        connect_clicked[sender] => move |_| {
                                            sender.input(Msg::FindPrevious);
                                        },
                                    },

                                    gtk4::Button {
                                        set_label: "↓",
                                        set_tooltip_text: Some("Next (Enter)"),
                                        connect_clicked[sender] => move |_| {
                                            sender.input(Msg::FindNext);
                                        },
                                    },

                                    gtk4::Button {
                                        set_label: "×",
                                        set_tooltip_text: Some("Close (Escape)"),
                                        connect_clicked[sender] => move |_| {
                                            sender.input(Msg::CloseFindDialog);
                                        },
                                    },
                                },

                                // Replace input row
                                gtk4::Box {
                                    set_orientation: gtk4::Orientation::Horizontal,
                                    set_spacing: 4,

                                    gtk4::Label {
                                        set_text: "Replace:",
                                        set_width_request: 60,
                                    },

                                    #[name = "replace_entry"]
                                    gtk4::Entry {
                                        set_placeholder_text: Some("Replace with"),
                                        set_hexpand: true,

                                        connect_changed[sender] => move |entry| {
                                            let text = entry.text().to_string();
                                            sender.input(Msg::ReplaceTextChanged(text));
                                        },
                                    },

                                    gtk4::Button {
                                        set_label: "Replace",
                                        connect_clicked[sender] => move |_| {
                                            sender.input(Msg::ReplaceNext);
                                        },
                                    },

                                    gtk4::Button {
                                        set_label: "Replace All",
                                        connect_clicked[sender] => move |_| {
                                            sender.input(Msg::ReplaceAll);
                                        },
                                    },
                                },

                                // Match count label
                                #[name = "match_count_label"]
                                gtk4::Label {
                                    set_text: "No matches",
                                    set_halign: gtk4::Align::Start,
                                    set_css_classes: &["find-match-count"],
                                },
                            }
                        }
                    }
                }
                }  // close main_hbox
            }  // close outer gtk4::Box
            }  // close window_overlay
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

        let engine = {
            let mut e = Engine::new();
            e.plugin_init();
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

        // Create TreeStore with 6 columns: Icon, Name, FullPath, FgColor, Indicator, IndicatorColor
        let tree_store = gtk4::TreeStore::new(&[
            gtk4::glib::Type::STRING, // 0: Icon
            gtk4::glib::Type::STRING, // 1: Name
            gtk4::glib::Type::STRING, // 2: Full path
            gtk4::glib::Type::STRING, // 3: Foreground color (hex)
            gtk4::glib::Type::STRING, // 4: Indicator text (e.g. "M", "2⚠", "1✗")
            gtk4::glib::Type::STRING, // 5: Indicator foreground color (hex)
        ]);

        let file_tree_view_ref = Rc::new(RefCell::new(None));
        let name_cell_ref: Rc<RefCell<Option<gtk4::CellRendererText>>> =
            Rc::new(RefCell::new(None));
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
        let tab_visible_counts_cell: Rc<RefCell<Vec<(crate::core::window::GroupId, usize)>>> =
            Rc::new(RefCell::new(Vec::new()));
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
            tree_store: Some(tree_store.clone()),
            tree_has_focus: false,
            file_tree_view: file_tree_view_ref.clone(),
            name_cell: name_cell_ref.clone(),
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
            settings_monitor,
            sender: sender.input_sender().clone(),
            find_dialog_visible: false,
            find_text: String::new(),
            replace_text: String::new(),
            find_case_sensitive: false,
            find_whole_word: false,
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
            settings_list_box: Rc::new(RefCell::new(None)),
            settings_sections: Rc::new(RefCell::new(Vec::new())),
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
            nav_arrow_rects: nav_arrow_rects_cell.clone(),
            tab_visible_counts: tab_visible_counts_cell.clone(),
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
        *file_tree_view_ref.borrow_mut() = Some(widgets.file_tree_view.clone());
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

        // ── Settings sidebar form (built imperatively) ─────────────────────────
        {
            let panel = widgets.settings_panel.clone();
            let engine_b = engine.borrow();

            // Header row
            let header_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            header_row.set_css_classes(&["sidebar-header"]);
            let title_lbl = gtk4::Label::new(Some("  SETTINGS"));
            title_lbl.set_css_classes(&["sidebar-title"]);
            title_lbl.set_halign(gtk4::Align::Start);
            title_lbl.set_hexpand(true);
            header_row.append(&title_lbl);
            panel.append(&header_row);

            // Search entry
            let search_entry = gtk4::SearchEntry::new();
            search_entry.set_placeholder_text(Some("Search settings..."));
            search_entry.set_margin_start(8);
            search_entry.set_margin_end(8);
            search_entry.set_margin_top(4);
            search_entry.set_margin_bottom(4);
            search_entry.set_hexpand(true);
            search_entry.set_width_chars(1);
            panel.append(&search_entry);

            // Scrolled list of settings rows
            let scroll = gtk4::ScrolledWindow::new();
            scroll.set_vexpand(true);
            scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
            scroll.set_overlay_scrolling(false);

            let list_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            list_box.set_margin_bottom(8);

            let sender_s = sender.input_sender().clone();
            let sections = build_settings_form(&list_box, &engine_b.settings, &sender_s);
            drop(engine_b);

            // Store refs so the settings panel can be rebuilt when reopened.
            *model.settings_list_box.borrow_mut() = Some(list_box.clone());
            *model.settings_sections.borrow_mut() = sections;

            // Wire up search filtering: show/hide rows + category headers.
            // Capture the shared Rc so the callback still works after a rebuild.
            let sections_rc = model.settings_sections.clone();
            search_entry.connect_search_changed(move |entry| {
                let query = entry.text().to_string().to_lowercase();
                for (header, rows) in sections_rc.borrow().iter() {
                    let mut any_visible = false;
                    for (search_text, row) in rows {
                        let visible = query.is_empty() || search_text.contains(&query);
                        row.set_visible(visible);
                        if visible {
                            any_visible = true;
                        }
                    }
                    header.set_visible(any_visible);
                }
            });

            scroll.set_child(Some(&list_box));
            panel.append(&scroll);

            // Bottom: quick access to settings.json
            let open_btn = gtk4::Button::with_label("Open settings.json");
            open_btn.set_margin_start(8);
            open_btn.set_margin_end(8);
            open_btn.set_margin_top(4);
            open_btn.set_margin_bottom(8);
            let s_open = sender.input_sender().clone();
            open_btn.connect_clicked(move |_| {
                s_open.send(Msg::OpenSettingsFile).ok();
            });
            panel.append(&open_btn);
        }

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
                // The handle strip sits right after the sidebar.
                // activity bar (48px) + sidebar_width = left edge of handle.
                const ACTIVITY_W: f64 = 48.0;
                let aw = sb.allocated_width();
                let sidebar_right = ACTIVITY_W + aw as f64;
                // Accept clicks within ±10 px of the handle centre.
                if (x - (sidebar_right + 3.0)).abs() <= 10.0 {
                    is_sb_drag_begin.set(true);
                    // Use width_request (what we control) not allocated_width
                    // (which may be larger due to GTK layout).
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
                        let popup_h = (items.len() as f64 + 1.0) * line_height;
                        if y >= popup_y
                            && y < popup_y + popup_h
                            && x >= popup_x
                            && x < popup_x + popup_w
                        {
                            let raw_idx = ((y - popup_y) / line_height) as usize;
                            let item_real = raw_idx.saturating_sub(1);
                            if raw_idx >= 1
                                && item_real < items.len()
                                && !items[item_real].separator
                            {
                                let action = items[item_real].action.to_string();
                                drop(engine);
                                sender_dd
                                    .send(Msg::MenuActivateItem(open_idx, item_real, action))
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
                        let popup_h = (items.len() as f64 + 1.0) * line_height;
                        if y >= popup_y
                            && y < popup_y + popup_h
                            && x >= popup_x
                            && x < popup_x + popup_w
                        {
                            let raw_idx = ((y - popup_y) / line_height) as usize;
                            let item_real = raw_idx.saturating_sub(1);
                            if raw_idx >= 1
                                && item_real < items.len()
                                && !items[item_real].separator
                            {
                                if engine.menu_highlighted_item != Some(item_real) {
                                    drop(engine);
                                    sender_motion.send(Msg::MenuHighlight(Some(item_real))).ok();
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

        // ── Dynamic activity bar buttons for extension-provided panels ────────
        {
            let eng = engine.borrow();
            let mut panels: Vec<_> = eng.ext_panels.values().collect();
            panels.sort_by(|a, b| a.name.cmp(&b.name));
            // Find the Separator widget in the activity bar (push-to-bottom spacer).
            // Insert ext panel buttons just before it.
            let activity_bar = &widgets.activity_bar;
            let mut separator_widget: Option<gtk4::Widget> = None;
            let mut child = activity_bar.first_child();
            while let Some(ref w) = child {
                if w.downcast_ref::<gtk4::Separator>().is_some() {
                    separator_widget = Some(w.clone());
                    break;
                }
                child = w.next_sibling();
            }
            // Track the last button we inserted so the next one goes after it.
            let mut insert_after: Option<gtk4::Widget> =
                separator_widget.as_ref().and_then(|s| s.prev_sibling());
            for panel in &panels {
                let btn = gtk4::Button::new();
                btn.set_label(&panel.icon.to_string());
                btn.set_tooltip_text(Some(&panel.title));
                btn.set_width_request(48);
                btn.set_height_request(48);
                btn.set_css_classes(&["activity-button"]);
                let panel_name = panel.name.clone();
                let sender_btn = sender.input_sender().clone();
                btn.connect_clicked(move |_| {
                    sender_btn
                        .send(Msg::SwitchPanel(SidebarPanel::ExtPanel(panel_name.clone())))
                        .ok();
                });
                activity_bar.insert_child_after(&btn, insert_after.as_ref());
                insert_after = Some(btn.upcast());
            }
        }

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

        // Build tree from current working directory
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (dir_fg_hex, file_fg_hex) = {
            let theme = Theme::from_name(&engine.borrow().settings.colorscheme);
            let file_fg = if theme.is_light() {
                theme.foreground.to_hex()
            } else {
                theme.status_fg.to_hex()
            };
            (theme.explorer_dir_fg.to_hex(), file_fg)
        };
        build_file_tree_with_root(
            &tree_store,
            &cwd,
            engine.borrow().settings.show_hidden_files,
            &dir_fg_hex,
            &file_fg_hex,
        );

        // Read font family for nerd font icon rendering
        let nf_font = engine.borrow().settings.font_family.clone();

        // Setup TreeView columns
        // Single column with icon + filename (so they indent together)
        let col = gtk4::TreeViewColumn::new();

        // Icon cell renderer (non-expanding) — must use the nerd font for glyph support
        let icon_cell = gtk4::CellRendererText::new();
        icon_cell.set_property("font", &nf_font);
        col.pack_start(&icon_cell, false);
        col.add_attribute(&icon_cell, "text", 0);
        col.add_attribute(&icon_cell, "foreground", 3);

        // Filename cell renderer (expanding) — made editable on demand for inline rename
        let name_cell = gtk4::CellRendererText::new();
        col.pack_start(&name_cell, true);
        col.add_attribute(&name_cell, "text", 1);
        col.add_attribute(&name_cell, "foreground", 3);

        // Store name_cell for later use by StartInlineNewFile/Folder handlers
        *name_cell_ref.borrow_mut() = Some(name_cell.clone());

        // Handle inline cell editing for rename and new file/folder creation
        {
            let sender_for_edit = sender.clone();
            let ts_for_edit = tree_store.clone();
            let name_cell_for_cancel = name_cell.clone();
            let ts_for_cancel = tree_store.clone();
            let sender_for_cancel = sender.clone();
            name_cell.connect_edited(move |cell, tree_path, new_text| {
                // Disable editable after edit completes
                cell.set_property("editable", false);
                let new_name = new_text.trim();
                // Get the path/marker from the TreeStore (column 2)
                if let Some(iter) = ts_for_edit.iter(&tree_path) {
                    let path_str: String =
                        ts_for_edit.get_value(&iter, 2).get().unwrap_or_default();
                    if let Some(parent_dir) = path_str.strip_prefix("__NEW_FILE__") {
                        // New file creation — remove temporary row
                        ts_for_edit.remove(&iter);
                        if !new_name.is_empty() {
                            sender_for_edit.input(Msg::CreateFile(
                                PathBuf::from(parent_dir),
                                new_name.to_string(),
                            ));
                        }
                    } else if let Some(parent_dir) = path_str.strip_prefix("__NEW_FOLDER__") {
                        // New folder creation — remove temporary row
                        ts_for_edit.remove(&iter);
                        if !new_name.is_empty() {
                            sender_for_edit.input(Msg::CreateFolder(
                                PathBuf::from(parent_dir),
                                new_name.to_string(),
                            ));
                        }
                    } else if !new_name.is_empty() && !path_str.is_empty() {
                        // Regular rename
                        let old_path = PathBuf::from(&path_str);
                        sender_for_edit.input(Msg::RenameFile(old_path, new_name.to_string()));
                    }
                }
            });
            name_cell.connect_editing_canceled(move |_cell| {
                // Disable editable when editing is cancelled
                name_cell_for_cancel.set_property("editable", false);
                // Remove any temporary new-entry rows
                if let Some(iter) = ts_for_cancel.iter_first() {
                    remove_new_entry_rows(&ts_for_cancel, &iter);
                }
                sender_for_cancel.input(Msg::RefreshFileTree);
            });
        }

        // Indicator cell renderer (right-aligned, non-expanding) for M/error/warning badges
        let indicator_cell = gtk4::CellRendererText::new();
        indicator_cell.set_property("xalign", 1.0f32);
        col.pack_end(&indicator_cell, false);
        col.add_attribute(&indicator_cell, "text", 4);
        col.add_attribute(&indicator_cell, "foreground", 5);

        widgets.file_tree_view.append_column(&col);

        // Set the model on the TreeView
        widgets.file_tree_view.set_model(Some(&tree_store));

        // Lazy-load: populate directory children when the user expands a row.
        {
            let engine_ref = engine.clone();
            let tree_store_ref = tree_store.clone();
            widgets
                .file_tree_view
                .connect_row_expanded(move |_tree_view, iter, _tree_path| {
                    let e = engine_ref.borrow();
                    let show_hidden = e.settings.show_hidden_files;
                    let theme = Theme::from_name(&e.settings.colorscheme);
                    let dir_fg_hex = theme.explorer_dir_fg.to_hex();
                    let file_fg_hex = if theme.is_light() {
                        theme.foreground.to_hex()
                    } else {
                        theme.status_fg.to_hex()
                    };
                    drop(e);
                    tree_row_expanded(
                        &tree_store_ref,
                        iter,
                        show_hidden,
                        &dir_fg_hex,
                        &file_fg_hex,
                    );
                });
        }

        // Expand the root node so the tree contents are visible
        widgets
            .file_tree_view
            .expand_row(&gtk4::TreePath::from_indices(&[0]), false);

        // Highlight the active file from the restored session in the tree.
        if let Some(path) = engine.borrow().file_path().cloned() {
            highlight_file_in_tree(&widgets.file_tree_view, &path);
        }

        // Connect double-click signal to open files
        let sender_for_tree = sender.clone();
        widgets
            .file_tree_view
            .connect_row_activated(move |tree_view, tree_path, _column| {
                if let Some(model) = tree_view.model() {
                    if let Some(iter) = model.iter(tree_path) {
                        // Use TreeModelExt::get to retrieve the value
                        let full_path: String = model.get_value(&iter, 2).get().unwrap_or_default();

                        let path_buf = PathBuf::from(full_path);
                        if path_buf.is_file() {
                            sender_for_tree.input(Msg::OpenFileFromSidebar(path_buf));
                        } else if path_buf.is_dir() {
                            if tree_view.row_expanded(tree_path) {
                                tree_view.collapse_row(tree_path);
                            } else {
                                tree_view.expand_row(tree_path, false);
                            }
                        }
                    }
                }
            });

        // Connect single-click for preview mode
        let sender_for_click = sender.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(1); // Left mouse button
        gesture.connect_released(move |gesture, n_press, x, y| {
            if n_press != 1 {
                return; // Double-click handled by row_activated
            }
            let widget = gesture.widget();
            if let Some(tree_view) = widget.downcast_ref::<gtk4::TreeView>() {
                if let Some((Some(path), _, _, _)) = tree_view.path_at_pos(x as i32, y as i32) {
                    if let Some(model) = tree_view.model() {
                        if let Some(iter) = model.iter(&path) {
                            let full_path: String =
                                model.get_value(&iter, 2).get().unwrap_or_default();
                            let path_buf = PathBuf::from(full_path);
                            if path_buf.is_file() {
                                sender_for_click.input(Msg::PreviewFileFromSidebar(path_buf));
                            } else {
                                // Clicking a directory keeps explorer focus so
                                // keyboard shortcuts (a/A/D/r) work immediately.
                                sender_for_click.input(Msg::FocusExplorer);
                            }
                        }
                    }
                }
            }
        });
        widgets.file_tree_view.add_controller(gesture);

        // Right-click context menu — plain Popover with flat buttons (no
        // PopoverMenu, avoids GTK4 internal ScrolledWindow sizing issues).
        {
            let sender_rc = sender.clone();
            let ctx_pop_rc = active_ctx_popover_ref.clone();
            let engine_ctx = engine.clone();
            let name_cell_rc = name_cell.clone();
            let right_click = gtk4::GestureClick::new();
            right_click.set_button(3);
            right_click.connect_pressed(move |gesture, _n_press, x, y| {
                let widget = gesture.widget();
                let Some(tree_view) = widget.downcast_ref::<gtk4::TreeView>() else {
                    return;
                };
                // Select the row under cursor
                if let Some((Some(tp), _, _, _)) = tree_view.path_at_pos(x as i32, y as i32) {
                    tree_view.selection().select_path(&tp);
                }
                let selected_path: Option<PathBuf> =
                    tree_view.selection().selected().and_then(|(model, iter)| {
                        let s: String = model.get_value(&iter, 2).get().ok()?;
                        if s.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(s))
                        }
                    });
                let Some(target) = selected_path else { return };
                let is_dir = target.is_dir();

                // Build gio::Menu from engine-generated items (single source of truth).
                engine_ctx
                    .borrow_mut()
                    .open_explorer_context_menu(target.clone(), is_dir, 0, 0);
                let items: Vec<core::engine::ContextMenuItem> = engine_ctx
                    .borrow()
                    .context_menu
                    .as_ref()
                    .map(|cm| cm.items.clone())
                    .unwrap_or_default();
                engine_ctx.borrow_mut().close_context_menu();
                let menu = build_gio_menu_from_engine_items(&items, "ctx");

                // Collect enabled state from engine items keyed by action string.
                let ctx_enabled: std::collections::HashMap<String, bool> = items
                    .iter()
                    .map(|it| (it.action.clone(), it.enabled))
                    .collect();

                // Build action group. `add_ctx_action` respects engine-driven enabled state.
                let actions = gtk4::gio::SimpleActionGroup::new();
                // Helper: register action and apply engine-driven enabled state.
                let add_action = |actions: &gtk4::gio::SimpleActionGroup,
                                  a: &gtk4::gio::SimpleAction| {
                    if ctx_enabled.get(a.name().as_str()) == Some(&false) {
                        a.set_enabled(false);
                    }
                    actions.add_action(a);
                };
                let parent_dir = if target.is_dir() {
                    target.clone()
                } else {
                    target
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                };

                {
                    let s = sender_rc.clone();
                    let pd = parent_dir.clone();
                    let a = gtk4::gio::SimpleAction::new("new_file", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::StartInlineNewFile(pd.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let pd = parent_dir.clone();
                    let a = gtk4::gio::SimpleAction::new("new_folder", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::StartInlineNewFolder(pd.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let tv = tree_view.clone();
                    let nc = name_cell_rc.clone();
                    let pop_ref = ctx_pop_rc.clone();
                    let a = gtk4::gio::SimpleAction::new("rename", None);
                    a.connect_activate(move |_, _| {
                        // Close the context menu popover first so it doesn't
                        // steal focus from the inline editor.
                        if let Some(ref p) = *pop_ref.borrow() {
                            p.popdown();
                        }
                        // Start inline cell editing on idle so the popover
                        // has time to close and release focus.
                        let tv2 = tv.clone();
                        let nc2 = nc.clone();
                        gtk4::glib::idle_add_local_once(move || {
                            nc2.set_property("editable", true);
                            if let Some(column) = tv2.column(0) {
                                if let Some((model, iter)) = tv2.selection().selected() {
                                    let tree_path = model.path(&iter);
                                    gtk4::prelude::TreeViewExt::set_cursor(
                                        &tv2,
                                        &tree_path,
                                        Some(&column),
                                        true,
                                    );
                                }
                            }
                        });
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("delete", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::ConfirmDeletePath(tgt.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("copy_path", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::CopyPath(tgt.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("copy_relative_path", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::CopyRelativePath(tgt.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("reveal", None);
                    a.connect_activate(move |_, _| {
                        let dir = if tgt.is_dir() {
                            tgt.clone()
                        } else {
                            tgt.parent()
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
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("select_for_diff", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::SelectForDiff(tgt.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("diff_with_selected", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::DiffWithSelected(tgt.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("open_side", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::OpenSide(tgt.clone()));
                    });
                    add_action(&actions, &a);
                }
                {
                    let eng = engine_ctx.clone();
                    let tgt = target.clone();
                    let a = gtk4::gio::SimpleAction::new("open_side_vsplit", None);
                    a.connect_activate(move |_, _| {
                        let mut e = eng.borrow_mut();
                        e.split_window(crate::core::window::SplitDirection::Vertical, None);
                        let _ = e.open_file_with_mode(&tgt, crate::core::OpenMode::Permanent);
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let tgt = target.clone();
                    let tgt_is_dir = is_dir;
                    let a = gtk4::gio::SimpleAction::new("open_terminal", None);
                    a.connect_activate(move |_, _| {
                        let dir = if tgt_is_dir {
                            tgt.clone()
                        } else {
                            tgt.parent()
                                .unwrap_or(std::path::Path::new("."))
                                .to_path_buf()
                        };
                        s.input(Msg::OpenTerminalAt(dir));
                    });
                    add_action(&actions, &a);
                }
                {
                    let s = sender_rc.clone();
                    let a = gtk4::gio::SimpleAction::new("find_in_folder", None);
                    a.connect_activate(move |_, _| {
                        s.input(Msg::ToggleFocusSearch);
                    });
                    add_action(&actions, &a);
                }

                // Clean up previous popover, create new PopoverMenu.
                let n_rows = menu_row_count(&menu);
                // Parent to the ScrolledWindow (tree_view's parent) so that
                // hover events work correctly — PopoverMenu inside a
                // ScrolledWindow's child doesn't receive motion events properly.
                let popover_parent: gtk4::Widget = tree_view
                    .parent()
                    .unwrap_or_else(|| tree_view.clone().upcast());
                let (px, py) = tree_view
                    .translate_coordinates(&popover_parent, x, y)
                    .unwrap_or((x, y));
                popover_parent.insert_action_group("ctx", Some(&actions));
                swap_ctx_popover(&ctx_pop_rc, {
                    let popover = gtk4::PopoverMenu::from_model(Some(&menu));
                    popover.set_parent(&popover_parent);
                    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                        px as i32, py as i32, 1, 1,
                    )));
                    popover.set_has_arrow(false);
                    popover.set_position(gtk4::PositionType::Right);
                    popover.set_size_request(-1, n_rows * 22 + 14);
                    popover
                });
                if let Some(ref p) = *ctx_pop_rc.borrow() {
                    p.popup();
                }
            });
            widgets.file_tree_view.add_controller(right_click);
        }

        // Drag-and-drop: DragSource (initiator) + DropTarget (receiver)
        //
        // We store the dragged path in a shared Rc so the drop handler can
        // read it directly — avoids relying on GValue content negotiation
        // which can crash on some GTK4 builds when types don't match.
        {
            let drag_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

            // DragSource
            let drag_source = gtk4::DragSource::new();
            drag_source.set_actions(gtk4::gdk::DragAction::MOVE);
            let drag_path_src = drag_path.clone();
            drag_source.connect_prepare(move |ds, x, y| {
                let widget = ds.widget();
                let tree_view = widget.downcast_ref::<gtk4::TreeView>()?;
                let file_path: Option<PathBuf> = (|| {
                    let (tp, _, _, _) = tree_view.path_at_pos(x as i32, y as i32)?;
                    let model = tree_view.model()?;
                    let iter = model.iter(&tp?)?;
                    let s: String = model.get_value(&iter, 2).get().ok()?;
                    if s.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(s))
                    }
                })();
                *drag_path_src.borrow_mut() = file_path.clone();
                // Provide a dummy string content so GTK accepts the drag gesture.
                file_path.map(|p| {
                    let path_str: String = p.to_string_lossy().into_owned();
                    gtk4::gdk::ContentProvider::for_value(&path_str.to_value())
                })
            });
            let drag_path_icon = drag_path.clone();
            drag_source.connect_drag_begin(move |_source, drag| {
                // Set a custom drag icon — prevents GTK from snapshotting the
                // TreeView row, which can cause a core dump on GTK4 >= 4.10.
                let icon_widget = gtk4::DragIcon::for_drag(drag);
                if let Some(drag_icon) = icon_widget.downcast_ref::<gtk4::DragIcon>() {
                    let name = drag_path_icon
                        .borrow()
                        .as_ref()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_else(|| "File".to_string());
                    let label = gtk4::Label::new(Some(&name));
                    drag_icon.set_child(Some(&label));
                }
            });
            let drag_path_end = drag_path.clone();
            drag_source.connect_drag_end(move |_source, _drag, _delete_data| {
                // Clear leftover drag state (covers cancelled/failed drags).
                *drag_path_end.borrow_mut() = None;
            });
            widgets.file_tree_view.add_controller(drag_source);

            // DropTarget
            let sender_drop = sender.clone();
            let drag_path_drop = drag_path.clone();
            let drop_target =
                gtk4::DropTarget::new(gtk4::glib::Type::STRING, gtk4::gdk::DragAction::MOVE);
            drop_target.connect_drop(move |dt, _value, x, y| {
                // Read source from the shared Rc (set by connect_prepare).
                let src = drag_path_drop.borrow_mut().take();
                let Some(src) = src else {
                    return false;
                };
                let widget = dt.widget();
                let Some(tree_view) = widget.downcast_ref::<gtk4::TreeView>() else {
                    return false;
                };
                // Find destination directory at drop position.
                let dest_dir: Option<PathBuf> = (|| {
                    let (tp, _, _, _) = tree_view.path_at_pos(x as i32, y as i32)?;
                    let model = tree_view.model()?;
                    let iter = model.iter(&tp?)?;
                    let s: String = model.get_value(&iter, 2).get().ok()?;
                    if s.is_empty() {
                        return None;
                    }
                    let p = PathBuf::from(s);
                    Some(if p.is_dir() {
                        p
                    } else {
                        p.parent()
                            .unwrap_or(std::path::Path::new("."))
                            .to_path_buf()
                    })
                })();
                let Some(dest_dir) = dest_dir else {
                    return false;
                };
                // Don't move to the same directory
                if src.parent() == Some(dest_dir.as_path()) {
                    return false;
                }
                sender_drop.input(Msg::MoveFile(src, dest_dir));
                true
            });
            widgets.file_tree_view.add_controller(drop_target);
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
                let wildmenu_px = if engine.wildmenu_items.is_empty() {
                    0.0
                } else {
                    lh
                };
                let status_h = lh * 2.0 + wildmenu_px;
                let dbg_px = if engine.debug_toolbar_visible {
                    lh
                } else {
                    0.0
                };
                let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
                    6.0 * lh
                } else {
                    0.0
                };
                let term_px = if engine.terminal_open || engine.bottom_panel_open {
                    (engine.session.terminal_panel_rows as f64 + 2.0) * lh
                } else {
                    0.0
                };
                let editor_bottom = height - status_h - dbg_px - qf_px - term_px;
                let content_bounds = core::window::WindowRect::new(0.0, 0.0, width, editor_bottom);
                let dividers = engine.group_layout.dividers(content_bounds, &mut 0);
                // Check if click is in a scrollbar zone (rightmost 10px of any
                // window rect). If so, skip divider claim to let the scrollbar
                // handle the click instead.
                let tab_bar_h = if engine.settings.breadcrumbs {
                    lh * 2.0
                } else {
                    lh
                };
                let (window_rects, _) =
                    engine.calculate_group_window_rects(content_bounds, tab_bar_h);
                let in_scrollbar = window_rects.iter().any(|(_, r)| {
                    let sb_zone = 10.0; // scrollbar width + margin
                    x >= r.x + r.width - sb_zone
                        && x <= r.x + r.width
                        && y >= r.y
                        && y < r.y + r.height
                });
                if !in_scrollbar {
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
                    let wildmenu_px = if engine.wildmenu_items.is_empty() {
                        0.0
                    } else {
                        lh
                    };
                    let status_h = lh * 2.0 + wildmenu_px;
                    let dbg_px = if engine.debug_toolbar_visible {
                        lh
                    } else {
                        0.0
                    };
                    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
                        6.0 * lh
                    } else {
                        0.0
                    };
                    let term_px = if engine.terminal_open || engine.bottom_panel_open {
                        (engine.session.terminal_panel_rows as f64 + 2.0) * lh
                    } else {
                        0.0
                    };
                    let editor_bottom = height - status_h - dbg_px - qf_px - term_px;
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
        let dialog_btn_for_draw = model.dialog_btn_rects.clone();
        let editor_hover_rect_for_draw = model.editor_hover_popup_rect.clone();
        let editor_hover_links_for_draw = model.editor_hover_link_rects.clone();
        let mouse_pos_for_draw = mouse_pos_cell.clone();
        let tab_vis_for_draw = tab_visible_counts_cell.clone();
        widgets
            .drawing_area
            .set_draw_func(move |_, cr, width, height| {
                // Wrap in catch_unwind to prevent GTK abort on panic in extern "C" callback.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                        &dialog_btn_for_draw,
                        &editor_hover_rect_for_draw,
                        &editor_hover_links_for_draw,
                        mouse_pos_for_draw.get(),
                        &tab_vis_for_draw,
                    );
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
            let mc = gtk4::EventControllerMotion::new();
            mc.connect_motion(move |_, x, y| {
                pos_cell.set((x, y));
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
                self.handle_tab_right_click(group_id, tab_idx, x, y, &sender);
            }
            Msg::TabSwitcherRelease => {
                // Handled directly by the root EventControllerKey release handler.
                // Kept as a no-op for exhaustive match.
            }
            Msg::EditorRightClick { x, y } => {
                self.handle_editor_right_click(x, y);
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
                ) {
                    engine.add_cursor_at_pos(line, col);
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
                );
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
            Msg::ToggleFindDialog
            | Msg::FindTextChanged(_)
            | Msg::ReplaceTextChanged(_)
            | Msg::FindNext
            | Msg::FindPrevious
            | Msg::ReplaceNext
            | Msg::ReplaceAll
            | Msg::CloseFindDialog
            | Msg::WindowResized { .. }
            | Msg::SidebarResized => {
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
    let tab_bar_height = if engine.settings.breadcrumbs {
        line_height * 2.0
    } else {
        line_height
    };
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let status_bar_height = line_height * 2.0 + wildmenu_px;
    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };
    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        6.0 * line_height
    } else {
        0.0
    };
    let term_px = if engine.terminal_open || engine.bottom_panel_open {
        (engine.session.terminal_panel_rows as f64 + 2.0) * line_height
    } else {
        0.0
    };
    let editor_bounds = core::WindowRect::new(
        0.0,
        0.0,
        da_width,
        da_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
    );
    let (window_rects, _dividers) =
        engine.calculate_group_window_rects(editor_bounds, tab_bar_height);

    // Hide scrollbars for windows not in the current visible set
    // (e.g. windows in non-active tabs).
    let visible_ids: std::collections::HashSet<core::WindowId> =
        window_rects.iter().map(|(wid, _)| *wid).collect();
    for (wid, ws) in scrollbars.iter() {
        if visible_ids.contains(wid) {
            ws.vertical.set_visible(true);
            ws.cursor_indicator.set_visible(true);
        } else {
            ws.vertical.set_visible(false);
            ws.cursor_indicator.set_visible(false);
        }
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
                        if let Some(ref tree) = *self.file_tree_view.borrow() {
                            highlight_file_in_tree(tree, &path);
                        }
                        if let Some(ref drawing) = *self.drawing_area.borrow() {
                            drawing.grab_focus();
                        }
                        self.tree_has_focus = false;
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

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut last_file: Option<PathBuf> = None;

        for (idx, m) in results.iter().enumerate() {
            // Add a file header row when the file changes
            if last_file.as_deref() != Some(&m.file) {
                last_file = Some(m.file.clone());
                let rel = m.file.strip_prefix(&cwd).unwrap_or(&m.file);
                let file_label = gtk4::Label::new(None);
                let header_markup = format!(
                    "<b><span foreground='#569cd6'>{}</span></b>",
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
                "<span foreground='#cccccc'>{}</span>",
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
        let tab_bar_height = if engine.settings.breadcrumbs {
            line_height * 2.0
        } else {
            line_height
        };
        let wildmenu_px = if engine.wildmenu_items.is_empty() {
            0.0
        } else {
            line_height
        };
        let status_bar_height = line_height * 2.0 + wildmenu_px;
        let debug_toolbar_px = if engine.debug_toolbar_visible {
            line_height
        } else {
            0.0
        };
        let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
            6.0 * line_height
        } else {
            0.0
        };
        let term_px = if engine.terminal_open || engine.bottom_panel_open {
            (engine.session.terminal_panel_rows as f64 + 2.0) * line_height
        } else {
            0.0
        };

        let editor_bounds = WindowRect::new(
            0.0,
            0.0,
            da_width,
            da_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
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
        // (e.g. windows in non-active tabs).
        let visible_ids: std::collections::HashSet<core::WindowId> =
            window_rects.iter().map(|(wid, _)| *wid).collect();
        for (wid, ws) in scrollbars.iter() {
            if visible_ids.contains(wid) {
                ws.vertical.set_visible(true);
                ws.cursor_indicator.set_visible(true);
            } else {
                ws.vertical.set_visible(false);
                ws.cursor_indicator.set_visible(false);
            }
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
        cursor_indicator.set_draw_func(|_, cr, w, h| {
            cr.set_source_rgba(0.5, 0.5, 0.5, 0.8);
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
                self.tree_has_focus = false;
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
                    if let Some(ref tree) = *self.file_tree_view.borrow() {
                        highlight_file_in_tree(tree, &path);
                    }
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
        // Apply tab visible counts reported by the last draw callback.
        {
            let counts = self.tab_visible_counts.borrow().clone();
            if !counts.is_empty() {
                let mut engine = self.engine.borrow_mut();
                for (group_id, count) in &counts {
                    engine.set_tab_visible_count(*group_id, *count);
                }
                self.tab_visible_counts.borrow_mut().clear();
            }
        }
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
        // Explorer refresh after confirmed file move.
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            sender.input(Msg::RefreshFileTree);
        }
        // Auto-refresh SC panel every 2s to pick up external git changes.
        // Also refresh when Explorer is active (for git status indicators).
        if self.sidebar_visible
            && (self.active_panel == SidebarPanel::Git
                || self.active_panel == SidebarPanel::Explorer)
            && self.last_sc_refresh.elapsed() >= std::time::Duration::from_secs(2)
        {
            self.engine.borrow_mut().sc_refresh();
            self.last_sc_refresh = std::time::Instant::now();
            if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
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
        // Update explorer tree indicators (modified/diagnostics) every ~1s.
        if self.last_tree_indicator_update.elapsed() >= std::time::Duration::from_secs(1) {
            self.last_tree_indicator_update = std::time::Instant::now();
            if let Some(ref store) = self.tree_store {
                let engine = self.engine.borrow();
                let (git_statuses, diag_counts) = engine.explorer_indicators();
                let theme = Theme::from_name(&engine.settings.colorscheme);
                update_tree_indicators(
                    store,
                    &git_statuses,
                    &diag_counts,
                    &theme.git_added.to_hex(),
                    &theme.git_modified.to_hex(),
                    &theme.git_deleted.to_hex(),
                    &theme.diagnostic_error.to_hex(),
                    &theme.diagnostic_warning.to_hex(),
                );
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
                let _action = self.engine.borrow_mut().dialog_click_button(idx);
                if self.engine.borrow().explorer_needs_refresh {
                    self.engine.borrow_mut().explorer_needs_refresh = false;
                    sender.input(Msg::RefreshFileTree);
                }
            } else if outside {
                self.engine.borrow_mut().dialog = None;
                self.engine.borrow_mut().pending_move = None;
            }
            self.draw_needed.set(true);
        } else {
            // ── Status bar branch click — open branch picker ─────────────
            if self.cached_line_height > 0.0 {
                let lh = self.cached_line_height;
                let engine = self.engine.borrow();
                let wildmenu_px = if engine.wildmenu_items.is_empty() {
                    0.0
                } else {
                    lh
                };
                let status_bar_height = lh * 2.0 + wildmenu_px;
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
                    let status_h = 2.0 * self.cached_line_height;
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
                    // Right-aligned buttons (3 chars each): + ⊞ ×
                    let close_x = width - self.cached_char_width * 2.0;
                    let split_x = width - self.cached_char_width * 4.0;
                    let add_x = width - self.cached_char_width * 6.0;
                    if x < tab_area_px && self.cached_char_width > 0.0 {
                        let idx =
                            (x / (TERMINAL_TAB_COLS as f64 * self.cached_char_width)) as usize;
                        sender.input(Msg::TerminalSwitchTab(idx));
                    } else if x >= close_x {
                        sender.input(Msg::TerminalCloseActiveTab);
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
                        let wildmenu_px = if engine.wildmenu_items.is_empty() {
                            0.0
                        } else {
                            lh
                        };
                        let status_h = lh * 2.0 + wildmenu_px;
                        let dbg_px = if engine.debug_toolbar_visible {
                            lh
                        } else {
                            0.0
                        };
                        let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
                            6.0 * lh
                        } else {
                            0.0
                        };
                        let term_px = if engine.terminal_open || engine.bottom_panel_open {
                            (engine.session.terminal_panel_rows as f64 + 2.0) * lh
                        } else {
                            0.0
                        };
                        let editor_bottom = height - status_h - dbg_px - qf_px - term_px;
                        let content_bounds =
                            core::window::WindowRect::new(0.0, 0.0, width, editor_bottom);
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
                        let click_result = handle_mouse_click(
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
                        );
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
                                // Tab bar / split button click — skip hooks.
                                // Record drag start position for tab drag-and-drop.
                                self.tab_drag_start = Some((x, y));
                                // Defer sidebar tree highlight so tab switch renders instantly.
                                let new_file_path = engine.file_path().cloned();
                                drop(engine);
                                if new_file_path != file_before_click {
                                    if let Some(path) = new_file_path {
                                        let tree_ref = self.file_tree_view.clone();
                                        gtk4::glib::timeout_add_local_once(
                                            std::time::Duration::from_millis(50),
                                            move || {
                                                if let Some(ref tree) = *tree_ref.borrow() {
                                                    highlight_file_in_tree(tree, &path);
                                                }
                                            },
                                        );
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
                    // editor click.  highlight_file_in_tree does a full DFS of the
                    // GTK TreeStore which is O(N_files) and very slow in debug builds.
                    let new_file_path = engine.file_path().cloned();
                    drop(engine);
                    if new_file_path != file_before_click {
                        if let Some(path) = new_file_path {
                            if let Some(ref tree) = *self.file_tree_view.borrow() {
                                highlight_file_in_tree(tree, &path);
                            }
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
            let wildmenu_px = if engine.wildmenu_items.is_empty() {
                0.0
            } else {
                lh
            };
            let status_h = lh * 2.0 + wildmenu_px;
            let dbg_px = if engine.debug_toolbar_visible {
                lh
            } else {
                0.0
            };
            let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
                6.0 * lh
            } else {
                0.0
            };
            let term_px = if engine.terminal_open || engine.bottom_panel_open {
                (engine.session.terminal_panel_rows as f64 + 2.0) * lh
            } else {
                0.0
            };
            let editor_bottom = height - status_h - dbg_px - qf_px - term_px;
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
                let status_h = 2.0 * self.cached_line_height;
                let available = (height - y - status_h).max(0.0);
                let new_rows = ((available / self.cached_line_height) as u16)
                    .saturating_sub(2)
                    .clamp(5, 30);
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
                let status_h = 2.0 * self.cached_line_height;
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
                    let status_h = 2.0 * self.cached_line_height;
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
        self.draw_needed.set(true);
    }

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
                        sender.input(Msg::ToggleFindDialog);
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
                self.tree_has_focus = false;

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
                self.tree_has_focus = false;
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
                    // Rebuild settings form so widgets reflect current engine.settings
                    // (e.g. toggles changed via :set command since the panel was last open).
                    if self.active_panel == SidebarPanel::Settings {
                        if let Some(ref lb) = *self.settings_list_box.borrow() {
                            while let Some(child) = lb.first_child() {
                                lb.remove(&child);
                            }
                            let engine = self.engine.borrow();
                            let new_sections =
                                build_settings_form(lb, &engine.settings, &self.sender);
                            drop(engine);
                            *self.settings_sections.borrow_mut() = new_sections;
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
                        _ => {}
                    }
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
                self.tree_has_focus = false;
                if let Some(ref tree) = *self.file_tree_view.borrow() {
                    highlight_file_in_tree(tree, &path);
                }
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                self.tree_has_focus = false;
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
                self.tree_has_focus = false;
                self.draw_needed.set(true);
            }
            Msg::PreviewFileFromSidebar(path) => {
                let mut engine = self.engine.borrow_mut();
                // Single-click: open as a preview tab (replaceable by next single-click).
                engine.open_file_preview(&path);
                drop(engine);
                if let Some(ref tree) = *self.file_tree_view.borrow() {
                    highlight_file_in_tree(tree, &path);
                }
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                self.tree_has_focus = false;
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
                        // Highlight the new folder in the tree after refresh
                        let tree_ref = self.file_tree_view.clone();
                        let path = folder_path.clone();
                        gtk4::glib::idle_add_local_once(move || {
                            if let Some(ref tree) = *tree_ref.borrow() {
                                highlight_file_in_tree(tree, &path);
                            }
                        });
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message =
                            format!("Error creating folder '{}': {}", name, e);
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::StartInlineNewFile(parent_dir) => {
                let is_folder = false;
                self.start_inline_new_entry(parent_dir, is_folder);
            }
            Msg::StartInlineNewFolder(parent_dir) => {
                let is_folder = true;
                self.start_inline_new_entry(parent_dir, is_folder);
            }
            Msg::ExplorerActivateSelected => {
                if let Some(ref tv) = *self.file_tree_view.borrow() {
                    // Try cursor position first (tracks arrow-key navigation),
                    // fall back to selection.
                    use gtk4::prelude::TreeViewExt;
                    let tp = TreeViewExt::cursor(tv).0.or_else(|| {
                        tv.selection()
                            .selected()
                            .map(|(_, iter)| tv.model().unwrap().path(&iter))
                    });
                    let model = tv.model();
                    if let (Some(tp), Some(model)) = (tp, model) {
                        // Sync selection to cursor so visual highlight matches.
                        tv.selection().select_path(&tp);
                        if let Some(iter) = model.iter(&tp) {
                            let full_path: String =
                                model.get_value(&iter, 2).get().unwrap_or_default();
                            let path_buf = PathBuf::from(&full_path);
                            if path_buf.is_dir() {
                                if tv.row_expanded(&tp) {
                                    tv.collapse_row(&tp);
                                } else {
                                    tv.expand_row(&tp, false);
                                }
                            } else if path_buf.is_file() {
                                sender.input(Msg::OpenFileFromSidebar(path_buf));
                            }
                        }
                    }
                }
            }
            Msg::ExplorerAction(key_str) => {
                use crate::core::settings::ExplorerAction;
                // Resolve the action first, then drop the engine borrow before
                // calling methods that may re-borrow (e.g. start_inline_new_entry).
                let action = key_str
                    .chars()
                    .next()
                    .and_then(|ch| self.engine.borrow().settings.explorer_keys.resolve(ch));
                if let Some(action) = action {
                    match action {
                        ExplorerAction::NewFile => {
                            let parent_dir = selected_parent_dir_from_app(&self.file_tree_view);
                            self.start_inline_new_entry(parent_dir, false);
                        }
                        ExplorerAction::NewFolder => {
                            let parent_dir = selected_parent_dir_from_app(&self.file_tree_view);
                            self.start_inline_new_entry(parent_dir, true);
                        }
                        ExplorerAction::Delete => {
                            if let Some(path) = selected_file_path_from_app(&self.file_tree_view) {
                                sender.input(Msg::ConfirmDeletePath(path));
                            }
                        }
                        ExplorerAction::Rename => {
                            // Trigger GTK native inline cell editing
                            let tv_ref = self.file_tree_view.clone();
                            let nc_ref = self.name_cell.clone();
                            gtk4::glib::idle_add_local_once(move || {
                                if let Some(ref tv) = *tv_ref.borrow() {
                                    if let Some(ref nc) = *nc_ref.borrow() {
                                        nc.set_property("editable", true);
                                        if let Some(column) = tv.column(0) {
                                            if let Some((model, iter)) = tv.selection().selected() {
                                                let tree_path = model.path(&iter);
                                                gtk4::prelude::TreeViewExt::set_cursor(
                                                    tv,
                                                    &tree_path,
                                                    Some(&column),
                                                    true,
                                                );
                                            }
                                        }
                                    }
                                }
                            });
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
                if let Some(ref store) = self.tree_store {
                    let cwd = self.engine.borrow().cwd.clone();
                    let (dir_fg_hex, file_fg_hex) = {
                        let theme = Theme::from_name(&self.engine.borrow().settings.colorscheme);
                        let file_fg = if theme.is_light() {
                            theme.foreground.to_hex()
                        } else {
                            theme.status_fg.to_hex()
                        };
                        (theme.explorer_dir_fg.to_hex(), file_fg)
                    };
                    store.clear();
                    build_file_tree_with_root(
                        store,
                        &cwd,
                        self.engine.borrow().settings.show_hidden_files,
                        &dir_fg_hex,
                        &file_fg_hex,
                    );
                    // Update explorer indicators (modified/diagnostics)
                    {
                        let engine = self.engine.borrow();
                        let (git_statuses, diag_counts) = engine.explorer_indicators();
                        let theme = Theme::from_name(&engine.settings.colorscheme);
                        update_tree_indicators(
                            store,
                            &git_statuses,
                            &diag_counts,
                            &theme.git_added.to_hex(),
                            &theme.git_modified.to_hex(),
                            &theme.git_deleted.to_hex(),
                            &theme.diagnostic_error.to_hex(),
                            &theme.diagnostic_warning.to_hex(),
                        );
                    }
                    if let Some(ref tv) = *self.file_tree_view.borrow() {
                        tv.expand_row(&gtk4::TreePath::from_indices(&[0]), false);
                        // Highlight the active file in the tree after rebuild.
                        if let Some(path) = self.engine.borrow().file_path().cloned() {
                            highlight_file_in_tree(tv, &path);
                        }
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::FocusExplorer => {
                // Ensure sidebar is visible and explorer is active
                self.sidebar_visible = true;
                self.active_panel = SidebarPanel::Explorer;
                self.tree_has_focus = true;
                self.engine.borrow_mut().explorer_has_focus = true;

                // Grab focus on tree view
                if let Some(ref tree) = *self.file_tree_view.borrow() {
                    tree.grab_focus();
                }

                self.draw_needed.set(true);
            }
            Msg::ToggleFocusExplorer => {
                if self.tree_has_focus {
                    // Already focused — return to editor
                    self.tree_has_focus = false;
                    self.engine.borrow_mut().explorer_has_focus = false;
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                } else {
                    self.sidebar_visible = true;
                    self.active_panel = SidebarPanel::Explorer;
                    self.tree_has_focus = true;
                    self.engine.borrow_mut().explorer_has_focus = true;
                    if let Some(ref tree) = *self.file_tree_view.borrow() {
                        tree.grab_focus();
                    }
                }
                self.draw_needed.set(true);
            }
            Msg::ToggleFocusSearch => {
                // Toggle between showing the search panel and returning to the editor.
                // When "exiting" we keep the sidebar visible (don't touch sidebar_visible)
                // to avoid a white-area artifact from the Revealer animation — Ctrl+B
                // closes the sidebar entirely.
                self.tree_has_focus = false;
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
                self.tree_has_focus = false;
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.explorer_has_focus = false;
                    engine.dap_sidebar_has_focus = false;
                }

                // Grab focus on drawing area
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }

                self.draw_needed.set(true);
            }
            _ => unreachable!(),
        }
    }

    /// Insert a temporary row in the TreeStore and start inline editing for new file/folder.
    fn start_inline_new_entry(&self, parent_dir: PathBuf, is_folder: bool) {
        // Extract colorscheme before borrowing tree_view to avoid RefCell conflicts.
        let colorscheme = self.engine.borrow().settings.colorscheme.clone();
        let theme = Theme::from_name(&colorscheme);
        let fg_hex = theme.foreground.to_hex();

        if let Some(ref tree_view) = *self.file_tree_view.borrow() {
            if let Some(model) = tree_view.model() {
                if let Some(tree_store) = model.downcast_ref::<gtk4::TreeStore>() {
                    // Find the parent iter in the tree store
                    let parent_iter = find_tree_iter_for_path(tree_store, &parent_dir);

                    // Expand the parent row if it exists
                    if let Some(ref pi) = parent_iter {
                        let path = tree_store.path(pi);
                        tree_view.expand_row(&path, false);
                    }

                    // Insert a new row as the first child
                    let new_iter = tree_store.prepend(parent_iter.as_ref());
                    let icon = if is_folder { "\u{f07b}" } else { "\u{f15b}" };
                    let marker = if is_folder {
                        format!("__NEW_FOLDER__{}", parent_dir.display())
                    } else {
                        format!("__NEW_FILE__{}", parent_dir.display())
                    };
                    // Use valid hex colors to avoid GTK "Don't know color ''" warnings
                    tree_store.set(
                        &new_iter,
                        &[
                            (0, &icon.to_value()),
                            (1, &"".to_value()),
                            (2, &marker.to_value()),
                            (3, &fg_hex.to_value()),
                            (4, &"".to_value()),
                            (5, &fg_hex.to_value()),
                        ],
                    );

                    // Start inline editing on the new row.
                    // Wrapped in catch_unwind because GTK set_cursor with
                    // start_editing=true can abort the process if it panics
                    // inside an extern "C" callback.
                    let tv = tree_view.clone();
                    let name_cell_ref = self.name_cell.clone();
                    let new_row_path = tree_store.path(&new_iter);
                    gtk4::glib::idle_add_local_once(move || {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if let Some(ref nc) = *name_cell_ref.borrow() {
                                nc.set_property("editable", true);
                                if let Some(column) = tv.column(0) {
                                    gtk4::prelude::TreeViewExt::set_cursor(
                                        &tv,
                                        &new_row_path,
                                        Some(&column),
                                        true,
                                    );
                                }
                            }
                        }));
                    });
                }
            }
        }
    }

    fn handle_find_replace_msg(&mut self, msg: Msg) {
        match msg {
            Msg::ToggleFindDialog => {
                self.find_dialog_visible = !self.find_dialog_visible;
                self.draw_needed.set(true);
            }
            Msg::FindTextChanged(text) => {
                self.find_text = text.clone();
                let mut engine = self.engine.borrow_mut();
                engine.search_query = text;
                engine.run_search();
                self.draw_needed.set(true);
            }
            Msg::ReplaceTextChanged(text) => {
                self.replace_text = text;
            }
            Msg::FindNext => {
                let mut engine = self.engine.borrow_mut();
                engine.search_next();
                self.draw_needed.set(true);
            }
            Msg::FindPrevious => {
                let mut engine = self.engine.borrow_mut();
                engine.search_prev();
                self.draw_needed.set(true);
            }
            Msg::ReplaceNext => {
                let mut engine = self.engine.borrow_mut();

                // Replace current match and find next
                if let Some(current_idx) = engine.search_index {
                    if let Some(&(start, end)) = engine.search_matches.get(current_idx) {
                        engine.start_undo_group();
                        engine.delete_with_undo(start, end);
                        engine.insert_with_undo(start, &self.replace_text);
                        engine.finish_undo_group();

                        // Re-run search and move to next
                        engine.run_search();
                        engine.search_next();
                    }
                }

                self.draw_needed.set(true);
            }
            Msg::ReplaceAll => {
                let mut engine = self.engine.borrow_mut();
                let pattern = engine.search_query.clone();
                let replacement = self.replace_text.clone();

                // Replace in entire buffer
                let last_line = engine.buffer().len_lines().saturating_sub(1);
                match engine.replace_in_range(Some((0, last_line)), &pattern, &replacement, "g") {
                    Ok(count) => {
                        engine.message = format!(
                            "Replaced {} occurrence{}",
                            count,
                            if count == 1 { "" } else { "s" }
                        );
                    }
                    Err(e) => {
                        engine.message = e;
                    }
                }

                // Re-run search to update highlights
                engine.run_search();
                self.draw_needed.set(true);
            }
            Msg::CloseFindDialog => {
                self.find_dialog_visible = false;

                // Clear search highlights
                let mut engine = self.engine.borrow_mut();
                engine.search_matches.clear();
                engine.search_index = None;

                // Return focus to editor
                if let Some(ref drawing_area) = *self.drawing_area.borrow() {
                    drawing_area.grab_focus();
                }

                self.draw_needed.set(true);
            }
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
                    // No unsaved changes — save session and exit immediately.
                    self.save_session_and_exit();
                }
                // Show a confirmation dialog listing the choice.
                let dialog = gtk4::Dialog::with_buttons(
                    Some("Unsaved Changes"),
                    Some(&self.window),
                    gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
                    &[
                        ("Save All & Quit", gtk4::ResponseType::Accept),
                        ("Quit Without Saving", gtk4::ResponseType::Reject),
                        ("Cancel", gtk4::ResponseType::Cancel),
                    ],
                );
                let label = gtk4::Label::new(Some(
                    "You have unsaved changes.\nDo you want to save before quitting?",
                ));
                label.set_margin_top(12);
                label.set_margin_bottom(12);
                label.set_margin_start(12);
                label.set_margin_end(12);
                dialog.content_area().append(&label);
                let engine_clone = self.engine.clone();
                let s = sender.input_sender().clone();
                dialog.connect_response(move |dlg, resp| {
                    dlg.close();
                    match resp {
                        gtk4::ResponseType::Accept => {
                            engine_clone.borrow_mut().save_all_dirty();
                            s.send(Msg::QuitConfirmed).ok();
                        }
                        gtk4::ResponseType::Reject => {
                            s.send(Msg::QuitConfirmed).ok();
                        }
                        _ => {} // Cancel — do nothing
                    }
                });
                dialog.present();
            }

            Msg::QuitConfirmed => {
                // Save session state then exit the process.
                self.save_session_and_exit();
            }

            Msg::ShowCloseTabConfirm => {
                let dialog = gtk4::Dialog::with_buttons(
                    Some("Unsaved Changes"),
                    Some(&self.window),
                    gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
                    &[
                        ("Save & Close Tab", gtk4::ResponseType::Accept),
                        ("Discard & Close Tab", gtk4::ResponseType::Reject),
                        ("Cancel", gtk4::ResponseType::Cancel),
                    ],
                );
                let label = gtk4::Label::new(Some(
                    "This file has unsaved changes.\nDo you want to save before closing the tab?",
                ));
                label.set_margin_top(12);
                label.set_margin_bottom(12);
                label.set_margin_start(12);
                label.set_margin_end(12);
                dialog.content_area().append(&label);
                let s = sender.input_sender().clone();
                dialog.connect_response(move |dlg, resp| {
                    dlg.close();
                    match resp {
                        gtk4::ResponseType::Accept => {
                            s.send(Msg::CloseTabConfirmed { save: true }).ok();
                        }
                        gtk4::ResponseType::Reject => {
                            s.send(Msg::CloseTabConfirmed { save: false }).ok();
                        }
                        _ => {} // Cancel — do nothing
                    }
                });
                dialog.present();
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
}

/// Map a visible row index (0-based from scroll_top) to the corresponding
/// buffer line index, skipping lines hidden inside closed folds.
fn view_row_to_buf_line(
    view: &crate::core::view::View,
    scroll_top: usize,
    view_row: usize,
    total_lines: usize,
) -> usize {
    let mut buf_line = scroll_top;
    let mut visible = 0usize;
    while buf_line < total_lines {
        if view.is_line_hidden(buf_line) {
            buf_line += 1;
            continue;
        }
        if visible == view_row {
            return buf_line;
        }
        visible += 1;
        if let Some(fold) = view.fold_at(buf_line) {
            buf_line = fold.end + 1;
        } else {
            buf_line += 1;
        }
    }
    // Clamp to last valid line
    total_lines.saturating_sub(1)
}

/// Like `view_row_to_buf_line`, but accounts for word-wrapped lines.
/// Returns `(buffer_line, segment_col_offset)` — the segment offset is the
/// character index within the buffer line where the clicked visual segment starts.
fn view_row_to_buf_pos_wrap(
    view: &crate::core::view::View,
    buffer: &crate::core::buffer::Buffer,
    scroll_top: usize,
    view_row: usize,
    total_lines: usize,
    viewport_cols: usize,
) -> (usize, usize) {
    let mut buf_line = scroll_top;
    let mut visible = 0usize;
    while buf_line < total_lines {
        if view.is_line_hidden(buf_line) {
            buf_line += 1;
            continue;
        }
        // Compute how many visual rows this buffer line occupies when wrapped.
        let line_str = buffer.content.line(buf_line).to_string();
        let line_str = line_str.trim_end_matches('\n');
        let segments = render::compute_word_wrap_segments(line_str, viewport_cols);
        let visual_rows = segments.len();
        if view_row < visible + visual_rows {
            // The clicked row falls within this buffer line.
            let seg_idx = view_row - visible;
            let seg_col_offset = segments.get(seg_idx).map(|&(start, _)| start).unwrap_or(0);
            return (buf_line, seg_col_offset);
        }
        visible += visual_rows;
        if let Some(fold) = view.fold_at(buf_line) {
            buf_line = fold.end + 1;
        } else {
            buf_line += 1;
        }
    }
    (total_lines.saturating_sub(1), 0)
}

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

/// Compute editor window rects with the same formula used by draw_editor and
/// sync_scrollbar, so event handlers can do hit-testing without duplicating the
/// layout logic.
fn compute_editor_window_rects(
    engine: &Engine,
    da_width: f64,
    da_height: f64,
    line_height: f64,
) -> Vec<(core::WindowId, core::WindowRect)> {
    let tab_bar_height = if engine.settings.breadcrumbs {
        line_height * 2.0
    } else {
        line_height
    };
    let wildmenu_px = if engine.wildmenu_items.is_empty() {
        0.0
    } else {
        line_height
    };
    let status_bar_height = line_height * 2.0 + wildmenu_px;
    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };
    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        6.0 * line_height
    } else {
        0.0
    };
    let term_px = if engine.terminal_open || engine.bottom_panel_open {
        (engine.session.terminal_panel_rows as f64 + 2.0) * line_height
    } else {
        0.0
    };
    let editor_bounds = core::WindowRect::new(
        0.0,
        0.0,
        da_width,
        da_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
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
    let tab_bar_height = if engine.settings.breadcrumbs {
        line_height * 2.0
    } else {
        line_height
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
    let tab_inner_gap = 4.0_f64;
    let tab_outer_gap = 4.0_f64;
    let close_pad = char_width;

    for (gid, grect) in &group_rects {
        if engine.is_tab_bar_hidden(*gid) {
            continue;
        }
        let tab_y = grect.y - tab_bar_height;
        if my < tab_y || my >= tab_y + line_height || mx < grect.x || mx >= grect.x + grect.width {
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
                let slot_w = tab_w + tab_inner_gap + close_w + tab_outer_gap;
                if local_x >= tab_x && local_x < tab_x + slot_w {
                    let close_x_start = tab_x + tab_w + tab_inner_gap - close_pad;
                    let close_x_end = tab_x + slot_w;
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
    let tab_bar_height = if engine.settings.breadcrumbs {
        line_height * 2.0
    } else {
        line_height
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
    let tab_inner_gap = 4.0_f64;
    let tab_outer_gap = 4.0_f64;

    for (gid, grect) in &group_rects {
        if engine.is_tab_bar_hidden(*gid) {
            continue;
        }
        let tab_y = grect.y - tab_bar_height;
        if my < tab_y || my >= tab_y + line_height || mx < grect.x || mx >= grect.x + grect.width {
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
                let slot_w = tab_w + tab_inner_gap + close_w + tab_outer_gap;
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

            let bt = std::backtrace::Backtrace::force_capture();
            let loc_str = info
                .location()
                .map(|l| format!("  at {}:{}:{}\n", l.file(), l.line(), l.column()))
                .unwrap_or_default();
            let crash_msg = format!("PANIC: {}\n{}backtrace:\n{}\n", info, loc_str, bt);
            let _ = std::fs::write("/tmp/vimcode-crash.log", &crash_msg);
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
