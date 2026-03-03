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

mod core;
mod icons;
mod render;
mod tui_main;

use core::engine::EngineAction;
use core::lsp::DiagnosticSeverity;
use core::settings::{parse_key_binding, LineNumberMode};
use core::{Engine, GitLineStatus, OpenMode, WindowRect};
use render::{
    build_screen_layout, CommandLineData, CursorShape, RenderedWindow, SelectionKind,
    SelectionRange, StyledSpan, TabInfo, Theme,
};

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Variants used in later phases
enum SidebarPanel {
    Explorer,
    Search,
    Debug,
    Git,
    Settings,
    None,
}

use std::collections::HashMap;

use copypasta_ext::ClipboardProviderExt;

/// Returns true if `key` + `state` match a panel_keys binding string like `<C-b>`, `<C-S-e>`.
fn matches_gtk_key(binding: &str, key: gdk::Key, state: gdk::ModifierType) -> bool {
    let Some((ctrl, shift, alt, key_char)) = parse_key_binding(binding) else {
        return false;
    };
    if ctrl != state.contains(gdk::ModifierType::CONTROL_MASK) {
        return false;
    }
    if shift != state.contains(gdk::ModifierType::SHIFT_MASK) {
        return false;
    }
    if alt != state.contains(gdk::ModifierType::ALT_MASK) {
        return false;
    }
    key.to_unicode()
        .map(|c| c.to_ascii_lowercase() == key_char)
        .unwrap_or(false)
}

struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
    sidebar_visible: bool,
    active_panel: SidebarPanel,
    tree_store: Option<gtk4::TreeStore>,
    tree_has_focus: bool,
    file_tree_view: Rc<RefCell<Option<gtk4::TreeView>>>,
    drawing_area: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    menu_bar_da: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    debug_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    git_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    sidebar_inner_box: Rc<RefCell<Option<gtk4::Box>>>,
    /// Direct ref to the sidebar Revealer for programmatic open/close.
    sidebar_revealer: Rc<RefCell<Option<gtk4::Revealer>>>,
    /// Direct refs to each panel's outer Box for programmatic show/hide.
    explorer_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    search_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    debug_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    git_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    settings_panel_box: Rc<RefCell<Option<gtk4::Box>>>,
    // Per-window scrollbars and indicators
    window_scrollbars: Rc<RefCell<HashMap<core::WindowId, WindowScrollbars>>>,
    overlay: Rc<RefCell<Option<gtk4::Overlay>>>,
    cached_line_height: f64,
    cached_char_width: f64,
    /// Shared with the drawing-area resize callback so scrollbars can be
    /// repositioned synchronously (before each frame) without going through
    /// Relm4's async message queue.
    line_height_cell: Rc<Cell<f64>>,
    char_width_cell: Rc<Cell<f64>>,
    /// Shared with draw closure: hovered state for Cairo h scrollbars.
    h_sb_hovered_cell: Rc<Cell<bool>>,
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
    /// File selected as "left side" for a two-way diff (via context menu).
    diff_selected_file: Option<PathBuf>,
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
    /// True while the user is dragging the terminal panel's scrollbar thumb.
    terminal_sb_dragging: bool,
    /// True while the user drags the terminal header row to resize the panel.
    terminal_resize_dragging: bool,
    /// True while the user drags the terminal split divider left/right.
    terminal_split_dragging: bool,
    /// Reference to the root GTK window used for minimize / maximize / close actions.
    window: gtk4::Window,
    /// Last time sc_refresh() was called for the Git sidebar auto-refresh.
    last_sc_refresh: std::time::Instant,
    /// Full-window overlay DrawingArea that draws the menu dropdown.
    /// Can-target toggles true/false with menu open/close.
    menu_dropdown_da: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    /// Cached line height shared with menu_dropdown_da draw/click closures.
    menu_dd_line_height: Rc<Cell<f64>>,
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
    },
    /// Toggle sidebar visibility.
    ToggleSidebar,
    /// Switch to a different sidebar panel.
    SwitchPanel(SidebarPanel),
    /// Open file from sidebar tree view (switches to existing tab or opens new permanent tab).
    /// Used for double-click.
    OpenFileFromSidebar(PathBuf),
    /// Preview file from sidebar tree view (single-click, replaces current preview tab).
    PreviewFileFromSidebar(PathBuf),
    /// Create a new file: (parent_dir, name).
    CreateFile(PathBuf, String),
    /// Create a new folder: (parent_dir, name).
    CreateFolder(PathBuf, String),
    /// Show confirmation dialog before deleting.
    ConfirmDeletePath(PathBuf),
    /// Delete a file or folder at the given path (after confirmation).
    DeletePath(PathBuf),
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
    WindowResized { width: i32, height: i32 },
    /// Window closing (save session state).
    WindowClosing { width: i32, height: i32 },
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
    MouseScroll { delta_x: f64, delta_y: f64 },
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
    /// Mouse moved (used for hover detection on Cairo-drawn scrollbars).
    MouseMove { x: f64, y: f64 },
    /// Rename a file: (old_path, new_name_without_dir)
    RenameFile(PathBuf, String),
    /// Move a file to a different directory: (src, dest_dir)
    MoveFile(PathBuf, PathBuf),
    /// Copy the file path to the clipboard.
    CopyPath(PathBuf),
    /// Remember this file as the "left side" for a two-way diff.
    SelectForDiff(PathBuf),
    /// Open a vsplit diff: current file is right side, stored path is left.
    DiffWithSelected(PathBuf),
    /// GDK clipboard text arrived for pasting into command/search/insert input.
    ClipboardPasteToInput { text: String },
    /// Toggle the integrated terminal panel open/closed.
    ToggleTerminal,
    /// Open a new terminal tab.
    NewTerminalTab,
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
    TerminalMouseDown { row: u16, col: u16 },
    /// Mouse dragged to terminal cell (row, col).
    TerminalMouseDrag { row: u16, col: u16 },
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
    /// Activate a menu item: (menu_idx, item_idx, action_str).
    MenuActivateItem(usize, usize, String),
    /// Click in the debug sidebar DrawingArea (x, y coordinates in pixels).
    DebugSidebarClick(f64, f64),
    /// Key press in the debug sidebar DrawingArea.
    DebugSidebarKey(String, bool),
    /// Scroll in the debug sidebar DrawingArea (dy value from EventControllerScroll).
    DebugSidebarScroll(f64),
    /// Click in the Source Control sidebar DrawingArea (x, y coordinates in pixels).
    ScSidebarClick(f64, f64),
    /// Key press in the Source Control sidebar DrawingArea.
    ScKey(String, bool),
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
                        set_label: "\u{f002}",
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

                    // Container for both panels (Explorer and Settings)
                    #[name = "sidebar_inner_box"]
                    gtk4::Box {
                        set_orientation: gtk4::Orientation::Vertical,
                        set_width_request: 260,

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
                                    show_name_prompt_dialog("New File", "", {
                                        let s = sender.clone();
                                        move |name| s.input(Msg::CreateFile(parent_dir.clone(), name))
                                    });
                                }
                            },

                            gtk4::Button {
                                set_label: "\u{f07b}",
                                set_tooltip_text: Some("New Folder"),
                                set_width_request: 32,
                                set_height_request: 32,
                                connect_clicked[sender, file_tree_view] => move |_| {
                                    let parent_dir = selected_parent_dir(&file_tree_view);
                                    show_name_prompt_dialog("New Folder", "", {
                                        let s = sender.clone();
                                        move |name| s.input(Msg::CreateFolder(parent_dir.clone(), name))
                                    });
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

                                        // Arrow keys for navigation - let TreeView handle them
                                        if matches!(key_name.as_str(), "Up" | "Down" | "Left" | "Right" | "Return" | "space") {
                                            return gtk4::glib::Propagation::Proceed;
                                        }

                                        // Stop all other keys from triggering TreeView search
                                        gtk4::glib::Propagation::Stop
                                    }
                                },
                            },
                        },
                        },

                        // Settings panel
                        #[name = "settings_panel"]
                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_css_classes: &["sidebar"],

                            #[watch]
                            set_visible: model.active_panel == SidebarPanel::Settings,

                        gtk4::Box {
                            set_orientation: gtk4::Orientation::Vertical,
                            set_margin_all: 12,
                            set_spacing: 12,

                            gtk4::Label {
                                set_text: "Settings",
                                set_halign: gtk4::Align::Start,
                                set_css_classes: &["heading"],
                            },

                            gtk4::Button {
                                set_label: "Open settings.json",

                                connect_clicked[sender] => move |_| {
                                    sender.input(Msg::OpenSettingsFile);
                                }
                            },

                            gtk4::Label {
                                set_text: "Settings file will auto-reload on save",
                                set_halign: gtk4::Align::Start,
                                set_css_classes: &["dim-label"],
                            },
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
                    },
                },

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
                                connect_key_pressed[sender, engine] => move |_, key, _, modifier| {
                                    let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                                    let unicode = key.to_unicode().filter(|c| !c.is_control());
                                    let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                                    let shift = modifier.contains(gdk::ModifierType::SHIFT_MASK);
                                    let alt = modifier.contains(gdk::ModifierType::ALT_MASK);

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

                                    // Alt-M: toggle Vim ↔ VSCode editing mode
                                    if alt && !ctrl && !shift && unicode == Some('m') {
                                        engine.borrow_mut().toggle_editor_mode();
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
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
                                        sender.input(Msg::KeyPress {
                                            key_name: "p".to_string(),
                                            unicode: Some('p'),
                                            ctrl: true,
                                        });
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.live_grep, key, modifier) {
                                        sender.input(Msg::KeyPress {
                                            key_name: "g".to_string(),
                                            unicode: Some('g'),
                                            ctrl: true,
                                        });
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.add_cursor, key, modifier) {
                                        engine.borrow_mut().add_cursor_at_next_match();
                                        sender.input(Msg::Resize);
                                        return gtk4::glib::Propagation::Stop;
                                    }
                                    if matches_gtk_key(&pk.select_all_matches, key, modifier) {
                                        engine.borrow_mut().select_all_word_occurrences();
                                        sender.input(Msg::Resize);
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

                                    // In VSCode mode, transform Shift+Arrow to "Shift_" prefixed
                                    // key names so the engine's vscode handler can distinguish them.
                                    let effective_key = if engine.borrow().is_vscode_mode() && shift {
                                        match key_name.as_str() {
                                            "Right" => "Shift_Right".to_string(),
                                            "Left"  => "Shift_Left".to_string(),
                                            "Up"    => "Shift_Up".to_string(),
                                            "Down"  => "Shift_Down".to_string(),
                                            "Home"  => "Shift_Home".to_string(),
                                            "End"   => "Shift_End".to_string(),
                                            _       => key_name,
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
                                    if modifier.contains(gdk::ModifierType::CONTROL_MASK) {
                                        sender.input(Msg::CtrlMouseClick { x, y, width, height });
                                    } else if n_press >= 2 {
                                        sender.input(Msg::MouseDoubleClick { x, y, width, height });
                                    } else {
                                        sender.input(Msg::MouseClick { x, y, width, height });
                                    }
                                }
                            },

                            add_controller = gtk4::GestureDrag {
                                set_button: 1,
                                connect_drag_update[sender, drawing_area] => move |gesture, dx, dy| {
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
                                drawing_area.queue_draw();
                                if model.redraw { &["vim-code", "even"] } else { &["vim-code", "odd"] }
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
        // Load CSS before creating widgets
        load_css();

        let engine = {
            let mut e = Engine::new();
            e.restore_session_files();
            if let Some(ref path) = file_path {
                e.open_file_in_tab(path);
            }
            e
        };

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

        // Create TreeStore with 3 columns: Icon(String), Name(String), FullPath(String)
        let tree_store = gtk4::TreeStore::new(&[
            gtk4::glib::Type::STRING, // Icon
            gtk4::glib::Type::STRING, // Name
            gtk4::glib::Type::STRING, // Full path
        ]);

        let file_tree_view_ref = Rc::new(RefCell::new(None));
        let drawing_area_ref = Rc::new(RefCell::new(None));
        let menu_bar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> = Rc::new(RefCell::new(None));
        let menu_dropdown_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let menu_dd_lh: Rc<Cell<f64>> = Rc::new(Cell::new(24.0));
        let debug_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let git_sidebar_da_ref: Rc<RefCell<Option<gtk4::DrawingArea>>> =
            Rc::new(RefCell::new(None));
        let overlay_ref = Rc::new(RefCell::new(None));
        let window_scrollbars_ref = Rc::new(RefCell::new(HashMap::new()));
        let line_height_cell: Rc<Cell<f64>> = Rc::new(Cell::new(24.0));
        let char_width_cell: Rc<Cell<f64>> = Rc::new(Cell::new(9.0));
        // Shared state for Cairo h scrollbar hover/drag — read by set_draw_func closure.
        let h_sb_hovered_cell: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let h_sb_drag_cell: Rc<Cell<Option<core::WindowId>>> = Rc::new(Cell::new(None));
        let sidebar_inner_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let sidebar_revealer_ref: Rc<RefCell<Option<gtk4::Revealer>>> = Rc::new(RefCell::new(None));
        // Saves the sidebar width at the start of a drag so we can compute
        // initial_width + total_offset instead of accumulating delta per event.
        let sidebar_drag_start_w: Rc<Cell<i32>> = Rc::new(Cell::new(300));
        let explorer_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let search_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let debug_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let git_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
        let settings_panel_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
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
                        // ChangesDoneHint fires once after the write completes, avoiding
                        // multiple events during a single save. Fall back to Changed on
                        // filesystems that don't emit ChangesDoneHint.
                        if event == gio::FileMonitorEvent::ChangesDoneHint
                            || event == gio::FileMonitorEvent::Changed
                        {
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
            redraw: false,
            sidebar_visible,
            window: root.clone(),
            active_panel: SidebarPanel::Explorer,
            tree_store: Some(tree_store.clone()),
            tree_has_focus: false,
            file_tree_view: file_tree_view_ref.clone(),
            drawing_area: drawing_area_ref.clone(),
            menu_bar_da: menu_bar_da_ref.clone(),
            debug_sidebar_da_ref: debug_sidebar_da_ref.clone(),
            git_sidebar_da_ref: git_sidebar_da_ref.clone(),
            window_scrollbars: window_scrollbars_ref.clone(),
            overlay: overlay_ref.clone(),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
            line_height_cell: line_height_cell.clone(),
            char_width_cell: char_width_cell.clone(),
            h_sb_hovered_cell: h_sb_hovered_cell.clone(),
            h_sb_drag_cell: h_sb_drag_cell.clone(),
            settings_monitor,
            sender: sender.input_sender().clone(),
            find_dialog_visible: false,
            find_text: String::new(),
            replace_text: String::new(),
            find_case_sensitive: false,
            find_whole_word: false,
            sidebar_inner_box: sidebar_inner_box_ref.clone(),
            sidebar_revealer: sidebar_revealer_ref.clone(),
            explorer_panel_box: explorer_panel_box_ref.clone(),
            search_panel_box: search_panel_box_ref.clone(),
            debug_panel_box: debug_panel_box_ref.clone(),
            git_panel_box: git_panel_box_ref.clone(),
            settings_panel_box: settings_panel_box_ref.clone(),
            project_search_status: String::new(),
            search_results_list: search_results_list_ref.clone(),
            diff_selected_file: None,
            last_clipboard_content: None,
            clipboard,
            h_sb_dragging: None,
            h_sb_hovered: false,
            terminal_sb_dragging: false,
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            last_sc_refresh: std::time::Instant::now(),
            menu_dropdown_da: menu_dropdown_da_ref.clone(),
            menu_dd_line_height: menu_dd_lh.clone(),
        };
        let widgets = view_output!();

        // Store widget references
        *file_tree_view_ref.borrow_mut() = Some(widgets.file_tree_view.clone());
        *drawing_area_ref.borrow_mut() = Some(widgets.drawing_area.clone());
        *menu_bar_da_ref.borrow_mut() = Some(widgets.menu_bar_da.clone());
        *overlay_ref.borrow_mut() = Some(widgets.editor_overlay.clone());
        *sidebar_inner_box_ref.borrow_mut() = Some(widgets.sidebar_inner_box.clone());
        *sidebar_revealer_ref.borrow_mut() = Some(widgets.sidebar_revealer.clone());
        *explorer_panel_box_ref.borrow_mut() = Some(widgets.explorer_panel.clone());
        *search_panel_box_ref.borrow_mut() = Some(widgets.search_panel.clone());
        *debug_panel_box_ref.borrow_mut() = Some(widgets.debug_panel.clone());
        *git_panel_box_ref.borrow_mut() = Some(widgets.git_panel.clone());
        *settings_panel_box_ref.borrow_mut() = Some(widgets.settings_panel.clone());
        *search_results_list_ref.borrow_mut() = Some(widgets.search_results_list.clone());

        // ── Sidebar resize drag handle ─────────────────────────────────────────
        // Set up the GestureDrag imperatively (outside the view! macro) so that
        // the variable captures work reliably with the post-view_output!() refs.
        {
            let gesture = gtk4::GestureDrag::new();
            let sb_ref = sidebar_inner_box_ref.clone();
            let sw = sidebar_drag_start_w.clone();
            gesture.connect_drag_begin(move |_, _, _| {
                if let Some(ref sb) = *sb_ref.borrow() {
                    sw.set(sb.width_request());
                }
            });
            let sb_ref2 = sidebar_inner_box_ref.clone();
            let sw2 = sidebar_drag_start_w.clone();
            gesture.connect_drag_update(move |_, dx, _| {
                let new_w = (sw2.get() as f64 + dx).round() as i32;
                if let Some(ref sb) = *sb_ref2.borrow() {
                    sb.set_width_request(new_w.clamp(80, 600));
                }
            });
            let sender_resize = sender.input_sender().clone();
            gesture.connect_drag_end(move |_, _, _| {
                sender_resize.send(Msg::SidebarResized).ok();
            });
            widgets.sidebar_resize_handle.add_controller(gesture);
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
                    let theme = Theme::onedark();
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
                        .active_buffer_name()
                        .map(|n| format!("VimCode \u{2014} {}", n))
                        .unwrap_or_else(|| "VimCode".to_string());
                    let data = render::MenuBarData {
                        open_menu_idx: engine.menu_open_idx,
                        open_items,
                        open_menu_col,
                        highlighted_item_idx: engine.menu_highlighted_item,
                        title,
                        show_window_controls: true,
                        is_vscode_mode: engine.is_vscode_mode(),
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

            widgets.window_overlay.add_overlay(&menu_dd_da);
            *menu_dropdown_da_ref.borrow_mut() = Some(menu_dd_da);
        }

        // ── Menu bar DrawingArea setup ─────────────────────────────────────────
        // Draw function: renders menu labels using the same Cairo helper.
        {
            let engine = engine.clone();
            widgets.menu_bar_da.set_draw_func(move |da, cr, _w, _h| {
                let engine = engine.borrow();
                // Menu bar is always visible in GTK (acts as the window title bar).
                let theme = Theme::onedark();
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
                    .active_buffer_name()
                    .map(|n| format!("VimCode \u{2014} {}", n))
                    .unwrap_or_else(|| "VimCode".to_string());
                let data = render::MenuBarData {
                    open_menu_idx: engine.menu_open_idx,
                    open_items,
                    open_menu_col,
                    highlighted_item_idx: engine.menu_highlighted_item,
                    title,
                    show_window_controls: true,
                    is_vscode_mode: engine.is_vscode_mode(),
                };
                let w = da.width() as f64;
                let h = da.height() as f64;
                draw_menu_bar(cr, &data, &theme, 0.0, 0.0, w, h);
            });
        }
        // Click gesture: open/close individual menus (no hamburger zone here).
        {
            let sender_menu = sender.input_sender().clone();
            let engine_menu = engine.clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |_, _, x, _y| {
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
                // Click in empty part of bar → close any open dropdown
                if engine.menu_open_idx.is_some() {
                    sender_menu.send(Msg::CloseMenu).ok();
                }
            });
            widgets.menu_bar_da.add_controller(gesture);
        }
        // ── Debug sidebar DrawingArea setup ───────────────────────────────────
        {
            let engine = engine.clone();
            widgets
                .debug_sidebar_da
                .set_draw_func(move |da, cr, _w, _h| {
                    let engine = engine.borrow();
                    let theme = Theme::onedark();
                    let font_desc = FontDescription::from_string(UI_FONT);
                    let pango_ctx = pangocairo::create_context(cr);
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&font_desc));
                    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64
                        / pango::SCALE as f64;
                    let char_width =
                        font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
                    let screen = build_screen_layout(&engine, &theme, &[], line_height, char_width);
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
                let theme = Theme::onedark();
                let font_desc = FontDescription::from_string(UI_FONT);
                let pango_ctx = pangocairo::create_context(cr);
                let layout = pango::Layout::new(&pango_ctx);
                layout.set_font_description(Some(&font_desc));
                let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
                let line_height =
                    (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
                let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
                let screen = build_screen_layout(&engine, &theme, &[], line_height, char_width);
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
            gesture.connect_pressed(move |_, _, x, y| {
                sender_sc.send(Msg::ScSidebarClick(x, y)).ok();
            });
            widgets.git_sidebar_da.add_controller(gesture);
        }
        *git_sidebar_da_ref.borrow_mut() = Some(widgets.git_sidebar_da.clone());

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

        // Restore saved sidebar width
        {
            let saved_width = engine.borrow().session.sidebar_width;
            widgets.sidebar_inner_box.set_width_request(saved_width);
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
        build_file_tree_with_root(&tree_store, &cwd);

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

        // Filename cell renderer (expanding)
        let name_cell = gtk4::CellRendererText::new();
        col.pack_start(&name_cell, true);
        col.add_attribute(&name_cell, "text", 1);

        widgets.file_tree_view.append_column(&col);

        // Set the model on the TreeView
        widgets.file_tree_view.set_model(Some(&tree_store));

        // Expand the root node so the tree contents are visible
        widgets
            .file_tree_view
            .expand_row(&gtk4::TreePath::from_indices(&[0]), false);

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
                        }
                        // If directory, do nothing for now (expand/collapse works automatically)
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
                            }
                        }
                    }
                }
            }
        });
        widgets.file_tree_view.add_controller(gesture);

        // Right-click context menu
        {
            let sender_rc = sender.clone();
            let right_click = gtk4::GestureClick::new();
            right_click.set_button(3); // right mouse button
            right_click.connect_pressed(move |gesture, _n_press, x, y| {
                let widget = gesture.widget();
                let Some(tree_view) = widget.downcast_ref::<gtk4::TreeView>() else {
                    return;
                };
                // Select the row under the cursor
                if let Some((Some(tp), _, _, _)) = tree_view.path_at_pos(x as i32, y as i32) {
                    tree_view.selection().select_path(&tp);
                }
                // Get selected path
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

                // Build a simple popover with buttons
                let menu_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
                menu_box.set_margin_all(4);

                let add_btn = |label: &str| -> gtk4::Button {
                    let b = gtk4::Button::with_label(label);
                    b.set_has_frame(false);
                    b.set_halign(gtk4::Align::Fill);
                    b
                };

                let btn_new_file = add_btn("New File");
                let btn_new_folder = add_btn("New Folder");
                let btn_rename = add_btn("Rename  F2");
                let btn_delete = add_btn("Delete  Del");
                let btn_copy_path = add_btn("Copy Path");
                let btn_select_diff = add_btn("Select for Diff");

                menu_box.append(&btn_new_file);
                menu_box.append(&btn_new_folder);
                menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
                menu_box.append(&btn_rename);
                menu_box.append(&btn_delete);
                menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
                menu_box.append(&btn_copy_path);
                menu_box.append(&btn_select_diff);

                let popover = gtk4::Popover::new();
                popover.set_child(Some(&menu_box));
                popover.set_parent(tree_view);
                // Position near cursor
                let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                popover.set_pointing_to(Some(&rect));
                popover.set_autohide(true);
                popover.popup();

                // Determine parent dir for "New File" / "New Folder"
                let parent_dir = if target.is_dir() {
                    target.clone()
                } else {
                    target
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                };

                // Wire up buttons
                let s = sender_rc.clone();
                let pd = parent_dir.clone();
                let p = popover.clone();
                btn_new_file.connect_clicked(move |_| {
                    p.popdown();
                    let pd2 = pd.clone();
                    show_name_prompt_dialog("New File", "", {
                        let s2 = s.clone();
                        move |name| s2.input(Msg::CreateFile(pd2.clone(), name))
                    });
                });
                let s = sender_rc.clone();
                let pd = parent_dir.clone();
                let p = popover.clone();
                btn_new_folder.connect_clicked(move |_| {
                    p.popdown();
                    let pd2 = pd.clone();
                    show_name_prompt_dialog("New Folder", "", {
                        let s2 = s.clone();
                        move |name| s2.input(Msg::CreateFolder(pd2.clone(), name))
                    });
                });
                let s = sender_rc.clone();
                let tgt = target.clone();
                let tv_clone = tree_view.clone();
                let p = popover.clone();
                btn_rename.connect_clicked(move |_| {
                    // Trigger inline rename via a simple dialog
                    let dialog = gtk4::Dialog::with_buttons(
                        Some("Rename"),
                        None::<&gtk4::Window>,
                        gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
                        &[
                            ("Rename", gtk4::ResponseType::Accept),
                            ("Cancel", gtk4::ResponseType::Cancel),
                        ],
                    );
                    let entry = gtk4::Entry::new();
                    let current = tgt
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    entry.set_text(&current);
                    entry.select_region(0, -1);
                    dialog.content_area().append(&entry);
                    dialog.set_default_response(gtk4::ResponseType::Accept);
                    entry.set_activates_default(true);
                    let s2 = s.clone();
                    let tgt2 = tgt.clone();
                    let tv2 = tv_clone.clone();
                    dialog.connect_response(move |dlg, resp| {
                        if resp == gtk4::ResponseType::Accept {
                            let new_name = entry.text().to_string();
                            if !new_name.is_empty() {
                                s2.input(Msg::RenameFile(tgt2.clone(), new_name));
                            }
                        }
                        tv2.grab_focus();
                        dlg.close();
                    });
                    dialog.present();
                    p.popdown();
                });
                let s = sender_rc.clone();
                let tgt = target.clone();
                let p = popover.clone();
                btn_delete.connect_clicked(move |_| {
                    p.popdown();
                    s.input(Msg::ConfirmDeletePath(tgt.clone()));
                });
                let s = sender_rc.clone();
                let tgt = target.clone();
                let p = popover.clone();
                btn_copy_path.connect_clicked(move |_| {
                    s.input(Msg::CopyPath(tgt.clone()));
                    p.popdown();
                });
                let s = sender_rc.clone();
                let tgt = target.clone();
                let p = popover.clone();
                btn_select_diff.connect_clicked(move |_| {
                    s.input(Msg::SelectForDiff(tgt.clone()));
                    p.popdown();
                });
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

        // Track resize to update viewport_lines and viewport_cols
        let sender_clone = sender.clone();
        let engine_for_resize = engine.clone();
        widgets
            .drawing_area
            .connect_resize(move |_, width, height| {
                let line_height_approx = 24.0_f64;
                let char_width_approx = 9.0_f64; // Approximate for monospace font

                let total_lines = (height as f64 / line_height_approx).floor() as usize;
                let viewport_lines = total_lines.saturating_sub(2);

                let total_cols = (width as f64 / char_width_approx).floor() as usize;
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
        let h_sb_drag_for_draw = h_sb_drag_cell.clone();
        widgets
            .drawing_area
            .set_draw_func(move |_, cr, width, height| {
                let engine = engine_clone.borrow();
                draw_editor(
                    cr,
                    &engine,
                    width,
                    height,
                    &sender_for_draw,
                    h_sb_hovered_for_draw.get(),
                    h_sb_drag_for_draw.get(),
                );
            });

        // Motion controller: drives h scrollbar hover colour changes.
        {
            let sender_motion = sender.input_sender().clone();
            let sender_leave = sender.input_sender().clone();
            let mc = gtk4::EventControllerMotion::new();
            mc.connect_motion(move |_, x, y| {
                sender_motion.send(Msg::MouseMove { x, y }).ok();
            });
            // When the pointer leaves the drawing area, clear hover state.
            mc.connect_leave(move |_| {
                sender_leave.send(Msg::MouseMove { x: -1.0, y: -1.0 }).ok();
            });
            widgets.drawing_area.add_controller(mc);
        }

        // Ensure drawing area has focus on startup
        widgets.drawing_area.grab_focus();

        // Poll for background search results every 50 ms.
        let sender_for_poll = sender.input_sender().clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            sender_for_poll.send(Msg::SearchPollTick).ok();
            gtk4::glib::ControlFlow::Continue
        });

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
                // Handle Ctrl-Shift-V paste (sent as synthetic "PasteClipboard" key):
                // do async GDK clipboard read → ClipboardPasteToInput
                if key_name == "PasteClipboard" {
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
                    return;
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

                let (action, prev_tab) = {
                    let mut engine = self.engine.borrow_mut();
                    let prev = engine.active_tab;
                    let a = engine.handle_key(&key_name, unicode, ctrl);
                    (a, prev)
                };

                match action {
                    EngineAction::Quit | EngineAction::SaveQuit => {
                        // Save current file position before exiting
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
                        engine.collect_session_open_files();
                        if let Some(ref root) = engine.workspace_root.clone() {
                            engine.save_session_for_workspace(root);
                        }
                        let _ = engine.session.save();
                        engine.lsp_shutdown();
                        drop(engine);
                        std::process::exit(0);
                    }
                    EngineAction::OpenFile(path) => {
                        let mut engine = self.engine.borrow_mut();
                        // :e and other explicit commands always open as permanent
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
                        sender.input(Msg::NewTerminalTab);
                    }
                    EngineAction::OpenFolderDialog => {
                        sender.input(Msg::OpenFolderDialog);
                    }
                    EngineAction::OpenWorkspaceDialog => {
                        sender.input(Msg::OpenWorkspaceDialog);
                    }
                    EngineAction::SaveWorkspaceAsDialog => {
                        sender.input(Msg::SaveWorkspaceAsDialog);
                    }
                    EngineAction::OpenRecentDialog => {
                        sender.input(Msg::OpenRecentDialog);
                    }
                    EngineAction::QuitWithUnsaved => {
                        sender.input(Msg::ShowQuitConfirm);
                    }
                    EngineAction::ToggleSidebar => {
                        sender.input(Msg::ToggleSidebar);
                    }
                    EngineAction::None | EngineAction::Error => {}
                }

                // Process macro playback queue if active
                loop {
                    let (has_more, action) = {
                        let mut engine = self.engine.borrow_mut();
                        engine.advance_macro_playback()
                    };

                    // Handle actions from macro playback
                    match action {
                        EngineAction::Quit | EngineAction::SaveQuit => {
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
                            engine.collect_session_open_files();
                            if let Some(ref root) = engine.workspace_root.clone() {
                                engine.save_session_for_workspace(root);
                            }
                            let _ = engine.session.save();
                            engine.lsp_shutdown();
                            drop(engine);
                            std::process::exit(0);
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
                            sender.input(Msg::ToggleTerminal);
                        }
                        EngineAction::OpenFolderDialog
                        | EngineAction::OpenWorkspaceDialog
                        | EngineAction::SaveWorkspaceAsDialog
                        | EngineAction::OpenRecentDialog => {
                            // Dialog actions don't fire during macro playback
                        }
                        EngineAction::QuitWithUnsaved => {
                            sender.input(Msg::ShowQuitConfirm);
                        }
                        EngineAction::ToggleSidebar => {
                            sender.input(Msg::ToggleSidebar);
                        }
                        EngineAction::None | EngineAction::Error => {}
                    }

                    if !has_more {
                        break;
                    }
                }

                // Reveal the active file in the sidebar when tab changed (gt/gT/:tabn/:tabp)
                {
                    let engine = self.engine.borrow();
                    if engine.active_tab != prev_tab {
                        let file_path = engine.file_path().cloned();
                        drop(engine);
                        if let Some(path) = file_path {
                            if let Some(ref tree) = *self.file_tree_view.borrow() {
                                highlight_file_in_tree(tree, &path);
                            }
                        }
                    }
                }

                // Sync the unnamed register to the system clipboard if it changed.
                // The comparison is O(1); actual write is deferred to the background thread.
                self.sync_plus_register_to_clipboard();

                // If a yank just happened, schedule a 200 ms one-shot to clear the highlight.
                if self.engine.borrow().yank_highlight.is_some() {
                    let s = sender.clone();
                    gtk4::glib::timeout_add_local_once(
                        std::time::Duration::from_millis(200),
                        move || {
                            s.input(Msg::ClearYankHighlight);
                        },
                    );
                }

                self.redraw = !self.redraw;
            }
            Msg::ClearYankHighlight => {
                self.engine.borrow_mut().clear_yank_highlight();
                self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
            }
            Msg::MouseClick {
                x,
                y,
                width,
                height,
            } => {
                // Clicking in the editor clears debug sidebar focus.
                self.engine.borrow_mut().dap_sidebar_has_focus = false;
                // Check if click lands in the terminal panel before general handling.
                // Layout (bottom to top): status | toolbar | terminal | quickfix | DAP | editor
                let in_terminal = if self.cached_line_height > 0.0 {
                    let engine = self.engine.borrow();
                    if engine.terminal_open || engine.bottom_panel_open {
                        let term_px = (engine.session.terminal_panel_rows as f64 + 1.0)
                            * self.cached_line_height;
                        let status_h = 2.0 * self.cached_line_height;
                        let toolbar_px = if engine.debug_toolbar_visible {
                            self.cached_line_height
                        } else {
                            0.0
                        };
                        let term_y = height - status_h - toolbar_px - term_px;
                        if y >= term_y {
                            Some((term_y, y >= term_y + self.cached_line_height))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((term_y, in_content)) = in_terminal {
                    // Click on the tab bar row: switch active bottom panel tab.
                    if !in_content {
                        // Determine which tab was clicked by measuring label widths.
                        // Labels mirror draw_bottom_panel_tabs: Terminal then Debug Output.
                        // Use a simple fixed-width estimate (label char count × char_width).
                        let cw = self.cached_char_width.max(1.0);
                        let terminal_label = "  \u{f489}  Terminal  ";
                        let debug_label = "  \u{f188}  Debug Output  ";
                        let terminal_w = terminal_label.chars().count() as f64 * cw;
                        let tab_x = x - 4.0; // offset matches draw_bottom_panel_tabs cursor_x start
                        let new_kind = if tab_x < terminal_w {
                            render::BottomPanelKind::Terminal
                        } else if tab_x < terminal_w + 8.0 + debug_label.chars().count() as f64 * cw
                        {
                            render::BottomPanelKind::DebugOutput
                        } else {
                            self.engine.borrow().bottom_panel_kind.clone()
                        };
                        self.engine.borrow_mut().bottom_panel_kind = new_kind;
                        sender.input(Msg::Resize); // triggers redraw
                        return;
                    }
                    self.engine.borrow_mut().terminal_has_focus = true;
                    if in_content {
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
                                let row = ((y - term_y - self.cached_line_height)
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
                    self.redraw = true;
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
                            self.redraw = !self.redraw;
                            return; // consume click; don't send it to the editor
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
                            handle_mouse_click(&mut engine, x, y, width, height);
                        }

                        // Reveal the active file in the sidebar tree (e.g. after tab click)
                        let file_path = engine.file_path().cloned();
                        drop(engine);
                        if let Some(path) = file_path {
                            if let Some(ref tree) = *self.file_tree_view.borrow() {
                                highlight_file_in_tree(tree, &path);
                            }
                        }
                        self.redraw = !self.redraw;
                    }
                }
            }
            Msg::CtrlMouseClick {
                x,
                y,
                width,
                height,
            } => {
                let mut engine = self.engine.borrow_mut();
                if let ClickTarget::BufferPos(_, line, col) =
                    pixel_to_click_target(&mut engine, x, y, width, height)
                {
                    engine.add_cursor_at_pos(line, col);
                }
                self.redraw = !self.redraw;
            }
            Msg::MouseDoubleClick {
                x,
                y,
                width,
                height,
            } => {
                let mut engine = self.engine.borrow_mut();
                handle_mouse_double_click(&mut engine, x, y, width, height);
                self.redraw = !self.redraw;
            }
            Msg::MouseDrag {
                x,
                y,
                width,
                height,
            } => {
                // H scrollbar thumb drag — convert pointer delta to scroll_left.
                if let Some(ref state) = self.h_sb_dragging {
                    if state.px_per_col > 0.0 {
                        let delta_cols =
                            ((x - state.drag_start_x) / state.px_per_col).round() as isize;
                        let new_left =
                            (state.scroll_left_at_start as isize + delta_cols).max(0) as usize;
                        self.engine
                            .borrow_mut()
                            .set_scroll_left_for_window(state.window_id, new_left);
                        self.redraw = !self.redraw;
                    }
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
                        self.redraw = true;
                    }
                // Terminal panel resize drag.
                } else if self.terminal_resize_dragging {
                    if self.cached_line_height > 0.0 {
                        let status_h = 2.0 * self.cached_line_height;
                        let available = (height - y - status_h).max(0.0);
                        let new_rows = ((available / self.cached_line_height) as u16)
                            .saturating_sub(1)
                            .clamp(5, 30);
                        self.engine.borrow_mut().session.terminal_panel_rows = new_rows;
                        self.redraw = true;
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
                        let term_px = (self.engine.borrow().session.terminal_panel_rows as f64
                            + 1.0)
                            * self.cached_line_height;
                        let status_h = 2.0 * self.cached_line_height;
                        let toolbar_px = if self.engine.borrow().debug_toolbar_visible {
                            self.cached_line_height
                        } else {
                            0.0
                        };
                        let term_y = height - status_h - toolbar_px - term_px;
                        let content_y = term_y + self.cached_line_height;
                        let content_h = term_px - self.cached_line_height;
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
                    self.redraw = true;
                } else {
                    // Check if drag is in the terminal content area (text selection).
                    let in_terminal = if self.cached_line_height > 0.0 {
                        let engine = self.engine.borrow();
                        if engine.terminal_open || engine.bottom_panel_open {
                            let term_px = (engine.session.terminal_panel_rows as f64 + 1.0)
                                * self.cached_line_height;
                            let status_h = 2.0 * self.cached_line_height;
                            let toolbar_px = if engine.debug_toolbar_visible {
                                self.cached_line_height
                            } else {
                                0.0
                            };
                            let term_y = height - status_h - toolbar_px - term_px;
                            if y >= term_y + self.cached_line_height {
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
                        let row = ((y - term_y - self.cached_line_height) / self.cached_line_height)
                            as u16;
                        let col = (x / self.cached_char_width.max(1.0)) as u16;
                        if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                            if let Some(ref mut sel) = term.selection {
                                sel.end_row = row;
                                sel.end_col = col;
                            }
                        }
                        self.redraw = true;
                    } else {
                        let mut engine = self.engine.borrow_mut();
                        handle_mouse_drag(&mut engine, x, y, width, height);
                        self.redraw = !self.redraw;
                    }
                }
            }
            Msg::MouseUp => {
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
                let mut engine = self.engine.borrow_mut();
                engine.mouse_drag_active = false;
                self.redraw = !self.redraw;
            }
            Msg::MouseMove { x, y } => {
                // Update hover state for Cairo h scrollbars; redraw only if it changed.
                let lh = self.cached_line_height;
                let cw = self.cached_char_width;
                let (da_w, da_h) = if let Some(da) = self.drawing_area.borrow().as_ref() {
                    (da.width() as f64, da.height() as f64)
                } else {
                    return;
                };
                let engine = self.engine.borrow();
                let rects = compute_editor_window_rects(&engine, da_w, da_h, lh);
                let now_hovered = h_scrollbar_hit_test(&engine, x, y, &rects, cw, lh).is_some();
                drop(engine);
                if now_hovered != self.h_sb_hovered {
                    self.h_sb_hovered = now_hovered;
                    self.h_sb_hovered_cell.set(now_hovered);
                    self.redraw = !self.redraw;
                }
            }
            Msg::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.redraw = !self.redraw;

                // Directly control the revealer and panel visibility.
                let show = self.sidebar_visible;
                let p = self.active_panel;
                if let Some(ref r) = *self.sidebar_revealer.borrow() {
                    r.set_reveal_child(show);
                }
                for (which, panel_ref) in [
                    (SidebarPanel::Explorer, &self.explorer_panel_box),
                    (SidebarPanel::Search, &self.search_panel_box),
                    (SidebarPanel::Debug, &self.debug_panel_box),
                    (SidebarPanel::Git, &self.git_panel_box),
                    (SidebarPanel::Settings, &self.settings_panel_box),
                ] {
                    if let Some(ref b) = *panel_ref.borrow() {
                        b.set_visible(show && p == which);
                    }
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
                } else {
                    // Different panel - switch and ensure visible
                    self.active_panel = panel;
                    self.sidebar_visible = true;
                    // Refresh SC data when switching to the Git panel
                    if self.active_panel == SidebarPanel::Git {
                        let mut engine = self.engine.borrow_mut();
                        engine.sc_refresh();
                        engine.sc_has_focus = true;
                        drop(engine);
                        if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                            da.grab_focus();
                        }
                    }
                }
                // Directly set visibility on the revealer and each panel box.
                let p = self.active_panel;
                let show_sidebar = self.sidebar_visible;
                if let Some(ref r) = *self.sidebar_revealer.borrow() {
                    r.set_reveal_child(show_sidebar);
                }
                for (which, panel_ref) in [
                    (SidebarPanel::Explorer, &self.explorer_panel_box),
                    (SidebarPanel::Search, &self.search_panel_box),
                    (SidebarPanel::Debug, &self.debug_panel_box),
                    (SidebarPanel::Git, &self.git_panel_box),
                    (SidebarPanel::Settings, &self.settings_panel_box),
                ] {
                    if let Some(ref b) = *panel_ref.borrow() {
                        b.set_visible(show_sidebar && p == which);
                    }
                }
                self.redraw = !self.redraw;
            }
            Msg::OpenFileFromSidebar(path) => {
                let mut engine = self.engine.borrow_mut();
                // Open in a new tab, or switch to the existing tab that shows this file.
                engine.open_file_in_tab(&path);
                drop(engine);
                if let Some(ref tree) = *self.file_tree_view.borrow() {
                    highlight_file_in_tree(tree, &path);
                }
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }
                self.tree_has_focus = false;
                self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
            }
            Msg::CreateFile(parent_dir, name) => {
                // Validate name
                if let Err(msg) = validate_name(&name) {
                    self.engine.borrow_mut().message = msg;
                    self.redraw = !self.redraw;
                    return;
                }

                let file_path = parent_dir.join(&name);

                // Check if already exists
                if file_path.exists() {
                    self.engine.borrow_mut().message = format!("'{}' already exists", name);
                    self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
            }
            Msg::CreateFolder(parent_dir, name) => {
                // Validate name
                if let Err(msg) = validate_name(&name) {
                    self.engine.borrow_mut().message = msg;
                    self.redraw = !self.redraw;
                    return;
                }

                let folder_path = parent_dir.join(&name);

                // Check if already exists
                if folder_path.exists() {
                    self.engine.borrow_mut().message = format!("'{}' already exists", name);
                    self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
            }
            Msg::ConfirmDeletePath(path) => {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let item_type = if path.is_dir() { "folder" } else { "file" };
                let dialog = gtk4::Dialog::with_buttons(
                    Some("Confirm Delete"),
                    None::<&gtk4::Window>,
                    gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
                    &[
                        ("Delete", gtk4::ResponseType::Accept),
                        ("Cancel", gtk4::ResponseType::Cancel),
                    ],
                );
                let label =
                    gtk4::Label::new(Some(&format!("Delete {} '{}'?", item_type, filename)));
                label.set_margin_all(12);
                dialog.content_area().append(&label);
                let s = sender.clone();
                dialog.connect_response(move |dlg, resp| {
                    if resp == gtk4::ResponseType::Accept {
                        s.input(Msg::DeletePath(path.clone()));
                    }
                    dlg.close();
                });
                dialog.present();
            }
            Msg::DeletePath(path) => {
                // Get filename for message
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let is_dir = path.is_dir();
                let item_type = if is_dir { "folder" } else { "file" };

                // Check if path exists
                if !path.exists() {
                    self.engine.borrow_mut().message = format!("'{}' does not exist", filename);
                    self.redraw = !self.redraw;
                    return;
                }

                // Attempt deletion
                let result = if is_dir {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };

                match result {
                    Ok(_) => {
                        self.engine.borrow_mut().message =
                            format!("Deleted {}: '{}'", item_type, filename);

                        // If deleted file was open, close its buffer
                        // Find buffer by path and delete it
                        if !is_dir {
                            let path_str = path.to_string_lossy();
                            let mut engine = self.engine.borrow_mut();
                            if let Some(buffer_id) = engine.buffer_manager.find_by_path(&path_str) {
                                // Delete the buffer (force=true since file is gone anyway)
                                let _ = engine.delete_buffer(buffer_id, true);
                            }
                        }

                        sender.input(Msg::RefreshFileTree);
                    }
                    Err(e) => {
                        let msg = match e.kind() {
                            std::io::ErrorKind::PermissionDenied => {
                                format!("Permission denied: '{}'", filename)
                            }
                            std::io::ErrorKind::NotFound => format!("'{}' not found", filename),
                            _ => format!("Error deleting '{}': {}", filename, e),
                        };
                        self.engine.borrow_mut().message = msg;
                    }
                }
                self.redraw = !self.redraw;
            }
            Msg::RefreshFileTree => {
                if let Some(ref store) = self.tree_store {
                    match std::env::current_dir() {
                        Ok(cwd) => {
                            store.clear();
                            build_file_tree_with_root(store, &cwd);
                            if let Some(ref tv) = *self.file_tree_view.borrow() {
                                tv.expand_row(&gtk4::TreePath::from_indices(&[0]), false);
                            }
                        }
                        Err(e) => {
                            self.engine.borrow_mut().message =
                                format!("Error refreshing tree: {}", e);
                        }
                    }
                }
                self.redraw = !self.redraw;
            }
            Msg::FocusExplorer => {
                // Ensure sidebar is visible and explorer is active
                self.sidebar_visible = true;
                self.active_panel = SidebarPanel::Explorer;
                self.tree_has_focus = true;

                // Grab focus on tree view
                if let Some(ref tree) = *self.file_tree_view.borrow() {
                    tree.grab_focus();
                }

                self.redraw = !self.redraw;
            }
            Msg::ToggleFocusExplorer => {
                if self.tree_has_focus {
                    // Already focused — return to editor
                    self.tree_has_focus = false;
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                } else {
                    self.sidebar_visible = true;
                    self.active_panel = SidebarPanel::Explorer;
                    self.tree_has_focus = true;
                    if let Some(ref tree) = *self.file_tree_view.borrow() {
                        tree.grab_focus();
                    }
                }
                self.redraw = !self.redraw;
            }
            Msg::ToggleFocusSearch => {
                // Same pattern as ToggleFocusExplorer: toggle between showing the search
                // panel and returning to the editor.  When "exiting" we keep the sidebar
                // visible (don't touch sidebar_visible) to avoid a white-area artifact
                // from the Revealer animation — Ctrl+B closes the sidebar entirely.
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
                self.redraw = !self.redraw;
            }
            Msg::FocusEditor => {
                self.tree_has_focus = false;
                self.engine.borrow_mut().dap_sidebar_has_focus = false;

                // Grab focus on drawing area
                if let Some(ref drawing) = *self.drawing_area.borrow() {
                    drawing.grab_focus();
                }

                self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
            }
            Msg::HorizontalScrollbarChanged { window_id, value } => {
                let mut engine = self.engine.borrow_mut();
                engine.set_scroll_left_for_window(window_id, value.round() as usize);
                drop(engine);
                self.redraw = !self.redraw;
            }
            Msg::MouseScroll { delta_x, delta_y } => {
                let mut engine = self.engine.borrow_mut();
                if delta_y.abs() > 0.01 {
                    let lines = engine.buffer().len_lines().saturating_sub(1);
                    let scroll_amount = (delta_y * 3.0).round() as isize;
                    let st = engine.view().scroll_top as isize;
                    let new_top = (st + scroll_amount).clamp(0, lines as isize) as usize;
                    engine.set_scroll_top(new_top);
                    engine.ensure_cursor_visible();
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
                self.redraw = !self.redraw;
            }
            Msg::CacheFontMetrics(line_height, char_width) => {
                let old_char_width = self.cached_char_width;
                self.cached_line_height = line_height;
                self.cached_char_width = char_width;
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
                self.redraw = !self.redraw;
            }
            Msg::SettingsFileChanged => {
                // Use load_with_validation (not load) to avoid writing back to the file,
                // which would trigger the watcher again and cause an infinite reload loop.
                // Silently ignore errors — the file may be mid-write.
                if let Ok(new_settings) = core::settings::Settings::load_with_validation() {
                    let mut engine = self.engine.borrow_mut();
                    engine.settings = new_settings;
                    engine.message = "Settings reloaded".to_string();
                    drop(engine);

                    // Force redraw to apply new font/line number settings
                    if let Some(drawing_area) = self.drawing_area.borrow().as_ref() {
                        drawing_area.queue_draw();
                    }
                    self.redraw = !self.redraw;
                }
            }
            Msg::ToggleFindDialog => {
                self.find_dialog_visible = !self.find_dialog_visible;
                self.redraw = !self.redraw;
            }
            Msg::FindTextChanged(text) => {
                self.find_text = text.clone();
                let mut engine = self.engine.borrow_mut();
                engine.search_query = text;
                engine.run_search();
                self.redraw = !self.redraw;
            }
            Msg::ReplaceTextChanged(text) => {
                self.replace_text = text;
            }
            Msg::FindNext => {
                let mut engine = self.engine.borrow_mut();
                engine.search_next();
                self.redraw = !self.redraw;
            }
            Msg::FindPrevious => {
                let mut engine = self.engine.borrow_mut();
                engine.search_prev();
                self.redraw = !self.redraw;
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

                self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
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

                self.redraw = !self.redraw;
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
                if let Some(ref sb) = *self.sidebar_inner_box.borrow() {
                    let w = sb.width_request();
                    self.engine.borrow_mut().session.sidebar_width = w;
                    let _ = self.engine.borrow().session.save();
                }
            }
            Msg::ProjectSearchQueryChanged(q) => {
                self.engine.borrow_mut().project_search_query = q;
            }
            Msg::ProjectSearchToggleCase => {
                self.engine.borrow_mut().toggle_project_search_case();
                self.redraw = true;
            }
            Msg::ProjectSearchToggleWholeWord => {
                self.engine.borrow_mut().toggle_project_search_whole_word();
                self.redraw = true;
            }
            Msg::ProjectSearchToggleRegex => {
                self.engine.borrow_mut().toggle_project_search_regex();
                self.redraw = true;
            }
            Msg::ProjectReplaceTextChanged(t) => {
                self.engine.borrow_mut().project_replace_text = t;
            }
            Msg::ProjectReplaceAll => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                self.engine.borrow_mut().start_project_replace(cwd);
                let status = self.engine.borrow().message.clone();
                self.project_search_status = status;
                self.redraw = true;
            }
            Msg::ProjectSearchSubmit => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                self.engine.borrow_mut().start_project_search(cwd);
                let status = self.engine.borrow().message.clone();
                self.project_search_status = status;
                self.redraw = true;
            }
            Msg::SearchPollTick => {
                if self.engine.borrow_mut().poll_project_search() {
                    let status = self.engine.borrow().message.clone();
                    self.project_search_status = status;
                    let s = self.sender.clone();
                    self.rebuild_search_results(&s);
                    self.redraw = true;
                }
                if self.engine.borrow_mut().poll_project_replace() {
                    let status = self.engine.borrow().message.clone();
                    self.project_search_status = status;
                    let s = self.sender.clone();
                    self.rebuild_search_results(&s);
                    self.redraw = true;
                }
                // LSP: flush debounced didChange notifications and poll for events
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.lsp_flush_changes();
                    if engine.poll_lsp() {
                        self.redraw = true;
                    }
                }
                // Terminal: drain PTY output and refresh display if needed
                if self.engine.borrow_mut().poll_terminal() {
                    self.redraw = true;
                }
                // DAP: drain adapter events (breakpoint hits, stops, output)
                {
                    let mut engine = self.engine.borrow_mut();
                    if engine.poll_dap() {
                        self.redraw = true;
                    }
                    // Auto-switch to Debug sidebar when a session starts.
                    if engine.dap_wants_sidebar {
                        engine.dap_wants_sidebar = false;
                        self.active_panel = SidebarPanel::Debug;
                        self.sidebar_visible = true;
                        self.redraw = true;
                    }
                }
                // Explicitly redraw the debug sidebar if it's active so the
                // Run/Stop button text and section data stay in sync.
                if self.active_panel == SidebarPanel::Debug {
                    if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                }
                // Auto-refresh SC panel every 2s to pick up external git changes.
                if self.sidebar_visible
                    && self.active_panel == SidebarPanel::Git
                    && self.last_sc_refresh.elapsed() >= std::time::Duration::from_secs(2)
                {
                    self.engine.borrow_mut().sc_refresh();
                    self.last_sc_refresh = std::time::Instant::now();
                    if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                        da.queue_draw();
                    }
                    self.redraw = true;
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
                self.redraw = true;
            }
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
                self.redraw = !self.redraw;
            }
            Msg::MoveFile(src, dest_dir) => {
                let name = src
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let result = self.engine.borrow_mut().move_file(&src, &dest_dir);
                match result {
                    Ok(()) => {
                        self.engine.borrow_mut().message =
                            format!("Moved '{}' to '{}'", name, dest_dir.display());
                        // Refresh tree inline so we can highlight the moved file
                        if let Some(ref store) = self.tree_store {
                            if let Ok(cwd) = std::env::current_dir() {
                                store.clear();
                                build_file_tree_with_root(store, &cwd);
                            }
                        }
                        let new_path = dest_dir.join(&name);
                        if let Some(ref tree) = *self.file_tree_view.borrow() {
                            tree.expand_row(&gtk4::TreePath::from_indices(&[0]), false);
                            highlight_file_in_tree(tree, &new_path);
                        }
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message = e;
                    }
                }
                self.redraw = !self.redraw;
            }
            Msg::CopyPath(path) => {
                let path_str = path.to_string_lossy().to_string();
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&path_str);
                    self.engine.borrow_mut().message = format!("Copied: {}", path_str);
                }
                self.redraw = !self.redraw;
            }
            Msg::SelectForDiff(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                self.diff_selected_file = Some(path);
                self.engine.borrow_mut().message = format!(
                    "Selected '{}' for diff. Right-click another file → Diff with…",
                    name
                );
                self.redraw = !self.redraw;
            }
            Msg::DiffWithSelected(right_path) => {
                if let Some(left_path) = self.diff_selected_file.take() {
                    // Open left file and mark it as diff left side
                    self.engine.borrow_mut().open_file_in_tab(&left_path);
                    self.engine.borrow_mut().cmd_diffthis();
                    // Open right file in vsplit + activate diff
                    self.engine.borrow_mut().cmd_diffsplit(&right_path);
                } else {
                    self.engine.borrow_mut().message =
                        "No file selected for diff. Right-click a file → Select for Diff first."
                            .to_string();
                }
                self.redraw = !self.redraw;
            }
            Msg::ClipboardPasteToInput { text } => {
                // GDK clipboard text arrived for Ctrl-Shift-V paste into command/search/insert.
                use core::Mode;
                let mut engine = self.engine.borrow_mut();
                match engine.mode {
                    Mode::Command | Mode::Search => {
                        engine.paste_text_to_input(&text);
                    }
                    Mode::Insert => {
                        for ch in text.chars() {
                            if ch == '\n' || ch == '\r' {
                                engine.handle_key("Return", None, false);
                            } else {
                                engine.handle_key("", Some(ch), false);
                            }
                        }
                    }
                    _ => {}
                }
                self.redraw = !self.redraw;
            }
            Msg::WindowClosing { width, height } => {
                let mut engine = self.engine.borrow_mut();
                engine.session.window.width = width;
                engine.session.window.height = height;
                engine.session.explorer_visible = self.sidebar_visible;
                // Save sidebar width on close too
                if let Some(ref sb) = *self.sidebar_inner_box.borrow() {
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
            Msg::ToggleTerminal => {
                let needs_new_tab = {
                    let engine = self.engine.borrow();
                    (!engine.terminal_open || !engine.terminal_has_focus)
                        && engine.terminal_panes.is_empty()
                };
                if needs_new_tab {
                    // Use the actual drawing area width so the PTY matches the visible panel.
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
                    let rows = self.engine.borrow().session.terminal_panel_rows;
                    self.engine.borrow_mut().terminal_new_tab(cols, rows);
                } else {
                    self.engine.borrow_mut().toggle_terminal();
                }
                self.redraw = true;
            }
            Msg::NewTerminalTab => {
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
                let rows = self.engine.borrow().session.terminal_panel_rows;
                self.engine.borrow_mut().terminal_new_tab(cols, rows);
                self.redraw = true;
            }
            Msg::TerminalSwitchTab(idx) => {
                self.engine.borrow_mut().terminal_switch_tab(idx);
                self.redraw = true;
            }
            Msg::TerminalCloseActiveTab => {
                self.engine.borrow_mut().terminal_close_active_tab();
                self.redraw = true;
            }
            Msg::TerminalKill => {
                self.engine.borrow_mut().terminal_close_active_tab();
                self.redraw = true;
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
                self.redraw = true;
            }
            Msg::TerminalSplitFocus(idx) => {
                let mut engine = self.engine.borrow_mut();
                if engine.terminal_split && idx < engine.terminal_panes.len() {
                    engine.terminal_active = idx;
                }
                self.redraw = true;
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
                self.redraw = true;
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
                self.redraw = true;
            }
            Msg::TerminalMouseDrag { row, col } => {
                if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                    if let Some(ref mut sel) = term.selection {
                        sel.end_row = row;
                        sel.end_col = col;
                    }
                }
                self.redraw = true;
            }
            Msg::TerminalMouseUp => {
                // Selection stays in place; user can now copy
                self.redraw = true;
            }
            Msg::TerminalFindOpen => {
                self.engine.borrow_mut().terminal_find_open();
                self.redraw = true;
            }
            Msg::TerminalFindClose => {
                self.engine.borrow_mut().terminal_find_close();
                self.redraw = true;
            }
            Msg::TerminalFindChar(ch) => {
                self.engine.borrow_mut().terminal_find_char(ch);
                self.redraw = true;
            }
            Msg::TerminalFindBackspace => {
                self.engine.borrow_mut().terminal_find_backspace();
                self.redraw = true;
            }
            Msg::TerminalFindNext => {
                self.engine.borrow_mut().terminal_find_next();
                self.redraw = true;
            }
            Msg::TerminalFindPrev => {
                self.engine.borrow_mut().terminal_find_prev();
                self.redraw = true;
            }
            Msg::ToggleMenuBar => {
                // In GTK the menu bar is always on (it's our title bar).
                // Just queue a redraw so the menu labels re-render.
                if let Some(ref da) = *self.menu_bar_da.borrow() {
                    da.queue_draw();
                }
                self.redraw = true;
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
                self.redraw = true;
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
                self.redraw = true;
            }
            Msg::MenuActivateItem(menu_idx, item_idx, action) => {
                // Intercept dialog actions that need GTK-side handling
                match action.as_str() {
                    "open_file_dialog" => {
                        sender.input(Msg::OpenFileDialog);
                    }
                    "open_folder_dialog" => {
                        sender.input(Msg::OpenFolderDialog);
                    }
                    "open_workspace_dialog" => {
                        sender.input(Msg::OpenWorkspaceDialog);
                    }
                    "save_workspace_as_dialog" => {
                        sender.input(Msg::SaveWorkspaceAsDialog);
                    }
                    "openrecent" => {
                        sender.input(Msg::OpenRecentDialog);
                    }
                    "find" => {
                        // Open the GTK find/replace dialog (same as Ctrl+F).
                        self.engine.borrow_mut().close_menu();
                        sender.input(Msg::ToggleFindDialog);
                    }
                    "quit_menu" => {
                        // Close the menu first, then quit (or show dialog if unsaved).
                        self.engine.borrow_mut().close_menu();
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
                self.redraw = true;
            }
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
                self.redraw = !self.redraw;
            }
            Msg::DebugSidebarKey(key_name, ctrl) => {
                let mut engine = self.engine.borrow_mut();
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
                // Map GTK key names to engine key names
                let mapped = match key_name.as_str() {
                    "Return" | "KP_Enter" => "Return",
                    "Escape" => "Escape",
                    "Tab" | "ISO_Left_Tab" => "Tab",
                    "Down" => "Down",
                    "Up" => "Up",
                    "Home" => "Home",
                    "End" => "End",
                    "Page_Down" | "KP_Page_Down" => "PageDown",
                    "Page_Up" | "KP_Page_Up" => "PageUp",
                    "space" => " ",
                    "j" => "j",
                    "k" => "k",
                    "g" => "g",
                    "G" => "G",
                    "x" => "x",
                    "d" => "d",
                    "q" => "q",
                    _ => "",
                };
                if !mapped.is_empty() {
                    engine.handle_debug_sidebar_key(mapped, ctrl);
                }
                let still_focused = engine.dap_sidebar_has_focus;
                drop(engine);

                if !still_focused {
                    // Return focus to the editor
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                }
                if let Some(ref da) = *self.debug_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.redraw = !self.redraw;
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
                self.redraw = !self.redraw;
            }
            Msg::ScSidebarClick(x_click, y) => {
                let lh = self.cached_line_height;
                if lh <= 0.0 {
                    return;
                }
                let row_idx = (y / lh) as usize;
                self.tree_has_focus = false;
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.grab_focus();
                }
                // Update selection/focus in one borrow scope.
                // Returns: Some("Return") = open file, Some("Tab") = toggle expand, None = no-op
                let action: Option<&'static str> = {
                    let mut engine = self.engine.borrow_mut();
                    engine.sc_has_focus = true;
                    if row_idx == 0 {
                        // Panel header — no-op
                        None
                    } else if row_idx == 1 {
                        // Commit input row
                        engine.sc_commit_input_active = true;
                        None
                    } else if row_idx == 2 {
                        // Button row: Commit (~50%), Push/Pull/Sync (~17% each, icon-only).
                        if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                            let da_w = da.width() as f64;
                            if da_w > 0.0 {
                                let commit_w = da_w / 2.0;
                                let btn_idx = if x_click < commit_w {
                                    0
                                } else {
                                    let icon_w = (da_w - commit_w) / 3.0;
                                    ((1.0 + (x_click - commit_w) / icon_w) as usize).min(3)
                                };
                                engine.sc_activate_button(btn_idx);
                            }
                        }
                        None
                    } else {
                        // GTK does not render a "(no changes)" hint for empty
                        // expanded sections, so empty_section_hint = false.
                        match engine.sc_visual_row_to_flat(row_idx, false) {
                            Some((flat_idx, is_header)) => {
                                engine.sc_selected = flat_idx;
                                if is_header {
                                    Some("Tab")
                                } else {
                                    Some("Return")
                                }
                            }
                            None => None,
                        }
                    }
                };
                if let Some(key) = action {
                    self.engine.borrow_mut().handle_sc_key(key, false, None);
                    // Click opens the file but keeps panel focus so s/d work immediately.
                    // (Keyboard Enter clears sc_has_focus to return to the editor.)
                    if key == "Return" {
                        let mut engine = self.engine.borrow_mut();
                        engine.sc_has_focus = true;
                        drop(engine);
                        if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                            da.grab_focus();
                        }
                    }
                }
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.redraw = !self.redraw;
            }
            Msg::ScKey(key_name, ctrl) => {
                let mut engine = self.engine.borrow_mut();
                if engine.sc_commit_input_active {
                    // In commit input mode, pass everything through.
                    let (mapped_key, unicode): (&str, Option<char>) = match key_name.as_str() {
                        "Return" | "KP_Enter" => ("Return", None),
                        "Escape" => ("Escape", None),
                        "BackSpace" => ("BackSpace", None),
                        other => {
                            let mut chars = other.chars();
                            if let (Some(ch), None) = (chars.next(), chars.next()) {
                                (other, Some(ch))
                            } else {
                                (other, None)
                            }
                        }
                    };
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
                        _ => "",
                    };
                    if !mapped.is_empty() {
                        engine.handle_sc_key(mapped, ctrl, None);
                    }
                }
                let still_focused = engine.sc_has_focus;
                drop(engine);
                if !still_focused {
                    if let Some(ref drawing) = *self.drawing_area.borrow() {
                        drawing.grab_focus();
                    }
                }
                if let Some(ref da) = *self.git_sidebar_da_ref.borrow() {
                    da.queue_draw();
                }
                self.redraw = !self.redraw;
            }
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
                self.redraw = true;
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
                self.redraw = true;
            }
            Msg::OpenWorkspaceDialog => {
                let engine = self.engine.clone();
                let sender2 = sender.input_sender().clone();
                let dialog = gtk4::FileDialog::new();
                dialog.set_title("Open Workspace");
                let win = self.window.clone();
                dialog.open(Some(&win), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = gtk4::prelude::FileExt::path(&file) {
                            engine.borrow_mut().open_workspace(&path);
                            sender2.send(Msg::RefreshFileTree).ok();
                        }
                    }
                });
                self.redraw = true;
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
                self.redraw = true;
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
                self.redraw = true;
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
    let tab_bar_height = line_height;
    let status_bar_height = line_height * 2.0;
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
        (engine.session.terminal_panel_rows as f64 + 1.0) * line_height
    } else {
        0.0
    };
    let content_bounds = core::WindowRect::new(
        0.0,
        tab_bar_height,
        da_width,
        da_height - tab_bar_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
    );
    let window_rects = engine.calculate_window_rects(content_bounds);

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
        ws.vertical.set_halign(gtk4::Align::Start);
        ws.vertical.set_valign(gtk4::Align::Start);
        ws.vertical
            .set_margin_start(rect.x as i32 + (rect.width - 10.0) as i32);
        ws.vertical.set_margin_top(rect.y as i32);
        ws.vertical
            .set_height_request((rect.height as i32 - 10).max(0));

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
        engine.lsp_shutdown();
        drop(engine);
        std::process::exit(0);
    }

    /// Sync the unnamed `"` register to the system clipboard whenever its content changes
    /// (clipboard=unnamedplus semantics: every yank/cut is auto-copied).
    /// Uses the background arboard thread to avoid blocking GTK's X11 connection.
    fn sync_plus_register_to_clipboard(&mut self) {
        let new_content = self
            .engine
            .borrow()
            .registers
            .get(&'"')
            .filter(|(s, _)| !s.is_empty())
            .map(|(s, _)| s.clone());

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
        let tab_bar_height = line_height;
        let status_bar_height = line_height * 2.0;
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
            (engine.session.terminal_panel_rows as f64 + 1.0) * line_height
        } else {
            0.0
        };

        let content_bounds = WindowRect::new(
            0.0,
            tab_bar_height,
            da_width,
            da_height - tab_bar_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
            // No gap: h-scrollbar overlays the content bottom (VSCode style)
        );

        let window_rects = engine.calculate_window_rects(content_bounds);

        // Remove scrollbars for windows that no longer exist
        scrollbars.retain(|window_id, _| engine.windows.contains_key(window_id));

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

                let indicator_x = rect.x as i32 + (rect.width - 10.0) as i32;
                ws.cursor_indicator.set_margin_start(indicator_x);
                ws.cursor_indicator.set_margin_top(indicator_y as i32);

                // Ensure size stays fixed (defensive coding)
                ws.cursor_indicator.set_width_request(10);
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
        // Vertical scrollbar
        let v_adj = gtk4::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 20.0);
        let vertical = gtk4::Scrollbar::new(gtk4::Orientation::Vertical, Some(&v_adj));
        vertical.set_width_request(10);
        vertical.set_hexpand(false);
        vertical.set_vexpand(false);

        // Cursor indicator
        let cursor_indicator = gtk4::DrawingArea::new();
        cursor_indicator.set_width_request(10);
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

/// Pango font description string for UI panels (menu bar, sidebars, dropdown).
/// Matches VSCode's Linux font stack at 11pt ≈ 13px @ 96 dpi.
const UI_FONT: &str = "Segoe UI, Ubuntu, Droid Sans, Sans 10";

fn draw_editor(
    cr: &Context,
    engine: &Engine,
    width: i32,
    height: i32,
    sender: &relm4::Sender<Msg>,
    h_sb_hovered: bool,
    h_sb_dragging_window: Option<core::WindowId>,
) {
    let theme = Theme::onedark();

    // 1. Background
    let (bg_r, bg_g, bg_b) = theme.background.to_cairo();
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.paint().expect("Invalid cairo surface");

    // 2. Setup Pango
    let pango_ctx = pangocairo::create_context(cr);
    let layout = pango::Layout::new(&pango_ctx);

    // Use configurable font from settings
    let font_str = format!(
        "{} {}",
        engine.settings.font_family, engine.settings.font_size
    );
    let font_desc = FontDescription::from_string(&font_str);
    layout.set_font_description(Some(&font_desc));

    // Derive line height and char width from font metrics
    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;
    let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;

    // Cache font metrics for use in sync_scrollbar
    sender
        .send(Msg::CacheFontMetrics(line_height, char_width))
        .ok();

    // Calculate layout regions
    let tab_bar_height = line_height; // Always show tab bar
    let status_bar_height = line_height * 2.0; // status + command line

    // Reserve space for the quickfix panel when open
    const QUICKFIX_ROWS: usize = 6; // 1 header + 5 result rows
    let qf_px = if engine.quickfix_open && !engine.quickfix_items.is_empty() {
        QUICKFIX_ROWS as f64 * line_height
    } else {
        0.0
    };

    // Reserve space for the bottom panel when open (1 tab-bar row + content rows).
    // Triggered by either a live terminal OR the debug output panel being shown.
    let term_px = if engine.terminal_open || engine.bottom_panel_open {
        (engine.session.terminal_panel_rows as usize + 1) as f64 * line_height
    } else {
        0.0
    };

    let debug_toolbar_px = if engine.debug_toolbar_visible {
        line_height
    } else {
        0.0
    };

    // Calculate window rects for the current tab
    // Note: menu bar is now a separate full-width GTK widget above the drawing area,
    // so we do NOT subtract menu_bar_px from the content bounds here.
    let content_bounds = WindowRect::new(
        0.0,
        tab_bar_height,
        width as f64,
        height as f64 - tab_bar_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
        // No gap reserved: h-scrollbar overlays the bottom of content (VSCode style)
    );
    let window_rects = engine.calculate_window_rects(content_bounds);

    // Build the platform-agnostic screen layout
    let screen = build_screen_layout(engine, &theme, &window_rects, line_height, char_width);

    // 3b. Draw tab bar (at y=0; the menu bar widget is above the drawing_area)
    draw_tab_bar(
        cr,
        &layout,
        &theme,
        &screen.tab_bar,
        width as f64,
        line_height,
        0.0,
    );

    // 4. Draw each window
    for rendered_window in &screen.windows {
        draw_window(
            cr,
            &layout,
            &font_metrics,
            &theme,
            rendered_window,
            char_width,
            line_height,
        );
    }

    // 5. Draw window separators
    draw_window_separators(cr, &window_rects, &theme);

    // 5b. Draw completion popup (on top of everything else)
    draw_completion_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c. Draw hover popup (on top of everything else)
    draw_hover_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5c2. Draw signature-help popup (on top of everything else, shown in insert mode)
    draw_signature_popup(cr, &layout, &screen, &theme, line_height, char_width);

    // 5d. Draw fuzzy file-picker modal (on top of everything else)
    draw_fuzzy_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
        char_width,
    );

    // 5e. Draw live grep modal (on top of everything else)
    draw_live_grep_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
    );

    // 5e2. Draw command palette modal (on top of everything else)
    draw_command_palette_popup(
        cr,
        &layout,
        &screen,
        &theme,
        width as f64,
        height as f64,
        line_height,
    );

    // 5f2. Draw quickfix panel (persistent bottom strip above status bar)
    if qf_px > 0.0 {
        let qf_y = height as f64 - status_bar_height - debug_toolbar_px - qf_px - term_px;
        draw_quickfix_panel(
            cr,
            &layout,
            &screen,
            &theme,
            0.0,
            qf_y,
            width as f64,
            qf_px,
            line_height,
        );
    }

    // 5g. Draw bottom panel (Terminal or Debug Output) with a tab bar.
    if term_px > 0.0 {
        let term_y = height as f64 - status_bar_height - debug_toolbar_px - term_px;
        // Tab bar row (1 line high) at the top of the bottom panel area.
        draw_bottom_panel_tabs(
            cr,
            &layout,
            &screen,
            &theme,
            0.0,
            term_y,
            width as f64,
            line_height,
        );
        match screen.bottom_tabs.active {
            render::BottomPanelKind::Terminal => {
                if let Some(ref term_panel) = screen.bottom_tabs.terminal {
                    draw_terminal_panel(
                        cr,
                        &layout,
                        term_panel,
                        &theme,
                        0.0,
                        term_y + line_height, // skip tab bar row
                        width as f64,
                        term_px - line_height,
                        line_height,
                        char_width,
                        sender,
                    );
                }
            }
            render::BottomPanelKind::DebugOutput => {
                draw_debug_output(
                    cr,
                    &layout,
                    &screen.bottom_tabs.output_lines,
                    &theme,
                    0.0,
                    term_y + line_height,
                    width as f64,
                    term_px - line_height,
                    line_height,
                );
            }
        }
    }

    // 5h. Draw debug toolbar strip if visible (above status bar)
    if let Some(ref toolbar) = screen.debug_toolbar {
        let toolbar_y = height as f64 - status_bar_height - debug_toolbar_px;
        draw_debug_toolbar(
            cr,
            toolbar,
            &theme,
            0.0,
            toolbar_y,
            width as f64,
            line_height,
        );
    }

    // 5i. Draw horizontal scrollbars in Cairo (VSCode-style overlay on window bottom)
    draw_h_scrollbars(
        cr,
        engine,
        &window_rects,
        char_width,
        line_height,
        h_sb_hovered,
        h_sb_dragging_window,
    );

    // 6. Status Line (second-to-last line)
    let status_y = height as f64 - status_bar_height;
    draw_status_line(
        cr,
        &layout,
        &theme,
        &screen.status_left,
        &screen.status_right,
        width as f64,
        status_y,
        line_height,
    );

    // 7. Command Line (last line)
    let cmd_y = status_y + line_height;
    draw_command_line(
        cr,
        &layout,
        &theme,
        &screen.command,
        width as f64,
        cmd_y,
        line_height,
    );
}
// Note: menu dropdown is drawn by the full-window `menu_dropdown_da` overlay,
// not here — this drawing_area's x=0 is offset from the window left by the
// activity bar, so popup_x values would appear at the wrong horizontal position.

/// Compute editor window rects with the same formula used by draw_editor and
/// sync_scrollbar, so event handlers can do hit-testing without duplicating the
/// layout logic.
fn compute_editor_window_rects(
    engine: &Engine,
    da_width: f64,
    da_height: f64,
    line_height: f64,
) -> Vec<(core::WindowId, core::WindowRect)> {
    let tab_bar_height = line_height;
    let status_bar_height = line_height * 2.0;
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
        (engine.session.terminal_panel_rows as f64 + 1.0) * line_height
    } else {
        0.0
    };
    let content_bounds = core::WindowRect::new(
        0.0,
        tab_bar_height,
        da_width,
        da_height - tab_bar_height - status_bar_height - debug_toolbar_px - qf_px - term_px,
    );
    engine.calculate_window_rects(content_bounds)
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

    let max_line_length = buffer_state
        .buffer
        .content
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as f64;

    let v_scrollbar_px = 10.0_f64;
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

/// Draw thin Cairo horizontal scrollbars that overlay the bottom of each editor
/// window (VSCode style). Only shown when content is wider than the viewport.
/// `hovered` — mouse is over any scrollbar track (brightens the thumb).
/// `dragging_window` — window being dragged (shows the active/dragging colour).
fn draw_h_scrollbars(
    cr: &Context,
    engine: &Engine,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
    hovered: bool,
    dragging_window: Option<core::WindowId>,
) {
    for (window_id, rect) in window_rects {
        let Some((track_x, track_y, track_w, sb_height, thumb_x, thumb_w, _, _)) =
            h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        else {
            continue;
        };

        let is_active = dragging_window == Some(*window_id);

        // Track background (slightly darker when hovered/active)
        let track_alpha = if hovered || is_active { 0.35 } else { 0.20 };
        cr.set_source_rgba(0.0, 0.0, 0.0, track_alpha);
        cr.rectangle(track_x, track_y, track_w, sb_height);
        cr.fill().ok();

        // Thumb: brighter on hover, brighter still on active drag
        let thumb_alpha = if is_active {
            0.85
        } else if hovered {
            0.70
        } else {
            0.50
        };
        cr.set_source_rgba(0.65, 0.65, 0.65, thumb_alpha);
        cr.rectangle(thumb_x, track_y, thumb_w, sb_height);
        cr.fill().ok();
    }
}

fn draw_tab_bar(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    tabs: &[TabInfo],
    width: f64,
    line_height: f64,
    y_offset: f64,
) {
    // Tab bar background
    let (r, g, b) = theme.tab_bar_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(0.0, y_offset, width, line_height);
    cr.fill().unwrap();

    // Save current font description so we can restore after rendering previews
    let normal_font = layout.font_description().unwrap_or_default();
    let mut italic_font = normal_font.clone();
    italic_font.set_style(pango::Style::Italic);

    let mut x = 0.0;
    for tab in tabs {
        // Use italic font for preview tabs
        if tab.preview {
            layout.set_font_description(Some(&italic_font));
        } else {
            layout.set_font_description(Some(&normal_font));
        }

        layout.set_text(&tab.name);
        let (tab_width, _) = layout.pixel_size();

        // Tab background
        let bg = if tab.active {
            theme.tab_active_bg
        } else {
            theme.tab_bar_bg
        };
        let (br, bg_g, bb) = bg.to_cairo();
        cr.set_source_rgb(br, bg_g, bb);
        cr.rectangle(x, y_offset, tab_width as f64, line_height);
        cr.fill().unwrap();

        // Tab text — dimmed colours for preview tabs
        cr.move_to(x, y_offset);
        let fg = if tab.preview {
            if tab.active {
                theme.tab_preview_active_fg
            } else {
                theme.tab_preview_inactive_fg
            }
        } else if tab.active {
            theme.tab_active_fg
        } else {
            theme.tab_inactive_fg
        };
        let (fr, fg_g, fb) = fg.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        pangocairo::show_layout(cr, layout);

        x += tab_width as f64 + 2.0;
    }

    // Restore normal font for subsequent rendering
    layout.set_font_description(Some(&normal_font));
}

#[allow(clippy::too_many_arguments)]
fn draw_window(
    cr: &Context,
    layout: &pango::Layout,
    font_metrics: &pango::FontMetrics,
    theme: &Theme,
    rw: &RenderedWindow,
    char_width: f64,
    line_height: f64,
) {
    let rect = &rw.rect;

    // Gutter pixel width
    let gutter_width = rw.gutter_char_width as f64 * char_width;

    // Apply horizontal scroll offset
    let h_scroll_offset = rw.scroll_left as f64 * char_width;
    let text_x_offset = rect.x + gutter_width - h_scroll_offset;

    // Window background
    let bg = if rw.show_active_bg {
        theme.active_background
    } else {
        theme.background
    };
    let (br, bg_g, bb) = bg.to_cairo();
    cr.set_source_rgb(br, bg_g, bb);
    cr.rectangle(rect.x, rect.y, rect.width, rect.height);
    cr.fill().unwrap();

    // Diff / DAP stopped-line background (drawn before selection so selection is on top)
    for (view_idx, rl) in rw.lines.iter().enumerate() {
        let y = rect.y + view_idx as f64 * line_height;
        let bg_color = if rl.is_dap_current {
            Some(theme.dap_stopped_bg)
        } else if let Some(diff_status) = rl.diff_status {
            use crate::core::engine::DiffLine;
            match diff_status {
                DiffLine::Added => Some(theme.diff_added_bg),
                DiffLine::Removed => Some(theme.diff_removed_bg),
                DiffLine::Same => None,
            }
        } else {
            None
        };
        if let Some(color) = bg_color {
            let (dr, dg, db) = color.to_cairo();
            cr.set_source_rgb(dr, dg, db);
            cr.rectangle(rect.x, y, rect.width, line_height);
            cr.fill().unwrap();
        }
    }

    // Visual selection highlight (drawn before text so text renders on top)
    if let Some(sel) = &rw.selection {
        draw_visual_selection(
            cr,
            layout,
            sel,
            &rw.lines,
            rect,
            line_height,
            rw.scroll_top,
            text_x_offset,
            theme.selection,
            theme.selection_alpha,
        );
    }

    // Yank highlight (brief flash after yank)
    if let Some(yh) = &rw.yank_highlight {
        draw_visual_selection(
            cr,
            layout,
            yh,
            &rw.lines,
            rect,
            line_height,
            rw.scroll_top,
            text_x_offset,
            theme.yank_highlight_bg,
            theme.yank_highlight_alpha,
        );
    }

    // Render gutter (bp marker + git marker + fold indicators + optional line numbers)
    if rw.gutter_char_width > 0 {
        for (view_idx, rl) in rw.lines.iter().enumerate() {
            let y = rect.y + view_idx as f64 * line_height;

            // Track how many left-aligned marker chars have been rendered.
            let mut char_offset = 0usize;

            // Breakpoint column — leftmost when any breakpoints/session active.
            if rw.has_breakpoints {
                let bp_ch: String = rl.gutter_text.chars().take(1).collect();
                let bp_color = if rl.is_dap_current || rl.is_breakpoint {
                    theme.diagnostic_error
                } else {
                    theme.line_number_fg
                };
                layout.set_text(&bp_ch);
                layout.set_attributes(None);
                let (br, bg_c, bb) = bp_color.to_cairo();
                cr.set_source_rgb(br, bg_c, bb);
                cr.move_to(rect.x + 3.0, y);
                pangocairo::show_layout(cr, layout);
                char_offset += 1;
            }

            // Git marker column.
            if rw.has_git_diff {
                let git_ch: String = rl.gutter_text.chars().skip(char_offset).take(1).collect();
                let git_color = match rl.git_diff {
                    Some(GitLineStatus::Added) => theme.git_added,
                    Some(GitLineStatus::Modified) => theme.git_modified,
                    None => theme.line_number_fg,
                };
                layout.set_text(&git_ch);
                layout.set_attributes(None);
                let (gr, gg, gb) = git_color.to_cairo();
                cr.set_source_rgb(gr, gg, gb);
                cr.move_to(rect.x + char_offset as f64 * char_width + 3.0, y);
                pangocairo::show_layout(cr, layout);
                char_offset += 1;

                // Fold+numbers portion right-aligned.
                let rest: String = rl.gutter_text.chars().skip(char_offset).collect();
                layout.set_text(&rest);
                layout.set_attributes(None);
            } else if char_offset > 0 {
                // bp column only — rest is fold+numbers.
                let rest: String = rl.gutter_text.chars().skip(char_offset).collect();
                layout.set_text(&rest);
                layout.set_attributes(None);
            } else {
                // No marker columns.
                layout.set_text(&rl.gutter_text);
                layout.set_attributes(None);
            }

            let (num_width, _) = layout.pixel_size();
            let num_x = rect.x + gutter_width - num_width as f64 - char_width + 3.0;

            let num_color = if rw.is_active && rl.is_current_line {
                theme.line_number_active_fg
            } else {
                theme.line_number_fg
            };
            let (nr, ng, nb) = num_color.to_cairo();
            cr.set_source_rgb(nr, ng, nb);
            cr.move_to(num_x, y);
            pangocairo::show_layout(cr, layout);

            // Diagnostic gutter icon (colored dot at leftmost gutter position)
            if let Some(severity) = rw.diagnostic_gutter.get(&rl.line_idx) {
                let diag_color = match severity {
                    DiagnosticSeverity::Error => theme.diagnostic_error,
                    DiagnosticSeverity::Warning => theme.diagnostic_warning,
                    DiagnosticSeverity::Information => theme.diagnostic_info,
                    DiagnosticSeverity::Hint => theme.diagnostic_hint,
                };
                let (dr, dg, db) = diag_color.to_cairo();
                cr.set_source_rgb(dr, dg, db);
                let dot_r = line_height * 0.2;
                let dot_cx = rect.x + 3.0 + dot_r;
                let dot_cy = y + line_height * 0.5;
                cr.arc(dot_cx, dot_cy, dot_r, 0.0, 2.0 * std::f64::consts::PI);
                cr.fill().ok();
            }
        }
    } // end gutter rendering block

    // Clip text area (excluding gutter)
    cr.save().unwrap();
    cr.rectangle(
        rect.x + gutter_width,
        rect.y,
        rect.width - gutter_width,
        rect.height,
    );
    cr.clip();

    // Render each visible line
    for (view_idx, rl) in rw.lines.iter().enumerate() {
        let y = rect.y + view_idx as f64 * line_height;

        layout.set_text(&rl.raw_text);

        let attrs = build_pango_attrs(&rl.spans);
        layout.set_attributes(Some(&attrs));

        let (fr, fg_g, fb) = theme.foreground.to_cairo();
        cr.set_source_rgb(fr, fg_g, fb);
        cr.move_to(text_x_offset, y);
        pangocairo::show_layout(cr, layout);

        // Diagnostic underlines (wavy squiggles)
        for dm in &rl.diagnostics {
            let diag_color = match dm.severity {
                DiagnosticSeverity::Error => theme.diagnostic_error,
                DiagnosticSeverity::Warning => theme.diagnostic_warning,
                DiagnosticSeverity::Information => theme.diagnostic_info,
                DiagnosticSeverity::Hint => theme.diagnostic_hint,
            };
            let (dr, dg, db) = diag_color.to_cairo();
            cr.set_source_rgb(dr, dg, db);
            cr.set_line_width(1.0);

            let start_byte = rl
                .raw_text
                .char_indices()
                .nth(dm.start_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());
            let end_byte = rl
                .raw_text
                .char_indices()
                .nth(dm.end_col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());

            layout.set_text(&rl.raw_text);
            layout.set_attributes(None);

            let start_pos = layout.index_to_pos(start_byte as i32);
            let end_pos = layout.index_to_pos(end_byte as i32);
            let x0 = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;
            let x1 = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;
            let underline_y = y + line_height - 2.0;

            // Draw wavy underline
            let wave_h = 1.5;
            let wave_len = 4.0;
            cr.move_to(x0, underline_y);
            let mut wx = x0;
            let mut up = true;
            while wx < x1 {
                let next_x = (wx + wave_len).min(x1);
                let cy = if up {
                    underline_y - wave_h
                } else {
                    underline_y + wave_h
                };
                cr.curve_to(
                    wx + (next_x - wx) * 0.5,
                    cy,
                    wx + (next_x - wx) * 0.5,
                    cy,
                    next_x,
                    underline_y,
                );
                wx = next_x;
                up = !up;
            }
            cr.stroke().ok();
        }
    }

    cr.restore().unwrap();

    // Render cursor
    if let Some((cursor_pos, cursor_shape)) = &rw.cursor {
        if let Some(rl) = rw.lines.get(cursor_pos.view_line) {
            layout.set_text(&rl.raw_text);
            layout.set_attributes(None);

            let byte_offset: usize = rl
                .raw_text
                .char_indices()
                .nth(cursor_pos.col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());

            let pos = layout.index_to_pos(byte_offset as i32);
            let cursor_x = text_x_offset + pos.x() as f64 / pango::SCALE as f64;
            let char_w = pos.width() as f64 / pango::SCALE as f64;
            let cursor_y = rect.y + cursor_pos.view_line as f64 * line_height;

            let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
            let char_w = if char_w > 0.0 {
                char_w
            } else {
                font_metrics.approximate_char_width() as f64 / pango::SCALE as f64
            };
            match cursor_shape {
                CursorShape::Block => {
                    cr.set_source_rgba(cr_r, cr_g, cr_b, theme.cursor_normal_alpha);
                    cr.rectangle(cursor_x, cursor_y, char_w, line_height);
                    cr.fill().unwrap();
                }
                CursorShape::Bar => {
                    cr.set_source_rgb(cr_r, cr_g, cr_b);
                    cr.rectangle(cursor_x, cursor_y, 2.0, line_height);
                    cr.fill().unwrap();
                }
                CursorShape::Underline => {
                    cr.set_source_rgb(cr_r, cr_g, cr_b);
                    let bar_h = (line_height * 0.12).max(2.0);
                    cr.rectangle(cursor_x, cursor_y + line_height - bar_h, char_w, bar_h);
                    cr.fill().unwrap();
                }
            }
        }
    }

    // Secondary cursors (multi-cursor Alt-D) — dimmed block at each extra position.
    let fallback_char_w = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
    let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
    for extra_pos in &rw.extra_cursors {
        if let Some(rl) = rw.lines.get(extra_pos.view_line) {
            layout.set_text(&rl.raw_text);
            layout.set_attributes(None);
            let byte_offset: usize = rl
                .raw_text
                .char_indices()
                .nth(extra_pos.col)
                .map(|(i, _)| i)
                .unwrap_or(rl.raw_text.len());
            let pos = layout.index_to_pos(byte_offset as i32);
            let ex = text_x_offset + pos.x() as f64 / pango::SCALE as f64;
            let ew = {
                let w = pos.width() as f64 / pango::SCALE as f64;
                if w > 0.0 {
                    w
                } else {
                    fallback_char_w
                }
            };
            let ey = rect.y + extra_pos.view_line as f64 * line_height;
            cr.set_source_rgba(cr_r, cr_g, cr_b, theme.cursor_normal_alpha * 0.5);
            cr.rectangle(ex, ey, ew, line_height);
            cr.fill().unwrap();
        }
    }
}

/// Convert a slice of [`StyledSpan`]s into a Pango [`AttrList`].
fn build_pango_attrs(spans: &[StyledSpan]) -> AttrList {
    let attrs = AttrList::new();
    for span in spans {
        let (fr, fg_g, fb) = span.style.fg.to_pango_u16();
        let mut fg_attr = AttrColor::new_foreground(fr, fg_g, fb);
        fg_attr.set_start_index(span.start_byte as u32);
        fg_attr.set_end_index(span.end_byte as u32);
        attrs.insert(fg_attr);

        if let Some(bg) = span.style.bg {
            let (br, bg_g, bb) = bg.to_pango_u16();
            let mut bg_attr = AttrColor::new_background(br, bg_g, bb);
            bg_attr.set_start_index(span.start_byte as u32);
            bg_attr.set_end_index(span.end_byte as u32);
            attrs.insert(bg_attr);
        }
    }
    attrs
}

#[allow(clippy::too_many_arguments)]
fn draw_visual_selection(
    cr: &Context,
    layout: &pango::Layout,
    sel: &SelectionRange,
    lines: &[render::RenderedLine],
    rect: &WindowRect,
    line_height: f64,
    scroll_top: usize,
    text_x_offset: f64,
    color: render::Color,
    alpha: f64,
) {
    let visible_lines = lines.len();
    let (sr, sg, sb) = color.to_cairo();
    cr.set_source_rgba(sr, sg, sb, alpha);

    match sel.kind {
        SelectionKind::Line => {
            for line_idx in sel.start_line..=sel.end_line {
                if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                    let view_idx = line_idx - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    let highlight_width = rect.width - (text_x_offset - rect.x);
                    cr.rectangle(text_x_offset, y, highlight_width, line_height);
                }
            }
            cr.fill().unwrap();
        }
        SelectionKind::Char => {
            if sel.start_line == sel.end_line {
                // Single-line selection
                if sel.start_line >= scroll_top && sel.start_line < scroll_top + visible_lines {
                    let view_idx = sel.start_line - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    let line_text = &lines[view_idx].raw_text;

                    layout.set_text(line_text);
                    layout.set_attributes(None);

                    let start_byte = line_text
                        .char_indices()
                        .nth(sel.start_col)
                        .map(|(i, _)| i)
                        .unwrap_or(line_text.len());
                    let start_pos = layout.index_to_pos(start_byte as i32);
                    let start_x = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;

                    let end_col = (sel.end_col + 1).min(line_text.chars().count());
                    let end_byte = line_text
                        .char_indices()
                        .nth(end_col)
                        .map(|(i, _)| i)
                        .unwrap_or(line_text.len());
                    let end_pos = layout.index_to_pos(end_byte as i32);
                    let end_x = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;

                    cr.rectangle(start_x, y, end_x - start_x, line_height);
                    cr.fill().unwrap();
                }
            } else {
                // Multi-line selection
                for line_idx in sel.start_line..=sel.end_line {
                    if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                        let view_idx = line_idx - scroll_top;
                        let y = rect.y + view_idx as f64 * line_height;
                        let line_text = &lines[view_idx].raw_text;

                        layout.set_text(line_text);
                        layout.set_attributes(None);

                        if line_idx == sel.start_line {
                            let start_byte = line_text
                                .char_indices()
                                .nth(sel.start_col)
                                .map(|(i, _)| i)
                                .unwrap_or(line_text.len());
                            let start_pos = layout.index_to_pos(start_byte as i32);
                            let start_x =
                                text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;
                            let (line_width, _) = layout.pixel_size();
                            cr.rectangle(
                                start_x,
                                y,
                                text_x_offset + line_width as f64 - start_x,
                                line_height,
                            );
                            cr.fill().unwrap();
                        } else if line_idx == sel.end_line {
                            let end_col = (sel.end_col + 1).min(line_text.chars().count());
                            let end_byte = line_text
                                .char_indices()
                                .nth(end_col)
                                .map(|(i, _)| i)
                                .unwrap_or(line_text.len());
                            let end_pos = layout.index_to_pos(end_byte as i32);
                            let end_x = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;
                            cr.rectangle(text_x_offset, y, end_x - text_x_offset, line_height);
                            cr.fill().unwrap();
                        } else {
                            let (line_width, _) = layout.pixel_size();
                            cr.rectangle(text_x_offset, y, line_width as f64, line_height);
                            cr.fill().unwrap();
                        }
                    }
                }
            }
        }
        SelectionKind::Block => {
            for line_idx in sel.start_line..=sel.end_line {
                if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                    let view_idx = line_idx - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    let line_text = &lines[view_idx].raw_text;
                    let line_len = line_text.chars().count();

                    layout.set_text(line_text);
                    layout.set_attributes(None);

                    if sel.start_col < line_len {
                        let start_byte = line_text
                            .char_indices()
                            .nth(sel.start_col)
                            .map(|(i, _)| i)
                            .unwrap_or(line_text.len());
                        let start_pos = layout.index_to_pos(start_byte as i32);
                        let start_x = text_x_offset + start_pos.x() as f64 / pango::SCALE as f64;

                        let block_end_col = (sel.end_col + 1).min(line_len);
                        let end_byte = line_text
                            .char_indices()
                            .nth(block_end_col)
                            .map(|(i, _)| i)
                            .unwrap_or(line_text.len());
                        let end_pos = layout.index_to_pos(end_byte as i32);
                        let end_x = text_x_offset + end_pos.x() as f64 / pango::SCALE as f64;

                        cr.rectangle(start_x, y, end_x - start_x, line_height);
                    }
                }
            }
            cr.fill().unwrap();
        }
    }
}

fn draw_window_separators(
    cr: &Context,
    window_rects: &[(core::WindowId, WindowRect)],
    theme: &Theme,
) {
    if window_rects.len() <= 1 {
        return;
    }

    let (sr, sg, sb) = theme.separator.to_cairo();
    cr.set_source_rgb(sr, sg, sb);
    cr.set_line_width(1.0);

    // Draw separators between adjacent windows
    for i in 0..window_rects.len() {
        for j in (i + 1)..window_rects.len() {
            let (_, rect_a) = &window_rects[i];
            let (_, rect_b) = &window_rects[j];

            // Check if they share a horizontal edge
            if (rect_a.y + rect_a.height - rect_b.y).abs() < 2.0 {
                let x_start = rect_a.x.max(rect_b.x);
                let x_end = (rect_a.x + rect_a.width).min(rect_b.x + rect_b.width);
                if x_end > x_start {
                    cr.move_to(x_start, rect_a.y + rect_a.height);
                    cr.line_to(x_end, rect_a.y + rect_a.height);
                    cr.stroke().unwrap();
                }
            }

            // Check if they share a vertical edge
            if (rect_a.x + rect_a.width - rect_b.x).abs() < 2.0 {
                let y_start = rect_a.y.max(rect_b.y);
                let y_end = (rect_a.y + rect_a.height).min(rect_b.y + rect_b.height);
                if y_end > y_start {
                    cr.move_to(rect_a.x + rect_a.width, y_start);
                    cr.line_to(rect_a.x + rect_a.width, y_end);
                    cr.stroke().unwrap();
                }
            }
        }
    }
}

fn draw_completion_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
) {
    let Some(menu) = &screen.completion else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };
    let Some((cursor_pos, _)) = &active_win.cursor else {
        return;
    };

    // Anchor popup below the cursor cell, to the right of the gutter.
    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let popup_x =
        active_win.rect.x + gutter_width + cursor_pos.col as f64 * char_width - h_scroll_offset;
    let popup_y = active_win.rect.y + (cursor_pos.view_line + 1) as f64 * line_height;

    let visible = menu.candidates.len().min(10);
    let popup_w = ((menu.max_width + 2) as f64 * char_width).max(100.0);
    let popup_h = visible as f64 * line_height;

    // Background
    let (r, g, b) = theme.completion_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.completion_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Items
    for (i, candidate) in menu.candidates.iter().enumerate().take(visible) {
        let item_y = popup_y + i as f64 * line_height;

        // Selected row highlight
        if i == menu.selected_idx {
            let (r, g, b) = theme.completion_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x, item_y, popup_w, line_height);
            cr.fill().ok();
        }

        // Candidate text
        let (r, g, b) = theme.completion_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        let display = format!(" {}", candidate);
        layout.set_text(&display);
        layout.set_attributes(None);
        cr.move_to(popup_x, item_y);
        pangocairo::show_layout(cr, layout);
    }
}

fn draw_hover_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
) {
    let Some(hover) = &screen.hover else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };

    // Position above the anchor line
    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let anchor_view_line = hover.anchor_line.saturating_sub(active_win.scroll_top);
    let popup_x =
        active_win.rect.x + gutter_width + hover.anchor_col as f64 * char_width - h_scroll_offset;

    // Split text into lines and measure
    let text_lines: Vec<&str> = hover.text.lines().collect();
    let num_lines = text_lines.len().min(20) as f64;
    let max_line_len = text_lines.iter().map(|l| l.len()).max().unwrap_or(10);
    let popup_w = ((max_line_len + 2) as f64 * char_width).max(100.0);
    let popup_h = num_lines * line_height + 4.0;

    // Place above cursor if possible, otherwise below
    let popup_y = if anchor_view_line as f64 * line_height > popup_h {
        active_win.rect.y + anchor_view_line as f64 * line_height - popup_h
    } else {
        active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height
    };

    // Background
    let (r, g, b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.hover_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Text
    let (r, g, b) = theme.hover_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    for (i, text_line) in text_lines.iter().enumerate().take(20) {
        let display = format!(" {}", text_line);
        layout.set_text(&display);
        layout.set_attributes(None);
        cr.move_to(popup_x, popup_y + 2.0 + i as f64 * line_height);
        pangocairo::show_layout(cr, layout);
    }
}

fn draw_signature_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    line_height: f64,
    char_width: f64,
) {
    let Some(sig) = &screen.signature_help else {
        return;
    };
    let Some(active_win) = screen
        .windows
        .iter()
        .find(|w| w.window_id == screen.active_window_id)
    else {
        return;
    };

    let gutter_width = active_win.gutter_char_width as f64 * char_width;
    let h_scroll_offset = active_win.scroll_left as f64 * char_width;
    let anchor_view_line = sig.anchor_line.saturating_sub(active_win.scroll_top);
    let popup_x =
        active_win.rect.x + gutter_width + sig.anchor_col as f64 * char_width - h_scroll_offset;

    let popup_w = ((sig.label.len() + 4) as f64 * char_width).max(120.0);
    let popup_h = line_height + 4.0;

    // Place above the cursor if space allows, otherwise below.
    let popup_y = if anchor_view_line as f64 * line_height > popup_h {
        active_win.rect.y + anchor_view_line as f64 * line_height - popup_h
    } else {
        active_win.rect.y + (anchor_view_line as f64 + 1.0) * line_height
    };

    // Background
    let (r, g, b) = theme.hover_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.hover_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Build Pango attr list: active parameter in keyword color, rest in hover_fg.
    let display = format!(" {}", sig.label);
    let offset = 1usize; // accounts for the leading space

    let attrs = AttrList::new();
    let (fr, fg_g, fb) = theme.hover_fg.to_pango_u16();
    let mut base_attr = AttrColor::new_foreground(fr, fg_g, fb);
    base_attr.set_start_index(0);
    base_attr.set_end_index(display.len() as u32);
    attrs.insert(base_attr);

    if let Some(idx) = sig.active_param {
        if let Some(&(start, end)) = sig.params.get(idx) {
            let (kr, kg, kb) = theme.keyword.to_pango_u16();
            let mut kw_attr = AttrColor::new_foreground(kr, kg, kb);
            kw_attr.set_start_index((offset + start) as u32);
            kw_attr.set_end_index((offset + end) as u32);
            attrs.insert(kw_attr);
        }
    }

    layout.set_text(&display);
    layout.set_attributes(Some(&attrs));
    cr.move_to(popup_x, popup_y + 2.0);
    pangocairo::show_layout(cr, layout);
    layout.set_attributes(None);
}

#[allow(clippy::too_many_arguments)]
fn draw_fuzzy_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
    _char_width: f64,
) {
    let Some(fuzzy) = &screen.fuzzy else {
        return;
    };

    // Size: 60% of editor width (min 400px), 55% of editor height (min 300px)
    let popup_w = (editor_width * 0.6).max(400.0);
    let popup_h = (editor_height * 0.55).max(300.0);

    // Centered in editor area
    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    // Background
    let (r, g, b) = theme.fuzzy_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Title row: "  Find Files  (N/M files)"
    let title = format!(
        "  Find Files  ({}/{} files)",
        fuzzy.results.len(),
        fuzzy.total_files
    );
    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&title);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y);
    pangocairo::show_layout(cr, layout);

    // Query row: "> " + query
    let query_text = format!("> {}_", fuzzy.query);
    let (r, g, b) = theme.fuzzy_query_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&query_text);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y + line_height);
    pangocairo::show_layout(cr, layout);

    // Horizontal separator
    let sep_y = popup_y + 2.0 * line_height;
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.move_to(popup_x, sep_y);
    cr.line_to(popup_x + popup_w, sep_y);
    cr.stroke().ok();

    // Result rows
    let rows_area_h = popup_h - 2.0 * line_height - 2.0; // minus title, query, sep
    let visible_rows = ((rows_area_h / line_height) as usize).min(fuzzy.results.len());
    for (i, display) in fuzzy.results.iter().enumerate().take(visible_rows) {
        let item_y = sep_y + 1.0 + i as f64 * line_height;
        let is_selected = i == fuzzy.selected_idx;

        // Selected row highlight
        if is_selected {
            let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x, item_y, popup_w, line_height);
            cr.fill().ok();
        }

        // Row text with ▶ prefix for selected
        let prefix = if is_selected { "▶ " } else { "  " };
        let row_text = format!("{}{}", prefix, display);
        let (r, g, b) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(&row_text);
        layout.set_attributes(None);
        cr.move_to(popup_x, item_y);
        pangocairo::show_layout(cr, layout);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_live_grep_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
) {
    let Some(grep) = &screen.live_grep else {
        return;
    };

    // Size: 80% of editor width (min 600px), 65% of editor height (min 400px)
    let popup_w = (editor_width * 0.8).max(600.0);
    let popup_h = (editor_height * 0.65).max(400.0);

    // Centered in editor area
    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    // Background
    let (r, g, b) = theme.fuzzy_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Title row
    let title = format!("  Live Grep  {} matches", grep.total_matches);
    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&title);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y);
    pangocairo::show_layout(cr, layout);

    // Query row: "> " + query
    let query_text = format!("> {}_", grep.query);
    let (r, g, b) = theme.fuzzy_query_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&query_text);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y + line_height);
    pangocairo::show_layout(cr, layout);

    // Horizontal separator
    let sep_y = popup_y + 2.0 * line_height;
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.move_to(popup_x, sep_y);
    cr.line_to(popup_x + popup_w, sep_y);
    cr.stroke().ok();

    // Two-column layout: left pane = 40% of popup width
    let left_pane_w = popup_w * 0.4;
    let right_pane_x = popup_x + left_pane_w + 1.0;
    let right_pane_w = popup_w - left_pane_w - 1.0;

    // Vertical separator between panes
    cr.move_to(popup_x + left_pane_w, sep_y);
    cr.line_to(popup_x + left_pane_w, popup_y + popup_h);
    cr.stroke().ok();

    let rows_area_h = popup_h - 2.0 * line_height - 2.0;
    let visible_rows = (rows_area_h / line_height) as usize;

    // Compute scroll offset so the selected row is always visible.
    // Stateless: derived entirely from selected_idx each frame.
    let scroll_top = if grep.selected_idx < visible_rows {
        0
    } else {
        grep.selected_idx + 1 - visible_rows
    };

    // Left pane: result rows — clipped to left_pane_w to prevent text spill
    cr.save().ok();
    cr.rectangle(popup_x, sep_y, left_pane_w, rows_area_h + 2.0);
    cr.clip();

    for i in 0..visible_rows {
        let result_idx = scroll_top + i;
        let Some(display) = grep.results.get(result_idx) else {
            break;
        };
        let item_y = sep_y + 1.0 + i as f64 * line_height;
        let is_selected = result_idx == grep.selected_idx;

        if is_selected {
            let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x, item_y, left_pane_w, line_height);
            cr.fill().ok();
        }

        let prefix = if is_selected { "▶ " } else { "  " };
        let row_text = format!("{}{}", prefix, display);
        let (r, g, b) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(&row_text);
        layout.set_attributes(None);
        cr.move_to(popup_x, item_y);
        pangocairo::show_layout(cr, layout);
    }

    cr.restore().ok();

    // Right pane: preview lines — clipped to right pane bounds
    cr.save().ok();
    cr.rectangle(right_pane_x, sep_y, right_pane_w, rows_area_h + 2.0);
    cr.clip();

    for (i, (lineno, text, is_match)) in grep.preview_lines.iter().enumerate().take(visible_rows) {
        let item_y = sep_y + 1.0 + i as f64 * line_height;
        let preview_text = format!("{:4}: {}", lineno, text);

        if *is_match {
            let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
        } else {
            let (r, g, b) = theme.fuzzy_fg.to_cairo();
            cr.set_source_rgb(r, g, b);
        }

        layout.set_text(&preview_text);
        layout.set_attributes(None);
        cr.move_to(right_pane_x, item_y);
        pangocairo::show_layout(cr, layout);
    }

    cr.restore().ok();
}

/// Draw the command palette modal (Ctrl+Shift+P) on top of the editor.
fn draw_command_palette_popup(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_width: f64,
    editor_height: f64,
    line_height: f64,
) {
    let Some(palette) = &screen.command_palette else {
        return;
    };

    // Size: 55% of editor width (min 500px), 60% of editor height (min 350px)
    let popup_w = (editor_width * 0.55).max(500.0);
    let popup_h = (editor_height * 0.60).max(350.0);

    // Centered in editor area
    let popup_x = (editor_width - popup_w) / 2.0;
    let popup_y = (editor_height - popup_h) / 2.0;

    // Background
    let (r, g, b) = theme.fuzzy_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.fill().ok();

    // Border
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.rectangle(popup_x, popup_y, popup_w, popup_h);
    cr.stroke().ok();

    // Title row
    let title = format!("  Command Palette  ({} commands)", palette.items.len());
    let (r, g, b) = theme.fuzzy_title_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&title);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y);
    pangocairo::show_layout(cr, layout);

    // Query row: "> " + query + cursor
    let query_text = format!("> {}_", palette.query);
    let (r, g, b) = theme.fuzzy_query_fg.to_cairo();
    cr.set_source_rgb(r, g, b);
    layout.set_text(&query_text);
    layout.set_attributes(None);
    cr.move_to(popup_x, popup_y + line_height);
    pangocairo::show_layout(cr, layout);

    // Horizontal separator
    let sep_y = popup_y + 2.0 * line_height;
    let (r, g, b) = theme.fuzzy_border.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(1.0);
    cr.move_to(popup_x, sep_y);
    cr.line_to(popup_x + popup_w, sep_y);
    cr.stroke().ok();

    // Result rows (label left, shortcut right-aligned, VSCode style)
    let rows_area_h = popup_h - 2.0 * line_height - 2.0;
    let visible_rows = (rows_area_h / line_height) as usize;
    let total_items = palette.items.len();
    let items_to_show = palette.items.iter().skip(palette.scroll_top);

    // Scrollbar geometry (6px wide strip on the right edge)
    const SB_W: f64 = 6.0;
    let sb_x = popup_x + popup_w - SB_W;
    let sb_track_y = sep_y + 1.0;
    let sb_track_h = rows_area_h;

    // Content area is narrowed by scrollbar width
    let content_w = popup_w - SB_W;

    for (i, (label, shortcut)) in items_to_show.enumerate().take(visible_rows) {
        let item_y = sep_y + 1.0 + i as f64 * line_height;
        let display_idx = palette.scroll_top + i;
        let is_selected = display_idx == palette.selected_idx;

        // Selected row highlight
        if is_selected {
            let (r, g, b) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(r, g, b);
            cr.rectangle(popup_x, item_y, content_w, line_height);
            cr.fill().ok();
        }

        // Label (left-aligned with ▶ prefix for selected)
        let prefix = if is_selected { "▶ " } else { "  " };
        let row_text = format!("{}{}", prefix, label);
        let (r, g, b) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(r, g, b);
        layout.set_text(&row_text);
        layout.set_attributes(None);
        cr.move_to(popup_x, item_y);
        pangocairo::show_layout(cr, layout);

        // Shortcut (right-aligned within content area, dimmed)
        if !shortcut.is_empty() {
            let shortcut_text = format!("{}  ", shortcut);
            let (r, g, b) = theme.fuzzy_border.to_cairo();
            cr.set_source_rgb(r, g, b);
            layout.set_text(&shortcut_text);
            layout.set_attributes(None);
            let (sc_w, _) = layout.pixel_size();
            cr.move_to(popup_x + content_w - sc_w as f64, item_y);
            pangocairo::show_layout(cr, layout);
        }
    }

    // Scrollbar — only draw when content overflows
    if total_items > visible_rows && visible_rows > 0 {
        // Track background
        let (tr, tg, tb) = theme.fuzzy_bg.to_cairo();
        cr.set_source_rgb(tr * 0.7, tg * 0.7, tb * 0.7);
        cr.rectangle(sb_x, sb_track_y, SB_W, sb_track_h);
        cr.fill().ok();

        // Thumb position and size
        let thumb_ratio = visible_rows as f64 / total_items as f64;
        let thumb_h = (sb_track_h * thumb_ratio).max(8.0);
        let max_scroll = total_items.saturating_sub(visible_rows) as f64;
        let scroll_frac = if max_scroll > 0.0 {
            palette.scroll_top as f64 / max_scroll
        } else {
            0.0
        };
        let thumb_y = sb_track_y + scroll_frac * (sb_track_h - thumb_h);

        let (br, bg_c, bb) = theme.fuzzy_border.to_cairo();
        cr.set_source_rgb(br, bg_c, bb);
        cr.rectangle(sb_x + 1.0, thumb_y, SB_W - 2.0, thumb_h);
        cr.fill().ok();
    }
}

/// Draw the tab bar for the bottom panel (Terminal / Debug Output).
/// One row high at `(x, y)`, full width `w`.
#[allow(clippy::too_many_arguments)]
fn draw_bottom_panel_tabs(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    line_height: f64,
) {
    let (hr, hg, hb) = theme.status_bg.to_cairo();
    let (fr, fg, fb) = theme.status_fg.to_cairo();
    let (ar, ag, ab) = theme.tab_active_fg.to_cairo();

    // Background.
    cr.set_source_rgb(hr, hg, hb);
    cr.rectangle(x, y, w, line_height);
    cr.fill().ok();

    layout.set_attributes(None);

    let tabs = [
        ("  \u{f489}  Terminal  ", render::BottomPanelKind::Terminal), // nf-md-console
        (
            "  \u{f188}  Debug Output  ",
            render::BottomPanelKind::DebugOutput,
        ), // nf-fa-bug
    ];

    let mut cursor_x = x + 4.0;
    for (label, kind) in &tabs {
        let is_active = screen.bottom_tabs.active == *kind;
        let (lr, lg, lb) = if is_active {
            (ar, ag, ab)
        } else {
            (fr, fg, fb)
        };
        cr.set_source_rgb(lr, lg, lb);
        layout.set_text(label);
        cr.move_to(cursor_x, y);
        pangocairo::show_layout(cr, layout);
        // Underline the active tab.
        if is_active {
            let extents = layout.pixel_extents().1;
            let tab_w = extents.width() as f64;
            cr.set_source_rgb(ar, ag, ab);
            cr.rectangle(cursor_x, y + line_height - 2.0, tab_w, 2.0);
            cr.fill().ok();
        }
        let extents = layout.pixel_extents().1;
        cursor_x += extents.width() as f64 + 8.0;
    }
}

/// Draw debug output lines (read-only scrolling log).
#[allow(clippy::too_many_arguments)]
fn draw_debug_output(
    cr: &Context,
    layout: &pango::Layout,
    output_lines: &[String],
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    let (br, bg_g, bb) = theme.completion_bg.to_cairo();
    cr.set_source_rgb(br, bg_g, bb);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    let (fr, fg, fb) = theme.fuzzy_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);
    layout.set_attributes(None);

    let visible_rows = (h / line_height) as usize;
    let start = output_lines.len().saturating_sub(visible_rows);
    for (row, line_text) in output_lines.iter().skip(start).enumerate() {
        let ry = y + row as f64 * line_height;
        let text = format!("  {line_text}");
        layout.set_text(&text);
        cr.move_to(x, ry);
        pangocairo::show_layout(cr, layout);
    }
}

/// Draw the VSCode-style debug sidebar content.
///
/// Shows four sections stacked vertically:
///   • VARIABLES (with ▶/▼ expansion)
///   • WATCH (expressions + values)
///   • CALL STACK (frames, active highlighted)
///   • BREAKPOINTS (file:line list)
///
/// A 2-row header at the top shows the session status and a Run/Stop button.
#[allow(clippy::too_many_arguments)]
fn draw_debug_sidebar(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    let sidebar = &screen.debug_sidebar;

    let (bg_r, bg_g, bg_b) = theme.completion_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.status_fg.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();
    let (act_r, act_g, act_b) = theme.tab_active_fg.to_cairo();

    // Paint sidebar background.
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);

    // ── Row 0: header strip ─────────────────────────────────────────────────
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y, w, line_height);
    cr.fill().ok();

    let cfg_name = sidebar.launch_config_name.as_deref().unwrap_or("no config");
    let header_text = format!("  \u{f188} DEBUG  |  {cfg_name}");
    cr.set_source_rgb(fg_r, fg_g, fg_b);
    layout.set_text(&header_text);
    cr.move_to(x + 4.0, y);
    pangocairo::show_layout(cr, layout);

    // ── Row 1: Run/Stop button ───────────────────────────────────────────────
    let btn_y = y + line_height;
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, btn_y, w, line_height);
    cr.fill().ok();

    let (btn_label, btn_color) = if sidebar.session_active && sidebar.stopped {
        ("\u{f04b}  Continue", (0.38_f64, 0.73_f64, 0.45_f64)) // green play
    } else if sidebar.session_active {
        ("\u{f04d}  Stop", (0.86_f64, 0.27_f64, 0.22_f64)) // red stop
    } else {
        ("\u{f04b}  Start Debugging", (0.38_f64, 0.73_f64, 0.45_f64)) // green play
    };
    cr.set_source_rgb(btn_color.0, btn_color.1, btn_color.2);
    layout.set_text(btn_label);
    cr.move_to(x + 8.0, btn_y);
    pangocairo::show_layout(cr, layout);

    // ── Sections with fixed-height allocation + per-section scrolling ──────
    let sections: [(
        &str,
        &[render::DebugSidebarItem],
        render::DebugSidebarSection,
        usize,
    ); 4] = [
        (
            "\u{f6a9} VARIABLES",
            &sidebar.variables,
            render::DebugSidebarSection::Variables,
            0,
        ),
        (
            "\u{f06e} WATCH",
            &sidebar.watch,
            render::DebugSidebarSection::Watch,
            1,
        ),
        (
            "\u{f020e} CALL STACK",
            &sidebar.frames,
            render::DebugSidebarSection::CallStack,
            2,
        ),
        (
            "\u{f111} BREAKPOINTS",
            &sidebar.breakpoints,
            render::DebugSidebarSection::Breakpoints,
            3,
        ),
    ];

    // Compute per-section content heights (equal share of remaining space).
    // Available px after header(1) + button(1) = 2 line_heights.
    // Each section has 1 header row (4 total), so content px = h - 6*line_height.
    let content_px = (h - 6.0 * line_height).max(0.0);
    let sec_content_h = (content_px / 4.0).floor();

    let mut cursor_y = btn_y + line_height;
    let max_y = y + h;

    let (sb_r, sb_g, sb_b) = (0.5_f64, 0.5_f64, 0.5_f64); // scrollbar thumb color

    for (section_label, items, section_kind, sec_idx) in &sections {
        if cursor_y >= max_y {
            break;
        }

        // Section header row.
        let is_active_section = sidebar.active_section == *section_kind;
        let (shr, shg, shb) = if is_active_section {
            (act_r, act_g, act_b)
        } else {
            (fg_r, fg_g, fg_b)
        };
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, cursor_y, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(shr, shg, shb);
        layout.set_text(section_label);
        cr.move_to(x + 4.0, cursor_y);
        pangocairo::show_layout(cr, layout);
        cursor_y += line_height;

        let scroll_off = sidebar.scroll_offsets[*sec_idx];
        let sec_height = sidebar.section_heights[*sec_idx] as usize;
        let visible_rows = if sec_height > 0 {
            sec_height
        } else {
            (sec_content_h / line_height).floor() as usize
        };

        // Clip to section content area.
        let section_start_y = cursor_y;
        let section_end_y = (cursor_y + visible_rows as f64 * line_height).min(max_y);

        cr.save().ok();
        cr.rectangle(x, section_start_y, w, section_end_y - section_start_y);
        cr.clip();

        // Render items within the allocated height.
        for row_offset in 0..visible_rows {
            let item_y = section_start_y + row_offset as f64 * line_height;
            if item_y >= section_end_y {
                break;
            }
            let item_idx = scroll_off + row_offset;
            if items.is_empty() && row_offset == 0 {
                // Empty hint.
                cr.set_source_rgb(dim_r, dim_g, dim_b);
                let hint = if sidebar.session_active {
                    "  (empty)"
                } else {
                    "  (not running)"
                };
                layout.set_text(hint);
                cr.move_to(x + 4.0, item_y);
                pangocairo::show_layout(cr, layout);
            } else if item_idx < items.len() {
                let item = &items[item_idx];
                if item.is_selected {
                    cr.set_source_rgb(sel_r, sel_g, sel_b);
                    cr.rectangle(x, item_y, w, line_height);
                    cr.fill().ok();
                    cr.set_source_rgb(fg_r, fg_g, fg_b);
                } else {
                    cr.set_source_rgb(dim_r, dim_g, dim_b);
                }
                let indent_px = item.indent as f64 * 12.0;
                layout.set_text(&item.text);
                cr.move_to(x + 4.0 + indent_px, item_y);
                pangocairo::show_layout(cr, layout);
            }
        }

        // Draw scrollbar if items exceed visible height.
        if items.len() > visible_rows && visible_rows > 0 {
            let sb_w = 4.0_f64;
            let sb_x = x + w - sb_w;
            let track_h = visible_rows as f64 * line_height;
            let total_items = items.len();
            let thumb_h = ((visible_rows as f64 / total_items as f64) * track_h)
                .ceil()
                .max(line_height * 0.5);
            let max_scroll = total_items - visible_rows;
            let thumb_top = if max_scroll > 0 {
                (scroll_off as f64 / max_scroll as f64) * (track_h - thumb_h)
            } else {
                0.0
            };
            // Track background.
            cr.set_source_rgba(0.3, 0.3, 0.3, 0.3);
            cr.rectangle(sb_x, section_start_y, sb_w, track_h);
            cr.fill().ok();
            // Thumb.
            cr.set_source_rgb(sb_r, sb_g, sb_b);
            cr.rectangle(sb_x, section_start_y + thumb_top, sb_w, thumb_h);
            cr.fill().ok();
        }

        cr.restore().ok();
        cursor_y = section_start_y + visible_rows as f64 * line_height;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_quickfix_panel(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    editor_x: f64,
    editor_y: f64,
    editor_w: f64,
    qf_px: f64,
    line_height: f64,
) {
    let Some(qf) = &screen.quickfix else {
        return;
    };

    // Header row
    let (hr, hg, hb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(hr, hg, hb);
    cr.rectangle(editor_x, editor_y, editor_w, line_height);
    cr.fill().ok();
    let focus_mark = if qf.has_focus { " [FOCUS]" } else { "" };
    let title = format!("  QUICKFIX  ({} items){}", qf.total_items, focus_mark);
    let (fr, fg, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);
    layout.set_attributes(None);
    layout.set_text(&title);
    cr.move_to(editor_x, editor_y);
    pangocairo::show_layout(cr, layout);

    // Result rows
    let visible_rows = ((qf_px / line_height) as usize).saturating_sub(1);
    let scroll_top = (qf.selected_idx + 1).saturating_sub(visible_rows);
    for row_idx in 0..visible_rows {
        let item_idx = scroll_top + row_idx;
        if item_idx >= qf.items.len() {
            break;
        }
        let ry = editor_y + line_height * (row_idx + 1) as f64;
        let is_selected = item_idx == qf.selected_idx;
        if is_selected {
            let (sr, sg, sb) = theme.fuzzy_selected_bg.to_cairo();
            cr.set_source_rgb(sr, sg, sb);
            cr.rectangle(editor_x, ry, editor_w, line_height);
            cr.fill().ok();
        }
        let prefix = if is_selected { "▶ " } else { "  " };
        let text = format!("{}{}", prefix, qf.items[item_idx]);
        let (ir, ig, ib) = theme.fuzzy_fg.to_cairo();
        cr.set_source_rgb(ir, ig, ib);
        layout.set_text(&text);
        cr.move_to(editor_x, ry);
        pangocairo::show_layout(cr, layout);
    }
}

/// Nerd Font icons for the terminal panel toolbar.
const NF_CLOSE: &str = "󰅖"; // nf-md-close_box
const NF_SPLIT: &str = "󰤼"; // nf-md-view_split_vertical

/// Draw the integrated terminal bottom panel.
#[allow(clippy::too_many_arguments)]
fn draw_terminal_panel(
    cr: &Context,
    layout: &pango::Layout,
    panel: &render::TerminalPanel,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    term_px: f64,
    line_height: f64,
    char_width: f64,
    sender: &relm4::Sender<Msg>,
) {
    // Toolbar row (header)
    let (hr, hg, hb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(hr, hg, hb);
    cr.rectangle(x, y, w, line_height);
    cr.fill().ok();

    let (fr, fg2, fb) = theme.status_fg.to_cairo();
    layout.set_attributes(None);

    if panel.find_active {
        // Find bar mode: replace tab strip with query + match count
        let match_info = if panel.find_match_count == 0 {
            if panel.find_query.is_empty() {
                String::new()
            } else {
                "  (no matches)".to_string()
            }
        } else {
            format!(
                "  ({}/{})",
                panel.find_selected_idx + 1,
                panel.find_match_count
            )
        };
        let find_text = format!(" FIND: {}█{}", panel.find_query, match_info);
        cr.set_source_rgb(fr, fg2, fb);
        layout.set_text(&find_text);
        cr.move_to(x, y);
        pangocairo::show_layout(cr, layout);
        // Close icon right-aligned
        layout.set_text(NF_CLOSE);
        let (cw, _) = layout.pixel_size();
        cr.move_to(x + w - cw as f64 - 4.0, y);
        pangocairo::show_layout(cr, layout);
    } else {
        // Tab strip — each tab is 4 chars: "[N] "
        const TERMINAL_TAB_COLS: usize = 4;
        let mut tab_x = x;
        for i in 0..panel.tab_count {
            let label = format!("[{}] ", i + 1);
            if i == panel.active_tab {
                // Active tab: inverted colors (cursor background)
                let (ar, ag, ab) = theme.cursor.to_cairo();
                cr.set_source_rgb(ar, ag, ab);
                cr.rectangle(tab_x, y, char_width * TERMINAL_TAB_COLS as f64, line_height);
                cr.fill().ok();
                let (br, bg_, bb) = theme.background.to_cairo();
                cr.set_source_rgb(br, bg_, bb);
            } else {
                cr.set_source_rgb(fr, fg2, fb);
            }
            layout.set_text(&label);
            cr.move_to(tab_x, y);
            pangocairo::show_layout(cr, layout);
            tab_x += char_width * TERMINAL_TAB_COLS as f64;
        }

        // If no tabs yet (panel open but spawning), show a minimal title
        if panel.tab_count == 0 {
            cr.set_source_rgb(fr, fg2, fb);
            layout.set_text("  TERMINAL");
            cr.move_to(x, y);
            pangocairo::show_layout(cr, layout);
        }

        // Right-aligned toolbar buttons: + ⊞ ×  (each ~2 chars wide)
        cr.set_source_rgb(fr, fg2, fb);
        let btn_text = format!("+ {} {}", NF_SPLIT, NF_CLOSE);
        layout.set_text(&btn_text);
        let (btn_w, _) = layout.pixel_size();
        cr.move_to(x + w - btn_w as f64 - 4.0, y);
        pangocairo::show_layout(cr, layout);
    }

    // close_x / split_x used by click detection in MouseClick handler
    let _ = sender; // click detection handled in MouseClick

    // Scrollbar geometry
    const SB_W: f64 = 6.0;
    let content_y = y + line_height;
    let content_h = term_px - line_height;
    let rows_to_draw = ((term_px / line_height) as usize).saturating_sub(1);
    let total = panel.scrollback_rows + rows_to_draw;
    let (thumb_top_px, thumb_bot_px) = if panel.scrollback_rows == 0 {
        (0.0, content_h) // no scrollback → full bar
    } else {
        let thumb_h = ((rows_to_draw as f64 / total as f64) * content_h).max(4.0);
        let max_off = panel.scrollback_rows as f64;
        let frac = if panel.scroll_offset == 0 {
            1.0 // at live bottom → thumb at bottom
        } else {
            1.0 - (panel.scroll_offset as f64 / max_off).min(1.0)
        };
        let thumb_t = frac * (content_h - thumb_h);
        (thumb_t, thumb_t + thumb_h)
    };

    // Draw scrollbar track (right edge of whole panel)
    let sb_x = x + w - SB_W;
    let (tbr, tbg, tbb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(tbr * 1.4, tbg * 1.4, tbb * 1.4); // slightly lighter than header
    cr.rectangle(sb_x, content_y, SB_W, content_h);
    cr.fill().ok();
    // Draw scrollbar thumb
    let (fr, fg2, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgba(fr, fg2, fb, 0.5);
    cr.rectangle(
        sb_x + 1.0,
        content_y + thumb_top_px,
        SB_W - 2.0,
        thumb_bot_px - thumb_top_px,
    );
    cr.fill().ok();

    // ── Split view: draw left pane + divider + right pane ─────────────────────
    if let Some(ref left_rows) = panel.split_left_rows {
        let half_w = panel.split_left_cols as f64 * char_width;
        let div_x = x + half_w;

        // Fill both halves with terminal default bg.
        cr.set_source_rgb(30.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0);
        cr.rectangle(x, content_y, w - SB_W, content_h);
        cr.fill().ok();

        // Draw left pane cells.
        draw_terminal_cells(
            cr,
            layout,
            left_rows,
            x,
            content_y,
            half_w,
            line_height,
            char_width,
            theme,
        );

        // Draw divider (1px vertical line).
        let (dr, dg, db) = theme.separator.to_cairo();
        cr.set_source_rgb(dr, dg, db);
        cr.rectangle(div_x, content_y, 1.0, content_h);
        cr.fill().ok();

        // Draw right pane cells.
        draw_terminal_cells(
            cr,
            layout,
            &panel.rows,
            div_x + 1.0,
            content_y,
            half_w - 1.0,
            line_height,
            char_width,
            theme,
        );
        return;
    }

    // ── Normal single-pane view ────────────────────────────────────────────────
    // Content rows (terminal cells)
    let cell_area_w = w - SB_W;

    // Fill the entire content area with the default terminal background first.
    cr.set_source_rgb(30.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0);
    cr.rectangle(x, content_y, cell_area_w, content_h);
    cr.fill().ok();

    draw_terminal_cells(
        cr,
        layout,
        &panel.rows,
        x,
        content_y,
        cell_area_w,
        line_height,
        char_width,
        theme,
    );
}

/// Draw a grid of terminal cells into a rectangular region.
#[allow(clippy::too_many_arguments)]
fn draw_terminal_cells(
    cr: &Context,
    layout: &pango::Layout,
    rows: &[Vec<render::TerminalCell>],
    x: f64,
    content_y: f64,
    cell_area_w: f64,
    line_height: f64,
    char_width: f64,
    theme: &Theme,
) {
    for (row_idx, row) in rows.iter().enumerate() {
        let row_y = content_y + row_idx as f64 * line_height;
        let mut cell_x = x;
        for cell in row {
            if cell_x + char_width > x + cell_area_w {
                break;
            }
            let (br, bg, bb) = cell.bg;
            let (fr, fg2, fb) = cell.fg;

            // Cell background
            let (draw_br, draw_bg, draw_bb) = if cell.is_cursor {
                // Cursor: inverted colors (white on normal bg)
                (fr, fg2, fb)
            } else if cell.is_find_active {
                // Active find match: orange background
                (255u8, 165u8, 0u8)
            } else if cell.is_find_match {
                // Other find matches: dark amber background
                (100u8, 80u8, 20u8)
            } else if cell.selected {
                // Selection highlight (use theme selection color)
                let (sr, sg, sb) = theme.selection.to_cairo();
                ((sr * 255.0) as u8, (sg * 255.0) as u8, (sb * 255.0) as u8)
            } else {
                (br, bg, bb)
            };
            cr.set_source_rgb(
                draw_br as f64 / 255.0,
                draw_bg as f64 / 255.0,
                draw_bb as f64 / 255.0,
            );
            cr.rectangle(cell_x, row_y, char_width, line_height);
            cr.fill().ok();

            // Cell foreground text
            let ch_str = cell.ch.to_string();
            if cell.ch != ' ' {
                let (draw_fr, draw_fg, draw_fb) = if cell.is_cursor {
                    (br, bg, bb) // inverted for cursor
                } else if cell.is_find_active {
                    (0u8, 0u8, 0u8) // black text on orange
                } else {
                    (fr, fg2, fb)
                };
                cr.set_source_rgb(
                    draw_fr as f64 / 255.0,
                    draw_fg as f64 / 255.0,
                    draw_fb as f64 / 255.0,
                );

                // Apply bold/italic via Pango attributes if needed
                let attrs = AttrList::new();
                if cell.bold {
                    attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
                }
                if cell.italic {
                    attrs.insert(pango::AttrInt::new_style(pango::Style::Italic));
                }
                if cell.underline {
                    attrs.insert(pango::AttrInt::new_underline(pango::Underline::Single));
                }
                layout.set_attributes(Some(&attrs));
                layout.set_text(&ch_str);
                cr.move_to(cell_x, row_y);
                pangocairo::show_layout(cr, layout);
                layout.set_attributes(None);
            }

            cell_x += char_width;
        }
    }
}

/// Translate a GTK key event to PTY input bytes.
/// Returns an empty vec for keys that have no PTY mapping.
fn gtk_key_to_pty_bytes(key_name: &str, unicode: Option<char>, ctrl: bool) -> Vec<u8> {
    if ctrl {
        // Ctrl+char → byte & 0x1f
        if let Some(ch) = unicode {
            let b = ch as u8;
            if b.is_ascii() {
                return vec![b & 0x1f];
            }
        }
        // Named control keys when ctrl held
        return match key_name {
            "Return" => b"\r".to_vec(),
            "BackSpace" => b"\x7f".to_vec(),
            "Tab" => b"\t".to_vec(),
            _ => vec![],
        };
    }

    match key_name {
        "Return" | "KP_Enter" => b"\r".to_vec(),
        "BackSpace" => b"\x7f".to_vec(),
        "Tab" => b"\t".to_vec(),
        "Escape" => b"\x1b".to_vec(),
        "Up" | "KP_Up" => b"\x1b[A".to_vec(),
        "Down" | "KP_Down" => b"\x1b[B".to_vec(),
        "Right" | "KP_Right" => b"\x1b[C".to_vec(),
        "Left" | "KP_Left" => b"\x1b[D".to_vec(),
        "Home" | "KP_Home" => b"\x1b[H".to_vec(),
        "End" | "KP_End" => b"\x1b[F".to_vec(),
        "Delete" | "KP_Delete" => b"\x1b[3~".to_vec(),
        "Insert" | "KP_Insert" => b"\x1b[2~".to_vec(),
        "Page_Up" | "KP_Page_Up" => b"\x1b[5~".to_vec(),
        "Page_Down" | "KP_Page_Down" => b"\x1b[6~".to_vec(),
        "F1" => b"\x1bOP".to_vec(),
        "F2" => b"\x1bOQ".to_vec(),
        "F3" => b"\x1bOR".to_vec(),
        "F4" => b"\x1bOS".to_vec(),
        "F5" => b"\x1b[15~".to_vec(),
        "F6" => b"\x1b[17~".to_vec(),
        "F7" => b"\x1b[18~".to_vec(),
        "F8" => b"\x1b[19~".to_vec(),
        "F9" => b"\x1b[20~".to_vec(),
        "F10" => b"\x1b[21~".to_vec(),
        "F11" => b"\x1b[23~".to_vec(),
        "F12" => b"\x1b[24~".to_vec(),
        _ => {
            // Regular printable character
            if let Some(ch) = unicode {
                ch.to_string().into_bytes()
            } else {
                vec![]
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_status_line(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    left: &str,
    right: &str,
    width: f64,
    y: f64,
    line_height: f64,
) {
    let (br, bg, bb) = theme.status_bg.to_cairo();
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().unwrap();

    layout.set_attributes(None);

    let (fr, fg, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);

    layout.set_text(left);
    cr.move_to(0.0, y);
    pangocairo::show_layout(cr, layout);

    layout.set_text(right);
    let (right_w, _) = layout.pixel_size();
    cr.move_to(width - right_w as f64, y);
    pangocairo::show_layout(cr, layout);
}

fn draw_command_line(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    cmd: &CommandLineData,
    width: f64,
    y: f64,
    line_height: f64,
) {
    let (br, bg, bb) = theme.command_bg.to_cairo();
    cr.set_source_rgb(br, bg, bb);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().unwrap();

    if !cmd.text.is_empty() {
        layout.set_text(&cmd.text);
        layout.set_attributes(None);

        let (fr, fg, fb) = theme.command_fg.to_cairo();
        cr.set_source_rgb(fr, fg, fb);

        if cmd.right_align {
            let (text_w, _) = layout.pixel_size();
            cr.move_to(width - text_w as f64, y);
        } else {
            cr.move_to(0.0, y);
        }
        pangocairo::show_layout(cr, layout);
    }

    // Command-line insert cursor
    if cmd.show_cursor {
        layout.set_text(&cmd.cursor_anchor_text);
        layout.set_attributes(None);
        let (text_w, _) = layout.pixel_size();
        let (cr_r, cr_g, cr_b) = theme.cursor.to_cairo();
        cr.set_source_rgb(cr_r, cr_g, cr_b);
        cr.rectangle(text_w as f64, y, 2.0, line_height);
        cr.fill().unwrap();
    }
}

fn draw_menu_bar(
    cr: &Context,
    data: &render::MenuBarData,
    theme: &Theme,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    // VSCode title bar background: #3c3c3c
    cr.set_source_rgb(0.235, 0.235, 0.235);
    cr.rectangle(x, y, width, height);
    let _ = cr.fill();

    let pango_ctx = pangocairo::create_context(cr);
    let font_desc = pango::FontDescription::from_string(UI_FONT);
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&font_desc));

    let (fr, fg, fb) = theme.status_fg.to_cairo();
    cr.set_source_rgb(fr, fg, fb);

    // Menu labels
    let mut cursor_x = x + 8.0;

    for (idx, (name, _, _)) in render::MENU_STRUCTURE.iter().enumerate() {
        let is_open = data.open_menu_idx == Some(idx);
        if is_open {
            let (ar, ag, ab) = theme.keyword.to_cairo();
            cr.set_source_rgb(ar, ag, ab);
        } else {
            cr.set_source_rgb(fr, fg, fb);
        }
        layout.set_text(name);
        let (lw, lh) = layout.pixel_size();
        cr.move_to(cursor_x, y + (height - lh as f64) / 2.0);
        pangocairo::show_layout(cr, &layout);
        cursor_x += lw as f64 + 10.0;
    }

    // Title centered in remaining space (dimmed)
    if !data.title.is_empty() {
        let (dr, dg, db) = theme.line_number_fg.to_cairo();
        cr.set_source_rgb(dr, dg, db);
        layout.set_text(&data.title);
        let (title_w, title_h) = layout.pixel_size();
        let title_x = (x + width - title_w as f64) / 2.0 + x / 2.0;
        cr.move_to(
            title_x.max(cursor_x + 8.0),
            y + (height - title_h as f64) / 2.0,
        );
        pangocairo::show_layout(cr, &layout);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_menu_dropdown(
    cr: &Context,
    data: &render::MenuBarData,
    theme: &Theme,
    x: f64,
    anchor_y: f64,
    _width: f64,
    _height: f64,
    line_height: f64,
) {
    if data.open_items.is_empty() {
        return;
    }

    let pango_ctx = pangocairo::create_context(cr);
    let font_desc = pango::FontDescription::from_string(UI_FONT);
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&font_desc));

    // Compute popup_x the same way draw_menu_bar does: measure each label
    let mut popup_x = x + 8.0;
    if let Some(midx) = data.open_menu_idx {
        for i in 0..midx {
            if let Some((name, _, _)) = render::MENU_STRUCTURE.get(i) {
                layout.set_text(name);
                let (lw, _) = layout.pixel_size();
                popup_x += lw as f64 + 10.0;
            }
        }
    }
    let item_count = data.open_items.len() as f64;
    let popup_width = 220.0_f64;
    let popup_height = (item_count + 1.0) * line_height;
    let popup_y = anchor_y;

    // Background — VSCode dropdown: #3c3c3c
    cr.set_source_rgb(0.235, 0.235, 0.235);
    cr.rectangle(popup_x, popup_y, popup_width, popup_height);
    let _ = cr.fill();

    // Border — VSCode: slightly lighter #454545
    cr.set_source_rgb(0.271, 0.271, 0.271);
    cr.rectangle(popup_x, popup_y, popup_width, popup_height);
    let _ = cr.stroke();

    // Items
    let (fr, fg_c, fb) = theme.foreground.to_cairo();
    let (sr, sg, sb) = theme.line_number_fg.to_cairo();
    cr.set_source_rgb(fr, fg_c, fb);
    let mut item_y = popup_y + line_height * 0.1;
    for item in &data.open_items {
        item_y += line_height;
        if item.separator {
            cr.set_source_rgb(sr, sg, sb);
            cr.move_to(popup_x + 4.0, item_y - line_height * 0.5);
            cr.line_to(popup_x + popup_width - 4.0, item_y - line_height * 0.5);
            let _ = cr.stroke();
            cr.set_source_rgb(fr, fg_c, fb);
        } else {
            layout.set_text(item.label);
            let (_, lh) = layout.pixel_size();
            cr.set_source_rgb(fr, fg_c, fb);
            cr.move_to(popup_x + 8.0, item_y - lh as f64 * 0.9);
            pangocairo::show_layout(cr, &layout);
            let sc = if data.is_vscode_mode && !item.vscode_shortcut.is_empty() {
                item.vscode_shortcut
            } else {
                item.shortcut
            };
            if !sc.is_empty() {
                layout.set_text(sc);
                let (sc_w, _) = layout.pixel_size();
                cr.set_source_rgb(sr, sg, sb);
                cr.move_to(
                    popup_x + popup_width - sc_w as f64 - 8.0,
                    item_y - lh as f64 * 0.9,
                );
                pangocairo::show_layout(cr, &layout);
                cr.set_source_rgb(fr, fg_c, fb);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_source_control_panel(
    cr: &Context,
    layout: &pango::Layout,
    screen: &render::ScreenLayout,
    theme: &Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    line_height: f64,
) {
    let Some(ref sc) = screen.source_control else {
        return;
    };

    let (bg_r, bg_g, bg_b) = theme.completion_bg.to_cairo();
    let (hdr_r, hdr_g, hdr_b) = theme.status_bg.to_cairo();
    let (fg_r, fg_g, fg_b) = theme.status_fg.to_cairo();
    let (dim_r, dim_g, dim_b) = theme.line_number_fg.to_cairo();
    let (sel_r, sel_g, sel_b) = theme.fuzzy_selected_bg.to_cairo();
    let (add_r, add_g, add_b) = theme.diff_added_bg.to_cairo();
    let (del_r, del_g, del_b) = theme.diff_removed_bg.to_cairo();

    // Background
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();

    layout.set_attributes(None);

    let mut row: usize = 0;

    // ── Row 0: header "SOURCE CONTROL" ──────────────────────────────────────
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(x, y + row as f64 * line_height, w, line_height);
    cr.fill().ok();

    let branch_str = format!(
        "  \u{e702} SOURCE CONTROL   {}  ↑{}↓{}",
        sc.branch, sc.ahead, sc.behind
    );
    cr.set_source_rgb(fg_r, fg_g, fg_b);
    layout.set_text(&branch_str);
    let (lw, lh) = layout.pixel_size();
    cr.move_to(
        x + 2.0,
        y + row as f64 * line_height + (line_height - lh as f64) / 2.0,
    );
    pangocairo::show_layout(cr, layout);
    let _ = (lw, lh);
    row += 1;

    // ── Row 1: commit input row ───────────────────────────────────────────────
    if row as f64 * line_height < h {
        let (inp_bg_r, inp_bg_g, inp_bg_b) = if sc.commit_input_active {
            theme.fuzzy_selected_bg.to_cairo()
        } else {
            theme.completion_bg.to_cairo()
        };
        cr.set_source_rgb(inp_bg_r, inp_bg_g, inp_bg_b);
        cr.rectangle(x, y + row as f64 * line_height, w, line_height);
        cr.fill().ok();
        let prompt = if sc.commit_input_active {
            format!(" \u{f044}  {}|", sc.commit_message)
        } else if sc.commit_message.is_empty() {
            " \u{f044}  Message (press c to type)".to_string()
        } else {
            format!(" \u{f044}  {}", sc.commit_message)
        };
        let (prompt_r, prompt_g, prompt_b) = if sc.commit_input_active {
            (fg_r, fg_g, fg_b)
        } else {
            (dim_r, dim_g, dim_b)
        };
        cr.set_source_rgb(prompt_r, prompt_g, prompt_b);
        layout.set_text(&prompt);
        let (_, lh2) = layout.pixel_size();
        cr.move_to(
            x + 2.0,
            y + row as f64 * line_height + (line_height - lh2 as f64) / 2.0,
        );
        pangocairo::show_layout(cr, layout);
        row += 1;
    }

    // ── Row 2: action buttons ────────────────────────────────────────────────
    if row as f64 * line_height < h {
        // Commit gets ~50% of the width (with label text).
        // Push / Pull / Sync get equal shares of the remaining width, icon only.
        let commit_w = w / 2.0;
        let remain_w = w - commit_w;
        let icon_w = remain_w / 3.0;
        let btn_y_base = y + row as f64 * line_height;

        // Helper: fill and label one button segment.
        let draw_btn = |bx: f64, seg_w: f64, text: &str, focused: bool| {
            let (fill_r, fill_g, fill_b) = if focused {
                (hdr_r, hdr_g, hdr_b)
            } else {
                (bg_r, bg_g, bg_b)
            };
            let (text_r, text_g, text_b) = if focused {
                (fg_r, fg_g, fg_b)
            } else {
                (dim_r, dim_g, dim_b)
            };
            cr.set_source_rgb(fill_r, fill_g, fill_b);
            cr.rectangle(bx, btn_y_base, seg_w, line_height);
            cr.fill().ok();
            cr.set_source_rgb(text_r, text_g, text_b);
            layout.set_text(text);
            let (_, lh_btn) = layout.pixel_size();
            cr.move_to(bx + 2.0, btn_y_base + (line_height - lh_btn as f64) / 2.0);
            pangocairo::show_layout(cr, layout);
        };

        // Commit button (index 0)
        draw_btn(
            x,
            commit_w,
            " \u{e729} Commit",
            sc.button_focused == Some(0),
        );
        // Push (index 1)
        draw_btn(
            x + commit_w,
            icon_w,
            " \u{f093}",
            sc.button_focused == Some(1),
        );
        // Pull (index 2)
        draw_btn(
            x + commit_w + icon_w,
            icon_w,
            " \u{f019}",
            sc.button_focused == Some(2),
        );
        // Sync (index 3): fill to the right edge
        draw_btn(
            x + commit_w + icon_w * 2.0,
            w - (commit_w + icon_w * 2.0),
            " \u{f021}",
            sc.button_focused == Some(3),
        );

        row += 1;
    }

    // Helper to draw a section
    let draw_section = |cr: &Context,
                        layout: &pango::Layout,
                        title: &str,
                        items: &[String],
                        expanded: bool,
                        base_row: &mut usize,
                        flat_start: usize,
                        selected: usize| {
        let arrow = if expanded { "▼" } else { "▶" };
        let header_text = format!("  {} {} ({})", arrow, title, items.len());
        cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
        cr.rectangle(x, y + *base_row as f64 * line_height, w, line_height);
        cr.fill().ok();
        cr.set_source_rgb(fg_r, fg_g, fg_b);
        layout.set_text(&header_text);
        let (_, lh) = layout.pixel_size();
        cr.move_to(
            x + 2.0,
            y + *base_row as f64 * line_height + (line_height - lh as f64) / 2.0,
        );
        pangocairo::show_layout(cr, layout);
        *base_row += 1;

        if expanded {
            for (i, item) in items.iter().enumerate() {
                let flat_idx = flat_start + 1 + i; // +1 for section header
                let is_sel = flat_idx == selected;
                if is_sel {
                    cr.set_source_rgb(sel_r, sel_g, sel_b);
                    cr.rectangle(x, y + *base_row as f64 * line_height, w, line_height);
                    cr.fill().ok();
                }
                cr.set_source_rgb(
                    if is_sel { fg_r } else { dim_r },
                    if is_sel { fg_g } else { dim_g },
                    if is_sel { fg_b } else { dim_b },
                );
                layout.set_text(&format!("    {}", item));
                let (_, lh) = layout.pixel_size();
                cr.move_to(
                    x + 2.0,
                    y + *base_row as f64 * line_height + (line_height - lh as f64) / 2.0,
                );
                pangocairo::show_layout(cr, layout);
                *base_row += 1;
                if y + *base_row as f64 * line_height > y + h {
                    break;
                }
            }
        }
    };

    // Compute flat start offsets
    let staged_items: Vec<String> = sc
        .staged
        .iter()
        .map(|f| format!("{} {}", f.status_char, f.path))
        .collect();
    let unstaged_items: Vec<String> = sc
        .unstaged
        .iter()
        .map(|f| format!("{} {}", f.status_char, f.path))
        .collect();
    let wt_items: Vec<String> = sc
        .worktrees
        .iter()
        .map(|wt| {
            let marker = if wt.is_current { "\u{2714} " } else { "  " };
            format!("{}{} {}", marker, wt.branch, wt.path)
        })
        .collect();

    let staged_flat_start = 0usize;
    let unstaged_flat_start = 1 + if sc.sections_expanded[0] {
        sc.staged.len()
    } else {
        0
    };
    let wt_flat_start = unstaged_flat_start
        + 1
        + if sc.sections_expanded[1] {
            sc.unstaged.len()
        } else {
            0
        };
    let show_worktrees = sc.worktrees.len() > 1;
    let log_flat_start = if show_worktrees {
        wt_flat_start
            + 1
            + if sc.sections_expanded[2] {
                sc.worktrees.len()
            } else {
                0
            }
    } else {
        wt_flat_start
    };

    // Draw staged section
    if row as f64 * line_height < h {
        draw_section(
            cr,
            layout,
            "STAGED CHANGES",
            &staged_items,
            sc.sections_expanded[0],
            &mut row,
            staged_flat_start,
            sc.selected,
        );
    }

    // Color hint for diff-add
    let _ = (add_r, add_g, add_b, del_r, del_g, del_b);

    // Draw unstaged section
    if row as f64 * line_height < h {
        draw_section(
            cr,
            layout,
            "CHANGES",
            &unstaged_items,
            sc.sections_expanded[1],
            &mut row,
            unstaged_flat_start,
            sc.selected,
        );
    }

    // Draw worktrees section (only when there are linked worktrees beyond the main one).
    if row as f64 * line_height < h && show_worktrees {
        draw_section(
            cr,
            layout,
            "WORKTREES",
            &wt_items,
            sc.sections_expanded[2],
            &mut row,
            wt_flat_start,
            sc.selected,
        );
    }

    // Draw log section (RECENT COMMITS) — always present.
    if row as f64 * line_height < h {
        let log_items: Vec<String> = sc
            .log
            .iter()
            .map(|e| format!("{} {}", e.hash, e.message))
            .collect();
        draw_section(
            cr,
            layout,
            "\u{f417} RECENT COMMITS",
            &log_items,
            sc.sections_expanded[3],
            &mut row,
            log_flat_start,
            sc.selected,
        );
    }
}

fn draw_debug_toolbar(
    cr: &Context,
    toolbar: &render::DebugToolbarData,
    theme: &Theme,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let (r, g, b) = theme.status_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(x, y, width, height);
    let _ = cr.fill();

    let (fr, fg_c, fb) = if toolbar.session_active {
        theme.status_fg.to_cairo()
    } else {
        theme.line_number_fg.to_cairo()
    };
    cr.set_source_rgb(fr, fg_c, fb);

    let mut cursor_x = x + 8.0;
    for (idx, btn) in toolbar.buttons.iter().enumerate() {
        if idx == 4 {
            // Separator
            let (dr, dg, db) = theme.line_number_fg.to_cairo();
            cr.set_source_rgb(dr, dg, db);
            cr.move_to(cursor_x, y + 2.0);
            cr.line_to(cursor_x, y + height - 2.0);
            let _ = cr.stroke();
            cr.set_source_rgb(fr, fg_c, fb);
            cursor_x += 8.0;
        }
        cr.move_to(cursor_x, y + height * 0.7);
        let text = format!("{} ({}) ", btn.icon, btn.key_hint);
        let _ = cr.show_text(&text);
        cursor_x += text.len() as f64 * 7.0;
    }
}

/// Result of converting pixel coordinates to buffer position.
enum ClickTarget {
    /// Click was in the tab bar, tab already switched.
    TabBar,
    /// Click was in gutter — fold already toggled.
    Gutter,
    /// Click resolved to a buffer position in a specific window.
    BufferPos(core::WindowId, usize, usize),
    /// Click was outside any actionable area.
    None,
}

/// Convert pixel (x, y) to a buffer position (window_id, line, col).
/// Also handles tab-bar clicks and gutter fold toggles.
fn pixel_to_click_target(
    engine: &mut Engine,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> ClickTarget {
    use gtk4::cairo::{Context as CairoContext, Format, ImageSurface};

    let surface = ImageSurface::create(Format::Rgb24, 1, 1).unwrap();
    let cr = CairoContext::new(&surface).unwrap();

    let pango_ctx = pangocairo::create_context(&cr);
    let font_str = format!(
        "{} {}",
        engine.settings.font_family, engine.settings.font_size
    );
    let font_desc = FontDescription::from_string(&font_str);
    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;

    let tab_bar_height = line_height;

    // Check if click is in tab bar (menu bar is a separate widget above, not in drawing_area)
    if y < tab_bar_height {
        let layout = pangocairo::create_layout(&cr);
        layout.set_font_description(Some(&font_desc));

        let normal_font = font_desc.clone();
        let mut italic_font = font_desc.clone();
        italic_font.set_style(pango::Style::Italic);

        let mut tab_x = 0.0;
        for (i, tab) in engine.tabs.iter().enumerate() {
            let window_id = tab.active_window;
            let (name, is_preview) = if let Some(window) = engine.windows.get(&window_id) {
                if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                    let dirty = if state.dirty { "*" } else { "" };
                    (
                        format!(" {}: {}{} ", i + 1, state.display_name(), dirty),
                        state.preview,
                    )
                } else {
                    (format!(" {}: [No Name] ", i + 1), false)
                }
            } else {
                (format!(" {}: [No Name] ", i + 1), false)
            };

            if is_preview {
                layout.set_font_description(Some(&italic_font));
            } else {
                layout.set_font_description(Some(&normal_font));
            }

            layout.set_text(&name);
            let (tab_width, _) = layout.pixel_size();

            if x >= tab_x && x < tab_x + tab_width as f64 {
                engine.goto_tab(i);
                return ClickTarget::TabBar;
            }

            tab_x += tab_width as f64 + 2.0;
        }
        return ClickTarget::TabBar;
    }

    let status_bar_height = line_height * 2.0;
    let content_bounds = WindowRect::new(
        0.0,
        tab_bar_height,
        width,
        height - tab_bar_height - status_bar_height,
    );

    if y >= content_bounds.y + content_bounds.height {
        return ClickTarget::None;
    }

    let window_rects = engine.calculate_window_rects(content_bounds);
    let clicked_window = window_rects.iter().find(|(_, rect)| {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    });

    let (window_id, rect) = match clicked_window {
        Some((id, r)) => (*id, r),
        None => return ClickTarget::None,
    };

    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return ClickTarget::None,
    };

    let buffer_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return ClickTarget::None,
    };

    let buffer = &buffer_state.buffer;
    let view = &window.view;

    let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
    let total_lines = buffer.content.len_lines();
    let has_git = !buffer_state.git_diff.is_empty();
    let has_bp_click = {
        let key = buffer_state
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        !engine
            .dap_breakpoints
            .get(&key)
            .is_none_or(|v| v.is_empty())
            || engine.dap_session_active
    };
    let gutter_char_width = render::calculate_gutter_cols(
        engine.settings.line_numbers,
        total_lines,
        char_width,
        has_git,
        has_bp_click,
    );
    let gutter_width = gutter_char_width as f64 * char_width;

    let per_window_status = if engine.windows.len() > 1 {
        line_height
    } else {
        0.0
    };
    let text_area_height = rect.height - per_window_status;

    if y >= rect.y + text_area_height {
        return ClickTarget::None;
    }

    let relative_y = y - rect.y;
    let view_row = (relative_y / line_height).floor() as usize;
    let line = view_row_to_buf_line(view, view.scroll_top, view_row, total_lines);

    // Gutter click
    if x >= rect.x && x < rect.x + gutter_width && gutter_width > 0.0 {
        // If the breakpoint column is visible and the click landed in its cell
        // (always the leftmost gutter character), toggle the breakpoint instead
        // of the fold.
        if has_bp_click && x < rect.x + char_width {
            let file = buffer_state
                .file_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            engine.dap_toggle_breakpoint(&file, line as u64 + 1);
        } else {
            engine.toggle_fold_at_line(line);
        }
        return ClickTarget::Gutter;
    }

    let relative_x = x - (rect.x + gutter_width);
    let line = line.min(buffer.content.len_lines().saturating_sub(1));
    let line_text = buffer.content.line(line).to_string();

    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&font_desc));

    let mut col = 0;

    if !line_text.is_empty() {
        let expanded_text = line_text.replace('\t', "    ");
        layout.set_text(&expanded_text);

        let mut best_col = 0;
        let mut prev_width = 0.0;

        let char_indices: Vec<(usize, char)> = expanded_text.char_indices().collect();

        for i in 0..char_indices.len() {
            let (byte_idx, _) = char_indices[i];

            let next_byte_idx = if i + 1 < char_indices.len() {
                char_indices[i + 1].0
            } else {
                expanded_text.len()
            };

            layout.set_text(&expanded_text[..next_byte_idx]);
            let (curr_width, _) = layout.pixel_size();
            let curr_width_f64 = curr_width as f64;

            if relative_x >= prev_width && relative_x < curr_width_f64 {
                best_col = byte_idx;
                break;
            }

            prev_width = curr_width_f64;

            if i == char_indices.len() - 1 && relative_x >= curr_width_f64 {
                best_col = next_byte_idx;
            }
        }

        if relative_x < 0.0 {
            best_col = 0;
        }

        let mut original_col = 0;
        let mut expanded_pos = 0;
        for ch in line_text.chars() {
            if expanded_pos >= best_col {
                break;
            }
            if ch == '\t' {
                expanded_pos += 4;
            } else {
                expanded_pos += ch.len_utf8();
            }
            original_col += 1;
        }
        col = original_col;
    }

    ClickTarget::BufferPos(window_id, line, col)
}

/// Handle mouse click by converting coordinates to buffer position.
fn handle_mouse_click(engine: &mut Engine, x: f64, y: f64, width: f64, height: f64) {
    if let ClickTarget::BufferPos(wid, line, col) =
        pixel_to_click_target(engine, x, y, width, height)
    {
        engine.mouse_click(wid, line, col);
    }
}

/// Handle mouse double-click — select word at position.
fn handle_mouse_double_click(engine: &mut Engine, x: f64, y: f64, width: f64, height: f64) {
    if let ClickTarget::BufferPos(wid, line, col) =
        pixel_to_click_target(engine, x, y, width, height)
    {
        engine.mouse_double_click(wid, line, col);
    }
}

/// Handle mouse drag — extend visual selection.
fn handle_mouse_drag(engine: &mut Engine, x: f64, y: f64, width: f64, height: f64) {
    if let ClickTarget::BufferPos(wid, line, col) =
        pixel_to_click_target(engine, x, y, width, height)
    {
        engine.mouse_drag(wid, line, col);
    }
}

fn load_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(
        "
        /* Custom titlebar — VSCode title bar color #3c3c3c.
           CSD provides edge resize handles; WindowHandle enables drag-to-move. */
        .custom-titlebar {
            background-color: #3c3c3c;
            min-height: 0;
            padding: 0;
            margin: 0;
            border: none;
            box-shadow: none;
        }
        headerbar {
            min-height: 0;
            padding: 0;
            margin: 0;
            border: none;
            box-shadow: none;
            background: transparent;
        }

        /* VSCode UI font stack — 'Segoe UI' on Windows, 'Ubuntu' on Ubuntu,
           system-ui/sans elsewhere.  13px matches VSCode default UI size. */
        .sidebar,
        .sidebar *,
        .sidebar-header,
        .sidebar-title,
        .search-results-list,
        .search-results-list *,
        .search-file-header {
            font-family: 'Segoe UI', system-ui, -apple-system, 'Ubuntu', 'Droid Sans', sans-serif;
            font-size: 13px;
        }

        /* Window control buttons — VSCode style (transparent bg, subtle hover) */
        .window-control {
            background: transparent;
            border: none;
            border-radius: 0;
            color: #cccccc;
            font-size: 13px;
            padding: 0;
            min-width: 46px;
            min-height: 30px;
        }
        .window-control:hover {
            background-color: rgba(255, 255, 255, 0.12);
            color: #ffffff;
        }
        .window-control:active {
            background-color: rgba(255, 255, 255, 0.08);
        }
        /* Close button: red on hover, matching Windows/VSCode */
        .window-control:last-child:hover {
            background-color: #e81123;
            color: #ffffff;
        }
        .window-control:last-child:active {
            background-color: #f1707a;
            color: #ffffff;
        }

        /* Activity Bar */
        .activity-bar {
            background-color: #252526;
            border-right: 1px solid #3e3e42;
        }

        .activity-button {
            background: transparent;
            border: none;
            border-radius: 0;
            font-size: 24px;
            color: #cccccc;
            padding: 0;
        }
        
        .activity-button:hover {
            background-color: #2a2d2e;
        }
        
        .activity-button.active {
            background-color: #094771;
            border-left: 2px solid #0e639c;
        }
        
        .activity-button:disabled {
            opacity: 0.4;
        }
        
        /* Sidebar */
        .sidebar {
            background-color: #252526;
            border-right: 1px solid #3e3e42;
        }
        
        .sidebar label {
            color: #cccccc;
        }
        
        /* Explorer Toolbar */
        .explorer-toolbar {
            background-color: #2d2d30;
            border-bottom: 1px solid #3e3e42;
        }
        
        .explorer-toolbar button {
            background: transparent;
            border: 1px solid transparent;
            border-radius: 2px;
            color: #cccccc;
            font-size: 16px;
            padding: 4px;
        }
        
        .explorer-toolbar button:hover {
            background-color: #2a2d2e;
            border-color: #0e639c;
        }
        
        .explorer-toolbar button:active {
            background-color: #094771;
        }
        
        /* Tree View - VSCode Style */
        treeview {
            background-color: #252526;
            color: #cccccc;
            border: none;
            font-family: 'Segoe UI', system-ui, -apple-system, 'Ubuntu', 'Droid Sans', sans-serif;
            font-size: 13px;
            outline: none;
        }
        
        /* Selection - VSCode style with left accent */
        treeview:selected {
            background-color: rgba(9, 71, 113, 0.3);
            border-left: 3px solid #0e639c;
        }
        
        treeview:selected:focus {
            background-color: rgba(9, 71, 113, 0.5);
        }
        
        /* Hover - very subtle */
        treeview row:hover {
            background-color: rgba(42, 45, 46, 0.5);
        }
        
        /* Better padding and spacing */
        treeview row {
            padding: 4px 8px;
            min-height: 22px;
        }
        
        /* Expander (arrow) styling - more subtle */
        treeview expander {
            min-width: 16px;
            min-height: 16px;
        }
        
        treeview expander:checked {
            color: #cccccc;
        }
        
        treeview expander:not(:checked) {
            color: #999999;
        }

        /* Thin overlay scrollbars */
        scrollbar {
            background: transparent;
            transition: opacity 200ms ease-out;
        }

        scrollbar.vertical {
            min-width: 10px;
        }

        scrollbar.horizontal {
            min-height: 10px;
            max-height: 10px;
        }

        scrollbar.horizontal slider {
            min-height: 10px;
            max-height: 10px;
        }

        scrollbar slider {
            min-width: 10px;
            min-height: 40px;
            background: rgba(255, 255, 255, 0.3);
            border-radius: 5px;
        }

        scrollbar slider:hover {
            background: rgba(255, 255, 255, 0.5);
        }

        scrollbar slider:active {
            background: rgba(255, 255, 255, 0.7);
        }

        /* Scrollbars — subtle but always visible */
        scrollbar:not(:hover):not(:active) {
            opacity: 0.4;
        }

        /* Search results ListBox — GTK4 CSS node for GtkListBox is 'list' */
        .search-results-list {
            background-color: #252526;
            color: #cccccc;
        }

        .search-results-list > row {
            background-color: #252526;
            color: #cccccc;
            padding: 2px 4px;
            min-height: 20px;
        }

        .search-results-list > row:hover {
            background-color: #2a2d2e;
        }

        .search-results-list > row:selected,
        .search-results-list > row:selected:focus {
            background-color: rgba(9, 71, 113, 0.5);
            color: #cccccc;
        }

        /* Search results ScrolledWindow background */
        .search-results-scroll {
            background-color: #252526;
        }

        /* Labels inside search results list */
        .search-results-list label {
            color: #cccccc;
            background-color: transparent;
        }

        .search-results-list > row:selected label,
        .search-results-list > row:selected:focus label {
            color: #cccccc;
        }

        /* File-header rows in search results */
        .search-file-header {
            color: #569cd6;
            font-weight: bold;
            font-size: 12px;
        }

        /* Search input entry inside sidebar */
        .sidebar entry {
            background-color: #3c3c3c;
            color: #cccccc;
            border: 1px solid #3e3e42;
            border-radius: 2px;
            padding: 4px;
        }

        .sidebar entry:focus {
            border-color: #0e639c;
        }

        /* Search toggle buttons (Aa / Ab| / .*) */
        .search-toggle-btn {
            background: transparent;
            color: #808080;
            border: 1px solid #3e3e42;
            border-radius: 2px;
            padding: 2px 6px;
            min-width: 0;
            min-height: 0;
            font-size: 12px;
        }
        .search-toggle-btn:hover {
            background-color: #2a2d2e;
        }
        .search-toggle-btn:checked {
            background-color: #0e639c;
            color: #ffffff;
            border-color: #0e639c;
        }

        /* Horizontal editor scrollbar — overlays the bottom of editor content.
           Semi-transparent like VSCode so text beneath is still visible.
           min-height/min-width: 0 prevents the GTK theme from forcing the
           widget taller than our height_request(10), which would push it
           into the status line. */
        .h-editor-scrollbar {
            background: transparent;
            border: none;
            padding: 0;
            min-height: 0;
            min-width: 0;
        }
        .h-editor-scrollbar trough {
            background: transparent;
            border: none;
            min-height: 0;
            min-width: 0;
            padding: 0;
        }
        .h-editor-scrollbar slider {
            background: rgba(100, 100, 100, 0.45);
            border-radius: 2px;
            min-height: 0;
            min-width: 20px;
            margin: 1px 0;
        }
        .h-editor-scrollbar slider:hover {
            background: rgba(150, 150, 150, 0.7);
        }

        /* Find/Replace Dialog */
        .find-dialog {
            background-color: #2d2d30;
            border: 1px solid #3e3e42;
            border-radius: 4px;
            padding: 12px;
        }

        .find-dialog entry {
            background-color: #3c3c3c;
            color: #cccccc;
            padding: 6px;
            border: 1px solid #3e3e42;
            border-radius: 2px;
        }

        .find-dialog button {
            background: transparent;
            border: 1px solid #3e3e42;
            color: #cccccc;
            padding: 6px 12px;
            border-radius: 2px;
        }

        .find-dialog button:hover {
            background-color: #2a2d2e;
        }

        .find-match-count {
            color: #858585;
            font-size: 11px;
        }
        ",
    );

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Build file tree with a root folder node at the top (like VSCode).
fn build_file_tree_with_root(store: &gtk4::TreeStore, root: &Path) {
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string())
        .to_uppercase();
    let root_iter = store.insert_with_values(
        None,
        None,
        &[
            (0, &""),
            (1, &root_name),
            (2, &root.to_string_lossy().to_string()),
        ],
    );
    build_file_tree(store, Some(&root_iter), root);
}

/// Build file tree recursively
/// TreeStore columns: [Icon(String), Name(String), FullPath(String)]
fn build_file_tree(store: &gtk4::TreeStore, parent: Option<&gtk4::TreeIter>, path: &Path) {
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return, // Handle permission errors silently
    };

    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();

    // Sort: directories first, then files, both alphabetically
    entries.sort_by(|a, b| {
        let a_is_dir = a.path().is_dir();
        let b_is_dir = b.path().is_dir();

        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files (optional - can make configurable later)
        if name.starts_with('.') && name != "." && name != ".." {
            continue; // Skip dotfiles for now
        }

        let is_dir = path.is_dir();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let icon = if is_dir {
            ""
        } else {
            crate::icons::file_icon(ext)
        };

        let iter = store.insert_with_values(
            parent,
            None,
            &[
                (0, &icon),
                (1, &name),
                (2, &path.to_string_lossy().to_string()),
            ],
        );

        // Recursively add subdirectories
        if is_dir {
            // Limit recursion depth to prevent hanging on deep trees
            let depth = parent.map_or(0, |_| 1); // Simple depth tracking
            if depth < 10 {
                build_file_tree(store, Some(&iter), &path);
            }
        }
    }
}

/// Get the parent directory for creating a new file/folder, based on the
/// currently selected tree row. If a directory is selected, use it. If a
/// file is selected, use its parent. Fallback: cwd.
fn selected_parent_dir(tv: &gtk4::TreeView) -> PathBuf {
    if let Some((model, iter)) = tv.selection().selected() {
        if let Ok(s) = model.get_value(&iter, 2).get::<String>() {
            if !s.is_empty() {
                let p = PathBuf::from(s);
                if p.is_dir() {
                    return p;
                }
                if let Some(parent) = p.parent() {
                    return parent.to_path_buf();
                }
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Show a modal dialog with a text entry prompting for a name.
/// `title` is the dialog title, `prefill` pre-populates the entry,
/// and `on_accept` is called with the entered text when the user confirms.
fn show_name_prompt_dialog<F: Fn(String) + 'static>(title: &str, prefill: &str, on_accept: F) {
    let dialog = gtk4::Dialog::with_buttons(
        Some(title),
        None::<&gtk4::Window>,
        gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
        &[
            ("Create", gtk4::ResponseType::Accept),
            ("Cancel", gtk4::ResponseType::Cancel),
        ],
    );
    let entry = gtk4::Entry::new();
    entry.set_text(prefill);
    entry.set_placeholder_text(Some("Enter name…"));
    if !prefill.is_empty() {
        entry.select_region(0, -1);
    }
    dialog.content_area().append(&entry);
    dialog.set_default_response(gtk4::ResponseType::Accept);
    entry.set_activates_default(true);
    dialog.connect_response(move |dlg, resp| {
        if resp == gtk4::ResponseType::Accept {
            let name = entry.text().to_string();
            if !name.is_empty() {
                on_accept(name);
            }
        }
        dlg.close();
    });
    dialog.present();
}

/// Validate filename for file/folder creation
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name cannot be empty".to_string());
    }

    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain slashes".to_string());
    }

    if name.contains('\0') {
        return Err("Name cannot contain null characters".to_string());
    }

    // Platform-specific invalid characters
    #[cfg(windows)]
    {
        if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
            return Err("Name contains invalid characters".to_string());
        }
    }

    // Reserved names
    if name == "." || name == ".." {
        return Err("Invalid name".to_string());
    }

    Ok(())
}

/// Find and select file in tree, expanding parents if needed
fn highlight_file_in_tree(tree_view: &gtk4::TreeView, file_path: &Path) {
    let Some(model) = tree_view.model() else {
        return;
    };
    let Some(tree_store) = model.downcast_ref::<gtk4::TreeStore>() else {
        return;
    };

    // Find the file in tree by full path (column 2)
    let path_str = file_path.to_string_lossy().to_string();

    if let Some(tree_path) = find_tree_path_for_file(tree_store, &path_str, None) {
        // Expand parents
        if tree_path.depth() > 1 {
            let mut parent_path = tree_path.clone();
            parent_path.up();
            tree_view.expand_to_path(&parent_path);
        }

        // Select the row
        tree_view.selection().select_path(&tree_path);

        // Scroll to make visible
        tree_view.scroll_to_cell(
            Some(&tree_path),
            None::<&gtk4::TreeViewColumn>,
            false,
            0.0,
            0.0,
        );
    }
}

/// Recursively find tree path for given file path string
fn find_tree_path_for_file(
    model: &gtk4::TreeStore,
    target_path: &str,
    parent: Option<&gtk4::TreeIter>,
) -> Option<gtk4::TreePath> {
    let n = model.iter_n_children(parent);

    for i in 0..n {
        let iter = if let Some(parent) = parent {
            model.iter_nth_child(Some(parent), i)?
        } else {
            model.iter_nth_child(None, i)?
        };

        // Check if this row matches
        let path_str: String = model.get_value(&iter, 2).get().ok()?;
        if path_str == target_path {
            return Some(model.path(&iter));
        }

        // Recursively check children
        if let Some(found) = find_tree_path_for_file(model, target_path, Some(&iter)) {
            return Some(found);
        }
    }

    None
}

fn main() {
    // Parse CLI args to get optional file path
    let args: Vec<String> = std::env::args().collect();

    // --tui flag: launch the terminal UI instead of GTK
    if args.iter().any(|a| a == "--tui") {
        let file_path = args
            .iter()
            .skip(1)
            .find(|a| !a.starts_with('-'))
            .map(PathBuf::from);
        tui_main::run(file_path);
        return;
    }

    let file_path = if args.len() > 1 && !args[1].starts_with('-') {
        Some(PathBuf::from(&args[1]))
    } else {
        None
    };

    let gtk_app = gtk4::Application::builder()
        .application_id("com.vimcode.VimCode")
        .flags(
            gtk4::gio::ApplicationFlags::NON_UNIQUE
                | gtk4::gio::ApplicationFlags::HANDLES_COMMAND_LINE,
        )
        .build();

    // Connect a dummy command-line handler to satisfy GIO
    gtk_app.connect_command_line(|app, _| {
        app.activate();
        0
    });

    let app = RelmApp::from_app(gtk_app);
    app.run::<App>(file_path);
}
