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
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod core;
mod icons;
mod render;
mod tui_main;

use core::engine::EngineAction;
use core::settings::LineNumberMode;
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
    Git,
    Settings,
    None,
}

use std::collections::HashMap;

struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
    sidebar_visible: bool,
    active_panel: SidebarPanel,
    tree_store: Option<gtk4::TreeStore>,
    tree_has_focus: bool,
    file_tree_view: Rc<RefCell<Option<gtk4::TreeView>>>,
    drawing_area: Rc<RefCell<Option<gtk4::DrawingArea>>>,
    sidebar_inner_box: Rc<RefCell<Option<gtk4::Box>>>,
    // Per-window scrollbars and indicators
    window_scrollbars: Rc<RefCell<HashMap<core::WindowId, WindowScrollbars>>>,
    overlay: Rc<RefCell<Option<gtk4::Overlay>>>,
    cached_line_height: f64,
    cached_char_width: f64,
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
}

/// Scrollbars and indicators for a single window
struct WindowScrollbars {
    vertical: gtk4::Scrollbar,
    horizontal: gtk4::Scrollbar,
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
    /// Create a new file with the given name.
    CreateFile(String),
    /// Create a new folder with the given name.
    CreateFolder(String),
    /// Delete a file or folder at the given path.
    DeletePath(PathBuf),
    /// Refresh the file tree from current working directory.
    RefreshFileTree,
    /// Focus the explorer panel (Ctrl-Shift-E).
    FocusExplorer,
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

            // Save window geometry on close
            connect_close_request[sender] => move |window| {
                let width = window.default_width();
                let height = window.default_height();
                sender.input(Msg::WindowClosing { width, height });
                gtk4::glib::Propagation::Proceed
            },

            #[name = "main_hbox"]
            gtk4::Box {
                set_orientation: gtk4::Orientation::Horizontal,

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

                    gtk4::Button {
                        set_label: "\u{f418}",
                        set_tooltip_text: Some("Git (disabled)"),
                        set_width_request: 48,
                        set_height_request: 48,
                        set_css_classes: &["activity-button"],
                        set_sensitive: false,
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
                        set_width_request: 300,

                        // Explorer panel
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
                                connect_clicked[sender] => move |_| {
                                    // Generate filename: newfile_1.txt, newfile_2.txt, etc.
                                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                                    let mut counter = 1;
                                    let mut filename = format!("newfile_{}.txt", counter);

                                    // Find next available number
                                    while cwd.join(&filename).exists() {
                                        counter += 1;
                                        filename = format!("newfile_{}.txt", counter);
                                    }

                                    sender.input(Msg::CreateFile(filename));
                                }
                            },

                            gtk4::Button {
                                set_label: "\u{f07b}",
                                set_tooltip_text: Some("New Folder"),
                                set_width_request: 32,
                                set_height_request: 32,
                                connect_clicked[sender] => move |_| {
                                    // Generate folder name: newfolder_1, newfolder_2, etc.
                                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                                    let mut counter = 1;
                                    let mut foldername = format!("newfolder_{}", counter);

                                    // Find next available number
                                    while cwd.join(&foldername).exists() {
                                        counter += 1;
                                        foldername = format!("newfolder_{}", counter);
                                    }

                                    sender.input(Msg::CreateFolder(foldername));
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
                                            sender.input(Msg::DeletePath(path));
                                        }
                                    }
                                }
                            },

                            gtk4::Button {
                                set_label: "\u{f021}",
                                set_tooltip_text: Some("Refresh"),
                                set_width_request: 32,
                                set_height_request: 32,
                                connect_clicked[sender] => move |_| {
                                    sender.input(Msg::RefreshFileTree);
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
                                    connect_key_pressed[sender] => move |_, key, _, _| {
                                        let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();

                                        // Escape returns focus to editor
                                        if key_name == "Escape" {
                                            sender.input(Msg::FocusEditor);
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
                    },
                },

                // Sidebar resize drag handle (6px wide, ew-resize cursor)
                #[name = "sidebar_resize_handle"]
                gtk4::Box {
                    set_width_request: 6,
                    set_css_classes: &["sidebar-resize-handle"],

                    #[watch]
                    set_visible: model.sidebar_visible,

                    add_controller = gtk4::GestureDrag {
                        connect_drag_update[sidebar_inner_box_ref] => move |_, dx, _| {
                            if let Some(ref sb) = *sidebar_inner_box_ref.borrow() {
                                let current = sb.width_request();
                                let new_w = (current as f64 + dx).round() as i32;
                                sb.set_width_request(new_w.clamp(100, 600));
                            }
                        },
                        connect_drag_end[sender] => move |_, _, _| {
                            sender.input(Msg::SidebarResized);
                        },
                    },
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
                                connect_key_pressed[sender] => move |_, key, _, modifier| {
                                    let key_name = key.name().map(|s| s.to_string()).unwrap_or_default();
                                    let unicode = key.to_unicode().filter(|c| !c.is_control());
                                    let ctrl = modifier.contains(gdk::ModifierType::CONTROL_MASK);
                                    let shift = modifier.contains(gdk::ModifierType::SHIFT_MASK);

                                    // Check for Ctrl-F to toggle find dialog
                                    if ctrl && !shift && unicode == Some('f') {
                                        sender.input(Msg::ToggleFindDialog);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Check for Ctrl-B to toggle sidebar
                                    if ctrl && !shift && unicode == Some('b') {
                                        sender.input(Msg::ToggleSidebar);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Check for Ctrl-Shift-E to focus explorer
                                    if ctrl && shift && (unicode == Some('E') || unicode == Some('e')) {
                                        sender.input(Msg::FocusExplorer);
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    // Check for Ctrl-Shift-F to open project search
                                    if ctrl && shift && (unicode == Some('F') || unicode == Some('f')) {
                                        sender.input(Msg::SwitchPanel(SidebarPanel::Search));
                                        return gtk4::glib::Propagation::Stop;
                                    }

                                    sender.input(Msg::KeyPress { key_name, unicode, ctrl });
                                    gtk4::glib::Propagation::Stop
                                }
                            },

                            add_controller = gtk4::GestureClick {
                                connect_pressed[sender, drawing_area] => move |_, _, x, y| {
                                    // Grab focus when clicking in editor
                                    drawing_area.grab_focus();

                                    let width = drawing_area.width() as f64;
                                    let height = drawing_area.height() as f64;
                                    sender.input(Msg::MouseClick { x, y, width, height });
                                }
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
            }
        }
    }

    fn init(
        file_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Load CSS before creating widgets
        load_css();

        let engine = match file_path {
            Some(ref path) => Engine::open(path),
            None => {
                let mut e = Engine::new();
                e.restore_session_files();
                e
            }
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
        let overlay_ref = Rc::new(RefCell::new(None));
        let window_scrollbars_ref = Rc::new(RefCell::new(HashMap::new()));
        let sidebar_inner_box_ref: Rc<RefCell<Option<gtk4::Box>>> = Rc::new(RefCell::new(None));
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
            active_panel: SidebarPanel::Explorer,
            tree_store: Some(tree_store.clone()),
            tree_has_focus: false,
            file_tree_view: file_tree_view_ref.clone(),
            drawing_area: drawing_area_ref.clone(),
            window_scrollbars: window_scrollbars_ref.clone(),
            overlay: overlay_ref.clone(),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
            settings_monitor,
            sender: sender.input_sender().clone(),
            find_dialog_visible: false,
            find_text: String::new(),
            replace_text: String::new(),
            find_case_sensitive: false,
            find_whole_word: false,
            sidebar_inner_box: sidebar_inner_box_ref.clone(),
            project_search_status: String::new(),
            search_results_list: search_results_list_ref.clone(),
        };
        let widgets = view_output!();

        // Store widget references
        *file_tree_view_ref.borrow_mut() = Some(widgets.file_tree_view.clone());
        *drawing_area_ref.borrow_mut() = Some(widgets.drawing_area.clone());
        *overlay_ref.borrow_mut() = Some(widgets.editor_overlay.clone());
        *sidebar_inner_box_ref.borrow_mut() = Some(widgets.sidebar_inner_box.clone());
        *search_results_list_ref.borrow_mut() = Some(widgets.search_results_list.clone());

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

        // Build tree from current working directory
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        build_file_tree(&tree_store, None, &cwd);

        // Debug: print entry count
        eprintln!("Tree entries: {}", tree_store.iter_n_children(None));

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

        // Set the actual title after widget creation
        root.set_title(Some(&title));

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

        let engine_clone = engine.clone();
        let sender_for_draw = sender.input_sender().clone();
        widgets
            .drawing_area
            .set_draw_func(move |_, cr, width, height| {
                let engine = engine_clone.borrow();
                draw_editor(cr, &engine, width, height, &sender_for_draw);
            });

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

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
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
                let action = {
                    let mut engine = self.engine.borrow_mut();
                    engine.handle_key(&key_name, unicode, ctrl)
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
                        let _ = engine.session.save();
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
                            let _ = engine.session.save();
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
                        EngineAction::None | EngineAction::Error => {}
                    }

                    if !has_more {
                        break;
                    }
                }

                self.redraw = !self.redraw;
            }
            Msg::Resize => {
                self.redraw = !self.redraw;
            }
            Msg::MouseClick {
                x,
                y,
                width,
                height,
            } => {
                let mut engine = self.engine.borrow_mut();
                handle_mouse_click(&mut engine, x, y, width, height);
                self.redraw = !self.redraw;
            }
            Msg::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.redraw = !self.redraw;

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
            Msg::CreateFile(name) => {
                // Validate name
                if let Err(msg) = validate_name(&name) {
                    self.engine.borrow_mut().message = msg;
                    self.redraw = !self.redraw;
                    return;
                }

                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let file_path = cwd.join(&name);

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
                        _sender.input(Msg::RefreshFileTree);

                        // Open the new file
                        _sender.input(Msg::OpenFileFromSidebar(file_path));
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message =
                            format!("Error creating '{}': {}", name, e);
                    }
                }
                self.redraw = !self.redraw;
            }
            Msg::CreateFolder(name) => {
                // Validate name
                if let Err(msg) = validate_name(&name) {
                    self.engine.borrow_mut().message = msg;
                    self.redraw = !self.redraw;
                    return;
                }

                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let folder_path = cwd.join(&name);

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
                        _sender.input(Msg::RefreshFileTree);
                    }
                    Err(e) => {
                        self.engine.borrow_mut().message =
                            format!("Error creating folder '{}': {}", name, e);
                    }
                }
                self.redraw = !self.redraw;
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

                        _sender.input(Msg::RefreshFileTree);
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
                            // Clear tree
                            store.clear();

                            // Rebuild
                            build_file_tree(store, None, &cwd);
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
            Msg::FocusEditor => {
                self.tree_has_focus = false;

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
            Msg::CacheFontMetrics(line_height, char_width) => {
                self.cached_line_height = line_height;
                self.cached_char_width = char_width;
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
        }

        // Sync scrollbar position to match engine state (except when scrollbar itself changed)
        if !is_scrollbar_msg {
            self.sync_scrollbar();
        }
    }
}

impl App {
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

        let content_bounds = WindowRect::new(
            0.0,
            tab_bar_height,
            da_width,
            da_height - tab_bar_height - status_bar_height - 10.0, // Reserve 10px for h-scrollbar
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

            // Position and sync horizontal scrollbar in the 10px gap above status line
            let h_scrollbar_y = da_height - status_bar_height - 10.0;

            // Use absolute positioning with Start alignment
            ws.horizontal.set_halign(gtk4::Align::Start);
            ws.horizontal.set_valign(gtk4::Align::Start);

            ws.horizontal.set_margin_start(rect.x as i32);
            ws.horizontal.set_margin_top(h_scrollbar_y as i32);
            ws.horizontal.set_width_request(rect.width as i32);
            ws.horizontal.set_height_request(10);

            let max_line_length = buffer_state
                .buffer
                .content
                .lines()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(80);

            // Compute visible text columns for this specific window.
            // We subtract the gutter (char cells × char_width) and the 10px
            // vertical scrollbar so that the thumb correctly reflects how much
            // of the longest line fits on screen.
            let cw = self.cached_char_width.max(1.0);
            let gutter_cols = render::calculate_gutter_cols(
                engine.settings.line_numbers,
                buffer_state.buffer.content.len_lines(),
                cw,
                !buffer_state.git_diff.is_empty(),
            );
            let gutter_px = gutter_cols as f64 * cw;
            let v_scrollbar_px = 10.0_f64;
            let visible_cols = ((rect.width - gutter_px - v_scrollbar_px) / cw)
                .floor()
                .max(1.0);

            let h_adj = ws.horizontal.adjustment();
            h_adj.set_upper(max_line_length as f64);
            h_adj.set_page_size(visible_cols);
            h_adj.set_value(window.view.scroll_left as f64);
        }

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

        // Horizontal scrollbar
        let h_adj = gtk4::Adjustment::new(0.0, 0.0, 200.0, 1.0, 10.0, 80.0);
        let horizontal = gtk4::Scrollbar::new(gtk4::Orientation::Horizontal, Some(&h_adj));
        horizontal.set_height_request(10);
        horizontal.set_hexpand(false);
        horizontal.set_vexpand(false);

        // Cursor indicator
        let cursor_indicator = gtk4::DrawingArea::new();
        cursor_indicator.set_width_request(10);
        cursor_indicator.set_height_request(4);
        cursor_indicator.set_can_target(false);
        // Set alignments and prevent expansion to maintain fixed 4px height
        cursor_indicator.set_halign(gtk4::Align::Start);
        cursor_indicator.set_valign(gtk4::Align::Start);
        cursor_indicator.set_hexpand(false);
        cursor_indicator.set_vexpand(false);
        cursor_indicator.set_draw_func(|_, cr, w, h| {
            // Darker grey color like VSCode (darker than scrollbar handle)
            cr.set_source_rgba(0.5, 0.5, 0.5, 0.8);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
        });

        // Add to overlay
        overlay.add_overlay(&vertical);
        overlay.add_overlay(&horizontal);
        overlay.add_overlay(&cursor_indicator);

        // Make scrollbars visible
        vertical.show();
        horizontal.show();
        cursor_indicator.show();

        // Connect signals (always, for all windows)
        let sender_v = sender.clone();
        v_adj.connect_value_changed(move |adj| {
            sender_v
                .send(Msg::VerticalScrollbarChanged {
                    window_id,
                    value: adj.value(),
                })
                .ok();
        });

        let sender_h = sender.clone();
        h_adj.connect_value_changed(move |adj| {
            sender_h
                .send(Msg::HorizontalScrollbarChanged {
                    window_id,
                    value: adj.value(),
                })
                .ok();
        });

        WindowScrollbars {
            vertical,
            horizontal,
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

fn draw_editor(
    cr: &Context,
    engine: &Engine,
    width: i32,
    height: i32,
    sender: &relm4::Sender<Msg>,
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

    // Calculate window rects for the current tab
    let content_bounds = WindowRect::new(
        0.0,
        tab_bar_height,
        width as f64,
        height as f64 - tab_bar_height - status_bar_height - 10.0, // Reserve 10px for h-scrollbar
    );
    let window_rects = engine.calculate_window_rects(content_bounds);

    // Build the platform-agnostic screen layout
    let screen = build_screen_layout(engine, &theme, &window_rects, line_height, char_width);

    // 3. Draw tab bar (always visible)
    draw_tab_bar(
        cr,
        &layout,
        &theme,
        &screen.tab_bar,
        width as f64,
        line_height,
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

fn draw_tab_bar(
    cr: &Context,
    layout: &pango::Layout,
    theme: &Theme,
    tabs: &[TabInfo],
    width: f64,
    line_height: f64,
) {
    // Tab bar background
    let (r, g, b) = theme.tab_bar_bg.to_cairo();
    cr.set_source_rgb(r, g, b);
    cr.rectangle(0.0, 0.0, width, line_height);
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
        cr.rectangle(x, 0.0, tab_width as f64, line_height);
        cr.fill().unwrap();

        // Tab text — dimmed colours for preview tabs
        cr.move_to(x, 0.0);
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
            theme,
        );
    }

    // Render gutter (git marker + fold indicators + optional line numbers)
    if rw.gutter_char_width > 0 {
        for (view_idx, rl) in rw.lines.iter().enumerate() {
            let y = rect.y + view_idx as f64 * line_height;

            if rw.has_git_diff {
                // Render git marker (first char of gutter_text) with git color.
                let git_ch: String = rl.gutter_text.chars().take(1).collect();
                let git_color = match rl.git_diff {
                    Some(GitLineStatus::Added) => theme.git_added,
                    Some(GitLineStatus::Modified) => theme.git_modified,
                    None => theme.line_number_fg,
                };
                layout.set_text(&git_ch);
                layout.set_attributes(None);
                let (gr, gg, gb) = git_color.to_cairo();
                cr.set_source_rgb(gr, gg, gb);
                cr.move_to(rect.x + 3.0, y);
                pangocairo::show_layout(cr, layout);

                // Render fold+numbers (rest of gutter_text) with normal color.
                let rest: String = rl.gutter_text.chars().skip(1).collect();
                layout.set_text(&rest);
                layout.set_attributes(None);
            } else {
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
    theme: &Theme,
) {
    let visible_lines = lines.len();
    let (sr, sg, sb) = theme.selection.to_cairo();
    cr.set_source_rgba(sr, sg, sb, theme.selection_alpha);

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

/// Handle mouse click by converting coordinates to buffer position.
/// This determines which window was clicked and moves the cursor there.
fn handle_mouse_click(engine: &mut Engine, x: f64, y: f64, width: f64, height: f64) {
    // Create Pango context to measure font metrics (matching draw_editor)
    use gtk4::cairo::{Context as CairoContext, Format, ImageSurface};

    // Create a temporary surface for Pango measurements
    let surface = ImageSurface::create(Format::Rgb24, 1, 1).unwrap();
    let cr = CairoContext::new(&surface).unwrap();

    let pango_ctx = pangocairo::create_context(&cr);
    // Use configurable font from settings (matching draw_editor)
    let font_str = format!(
        "{} {}",
        engine.settings.font_family, engine.settings.font_size
    );
    let font_desc = FontDescription::from_string(&font_str);

    // Get actual font metrics (matching draw_editor line 250-251)
    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;

    let tab_bar_height = line_height; // Always show tab bar

    // Check if click is in tab bar
    if y < tab_bar_height {
        // Calculate which tab was clicked
        let layout = pangocairo::create_layout(&cr);
        layout.set_font_description(Some(&font_desc));

        let normal_font = font_desc.clone();
        let mut italic_font = font_desc.clone();
        italic_font.set_style(pango::Style::Italic);

        let mut tab_x = 0.0;
        for (i, tab) in engine.tabs.iter().enumerate() {
            // Get buffer name and preview state (same logic as draw_tab_bar)
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

            // Use correct font for measuring
            if is_preview {
                layout.set_font_description(Some(&italic_font));
            } else {
                layout.set_font_description(Some(&normal_font));
            }

            layout.set_text(&name);
            let (tab_width, _) = layout.pixel_size();

            // Check if click is in this tab's bounds
            if x >= tab_x && x < tab_x + tab_width as f64 {
                engine.goto_tab(i);
                return;
            }

            tab_x += tab_width as f64 + 2.0;
        }
        return;
    }

    let status_bar_height = line_height * 2.0;

    let content_bounds = WindowRect::new(
        0.0,
        tab_bar_height,
        width,
        height - tab_bar_height - status_bar_height,
    );

    // Check if click is in status/command area
    if y >= content_bounds.y + content_bounds.height {
        // Click in status bar or command line - ignore for now
        return;
    }

    // Get window rects
    let window_rects = engine.calculate_window_rects(content_bounds);

    // Find which window was clicked
    let clicked_window = window_rects.iter().find(|(_, rect)| {
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    });

    let (window_id, rect) = match clicked_window {
        Some((id, r)) => (*id, r),
        None => return, // Click outside any window
    };

    // Get window and buffer info
    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return,
    };

    let buffer_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return,
    };

    let buffer = &buffer_state.buffer;
    let view = &window.view;

    // Calculate gutter width from render module (matches the actual draw function).
    let char_width = font_metrics.approximate_char_width() as f64 / pango::SCALE as f64;
    let total_lines = buffer.content.len_lines();
    let has_git = !buffer_state.git_diff.is_empty();
    let gutter_char_width = render::calculate_gutter_cols(
        engine.settings.line_numbers,
        total_lines,
        char_width,
        has_git,
    );
    let gutter_width = gutter_char_width as f64 * char_width;

    // Calculate per-window status bar height
    let per_window_status = if engine.windows.len() > 1 {
        line_height
    } else {
        0.0
    };
    let text_area_height = rect.height - per_window_status;

    // Check if click is in per-window status bar
    if y >= rect.y + text_area_height {
        return;
    }

    // Convert y coordinate to visible row index
    let relative_y = y - rect.y;
    let view_row = (relative_y / line_height).floor() as usize;

    // Map view_row → buffer line (fold-aware: skip hidden lines)
    let line = view_row_to_buf_line(view, view.scroll_top, view_row, total_lines);

    // Entire gutter is a click target for fold toggle
    if x >= rect.x && x < rect.x + gutter_width && gutter_width > 0.0 {
        engine.toggle_fold_at_line(line);
        return;
    }

    // Convert x coordinate to column using pixel-perfect Pango layout measurement
    let relative_x = x - (rect.x + gutter_width);

    // Get the actual line text (clamp line to valid range)
    let line = line.min(buffer.content.len_lines().saturating_sub(1));
    let line_text = buffer.content.line(line).to_string();

    // Create Pango layout with the line text
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&font_desc));

    // Find column by measuring text width character by character
    let mut col = 0;

    if !line_text.is_empty() {
        // Handle tabs by expanding them to spaces (4 spaces per tab)
        let expanded_text = line_text.replace('\t', "    ");
        layout.set_text(&expanded_text);

        let mut best_col = 0;
        let mut prev_width = 0.0;

        // Find which character the click falls within
        let char_indices: Vec<(usize, char)> = expanded_text.char_indices().collect();

        for i in 0..char_indices.len() {
            let (byte_idx, _) = char_indices[i];

            // Measure width up to and including this character
            let next_byte_idx = if i + 1 < char_indices.len() {
                char_indices[i + 1].0
            } else {
                expanded_text.len()
            };

            layout.set_text(&expanded_text[..next_byte_idx]);
            let (curr_width, _) = layout.pixel_size();
            let curr_width_f64 = curr_width as f64;

            // Check if click falls between prev_width and curr_width
            if relative_x >= prev_width && relative_x < curr_width_f64 {
                // Click is within this character, use its starting position
                best_col = byte_idx;
                break;
            }

            prev_width = curr_width_f64;

            // If we're at the last character and click is past it
            if i == char_indices.len() - 1 && relative_x >= curr_width_f64 {
                best_col = next_byte_idx;
            }
        }

        // If line is empty or click is before first character
        if relative_x < 0.0 {
            best_col = 0;
        }

        // Convert byte position in expanded text to column in original text
        // Account for tabs (each tab in original becomes 4 spaces in expanded)
        let mut original_col = 0;
        let mut expanded_pos = 0;
        for ch in line_text.chars() {
            if expanded_pos >= best_col {
                break;
            }
            if ch == '\t' {
                expanded_pos += 4; // Tab expands to 4 spaces
            } else {
                expanded_pos += ch.len_utf8();
            }
            original_col += 1;
        }
        col = original_col;
    }

    // Set cursor position for this window
    engine.set_cursor_for_window(window_id, line, col);
}

fn load_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(
        "
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
            font-family: Ubuntu, Roboto, sans-serif;
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
