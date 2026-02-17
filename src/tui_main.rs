//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self as ct_event, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as RColor, Modifier};
use ratatui::Terminal;

use crate::core::engine::EngineAction;
use crate::core::{Engine, OpenMode, WindowRect};
use crate::render::{
    self, build_screen_layout, Color, CursorShape, RenderedLine, RenderedWindow, SelectionKind,
    Theme,
};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Initialise the engine, set up the terminal, run the event loop, and restore
/// the terminal on exit.
pub fn run(file_path: Option<PathBuf>) {
    let mut engine = Engine::new();
    if let Some(path) = file_path {
        if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
            eprintln!("vimcode: {}", e);
        }
    }

    enable_raw_mode().expect("enable raw mode");
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).expect("enter alternate screen");

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal.clear().expect("clear terminal");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop(&mut terminal, &mut engine);
    }));

    restore_terminal(&mut terminal);

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
}

// ─── Event loop ───────────────────────────────────────────────────────────────

fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, engine: &mut Engine) {
    let theme = Theme::onedark();

    loop {
        terminal
            .draw(|frame| {
                let area = frame.size();
                let screen = build_screen_for_tui(engine, &theme, area);
                draw_frame(frame, &screen, &theme);
            })
            .expect("draw frame");

        if !ct_event::poll(Duration::from_millis(20)).expect("poll") {
            continue;
        }

        match ct_event::read().expect("read event") {
            Event::Key(key_event) => {
                if let Some((key_name, unicode, ctrl)) = translate_key(key_event) {
                    let action = engine.handle_key(&key_name, unicode, ctrl);
                    if handle_action(engine, action) {
                        break;
                    }
                    loop {
                        let (has_more, action) = engine.advance_macro_playback();
                        if handle_action(engine, action) {
                            return;
                        }
                        if !has_more {
                            break;
                        }
                    }
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

// ─── Screen layout bridging ───────────────────────────────────────────────────

fn build_screen_for_tui(engine: &Engine, theme: &Theme, area: Rect) -> render::ScreenLayout {
    let content_rows = area.height.saturating_sub(3); // tab + status + command
    let content_bounds = WindowRect::new(0.0, 0.0, area.width as f64, content_rows as f64);
    let window_rects = engine.calculate_window_rects(content_bounds);
    build_screen_layout(engine, theme, &window_rects, 1.0, 1.0)
}

// ─── Frame rendering ──────────────────────────────────────────────────────────

fn draw_frame(frame: &mut ratatui::Frame, screen: &render::ScreenLayout, theme: &Theme) {
    let area = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let tab_area = chunks[0];
    let editor_area = chunks[1];
    let status_area = chunks[2];
    let cmd_area = chunks[3];

    render_tab_bar(frame.buffer_mut(), tab_area, &screen.tab_bar, theme);
    render_all_windows(frame, editor_area, &screen.windows, theme);
    render_status_line(
        frame.buffer_mut(),
        status_area,
        &screen.status_left,
        &screen.status_right,
        theme,
    );
    render_command_line(frame.buffer_mut(), cmd_area, &screen.command, theme);
}

// ─── Cell helper ──────────────────────────────────────────────────────────────

/// Set a single buffer cell, bounds-checking against the buffer's area.
fn set_cell(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, ch: char, fg: RColor, bg: RColor) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        buf.get_mut(x, y).set_char(ch).set_fg(fg).set_bg(bg);
    }
}

fn set_cell_styled(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    ch: char,
    fg: RColor,
    bg: RColor,
    modifier: Modifier,
) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        let cell = buf.get_mut(x, y);
        cell.set_char(ch).set_fg(fg).set_bg(bg);
        cell.modifier = modifier;
    }
}

// ─── Tab bar ──────────────────────────────────────────────────────────────────

fn render_tab_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    tabs: &[render::TabInfo],
    theme: &Theme,
) {
    let bar_bg = rc(theme.tab_bar_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
    }

    let mut x = area.x;
    for tab in tabs {
        let (fg, bg) = match (tab.active, tab.preview) {
            (true, true) => (rc(theme.tab_preview_active_fg), rc(theme.tab_active_bg)),
            (true, false) => (rc(theme.tab_active_fg), rc(theme.tab_active_bg)),
            (false, true) => (rc(theme.tab_preview_inactive_fg), rc(theme.tab_bar_bg)),
            (false, false) => (rc(theme.tab_inactive_fg), rc(theme.tab_bar_bg)),
        };
        let modifier = if tab.preview {
            Modifier::ITALIC
        } else {
            Modifier::empty()
        };

        for ch in tab.name.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell_styled(buf, x, area.y, ch, fg, bg, modifier);
            x += 1;
        }
        if x < area.x + area.width {
            set_cell(buf, x, area.y, ' ', bar_bg, bar_bg);
            x += 1;
        }
    }
}

// ─── Editor windows ───────────────────────────────────────────────────────────

fn render_all_windows(
    frame: &mut ratatui::Frame,
    editor_area: Rect,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    for window in windows {
        let win_rect = Rect {
            x: editor_area.x + window.rect.x as u16,
            y: editor_area.y + window.rect.y as u16,
            width: window.rect.width as u16,
            height: window.rect.height as u16,
        };
        render_window(frame, win_rect, window, theme);
    }
    render_separators(frame.buffer_mut(), editor_area, windows, theme);
}

fn render_window(frame: &mut ratatui::Frame, area: Rect, window: &RenderedWindow, theme: &Theme) {
    let window_bg = rc(if window.show_active_bg {
        theme.active_background
    } else {
        theme.background
    });
    let default_fg = rc(theme.foreground);
    let gutter_w = window.gutter_char_width as u16;

    // Fill background
    for row in 0..area.height {
        for col in 0..area.width {
            set_cell(
                frame.buffer_mut(),
                area.x + col,
                area.y + row,
                ' ',
                default_fg,
                window_bg,
            );
        }
    }

    for (row_idx, line) in window.lines.iter().enumerate() {
        let screen_y = area.y + row_idx as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Gutter
        if gutter_w > 0 {
            let gutter_fg = rc(if line.is_current_line {
                theme.line_number_active_fg
            } else {
                theme.line_number_fg
            });
            for (i, ch) in line.gutter_text.chars().enumerate() {
                let gx = area.x + i as u16;
                if gx >= area.x + gutter_w {
                    break;
                }
                set_cell(frame.buffer_mut(), gx, screen_y, ch, gutter_fg, window_bg);
            }
        }

        // Text
        let text_area_x = area.x + gutter_w;
        let text_width = area.width.saturating_sub(gutter_w);
        render_text_line(
            frame.buffer_mut(),
            text_area_x,
            screen_y,
            text_width,
            line,
            window.scroll_left,
            theme,
            window_bg,
        );
    }

    // Selection overlay
    if let Some(sel) = &window.selection {
        render_selection(frame.buffer_mut(), area, window, sel, window_bg, theme);
    }

    // Cursor
    if let Some((cursor_pos, cursor_shape)) = &window.cursor {
        let cursor_screen_y = area.y + cursor_pos.view_line as u16;
        let vis_col = cursor_pos.col.saturating_sub(window.scroll_left) as u16;
        let cursor_screen_x = area.x + gutter_w + vis_col;

        let buf = frame.buffer_mut();
        let buf_area = buf.area;

        match cursor_shape {
            CursorShape::Block => {
                if cursor_screen_x < buf_area.x + buf_area.width
                    && cursor_screen_y < buf_area.y + buf_area.height
                {
                    let cell = buf.get_mut(cursor_screen_x, cursor_screen_y);
                    // Invert colours
                    let old_fg = cell.fg;
                    let old_bg = cell.bg;
                    cell.set_fg(old_bg).set_bg(old_fg);
                }
            }
            CursorShape::Bar => {
                frame.set_cursor(cursor_screen_x, cursor_screen_y);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_text_line(
    buf: &mut ratatui::buffer::Buffer,
    x_start: u16,
    y: u16,
    max_width: u16,
    line: &RenderedLine,
    scroll_left: usize,
    theme: &Theme,
    window_bg: RColor,
) {
    let raw = &line.raw_text;
    let chars: Vec<char> = raw.chars().filter(|&c| c != '\n' && c != '\r').collect();

    let mut char_fgs: Vec<Color> = vec![theme.foreground; chars.len()];
    let mut char_bgs: Vec<Option<Color>> = vec![None; chars.len()];

    for span in &line.spans {
        let start = byte_to_char_idx(raw, span.start_byte);
        let end = byte_to_char_idx(raw, span.end_byte).min(chars.len());
        for i in start..end {
            char_fgs[i] = span.style.fg;
            char_bgs[i] = span.style.bg;
        }
    }

    for i in scroll_left..chars.len() {
        let col = (i - scroll_left) as u16;
        if col >= max_width {
            break;
        }
        let fg = rc(char_fgs[i]);
        let bg = char_bgs[i].map(rc).unwrap_or(window_bg);
        set_cell(buf, x_start + col, y, chars[i], fg, bg);
    }
}

fn render_selection(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    window: &RenderedWindow,
    sel: &render::SelectionRange,
    window_bg: RColor,
    theme: &Theme,
) {
    let sel_bg = rc(theme.selection);
    let default_fg = rc(theme.foreground);
    let gutter_w = window.gutter_char_width as u16;
    let text_area_x = area.x + gutter_w;
    let text_width = area.width.saturating_sub(gutter_w) as usize;

    for (row_idx, line) in window.lines.iter().enumerate() {
        let buffer_line = window.scroll_top + row_idx;
        if buffer_line < sel.start_line || buffer_line > sel.end_line {
            continue;
        }
        let screen_y = area.y + row_idx as u16;

        let col_start = match sel.kind {
            SelectionKind::Line => 0,
            SelectionKind::Char => {
                if buffer_line == sel.start_line {
                    sel.start_col
                } else {
                    0
                }
            }
            SelectionKind::Block => sel.start_col,
        };
        let col_end = match sel.kind {
            SelectionKind::Line => usize::MAX,
            SelectionKind::Char => {
                if buffer_line == sel.end_line {
                    sel.end_col + 1
                } else {
                    usize::MAX
                }
            }
            SelectionKind::Block => sel.end_col + 1,
        };

        let char_count = line.raw_text.chars().filter(|&c| c != '\n').count().max(1);
        let effective_end = col_end.min(char_count);

        for col in col_start..effective_end {
            if col < window.scroll_left {
                continue;
            }
            let screen_col = (col - window.scroll_left) as u16;
            if screen_col >= text_width as u16 {
                break;
            }
            let sx = text_area_x + screen_col;
            let buf_area = buf.area;
            if sx < buf_area.x + buf_area.width && screen_y < buf_area.y + buf_area.height {
                let cell = buf.get_mut(sx, screen_y);
                let old_fg = cell.fg;
                cell.set_bg(sel_bg);
                // Keep text visible against selection background
                if old_fg == window_bg {
                    cell.set_fg(default_fg);
                }
            }
        }
    }
}

fn render_separators(
    buf: &mut ratatui::buffer::Buffer,
    editor_area: Rect,
    windows: &[RenderedWindow],
    theme: &Theme,
) {
    if windows.len() <= 1 {
        return;
    }
    let sep_fg = rc(theme.separator);
    let sep_bg = rc(theme.background);

    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            let a = &windows[i].rect;
            let b = &windows[j].rect;

            // Vertical separator
            if (a.x + a.width - b.x).abs() < 1.0 {
                let sep_x = editor_area.x + (a.x + a.width) as u16;
                let y_start = editor_area.y + a.y.max(b.y) as u16;
                let y_end = editor_area.y + (a.y + a.height).min(b.y + b.height) as u16;
                for y in y_start..y_end {
                    set_cell(buf, sep_x.saturating_sub(1), y, '│', sep_fg, sep_bg);
                }
            }

            // Horizontal separator
            if (a.y + a.height - b.y).abs() < 1.0 {
                let sep_y = editor_area.y + (a.y + a.height) as u16;
                let x_start = editor_area.x + a.x.max(b.x) as u16;
                let x_end = editor_area.x + (a.x + a.width).min(b.x + b.width) as u16;
                for x in x_start..x_end {
                    set_cell(buf, x, sep_y.saturating_sub(1), '─', sep_fg, sep_bg);
                }
            }
        }
    }
}

// ─── Status / command line ────────────────────────────────────────────────────

fn render_status_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    left: &str,
    right: &str,
    theme: &Theme,
) {
    let fg = rc(theme.status_fg);
    let bg = rc(theme.status_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }

    let mut x = area.x;
    for ch in left.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, area.y, ch, fg, bg);
        x += 1;
    }

    let right_chars: Vec<char> = right.chars().collect();
    let right_len = right_chars.len() as u16;
    if right_len <= area.width {
        let mut rx = area.x + area.width - right_len;
        for &ch in &right_chars {
            if rx >= area.x + area.width {
                break;
            }
            set_cell(buf, rx, area.y, ch, fg, bg);
            rx += 1;
        }
    }
}

fn render_command_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    command: &render::CommandLineData,
    theme: &Theme,
) {
    let fg = rc(theme.command_fg);
    let bg = rc(theme.command_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }

    if command.right_align {
        let chars: Vec<char> = command.text.chars().collect();
        let len = chars.len() as u16;
        if len <= area.width {
            let mut x = area.x + area.width - len;
            for &ch in &chars {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, area.y, ch, fg, bg);
                x += 1;
            }
        }
    } else {
        let mut x = area.x;
        for ch in command.text.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, area.y, ch, fg, bg);
            x += 1;
        }
    }

    // Command-line cursor (inverted block at insertion point)
    if command.show_cursor {
        let cursor_col = command.cursor_anchor_text.chars().count() as u16;
        let cx = area.x + cursor_col.min(area.width.saturating_sub(1));
        let buf_area = buf.area;
        if cx < buf_area.x + buf_area.width {
            let cell = buf.get_mut(cx, area.y);
            let old_fg = cell.fg;
            let old_bg = cell.bg;
            cell.set_fg(old_bg).set_bg(old_fg);
        }
    }
}

// ─── Input translation ────────────────────────────────────────────────────────

fn translate_key(event: KeyEvent) -> Option<(String, Option<char>, bool)> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    match event.code {
        KeyCode::Char(c) => {
            let unicode = if ctrl {
                Some(c.to_ascii_lowercase())
            } else {
                Some(c)
            };
            Some(("".to_string(), unicode, ctrl))
        }
        KeyCode::Esc => Some(("Escape".to_string(), None, false)),
        KeyCode::Enter => Some(("Return".to_string(), None, false)),
        KeyCode::Backspace => Some(("BackSpace".to_string(), None, false)),
        KeyCode::Delete => Some(("Delete".to_string(), None, false)),
        KeyCode::Tab => Some(("Tab".to_string(), None, false)),
        KeyCode::BackTab => Some(("ISO_Left_Tab".to_string(), None, false)),
        KeyCode::Up => Some(("Up".to_string(), None, false)),
        KeyCode::Down => Some(("Down".to_string(), None, false)),
        KeyCode::Left => Some(("Left".to_string(), None, false)),
        KeyCode::Right => Some(("Right".to_string(), None, false)),
        KeyCode::Home => Some(("Home".to_string(), None, false)),
        KeyCode::End => Some(("End".to_string(), None, false)),
        KeyCode::PageUp => Some(("Page_Up".to_string(), None, false)),
        KeyCode::PageDown => Some(("Page_Down".to_string(), None, false)),
        KeyCode::F(n) => Some((format!("F{}", n), None, false)),
        _ => None,
    }
}

// ─── Engine action handling ───────────────────────────────────────────────────

fn handle_action(engine: &mut Engine, action: EngineAction) -> bool {
    match action {
        EngineAction::Quit | EngineAction::SaveQuit => {
            save_session(engine);
            true
        }
        EngineAction::OpenFile(path) => {
            if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                engine.message = e;
            }
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
    let _ = engine.session.save();
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Convert a `render::Color` to a ratatui `Color::Rgb`.
#[inline]
fn rc(c: Color) -> RColor {
    RColor::Rgb(c.r, c.g, c.b)
}

/// Return the character index that corresponds to a byte offset in a UTF-8
/// string. Returns the total char count if `byte_offset` is past the end.
fn byte_to_char_idx(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())].chars().count()
}
