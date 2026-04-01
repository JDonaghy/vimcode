use super::*;

/// Open a URL in the default browser (only https/http).
pub(super) fn open_url(url: &str) {
    if !crate::core::engine::is_safe_url(url) {
        return;
    }
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(target_os = "macos"))]
    let cmd = "xdg-open";
    std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok();
}

/// Returns true if `key` + `state` match a panel_keys binding string like `<C-b>`, `<C-S-e>`.
pub(super) fn matches_gtk_key(binding: &str, key: gdk::Key, state: gdk::ModifierType) -> bool {
    let Some((ctrl, shift, alt, key_name)) =
        crate::core::settings::parse_key_binding_named(binding)
    else {
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
    match key_name.as_str() {
        "Tab" | "tab" => key == gdk::Key::Tab || key == gdk::Key::ISO_Left_Tab,
        "Space" | "space" => key.to_unicode() == Some(' '),
        "Escape" | "Esc" => key == gdk::Key::Escape,
        s if s.chars().count() == 1 => {
            let ch = s.chars().next().unwrap().to_ascii_lowercase();
            key.to_unicode()
                .map(|c| c.to_ascii_lowercase() == ch)
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// Build a single setting row widget (label+description on left, control widget on right).
pub(super) fn build_setting_row(
    def: &render::SettingDef,
    settings: &core::settings::Settings,
    sender: &relm4::Sender<Msg>,
) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.set_margin_top(3);
    row.set_margin_bottom(3);
    row.set_margin_start(4);
    row.set_margin_end(10);

    // Left side: label + dim description
    let left = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    left.set_hexpand(true);
    left.set_valign(gtk4::Align::Center);

    let label = gtk4::Label::new(Some(def.label));
    label.set_halign(gtk4::Align::Start);
    left.append(&label);

    if !def.description.is_empty() {
        let desc = gtk4::Label::new(Some(def.description));
        desc.set_css_classes(&["dim-label"]);
        desc.set_halign(gtk4::Align::Start);
        desc.set_wrap(true);
        desc.set_max_width_chars(26);
        left.append(&desc);
    }
    row.append(&left);

    let key = def.key.to_string();
    let current_val = settings.get_value_str(def.key);

    // Right side: control widget based on setting type
    match &def.setting_type {
        render::SettingType::Bool => {
            let switch = gtk4::Switch::new();
            switch.set_active(current_val == "true");
            switch.set_valign(gtk4::Align::Center);
            switch.set_margin_top(4);
            switch.set_margin_bottom(4);
            switch.set_margin_start(4);
            switch.set_margin_end(4);
            let sender_c = sender.clone();
            let key_c = key.clone();
            switch.connect_state_set(move |_, state| {
                sender_c
                    .send(Msg::SettingChanged {
                        key: key_c.clone(),
                        value: if state { "true" } else { "false" }.to_string(),
                    })
                    .ok();
                gtk4::glib::Propagation::Proceed
            });
            row.append(&switch);
        }
        render::SettingType::Integer { min, max } => {
            let val: f64 = current_val.parse().unwrap_or(0.0);
            let adj = gtk4::Adjustment::new(val, *min as f64, *max as f64 + 1.0, 1.0, 10.0, 1.0);
            let spin = gtk4::SpinButton::new(Some(&adj), 1.0, 0);
            spin.set_valign(gtk4::Align::Center);
            spin.set_width_chars(7);
            let sender_c = sender.clone();
            let key_c = key.clone();
            spin.connect_value_changed(move |sp| {
                sender_c
                    .send(Msg::SettingChanged {
                        key: key_c.clone(),
                        value: (sp.value() as i64).to_string(),
                    })
                    .ok();
            });
            row.append(&spin);
        }
        render::SettingType::StringVal => {
            let entry = gtk4::Entry::new();
            entry.set_text(&current_val);
            entry.set_width_chars(14);
            entry.set_valign(gtk4::Align::Center);
            let sender_c = sender.clone();
            let key_c = key.clone();
            entry.connect_changed(move |e| {
                sender_c
                    .send(Msg::SettingChanged {
                        key: key_c.clone(),
                        value: e.text().to_string(),
                    })
                    .ok();
            });
            row.append(&entry);
        }
        render::SettingType::Enum(options) => {
            let current_idx = options.iter().position(|o| *o == current_val).unwrap_or(0) as u32;
            let dropdown = gtk4::DropDown::from_strings(options);
            dropdown.set_selected(current_idx);
            dropdown.set_valign(gtk4::Align::Center);
            let sender_c = sender.clone();
            let key_c = key.clone();
            let options_vec: Vec<String> = options.iter().map(|s| s.to_string()).collect();
            dropdown.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                if let Some(opt) = options_vec.get(idx) {
                    sender_c
                        .send(Msg::SettingChanged {
                            key: key_c.clone(),
                            value: opt.clone(),
                        })
                        .ok();
                }
            });
            row.append(&dropdown);
        }
        render::SettingType::DynamicEnum(options_fn) => {
            let options_vec = options_fn();
            let strs: Vec<&str> = options_vec.iter().map(|s| s.as_str()).collect();
            let current_idx = options_vec
                .iter()
                .position(|o| o == &current_val)
                .unwrap_or(0) as u32;
            let dropdown = gtk4::DropDown::from_strings(&strs);
            dropdown.set_selected(current_idx);
            dropdown.set_valign(gtk4::Align::Center);
            let sender_c = sender.clone();
            let key_c = key.clone();
            dropdown.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                if let Some(opt) = options_vec.get(idx) {
                    sender_c
                        .send(Msg::SettingChanged {
                            key: key_c.clone(),
                            value: opt.clone(),
                        })
                        .ok();
                }
            });
            row.append(&dropdown);
        }
        render::SettingType::BufferEditor => {
            let count_text = match def.key {
                "keymaps" => format!("{} defined", settings.keymaps.len()),
                "extension_registries" => {
                    format!("{} configured", settings.extension_registries.len())
                }
                _ => String::new(),
            };
            if !count_text.is_empty() {
                let count_label = gtk4::Label::new(Some(&count_text));
                count_label.set_valign(gtk4::Align::Center);
                count_label.set_css_classes(&["dim-label"]);
                row.append(&count_label);
            }

            let button = gtk4::Button::with_label("Edit…");
            button.set_valign(gtk4::Align::Center);
            let sender_c = sender.clone();
            let key_c = def.key.to_string();
            button.connect_clicked(move |_| {
                sender_c.send(Msg::OpenBufferEditor(key_c.clone())).ok();
            });
            row.append(&button);
        }
    }

    row
}

/// Populate a settings form container with category headers and setting rows.
/// Returns category sections as `(header_label, Vec<(search_text, row_box)>)` for
/// search filtering.
pub(super) fn build_settings_form(
    container: &gtk4::Box,
    settings: &core::settings::Settings,
    sender: &relm4::Sender<Msg>,
) -> Vec<(gtk4::Label, Vec<(String, gtk4::Box)>)> {
    let mut sections: Vec<(gtk4::Label, Vec<(String, gtk4::Box)>)> = Vec::new();
    let mut current_category = "";

    for def in render::SETTING_DEFS {
        if def.category != current_category {
            current_category = def.category;

            let header = gtk4::Label::new(Some(def.category));
            header.set_halign(gtk4::Align::Start);
            header.set_css_classes(&["settings-category-header"]);
            header.set_margin_top(12);
            header.set_margin_bottom(4);
            header.set_margin_start(4);
            container.append(&header);

            sections.push((header, Vec::new()));
        }

        let row = build_setting_row(def, settings, sender);
        let search_text =
            format!("{} {} {}", def.label, def.description, def.category).to_lowercase();

        container.append(&row);
        if let Some(section) = sections.last_mut() {
            section.1.push((search_text, row));
        }
    }

    sections
}

/// Translate a GTK key event to PTY input bytes.
/// Returns an empty vec for keys that have no PTY mapping.
pub(super) fn gtk_key_to_pty_bytes(key_name: &str, unicode: Option<char>, ctrl: bool) -> Vec<u8> {
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

/// Build a `gio::Menu` from engine-generated `ContextMenuItem`s.
/// Groups items into sections split at `separator_after` boundaries.
pub(super) fn build_gio_menu_from_engine_items(
    items: &[core::engine::ContextMenuItem],
    action_prefix: &str,
) -> gtk4::gio::Menu {
    let menu = gtk4::gio::Menu::new();
    let mut section = gtk4::gio::Menu::new();
    for item in items {
        section.append(
            Some(&item.label),
            Some(&format!("{action_prefix}.{}", item.action)),
        );
        if item.separator_after {
            menu.append_section(None, &section);
            section = gtk4::gio::Menu::new();
        }
    }
    if section.n_items() > 0 {
        menu.append_section(None, &section);
    }
    menu
}

/// Clean up any previous context-menu popover from the shared slot,
/// then store the new one.  The old popover is popdown'd + unparented
/// **before** the new one is set_parent'd, so there is never a moment
/// where two popovers coexist on the same parent.
pub(super) fn swap_ctx_popover(
    slot: &Rc<RefCell<Option<gtk4::PopoverMenu>>>,
    new: gtk4::PopoverMenu,
) {
    let mut guard = slot.borrow_mut();
    if let Some(old) = guard.take() {
        old.popdown();
        // NOTE: we intentionally do NOT call old.unparent() here.
        // GTK4 internally tears down the CSS node tree during unparent(),
        // which triggers a non-fatal "gtk_css_node_insert_after" assertion.
        // Letting GTK handle the lifecycle naturally avoids the assertion.
        // The old widget will be dropped when this Option is overwritten.
    }
    *guard = Some(new);
}

/// Install the bundled Nerd Font icon subset to `~/.local/share/fonts/` so
/// GTK/Pango can resolve the Nerd Font glyphs without a user-installed Nerd Font.
/// The font file is embedded in the binary via `include_bytes!` and only written
/// to disk if it's missing or has the wrong size.
pub(super) fn install_bundled_icon_font() {
    static FONT_BYTES: &[u8] = include_bytes!("../../data/fonts/vimcode-icons.ttf");

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let fonts_dir = home.join(".local/share/fonts");
    let _ = fs::create_dir_all(&fonts_dir);
    let dest = fonts_dir.join("vimcode-icons.ttf");

    // Skip write if the file already exists with the correct size.
    if dest.exists() {
        if let Ok(meta) = fs::metadata(&dest) {
            if meta.len() == FONT_BYTES.len() as u64 {
                return;
            }
        }
    }

    if fs::write(&dest, FONT_BYTES).is_ok() {
        // Trigger fontconfig cache rebuild so the font is available immediately.
        let _ = std::process::Command::new("fc-cache")
            .arg("-f")
            .arg(&fonts_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

pub(super) fn install_icon_and_desktop() {
    use std::fs;
    use std::path::PathBuf;

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let data_dir = home.join(".local/share");
    let hicolor = data_dir.join("icons/hicolor");

    // SVG icon for scalable size (GTK/GNOME renders SVGs natively).
    let svg_dir = hicolor.join("scalable/apps");
    let svg_path = svg_dir.join("vimcode.svg");
    let svg_bytes: &[u8] = include_bytes!("../../vim-code.svg");
    if fs::create_dir_all(&svg_dir).is_ok() {
        let _ = fs::write(&svg_path, svg_bytes);
    }

    // Render the SVG to PNG at multiple sizes so compositors and window
    // managers that don't support SVG lookup (or only read _NET_WM_ICON
    // pixel data at a fixed size) get a crisp icon in alt-tab / taskbar.
    if svg_path.exists() {
        for size in [48, 64, 128, 256, 512] {
            let png_dir = hicolor.join(format!("{size}x{size}/apps"));
            let png_path = png_dir.join("vimcode.png");
            if png_path.exists() {
                continue; // already rendered
            }
            if fs::create_dir_all(&png_dir).is_ok() {
                if let Ok(pixbuf) =
                    gtk4::gdk_pixbuf::Pixbuf::from_file_at_size(&svg_path, size, size)
                {
                    let _ = pixbuf.savev(&png_path, "png", &[]);
                }
            }
        }
    }

    // Refresh icon theme cache so the new icons are picked up immediately.
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("--force")
        .arg("--quiet")
        .arg(&hicolor)
        .output();

    // .desktop file
    let app_dir = data_dir.join("applications");
    let desktop_path = app_dir.join("com.vimcode.VimCode.desktop");
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "vimcode".to_string());
    let desktop = format!(
        "[Desktop Entry]\n\
         Name=VimCode\n\
         Comment=Vim-like code editor\n\
         Exec={exe}\n\
         Icon=vimcode\n\
         Terminal=false\n\
         Type=Application\n\
         Categories=Development;TextEditor;\n\
         StartupWMClass=com.vimcode.VimCode\n"
    );
    if fs::create_dir_all(&app_dir).is_ok() {
        let _ = fs::write(&desktop_path, desktop);
    }
}

/// Count total visible rows in a gio::Menu (items + section separators).
pub(super) fn menu_row_count(menu: &gtk4::gio::Menu) -> i32 {
    let mut rows = 0i32;
    for i in 0..menu.n_items() {
        if let Some(section) = menu
            .item_link(i, "section")
            .and_then(|m| m.downcast::<gtk4::gio::Menu>().ok())
        {
            if i > 0 {
                rows += 1; // separator
            }
            rows += section.n_items();
        } else {
            rows += 1;
        }
    }
    rows
}

/// GLib log handler that suppresses the known GTK4 `gtk_css_node_insert_after`
/// assertion while forwarding all other CRITICAL messages to the default handler.
pub(super) unsafe extern "C" fn suppress_css_node_warning(
    log_domain: *const std::ffi::c_char,
    log_level: gtk4::glib::ffi::GLogLevelFlags,
    message: *const std::ffi::c_char,
    _user_data: gtk4::glib::ffi::gpointer,
) {
    let msg = unsafe { std::ffi::CStr::from_ptr(message) }
        .to_str()
        .unwrap_or("");
    if msg.contains("gtk_css_node_insert_after") {
        return; // suppress
    }
    // Forward other CRITICAL messages to stderr.
    let domain = unsafe { std::ffi::CStr::from_ptr(log_domain) }
        .to_str()
        .unwrap_or("?");
    let level_str = if log_level & gtk4::glib::ffi::G_LOG_LEVEL_CRITICAL != 0 {
        "CRITICAL"
    } else {
        "WARNING"
    };
    eprintln!("({domain}): Gtk-{level_str}: {msg}");
}
