//! Native Windows GUI backend for VimCode.
//!
//! Uses Win32 HWND + Direct2D + DirectWrite to render the same `ScreenLayout`
//! that the GTK and TUI backends consume. No GTK/Cairo/Pango dependencies.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.
#![allow(unused_assignments, unused_variables)]

pub mod draw;
pub mod input;

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::core::engine::{open_url_in_browser, Engine, EngineAction, OpenMode};
use crate::core::window::{DropZone, GroupId, SplitDirection, WindowId, WindowRect};
use crate::icons;
use crate::render::{
    self, build_screen_layout, build_window_status_line, ScreenLayout, StatusAction, Theme,
    MENU_STRUCTURE,
};

use self::draw::DrawContext;
use self::input::{translate_char, translate_vk};

// Timer ID for periodic ticks (LSP poll, syntax debounce, swap files, etc.)
const TICK_TIMER_ID: usize = 1;
const TICK_INTERVAL_MS: u32 = 50;

/// Double-click detection threshold.
const DOUBLE_CLICK_MS: u64 = 400;

/// Hit zone width (pixels) for sidebar resize drag handle.
const SIDEBAR_RESIZE_HIT_PX: f32 = 5.0;
const SCROLLBAR_WIDTH: f32 = 6.0;

/// Width of each caption button (min/max/close) in DIPs.
const CAPTION_BTN_WIDTH: f32 = 46.0;
/// Number of caption buttons.
const CAPTION_BTN_COUNT: f32 = 3.0;
/// Title/menu bar height multiplier (relative to line_height).
/// Gives the menu bar more vertical breathing room than a code line.
const TITLE_BAR_HEIGHT_MULT: f32 = 1.8;
/// Top inset in DIPs to avoid content clipping against the DWM frame top edge.
/// DWM paints over the top few pixels of the client area for the window shadow,
/// so we need to push content down below that zone.
const TITLE_BAR_TOP_INSET: f32 = 6.0;
/// Tab bar height multiplier (relative to line_height).
/// Makes tabs taller like VSCode (~1.6× code line height).
const TAB_BAR_HEIGHT_MULT: f32 = 1.5;

// ─── Per-window state stored in a thread-local ──────────────────────────────

/// Cached tab slot positions from the last draw pass, used for click hit-testing.
#[derive(Clone, Debug)]
struct TabSlot {
    group_id: GroupId,
    tab_idx: usize,
    x_start: f32,
    x_end: f32,
    close_x_start: f32,
    y: f32,
    height: f32,
}

/// Cached window rectangle from the last draw pass, for mouse-to-editor mapping.
#[derive(Clone, Debug)]
struct CachedWindowRect {
    window_id: WindowId,
    group_id: GroupId,
    rect: WindowRect,
    gutter_chars: usize,
}

// ─── Sidebar ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarPanel {
    Explorer,
    Search,
    Debug,
    Git,
    Extensions,
    Ai,
    Settings,
}

#[derive(Debug, Clone)]
struct ExplorerRow {
    depth: usize,
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_expanded: bool,
}

struct WinSidebar {
    visible: bool,
    active_panel: SidebarPanel,
    /// Activity bar width in pixels (icon column).
    activity_bar_px: f32,
    /// Sidebar panel width in pixels.
    panel_width: f32,
    // Explorer state
    rows: Vec<ExplorerRow>,
    expanded: HashSet<PathBuf>,
    selected: usize,
    scroll_top: usize,
    show_hidden: bool,
    sort_case_insensitive: bool,
    /// Set when tree needs to be rebuilt (file opened, folder toggled, etc.)
    dirty: bool,
    /// Whether the sidebar has keyboard focus.
    has_focus: bool,
}

impl WinSidebar {
    fn new() -> Self {
        Self {
            visible: false,
            active_panel: SidebarPanel::Explorer,
            activity_bar_px: 48.0,
            panel_width: 220.0,
            rows: Vec::new(),
            expanded: HashSet::new(),
            selected: 0,
            scroll_top: 0,
            show_hidden: false,
            sort_case_insensitive: true,
            dirty: true,
            has_focus: false,
        }
    }

    /// Total width consumed by the sidebar (activity bar + panel, or just activity bar).
    fn total_width(&self) -> f32 {
        if self.visible {
            self.activity_bar_px + self.panel_width
        } else {
            self.activity_bar_px
        }
    }

    fn build_rows(&mut self, root: &Path) {
        self.rows.clear();
        let root_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());
        let root_expanded = self.expanded.contains(root);
        self.rows.push(ExplorerRow {
            depth: 0,
            name: root_name.to_uppercase(),
            path: root.to_path_buf(),
            is_dir: true,
            is_expanded: root_expanded,
        });
        if root_expanded {
            collect_explorer_rows(
                root,
                1,
                &self.expanded,
                self.show_hidden,
                self.sort_case_insensitive,
                &mut self.rows,
            );
        }
        if !self.rows.is_empty() && self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    fn toggle_expand(&mut self, idx: usize) {
        if idx >= self.rows.len() || !self.rows[idx].is_dir {
            return;
        }
        let path = self.rows[idx].path.clone();
        if self.expanded.contains(&path) {
            self.expanded.remove(&path);
        } else {
            self.expanded.insert(path);
        }
    }
}

fn collect_explorer_rows(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    show_hidden: bool,
    case_insensitive: bool,
    out: &mut Vec<ExplorerRow>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by(|a, b| {
        let ad = a.path().is_dir();
        let bd = b.path().is_dir();
        match (ad, bd) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                if case_insensitive {
                    let an = a.file_name().to_string_lossy().to_lowercase();
                    let bn = b.file_name().to_string_lossy().to_lowercase();
                    an.cmp(&bn)
                } else {
                    a.file_name().cmp(&b.file_name())
                }
            }
        }
    });
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && !show_hidden {
            continue;
        }
        let is_dir = path.is_dir();
        let is_expanded = is_dir && expanded.contains(&path);
        out.push(ExplorerRow {
            depth,
            name,
            path: path.clone(),
            is_dir,
            is_expanded,
        });
        if is_expanded {
            collect_explorer_rows(
                &path,
                depth + 1,
                expanded,
                show_hidden,
                case_insensitive,
                out,
            );
        }
    }
}

struct AppState {
    engine: Engine,
    theme: Theme,
    d2d_factory: ID2D1Factory,
    dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    /// Proportional UI font (Segoe UI) for menus and tab labels.
    ui_text_format: IDWriteTextFormat,
    /// Icon font for activity bar and toolbar icons (larger, tries Symbols Nerd Font).
    icon_text_format: IDWriteTextFormat,
    render_target: Option<ID2D1HwndRenderTarget>,
    hwnd: HWND,
    char_width: f32,
    line_height: f32,
    dpi_scale: f32,

    // ── Cached layout from last draw pass ────────────────────────────────
    tab_slots: Vec<TabSlot>,
    cached_window_rects: Vec<CachedWindowRect>,
    /// Cached breadcrumb bars from last draw pass (for click handling).
    cached_breadcrumbs: Vec<render::BreadcrumbBar>,
    /// Cached diff toolbar button positions: (group_id, prev_x, next_x, fold_x, btn_w, bar_y, bar_h).
    cached_diff_toolbar_btns: Vec<(GroupId, f32, f32, f32, f32, f32, f32)>,
    /// Cached group dividers from last draw pass (for drag handling).
    cached_dividers: Vec<crate::core::window::GroupDivider>,

    // ── Sidebar ───────────────────────────────────────────────────────────
    sidebar: WinSidebar,

    // ── Hot-reload tracking ──────────────────────────────────────────────
    current_colorscheme: String,
    current_font_size: i32,

    // ── Mouse state ──────────────────────────────────────────────────────
    mouse_text_drag: bool,
    sidebar_resize_drag: bool,
    scrollbar_drag: Option<WindowId>,
    terminal_resize_drag: bool,
    /// Active group divider drag: split_index being dragged.
    group_divider_drag: Option<usize>,
    /// True when dragging to select text in the terminal panel.
    terminal_text_drag: bool,
    last_click_time: Instant,
    last_click_pos: (i16, i16),

    // ── Tab drag-and-drop ────────────────────────────────────────────────
    /// Mousedown position on a tab, before threshold is reached.
    tab_drag_start: Option<(f32, f32, GroupId, usize)>,
    /// True when actively dragging a tab (threshold exceeded).
    tab_dragging: bool,
    /// True when dragging the terminal split divider.
    terminal_split_drag: bool,

    // ── Clipboard sync ────────────────────────────────────────────────────
    /// Last known `"` register content, for detecting yank changes.
    last_clipboard_register: Option<String>,

    /// X position of the hovered tab for tooltip placement.
    tab_tooltip_x: f32,

    // ── Periodic refresh ─────────────────────────────────────────────────
    last_sidebar_refresh: Instant,

    // ── Layout cache ─────────────────────────────────────────────────────
    /// Bottom chrome height in DIPs (status bar + cmd line + terminal + above-terminal status)
    bottom_chrome_px: f32,
    /// Cached popup bounding rectangles from the last draw pass, for mouse hit-testing.
    popup_rects: CachedPopupRects,

    // ── Caption buttons (custom title bar) ──────────────────────────────
    /// Which caption button is hovered (0=min, 1=max, 2=close), or None.
    caption_hover: Option<usize>,
}

thread_local! {
    static APP: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

// ─── Entry point ────────────────────────────────────────────────────────────

pub fn run(file_path: Option<PathBuf>) {
    // Initialize COM (needed for native file dialogs, etc.)
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    }

    // Enable per-monitor DPI awareness (Windows 10 1703+)
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let mut engine = Engine::new();
    engine.menu_bar_visible = true;
    icons::set_nerd_fonts(engine.settings.use_nerd_fonts);
    engine.plugin_init();
    engine.ext_refresh();

    if let Some(path) = file_path {
        if path.is_dir() {
            engine.open_folder(&path);
        } else {
            let _ = engine.open_file_with_mode(&path, OpenMode::Permanent);
        }
    } else {
        engine.restore_session_files();
    }

    // Windows clipboard: use the same powershell-based clipboard as TUI
    setup_win_clipboard(&mut engine);

    let theme = Theme::from_name(&engine.settings.colorscheme);

    // Panic hook for crash recovery
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::core::swap::run_emergency_flush();
            if let Some(path) = crate::core::swap::write_crash_log(info) {
                eprintln!("Crash log written to {}", path.display());
            }
            prev_hook(info);
        }));
    }
    unsafe {
        crate::core::swap::register_emergency_engine(&engine as *const _);
    }

    // Create Direct2D and DirectWrite factories
    let d2d_factory: ID2D1Factory =
        unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
            .expect("D2D1CreateFactory");

    let dwrite_factory: IDWriteFactory =
        unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.expect("DWriteCreateFactory");

    // Register window class + create window (before text format, so we can get DPI)
    let hwnd = create_window();

    // Extend the DWM frame into the client area so we can draw a custom title bar
    // while keeping the native window shadow and rounded corners (Win11).
    unsafe {
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 1, // 1px top margin gives us shadow + lets WM_NCCALCSIZE work
            cyBottomHeight: 0,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }

    // Get initial DPI and compute scale factor
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi_scale = dpi as f32 / 96.0;

    // Create text format (monospace font, size from settings, scaled by DPI)
    let initial_colorscheme = engine.settings.colorscheme.clone();
    let initial_font_size = engine.settings.font_size;
    let font_size = initial_font_size as f32 * dpi_scale;
    let text_format: IDWriteTextFormat = unsafe {
        dwrite_factory.CreateTextFormat(
            w!("Consolas"),
            None,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            font_size,
            w!("en-us"),
        )
    }
    .expect("CreateTextFormat");

    // Create proportional UI font (Segoe UI) for menus and tab labels
    let ui_font_size = 13.0 * dpi_scale; // 13px is VSCode's menu font size
    let ui_text_format: IDWriteTextFormat = unsafe {
        dwrite_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            ui_font_size,
            w!("en-us"),
        )
    }
    .expect("CreateTextFormat for UI font");

    // Icon text format for activity bar — use "Segoe Fluent Icons" (Win11) or
    // "Segoe MDL2 Assets" (Win10+). These ship with Windows and have standard
    // icon codepoints. GDI-registered fonts are NOT visible to DirectWrite, so
    // we can't use the bundled Nerd Font TTF here.
    let icon_font_size = 20.0 * dpi_scale; // 20px — matches GTK's 24px visually
    let icon_text_format: IDWriteTextFormat = unsafe {
        dwrite_factory
            .CreateTextFormat(
                w!("Segoe Fluent Icons"),
                None,
                DWRITE_FONT_WEIGHT_REGULAR,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                icon_font_size,
                w!("en-us"),
            )
            .or_else(|_| {
                dwrite_factory.CreateTextFormat(
                    w!("Segoe MDL2 Assets"),
                    None,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    icon_font_size,
                    w!("en-us"),
                )
            })
            .or_else(|_| {
                dwrite_factory.CreateTextFormat(
                    w!("Consolas"),
                    None,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    icon_font_size,
                    w!("en-us"),
                )
            })
    }
    .expect("CreateTextFormat for icon font");
    unsafe {
        let _ = icon_text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        let _ = icon_text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    }

    // Measure monospace dimensions (already includes DPI scaling via font size)
    let char_width = draw::measure_char_width(&dwrite_factory, &text_format);
    let line_height = draw::measure_line_height(&dwrite_factory, &text_format);

    // Store state in thread-local
    APP.with(|app| {
        *app.borrow_mut() = Some(AppState {
            engine,
            theme,
            d2d_factory,
            dwrite_factory,
            text_format,
            ui_text_format,
            icon_text_format,
            render_target: None,
            hwnd,
            char_width,
            line_height,
            dpi_scale,
            tab_slots: Vec::new(),
            cached_window_rects: Vec::new(),
            cached_breadcrumbs: Vec::new(),
            cached_diff_toolbar_btns: Vec::new(),
            cached_dividers: Vec::new(),
            sidebar: WinSidebar::new(),
            current_colorscheme: initial_colorscheme,
            current_font_size: initial_font_size,
            mouse_text_drag: false,
            sidebar_resize_drag: false,
            scrollbar_drag: None,
            terminal_resize_drag: false,
            group_divider_drag: None,
            terminal_text_drag: false,
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
            tab_drag_start: None,
            tab_dragging: false,
            terminal_split_drag: false,
            last_clipboard_register: None,
            tab_tooltip_x: 0.0,
            last_sidebar_refresh: Instant::now(),
            bottom_chrome_px: 0.0,
            popup_rects: CachedPopupRects::default(),
            caption_hover: None,
        });
    });

    // Create render target now that we have the HWND
    create_render_target();

    // Show window and force frame recalculation so WM_NCCALCSIZE removes the title bar
    // immediately (without this, the standard title bar is visible until the first
    // maximize/restore cycle).
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
        UpdateWindow(hwnd).expect("UpdateWindow");
        SetTimer(Some(hwnd), TICK_TIMER_ID, TICK_INTERVAL_MS, None);
    }

    // Message loop
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// ─── Window creation ─────────────────────────────────────────────────────────

fn create_window() -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).expect("GetModuleHandleW");
        let class_name = w!("VimCodeWindow");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_IBEAM).unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };

        RegisterClassExW(&wc);

        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("VimCode"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            800,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("CreateWindowExW")
    }
}

fn create_render_target() {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(state.hwnd, &mut rc);
        }
        let size = D2D_SIZE_U {
            width: (rc.right - rc.left) as u32,
            height: (rc.bottom - rc.top) as u32,
        };

        let dpi = state.dpi_scale * 96.0;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            ..Default::default()
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: state.hwnd,
            pixelSize: size,
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };

        let rt: ID2D1HwndRenderTarget = unsafe {
            state
                .d2d_factory
                .CreateHwndRenderTarget(&props, &hwnd_props)
        }
        .expect("CreateHwndRenderTarget");

        state.render_target = Some(rt);
    });
}

// ─── Window procedure ────────────────────────────────────────────────────────

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Catch panics so they don't unwind across the FFI boundary (which is UB).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wnd_proc_inner(hwnd, msg, wparam, lparam)
    }));
    match result {
        Ok(lresult) => lresult,
        Err(_) => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn wnd_proc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // ── Custom title bar ─────────────────────────────────────────
        WM_NCCALCSIZE => {
            if wparam.0 != 0 {
                // Return 0 to remove the entire non-client area (title bar + borders).
                // The DWM frame extension preserves the native window shadow.
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                if is_maximized(hwnd) {
                    // When maximized, Windows extends the window past the screen
                    // edge by the frame thickness. Add that offset so content
                    // doesn't overflow onto adjacent monitors.
                    let dpi = GetDpiForWindow(hwnd);
                    let frame_y = GetSystemMetricsForDpi(SM_CYFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    params.rgrc[0].top += frame_y;
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_NCHITTEST => {
            let result = on_nchittest(hwnd, lparam);
            if result != HTNOWHERE {
                return LRESULT(result as isize);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_PAINT => {
            on_paint(hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            on_resize(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            // Consume bare Alt/F10 to prevent Windows menu-mode activation.
            // Without this, pressing Alt-M activates menu mode and steals
            // all subsequent keyboard input from the editor.
            let vk = VIRTUAL_KEY(wparam.0 as u16);
            if vk == VK_MENU || vk == VK_F10 {
                return LRESULT(0);
            }
            if on_key_down(wparam, lparam) {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        // Consume WM_SYSCHAR to prevent beep on Alt+letter combos
        WM_SYSCHAR => LRESULT(0),
        WM_CHAR => {
            on_char(wparam);
            LRESULT(0)
        }
        WM_IME_STARTCOMPOSITION => {
            on_ime_start_composition(hwnd);
            // Let DefWindowProc show the composition window
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_IME_COMPOSITION => {
            if (lparam.0 as u32 & GCS_RESULTSTR.0) != 0 {
                // The final committed string will arrive as WM_CHAR messages,
                // so we just need to pass through to DefWindowProc.
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            on_mouse_down(hwnd, lparam);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_mouse_up(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            // Let on_mouse_move set the cursor; only override in client area
            if (lparam.0 & 0xFFFF) as u16 == 1 {
                // HTCLIENT — we handle cursor ourselves
                LRESULT(1)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_LBUTTONDBLCLK => {
            on_mouse_dblclick(hwnd, lparam);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            on_right_click(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            on_mouse_wheel(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TICK_TIMER_ID {
                on_tick(hwnd);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            on_dpi_changed(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_CLOSE => {
            // Check for unsaved changes before closing
            let has_unsaved = APP.with(|app| {
                let app = app.borrow();
                app.as_ref()
                    .is_some_and(|state| state.engine.has_any_unsaved())
            });
            if has_unsaved {
                APP.with(|app| {
                    let mut app = app.borrow_mut();
                    if let Some(state) = app.as_mut() {
                        use crate::core::engine::DialogButton;
                        state.engine.show_dialog(
                            "quit_unsaved",
                            "Unsaved Changes",
                            vec![
                                "You have unsaved changes. Do you want to save before quitting?"
                                    .to_string(),
                            ],
                            vec![
                                DialogButton {
                                    label: "Save All & Quit".into(),
                                    hotkey: 's',
                                    action: "save_quit".into(),
                                },
                                DialogButton {
                                    label: "Quit Without Saving".into(),
                                    hotkey: 'd',
                                    action: "discard_quit".into(),
                                },
                                DialogButton {
                                    label: "Cancel".into(),
                                    hotkey: '\0',
                                    action: "cancel".into(),
                                },
                            ],
                        );
                    }
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0) // Don't close — let the dialog handle it
            } else {
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
        }
        WM_DESTROY => {
            on_destroy();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ─── Message handlers ────────────────────────────────────────────────────────

fn is_maximized(hwnd: HWND) -> bool {
    let mut wp = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetWindowPlacement(hwnd, &mut wp);
    }
    wp.showCmd == SW_MAXIMIZE.0 as u32
}

/// Custom hit-test for the frameless title bar.
/// Returns the appropriate HT* value, or `HTNOWHERE` to fall through to DefWindowProc.
fn on_nchittest(hwnd: HWND, lparam: LPARAM) -> u32 {
    // Get cursor position in screen coords, convert to client coords
    let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let mut pt = POINT {
        x: screen_x,
        y: screen_y,
    };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }

    // Get client rect and DPI-aware sizes
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let client_width = (rc.right - rc.left) as f32;

    let dpi_scale = APP.with(|app| app.borrow().as_ref().map_or(1.0f32, |s| s.dpi_scale));
    let line_height = APP.with(|app| app.borrow().as_ref().map_or(20.0f32, |s| s.line_height));

    // Title bar height = top inset + line_height * multiplier
    let title_bar_height = (TITLE_BAR_TOP_INSET + line_height * TITLE_BAR_HEIGHT_MULT) * dpi_scale;
    // Resize border (only at top edge for the frameless window)
    let resize_border = 5.0 * dpi_scale;

    let px = pt.x as f32;
    let py = pt.y as f32;

    // Top resize border (thin strip at very top for resizing)
    if py < resize_border && !is_maximized(hwnd) {
        // Top corners
        if px < resize_border {
            return HTTOPLEFT;
        }
        if px > client_width - resize_border {
            return HTTOPRIGHT;
        }
        return HTTOP;
    }

    // Caption buttons area (right side of the title bar row)
    if py < title_bar_height {
        let btn_total = CAPTION_BTN_COUNT * CAPTION_BTN_WIDTH * dpi_scale;
        let btn_start = client_width - btn_total;

        if px >= btn_start {
            // Return HTCLIENT so the buttons are handled by our WM_LBUTTONDOWN handler
            // with proper hover/click rendering.
            return HTCLIENT;
        }

        // When a menu dropdown is open, the entire title bar should be HTCLIENT
        // so WM_MOUSEMOVE fires and we can switch between menus on hover.
        let menu_open = APP.with(|app| {
            app.borrow()
                .as_ref()
                .is_some_and(|s| s.engine.menu_open_idx.is_some())
        });
        if menu_open {
            return HTCLIENT;
        }

        // Check if we're over the menu bar items — those should be HTCLIENT
        // so mouse clicks reach our WM_LBUTTONDOWN handler.
        let menu_bar_visible = APP.with(|app| {
            app.borrow()
                .as_ref()
                .is_some_and(|s| s.engine.menu_bar_visible)
        });
        if menu_bar_visible {
            let char_width = APP.with(|app| app.borrow().as_ref().map_or(8.0f32, |s| s.char_width));
            // Menu labels start at 1*cw and extend across all menu items
            let mut menu_end = char_width; // initial offset
            for (name, _, _) in MENU_STRUCTURE.iter() {
                menu_end += (name.len() as f32 + 2.0) * char_width;
            }
            if px < menu_end * dpi_scale {
                return HTCLIENT;
            }
        }

        // Rest of the title bar row = draggable caption
        return HTCAPTION;
    }

    // Below the title bar — handled by client area
    HTNOWHERE
}

fn on_paint(hwnd: HWND) {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let Some(rt) = state.render_target.clone() else {
            return;
        };

        // Compute window rects for the engine
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(state.hwnd, &mut rc);
        }
        let width = (rc.right - rc.left) as f64;
        let height = (rc.bottom - rc.top) as f64;

        let cw = state.char_width as f64;
        let lh = state.line_height as f64;

        // Sidebar offset
        let sidebar_left = state.sidebar.total_width() as f64;

        // Menu/title bar takes a taller row at the top when visible (+ top inset)
        let menu_bar_height = if state.engine.menu_bar_visible {
            TITLE_BAR_TOP_INSET as f64 + lh * TITLE_BAR_HEIGHT_MULT as f64
        } else {
            0.0
        };

        // Terminal panel height (toolbar row + content rows)
        let terminal_height =
            if state.engine.terminal_open && !state.engine.terminal_panes.is_empty() {
                (state.engine.session.terminal_panel_rows as f64 + 1.0) * lh // +1 for toolbar
            } else {
                0.0
            };

        // Reserve: tab bar (taller) + optional breadcrumb row at the top of each group
        let tab_bar_height = if state.engine.settings.breadcrumbs {
            lh * TAB_BAR_HEIGHT_MULT as f64 + lh // taller tab bar + breadcrumb row
        } else {
            lh * TAB_BAR_HEIGHT_MULT as f64
        };
        let status_above_terminal = state.engine.settings.window_status_line
            && state.engine.settings.status_line_above_terminal
            && state.engine.terminal_open;
        let above_terminal_px = if status_above_terminal {
            2.0 * lh // separated status + cmd (above terminal)
        } else {
            0.0
        };
        let below_terminal_px = if status_above_terminal {
            0.0 // nothing below terminal
        } else if state.engine.settings.window_status_line {
            1.0 * lh // cmd line only (per-window status is inside editor windows)
        } else {
            2.0 * lh // global status + cmd (below terminal)
        };
        let bottom_chrome = below_terminal_px + terminal_height + above_terminal_px;
        // Store in same units as line_height (physical-px) — callers that need DIPs must divide by dpi_scale
        state.bottom_chrome_px = bottom_chrome as f32;
        let editor_bottom = height - bottom_chrome;

        // Use engine's group-aware rect calculation (handles splits)
        let editor_bounds = WindowRect::new(
            sidebar_left,
            menu_bar_height,
            width - sidebar_left,
            editor_bottom - menu_bar_height,
        );
        let (window_rects, _dividers) = state
            .engine
            .calculate_group_window_rects(editor_bounds, tab_bar_height);

        // Update viewports for all windows based on their rects
        for (wid, wrect) in &window_rects {
            let vp_lines = (wrect.height / lh).floor() as usize;
            let vp_cols = (wrect.width / cw).floor() as usize;
            state
                .engine
                .set_viewport_for_window(*wid, vp_lines.saturating_sub(1), vp_cols);
        }

        // Rebuild explorer rows only when dirty
        if state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Explorer
            && state.sidebar.dirty
        {
            if let Some(ref root) = state.engine.workspace_root.clone() {
                state.sidebar.build_rows(root);
                state.sidebar.dirty = false;
            }
        }

        let screen = build_screen_layout(&state.engine, &state.theme, &window_rects, lh, cw, true);

        // Cache window rects for mouse hit-testing
        cache_layout(state, &screen, &window_rects);

        let ctx = DrawContext {
            rt: &rt,
            dwrite: &state.dwrite_factory,
            format: &state.text_format,
            ui_format: &state.ui_text_format,
            icon_format: &state.icon_text_format,
            theme: &state.theme,
            char_width: state.char_width,
            line_height: state.line_height,
            editor_left: state.sidebar.total_width(),
            tab_tooltip_x: state.tab_tooltip_x,
            caption_hover: state.caption_hover,
            is_maximized: is_maximized(state.hwnd),
        };

        unsafe {
            rt.BeginDraw();
            ctx.draw_frame(&screen);
            // Draw sidebar on top of left edge (activity bar always, panel when visible)
            let menu_bar_y = if state.engine.menu_bar_visible {
                TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT
            } else {
                0.0
            };
            ctx.draw_sidebar(
                &state.sidebar,
                &screen,
                menu_bar_y,
                state.bottom_chrome_px,
                &state.engine,
            );
            // Context menu on top of sidebar
            if let Some(ref ctxm) = screen.context_menu {
                ctx.draw_context_menu(ctxm);
            }
            // Tab drag overlay (drop zone highlight + ghost label)
            ctx.draw_tab_drag_overlay(&screen, &state.engine);
            // Menu dropdown on top of sidebar
            if let Some(ref menu) = screen.menu_bar {
                ctx.draw_menu_dropdown(menu);
            }
            // Dialog (on top of everything except notifications)
            if let Some(ref dialog) = screen.dialog {
                ctx.draw_dialog(dialog);
            }
            // Notification toasts
            ctx.draw_notifications(&state.engine.notifications);
            let _ = rt.EndDraw(None, None);
        }

        // Cache popup rects for mouse hit-testing
        cache_popup_rects(state, &screen);

        // Validate the paint
        unsafe {
            let _ = ValidateRect(Some(hwnd), None);
        }
    });
}

/// Recompute popup bounding rectangles from the screen layout (mirrors draw positioning).
fn cache_popup_rects(state: &mut AppState, screen: &ScreenLayout) {
    let cw = state.char_width;
    let lh = state.line_height;
    let rt_w = {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(state.hwnd, &mut rc);
        }
        (rc.right - rc.left) as f32 / state.dpi_scale
    };
    let rt_h = {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(state.hwnd, &mut rc);
        }
        (rc.bottom - rc.top) as f32 / state.dpi_scale
    };

    let mut rects = CachedPopupRects::default();

    // Editor hover popup rect
    if let Some(ref eh) = screen.editor_hover {
        let lines = &eh.rendered.lines;
        if !lines.is_empty() {
            let max_height = 20;
            let scroll = eh.scroll_top;
            let visible_count = lines.len().saturating_sub(scroll).min(max_height);
            if visible_count > 0 {
                let num_lines = lines.len().min(max_height);
                let content_w = (eh.popup_width as f32 + 2.0) * cw;
                let popup_w = content_w.clamp(12.0 * cw, rt_w * 0.7);
                let popup_h = num_lines as f32 * lh + 8.0;

                let active = screen.windows.iter().find(|w| w.is_active);
                let (x, y) = if let Some(rw) = active {
                    let gutter_px = rw.gutter_char_width as f32 * cw;
                    let view_line = eh.anchor_line.saturating_sub(eh.frozen_scroll_top);
                    let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left);
                    let cx = rw.rect.x as f32 + gutter_px + vis_col as f32 * cw;
                    let cy = rw.rect.y as f32 + view_line as f32 * lh;
                    let fy = if cy >= popup_h + 4.0 {
                        cy - popup_h
                    } else {
                        cy + lh
                    };
                    (cx.min(rt_w - popup_w - 4.0).max(0.0), fy.max(0.0))
                } else {
                    (0.0, 0.0)
                };
                rects.editor_hover = Some(PopupRect {
                    x,
                    y,
                    w: popup_w,
                    h: popup_h,
                });
            }
        }
    }

    // Panel hover popup rect (mirrors draw_panel_hover positioning)
    if let Some(ref ph) = screen.panel_hover {
        let lines = &ph.rendered.lines;
        if !lines.is_empty() {
            let max_height = 20;
            let num_lines = lines.len().min(max_height);
            let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(10);
            let popup_w = ((max_len + 4) as f32 * cw).clamp(12.0 * cw, rt_w * 0.5);
            let popup_h = num_lines as f32 * lh + 8.0;
            let x = state.sidebar.total_width() + 2.0;
            let y = (ph.item_index as f32 * lh + lh * 2.0)
                .min(rt_h - popup_h)
                .max(0.0);
            rects.panel_hover = Some(PopupRect {
                x,
                y,
                w: popup_w,
                h: popup_h,
            });
        }
    }

    // Debug toolbar rect
    if let Some(ref toolbar) = screen.debug_toolbar {
        let btn_count = toolbar.buttons.len();
        if btn_count > 0 {
            let btn_w = cw * 4.0;
            let total_w = btn_count as f32 * btn_w;
            let x = (rt_w - total_w) / 2.0;
            let y = rt_h - 3.0 * lh; // above status+cmd
            rects.debug_toolbar = Some(PopupRect {
                x,
                y,
                w: total_w,
                h: lh,
            });
            for (i, btn) in toolbar.buttons.iter().enumerate() {
                let bx = x + i as f32 * btn_w;
                rects.debug_toolbar_buttons.push((
                    PopupRect {
                        x: bx,
                        y,
                        w: btn_w,
                        h: lh,
                    },
                    btn.action.to_string(),
                    btn.enabled,
                ));
            }
        }
    }

    state.popup_rects = rects;
}

/// Cache tab slot positions and window rects after each draw for mouse hit-testing.
/// Measure text width using the proportional UI font (same as draw_tabs).
fn measure_ui_text_width(dwrite: &IDWriteFactory, format: &IDWriteTextFormat, text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let layout: IDWriteTextLayout = dwrite
            .CreateTextLayout(&wide, format, 10000.0, 1000.0)
            .expect("CreateTextLayout");
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics).expect("GetMetrics");
        metrics.width
    }
}

fn cache_layout(
    state: &mut AppState,
    screen: &ScreenLayout,
    window_rects: &[(WindowId, WindowRect)],
) {
    let cw = state.char_width;
    let lh = state.line_height;

    // Cache tab slots from screen layout
    state.tab_slots.clear();
    if let Some(ref split) = screen.editor_group_split {
        // Multi-group: each group has its own tab bar
        for gtb in &split.group_tab_bars {
            let tab_h = lh * TAB_BAR_HEIGHT_MULT;
            let has_bc = screen
                .breadcrumbs
                .iter()
                .any(|bc| bc.group_id == gtb.group_id && !bc.segments.is_empty());
            let bc_offset = if has_bc { lh } else { 0.0 };
            let tab_y = gtb.bounds.y as f32 - tab_h - bc_offset;
            let mut x = gtb.bounds.x as f32;
            let group_right = gtb.bounds.x as f32 + gtb.bounds.width as f32;
            let pad = 12.0_f32;
            for (idx, tab) in gtb.tabs.iter().enumerate() {
                let text_w =
                    measure_ui_text_width(&state.dwrite_factory, &state.ui_text_format, &tab.name);
                let tab_w = pad + text_w + pad + cw + pad * 0.5;
                // Skip tabs that start past the group's right edge
                if x >= group_right {
                    break;
                }
                let clamped_end = (x + tab_w).min(group_right);
                let close_x = x + tab_w - cw - pad * 0.5;
                state.tab_slots.push(TabSlot {
                    group_id: gtb.group_id,
                    tab_idx: idx,
                    x_start: x,
                    x_end: clamped_end,
                    close_x_start: close_x,
                    y: tab_y,
                    height: tab_h,
                });
                x += tab_w;
            }
        }
    } else {
        // Single-group: use the flat tab_bar
        let group_id = state.engine.active_group;
        let tab_y = if screen.menu_bar.is_some() {
            TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT
        } else {
            0.0f32
        };
        let tab_h = lh * TAB_BAR_HEIGHT_MULT;
        let mut x = state.sidebar.total_width(); // start after sidebar
        let pad = 12.0_f32;
        for (idx, tab) in screen.tab_bar.iter().enumerate() {
            let text_w =
                measure_ui_text_width(&state.dwrite_factory, &state.ui_text_format, &tab.name);
            let tab_w = pad + text_w + pad + cw + pad * 0.5;
            let close_x = x + tab_w - cw - pad * 0.5;
            state.tab_slots.push(TabSlot {
                group_id,
                tab_idx: idx,
                x_start: x,
                x_end: x + tab_w,
                close_x_start: close_x,
                y: tab_y,
                height: tab_h,
            });
            x += tab_w;
        }
    }

    // Cache breadcrumb bars for click handling
    state.cached_breadcrumbs = screen.breadcrumbs.clone();

    // Cache group dividers for drag handling
    state.cached_dividers = if let Some(ref split) = screen.editor_group_split {
        split.dividers.clone()
    } else {
        Vec::new()
    };

    // Cache diff toolbar button positions
    state.cached_diff_toolbar_btns.clear();
    {
        let cache_diff_toolbar = |dt: &render::DiffToolbarData,
                                  bar_x: f32,
                                  bar_y: f32,
                                  bar_w: f32,
                                  bar_h: f32,
                                  group_id: GroupId|
         -> (GroupId, f32, f32, f32, f32, f32, f32) {
            let mut parts: Vec<String> = Vec::new();
            if let Some(ref label) = dt.change_label {
                parts.push(label.clone());
            }
            parts.push("\u{2191}".to_string());
            parts.push("\u{2193}".to_string());
            parts.push("\u{2261}".to_string());
            let label = parts.join("  ");
            let label_w = label.chars().count() as f32 * cw;
            let rx = bar_x + bar_w - label_w - cw * 2.0;
            // Skip past the change label and its trailing "  "
            let btn_offset = if let Some(ref cl) = dt.change_label {
                (cl.chars().count() + 2) as f32 * cw
            } else {
                0.0
            };
            let prev_x = rx + btn_offset;
            let next_x = prev_x + 3.0 * cw; // "↑  " = 3 chars
            let fold_x = next_x + 3.0 * cw; // "↓  " = 3 chars
            let btn_w = cw;
            (group_id, prev_x, next_x, fold_x, btn_w, bar_y, bar_h)
        };
        // Single-group diff toolbar
        if let Some(ref dt) = screen.diff_toolbar {
            let tab_y = if screen.menu_bar.is_some() {
                TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT
            } else {
                0.0f32
            };
            let tab_h = lh * TAB_BAR_HEIGHT_MULT;
            let bar_x = state.sidebar.total_width();
            let mut rc = RECT::default();
            unsafe {
                let _ = GetClientRect(state.hwnd, &mut rc);
            }
            let client_w = (rc.right - rc.left) as f32 / state.dpi_scale;
            let bar_w = client_w - bar_x;
            state.cached_diff_toolbar_btns.push(cache_diff_toolbar(
                dt,
                bar_x,
                tab_y,
                bar_w,
                tab_h,
                state.engine.active_group,
            ));
        }
        // Multi-group diff toolbars
        if let Some(ref split) = screen.editor_group_split {
            for gtb in &split.group_tab_bars {
                if let Some(ref dt) = gtb.diff_toolbar {
                    let tab_h = lh * TAB_BAR_HEIGHT_MULT;
                    let has_bc = screen
                        .breadcrumbs
                        .iter()
                        .any(|bc| bc.group_id == gtb.group_id && !bc.segments.is_empty());
                    let bc_offset = if has_bc { lh } else { 0.0 };
                    let tab_y = gtb.bounds.y as f32 - tab_h - bc_offset;
                    let bar_x = gtb.bounds.x as f32;
                    let bar_w = gtb.bounds.width as f32;
                    state.cached_diff_toolbar_btns.push(cache_diff_toolbar(
                        dt,
                        bar_x,
                        tab_y,
                        bar_w,
                        tab_h,
                        gtb.group_id,
                    ));
                }
            }
        }
    }

    // Cache window rects with gutter widths and group_id
    state.cached_window_rects.clear();
    for rw in &screen.windows {
        // Determine group_id by checking which group's bounds contain this window
        let gid = if let Some(ref split) = screen.editor_group_split {
            split
                .group_tab_bars
                .iter()
                .find(|gtb| {
                    let b = &gtb.bounds;
                    rw.rect.x >= b.x
                        && rw.rect.y >= b.y
                        && rw.rect.x < b.x + b.width
                        && rw.rect.y < b.y + b.height
                })
                .map(|gtb| gtb.group_id)
                .unwrap_or(state.engine.active_group)
        } else {
            state.engine.active_group
        };
        state.cached_window_rects.push(CachedWindowRect {
            window_id: rw.window_id,
            group_id: gid,
            rect: WindowRect::new(rw.rect.x, rw.rect.y, rw.rect.width, rw.rect.height),
            gutter_chars: rw.gutter_char_width,
        });
    }
}

fn on_resize(hwnd: HWND) {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
        }
        let size = D2D_SIZE_U {
            width: (rc.right - rc.left).max(1) as u32,
            height: (rc.bottom - rc.top).max(1) as u32,
        };

        if let Some(ref rt) = state.render_target {
            unsafe {
                let _ = rt.Resize(&size);
            }
        }

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let new_dpi = (wparam.0 & 0xFFFF) as u32;
    let new_scale = new_dpi as f32 / 96.0;

    // The suggested new window rect is in LPARAM
    let suggested_rect = unsafe { &*(lparam.0 as *const RECT) };

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        state.dpi_scale = new_scale;

        // Recreate text format with new DPI-scaled font size
        let font_size = state.engine.settings.font_size as f32 * new_scale;
        if let Ok(fmt) = unsafe {
            state.dwrite_factory.CreateTextFormat(
                w!("Consolas"),
                None,
                DWRITE_FONT_WEIGHT_REGULAR,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                w!("en-us"),
            )
        } {
            state.text_format = fmt;
            state.char_width = draw::measure_char_width(&state.dwrite_factory, &state.text_format);
            state.line_height =
                draw::measure_line_height(&state.dwrite_factory, &state.text_format);
        }

        // Resize window to suggested rect
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                suggested_rect.left,
                suggested_rect.top,
                suggested_rect.right - suggested_rect.left,
                suggested_rect.bottom - suggested_rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }

        // Recreate render target with new DPI (inline — can't call create_render_target
        // because we already hold the APP borrow)
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
        }
        let size = D2D_SIZE_U {
            width: (rc.right - rc.left).max(1) as u32,
            height: (rc.bottom - rc.top).max(1) as u32,
        };
        let dpi = new_scale * 96.0;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            ..Default::default()
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: size,
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        if let Ok(rt) = unsafe {
            state
                .d2d_factory
                .CreateHwndRenderTarget(&props, &hwnd_props)
        } {
            state.render_target = Some(rt);
        }

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_key_down(wparam: WPARAM, _lparam: LPARAM) -> bool {
    let vk = wparam.0 as u16;
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;

    let Some(key) = translate_vk(vk, ctrl, shift, alt) else {
        return false;
    };

    let should_quit = APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // ── Ctrl-T: toggle terminal ──────────────────────────────────────
        if ctrl && !shift && !alt && key.key_name == "t" {
            if state.engine.terminal_open && state.engine.terminal_has_focus {
                state.engine.close_terminal();
            } else if state.engine.terminal_open {
                state.engine.terminal_has_focus = true;
            } else {
                // Open terminal — compute cols from window width
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(state.hwnd, &mut rc);
                }
                let width = (rc.right - rc.left) as f32;
                let cols = ((width - state.sidebar.total_width()) / state.char_width) as u16;
                let rows = state.engine.session.terminal_panel_rows;
                if state.engine.terminal_panes.is_empty() {
                    state.engine.terminal_new_tab(cols, rows);
                }
                state.engine.terminal_open = true;
                state.engine.terminal_has_focus = true;
            }
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return false;
        }

        // ── Terminal key routing (when terminal has focus) ───────────────
        if state.engine.terminal_has_focus && state.engine.terminal_open {
            // Escape returns focus to editor
            if key.key_name == "Escape" {
                state.engine.terminal_has_focus = false;
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }

            // Ctrl+V / Ctrl+Shift+V: paste clipboard to PTY
            if ctrl && (key.key_name == "v" || key.key_name == "V") {
                let paste_text = state
                    .engine
                    .clipboard_read
                    .as_ref()
                    .and_then(|cb| cb().ok())
                    .filter(|t| !t.is_empty())
                    .or_else(|| {
                        state
                            .engine
                            .registers
                            .get(&'+')
                            .map(|(t, _)| t.clone())
                            .filter(|t| !t.is_empty())
                    })
                    .or_else(|| {
                        state
                            .engine
                            .registers
                            .get(&'"')
                            .map(|(t, _)| t.clone())
                            .filter(|t| !t.is_empty())
                    });
                if let Some(text) = paste_text {
                    state.engine.terminal_write(b"\x1b[200~");
                    state.engine.terminal_write(text.as_bytes());
                    state.engine.terminal_write(b"\x1b[201~");
                    state.engine.poll_terminal();
                } else {
                    state.engine.message = "Nothing to paste".to_string();
                }
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }

            // Ctrl+Y / Ctrl+Shift+C: copy terminal selection to clipboard
            if ctrl
                && ((key.key_name == "y" || key.key_name == "Y")
                    || (shift && (key.key_name == "c" || key.key_name == "C")))
            {
                let text = state
                    .engine
                    .active_terminal()
                    .and_then(|t| t.selected_text());
                if let Some(ref text) = text {
                    if let Some(ref cb) = state.engine.clipboard_write {
                        let _ = cb(text);
                    }
                    state.engine.message = "Copied".to_string();
                }
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }

            // Translate key to PTY escape sequence
            let data = translate_key_to_pty(&key.key_name, key.unicode, ctrl);
            if !data.is_empty() {
                state.engine.terminal_write(&data);
                state.engine.poll_terminal();
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
            }
            return false;
        }

        // Ctrl+Shift+E: toggle sidebar focus
        if ctrl && shift && key.key_name == "E" {
            if !state.sidebar.visible {
                state.sidebar.visible = true;
                state.sidebar.dirty = true;
                if state.sidebar.expanded.is_empty() {
                    if let Some(ref root) = state.engine.workspace_root.clone() {
                        state.sidebar.expanded.insert(root.clone());
                    }
                }
            }
            state.sidebar.has_focus = !state.sidebar.has_focus;
            state.sidebar.active_panel = SidebarPanel::Explorer;
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return false;
        }

        // ── Settings panel keyboard handling ─────────────────────────────
        if state.sidebar.has_focus
            && state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Settings
        {
            // Ctrl+V paste into search input or inline edit
            if ctrl && key.key_name == "v" {
                if state.engine.settings_input_active || state.engine.settings_editing.is_some() {
                    let text = match state.engine.clipboard_read {
                        Some(ref cb) => cb().ok(),
                        None => None,
                    };
                    if let Some(t) = text {
                        state.engine.settings_paste(&t);
                    }
                }
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }

            let (key_name, unicode): (&str, Option<char>) = match key.key_name.as_str() {
                "j" | "Down" => ("j", None),
                "k" | "Up" => ("k", None),
                "Tab" => ("Tab", None),
                "Return" => ("Return", None),
                "space" | " " => ("Space", None),
                "l" | "Right" => ("l", None),
                "h" | "Left" => ("h", None),
                "/" => ("/", None),
                "q" | "Escape" => ("Escape", None),
                "BackSpace" => ("BackSpace", None),
                _ => {
                    if let Some(ch) = key.unicode {
                        ("char", Some(ch))
                    } else {
                        ("", None)
                    }
                }
            };
            if !key_name.is_empty() {
                let ch = if key_name == "char" { unicode } else { None };
                state.engine.handle_settings_key(
                    if key_name == "char" { "" } else { key_name },
                    ctrl,
                    ch,
                );
                if !state.engine.settings_has_focus {
                    state.sidebar.has_focus = false;
                }
                // Keep selected item visible after navigation
                let content_h = {
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(state.hwnd, &mut rc);
                    }
                    let h = (rc.bottom - rc.top) as f32;
                    ((h - state.bottom_chrome_px - state.line_height * 2.0) / state.line_height)
                        .floor() as usize
                };
                if content_h > 0 {
                    if state.engine.settings_selected
                        >= state.engine.settings_scroll_top + content_h
                    {
                        state.engine.settings_scroll_top =
                            state.engine.settings_selected - content_h + 1;
                    } else if state.engine.settings_selected < state.engine.settings_scroll_top {
                        state.engine.settings_scroll_top = state.engine.settings_selected;
                    }
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return false;
        }

        // ── Extensions panel keyboard handling ─────────────────────────
        if state.engine.ext_sidebar_has_focus
            && state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Extensions
        {
            let (key_name, unicode): (&str, Option<char>) = match key.key_name.as_str() {
                "j" | "Down" => ("j", None),
                "k" | "Up" => ("k", None),
                "Return" => ("Return", None),
                "Tab" => ("Tab", None),
                "Escape" => ("Escape", None),
                "BackSpace" => ("BackSpace", None),
                _ => {
                    // Map single letters to their key name (matching TUI behavior)
                    if let Some(ch) = key.unicode {
                        match ch {
                            'i' => ("i", None),
                            'd' => ("d", None),
                            'u' => ("u", None),
                            'r' => ("r", None),
                            '/' => ("/", None),
                            'q' => ("Escape", None),
                            _ if !ch.is_control() => ("char", Some(ch)),
                            _ => ("", None),
                        }
                    } else {
                        ("", None)
                    }
                }
            };
            if !key_name.is_empty() {
                let ch = if key_name == "char" { unicode } else { None };
                state.engine.handle_ext_sidebar_key(
                    if key_name == "char" { "" } else { key_name },
                    ctrl,
                    ch,
                );
                if !state.engine.ext_sidebar_has_focus {
                    state.sidebar.has_focus = false;
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return false;
        }

        // Sidebar keyboard navigation when focused (Explorer only — other panels
        // are handled by the engine via handle_key → handle_sc_key / etc.)
        if state.sidebar.has_focus
            && state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Explorer
        {
            let handled = match key.key_name.as_str() {
                "Escape" => {
                    state.sidebar.has_focus = false;
                    true
                }
                "Up" | "k" if !ctrl => {
                    state.sidebar.selected = state.sidebar.selected.saturating_sub(1);
                    true
                }
                "Down" | "j" if !ctrl => {
                    if state.sidebar.selected + 1 < state.sidebar.rows.len() {
                        state.sidebar.selected += 1;
                    }
                    true
                }
                "Return" | "Right" | "l" if !ctrl => {
                    if state.sidebar.active_panel == SidebarPanel::Explorer {
                        let idx = state.sidebar.selected;
                        if idx < state.sidebar.rows.len() {
                            let is_dir = state.sidebar.rows[idx].is_dir;
                            let path = state.sidebar.rows[idx].path.clone();
                            if is_dir {
                                state.sidebar.toggle_expand(idx);
                                if let Some(ref root) = state.engine.workspace_root.clone() {
                                    state.sidebar.build_rows(root);
                                }
                            } else {
                                state.engine.open_file_in_tab(&path);
                                state.sidebar.has_focus = false;
                            }
                        }
                    }
                    true
                }
                "Left" | "h" if !ctrl => {
                    // Collapse directory
                    if state.sidebar.active_panel == SidebarPanel::Explorer {
                        let idx = state.sidebar.selected;
                        if idx < state.sidebar.rows.len()
                            && state.sidebar.rows[idx].is_dir
                            && state.sidebar.rows[idx].is_expanded
                        {
                            state.sidebar.toggle_expand(idx);
                            if let Some(ref root) = state.engine.workspace_root.clone() {
                                state.sidebar.build_rows(root);
                            }
                        }
                    }
                    true
                }
                _ => false,
            };
            if handled {
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }
        }

        // ── Menu bar keyboard handling ────────────────────────────────────
        if state.engine.menu_open_idx.is_some() {
            let handled = match key.key_name.as_str() {
                "Escape" => {
                    state.engine.close_menu();
                    true
                }
                "Down" => {
                    let seps: Vec<bool> = state
                        .engine
                        .menu_open_idx
                        .and_then(|idx| MENU_STRUCTURE.get(idx))
                        .map(|(_, _, items)| items.iter().map(|i| i.separator).collect())
                        .unwrap_or_default();
                    state.engine.menu_move_selection(1, &seps);
                    true
                }
                "Up" => {
                    let seps: Vec<bool> = state
                        .engine
                        .menu_open_idx
                        .and_then(|idx| MENU_STRUCTURE.get(idx))
                        .map(|(_, _, items)| items.iter().map(|i| i.separator).collect())
                        .unwrap_or_default();
                    state.engine.menu_move_selection(-1, &seps);
                    true
                }
                "Left" => {
                    let cur = state.engine.menu_open_idx.unwrap_or(0);
                    let prev = if cur == 0 {
                        MENU_STRUCTURE.len() - 1
                    } else {
                        cur - 1
                    };
                    state.engine.open_menu(prev);
                    true
                }
                "Right" => {
                    let cur = state.engine.menu_open_idx.unwrap_or(0);
                    let next = (cur + 1) % MENU_STRUCTURE.len();
                    state.engine.open_menu(next);
                    true
                }
                "Return" => {
                    if let Some((midx, item_idx)) = state.engine.menu_activate_highlighted() {
                        if let Some((_, _, items)) = MENU_STRUCTURE.get(midx) {
                            if let Some(item) = items.get(item_idx) {
                                activate_menu_item(state, midx, item_idx, item.action);
                            }
                        }
                    }
                    true
                }
                _ => false,
            };
            if handled {
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }
        }

        // Alt+letter opens menu bar menus (F=File, E=Edit, etc.)
        if alt && !ctrl && !shift {
            let ch = key.key_name.strip_prefix("Alt-").unwrap_or("");
            if ch.len() == 1 {
                let letter = ch.chars().next().unwrap();
                for (idx, (_, hotkey, _)) in MENU_STRUCTURE.iter().enumerate() {
                    if letter == *hotkey {
                        if state.engine.menu_open_idx == Some(idx) {
                            state.engine.close_menu();
                        } else {
                            state.engine.open_menu(idx);
                        }
                        unsafe {
                            let _ = InvalidateRect(Some(state.hwnd), None, false);
                        }
                        return false;
                    }
                }
            }
        }

        // Backend-level Alt key handling (matches TUI behavior)
        if key.key_name.starts_with("Alt-") {
            let handled = match key.key_name.as_str() {
                "Alt-m" => {
                    state.engine.toggle_editor_mode();
                    true
                }
                "Alt-," => {
                    state.engine.group_resize(-0.05);
                    true
                }
                "Alt-." => {
                    state.engine.group_resize(0.05);
                    true
                }
                _ => false,
            };
            if handled {
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return false;
            }
        }

        // Intercept paste keys to load system clipboard into registers
        if !key.ctrl && intercept_paste_key(state, &key.key_name, key.unicode) {
            sync_clipboard(state);
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return false;
        }

        // Ctrl+V in Insert/Replace/Command mode: paste from system clipboard
        if key.ctrl
            && (key.key_name == "v" || key.key_name == "V")
            && matches!(
                state.engine.mode,
                crate::core::Mode::Insert | crate::core::Mode::Replace
            )
        {
            if let Some(ref cb_read) = state.engine.clipboard_read {
                if let Ok(text) = cb_read() {
                    if !text.is_empty() {
                        for ch in text.chars() {
                            if ch == '\n' || ch == '\r' {
                                state.engine.handle_key("Return", None, false);
                            } else if !ch.is_control() {
                                state.engine.handle_key("", Some(ch), false);
                            }
                        }
                    }
                }
            }
            sync_clipboard(state);
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return false;
        }

        let action = state
            .engine
            .handle_key(&key.key_name, key.unicode, key.ctrl);
        let quit = handle_action_with_sidebar(state, action);
        sync_clipboard(state);
        unsafe {
            let _ = InvalidateRect(Some(state.hwnd), None, false);
        }
        quit
    });

    if should_quit {
        unsafe {
            let _ = DestroyWindow(APP.with(|app| app.borrow().as_ref().unwrap().hwnd));
        }
    }

    true
}

fn on_char(wparam: WPARAM) {
    let ch = char::from_u32(wparam.0 as u32).unwrap_or('\0');

    // Route characters to terminal if it has focus
    let terminal_handled = APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");
        if state.engine.terminal_has_focus && state.engine.terminal_open {
            if !ch.is_control() {
                state.engine.terminal_write(ch.to_string().as_bytes());
                state.engine.poll_terminal();
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
            }
            return true;
        }
        false
    });
    if terminal_handled {
        return;
    }

    let Some(key) = translate_char(ch) else {
        return;
    };

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // Settings panel character key handling
        if state.sidebar.has_focus
            && state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Settings
        {
            let (key_name, unicode): (&str, Option<char>) = match ch {
                'j' => ("j", None),
                'k' => ("k", None),
                'l' => ("l", None),
                'h' => ("h", None),
                '/' => ("/", None),
                'q' => ("Escape", None),
                ' ' => ("Space", None),
                c if !c.is_control() => ("char", Some(c)),
                _ => ("", None),
            };
            if !key_name.is_empty() {
                let ch_arg = if key_name == "char" { unicode } else { None };
                state.engine.handle_settings_key(
                    if key_name == "char" { "" } else { key_name },
                    false,
                    ch_arg,
                );
                if !state.engine.settings_has_focus {
                    state.sidebar.has_focus = false;
                }
                // Keep selected item visible
                let content_h = {
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(state.hwnd, &mut rc);
                    }
                    let h = (rc.bottom - rc.top) as f32;
                    ((h - state.bottom_chrome_px - state.line_height * 2.0) / state.line_height)
                        .floor() as usize
                };
                if content_h > 0 {
                    if state.engine.settings_selected
                        >= state.engine.settings_scroll_top + content_h
                    {
                        state.engine.settings_scroll_top =
                            state.engine.settings_selected - content_h + 1;
                    } else if state.engine.settings_selected < state.engine.settings_scroll_top {
                        state.engine.settings_scroll_top = state.engine.settings_selected;
                    }
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return;
        }

        // Extensions panel character key handling
        if state.engine.ext_sidebar_has_focus
            && state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Extensions
        {
            let (key_name, unicode): (&str, Option<char>) = match ch {
                'j' => ("j", None),
                'k' => ("k", None),
                'i' => ("i", None),
                'd' => ("d", None),
                'u' => ("u", None),
                'r' => ("r", None),
                '/' => ("/", None),
                'q' => ("Escape", None),
                c if !c.is_control() => ("char", Some(c)),
                _ => ("", None),
            };
            if !key_name.is_empty() {
                let ch_arg = if key_name == "char" { unicode } else { None };
                state.engine.handle_ext_sidebar_key(
                    if key_name == "char" { "" } else { key_name },
                    false,
                    ch_arg,
                );
                if !state.engine.ext_sidebar_has_focus {
                    state.sidebar.has_focus = false;
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return;
        }

        // Sidebar keyboard navigation for character keys (Explorer only)
        if state.sidebar.has_focus
            && state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Explorer
        {
            let handled = match ch {
                'j' => {
                    if state.sidebar.selected + 1 < state.sidebar.rows.len() {
                        state.sidebar.selected += 1;
                    }
                    true
                }
                'k' => {
                    state.sidebar.selected = state.sidebar.selected.saturating_sub(1);
                    true
                }
                'l' => {
                    if state.sidebar.active_panel == SidebarPanel::Explorer {
                        let idx = state.sidebar.selected;
                        if idx < state.sidebar.rows.len() {
                            let is_dir = state.sidebar.rows[idx].is_dir;
                            let path = state.sidebar.rows[idx].path.clone();
                            if is_dir {
                                if !state.sidebar.rows[idx].is_expanded {
                                    state.sidebar.toggle_expand(idx);
                                    if let Some(ref root) = state.engine.workspace_root.clone() {
                                        state.sidebar.build_rows(root);
                                    }
                                }
                            } else {
                                state.engine.open_file_in_tab(&path);
                                state.sidebar.has_focus = false;
                            }
                        }
                    }
                    true
                }
                'h' => {
                    if state.sidebar.active_panel == SidebarPanel::Explorer {
                        let idx = state.sidebar.selected;
                        if idx < state.sidebar.rows.len()
                            && state.sidebar.rows[idx].is_dir
                            && state.sidebar.rows[idx].is_expanded
                        {
                            state.sidebar.toggle_expand(idx);
                            if let Some(ref root) = state.engine.workspace_root.clone() {
                                state.sidebar.build_rows(root);
                            }
                        }
                    }
                    true
                }
                _ => false,
            };
            if handled {
                unsafe {
                    let _ = InvalidateRect(Some(state.hwnd), None, false);
                }
                return;
            }
        }

        // Intercept paste keys to load system clipboard
        if !key.ctrl && intercept_paste_key(state, &key.key_name, key.unicode) {
            sync_clipboard(state);
            unsafe {
                let _ = InvalidateRect(Some(state.hwnd), None, false);
            }
            return;
        }

        let action = state
            .engine
            .handle_key(&key.key_name, key.unicode, key.ctrl);
        let _ = handle_action_with_sidebar(state, action);
        sync_clipboard(state);
        unsafe {
            let _ = InvalidateRect(Some(state.hwnd), None, false);
        }
    });
}

/// Position the IME composition window at the current cursor location.
fn on_ime_start_composition(hwnd: HWND) {
    APP.with(|app| {
        let app = app.borrow();
        let Some(state) = app.as_ref() else { return };

        // Find the active window's cached rect and cursor position
        let active_wid = state.engine.active_window_id();
        let Some(cwr) = state
            .cached_window_rects
            .iter()
            .find(|r| r.window_id == active_wid)
        else {
            return;
        };

        let cursor = state.engine.cursor();
        let cursor_line = cursor.line;
        let cursor_col = cursor.col;
        let scroll_top = state.engine.view().scroll_top;
        let cw = state.char_width;
        let lh = state.line_height;

        // The editor area starts after gutter + rect origin
        let tab_bar_rows = if state.engine.settings.breadcrumbs {
            2.0
        } else {
            1.0
        };
        let gutter_px = cwr.gutter_chars as f32 * cw;
        let x = cwr.rect.x as f32 + gutter_px + (cursor_col as f32 * cw);
        let y = cwr.rect.y as f32 + tab_bar_rows * lh + ((cursor_line - scroll_top) as f32 * lh);

        // Convert from DIPs to physical pixels for the IME API
        let scale = state.dpi_scale;
        let px = (x * scale) as i32;
        let py = (y * scale) as i32;

        unsafe {
            let himc = ImmGetContext(hwnd);
            if !himc.is_invalid() {
                let cf = COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: POINT { x: px, y: py },
                    ..Default::default()
                };
                let _ = ImmSetCompositionWindow(himc, &cf);
                let _ = ImmReleaseContext(hwnd, himc);
            }
        }
    });
}

/// Check if a DIP-space coordinate is over a caption button.
/// Returns Some(0)=minimize, Some(1)=maximize, Some(2)=close, or None.
fn caption_button_at(px: f32, py: f32) -> Option<usize> {
    APP.with(|app| {
        let app = app.borrow();
        let state = app.as_ref()?;
        let title_h = TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT;
        if py >= title_h {
            return None; // below title bar row
        }
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(state.hwnd, &mut rc);
        }
        let client_width = (rc.right - rc.left) as f32 / state.dpi_scale;
        let btn_total = CAPTION_BTN_COUNT * CAPTION_BTN_WIDTH;
        let btn_start = client_width - btn_total;
        if px < btn_start {
            return None;
        }
        Some(((px - btn_start) / CAPTION_BTN_WIDTH) as usize)
    })
}

/// Extract pixel coordinates from LPARAM.
fn lparam_xy(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    // Convert physical pixels to DIPs (D2D coordinate space)
    let scale = APP.with(|app| app.borrow().as_ref().map_or(1.0, |state| state.dpi_scale));
    (x / scale, y / scale)
}

/// Find which cached window rect contains the given pixel position, and convert
/// to (window_id, buffer_line, column).
fn pixel_to_editor_pos(state: &AppState, px: f32, py: f32) -> Option<(WindowId, usize, usize)> {
    let cw = state.char_width;
    let lh = state.line_height;

    for cwr in &state.cached_window_rects {
        let rx = cwr.rect.x as f32;
        let ry = cwr.rect.y as f32;
        let rw = cwr.rect.width as f32;
        let rh = cwr.rect.height as f32;

        // Exclude scrollbar area (rightmost SCROLLBAR_WIDTH px)
        // Exclude per-window status bar (bottom line_height px)
        let status_h = if state.engine.settings.window_status_line {
            lh
        } else {
            0.0
        };
        if px >= rx && px < rx + rw - SCROLLBAR_WIDTH && py >= ry && py < ry + rh - status_h {
            let gutter_px = cwr.gutter_chars as f32 * cw;
            let view_row = ((py - ry) / lh).floor().max(0.0) as usize;

            // Look up the buffer line from the RenderedWindow lines
            // (handles wrapped lines). We use scroll_top + view_row as fallback.
            let w = state.engine.windows.get(&cwr.window_id);
            let scroll_top = w.map_or(0, |w| w.view.scroll_top);
            let buf_line = scroll_top + view_row;

            let text_x = px - rx - gutter_px;
            let scroll_left = w.map_or(0, |w| w.view.scroll_left);
            let col = (text_x / cw).max(0.0).floor() as usize + scroll_left;

            return Some((cwr.window_id, buf_line, col));
        }
    }
    None
}

/// Check if (px, py) is inside a scrollbar track; if so, return the window ID
/// and the scroll_top that corresponds to clicking at that Y position.
fn scrollbar_hit(state: &AppState, px: f32, py: f32) -> Option<(WindowId, usize)> {
    let lh = state.line_height;
    for cwr in &state.cached_window_rects {
        let rx = cwr.rect.x as f32;
        let ry = cwr.rect.y as f32;
        let rw_px = cwr.rect.width as f32;
        let rh_px = cwr.rect.height as f32;
        let sb_x = rx + rw_px - SCROLLBAR_WIDTH;

        if px >= sb_x && px < rx + rw_px && py >= ry && py < ry + rh_px {
            let w = state.engine.windows.get(&cwr.window_id);
            let total_lines = w.map_or(1, |w| {
                let bid = w.buffer_id;
                state
                    .engine
                    .buffer_manager
                    .get(bid)
                    .map_or(1, |bs| bs.buffer.len_lines())
            });
            // Subtract status line row from editor height
            let has_status = w.is_some_and(|_| state.engine.settings.window_status_line);
            let editor_h = rh_px - if has_status { lh } else { 0.0 };
            let viewport_lines = (editor_h / lh).floor() as usize;

            if total_lines <= viewport_lines {
                return None; // no scrollbar needed
            }

            let rel_y = (py - ry).clamp(0.0, editor_h);
            let new_top = crate::render::scrollbar_click_to_scroll_top(
                rel_y as f64,
                editor_h as f64,
                total_lines,
                viewport_lines,
            );
            return Some((cwr.window_id, new_top));
        }
    }
    None
}

fn on_mouse_down(hwnd: HWND, lparam: LPARAM) {
    let (px, py) = lparam_xy(lparam);
    let ix = (lparam.0 & 0xFFFF) as i16;
    let iy = ((lparam.0 >> 16) & 0xFFFF) as i16;

    // ── Caption button clicks (custom title bar) ─────────────────────
    {
        let btn = caption_button_at(px, py);
        if let Some(idx) = btn {
            unsafe {
                match idx {
                    0 => {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    1 => {
                        if is_maximized(hwnd) {
                            let _ = ShowWindow(hwnd, SW_RESTORE);
                        } else {
                            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                        }
                    }
                    _ => {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            }
            return;
        }
    }

    // Capture mouse so we get drag events even outside the window
    unsafe {
        SetCapture(hwnd);
    }

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // ── Sidebar resize drag start ────────────────────────────────────
        let edge = state.sidebar.total_width();
        if state.sidebar.visible && (px - edge).abs() < SIDEBAR_RESIZE_HIT_PX {
            state.sidebar_resize_drag = true;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Menu dropdown item click (must be before sidebar/activity bar) ──
        if let Some(midx) = state.engine.menu_open_idx {
            let cw = state.char_width;
            let lh = state.line_height;
            // Compute dropdown position (must match draw.rs logic)
            let pad = 8.0_f32;
            let mut popup_x = pad;
            for i in 0..midx {
                if let Some((name, _, _)) = MENU_STRUCTURE.get(i) {
                    popup_x +=
                        measure_ui_text_width(&state.dwrite_factory, &state.ui_text_format, name)
                            + pad * 2.0;
                }
            }
            let items = if let Some((_, _, items)) = MENU_STRUCTURE.get(midx) {
                items.to_vec()
            } else {
                Vec::new()
            };
            let max_label = items.iter().map(|i| i.label.len()).max().unwrap_or(4);
            let max_sc = items
                .iter()
                .map(|i| {
                    if state.engine.is_vscode_mode() && !i.vscode_shortcut.is_empty() {
                        i.vscode_shortcut.len()
                    } else {
                        i.shortcut.len()
                    }
                })
                .max()
                .unwrap_or(0);
            let popup_w = (max_label + max_sc + 6).clamp(20, 50) as f32 * cw;
            let popup_h = (items.len() as f32 + 1.0) * lh;
            let popup_y = TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT;

            // Check if click is on the menu bar labels (to switch menus)
            if py < lh {
                let pad = 8.0_f32;
                let mut label_x = pad;
                for (idx, (name, _, _)) in MENU_STRUCTURE.iter().enumerate() {
                    let label_w =
                        measure_ui_text_width(&state.dwrite_factory, &state.ui_text_format, name)
                            + pad * 2.0;
                    if px >= label_x && px < label_x + label_w {
                        if state.engine.menu_open_idx == Some(idx) {
                            state.engine.close_menu();
                        } else {
                            state.engine.open_menu(idx);
                        }
                        unsafe {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                        return;
                    }
                    label_x += label_w;
                }
            }

            // Check if click is inside the dropdown
            if px >= popup_x && px < popup_x + popup_w && py >= popup_y && py < popup_y + popup_h {
                let rel_y = py - popup_y - lh * 0.25;
                if rel_y < 0.0 {
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                }
                let item_idx = (rel_y / lh).floor() as usize;
                if item_idx < items.len() && !items[item_idx].separator && items[item_idx].enabled {
                    activate_menu_item(state, midx, item_idx, items[item_idx].action);
                }
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }

            // Click outside dropdown — close it and consume the click
            state.engine.close_menu();
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Menu bar clicks (when no dropdown is open) ───────────────────
        let title_bar_h = TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT;
        if state.engine.menu_bar_visible && py < title_bar_h {
            let pad = 8.0_f32;
            let mut label_x = pad;
            for (idx, (name, _, _)) in MENU_STRUCTURE.iter().enumerate() {
                let label_w =
                    measure_ui_text_width(&state.dwrite_factory, &state.ui_text_format, name)
                        + pad * 2.0;
                if px >= label_x && px < label_x + label_w {
                    state.engine.open_menu(idx);
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                }
                label_x += label_w;
            }
        }

        // ── Check activity bar clicks ────────────────────────────────────
        let ab_w = state.sidebar.activity_bar_px;
        let menu_y = if state.engine.menu_bar_visible {
            TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT
        } else {
            0.0
        };
        if px < ab_w && py >= menu_y {
            // Check if clicking the bottom-pinned Settings gear.
            // The gear is drawn at sidebar_bottom - line_height where sidebar_bottom = rt_h - bottom_chrome.
            // Use physical pixel coords (iy) directly for reliable hit-testing.
            let client_h = unsafe {
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                (rc.bottom - rc.top) as f32
            };
            let icon_row_h = ab_w; // square cells matching activity bar width
            let settings_y = client_h / state.dpi_scale - state.bottom_chrome_px - icon_row_h;
            let clicked_panel = if py >= settings_y {
                Some(SidebarPanel::Settings)
            } else {
                let row = ((py - menu_y) / icon_row_h).floor() as usize;
                let panels = [
                    SidebarPanel::Explorer,
                    SidebarPanel::Search,
                    SidebarPanel::Debug,
                    SidebarPanel::Git,
                    SidebarPanel::Extensions,
                    SidebarPanel::Ai,
                ];
                panels.get(row).copied()
            };
            if let Some(clicked_panel) = clicked_panel {
                if state.sidebar.visible && state.sidebar.active_panel == clicked_panel {
                    state.sidebar.visible = false;
                    state.sidebar.has_focus = false;
                    state.engine.clear_sidebar_focus();
                } else {
                    state.sidebar.active_panel = clicked_panel;
                    state.sidebar.visible = true;
                    state.sidebar.dirty = true;
                    state.sidebar.has_focus = true;
                    // Set engine-side focus for the appropriate panel
                    state.engine.clear_sidebar_focus();
                    match clicked_panel {
                        SidebarPanel::Settings => state.engine.settings_has_focus = true,
                        SidebarPanel::Git => {
                            state.engine.sc_has_focus = true;
                            state.engine.sc_refresh();
                        }
                        SidebarPanel::Extensions => state.engine.ext_sidebar_has_focus = true,
                        SidebarPanel::Ai => state.engine.ai_has_focus = true,
                        _ => {}
                    }
                    // Auto-expand root
                    if clicked_panel == SidebarPanel::Explorer && state.sidebar.expanded.is_empty()
                    {
                        if let Some(ref root) = state.engine.workspace_root.clone() {
                            state.sidebar.expanded.insert(root.clone());
                        }
                    }
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Check sidebar panel clicks ──────────────────────────────────
        if state.sidebar.visible && px < state.sidebar.total_width() && py >= menu_y {
            state.sidebar.has_focus = px >= ab_w; // focus panel area, not activity bar
            let row = ((py - menu_y) / state.line_height).floor() as usize;

            if state.sidebar.active_panel == SidebarPanel::Explorer {
                // Row 0 is the "EXPLORER" header — tree starts at row 1
                if row == 0 {
                    // Header click — ignore
                } else {
                    let tree_row = row - 1; // adjust for header
                    let vis_idx = state.sidebar.scroll_top + tree_row;
                    if vis_idx < state.sidebar.rows.len() {
                        state.sidebar.selected = vis_idx;
                        let is_dir = state.sidebar.rows[vis_idx].is_dir;
                        let path = state.sidebar.rows[vis_idx].path.clone();
                        if is_dir {
                            state.sidebar.toggle_expand(vis_idx);
                            if let Some(ref root) = state.engine.workspace_root.clone() {
                                state.sidebar.build_rows(root);
                            }
                        } else {
                            state.engine.open_file_preview(&path);
                        }
                    }
                }
            } else if state.sidebar.active_panel == SidebarPanel::Git {
                // Git panel: row 0 = header, row 1 = branch, row 2+ = content
                // Route through engine's sc_ methods
                if row >= 2 {
                    // Map visual row to flat index for the source control panel
                    let adjusted = row; // visual row in the panel
                    if let Some((flat_idx, is_header)) =
                        state.engine.sc_visual_row_to_flat(adjusted, true)
                    {
                        if is_header {
                            state.engine.handle_sc_key("Tab", false, None);
                        } else {
                            state.engine.sc_selected = flat_idx;
                        }
                    }
                }
                state.engine.sc_has_focus = true;
            } else if state.sidebar.active_panel == SidebarPanel::Extensions {
                // Extensions panel — Y layout must match draw_extensions_panel:
                // row 0: "EXTENSIONS" header at top
                // installed header at top + lh * 1.5
                // items follow at lh each
                // 0.3 * lh gap before available header
                state.engine.ext_sidebar_has_focus = true;
                let lh = state.line_height;
                let click_y = py - menu_y;
                let installed_len = state.engine.ext_installed_items().len();
                let inst_expanded = state.engine.ext_sidebar_sections_expanded[0];

                let inst_header_y = lh * 1.5;
                let inst_items_start_y = inst_header_y + lh;
                let inst_items_end_y = inst_items_start_y
                    + if inst_expanded {
                        installed_len as f32 * lh
                    } else {
                        0.0
                    };
                let avail_header_y = inst_items_end_y + lh * 0.3;
                let avail_items_start_y = avail_header_y + lh;

                if click_y >= inst_header_y && click_y < inst_header_y + lh {
                    state.engine.ext_sidebar_sections_expanded[0] = !inst_expanded;
                } else if inst_expanded
                    && click_y >= inst_items_start_y
                    && click_y < inst_items_end_y
                {
                    let idx = ((click_y - inst_items_start_y) / lh).floor() as usize;
                    if idx < installed_len {
                        state.engine.ext_sidebar_selected = idx;
                    }
                } else if click_y >= avail_header_y && click_y < avail_header_y + lh {
                    state.engine.ext_sidebar_sections_expanded[1] =
                        !state.engine.ext_sidebar_sections_expanded[1];
                } else if click_y >= avail_items_start_y {
                    let avail_idx = ((click_y - avail_items_start_y) / lh).floor() as usize;
                    let avail_len = state.engine.ext_available_items().len();
                    if avail_idx < avail_len {
                        state.engine.ext_sidebar_selected = installed_len + avail_idx;
                    }
                }
            } else if state.sidebar.active_panel == SidebarPanel::Settings {
                state.engine.settings_has_focus = true;
                if row >= 2 {
                    let fi = state.engine.settings_scroll_top + row - 2;
                    state.engine.settings_selected = fi;
                }
            } else if state.sidebar.active_panel == SidebarPanel::Ai {
                state.engine.ai_has_focus = true;
            } else if state.sidebar.active_panel == SidebarPanel::Search {
                state.engine.search_has_focus = true;
            } else if state.sidebar.active_panel == SidebarPanel::Debug {
                state.engine.dap_sidebar_has_focus = true;
            }

            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Scrollbar click-to-jump ──────────────────────────────────────
        if let Some((wid, new_top)) = scrollbar_hit(state, px, py) {
            state.scrollbar_drag = Some(wid);
            state.engine.set_scroll_top_for_window(wid, new_top);
            state.engine.sync_scroll_binds();
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Dialog button click handling (highest z-order) ────────────────
        if state.engine.dialog.is_some() {
            let lh = state.line_height;
            let cw = state.char_width;
            let (rt_w, rt_h) = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                (
                    (rc.right - rc.left) as f32 / state.dpi_scale,
                    (rc.bottom - rc.top) as f32 / state.dpi_scale,
                )
            };
            // Match geometry from draw_dialog (auto-sized width)
            let dialog = state.engine.dialog.as_ref().unwrap();
            let btn_total_w: f32 = dialog
                .buttons
                .iter()
                .map(|b| (b.label.chars().count() as f32 + 2.0) * cw + cw)
                .sum::<f32>()
                + cw * 2.0;
            let body_max_w = dialog
                .body
                .iter()
                .map(|line| line.chars().count() as f32 * cw + cw * 4.0)
                .fold(0.0f32, f32::max);
            let title_w = dialog.title.chars().count() as f32 * cw + cw * 4.0;
            let content_w = btn_total_w.max(body_max_w).max(title_w);
            let dialog_w = content_w.max(300.0).min(rt_w - 40.0);
            let dialog_h = (dialog.body.len() as f32 + 3.0) * lh + 20.0;
            let dx = (rt_w - dialog_w) / 2.0;
            let dy = (rt_h - dialog_h) / 2.0;
            let btn_y = dy + dialog_h - lh - 8.0;

            // Hit-test buttons (laid out right-to-left, matching draw_dialog)
            let mut bx = dx + dialog_w - cw;
            let btn_count = dialog.buttons.len();
            let mut clicked_btn: Option<usize> = None;
            for (i, btn) in dialog.buttons.iter().enumerate().rev() {
                let btn_w = (btn.label.chars().count() as f32 + 2.0) * cw;
                bx -= btn_w;
                if px >= bx && px < bx + btn_w && py >= btn_y && py < btn_y + lh {
                    clicked_btn = Some(i);
                    break;
                }
                bx -= cw;
            }

            let on_dialog = px >= dx && px < dx + dialog_w && py >= dy && py < dy + dialog_h;

            if let Some(idx) = clicked_btn {
                let action = state.engine.dialog_click_button(idx);
                let quit = handle_action_with_sidebar(state, action);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                if quit {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
            } else if !on_dialog {
                // Click outside dialog — dismiss
                state.engine.dialog = None;
                state.engine.pending_move = None;
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Picker click handling (intercept all clicks when picker is open) ──
        if state.engine.picker_open {
            let lh = state.line_height;
            let (rt_w, rt_h) = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                (
                    (rc.right - rc.left) as f32 / state.dpi_scale,
                    (rc.bottom - rc.top) as f32 / state.dpi_scale,
                )
            };
            // Match geometry from draw_picker
            let max_visible = 12usize;
            let has_preview = state.engine.picker_preview.is_some();
            let list_w = if has_preview { rt_w * 0.4 } else { rt_w * 0.6 };
            let total_w = if has_preview { rt_w * 0.8 } else { list_w };
            let header_h = lh * 2.0; // title + input
            let body_h = max_visible as f32 * lh;
            let total_h = header_h + body_h;
            let popup_x = (rt_w - total_w) / 2.0;
            let popup_y = rt_h * 0.15;

            let on_popup =
                px >= popup_x && px < popup_x + total_w && py >= popup_y && py < popup_y + total_h;
            let results_top = popup_y + header_h;
            let results_bottom = popup_y + total_h;
            let on_results =
                on_popup && py >= results_top && py < results_bottom && px < popup_x + list_w;

            if on_results {
                let clicked_idx =
                    state.engine.picker_scroll_top + ((py - results_top) / lh) as usize;
                if clicked_idx < state.engine.picker_items.len() {
                    state.engine.picker_selected = clicked_idx;
                    state.engine.picker_load_preview();
                }
            } else if !on_popup {
                state.engine.close_picker();
            }
            // Consume click — don't fall through to editor
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Context menu click handling (must be before any other click) ──────
        if state.engine.context_menu.is_some() {
            let handled = {
                let cm = state.engine.context_menu.as_ref().unwrap();
                let cw = state.char_width;
                let lh = state.line_height;
                let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
                let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
                let popup_w = (max_label + max_sc + 6).clamp(20, 50) as f32 * cw;
                let popup_h = cm.items.len() as f32 * lh;
                let menu_x = cm.screen_x as f32 * cw;
                let menu_y = cm.screen_y as f32 * lh;
                // Clamp to screen (same as draw_context_menu)
                let rt_w = {
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(hwnd, &mut rc);
                    }
                    (rc.right - rc.left) as f32 / state.dpi_scale
                };
                let rt_h = {
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(hwnd, &mut rc);
                    }
                    (rc.bottom - rc.top) as f32 / state.dpi_scale
                };
                let menu_x = menu_x.min(rt_w - popup_w).max(0.0);
                let menu_y = menu_y.min(rt_h - popup_h).max(0.0);

                if px >= menu_x && px < menu_x + popup_w && py >= menu_y && py < menu_y + popup_h {
                    // Click inside context menu — determine which item
                    let item_idx = ((py - menu_y) / lh).floor() as usize;
                    if item_idx < cm.items.len() && cm.items[item_idx].enabled {
                        true // Will handle below after releasing borrow
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if handled {
                let item_idx = {
                    let cm = state.engine.context_menu.as_ref().unwrap();
                    let lh = state.line_height;
                    let cw = state.char_width;
                    let max_label = cm.items.iter().map(|i| i.label.len()).max().unwrap_or(4);
                    let max_sc = cm.items.iter().map(|i| i.shortcut.len()).max().unwrap_or(0);
                    let popup_w = (max_label + max_sc + 6).clamp(20, 50) as f32 * cw;
                    let popup_h = cm.items.len() as f32 * lh;
                    let menu_x = cm.screen_x as f32 * cw;
                    let menu_y = cm.screen_y as f32 * lh;
                    let rt_h_val = {
                        let mut rc = RECT::default();
                        unsafe {
                            let _ = GetClientRect(hwnd, &mut rc);
                        }
                        (rc.bottom - rc.top) as f32 / state.dpi_scale
                    };
                    let rt_w_val = {
                        let mut rc = RECT::default();
                        unsafe {
                            let _ = GetClientRect(hwnd, &mut rc);
                        }
                        (rc.right - rc.left) as f32 / state.dpi_scale
                    };
                    let menu_y = menu_y.min(rt_h_val - popup_h).max(0.0);
                    ((py - menu_y) / lh).floor() as usize
                };
                state.engine.context_menu.as_mut().unwrap().selected = item_idx;
                let ctx = state.engine.context_menu_target_path();
                if let Some(action) = state.engine.context_menu_confirm() {
                    if let Some((ctx_path, ctx_is_dir)) = ctx {
                        handle_context_action(state, hwnd, &action, ctx_path, ctx_is_dir);
                    }
                }
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
            // Click outside — close menu
            state.engine.close_context_menu();
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Popup click handling (editor hover, panel hover, debug toolbar) ──
        // Editor hover popup — click gives focus, click outside dismisses
        if let Some(ref rect) = state.popup_rects.editor_hover {
            if rect.contains(px, py) {
                state.engine.editor_hover_focus();
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            } else {
                // Click outside hover → dismiss
                state.engine.dismiss_editor_hover();
            }
        }

        // Panel hover popup — click outside dismisses
        if let Some(ref rect) = state.popup_rects.panel_hover {
            if rect.contains(px, py) {
                // Click on panel hover — do nothing special (read-only)
                return;
            } else {
                state.engine.dismiss_panel_hover_now();
            }
        }

        // Debug toolbar buttons — dispatch via execute_command
        if state.popup_rects.debug_toolbar.is_some() {
            for (rect, action, enabled) in state.popup_rects.debug_toolbar_buttons.clone() {
                if rect.contains(px, py) && enabled {
                    let _ = state.engine.execute_command(&action);
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                }
            }
        }

        // ── Check tab bar hits first ─────────────────────────────────────
        for slot in &state.tab_slots {
            if px >= slot.x_start && px < slot.x_end && py >= slot.y && py < slot.y + slot.height {
                // Hit a tab — switch group and tab
                state.engine.active_group = slot.group_id;
                if px >= slot.close_x_start {
                    // Close button hit — check for unsaved changes first
                    state.engine.goto_tab(slot.tab_idx);
                    if state.engine.dirty() {
                        use crate::core::engine::DialogButton;
                        state.engine.show_dialog(
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
                                    label: "Discard".into(),
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
                    } else {
                        state.engine.close_tab();
                    }
                } else {
                    state.engine.goto_tab(slot.tab_idx);
                    // Record for potential tab drag (threshold checked on mouse move)
                    state.tab_drag_start = Some((px, py, slot.group_id, slot.tab_idx));
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // ── Diff toolbar button clicks ─────────────────────────────────
        for &(_, prev_x, next_x, fold_x, btn_w, bar_y, bar_h) in
            &state.cached_diff_toolbar_btns.clone()
        {
            if py >= bar_y && py < bar_y + bar_h {
                if px >= prev_x && px < prev_x + btn_w {
                    state.engine.jump_prev_hunk();
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                } else if px >= next_x && px < next_x + btn_w {
                    state.engine.jump_next_hunk();
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                } else if px >= fold_x && px < fold_x + btn_w {
                    state.engine.diff_toggle_hide_unchanged();
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                }
            }
        }

        // ── Breadcrumb clicks ────────────────────────────────────────────
        if state.engine.settings.breadcrumbs {
            let lh = state.line_height;
            let cw = state.char_width;
            for bc in state.cached_breadcrumbs.clone() {
                if bc.segments.is_empty() {
                    continue;
                }
                let bx = bc.bounds.x as f32;
                let by = bc.bounds.y as f32 - lh;
                let bw = bc.bounds.width as f32;
                if py >= by && py < by + lh && px >= bx && px < bx + bw {
                    let local_x = px - bx;
                    let sep_len = 3.0 * cw; // " › "
                    let mut x = cw; // left padding
                    state.engine.rebuild_breadcrumb_segments();
                    for (i, seg) in bc.segments.iter().enumerate() {
                        if i > 0 {
                            x += sep_len;
                        }
                        let label_w = seg.label.chars().count() as f32 * cw;
                        if local_x >= x && local_x < x + label_w {
                            state.engine.breadcrumb_selected = i;
                            state.engine.breadcrumb_open_scoped();
                            unsafe {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                            return;
                        }
                        x += label_w;
                    }
                    // Click on breadcrumb row but not on a segment — consume the click
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return;
                }
            }
        }

        // ── Group divider click → start drag ─────────────────────────────
        for div in &state.cached_dividers.clone() {
            let hit = match div.direction {
                SplitDirection::Vertical => {
                    (px - div.position as f32).abs() < 6.0
                        && py >= div.cross_start as f32
                        && py < (div.cross_start + div.cross_size) as f32
                }
                SplitDirection::Horizontal => {
                    (py - div.position as f32).abs() < 6.0
                        && px >= div.cross_start as f32
                        && px < (div.cross_start + div.cross_size) as f32
                }
            };
            if hit {
                state.group_divider_drag = Some(div.split_index);
                unsafe {
                    let _ = SetCapture(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // ── Terminal panel header click → start resize drag ─────────────
        if state.engine.terminal_open && !state.engine.terminal_panes.is_empty() {
            let rt_h = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                (rc.bottom - rc.top) as f32 / state.dpi_scale
            };
            let lh = state.line_height;
            let cw = state.char_width;
            let total_rows = state.engine.session.terminal_panel_rows as f32 + 1.0;
            let panel_y = rt_h - (total_rows + 2.0) * lh;
            let editor_left = state.sidebar.total_width();

            // Click on the toolbar row
            if py >= panel_y && px >= editor_left {
                // Clear sidebar focus when clicking on terminal area
                state.sidebar.has_focus = false;
                state.engine.clear_sidebar_focus();
            }
            if py >= panel_y && py < panel_y + lh && px >= editor_left {
                // Check toolbar buttons (right-aligned): × at width-2cw, split at width-4cw, + at width-6cw
                let btn_close_x = rt_h.max(0.0); // rt_h unused — use width
                let client_w = {
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(hwnd, &mut rc);
                    }
                    (rc.right - rc.left) as f32 / state.dpi_scale
                };
                if px >= client_w - cw * 2.0 {
                    // Close button — close active terminal tab
                    state.engine.terminal_close_active_tab();
                    state.engine.terminal_has_focus = true;
                } else if px >= client_w - cw * 4.0 {
                    // Split button — toggle terminal split
                    let full_cols = ((client_w - editor_left) / cw).floor() as u16;
                    let rows = state.engine.session.terminal_panel_rows;
                    state.engine.terminal_toggle_split(full_cols, rows);
                    state.engine.terminal_has_focus = true;
                } else if px >= client_w - cw * 6.0 {
                    // Add button — new terminal tab
                    let cols = ((client_w - editor_left) / cw).floor() as u16;
                    let rows = state.engine.session.terminal_panel_rows;
                    state.engine.terminal_new_tab(cols, rows);
                    state.engine.terminal_has_focus = true;
                } else {
                    // Check terminal tab labels (left-aligned)
                    let nf = crate::icons::nerd_fonts_enabled();
                    let mut tx = editor_left + cw;
                    let mut clicked_tab = None;
                    for i in 0..state.engine.terminal_panes.len() {
                        let label = if i == state.engine.terminal_active {
                            format!(" {} Terminal {} ", if nf { "\u{f120}" } else { "$" }, i + 1)
                        } else {
                            format!(" {} ", i + 1)
                        };
                        let tab_w = label.chars().count() as f32 * cw;
                        if px >= tx && px < tx + tab_w {
                            clicked_tab = Some(i);
                            break;
                        }
                        tx += tab_w;
                    }
                    if let Some(tab_idx) = clicked_tab {
                        state.engine.terminal_active = tab_idx;
                        state.engine.terminal_has_focus = true;
                    } else {
                        // Click on toolbar area (not a tab or button) — start resize drag
                        state.terminal_resize_drag = true;
                        state.engine.terminal_has_focus = true;
                    }
                }
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }

            // Click in terminal content area
            let content_y = panel_y + lh;
            if py >= content_y && px >= editor_left {
                // Handle split divider drag and pane switching
                if state.engine.terminal_split && state.engine.terminal_panes.len() >= 2 {
                    let split_cols = if state.engine.terminal_split_left_cols > 0 {
                        state.engine.terminal_split_left_cols
                    } else {
                        state.engine.terminal_panes[0].cols
                    };
                    let div_x = editor_left + split_cols as f32 * cw;
                    // Near divider (±5px) — start drag
                    if (px - div_x).abs() < 5.0 {
                        state.terminal_split_drag = true;
                        state.engine.terminal_has_focus = true;
                        unsafe {
                            let _ = SetCapture(hwnd);
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                        return;
                    }
                    // Switch focus to clicked pane
                    if px < div_x {
                        state.engine.terminal_active = 0;
                    } else {
                        state.engine.terminal_active = 1;
                    }
                }
                // Start terminal text selection
                let term_col = ((px - editor_left) / cw).floor() as u16;
                let term_row = ((py - content_y) / lh).floor() as u16;
                // Adjust for split pane — if clicking in the right pane, offset column
                let adj_col = if state.engine.terminal_split
                    && state.engine.terminal_panes.len() >= 2
                    && state.engine.terminal_active == 1
                {
                    let left_cols = if state.engine.terminal_split_left_cols > 0 {
                        state.engine.terminal_split_left_cols
                    } else {
                        state.engine.terminal_panes[0].cols
                    };
                    term_col.saturating_sub(left_cols + 1) // +1 for divider
                } else {
                    term_col
                };
                state.engine.terminal_scroll_reset();
                if let Some(term) = state.engine.active_terminal_mut() {
                    term.selection = Some(crate::core::terminal::TermSelection {
                        start_row: term_row,
                        start_col: adj_col,
                        end_row: term_row,
                        end_col: adj_col,
                    });
                }
                state.terminal_text_drag = true;
                state.engine.terminal_has_focus = true;
                unsafe {
                    let _ = SetCapture(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // ── Double-click detection ───────────────────────────────────────
        let now = Instant::now();
        let is_double = now.duration_since(state.last_click_time).as_millis()
            < DOUBLE_CLICK_MS as u128
            && state.last_click_pos == (ix, iy);
        state.last_click_time = now;
        state.last_click_pos = (ix, iy);

        // ── Per-window status bar click ─────────────────────────────────
        if state.engine.settings.window_status_line {
            let lh = state.line_height;
            let cw = state.char_width;
            let mut status_clicked = false;
            for cwr in &state.cached_window_rects {
                let rx = cwr.rect.x as f32;
                let ry = cwr.rect.y as f32;
                let rw_px = cwr.rect.width as f32;
                let rh_px = cwr.rect.height as f32;
                let status_y = ry + rh_px - lh;
                if px >= rx && px < rx + rw_px && py >= status_y && py < status_y + lh {
                    // Click is on this window's status bar
                    let is_active = cwr.window_id == state.engine.active_window_id();
                    let status = build_window_status_line(
                        &state.engine,
                        &state.theme,
                        cwr.window_id,
                        is_active,
                    );
                    let click_col = ((px - rx) / cw).floor() as usize;
                    let bar_width = (rw_px / cw).floor() as usize;
                    if let Some(action) = win_status_segment_hit_test(&status, bar_width, click_col)
                    {
                        if let Some(ea) = state.engine.handle_status_action(&action) {
                            use crate::core::engine::EngineAction;
                            match ea {
                                EngineAction::ToggleSidebar => {
                                    state.sidebar.visible = !state.sidebar.visible;
                                }
                                EngineAction::OpenTerminal => {
                                    let cols = (rw_px / cw).floor() as u16;
                                    state.engine.terminal_new_tab(
                                        cols,
                                        state.engine.session.terminal_panel_rows,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    status_clicked = true;
                    break;
                }
            }
            if status_clicked {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // ── Editor area click ────────────────────────────────────────────
        state.sidebar.has_focus = false;
        state.engine.terminal_has_focus = false;
        state.engine.clear_sidebar_focus();
        if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
            // Clear VSCode selection on click (matching GTK behavior)
            if state.engine.is_vscode_mode() {
                state.engine.vscode_clear_selection();
            }
            if is_double {
                state.engine.mouse_double_click(wid, line, col);
            } else {
                state.engine.mouse_click(wid, line, col);
            }
            state.mouse_text_drag = true;
        }

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_mouse_up(hwnd: HWND) {
    unsafe {
        let _ = ReleaseCapture();
    }

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");
        // Tab drag drop
        if state.tab_dragging {
            let zone = state.engine.tab_drop_zone;
            state.engine.tab_drag_drop(zone);
            state.tab_dragging = false;
            state.tab_drag_start = None;
        } else {
            state.tab_drag_start = None;
        }

        state.mouse_text_drag = false;
        state.sidebar_resize_drag = false;
        state.scrollbar_drag = None;
        state.terminal_split_drag = false;
        state.group_divider_drag = None;
        // Auto-copy terminal selection on mouse release
        if state.terminal_text_drag {
            state.terminal_text_drag = false;
            let text = state
                .engine
                .active_terminal()
                .and_then(|t| t.selected_text());
            if let Some(ref text) = text {
                if let Some(ref cb) = state.engine.clipboard_write {
                    let _ = cb(text);
                }
            }
        }
        if state.terminal_resize_drag {
            state.terminal_resize_drag = false;
            let rows = state.engine.session.terminal_panel_rows;
            let cw = state.char_width;
            let cols = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                ((rc.right - rc.left) as f32 / state.dpi_scale / cw).floor() as u16
            };
            state.engine.terminal_resize(cols, rows);
            let _ = state.engine.session.save();
        }
        state.engine.mouse_drag_active = false;
        state.engine.mouse_drag_origin_window = None;
        state.engine.mouse_drag_word_mode = false;

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_mouse_move(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let (px, py) = lparam_xy(lparam);
    let lbutton = wparam.0 & 0x0001 != 0;

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // Caption button hover tracking (inlined to avoid double RefCell borrow)
        let new_hover = {
            let title_h = TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT;
            if py >= title_h {
                None
            } else {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(state.hwnd, &mut rc);
                }
                let client_width = (rc.right - rc.left) as f32 / state.dpi_scale;
                let btn_total = CAPTION_BTN_COUNT * CAPTION_BTN_WIDTH;
                let btn_start = client_width - btn_total;
                if px < btn_start {
                    None
                } else {
                    Some(((px - btn_start) / CAPTION_BTN_WIDTH) as usize)
                }
            }
        };
        if new_hover != state.caption_hover {
            state.caption_hover = new_hover;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }

        // Terminal text selection drag in progress
        if state.terminal_text_drag && lbutton {
            let lh = state.line_height;
            let cw = state.char_width;
            let editor_left = state.sidebar.total_width();
            let rt_h = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(state.hwnd, &mut rc);
                }
                (rc.bottom - rc.top) as f32 / state.dpi_scale
            };
            let total_rows = state.engine.session.terminal_panel_rows as f32 + 1.0;
            let panel_y = rt_h - (total_rows + 2.0) * lh;
            let content_y = panel_y + lh;
            let term_col = ((px - editor_left) / cw).floor().max(0.0) as u16;
            let term_row = ((py - content_y) / lh).floor().max(0.0) as u16;
            let adj_col = if state.engine.terminal_split
                && state.engine.terminal_panes.len() >= 2
                && state.engine.terminal_active == 1
            {
                let left_cols = if state.engine.terminal_split_left_cols > 0 {
                    state.engine.terminal_split_left_cols
                } else {
                    state.engine.terminal_panes[0].cols
                };
                term_col.saturating_sub(left_cols + 1)
            } else {
                term_col
            };
            if let Some(term) = state.engine.active_terminal_mut() {
                if let Some(ref mut sel) = term.selection {
                    sel.end_row = term_row;
                    sel.end_col = adj_col;
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        } else if state.terminal_text_drag && !lbutton {
            state.terminal_text_drag = false;
            // Auto-copy selection to clipboard on mouse release
            let text = state
                .engine
                .active_terminal()
                .and_then(|t| t.selected_text());
            if let Some(ref text) = text {
                if let Some(ref cb) = state.engine.clipboard_write {
                    let _ = cb(text);
                }
            }
        }

        // Terminal split divider drag in progress
        if state.terminal_split_drag && lbutton {
            let editor_left = state.sidebar.total_width();
            let cw = state.char_width;
            let left_cols = ((px - editor_left) / cw).floor() as u16;
            let left_cols = left_cols.clamp(5, 200);
            state.engine.terminal_split_set_drag_cols(left_cols);
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        } else if state.terminal_split_drag && !lbutton {
            state.terminal_split_drag = false;
            let editor_left = state.sidebar.total_width();
            let cw = state.char_width;
            let rt_w = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                (rc.right - rc.left) as f32 / state.dpi_scale
            };
            let full_cols = ((rt_w - editor_left) / cw).floor() as u16;
            let left_cols = state.engine.terminal_split_left_cols;
            let right_cols = full_cols.saturating_sub(left_cols + 1); // +1 for divider
            let rows = state.engine.session.terminal_panel_rows;
            state
                .engine
                .terminal_split_finalize_drag(left_cols, right_cols, rows);
        }

        // Terminal panel resize drag in progress
        if state.terminal_resize_drag && lbutton {
            let rt_h = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                (rc.bottom - rc.top) as f32 / state.dpi_scale
            };
            let lh = state.line_height;
            // Available rows between drag position and bottom chrome (status + cmd = 2 rows)
            let available = ((rt_h - py) / lh).floor() as u16;
            let new_rows = available.saturating_sub(3).clamp(5, 50); // -3 for toolbar + status + cmd
            state.engine.session.terminal_panel_rows = new_rows;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        } else if state.terminal_resize_drag && !lbutton {
            state.terminal_resize_drag = false;
            let rows = state.engine.session.terminal_panel_rows;
            let cw = state.char_width;
            let cols = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                ((rc.right - rc.left) as f32 / state.dpi_scale / cw).floor() as u16
            };
            state.engine.terminal_resize(cols, rows);
            let _ = state.engine.session.save();
        }

        // Sidebar resize drag in progress
        if state.sidebar_resize_drag && lbutton {
            let ab_w = state.sidebar.activity_bar_px;
            let new_w = (px - ab_w).clamp(80.0, 600.0);
            state.sidebar.panel_width = new_w;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // Group divider drag in progress
        if let Some(split_index) = state.group_divider_drag {
            if lbutton {
                if let Some(div) = state
                    .cached_dividers
                    .iter()
                    .find(|d| d.split_index == split_index)
                {
                    let mouse_pos = match div.direction {
                        SplitDirection::Vertical => px as f64,
                        SplitDirection::Horizontal => py as f64,
                    };
                    let new_ratio = ((mouse_pos - div.axis_start) / div.axis_size).clamp(0.1, 0.9);
                    state
                        .engine
                        .group_layout
                        .set_ratio_at_index(split_index, new_ratio);
                }
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            } else {
                state.group_divider_drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
        }

        // Scrollbar drag in progress
        if let Some(wid) = state.scrollbar_drag {
            if lbutton {
                // Recompute scroll position from current Y using the cached rect
                if let Some(cwr) = state
                    .cached_window_rects
                    .iter()
                    .find(|c| c.window_id == wid)
                {
                    let ry = cwr.rect.y as f32;
                    let rh = cwr.rect.height as f32;
                    let lh = state.line_height;
                    let has_status = state.engine.settings.window_status_line;
                    let editor_h = rh - if has_status { lh } else { 0.0 };
                    let w = state.engine.windows.get(&wid);
                    let total_lines = w.map_or(1, |w| {
                        let bid = w.buffer_id;
                        state
                            .engine
                            .buffer_manager
                            .get(bid)
                            .map_or(1, |bs| bs.buffer.len_lines())
                    });
                    let viewport_lines = (editor_h / lh).floor() as usize;
                    let max_scroll = total_lines.saturating_sub(viewport_lines);
                    let rel_y = (py - ry).clamp(0.0, editor_h);
                    let ratio = rel_y / editor_h;
                    let new_top = ((ratio * max_scroll as f32) as usize).min(max_scroll);
                    state.engine.set_scroll_top_for_window(wid, new_top);
                    state.engine.sync_scroll_binds();
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                return;
            } else {
                state.scrollbar_drag = None;
            }
        }

        // Tab drag: threshold check and drag tracking
        if lbutton {
            if let Some((sx, sy, gid, tidx)) = state.tab_drag_start {
                let dx = (px - sx).abs();
                let dy = (py - sy).abs();
                // Start drag once we exceed a small threshold (5 DIPs)
                if dx + dy >= 5.0 && !state.tab_dragging {
                    state.tab_dragging = true;
                    state.tab_drag_start = None;
                    state.engine.tab_drag_begin(gid, tidx);
                }
            }
            if state.tab_dragging {
                state.engine.tab_drag_mouse = Some((px as f64, py as f64));
                // Compute drop zone based on cursor position
                let zone = compute_win_tab_drop_zone(state, px, py);
                state.engine.tab_drop_zone = zone;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // Menu hover: switch menus and highlight dropdown items
        if let Some(midx) = state.engine.menu_open_idx {
            if state.engine.menu_bar_visible {
                let cw = state.char_width;
                let lh = state.line_height;

                // Hover over menu bar labels — switch to that menu
                let title_h = TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT;
                if py < title_h {
                    let pad = 8.0_f32;
                    let mut label_x = pad;
                    for (idx, (name, _, _)) in MENU_STRUCTURE.iter().enumerate() {
                        let label_w = measure_ui_text_width(
                            &state.dwrite_factory,
                            &state.ui_text_format,
                            name,
                        ) + pad * 2.0;
                        if px >= label_x && px < label_x + label_w && idx != midx {
                            state.engine.open_menu(idx);
                            unsafe {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                            break;
                        }
                        label_x += label_w;
                    }
                } else {
                    // Hover over dropdown items — highlight
                    let pad = 8.0_f32;
                    let mut popup_x = pad;
                    for i in 0..midx {
                        if let Some((name, _, _)) = MENU_STRUCTURE.get(i) {
                            popup_x += measure_ui_text_width(
                                &state.dwrite_factory,
                                &state.ui_text_format,
                                name,
                            ) + pad * 2.0;
                        }
                    }
                    let items = MENU_STRUCTURE
                        .get(midx)
                        .map(|(_, _, items)| *items)
                        .unwrap_or(&[]);
                    let max_label = items.iter().map(|i| i.label.len()).max().unwrap_or(4);
                    let max_sc = items
                        .iter()
                        .map(|i| i.shortcut.len().max(i.vscode_shortcut.len()))
                        .max()
                        .unwrap_or(0);
                    let popup_w = (max_label + max_sc + 6).clamp(20, 50) as f32 * cw;
                    let popup_y = TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT;

                    if px >= popup_x && px < popup_x + popup_w && py >= popup_y {
                        let rel_y = py - popup_y - lh * 0.25;
                        let item_idx = if rel_y < 0.0 {
                            None
                        } else {
                            let idx = (rel_y / lh).floor() as usize;
                            if idx < items.len() && !items[idx].separator {
                                Some(idx)
                            } else {
                                None
                            }
                        };
                        if state.engine.menu_highlighted_item != item_idx {
                            state.engine.menu_highlighted_item = item_idx;
                            unsafe {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        }
                    } else if state.engine.menu_highlighted_item.is_some() {
                        state.engine.menu_highlighted_item = None;
                        unsafe {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
            }
        }

        // Context menu hover tracking — highlight items on mouse move
        if let Some(ref cm) = state.engine.context_menu {
            let cw = state.char_width;
            let lh = state.line_height;
            let max_label = cm
                .items
                .iter()
                .map(|i| i.label.chars().count() + i.shortcut.chars().count() + 4)
                .max()
                .unwrap_or(20);
            let popup_w = max_label as f32 * cw;
            let popup_h = cm.items.len() as f32 * lh;
            let menu_x = (cm.screen_x as f32 * cw)
                .min({
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(hwnd, &mut rc);
                    }
                    (rc.right - rc.left) as f32 / state.dpi_scale - popup_w
                })
                .max(0.0);
            let menu_y = (cm.screen_y as f32 * lh)
                .min({
                    let mut rc = RECT::default();
                    unsafe {
                        let _ = GetClientRect(hwnd, &mut rc);
                    }
                    (rc.bottom - rc.top) as f32 / state.dpi_scale - popup_h
                })
                .max(0.0);

            if px >= menu_x && px < menu_x + popup_w && py >= menu_y && py < menu_y + popup_h {
                let item_idx = ((py - menu_y) / lh).floor() as usize;
                if item_idx < cm.items.len()
                    && cm.items[item_idx].enabled
                    && cm.selected != item_idx
                {
                    state.engine.context_menu.as_mut().unwrap().selected = item_idx;
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
        }

        // Popup hover tracking: dismiss/cancel-dismiss for editor hover and panel hover
        if let Some(ref rect) = state.popup_rects.editor_hover {
            if rect.contains(px, py) {
                state.engine.cancel_editor_hover_dismiss();
            } else {
                state.engine.dismiss_editor_hover_delayed();
            }
        }
        if let Some(ref rect) = state.popup_rects.panel_hover {
            if rect.contains(px, py) {
                state.engine.cancel_panel_hover_dismiss();
            } else {
                // Only dismiss if also not over the sidebar
                if px >= state.sidebar.total_width() {
                    state.engine.dismiss_panel_hover();
                }
            }
        }

        // Tab tooltip: show on hover, dismiss on mouseout
        {
            let found_slot = state.tab_slots.iter().find(|slot| {
                px >= slot.x_start && px < slot.x_end && py >= slot.y && py < slot.y + slot.height
            });
            let tooltip = found_slot.and_then(|slot| {
                let group = state.engine.editor_groups.get(&slot.group_id)?;
                let tab = group.tabs.get(slot.tab_idx)?;
                let win = state.engine.windows.get(&tab.active_window)?;
                let bs = state.engine.buffer_manager.get(win.buffer_id)?;
                let raw_path = bs.file_path.as_ref()?;
                let path = crate::core::paths::strip_unc_prefix(raw_path);
                Some(path.display().to_string())
            });
            if let Some(slot) = found_slot {
                state.tab_tooltip_x = slot.x_start;
            }
            if tooltip != state.engine.tab_hover_tooltip {
                state.engine.tab_hover_tooltip = tooltip;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }

        // Text drag (inline instead of calling on_mouse_drag to avoid double borrow)
        if !state.sidebar_resize_drag && lbutton && state.mouse_text_drag {
            if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
                state.engine.mouse_drag(wid, line, col);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            return;
        }

        // Cursor shape: resize arrow near sidebar edge or group divider
        let edge = state.sidebar.total_width();
        let near_sidebar = state.sidebar.visible && (px - edge).abs() < SIDEBAR_RESIZE_HIT_PX;
        let hit_divider = state
            .cached_dividers
            .iter()
            .find(|div| match div.direction {
                SplitDirection::Vertical => {
                    (px - div.position as f32).abs() < 6.0
                        && py >= div.cross_start as f32
                        && py < (div.cross_start + div.cross_size) as f32
                }
                SplitDirection::Horizontal => {
                    (py - div.position as f32).abs() < 6.0
                        && px >= div.cross_start as f32
                        && px < (div.cross_start + div.cross_size) as f32
                }
            });
        // Determine if cursor is over an editor text area (I-beam) or UI chrome (arrow)
        let over_editor = {
            let editor_left = state.sidebar.total_width();
            let menu_h = if state.engine.menu_bar_visible {
                TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT
            } else {
                0.0
            };
            let tab_h = state.line_height * TAB_BAR_HEIGHT_MULT;
            let bc_h = if state.engine.settings.breadcrumbs {
                state.line_height
            } else {
                0.0
            };
            let chrome_top = menu_h + tab_h + bc_h;
            px >= editor_left
                && py >= chrome_top
                && state.engine.context_menu.is_none()
                && !state
                    .tab_slots
                    .iter()
                    .any(|s| px >= s.x_start && px < s.x_end && py >= s.y && py < s.y + s.height)
        };
        unsafe {
            let cursor_id = if near_sidebar {
                IDC_SIZEWE
            } else if let Some(div) = hit_divider {
                match div.direction {
                    SplitDirection::Vertical => IDC_SIZEWE,
                    SplitDirection::Horizontal => IDC_SIZENS,
                }
            } else if over_editor {
                IDC_IBEAM
            } else {
                IDC_ARROW
            };
            let cursor = LoadCursorW(None, cursor_id).unwrap_or_default();
            SetCursor(Some(cursor));
        }
    });
}

fn on_mouse_drag(hwnd: HWND, lparam: LPARAM) {
    let (px, py) = lparam_xy(lparam);

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        if !state.mouse_text_drag {
            return;
        }

        if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
            state.engine.mouse_drag(wid, line, col);
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    });
}

fn on_mouse_dblclick(hwnd: HWND, lparam: LPARAM) {
    let (px, py) = lparam_xy(lparam);

    unsafe {
        SetCapture(hwnd);
    }

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // Double-click on explorer file → open as Permanent (promotes preview tab)
        let ab_w = state.sidebar.activity_bar_px;
        let menu_y = if state.engine.menu_bar_visible {
            TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT
        } else {
            0.0
        };
        if state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Explorer
            && px >= ab_w
            && px < state.sidebar.total_width()
            && py >= menu_y
        {
            let row = ((py - menu_y) / state.line_height).floor() as usize;
            if row >= 1 {
                let vis_idx = state.sidebar.scroll_top + (row - 1);
                if vis_idx < state.sidebar.rows.len() && !state.sidebar.rows[vis_idx].is_dir {
                    let path = state.sidebar.rows[vis_idx].path.clone();
                    state.engine.open_file_in_tab(&path);
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // Double-click on extension panel → open extension README
        if state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Extensions
            && px >= ab_w
            && px < state.sidebar.total_width()
            && py >= menu_y
        {
            let lh = state.line_height;
            let click_y = py - menu_y;
            let installed_len = state.engine.ext_installed_items().len();
            let inst_expanded = state.engine.ext_sidebar_sections_expanded[0];

            let inst_header_y = lh * 1.5;
            let inst_items_start_y = inst_header_y + lh;
            let inst_items_end_y = inst_items_start_y
                + if inst_expanded {
                    installed_len as f32 * lh
                } else {
                    0.0
                };
            let avail_header_y = inst_items_end_y + lh * 0.3;
            let avail_items_start_y = avail_header_y + lh;

            // Only open readme on item rows, not section headers
            if inst_expanded && click_y >= inst_items_start_y && click_y < inst_items_end_y {
                let idx = ((click_y - inst_items_start_y) / lh).floor() as usize;
                if idx < installed_len {
                    state.engine.ext_sidebar_selected = idx;
                    state.engine.ext_open_selected_readme();
                }
            } else if click_y >= avail_items_start_y {
                let avail_idx = ((click_y - avail_items_start_y) / lh).floor() as usize;
                let avail_len = state.engine.ext_available_items().len();
                if avail_idx < avail_len {
                    state.engine.ext_sidebar_selected = installed_len + avail_idx;
                    state.engine.ext_open_selected_readme();
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // Double-click on editor → word-select
        if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
            state.engine.mouse_double_click(wid, line, col);
            state.mouse_text_drag = true;
        }

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_right_click(hwnd: HWND, lparam: LPARAM) {
    let (px, py) = lparam_xy(lparam);

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let lh = state.line_height;
        let cw = state.char_width;
        let screen_col = (px / cw).floor() as u16;
        let screen_row = (py / lh).floor() as u16;

        // Check tab bar right-click
        for slot in &state.tab_slots {
            if px >= slot.x_start && px < slot.x_end && py >= slot.y && py < slot.y + slot.height {
                state.engine.active_group = slot.group_id;
                state.engine.goto_tab(slot.tab_idx);
                state.engine.open_tab_context_menu(
                    slot.group_id,
                    slot.tab_idx,
                    screen_col,
                    screen_row,
                );
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // Check explorer sidebar right-click
        let ab_w = state.sidebar.activity_bar_px;
        if state.sidebar.visible
            && state.sidebar.active_panel == SidebarPanel::Explorer
            && px >= ab_w
            && px < state.sidebar.total_width()
        {
            let menu_y = if state.engine.menu_bar_visible {
                TITLE_BAR_TOP_INSET + state.line_height * TITLE_BAR_HEIGHT_MULT
            } else {
                0.0
            };
            let row = ((py - menu_y) / state.line_height).floor() as usize;
            // Row 0 is header, tree starts at row 1
            if row >= 1 {
                let vis_idx = state.sidebar.scroll_top + (row - 1);
                if vis_idx < state.sidebar.rows.len() {
                    let path = state.sidebar.rows[vis_idx].path.clone();
                    let is_dir = state.sidebar.rows[vis_idx].is_dir;
                    state
                        .engine
                        .open_explorer_context_menu(path, is_dir, screen_col, screen_row);
                } else if let Some(ref root) = state.engine.workspace_root.clone() {
                    state.engine.open_explorer_context_menu(
                        root.clone(),
                        true,
                        screen_col,
                        screen_row,
                    );
                }
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // Editor area right-click
        if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
            // Position cursor at click location first
            state.engine.mouse_click(wid, line, col);
            state
                .engine
                .open_editor_context_menu(screen_col, screen_row);
        }

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_mouse_wheel(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
    let lines = -(delta as i32) / 120 * 3; // 3 lines per notch

    // WM_MOUSEWHEEL gives screen coords — convert to client coords
    let screen_x = (lparam.0 & 0xFFFF) as i16;
    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16;
    let mut pt = POINT {
        x: screen_x as i32,
        y: screen_y as i32,
    };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    let scale = APP.with(|app| app.borrow().as_ref().map_or(1.0, |state| state.dpi_scale));
    let px = pt.x as f32 / scale;
    let py = pt.y as f32 / scale;

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // Picker scroll — intercept scroll when picker is open
        if state.engine.picker_open {
            let max = state.engine.picker_items.len().saturating_sub(1);
            if lines > 0 {
                state.engine.picker_selected =
                    (state.engine.picker_selected + lines as usize).min(max);
            } else {
                state.engine.picker_selected = state
                    .engine
                    .picker_selected
                    .saturating_sub((-lines) as usize);
            }
            state.engine.picker_load_preview();
            // Update scroll to keep selected item visible (12 visible items)
            let visible = 12;
            if state.engine.picker_selected < state.engine.picker_scroll_top {
                state.engine.picker_scroll_top = state.engine.picker_selected;
            } else if state.engine.picker_selected >= state.engine.picker_scroll_top + visible {
                state.engine.picker_scroll_top = state.engine.picker_selected + 1 - visible;
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // Editor hover popup scroll
        if let Some(ref rect) = state.popup_rects.editor_hover {
            if rect.contains(px, py) {
                state.engine.editor_hover_scroll(lines);
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // Sidebar scroll
        if state.sidebar.visible && px < state.sidebar.total_width() {
            let max = state.sidebar.rows.len().saturating_sub(1);
            if lines > 0 {
                state.sidebar.scroll_top = state
                    .sidebar
                    .scroll_top
                    .saturating_add(lines as usize)
                    .min(max);
            } else {
                state.sidebar.scroll_top =
                    state.sidebar.scroll_top.saturating_sub((-lines) as usize);
            }
        } else {
            // Editor scroll — use fold-aware scrolling
            let max_line = state.engine.buffer().len_lines().saturating_sub(1);
            if lines > 0 {
                state.engine.scroll_down_visible(lines as usize);
            } else {
                state.engine.scroll_up_visible((-lines) as usize);
            }
            // Keep cursor within the viewport (matching GTK behavior)
            let scrolloff = state.engine.settings.scrolloff;
            let vp = state.engine.view().viewport_lines.max(1);
            let cur = state.engine.view().cursor.line;
            let new_top = state.engine.view().scroll_top;
            if cur < new_top + scrolloff {
                state.engine.view_mut().cursor.line = (new_top + scrolloff).min(max_line);
                state.engine.clamp_cursor_col();
            } else if cur >= new_top + vp.saturating_sub(scrolloff) {
                state.engine.view_mut().cursor.line =
                    (new_top + vp.saturating_sub(scrolloff + 1)).min(max_line);
                state.engine.clamp_cursor_col();
            }
            state.engine.sync_scroll_binds();
        }

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_tick(hwnd: HWND) {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let mut needs_redraw = false;

        // Poll LSP
        if state.engine.poll_lsp() {
            needs_redraw = true;
        }

        // Poll DAP
        if state.engine.poll_dap() {
            needs_redraw = true;
        }

        // Poll terminals
        if state.engine.poll_terminal() {
            needs_redraw = true;
        }

        // Syntax debounce
        if state.engine.tick_syntax_debounce() {
            needs_redraw = true;
        }

        // Swap file periodic writes
        state.engine.tick_swap_files();

        // Periodic sidebar refresh (git status, explorer indicators) — every 2s
        if state.last_sidebar_refresh.elapsed().as_secs() >= 2 {
            state.engine.sc_refresh();
            state.sidebar.dirty = true;
            state.last_sidebar_refresh = Instant::now();
            needs_redraw = true;
        }

        // File watcher (external modification detection)
        state.engine.tick_file_watcher();

        // Notification ticker
        state.engine.tick_notifications();
        needs_redraw = true;

        // Hot-reload theme
        if state.engine.settings.colorscheme != state.current_colorscheme {
            state.theme = Theme::from_name(&state.engine.settings.colorscheme);
            state.current_colorscheme = state.engine.settings.colorscheme.clone();
            needs_redraw = true;
        }

        // Hot-reload font size (includes DPI scaling)
        if state.engine.settings.font_size != state.current_font_size {
            let new_size = state.engine.settings.font_size as f32 * state.dpi_scale;
            if let Ok(fmt) = unsafe {
                state.dwrite_factory.CreateTextFormat(
                    w!("Consolas"),
                    None,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    new_size,
                    w!("en-us"),
                )
            } {
                state.text_format = fmt;
                state.char_width =
                    draw::measure_char_width(&state.dwrite_factory, &state.text_format);
                state.line_height =
                    draw::measure_line_height(&state.dwrite_factory, &state.text_format);
                state.current_font_size = state.engine.settings.font_size;
                needs_redraw = true;
            }
        }

        // Update window title with current file name
        update_window_title(state);

        if needs_redraw {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    });
}

/// Update the Win32 window title to reflect the current file and dirty state.
fn update_window_title(state: &AppState) {
    let bid = state.engine.active_buffer_id();
    let (name, dirty) = state
        .engine
        .buffer_manager
        .get(bid)
        .map(|s| {
            let name = s
                .file_path
                .as_deref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "[No Name]".to_string());
            (name, s.dirty)
        })
        .unwrap_or_else(|| ("[No Name]".to_string(), false));

    let title = if dirty {
        format!("{} (modified) — VimCode", name)
    } else {
        format!("{} — VimCode", name)
    };
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = SetWindowTextW(state.hwnd, PCWSTR(wide.as_ptr()));
    }
}

fn on_destroy() {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        if let Some(state) = app.as_mut() {
            state.engine.cleanup_all_swaps();
            state.engine.lsp_shutdown();
            save_session(&mut state.engine);
        }
    });
}

// ─── Action handling ─────────────────────────────────────────────────────────

// ─── Menu item activation (intercepts dialog actions) ───────────────────────

/// Activate a menu item by action string. Dialog actions (open file/folder, save workspace)
/// are intercepted here and handled via native Win32 dialogs instead of going through
/// `execute_command`. All other actions fall through to the engine.
fn activate_menu_item(state: &mut AppState, menu_idx: usize, item_idx: usize, action: &str) {
    match action {
        "open_file_dialog" => {
            state.engine.close_menu();
            if let Some(path) = show_open_file_dialog(state.hwnd) {
                state.engine.open_file_in_tab(&path);
                state.sidebar.dirty = true;
            }
        }
        "open_folder_dialog" => {
            state.engine.close_menu();
            if let Some(path) = show_open_folder_dialog(state.hwnd) {
                state.engine.open_folder(&path);
                state.sidebar.dirty = true;
                state.sidebar.expanded.clear();
                state.sidebar.expanded.insert(path);
            }
        }
        "save_workspace_as_dialog" => {
            state.engine.close_menu();
            if let Some(path) = show_save_dialog(state.hwnd, ".vimcode-workspace") {
                state.engine.save_workspace_as(&path);
            }
        }
        _ => {
            let ea = state.engine.menu_activate_item(menu_idx, item_idx, action);
            handle_action_with_sidebar(state, ea);
        }
    }
}

// ─── Native file dialogs (IFileOpenDialog / IFileSaveDialog) ────────────────

/// Show a native "Open File" dialog. Returns the selected path, or `None` if cancelled.
fn show_open_file_dialog(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("All Files"),
            pszSpec: w!("*.*"),
        }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.SetTitle(w!("Open File")).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let result = dialog.GetResult().ok()?;
        let path_w = result.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(path_w.to_string().ok()?);
        CoTaskMemFree(Some(path_w.0 as *const _));
        Some(path)
    }
}

/// Show a native "Open Folder" dialog. Returns the selected path, or `None` if cancelled.
fn show_open_folder_dialog(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog
            .SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS)
            .ok()?;
        dialog.SetTitle(w!("Open Folder")).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let result = dialog.GetResult().ok()?;
        let path_w = result.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(path_w.to_string().ok()?);
        CoTaskMemFree(Some(path_w.0 as *const _));
        Some(path)
    }
}

/// Show a native "Save As" dialog with a suggested filename. Returns the selected path.
fn show_save_dialog(hwnd: HWND, suggested_name: &str) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let name_wide: Vec<u16> = suggested_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        dialog.SetFileName(PCWSTR(name_wide.as_ptr())).ok()?;
        dialog.SetTitle(w!("Save Workspace As")).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let result = dialog.GetResult().ok()?;
        let path_w = result.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(path_w.to_string().ok()?);
        CoTaskMemFree(Some(path_w.0 as *const _));
        Some(path)
    }
}

/// Process an engine action. Returns `true` if the app should quit.
/// `state` is optional — some callers only have the engine.
fn handle_action_with_sidebar(state: &mut AppState, action: EngineAction) -> bool {
    match action {
        EngineAction::ToggleSidebar => {
            state.sidebar.visible = !state.sidebar.visible;
            state.sidebar.dirty = true;
            if state.sidebar.visible && state.sidebar.expanded.is_empty() {
                if let Some(ref root) = state.engine.workspace_root.clone() {
                    state.sidebar.expanded.insert(root.clone());
                }
            }
        }
        EngineAction::OpenFolderDialog => {
            if let Some(path) = show_open_folder_dialog(state.hwnd) {
                state.engine.open_folder(&path);
                state.sidebar.dirty = true;
                state.sidebar.expanded.clear();
                state.sidebar.expanded.insert(path);
            }
            return false;
        }
        EngineAction::SaveWorkspaceAsDialog => {
            if let Some(path) = show_save_dialog(state.hwnd, ".vimcode-workspace") {
                state.engine.save_workspace_as(&path);
            }
            return false;
        }
        EngineAction::OpenRecentDialog => {
            // Open Recent is handled by the picker — engine already populates it
        }
        _ => {}
    }
    handle_action(&mut state.engine, action)
}

fn handle_action(engine: &mut Engine, action: EngineAction) -> bool {
    match action {
        EngineAction::Quit | EngineAction::SaveQuit => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            true
        }
        EngineAction::OpenFile(path) => {
            engine.open_file_in_tab(&path);
            false
        }
        EngineAction::QuitWithError => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            std::process::exit(1);
        }
        EngineAction::OpenUrl(url) => {
            open_url_in_browser(&url);
            false
        }
        EngineAction::OpenTerminal => {
            let rows = engine.session.terminal_panel_rows;
            let cols = 80; // will be updated by layout calculation
            engine.terminal_new_tab(cols, rows);
            false
        }
        EngineAction::RunInTerminal(cmd) => {
            let rows = engine.session.terminal_panel_rows;
            let cols = 80;
            engine.terminal_run_command(&cmd, cols, rows);
            false
        }
        EngineAction::OpenFolderDialog
        | EngineAction::OpenWorkspaceDialog
        | EngineAction::SaveWorkspaceAsDialog
        | EngineAction::OpenRecentDialog => false,
        EngineAction::QuitWithUnsaved => {
            use crate::core::engine::DialogButton;
            engine.show_dialog(
                "quit_unsaved",
                "Unsaved Changes",
                vec!["You have unsaved changes. Do you want to save before quitting?".to_string()],
                vec![
                    DialogButton {
                        label: "Save All & Quit".into(),
                        hotkey: 's',
                        action: "save_quit".into(),
                    },
                    DialogButton {
                        label: "Quit Without Saving".into(),
                        hotkey: 'd',
                        action: "discard_quit".into(),
                    },
                    DialogButton {
                        label: "Cancel".into(),
                        hotkey: '\0',
                        action: "cancel".into(),
                    },
                ],
            );
            false
        }
        EngineAction::ToggleSidebar => {
            // Handled at the caller side (needs AppState access)
            false
        }
        EngineAction::None | EngineAction::Error => false,
    }
}

fn save_session(engine: &mut Engine) {
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
}

// ─── Cached popup rectangles ────────────────────────────────────────────────

/// Bounding rectangle in DIPs for a popup/toolbar drawn in the last frame.
#[derive(Clone, Debug, Default)]
struct PopupRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl PopupRect {
    fn contains(&self, px: f32, py: f32) -> bool {
        self.w > 0.0 && px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Cached bounding rectangles for popups and toolbars, populated each paint.
#[derive(Clone, Debug, Default)]
struct CachedPopupRects {
    editor_hover: Option<PopupRect>,
    diff_peek: Option<PopupRect>,
    panel_hover: Option<PopupRect>,
    tab_tooltip: Option<PopupRect>,
    debug_toolbar: Option<PopupRect>,
    /// Per-button rects for debug toolbar actions (rect, action command, enabled).
    debug_toolbar_buttons: Vec<(PopupRect, String, bool)>,
}

// ─── Tab drag-and-drop drop zone computation ────────────────────────────────

/// Compute the drop zone for a tab drag at pixel position `(px, py)`.
/// Returns the appropriate `DropZone` based on which group/tab/edge the cursor is over.
/// Handle a context menu action after the user clicks an item.
fn handle_context_action(
    state: &mut AppState,
    hwnd: HWND,
    action: &str,
    ctx_path: PathBuf,
    ctx_is_dir: bool,
) {
    match action {
        "new_file" | "new_folder" => {
            let target = if ctx_is_dir {
                ctx_path.clone()
            } else {
                ctx_path.parent().unwrap_or(Path::new(".")).to_path_buf()
            };
            if action == "new_file" {
                state.engine.start_explorer_new_file(target);
            } else {
                state.engine.start_explorer_new_folder(target);
            }
        }
        "rename" => {
            state.engine.start_explorer_rename(ctx_path);
        }
        "delete" => {
            state.engine.confirm_delete_file(&ctx_path);
        }
        "open_terminal" => {
            let dir = if ctx_is_dir {
                ctx_path.clone()
            } else {
                ctx_path.parent().unwrap_or(Path::new(".")).to_path_buf()
            };
            let cw = state.char_width;
            let cols = {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rc);
                }
                ((rc.right - rc.left) as f32 / state.dpi_scale / cw).floor() as u16
            };
            let rows = state.engine.session.terminal_panel_rows;
            state.engine.terminal_new_tab_at(cols, rows, Some(&dir));
        }
        "find_in_folder" => {
            state
                .engine
                .open_picker(crate::core::engine::PickerSource::Grep);
        }
        // copy_path, copy_relative_path, reveal, open_side, select_for_diff, diff_with_selected
        // are handled by the engine's context_menu_confirm() directly
        _ => {}
    }
    state.sidebar.dirty = true;
}

fn compute_win_tab_drop_zone(state: &AppState, px: f32, py: f32) -> DropZone {
    let source = match &state.engine.tab_drag {
        Some(td) => (td.source_group, td.source_tab_index),
        None => return DropZone::None,
    };

    let lh = state.line_height;
    let tab_h = lh * TAB_BAR_HEIGHT_MULT;

    // Check if cursor is over a tab bar → TabReorder
    for slot in &state.tab_slots {
        if py >= slot.y && py < slot.y + slot.height {
            if px >= slot.x_start && px < slot.x_end {
                // Over this tab — insert before or after based on midpoint
                let mid = (slot.x_start + slot.x_end) / 2.0;
                let idx = if px < mid {
                    slot.tab_idx
                } else {
                    slot.tab_idx + 1
                };
                return DropZone::TabReorder(slot.group_id, idx);
            }
            // Over this group's tab bar but past the last tab — append
            // Use the number of tabs in this group (if available) as the insertion index.
            let tab_count = state
                .engine
                .editor_groups
                .get(&slot.group_id)
                .map_or(0, |g| g.tabs.len());
            return DropZone::TabReorder(slot.group_id, tab_count);
        }
    }

    // Check if cursor is over an editor area → Center or Split
    let multi_group = state.engine.editor_groups.len() > 1;

    for cwr in &state.cached_window_rects {
        let rx = cwr.rect.x as f32;
        let ry = cwr.rect.y as f32;
        let rw = cwr.rect.width as f32;
        let rh = cwr.rect.height as f32;

        if px >= rx && px < rx + rw && py >= ry && py < ry + rh {
            let gid = cwr.group_id;

            // Edge zone = 20% of dimension, min 30px
            let edge_x = (rw * 0.2).max(30.0);
            let edge_y = (rh * 0.2).max(30.0);

            let rel_x = px - rx;
            let rel_y = py - ry;

            if rel_x < edge_x {
                return DropZone::Split(gid, SplitDirection::Vertical, true);
            }
            if rel_x > rw - edge_x {
                return DropZone::Split(gid, SplitDirection::Vertical, false);
            }
            if rel_y < edge_y {
                return DropZone::Split(gid, SplitDirection::Horizontal, true);
            }
            if rel_y > rh - edge_y {
                return DropZone::Split(gid, SplitDirection::Horizontal, false);
            }

            // Center — merge into group (only if multi-group or different group)
            if multi_group || gid != source.0 {
                return DropZone::Center(gid);
            }
        }
    }

    DropZone::None
}

// ─── Clipboard ───────────────────────────────────────────────────────────────

/// Sync the unnamed register to the system clipboard after yank/delete operations.
fn sync_clipboard(state: &mut AppState) {
    let current = state
        .engine
        .registers
        .get(&'"')
        .filter(|(s, _)| !s.is_empty())
        .map(|(s, _)| s.clone());
    if current != state.last_clipboard_register {
        if let (Some(ref text), Some(ref cb_write)) = (&current, &state.engine.clipboard_write) {
            let _ = cb_write(text.as_str());
        }
        state.last_clipboard_register = current;
    }
}

/// Load system clipboard into registers before a paste key (p/P) so that
/// externally copied text can be pasted in the editor (clipboard=unnamedplus).
fn intercept_paste_key(state: &mut AppState, key_name: &str, unicode: Option<char>) -> bool {
    use crate::core::Mode;
    // Only intercept p/P in Normal/Visual modes with default register
    let is_paste = matches!(
        (&state.engine.mode, unicode),
        (
            Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock,
            Some('p' | 'P')
        )
    );
    if !is_paste {
        return false;
    }
    if !matches!(
        state.engine.selected_register,
        None | Some('"') | Some('+') | Some('*')
    ) {
        return false;
    }
    // Read system clipboard and load into registers
    if let Some(ref cb_read) = state.engine.clipboard_read {
        if let Ok(text) = cb_read() {
            if !text.is_empty() {
                state.engine.load_clipboard_for_paste(text);
            }
        }
    }
    // Let engine execute the paste
    state.engine.handle_key(key_name, unicode, false);
    true
}

fn setup_win_clipboard(engine: &mut Engine) {
    engine.clipboard_read = Some(Box::new(|| {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard"])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| String::from_utf8(o.stdout).map_err(|e| e.to_string()))
            .map(|s| s.trim_end_matches("\r\n").to_string())
    }));
    engine.clipboard_write = Some(Box::new(|text: &str| {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Set-Clipboard", "-Value", text])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())
            .and_then(|mut c| c.wait().map_err(|e| e.to_string()))
            .map(|_| ())
    }));
}

// ─── Terminal PTY key translation ───────────────────────────────────────────

/// Translate a win-gui key event into the byte sequence expected by a PTY.
fn translate_key_to_pty(key_name: &str, unicode: Option<char>, ctrl: bool) -> Vec<u8> {
    if ctrl {
        if let Some(ch) = unicode {
            let b = ch.to_ascii_lowercase() as u8;
            if b.is_ascii() {
                return vec![b & 0x1f];
            }
        }
        // Ctrl+named keys
        return match key_name {
            "BackSpace" => b"\x08".to_vec(),
            _ => vec![],
        };
    }

    match key_name {
        "Return" => b"\r".to_vec(),
        "BackSpace" => b"\x7f".to_vec(),
        "Tab" => b"\t".to_vec(),
        "Escape" => b"\x1b".to_vec(),
        "Up" => b"\x1b[A".to_vec(),
        "Down" => b"\x1b[B".to_vec(),
        "Right" => b"\x1b[C".to_vec(),
        "Left" => b"\x1b[D".to_vec(),
        "Home" => b"\x1b[H".to_vec(),
        "End" => b"\x1b[F".to_vec(),
        "Delete" => b"\x1b[3~".to_vec(),
        "Insert" => b"\x1b[2~".to_vec(),
        "Page_Up" => b"\x1b[5~".to_vec(),
        "Page_Down" => b"\x1b[6~".to_vec(),
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
            // Regular character — use unicode if available
            if let Some(ch) = unicode {
                ch.to_string().into_bytes()
            } else {
                vec![]
            }
        }
    }
}

/// Hit-test a per-window status bar click, returning the action if any segment matches.
fn win_status_segment_hit_test(
    status: &crate::render::WindowStatusLine,
    width: usize,
    click_col: usize,
) -> Option<StatusAction> {
    let right_width: usize = status
        .right_segments
        .iter()
        .map(|s| s.text.chars().count())
        .sum();
    let right_start = width.saturating_sub(right_width);

    let mut col = 0;
    for seg in &status.left_segments {
        let seg_len = seg.text.chars().count();
        if click_col >= col && click_col < col + seg_len {
            return seg.action.clone();
        }
        col += seg_len;
    }

    let mut col = right_start;
    for seg in &status.right_segments {
        let seg_len = seg.text.chars().count();
        if click_col >= col && click_col < col + seg_len {
            return seg.action.clone();
        }
        col += seg_len;
    }

    None
}
