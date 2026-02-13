use gtk4::cairo::Context;
use gtk4::pango::{self, AttrColor, AttrList, FontDescription};
use gtk4::prelude::*;
use pangocairo::functions as pangocairo;
use relm4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

mod core;
use core::{Engine, Mode};

struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
}

#[derive(Debug)]
enum Msg {
    KeyPress(gtk4::gdk::Key),
}

#[relm4::component]
impl SimpleComponent for App {
    type Init = ();
    type Input = Msg;
    type Output = ();

    view! {
        gtk4::Window {
            set_title: Some("VimCode - Phase 6: Syntax Highlighting"),
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
                        connect_key_pressed[sender] => move |_, key, _, _| {
                            sender.input(Msg::KeyPress(key));
                            gtk4::glib::Propagation::Stop
                        }
                    },

                    #[track = "model.redraw"]
                    set_tooltip_text: {
                        drawing_area.queue_draw();
                        None
                    },
                }
            }
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let engine = Rc::new(RefCell::new(Engine::new()));
        {
            let mut e = engine.borrow_mut();
            e.buffer.insert(0, "fn main() {\n    let greeting = \"Hello VimCode\";\n    println!(\"{}\", greeting);\n}\n");
            e.update_syntax();
        }

        let model = App {
            engine: engine.clone(),
            redraw: false,
        };
        let widgets = view_output!();

        let engine_clone = engine.clone();
        widgets.drawing_area.set_draw_func(move |_, cr, _, _| {
            let engine = engine_clone.borrow();
            draw_editor(cr, &engine);
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            Msg::KeyPress(key) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(key_name) = key.name() {
                    engine.handle_key(key_name.as_str());
                }

                self.redraw = !self.redraw;
            }
        }
    }
}

fn draw_editor(cr: &Context, engine: &Engine) {
    // 1. Background
    cr.set_source_rgb(0.1, 0.1, 0.1);
    cr.paint().expect("Invalid cairo surface");

    // 2. Setup Pango
    let pango_ctx = pangocairo::create_context(cr);
    let layout = pango::Layout::new(&pango_ctx);
    layout.set_font_description(Some(&FontDescription::from_string("Monospace 14")));

    let line_height = 24.0;

    // 3. Render Text with Highlights
    for (i, line) in engine.buffer.content.lines().enumerate() {
        let y = i as f64 * line_height;
        layout.set_text(&line.to_string());

        let line_start_byte = engine.buffer.content.line_to_byte(i);
        let line_end_byte = line_start_byte + line.len_bytes();

        let attrs = AttrList::new();

        for (start, end, scope) in &engine.highlights {
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

            // Color mapping
            let color_hex = match scope.as_str() {
                "keyword" | "operator" => "#c678dd",      // Purple
                "string" => "#98c379",                    // Green
                "comment" => "#5c6370",                   // Grey
                "function" | "method" => "#61afef",       // Blue
                "type" | "class" | "struct" => "#e5c07b", // Yellow/Orange
                "variable" => "#e06c75",                  // Red
                _ => "#abb2bf",                           // Default fg
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

        cr.move_to(0.0, y);

        // Base text color (fallback)
        cr.set_source_rgb(0.9, 0.9, 0.9);
        pangocairo::show_layout(cr, &layout);
    }

    // 4. Render Cursor
    if let Some(line) = engine.buffer.content.lines().nth(engine.cursor.line) {
        // Measure text to find cursor X
        // Clean layout for measurement
        layout.set_text(&line.to_string());
        layout.set_attributes(None);

        // Simplified X calc (assume monospace)
        let cursor_x = engine.cursor.col as f64 * 10.0;
        let cursor_y = engine.cursor.line as f64 * line_height;

        match engine.mode {
            Mode::Normal => {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.5);
                cr.rectangle(cursor_x, cursor_y, 10.0, line_height);
            }
            Mode::Insert => {
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.rectangle(cursor_x, cursor_y, 2.0, line_height);
            }
        }
        cr.fill().unwrap();
    }
}

fn main() {
    let app = RelmApp::new("org.vimcode.phase6");
    app.run::<App>(());
}
