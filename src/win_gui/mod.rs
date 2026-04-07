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
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::core::engine::{open_url_in_browser, Engine, EngineAction, OpenMode};
use crate::core::window::{GroupId, WindowId, WindowRect};
use crate::icons;
use crate::render::{self, build_screen_layout, ScreenLayout, Theme};

use self::draw::DrawContext;
use self::input::{translate_char, translate_vk};

// Timer ID for periodic ticks (LSP poll, syntax debounce, swap files, etc.)
const TICK_TIMER_ID: usize = 1;
const TICK_INTERVAL_MS: u32 = 50;

/// Double-click detection threshold.
const DOUBLE_CLICK_MS: u64 = 400;

/// Hit zone width (pixels) for sidebar resize drag handle.
const SIDEBAR_RESIZE_HIT_PX: f32 = 5.0;

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
            activity_bar_px: 36.0,
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
            collect_explorer_rows(&path, depth + 1, expanded, show_hidden, case_insensitive, out);
        }
    }
}

struct AppState {
    engine: Engine,
    theme: Theme,
    d2d_factory: ID2D1Factory,
    dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    render_target: Option<ID2D1HwndRenderTarget>,
    hwnd: HWND,
    char_width: f32,
    line_height: f32,
    dpi_scale: f32,

    // ── Cached layout from last draw pass ────────────────────────────────
    tab_slots: Vec<TabSlot>,
    cached_window_rects: Vec<CachedWindowRect>,

    // ── Sidebar ───────────────────────────────────────────────────────────
    sidebar: WinSidebar,

    // ── Hot-reload tracking ──────────────────────────────────────────────
    current_colorscheme: String,
    current_font_size: i32,

    // ── Mouse state ──────────────────────────────────────────────────────
    mouse_text_drag: bool,
    sidebar_resize_drag: bool,
    last_click_time: Instant,
    last_click_pos: (i16, i16),
}

thread_local! {
    static APP: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

// ─── Entry point ────────────────────────────────────────────────────────────

pub fn run(file_path: Option<PathBuf>) {
    let mut engine = Engine::new();
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

    let dwrite_factory: IDWriteFactory = unsafe {
        DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
    }
    .expect("DWriteCreateFactory");

    // Create text format (monospace font, size from settings)
    let initial_colorscheme = engine.settings.colorscheme.clone();
    let initial_font_size = engine.settings.font_size;
    let font_size = initial_font_size as f32;
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

    // Measure monospace dimensions
    let char_width = draw::measure_char_width(&dwrite_factory, &text_format);
    let line_height = draw::measure_line_height(&dwrite_factory, &text_format);

    // Register window class + create window
    let hwnd = create_window();

    // Store state in thread-local
    APP.with(|app| {
        *app.borrow_mut() = Some(AppState {
            engine,
            theme,
            d2d_factory,
            dwrite_factory,
            text_format,
            render_target: None,
            hwnd,
            char_width,
            line_height,
            dpi_scale: 1.0,
            tab_slots: Vec::new(),
            cached_window_rects: Vec::new(),
            sidebar: WinSidebar::new(),
            current_colorscheme: initial_colorscheme,
            current_font_size: initial_font_size,
            mouse_text_drag: false,
            sidebar_resize_drag: false,
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
        });
    });

    // Create render target now that we have the HWND
    create_render_target();

    // Show window
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
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

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
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

unsafe fn wnd_proc_inner(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
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
        WM_DESTROY => {
            on_destroy();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ─── Message handlers ────────────────────────────────────────────────────────

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

        // Reserve: 1 status bar + 1 command line at the bottom
        let tab_bar_height = lh;
        let editor_bottom = height - 2.0 * lh; // status + command line

        // Use engine's group-aware rect calculation (handles splits)
        let editor_bounds =
            WindowRect::new(sidebar_left, 0.0, width - sidebar_left, editor_bottom);
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
        if state.sidebar.visible && state.sidebar.active_panel == SidebarPanel::Explorer && state.sidebar.dirty {
            if let Some(ref root) = state.engine.workspace_root.clone() {
                state.sidebar.build_rows(root);
                state.sidebar.dirty = false;
            }
        }

        let screen = build_screen_layout(
            &state.engine,
            &state.theme,
            &window_rects,
            lh,
            cw,
            true,
        );

        // Cache window rects for mouse hit-testing
        cache_layout(state, &screen, &window_rects);

        let ctx = DrawContext {
            rt: &rt,
            dwrite: &state.dwrite_factory,
            format: &state.text_format,
            theme: &state.theme,
            char_width: state.char_width,
            line_height: state.line_height,
            editor_left: state.sidebar.total_width(),
        };

        unsafe {
            rt.BeginDraw();
            ctx.draw_frame(&screen);
            // Draw sidebar on top of left edge (activity bar always, panel when visible)
            ctx.draw_sidebar(&state.sidebar, &screen);
            // Notification toasts
            ctx.draw_notifications(&state.engine.notifications);
            let _ = rt.EndDraw(None, None);
        }

        // Validate the paint
        unsafe {
            let _ = ValidateRect(Some(hwnd), None);
        }
    });
}

/// Cache tab slot positions and window rects after each draw for mouse hit-testing.
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
            let tab_y = gtb.bounds.y as f32 - lh;
            let mut x = gtb.bounds.x as f32;
            for (idx, tab) in gtb.tabs.iter().enumerate() {
                let name_w = (tab.name.chars().count() as f32 + 3.0) * cw;
                let close_x = x + name_w - 2.0 * cw;
                state.tab_slots.push(TabSlot {
                    group_id: gtb.group_id,
                    tab_idx: idx,
                    x_start: x,
                    x_end: x + name_w,
                    close_x_start: close_x,
                    y: tab_y,
                    height: lh,
                });
                x += name_w;
            }
        }
    } else {
        // Single-group: use the flat tab_bar
        let group_id = state.engine.active_group;
        let tab_y = 0.0f32;
        let mut x = 0.0f32;
        for (idx, tab) in screen.tab_bar.iter().enumerate() {
            let name_w = (tab.name.chars().count() as f32 + 3.0) * cw;
            let close_x = x + name_w - 2.0 * cw;
            state.tab_slots.push(TabSlot {
                group_id,
                tab_idx: idx,
                x_start: x,
                x_end: x + name_w,
                close_x_start: close_x,
                y: tab_y,
                height: lh,
            });
            x += name_w;
        }
    }

    // Cache window rects with gutter widths
    state.cached_window_rects.clear();
    for rw in &screen.windows {
        state.cached_window_rects.push(CachedWindowRect {
            window_id: rw.window_id,
            rect: WindowRect::new(
                rw.rect.x,
                rw.rect.y,
                rw.rect.width,
                rw.rect.height,
            ),
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

        // Sidebar keyboard navigation when focused
        if state.sidebar.has_focus && state.sidebar.visible {
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
                                let _ = state.engine.open_file_with_mode(&path, OpenMode::Permanent);
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
                        if idx < state.sidebar.rows.len() && state.sidebar.rows[idx].is_dir && state.sidebar.rows[idx].is_expanded {
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

        let action = state
            .engine
            .handle_key(&key.key_name, key.unicode, key.ctrl);
        let quit = handle_action_with_sidebar(state, action);
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
    let Some(key) = translate_char(ch) else {
        return;
    };

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // Sidebar keyboard navigation for character keys (j/k/h/l)
        if state.sidebar.has_focus && state.sidebar.visible {
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
                                let _ = state.engine.open_file_with_mode(&path, OpenMode::Permanent);
                                state.sidebar.has_focus = false;
                            }
                        }
                    }
                    true
                }
                'h' => {
                    if state.sidebar.active_panel == SidebarPanel::Explorer {
                        let idx = state.sidebar.selected;
                        if idx < state.sidebar.rows.len() && state.sidebar.rows[idx].is_dir && state.sidebar.rows[idx].is_expanded {
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

        let action = state
            .engine
            .handle_key(&key.key_name, key.unicode, key.ctrl);
        let _ = handle_action_with_sidebar(state, action);
        unsafe {
            let _ = InvalidateRect(Some(state.hwnd), None, false);
        }
    });
}

/// Extract pixel coordinates from LPARAM.
fn lparam_xy(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x, y)
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

        // Exclude scrollbar area (rightmost 6px)
        let scrollbar_w = 6.0f32;
        if px >= rx && px < rx + rw - scrollbar_w && py >= ry && py < ry + rh {
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

fn on_mouse_down(hwnd: HWND, lparam: LPARAM) {
    let (px, py) = lparam_xy(lparam);
    let ix = (lparam.0 & 0xFFFF) as i16;
    let iy = ((lparam.0 >> 16) & 0xFFFF) as i16;

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

        // ── Check activity bar clicks ────────────────────────────────────
        let ab_w = state.sidebar.activity_bar_px;
        if px < ab_w {
            let row = (py / state.line_height).floor() as usize;
            let panels = [
                SidebarPanel::Explorer,
                SidebarPanel::Search,
                SidebarPanel::Debug,
                SidebarPanel::Git,
                SidebarPanel::Extensions,
                SidebarPanel::Ai,
            ];
            if row < panels.len() {
                let clicked_panel = panels[row];
                if state.sidebar.visible && state.sidebar.active_panel == clicked_panel {
                    state.sidebar.visible = false;
                } else {
                    state.sidebar.active_panel = clicked_panel;
                    state.sidebar.visible = true;
                    state.sidebar.dirty = true;
                    // Auto-expand root
                    if clicked_panel == SidebarPanel::Explorer && state.sidebar.expanded.is_empty() {
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
        if state.sidebar.visible && px < state.sidebar.total_width() {
            state.sidebar.has_focus = px >= ab_w; // focus panel area, not activity bar
            let row = (py / state.line_height).floor() as usize;

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
                            let _ = state.engine.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                    }
                }
            }
            // Other panels: Git, Debug, etc. — click handling TBD

            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return;
        }

        // ── Check tab bar hits first ─────────────────────────────────────
        for slot in &state.tab_slots {
            if px >= slot.x_start && px < slot.x_end && py >= slot.y && py < slot.y + slot.height {
                // Hit a tab — switch group and tab
                state.engine.active_group = slot.group_id;
                if px >= slot.close_x_start {
                    // Close button hit
                    state.engine.goto_tab(slot.tab_idx);
                    state.engine.close_tab();
                } else {
                    state.engine.goto_tab(slot.tab_idx);
                }
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return;
            }
        }

        // ── Double-click detection ───────────────────────────────────────
        let now = Instant::now();
        let is_double = now.duration_since(state.last_click_time).as_millis() < DOUBLE_CLICK_MS as u128
            && state.last_click_pos == (ix, iy);
        state.last_click_time = now;
        state.last_click_pos = (ix, iy);

        // ── Editor area click ────────────────────────────────────────────
        state.sidebar.has_focus = false;
        if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
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
        state.mouse_text_drag = false;
        state.sidebar_resize_drag = false;
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

        // Cursor shape: resize arrow near sidebar edge
        let edge = state.sidebar.total_width();
        let near_edge = state.sidebar.visible && (px - edge).abs() < SIDEBAR_RESIZE_HIT_PX;
        unsafe {
            let cursor = if near_edge {
                LoadCursorW(None, IDC_SIZEWE).unwrap_or_default()
            } else {
                LoadCursorW(None, IDC_IBEAM).unwrap_or_default()
            };
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

        // Only handle right-click in editor area
        if let Some((wid, line, col)) = pixel_to_editor_pos(state, px, py) {
            // Position cursor at click location first
            state.engine.mouse_click(wid, line, col);
            // Convert pixel to screen row/col for context menu positioning
            let lh = state.line_height;
            let cw = state.char_width;
            let screen_col = (px / cw).floor() as u16;
            let screen_row = (py / lh).floor() as u16;
            state.engine.open_editor_context_menu(screen_col, screen_row);
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
    let mut pt = POINT { x: screen_x as i32, y: screen_y as i32 };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    let px = pt.x as f32;

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        // Sidebar scroll
        if state.sidebar.visible && px < state.sidebar.total_width() {
            let max = state.sidebar.rows.len().saturating_sub(1);
            if lines > 0 {
                state.sidebar.scroll_top = state.sidebar.scroll_top.saturating_add(lines as usize).min(max);
            } else {
                state.sidebar.scroll_top = state.sidebar.scroll_top.saturating_sub((-lines) as usize);
            }
        } else {
            // Editor scroll
            let scroll_top = state.engine.view().scroll_top;
            let new_top = if lines > 0 {
                scroll_top.saturating_add(lines as usize)
            } else {
                scroll_top.saturating_sub((-lines) as usize)
            };
            let max = state.engine.buffer().len_lines().saturating_sub(1);
            state.engine.view_mut().scroll_top = new_top.min(max);
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

        // Syntax debounce
        if state.engine.tick_syntax_debounce() {
            needs_redraw = true;
        }

        // Swap file periodic writes
        state.engine.tick_swap_files();

        // Notification ticker
        state.engine.tick_notifications();
        needs_redraw = true;

        // Hot-reload theme
        if state.engine.settings.colorscheme != state.current_colorscheme {
            state.theme = Theme::from_name(&state.engine.settings.colorscheme);
            state.current_colorscheme = state.engine.settings.colorscheme.clone();
            needs_redraw = true;
        }

        // Hot-reload font size
        if state.engine.settings.font_size != state.current_font_size {
            let new_size = state.engine.settings.font_size as f32;
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

        if needs_redraw {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    });
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

/// Process an engine action. Returns `true` if the app should quit.
/// `state` is optional — some callers only have the engine.
fn handle_action_with_sidebar(state: &mut AppState, action: EngineAction) -> bool {
    if matches!(action, EngineAction::ToggleSidebar) {
        state.sidebar.visible = !state.sidebar.visible;
        state.sidebar.dirty = true;
        if state.sidebar.visible && state.sidebar.expanded.is_empty() {
            if let Some(ref root) = state.engine.workspace_root.clone() {
                state.sidebar.expanded.insert(root.clone());
            }
        }
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
            if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                engine.message = e;
            }
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
        EngineAction::OpenTerminal | EngineAction::RunInTerminal(_) => false,
        EngineAction::OpenFolderDialog
        | EngineAction::OpenWorkspaceDialog
        | EngineAction::SaveWorkspaceAsDialog
        | EngineAction::OpenRecentDialog => false,
        EngineAction::QuitWithUnsaved => false,
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

// ─── Clipboard ───────────────────────────────────────────────────────────────

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
