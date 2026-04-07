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
use std::path::PathBuf;

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
use crate::core::window::WindowRect;
use crate::icons;
use crate::render::{self, build_screen_layout, Theme};

use self::draw::DrawContext;
use self::input::{translate_char, translate_vk};

// Timer ID for periodic ticks (LSP poll, syntax debounce, swap files, etc.)
const TICK_TIMER_ID: usize = 1;
const TICK_INTERVAL_MS: u32 = 50;

// ─── Per-window state stored in a thread-local ──────────────────────────────

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

    // Create text format (monospace font)
    let font_size = 14.0f32;
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
            style: CS_HREDRAW | CS_VREDRAW,
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
            if on_key_down(wparam, lparam) {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_CHAR => {
            on_char(wparam);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_mouse_click(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            on_mouse_wheel(hwnd, wparam);
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

        let Some(ref rt) = state.render_target else {
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

        // Reserve rows: 1 tab bar + 1 status bar + 1 command line = 3 rows
        let chrome_rows = 3.0;
        let editor_y = lh; // below tab bar
        let editor_h = height - chrome_rows * lh;

        // Compute viewport for the engine
        let viewport_lines = (editor_h / lh).floor() as usize;
        let viewport_cols = (width / cw).floor() as usize;

        // Update all windows' viewport sizes
        let window_ids: Vec<_> = state.engine.windows.keys().copied().collect();
        for wid in &window_ids {
            state.engine.set_viewport_for_window(*wid, viewport_lines.saturating_sub(1), viewport_cols);
        }

        // Build window rects — single-window for now
        let window_rects: Vec<_> = state
            .engine
            .windows
            .keys()
            .map(|&wid| {
                (
                    wid,
                    WindowRect::new(0.0, editor_y, width, editor_h),
                )
            })
            .collect();

        let screen =
            build_screen_layout(&state.engine, &state.theme, &window_rects, lh, cw, true);

        let ctx = DrawContext {
            rt,
            dwrite: &state.dwrite_factory,
            format: &state.text_format,
            theme: &state.theme,
            char_width: state.char_width,
            line_height: state.line_height,
        };

        unsafe {
            rt.BeginDraw();
            ctx.draw_frame(&screen);
            let _ = rt.EndDraw(None, None);
        }

        // Validate the paint
        unsafe {
            let _ = ValidateRect(Some(hwnd), None);
        }
    });
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
        let action = state
            .engine
            .handle_key(&key.key_name, key.unicode, key.ctrl);
        let quit = handle_action(&mut state.engine, action);
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
        let action = state
            .engine
            .handle_key(&key.key_name, key.unicode, key.ctrl);
        let _ = handle_action(&mut state.engine, action);
        unsafe {
            let _ = InvalidateRect(Some(state.hwnd), None, false);
        }
    });
}

fn on_mouse_click(hwnd: HWND, lparam: LPARAM) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let cw = state.char_width;
        let lh = state.line_height;

        // Convert pixel to editor position
        // Tab bar occupies row 0, editor starts at row 1
        let editor_row = ((y - lh) / lh).floor().max(0.0) as usize;
        let gutter_chars = state
            .engine
            .windows
            .values()
            .next()
            .map(|w| {
                let bs = state.engine.buffer_manager.get(w.buffer_id);
                let total_lines = bs.map_or(1, |s| s.buffer.len_lines());
                render::calculate_gutter_cols(
                    state.engine.settings.line_numbers,
                    total_lines,
                    cw as f64,
                    false,
                    false,
                )
            })
            .unwrap_or(4);
        let text_x = x - (gutter_chars as f32) * cw;
        let col = (text_x / cw).max(0.0).floor() as usize;

        // Move cursor via engine's set_cursor_for_window (handles clamping)
        let scroll_top = state.engine.view().scroll_top;
        let target_line = scroll_top + editor_row;
        let active_wid = state.engine.active_window_id();
        state
            .engine
            .set_cursor_for_window(active_wid, target_line, col);

        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    });
}

fn on_mouse_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
    let lines = -(delta as i32) / 120 * 3; // 3 lines per notch

    APP.with(|app| {
        let mut app = app.borrow_mut();
        let state = app.as_mut().expect("AppState");

        let scroll_top = state.engine.view().scroll_top;
        let new_top = if lines > 0 {
            scroll_top.saturating_add(lines as usize)
        } else {
            scroll_top.saturating_sub((-lines) as usize)
        };
        let max = state.engine.buffer().len_lines().saturating_sub(1);
        state.engine.view_mut().scroll_top = new_top.min(max);

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
        // tick_notifications always causes a redraw check
        needs_redraw = true;

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
        EngineAction::ToggleSidebar => false,
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
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard"])
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| String::from_utf8(o.stdout).map_err(|e| e.to_string()))
            .map(|s| s.trim_end_matches("\r\n").to_string())
    }));
    engine.clipboard_write = Some(Box::new(|text: &str| {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Set-Clipboard", "-Value", text])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())
            .and_then(|mut c| c.wait().map_err(|e| e.to_string()))
            .map(|_| ())
    }));
}
