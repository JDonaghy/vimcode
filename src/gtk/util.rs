use super::*;

/// Open a URL in the default browser (only https/http).
pub(super) fn open_url(url: &str) {
    crate::core::engine::open_url_in_browser(url);
}

/// True when `key_name` is a GDK modifier-only key (a `Control_L`,
/// `Shift_R`, etc. event fired when the user presses a modifier
/// without combining it with another key).
///
/// Vim's input model has no concept of a modifier alone — modifiers
/// are always part of a chord. Forwarding these to the engine causes
/// the "dismiss on any non-completion key" path to fire on bare Ctrl
/// presses while the completion popup is open (#286).
pub(super) fn is_modifier_only_key(key_name: &str) -> bool {
    matches!(
        key_name,
        "Control_L"
            | "Control_R"
            | "Shift_L"
            | "Shift_R"
            | "Alt_L"
            | "Alt_R"
            | "Meta_L"
            | "Meta_R"
            | "Super_L"
            | "Super_R"
            | "Hyper_L"
            | "Hyper_R"
            | "ISO_Level3_Shift"
            | "Caps_Lock"
            | "Num_Lock"
            | "Scroll_Lock"
    )
}

/// Validate a filename for the explorer file / folder creation flow.
pub(super) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain slashes".to_string());
    }
    if name.contains('\0') {
        return Err("Name cannot contain null characters".to_string());
    }
    #[cfg(windows)]
    {
        if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
            return Err("Name contains invalid characters".to_string());
        }
    }
    if name == "." || name == ".." {
        return Err("Invalid name".to_string());
    }
    Ok(())
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

/// Global GLib structured-log writer that drops a couple of benign GTK4
/// CRITICAL messages and forwards everything else to GLib's default writer:
///   * the `gtk_css_node_insert_after` assertion spam, and
///   * the "Unable to register the application" D-Bus noise emitted when GTK
///     can't reach a usable session bus (the editor runs fine regardless).
///
/// GTK4 logs via `g_log_structured()`, which bypasses per-domain handlers
/// installed with `g_log_set_handler`; the writer func is the single
/// chokepoint that sees every message, so the filtering happens here.
pub(super) unsafe extern "C" fn gtk_log_writer(
    log_level: gtk4::glib::ffi::GLogLevelFlags,
    fields: *const gtk4::glib::ffi::GLogField,
    n_fields: usize,
    user_data: gtk4::glib::ffi::gpointer,
) -> gtk4::glib::ffi::GLogWriterOutput {
    let mut msg = "";
    if !fields.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(fields, n_fields) };
        for field in slice {
            if field.key.is_null() {
                continue;
            }
            let key = unsafe { std::ffi::CStr::from_ptr(field.key) }
                .to_str()
                .unwrap_or("");
            if key == "MESSAGE" && !field.value.is_null() {
                msg = if field.length < 0 {
                    unsafe { std::ffi::CStr::from_ptr(field.value as *const std::ffi::c_char) }
                        .to_str()
                        .unwrap_or("")
                } else {
                    let bytes = unsafe {
                        std::slice::from_raw_parts(field.value as *const u8, field.length as usize)
                    };
                    std::str::from_utf8(bytes).unwrap_or("")
                };
                break;
            }
        }
    }
    if msg.contains("gtk_css_node_insert_after")
        || msg.contains("Unable to register the application")
    {
        return gtk4::glib::ffi::G_LOG_WRITER_HANDLED;
    }
    unsafe { gtk4::glib::ffi::g_log_writer_default(log_level, fields, n_fields, user_data) }
}
