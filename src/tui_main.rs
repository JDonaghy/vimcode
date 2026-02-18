//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{
    self as ct_event, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as RColor, Modifier};
use ratatui::Terminal;

use crate::core::engine::EngineAction;
use crate::core::{Engine, GitLineStatus, Mode, OpenMode, WindowRect};
use crate::render::{
    self, build_screen_layout, Color, CursorShape, RenderedLine, RenderedWindow, SelectionKind,
    Theme,
};

// ─── Sidebar constants ────────────────────────────────────────────────────────

const SIDEBAR_WIDTH: u16 = 30;
const ACTIVITY_BAR_WIDTH: u16 = 3;

// ─── Activity bar panels ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum TuiPanel {
    Explorer,
    Settings,
}

// ─── Sidebar data structures ──────────────────────────────────────────────────

struct ExplorerRow {
    depth: usize,
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_expanded: bool,
}

struct TuiSidebar {
    visible: bool,
    has_focus: bool,
    active_panel: TuiPanel,
    selected: usize,
    scroll_top: usize,
    rows: Vec<ExplorerRow>,
    root: PathBuf,
    /// Set of directory paths that are currently expanded.
    expanded: HashSet<PathBuf>,
}

impl TuiSidebar {
    fn new(root: PathBuf, visible: bool) -> Self {
        let mut sb = TuiSidebar {
            visible,
            has_focus: false,
            active_panel: TuiPanel::Explorer,
            selected: 0,
            scroll_top: 0,
            rows: Vec::new(),
            root,
            expanded: HashSet::new(),
        };
        sb.build_rows();
        sb
    }

    fn build_rows(&mut self) {
        self.rows.clear();
        let root = self.root.clone();
        collect_rows(&root, 0, &self.expanded, &mut self.rows);
        if !self.rows.is_empty() && self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    fn toggle_dir(&mut self, idx: usize) {
        if idx < self.rows.len() && self.rows[idx].is_dir {
            let path = self.rows[idx].path.clone();
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            } else {
                self.expanded.insert(path);
            }
        }
        self.build_rows();
    }

    /// Scroll so `selected` is visible within the given viewport height.
    fn ensure_visible(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + viewport_height {
            self.scroll_top = self.selected + 1 - viewport_height;
        }
    }
}

/// Recursively build the flat list of visible rows, respecting the `expanded` set.
fn collect_rows(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<ExplorerRow>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    // Dirs first, then alphabetical
    entries.sort_by(|a, b| {
        let ad = a.path().is_dir();
        let bd = b.path().is_dir();
        match (ad, bd) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip dotfiles
        if name.starts_with('.') {
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
            collect_rows(&path, depth + 1, expanded, out);
        }
    }
}

/// ─── Prompt kind for CRUD operations ─────────────────────────────────────────
#[derive(Clone, Debug)]
enum PromptKind {
    NewFile,
    NewFolder,
    DeleteConfirm(PathBuf),
}

/// State for an active sidebar prompt shown in the command line area.
struct SidebarPrompt {
    kind: PromptKind,
    input: String,
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Initialise the engine, set up the terminal, run the event loop, and restore
/// the terminal on exit.
pub fn run(file_path: Option<PathBuf>) {
    let mut engine = Engine::new();
    if let Some(path) = file_path {
        if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
            eprintln!("vimcode: {}", e);
        }
    } else {
        engine.restore_session_files();
    }

    enable_raw_mode().expect("enable raw mode");
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).expect("enter alternate screen");

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
    let _ = execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = terminal.show_cursor();
}

// ─── Event loop ───────────────────────────────────────────────────────────────

fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, engine: &mut Engine) {
    let theme = Theme::onedark();

    // Initialise sidebar from session/settings
    let initial_visible =
        engine.session.explorer_visible || engine.settings.explorer_visible_on_startup;
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut sidebar = TuiSidebar::new(root, initial_visible);

    // Optional active prompt (for sidebar CRUD operations)
    let mut sidebar_prompt: Option<SidebarPrompt> = None;

    // Mutable sidebar width (default SIDEBAR_WIDTH, clamped 15..60)
    let mut sidebar_width: u16 = SIDEBAR_WIDTH;
    // True while user is dragging the resize handle
    let mut dragging_sidebar = false;
    // Cache of the last rendered layout for mouse hit-testing
    let mut last_layout: Option<render::ScreenLayout> = None;

    loop {
        // Sync viewport dimensions so ensure_cursor_visible uses real terminal size.
        // Layout: [activity_bar(3)] [sidebar(sw+1sep, if visible)] [editor_col]
        // editor_col: [tab(1)] / [editor] then global [status(1)] [cmd(1)]
        if let Ok(size) = terminal.size() {
            let content_rows = size.height.saturating_sub(3); // tab + status + cmd
            let gutter_approx = 4u16;
            let sidebar_cols = if sidebar.visible {
                sidebar_width + 1
            } else {
                0
            };
            let content_cols = size
                .width
                .saturating_sub(ACTIVITY_BAR_WIDTH + sidebar_cols + gutter_approx);
            engine.set_viewport_lines(content_rows.max(1) as usize);
            engine.set_viewport_cols(content_cols.max(1) as usize);
        }

        // Build layout before drawing so mouse handler can use it
        let screen = if let Ok(size) = terminal.size() {
            let area = Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            };
            let s = build_screen_for_tui(engine, &theme, area, &sidebar, sidebar_width);
            last_layout = Some(s);
            last_layout.as_ref()
        } else {
            last_layout.as_ref()
        };

        terminal
            .draw(|frame| {
                if let Some(s) = &screen {
                    draw_frame(
                        frame,
                        s,
                        &theme,
                        &sidebar,
                        engine,
                        &sidebar_prompt,
                        sidebar_width,
                    );
                }
            })
            .expect("draw frame");

        // Set terminal cursor shape to match mode / pending key.
        let cursor_style = if !sidebar.has_focus && engine.pending_key == Some('r') {
            SetCursorStyle::SteadyUnderScore
        } else if !sidebar.has_focus {
            match engine.mode {
                Mode::Insert => SetCursorStyle::BlinkingBar,
                _ => SetCursorStyle::SteadyBlock,
            }
        } else {
            SetCursorStyle::SteadyBlock
        };
        let _ = execute!(terminal.backend_mut(), cursor_style);

        if !ct_event::poll(Duration::from_millis(20)).expect("poll") {
            continue;
        }

        match ct_event::read().expect("read event") {
            Event::Key(key_event) => {
                // ── Prompt mode (sidebar CRUD) ──────────────────────────────
                if let Some(ref mut prompt) = sidebar_prompt {
                    match key_event.code {
                        KeyCode::Esc => {
                            sidebar_prompt = None;
                        }
                        KeyCode::Enter => {
                            let input = prompt.input.clone();
                            let kind = prompt.kind.clone();
                            sidebar_prompt = None;
                            handle_sidebar_prompt(engine, &mut sidebar, kind, input);
                        }
                        KeyCode::Backspace => {
                            prompt.input.pop();
                        }
                        KeyCode::Char(c)
                            if key_event.kind != KeyEventKind::Release
                                && !key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            // For delete confirm only accept y/n
                            if matches!(prompt.kind, PromptKind::DeleteConfirm(_)) {
                                if c == 'y' || c == 'n' {
                                    let kind = prompt.kind.clone();
                                    sidebar_prompt = None;
                                    if c == 'y' {
                                        handle_sidebar_prompt(
                                            engine,
                                            &mut sidebar,
                                            kind,
                                            "y".to_string(),
                                        );
                                    }
                                }
                            } else {
                                prompt.input.push(c);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── Sidebar focused ─────────────────────────────────────────
                if sidebar.has_focus && key_event.kind != KeyEventKind::Release {
                    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
                    match key_event.code {
                        // Return focus to editor
                        KeyCode::Esc => {
                            sidebar.has_focus = false;
                        }
                        KeyCode::Char('b') if ctrl => {
                            sidebar.has_focus = false;
                        }
                        // Navigate down
                        KeyCode::Char('j') | KeyCode::Down => {
                            if !sidebar.rows.is_empty() {
                                sidebar.selected =
                                    (sidebar.selected + 1).min(sidebar.rows.len() - 1);
                            }
                            if let Ok(size) = terminal.size() {
                                let h = size.height.saturating_sub(3) as usize; // header + status + cmd
                                sidebar.ensure_visible(h);
                            }
                        }
                        // Navigate up
                        KeyCode::Char('k') | KeyCode::Up => {
                            sidebar.selected = sidebar.selected.saturating_sub(1);
                            if let Ok(size) = terminal.size() {
                                let h = size.height.saturating_sub(3) as usize;
                                sidebar.ensure_visible(h);
                            }
                        }
                        // Expand dir / open file
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                            let idx = sidebar.selected;
                            if idx < sidebar.rows.len() {
                                if sidebar.rows[idx].is_dir {
                                    sidebar.toggle_dir(idx);
                                } else {
                                    let path = sidebar.rows[idx].path.clone();
                                    if let Err(e) =
                                        engine.open_file_with_mode(&path, OpenMode::Permanent)
                                    {
                                        engine.message = e;
                                    }
                                    sidebar.has_focus = false;
                                }
                            }
                        }
                        // Collapse dir / go to parent
                        KeyCode::Char('h') | KeyCode::Left => {
                            let idx = sidebar.selected;
                            if idx < sidebar.rows.len() {
                                if sidebar.rows[idx].is_dir && sidebar.rows[idx].is_expanded {
                                    // Collapse this dir
                                    sidebar.toggle_dir(idx);
                                } else {
                                    // Move to nearest parent row (lower depth)
                                    let target_depth = sidebar.rows[idx].depth;
                                    if target_depth > 0 {
                                        let parent_idx = sidebar.rows[..idx]
                                            .iter()
                                            .rposition(|r| r.depth < target_depth);
                                        if let Some(pi) = parent_idx {
                                            sidebar.selected = pi;
                                        }
                                    }
                                }
                            }
                        }
                        // New file prompt
                        KeyCode::Char('a') if !ctrl => {
                            sidebar_prompt = Some(SidebarPrompt {
                                kind: PromptKind::NewFile,
                                input: String::new(),
                            });
                        }
                        // New folder prompt
                        KeyCode::Char('A') if !ctrl => {
                            sidebar_prompt = Some(SidebarPrompt {
                                kind: PromptKind::NewFolder,
                                input: String::new(),
                            });
                        }
                        // Delete prompt
                        KeyCode::Char('D') if !ctrl => {
                            let idx = sidebar.selected;
                            if idx < sidebar.rows.len() {
                                let path = sidebar.rows[idx].path.clone();
                                sidebar_prompt = Some(SidebarPrompt {
                                    kind: PromptKind::DeleteConfirm(path),
                                    input: String::new(),
                                });
                            }
                        }
                        // Refresh
                        KeyCode::Char('R') if !ctrl => {
                            sidebar.build_rows();
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── Editor focused ──────────────────────────────────────────
                if let Some((key_name, unicode, ctrl)) = translate_key(key_event) {
                    // Ctrl-B: toggle sidebar visibility
                    if ctrl && key_name == "b" {
                        sidebar.visible = !sidebar.visible;
                        engine.session.explorer_visible = sidebar.visible;
                        let _ = engine.session.save();
                        continue;
                    }

                    // Ctrl-Shift-E: show sidebar and focus it
                    if key_event.kind != KeyEventKind::Release
                        && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && key_event.modifiers.contains(KeyModifiers::SHIFT)
                        && key_event.code == KeyCode::Char('e')
                    {
                        sidebar.visible = true;
                        sidebar.has_focus = true;
                        continue;
                    }

                    // Alt+Left/Right: resize sidebar
                    if key_event.modifiers.contains(KeyModifiers::ALT)
                        && key_event.kind != KeyEventKind::Release
                    {
                        match key_event.code {
                            KeyCode::Left => {
                                sidebar_width = sidebar_width.saturating_sub(1).max(15);
                                continue;
                            }
                            KeyCode::Right => {
                                sidebar_width = (sidebar_width + 1).min(60);
                                continue;
                            }
                            _ => {}
                        }
                    }

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
            Event::Mouse(mouse_event) => {
                sidebar_width = handle_mouse(
                    mouse_event,
                    &mut sidebar,
                    engine,
                    &terminal.size().ok(),
                    sidebar_width,
                    &mut dragging_sidebar,
                    last_layout.as_ref(),
                );
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

// ─── Mouse handling ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn handle_mouse(
    ev: MouseEvent,
    sidebar: &mut TuiSidebar,
    engine: &mut Engine,
    terminal_size: &Option<ratatui::layout::Rect>,
    sidebar_width: u16,
    dragging_sidebar: &mut bool,
    last_layout: Option<&render::ScreenLayout>,
) -> u16 {
    let col = ev.column;
    let row = ev.row;
    let term_height = terminal_size.map(|s| s.height).unwrap_or(24);

    // ── Separator drag (works anywhere, regardless of row) ────────────────────
    let sep_col = ACTIVITY_BAR_WIDTH + if sidebar.visible { sidebar_width } else { 0 };
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) if sidebar.visible && col == sep_col => {
            *dragging_sidebar = true;
            return sidebar_width;
        }
        MouseEventKind::Drag(MouseButton::Left) if *dragging_sidebar => {
            let new_w = col.saturating_sub(ACTIVITY_BAR_WIDTH);
            return new_w.clamp(15, 60);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            *dragging_sidebar = false;
            return sidebar_width;
        }
        // Scroll wheel in editor area
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let editor_left = ACTIVITY_BAR_WIDTH
                + if sidebar.visible {
                    sidebar_width + 1
                } else {
                    0
                };
            if col >= editor_left && row + 2 < term_height {
                let lines = engine.buffer().len_lines().saturating_sub(1);
                let st = engine.view().scroll_top;
                if matches!(ev.kind, MouseEventKind::ScrollUp) {
                    engine.set_scroll_top(st.saturating_sub(3));
                } else {
                    engine.set_scroll_top((st + 3).min(lines));
                }
                engine.ensure_cursor_visible();
            }
            return sidebar_width;
        }
        _ => {}
    }

    // Only process left-click presses from here on
    if ev.kind != MouseEventKind::Down(MouseButton::Left) {
        return sidebar_width;
    }

    // Bottom 2 rows are status + cmd — ignore
    if row + 2 >= term_height {
        return sidebar_width;
    }

    // ── Activity bar ──────────────────────────────────────────────────────────
    if col < ACTIVITY_BAR_WIDTH {
        match row {
            0 => {
                if sidebar.active_panel == TuiPanel::Explorer && sidebar.visible {
                    sidebar.visible = false;
                } else {
                    sidebar.active_panel = TuiPanel::Explorer;
                    sidebar.visible = true;
                }
                engine.session.explorer_visible = sidebar.visible;
                let _ = engine.session.save();
            }
            1 => {
                if sidebar.active_panel == TuiPanel::Settings && sidebar.visible {
                    sidebar.visible = false;
                } else {
                    sidebar.active_panel = TuiPanel::Settings;
                    sidebar.visible = true;
                }
                engine.session.explorer_visible = sidebar.visible;
                let _ = engine.session.save();
            }
            _ => {}
        }
        return sidebar_width;
    }

    // ── Sidebar panel area ────────────────────────────────────────────────────
    if sidebar.visible && col < ACTIVITY_BAR_WIDTH + sidebar_width {
        sidebar.has_focus = true;
        if sidebar.active_panel == TuiPanel::Explorer {
            if row == 0 {
                return sidebar_width; // header row
            }
            let tree_row = (row as usize).saturating_sub(1) + sidebar.scroll_top;
            if tree_row < sidebar.rows.len() {
                if sidebar.selected == tree_row {
                    if sidebar.rows[tree_row].is_dir {
                        sidebar.toggle_dir(tree_row);
                    } else {
                        let path = sidebar.rows[tree_row].path.clone();
                        if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                            engine.message = e;
                        }
                        sidebar.has_focus = false;
                    }
                } else {
                    sidebar.selected = tree_row;
                }
            }
        }
        return sidebar_width;
    }

    // ── Editor area ───────────────────────────────────────────────────────────
    sidebar.has_focus = false;
    let editor_left = ACTIVITY_BAR_WIDTH
        + if sidebar.visible {
            sidebar_width + 1
        } else {
            0
        };
    if col < editor_left {
        return sidebar_width; // separator column
    }

    let rel_col = col - editor_left;
    let editor_row = row.saturating_sub(1); // subtract tab bar row

    if let Some(layout) = last_layout {
        for rw in &layout.windows {
            let wx = rw.rect.x as u16;
            let wy = rw.rect.y as u16;
            let ww = rw.rect.width as u16;
            let wh = rw.rect.height as u16;

            if rel_col >= wx && rel_col < wx + ww && editor_row >= wy && editor_row < wy + wh {
                let viewport_lines = wh as usize;
                let has_scrollbar = rw.total_lines > viewport_lines;

                // Scrollbar click (rightmost column, if scrollbar is shown)
                if has_scrollbar && rel_col == wx + ww - 1 {
                    let ratio = (editor_row - wy) as f64 / wh as f64;
                    let new_top = (ratio * rw.total_lines as f64) as usize;
                    engine.set_cursor_for_window(rw.window_id, new_top, 0);
                    engine.ensure_cursor_visible();
                    return sidebar_width;
                }

                // Check gutter area
                let gutter = rw.gutter_char_width as u16;
                let view_row = (editor_row - wy) as usize;
                if gutter > 0 && rel_col >= wx && rel_col < wx + gutter {
                    // Any click in gutter toggles fold if there's a fold indicator
                    if let Some(rl) = rw.lines.get(view_row) {
                        let has_fold_indicator =
                            rl.gutter_text.chars().any(|c| c == '+' || c == '-');
                        if has_fold_indicator {
                            engine.toggle_fold_at_line(rl.line_idx);
                        }
                    }
                    return sidebar_width;
                }
                // Text area click — fold-aware row → buffer line mapping
                let buf_line = rw
                    .lines
                    .get(view_row)
                    .map(|l| l.line_idx)
                    .unwrap_or_else(|| rw.scroll_top + view_row);
                let col_in_text = (rel_col - wx - gutter) as usize + rw.scroll_left;
                engine.set_cursor_for_window(rw.window_id, buf_line, col_in_text);
                return sidebar_width;
            }
        }
    }

    sidebar_width
}

// ─── Screen layout bridging ───────────────────────────────────────────────────

fn build_screen_for_tui(
    engine: &Engine,
    theme: &Theme,
    area: Rect,
    sidebar: &TuiSidebar,
    sidebar_width: u16,
) -> render::ScreenLayout {
    // Global bottom rows: status(1) + cmd(1); editor column top: tab(1)
    let content_rows = area.height.saturating_sub(3); // tab + status + cmd
    let sidebar_cols = if sidebar.visible {
        sidebar_width + 1
    } else {
        0
    }; // +1 sep
    let content_cols = area.width.saturating_sub(ACTIVITY_BAR_WIDTH + sidebar_cols);
    let content_bounds = WindowRect::new(0.0, 0.0, content_cols as f64, content_rows as f64);
    let window_rects = engine.calculate_window_rects(content_bounds);
    build_screen_layout(engine, theme, &window_rects, 1.0, 1.0)
}

// ─── Frame rendering ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_frame(
    frame: &mut ratatui::Frame,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar: &TuiSidebar,
    engine: &Engine,
    sidebar_prompt: &Option<SidebarPrompt>,
    sidebar_width: u16,
) {
    let area = frame.size();

    // ── Global vertical split: [main_area] / [status(1)] / [cmd(1)] ──────────
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
    let main_area = v_chunks[0];
    let status_area = v_chunks[1];
    let cmd_area = v_chunks[2];

    // ── Horizontal split of main_area: [activity_bar] [sidebar?] [editor_col] ─
    let sidebar_constraint = if sidebar.visible {
        Constraint::Length(sidebar_width + 1) // +1 for separator
    } else {
        Constraint::Length(0)
    };
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(ACTIVITY_BAR_WIDTH),
            sidebar_constraint,
            Constraint::Min(0),
        ])
        .split(main_area);
    let activity_area = h_chunks[0];
    let sidebar_sep_area = h_chunks[1];
    let editor_col = h_chunks[2];

    // ── Editor column: [tab_bar(1)] / [editor_windows] ───────────────────────
    let ec_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(editor_col);
    let tab_area = ec_chunks[0];
    let editor_area = ec_chunks[1];

    // ── Render activity bar ───────────────────────────────────────────────────
    render_activity_bar(frame.buffer_mut(), activity_area, sidebar, theme);

    // ── Render sidebar + separator ────────────────────────────────────────────
    if sidebar.visible && sidebar_sep_area.width > 1 {
        let sidebar_area = Rect {
            x: sidebar_sep_area.x,
            y: sidebar_sep_area.y,
            width: sidebar_sep_area.width - 1,
            height: sidebar_sep_area.height,
        };
        let sep_x = sidebar_sep_area.x + sidebar_sep_area.width - 1;

        render_sidebar(frame.buffer_mut(), sidebar_area, sidebar, engine, theme);

        // Separator column
        let sep_fg = rc(theme.separator);
        let sep_bg = rc(theme.background);
        for y in sidebar_sep_area.y..sidebar_sep_area.y + sidebar_sep_area.height {
            set_cell(frame.buffer_mut(), sep_x, y, '│', sep_fg, sep_bg);
        }
    }

    // ── Render editor ─────────────────────────────────────────────────────────
    render_tab_bar(frame.buffer_mut(), tab_area, &screen.tab_bar, theme);
    render_all_windows(frame, editor_area, &screen.windows, theme);

    // ── Status / command ──────────────────────────────────────────────────────
    render_status_line(
        frame.buffer_mut(),
        status_area,
        &screen.status_left,
        &screen.status_right,
        theme,
    );

    if let Some(prompt) = sidebar_prompt {
        let prompt_text = match &prompt.kind {
            PromptKind::NewFile => format!("New file: {}", prompt.input),
            PromptKind::NewFolder => format!("New folder: {}", prompt.input),
            PromptKind::DeleteConfirm(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                format!("Delete '{}'? (y/n)", name)
            }
        };
        render_prompt_line(frame.buffer_mut(), cmd_area, &prompt_text, theme);
    } else {
        render_command_line(frame.buffer_mut(), cmd_area, &screen.command, theme);
    }
}

// ─── Sidebar CRUD handling ────────────────────────────────────────────────────

fn handle_sidebar_prompt(
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    kind: PromptKind,
    input: String,
) {
    let cwd = sidebar.root.clone();
    match kind {
        PromptKind::NewFile => {
            let name = input.trim();
            if !name.is_empty() {
                let path = cwd.join(name);
                if let Err(e) = fs::write(&path, "") {
                    engine.message = format!("Error creating file: {}", e);
                } else {
                    sidebar.build_rows();
                    if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                        engine.message = e;
                    }
                }
            }
        }
        PromptKind::NewFolder => {
            let name = input.trim();
            if !name.is_empty() {
                let path = cwd.join(name);
                if let Err(e) = fs::create_dir_all(&path) {
                    engine.message = format!("Error creating folder: {}", e);
                } else {
                    sidebar.build_rows();
                }
            }
        }
        PromptKind::DeleteConfirm(path) => {
            if input == "y" {
                let result = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                if let Err(e) = result {
                    engine.message = format!("Error deleting: {}", e);
                } else {
                    sidebar.build_rows();
                }
            }
        }
    }
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
    let viewport_lines = area.height as usize;
    let has_scrollbar = window.total_lines > viewport_lines && area.width > gutter_w + 1;

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
            let line_num_fg = rc(if line.is_current_line {
                theme.line_number_active_fg
            } else {
                theme.line_number_fg
            });
            for (i, ch) in line.gutter_text.chars().enumerate() {
                let gx = area.x + i as u16;
                if gx >= area.x + gutter_w {
                    break;
                }
                let fg = if window.has_git_diff && i == 0 {
                    rc(match line.git_diff {
                        Some(GitLineStatus::Added) => theme.git_added,
                        Some(GitLineStatus::Modified) => theme.git_modified,
                        None => theme.line_number_fg,
                    })
                } else {
                    line_num_fg
                };
                set_cell(frame.buffer_mut(), gx, screen_y, ch, fg, window_bg);
            }
        }

        // Text (narrowed by 1 when scrollbar is shown)
        let text_area_x = area.x + gutter_w;
        let text_width = area
            .width
            .saturating_sub(gutter_w)
            .saturating_sub(if has_scrollbar { 1 } else { 0 });
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

    // Scrollbar
    if has_scrollbar {
        render_scrollbar(
            frame.buffer_mut(),
            area,
            window.scroll_top,
            window.total_lines,
            viewport_lines,
            theme,
        );
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
                    let old_fg = cell.fg;
                    let old_bg = cell.bg;
                    cell.set_fg(old_bg).set_bg(old_fg);
                }
            }
            CursorShape::Bar | CursorShape::Underline => {
                frame.set_cursor(cursor_screen_x, cursor_screen_y);
            }
        }
    }
}

fn render_scrollbar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    scroll_top: usize,
    total_lines: usize,
    viewport_lines: usize,
    theme: &Theme,
) {
    if area.height == 0 || total_lines == 0 {
        return;
    }
    let track_fg = rc(theme.separator);
    let thumb_fg = rc(theme.status_fg);
    let sb_bg = rc(theme.background);
    let h = area.height as f64;
    let thumb_size = ((viewport_lines as f64 / total_lines as f64) * h)
        .ceil()
        .max(1.0) as u16;
    let thumb_top = ((scroll_top as f64 / total_lines as f64) * h).floor() as u16;
    let sb_x = area.x + area.width - 1;
    for dy in 0..area.height {
        let y = area.y + dy;
        let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
        let ch = if in_thumb { '█' } else { '░' };
        let fg = if in_thumb { thumb_fg } else { track_fg };
        set_cell(buf, sb_x, y, ch, fg, sb_bg);
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

// ─── Activity bar ─────────────────────────────────────────────────────────────

fn render_activity_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &TuiSidebar,
    theme: &Theme,
) {
    let bar_bg = rc(theme.tab_bar_bg);
    let active_bg = rc(theme.status_bg);
    let icon_fg = rc(theme.status_fg);
    let inactive_fg = rc(theme.line_number_fg);

    // Fill entire activity bar background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', inactive_fg, bar_bg);
        }
    }

    // Panel buttons: (row offset, panel, icon char)
    let buttons: &[(u16, TuiPanel, char)] = &[
        (0, TuiPanel::Explorer, '\u{f07c}'), // nf-fa-folder_open
        (1, TuiPanel::Settings, '\u{f013}'), // nf-fa-cog
    ];

    for &(row_off, panel, icon) in buttons {
        let y = area.y + row_off;
        if y >= area.y + area.height {
            break;
        }
        let is_active = sidebar.visible && sidebar.active_panel == panel;
        let (fg, bg) = if is_active {
            (icon_fg, active_bg)
        } else {
            (inactive_fg, bar_bg)
        };
        // Fill the full row for this button
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }
        // Icon centred in the 3-char width: col 1
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, icon, fg, bg);
        }
    }
}

// ─── Sidebar rendering ────────────────────────────────────────────────────────

fn render_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &TuiSidebar,
    engine: &Engine,
    theme: &Theme,
) {
    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let default_fg = rc(theme.foreground);
    let row_bg = rc(theme.tab_bar_bg);
    let active_file_fg = rc(theme.keyword);
    let sel_fg = row_bg;
    let sel_bg = default_fg;

    // Settings panel
    if sidebar.active_panel == TuiPanel::Settings {
        render_settings_panel(buf, area, theme);
        return;
    }

    // Collect open buffer paths for highlighting active files
    let open_paths: Vec<PathBuf> = engine
        .buffer_manager
        .list()
        .into_iter()
        .filter_map(|id| {
            engine
                .buffer_manager
                .get(id)
                .and_then(|s| s.file_path.as_ref())
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        })
        .collect();

    // ── Header row ──────────────────────────────────────────────────────
    if area.height == 0 {
        return;
    }
    let header_y = area.y;
    // Fill header
    for x in area.x..area.x + area.width {
        set_cell(buf, x, header_y, ' ', header_fg, header_bg);
    }
    // " EXPLORER" label
    let label = " EXPLORER";
    let mut x = area.x;
    for ch in label.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, header_y, ch, header_fg, header_bg);
        x += 1;
    }
    // Toolbar icons on the right: new file, new folder, delete, refresh
    let toolbar: &[&str] = &["\u{f15b}", "\u{f07b}", "\u{f1f8}", "\u{f021}"];
    let toolbar_str: String = toolbar.iter().fold(String::new(), |mut acc, s| {
        acc.push('[');
        acc.push_str(s);
        acc.push(']');
        acc
    });
    let toolbar_chars: Vec<char> = toolbar_str.chars().collect();
    let toolbar_len = toolbar_chars.len() as u16;
    if toolbar_len < area.width {
        let mut tx = area.x + area.width - toolbar_len;
        for &ch in &toolbar_chars {
            set_cell(buf, tx, header_y, ch, header_fg, header_bg);
            tx += 1;
        }
    }

    // ── Tree rows ────────────────────────────────────────────────────────
    let tree_height = area.height.saturating_sub(1) as usize;
    let visible_rows = sidebar
        .rows
        .iter()
        .enumerate()
        .skip(sidebar.scroll_top)
        .take(tree_height);

    for (i, (row_idx, row)) in visible_rows.enumerate() {
        let screen_y = area.y + 1 + i as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Fill row background
        for x in area.x..area.x + area.width {
            set_cell(buf, x, screen_y, ' ', default_fg, row_bg);
        }

        // Determine colours
        let is_selected = row_idx == sidebar.selected;
        let canonical_path = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());
        let is_active = open_paths.contains(&canonical_path);

        let (fg, bg) = if is_selected {
            (sel_fg, sel_bg)
        } else if is_active {
            (active_file_fg, row_bg)
        } else {
            (default_fg, row_bg)
        };

        // Build row string: indent + chevron/icon + name
        let indent = "  ".repeat(row.depth);
        let prefix = if row.is_dir {
            if row.is_expanded {
                "\u{25be} " // ▾
            } else {
                "\u{25b8} " // ▸
            }
        } else {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            // We format as "  {icon} " — two spaces, icon, space
            // Rendered char-by-char below
            let _ = ext; // used in the render step
            "  "
        };

        let mut x = area.x;
        // Indent
        for ch in indent.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, fg, bg);
            x += 1;
        }
        // Prefix (chevron or spaces)
        for ch in prefix.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, fg, bg);
            x += 1;
        }
        // File icon (only for files)
        if !row.is_dir {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let icon = crate::icons::file_icon(ext);
            for ch in icon.chars() {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, screen_y, ch, fg, bg);
                x += 1;
            }
            // Space after icon
            if x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', fg, bg);
                x += 1;
            }
        }
        // Name
        for ch in row.name.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, fg, bg);
            x += 1;
        }
    }
}

/// Render the settings panel (placeholder — settings are file-based).
fn render_settings_panel(buf: &mut ratatui::buffer::Buffer, area: Rect, theme: &Theme) {
    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let fg = rc(theme.foreground);
    let bg = rc(theme.tab_bar_bg);
    let dim_fg = rc(theme.line_number_fg);

    if area.height == 0 {
        return;
    }

    // Fill background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }
    }

    // Header
    let header_y = area.y;
    for x in area.x..area.x + area.width {
        set_cell(buf, x, header_y, ' ', header_fg, header_bg);
    }
    let mut x = area.x;
    for ch in " SETTINGS".chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, header_y, ch, header_fg, header_bg);
        x += 1;
    }

    // Content lines
    let lines: &[&str] = &[
        "",
        " Edit settings file:",
        " ~/.config/vimcode/settings.json",
        "",
        " Reload: :config reload",
        "",
        " Available settings:",
        "  line_numbers: none|abs|rel|hybrid",
        "  font_family: string",
        "  font_size: number",
        "  incremental_search: bool",
        "  explorer_visible_on_startup: bool",
    ];
    for (i, line) in lines.iter().enumerate() {
        let y = area.y + 1 + i as u16;
        if y >= area.y + area.height {
            break;
        }
        let mut x = area.x;
        for ch in line.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, y, ch, dim_fg, bg);
            x += 1;
        }
    }
}

/// Render a one-line prompt in the command area (used for sidebar CRUD input).
fn render_prompt_line(buf: &mut ratatui::buffer::Buffer, area: Rect, text: &str, theme: &Theme) {
    let fg = rc(theme.command_fg);
    let bg = rc(theme.command_bg);
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }
    let mut x = area.x;
    for ch in text.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, area.y, ch, fg, bg);
        x += 1;
    }
    // Show cursor at end of text
    if x < area.x + area.width {
        let cell = buf.get_mut(x, area.y);
        let old_fg = cell.fg;
        let old_bg = cell.bg;
        cell.set_fg(old_bg).set_bg(old_fg);
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
            let lower = c.to_ascii_lowercase();
            let (key_name, unicode) = if ctrl {
                // Engine dispatches Ctrl combos via key_name (e.g. "d" for Ctrl-D)
                (lower.to_string(), Some(lower))
            } else {
                ("".to_string(), Some(c))
            };
            Some((key_name, unicode, ctrl))
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
    engine.collect_session_open_files();
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
