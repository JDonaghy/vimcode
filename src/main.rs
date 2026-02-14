use gtk4::cairo::Context;
use gtk4::gdk;
use gtk4::pango::{self, AttrColor, AttrList, FontDescription};
use gtk4::prelude::*;
use pangocairo::functions as pangocairo;
use relm4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

mod core;
use core::buffer::Buffer;
use core::engine::EngineAction;
use core::{Cursor, Engine, Mode, WindowRect};

struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
}

#[derive(Debug)]
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

            gtk4::Box {
                set_orientation: gtk4::Orientation::Vertical,

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
                            sender.input(Msg::KeyPress { key_name, unicode, ctrl });
                            gtk4::glib::Propagation::Stop
                        }
                    },

                    #[watch]
                    set_css_classes: {
                        drawing_area.queue_draw();
                        if model.redraw { &["vim-code", "even"] } else { &["vim-code", "odd"] }
                    },
                }
            }
        }
    }

    fn init(
        file_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let engine = match file_path {
            Some(ref path) => Engine::open(path),
            None => Engine::new(),
        };

        // Set window title based on file
        let title = match engine.file_path() {
            Some(p) => format!("VimCode - {}", p.display()),
            None => "VimCode - [No Name]".to_string(),
        };

        let engine = Rc::new(RefCell::new(engine));

        let model = App {
            engine: engine.clone(),
            redraw: false,
        };
        let widgets = view_output!();

        // Set the actual title after widget creation
        root.set_title(Some(&title));

        // Track resize to update viewport_lines
        let sender_clone = sender.clone();
        let engine_for_resize = engine.clone();
        widgets.drawing_area.connect_resize(move |_, _, height| {
            let line_height_approx = 24.0_f64;
            let total_lines = (height as f64 / line_height_approx).floor() as usize;
            let viewport = total_lines.saturating_sub(2);
            {
                let mut e = engine_for_resize.borrow_mut();
                e.set_viewport_lines(viewport.max(1));
            }
            sender_clone.input(Msg::Resize);
        });

        let engine_clone = engine.clone();
        widgets
            .drawing_area
            .set_draw_func(move |_, cr, width, height| {
                let engine = engine_clone.borrow();
                draw_editor(cr, &engine, width, height);
            });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
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
                        std::process::exit(0);
                    }
                    EngineAction::OpenFile(path) => {
                        let mut engine = self.engine.borrow_mut();
                        // Use buffer manager to open the file in current window
                        match engine.buffer_manager.open_file(&path) {
                            Ok(buffer_id) => {
                                // Switch current window to the new buffer
                                let current = engine.active_buffer_id();
                                engine.buffer_manager.alternate_buffer = Some(current);
                                engine.active_window_mut().buffer_id = buffer_id;
                                engine.view_mut().cursor.line = 0;
                                engine.view_mut().cursor.col = 0;
                                engine.set_scroll_top(0);
                                engine.message = format!("\"{}\"", path.display());
                            }
                            Err(e) => {
                                engine.message = format!("Error: {}", e);
                            }
                        }
                    }
                    EngineAction::None | EngineAction::Error => {}
                }

                self.redraw = !self.redraw;
            }
            Msg::Resize => {
                self.redraw = !self.redraw;
            }
        }
    }
}

fn draw_editor(cr: &Context, engine: &Engine, width: i32, height: i32) {
    // 1. Background
    cr.set_source_rgb(0.1, 0.1, 0.1);
    cr.paint().expect("Invalid cairo surface");

    // 2. Setup Pango
    let pango_ctx = pangocairo::create_context(cr);
    let layout = pango::Layout::new(&pango_ctx);
    let font_desc = FontDescription::from_string("Monospace 14");
    layout.set_font_description(Some(&font_desc));

    // Derive line height from font metrics
    let font_metrics = pango_ctx.metrics(Some(&font_desc), None);
    let line_height = (font_metrics.ascent() + font_metrics.descent()) as f64 / pango::SCALE as f64;

    // Calculate layout regions
    let tab_bar_height = if engine.tabs.len() > 1 {
        line_height
    } else {
        0.0
    };
    let status_bar_height = line_height * 2.0; // status + command line

    // Calculate window rects for the current tab
    let content_bounds = WindowRect::new(
        0.0,
        tab_bar_height,
        width as f64,
        height as f64 - tab_bar_height - status_bar_height,
    );
    let window_rects = engine.calculate_window_rects(content_bounds);

    // 3. Draw tab bar if multiple tabs
    if engine.tabs.len() > 1 {
        draw_tab_bar(cr, &layout, engine, width as f64, line_height);
    }

    // 4. Draw each window
    for (window_id, rect) in &window_rects {
        let is_active = *window_id == engine.active_window_id();
        draw_window(
            cr,
            &layout,
            &font_metrics,
            engine,
            *window_id,
            rect,
            line_height,
            is_active,
        );
    }

    // 5. Draw window separators
    draw_window_separators(cr, &window_rects);

    // 6. Status Line (second-to-last line)
    let status_y = height as f64 - status_bar_height;
    draw_status_line(cr, &layout, engine, width as f64, status_y, line_height);

    // 7. Command Line (last line)
    let cmd_y = status_y + line_height;
    draw_command_line(cr, &layout, engine, width as f64, cmd_y, line_height);
}

fn draw_tab_bar(
    cr: &Context,
    layout: &pango::Layout,
    engine: &Engine,
    width: f64,
    line_height: f64,
) {
    // Tab bar background
    cr.set_source_rgb(0.15, 0.15, 0.2);
    cr.rectangle(0.0, 0.0, width, line_height);
    cr.fill().unwrap();

    let mut x = 0.0;
    for (i, tab) in engine.tabs.iter().enumerate() {
        let is_active = i == engine.active_tab;

        // Get first buffer name in this tab
        let window_id = tab.active_window;
        let name = if let Some(window) = engine.windows.get(&window_id) {
            if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                let dirty = if state.dirty { "*" } else { "" };
                format!(" {}: {}{} ", i + 1, state.display_name(), dirty)
            } else {
                format!(" {}: [No Name] ", i + 1)
            }
        } else {
            format!(" {}: [No Name] ", i + 1)
        };

        layout.set_text(&name);
        let (tab_width, _) = layout.pixel_size();

        // Tab background
        if is_active {
            cr.set_source_rgb(0.25, 0.25, 0.35);
        } else {
            cr.set_source_rgb(0.15, 0.15, 0.2);
        }
        cr.rectangle(x, 0.0, tab_width as f64, line_height);
        cr.fill().unwrap();

        // Tab text
        cr.move_to(x, 0.0);
        if is_active {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        } else {
            cr.set_source_rgb(0.7, 0.7, 0.7);
        }
        pangocairo::show_layout(cr, layout);

        x += tab_width as f64 + 2.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_window(
    cr: &Context,
    layout: &pango::Layout,
    font_metrics: &pango::FontMetrics,
    engine: &Engine,
    window_id: core::WindowId,
    rect: &WindowRect,
    line_height: f64,
    is_active: bool,
) {
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

    // Calculate visible area (leave room for per-window status bar)
    let per_window_status = if engine.windows.len() > 1 {
        line_height
    } else {
        0.0
    };
    let text_area_height = rect.height - per_window_status;
    let visible_lines = (text_area_height / line_height).floor() as usize;

    // Window background (slightly different for active)
    if is_active && engine.windows.len() > 1 {
        cr.set_source_rgb(0.12, 0.12, 0.12);
    } else {
        cr.set_source_rgb(0.1, 0.1, 0.1);
    }
    cr.rectangle(rect.x, rect.y, rect.width, rect.height);
    cr.fill().unwrap();

    // Render visual selection highlight (if in visual mode and this is active window)
    if is_active {
        match engine.mode {
            Mode::Visual | Mode::VisualLine => {
                if let Some(anchor) = engine.visual_anchor {
                    draw_visual_selection(
                        cr,
                        layout,
                        engine,
                        buffer,
                        &anchor,
                        &view.cursor,
                        rect,
                        line_height,
                        view.scroll_top,
                        visible_lines,
                    );
                }
            }
            _ => {}
        }
    }

    // Render text with highlights
    let scroll_top = view.scroll_top;
    let total_lines = buffer.content.len_lines();

    for view_idx in 0..visible_lines {
        let line_idx = scroll_top + view_idx;
        if line_idx >= total_lines {
            break;
        }

        let line = buffer.content.line(line_idx);
        let y = rect.y + view_idx as f64 * line_height;
        layout.set_text(&line.to_string());

        let line_start_byte = buffer.content.line_to_byte(line_idx);
        let line_end_byte = line_start_byte + line.len_bytes();

        let attrs = AttrList::new();

        for (start, end, scope) in &buffer_state.highlights {
            if *end <= line_start_byte || *start >= line_end_byte {
                continue;
            }

            let rel_start = if *start < line_start_byte {
                0
            } else {
                *start - line_start_byte
            };
            let rel_end = if *end > line_end_byte {
                line.len_bytes()
            } else {
                *end - line_start_byte
            };

            let color_hex = match scope.as_str() {
                "keyword" | "operator" => "#c678dd",
                "string" => "#98c379",
                "comment" => "#5c6370",
                "function" | "method" => "#61afef",
                "type" | "class" | "struct" => "#e5c07b",
                "variable" => "#e06c75",
                _ => "#abb2bf",
            };

            if let Ok(pango_color) = pango::Color::parse(color_hex) {
                let mut attr = AttrColor::new_foreground(
                    pango_color.red(),
                    pango_color.green(),
                    pango_color.blue(),
                );
                attr.set_start_index(rel_start as u32);
                attr.set_end_index(rel_end as u32);
                attrs.insert(attr);
            }
        }
        layout.set_attributes(Some(&attrs));

        cr.move_to(rect.x, y);
        cr.set_source_rgb(0.9, 0.9, 0.9);
        pangocairo::show_layout(cr, layout);
    }

    // Render cursor (only in active window)
    if is_active && view.cursor.line >= scroll_top && view.cursor.line < scroll_top + visible_lines
    {
        if let Some(line) = buffer.content.lines().nth(view.cursor.line) {
            let line_text = line.to_string();
            layout.set_text(&line_text);
            layout.set_attributes(None);

            let byte_offset: usize = line_text
                .char_indices()
                .nth(view.cursor.col)
                .map(|(i, _)| i)
                .unwrap_or(line_text.len());

            let pos = layout.index_to_pos(byte_offset as i32);
            let cursor_x = rect.x + pos.x() as f64 / pango::SCALE as f64;
            let char_w = pos.width() as f64 / pango::SCALE as f64;
            let cursor_y = rect.y + (view.cursor.line - scroll_top) as f64 * line_height;

            match engine.mode {
                Mode::Normal | Mode::Visual | Mode::VisualLine => {
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.5);
                    let w = if char_w > 0.0 {
                        char_w
                    } else {
                        font_metrics.approximate_char_width() as f64 / pango::SCALE as f64
                    };
                    cr.rectangle(cursor_x, cursor_y, w, line_height);
                }
                Mode::Insert => {
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    cr.rectangle(cursor_x, cursor_y, 2.0, line_height);
                }
                Mode::Command | Mode::Search => {
                    // No text cursor shown — cursor is in the command line
                }
            }
            cr.fill().unwrap();
        }
    }

    // Per-window status bar (only if multiple windows)
    if engine.windows.len() > 1 {
        let status_y = rect.y + rect.height - line_height;

        // Status bar background (different for active)
        if is_active {
            cr.set_source_rgb(0.25, 0.25, 0.35);
        } else {
            cr.set_source_rgb(0.18, 0.18, 0.25);
        }
        cr.rectangle(rect.x, status_y, rect.width, line_height);
        cr.fill().unwrap();

        let filename = buffer_state.display_name();
        let dirty_indicator = if buffer_state.dirty { " [+]" } else { "" };

        let status_text = format!(" {}{}", filename, dirty_indicator);
        layout.set_text(&status_text);
        layout.set_attributes(None);

        cr.move_to(rect.x, status_y);
        cr.set_source_rgb(0.9, 0.9, 0.9);
        pangocairo::show_layout(cr, layout);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_visual_selection(
    cr: &Context,
    layout: &pango::Layout,
    engine: &Engine,
    buffer: &Buffer,
    anchor: &Cursor,
    cursor: &Cursor,
    rect: &WindowRect,
    line_height: f64,
    scroll_top: usize,
    visible_lines: usize,
) {
    // Normalize selection (start <= end)
    let (start, end) =
        if anchor.line < cursor.line || (anchor.line == cursor.line && anchor.col <= cursor.col) {
            (*anchor, *cursor)
        } else {
            (*cursor, *anchor)
        };

    // Set highlight color (semi-transparent blue)
    cr.set_source_rgba(0.3, 0.5, 0.7, 0.3);

    match engine.mode {
        Mode::VisualLine => {
            // Line mode: highlight full lines
            for line_idx in start.line..=end.line {
                // Only draw if line is visible
                if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                    let view_idx = line_idx - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;
                    cr.rectangle(rect.x, y, rect.width, line_height);
                }
            }
            cr.fill().unwrap();
        }
        Mode::Visual => {
            // Character mode: highlight from start to end (inclusive)
            if start.line == end.line {
                // Single-line selection
                if start.line >= scroll_top && start.line < scroll_top + visible_lines {
                    let view_idx = start.line - scroll_top;
                    let y = rect.y + view_idx as f64 * line_height;

                    if let Some(line) = buffer.content.lines().nth(start.line) {
                        let line_text = line.to_string();
                        layout.set_text(&line_text);
                        layout.set_attributes(None);

                        // Calculate x position for start column
                        let start_byte: usize = line_text
                            .char_indices()
                            .nth(start.col)
                            .map(|(i, _)| i)
                            .unwrap_or(line_text.len());
                        let start_pos = layout.index_to_pos(start_byte as i32);
                        let start_x = rect.x + start_pos.x() as f64 / pango::SCALE as f64;

                        // Calculate x position for end column (inclusive, so +1)
                        let end_col = (end.col + 1).min(line_text.chars().count());
                        let end_byte: usize = line_text
                            .char_indices()
                            .nth(end_col)
                            .map(|(i, _)| i)
                            .unwrap_or(line_text.len());
                        let end_pos = layout.index_to_pos(end_byte as i32);
                        let end_x = rect.x + end_pos.x() as f64 / pango::SCALE as f64;

                        cr.rectangle(start_x, y, end_x - start_x, line_height);
                        cr.fill().unwrap();
                    }
                }
            } else {
                // Multi-line selection
                for line_idx in start.line..=end.line {
                    if line_idx >= scroll_top && line_idx < scroll_top + visible_lines {
                        let view_idx = line_idx - scroll_top;
                        let y = rect.y + view_idx as f64 * line_height;

                        if let Some(line) = buffer.content.lines().nth(line_idx) {
                            let line_text = line.to_string();
                            layout.set_text(&line_text);
                            layout.set_attributes(None);

                            if line_idx == start.line {
                                // First line: from start.col to end of line
                                let start_byte: usize = line_text
                                    .char_indices()
                                    .nth(start.col)
                                    .map(|(i, _)| i)
                                    .unwrap_or(line_text.len());
                                let start_pos = layout.index_to_pos(start_byte as i32);
                                let start_x = rect.x + start_pos.x() as f64 / pango::SCALE as f64;

                                let (line_width, _) = layout.pixel_size();
                                cr.rectangle(
                                    start_x,
                                    y,
                                    rect.x + line_width as f64 - start_x,
                                    line_height,
                                );
                                cr.fill().unwrap();
                            } else if line_idx == end.line {
                                // Last line: from start of line to end.col (inclusive)
                                let end_col = (end.col + 1).min(line_text.chars().count());
                                let end_byte: usize = line_text
                                    .char_indices()
                                    .nth(end_col)
                                    .map(|(i, _)| i)
                                    .unwrap_or(line_text.len());
                                let end_pos = layout.index_to_pos(end_byte as i32);
                                let end_x = rect.x + end_pos.x() as f64 / pango::SCALE as f64;

                                cr.rectangle(rect.x, y, end_x - rect.x, line_height);
                                cr.fill().unwrap();
                            } else {
                                // Middle lines: full line
                                let (line_width, _) = layout.pixel_size();
                                cr.rectangle(rect.x, y, line_width as f64, line_height);
                                cr.fill().unwrap();
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn draw_window_separators(cr: &Context, window_rects: &[(core::WindowId, WindowRect)]) {
    if window_rects.len() <= 1 {
        return;
    }

    cr.set_source_rgb(0.3, 0.3, 0.4);
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

fn draw_status_line(
    cr: &Context,
    layout: &pango::Layout,
    engine: &Engine,
    width: f64,
    y: f64,
    line_height: f64,
) {
    // Status bar background
    cr.set_source_rgb(0.2, 0.2, 0.3);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().unwrap();

    let mode_str = match engine.mode {
        Mode::Normal | Mode::Command | Mode::Search => "NORMAL",
        Mode::Insert => "INSERT",
        Mode::Visual => "VISUAL",
        Mode::VisualLine => "VISUAL LINE",
    };

    let filename = match engine.file_path() {
        Some(p) => p.display().to_string(),
        None => "[No Name]".to_string(),
    };

    let dirty_indicator = if engine.dirty() { " [+]" } else { "" };

    let left_status = format!(" -- {} -- {}{}", mode_str, filename, dirty_indicator);
    let cursor = engine.cursor();
    let right_status = format!(
        "Ln {}, Col {}  ({} lines) ",
        cursor.line + 1,
        cursor.col + 1,
        engine.buffer().len_lines()
    );

    layout.set_attributes(None);

    // Left side
    layout.set_text(&left_status);
    cr.move_to(0.0, y);
    cr.set_source_rgb(0.9, 0.9, 0.9);
    pangocairo::show_layout(cr, layout);

    // Right side
    layout.set_text(&right_status);
    let (right_w, _) = layout.pixel_size();
    cr.move_to(width - right_w as f64, y);
    pangocairo::show_layout(cr, layout);
}

fn draw_command_line(
    cr: &Context,
    layout: &pango::Layout,
    engine: &Engine,
    width: f64,
    y: f64,
    line_height: f64,
) {
    // Command line background
    cr.set_source_rgb(0.1, 0.1, 0.1);
    cr.rectangle(0.0, y, width, line_height);
    cr.fill().unwrap();

    let cmd_text = match engine.mode {
        Mode::Command => format!(":{}", engine.command_buffer),
        Mode::Search => format!("/{}", engine.command_buffer),
        Mode::Normal | Mode::Visual | Mode::VisualLine => {
            // Display count if present, otherwise show message
            if let Some(count) = engine.peek_count() {
                count.to_string()
            } else {
                engine.message.clone()
            }
        }
        _ => engine.message.clone(),
    };

    if !cmd_text.is_empty() {
        layout.set_text(&cmd_text);

        // Right-align count in Normal/Visual modes
        if (engine.mode == Mode::Normal
            || engine.mode == Mode::Visual
            || engine.mode == Mode::VisualLine)
            && engine.peek_count().is_some()
        {
            let (text_w, _) = layout.pixel_size();
            cr.move_to(width - text_w as f64, y);
        } else {
            cr.move_to(0.0, y);
        }

        cr.set_source_rgb(0.9, 0.9, 0.9);
        pangocairo::show_layout(cr, layout);
    }

    // Command-line cursor in Command/Search mode
    if engine.mode == Mode::Command || engine.mode == Mode::Search {
        let prefix = if engine.mode == Mode::Command {
            ":"
        } else {
            "/"
        };
        let full = format!("{}{}", prefix, engine.command_buffer);
        layout.set_text(&full);
        let (text_w, _) = layout.pixel_size();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.rectangle(text_w as f64, y, 2.0, line_height);
        cr.fill().unwrap();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file_path = args.get(1).map(PathBuf::from);

    // NON_UNIQUE prevents GTK from trying to pass files to an existing instance.
    // HANDLES_COMMAND_LINE lets us handle args ourselves instead of GTK treating
    // positional args as files to open.
    let gtk_app = gtk4::Application::builder()
        .application_id("org.vimcode.editor")
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
