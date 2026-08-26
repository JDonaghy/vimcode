use super::*;

/// Freedesktop application id. Must match the `.desktop` file's basename
/// (`{APP_ID}.desktop`) and its `StartupWMClass=` entry, because that triple
/// is what a desktop environment uses to map a live toplevel back to the
/// installed launcher — and therefore to its `Icon=` entry (#556).
pub(super) const APP_ID: &str = "com.vimcode.VimCode";

/// Human-readable application name (GLib `g_set_application_name`).
pub(super) const APP_NAME: &str = "VimCode";

/// Icon-theme name of the installed icon: `hicolor/*/apps/{ICON_NAME}.{svg,png}`
/// and the `.desktop` file's `Icon=` value.
pub(super) const ICON_NAME: &str = "vimcode";

/// Render the contents of vimcode's `.desktop` launcher.
///
/// Split out of [`install_icon_and_desktop`] so the `Icon=` / `StartupWMClass=`
/// entries — the two fields a desktop environment matches against a live
/// toplevel to pick the taskbar/dock icon — are assertable without touching
/// the user's `$HOME` (#556).
pub(super) fn desktop_entry(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Name={APP_NAME}\n\
         Comment=Vim-like code editor\n\
         Exec={exec}\n\
         Icon={ICON_NAME}\n\
         Terminal=false\n\
         Type=Application\n\
         Categories=Development;TextEditor;\n\
         StartupWMClass={APP_ID}\n"
    )
}

/// Stamp vimcode's application identity onto the process so the window
/// manager can match the toplevel to the installed `.desktop` file.
///
/// The quadraui GTK shell runner builds its `gtk4::Application` with a
/// hard-coded `org.quadraui.app` id and offers no override (see
/// `quadraui::gtk::run`'s "Window title + app id" doc — "A future stage may
/// add a builder API"). Since #540 flipped the GTK main loop from Relm4 to
/// that runner, vimcode inherited the generic id and the desktop environment
/// stopped resolving `com.vimcode.VimCode.desktop` — hence the generic
/// gear icon in the taskbar/dock (#556).
///
/// Both live GDK backends derive the toplevel's identity from the *process*,
/// not from `GApplication`:
///
/// * Wayland — `xdg_toplevel.set_app_id` is fed from `g_get_prgname()`.
/// * X11 — `WM_CLASS`'s instance name is `g_get_prgname()`, and the class
///   name is derived from it.
///
/// so setting `prgname` before the toplevel is realized is sufficient, and
/// needs nothing from quadraui. `gtk_window_set_default_icon_name` covers the
/// remaining case of a WM that reads `_NET_WM_ICON` pixel data instead of
/// matching a launcher.
///
/// Idempotent — called once from [`super::run`] right after `gtk4::init()`
/// and re-asserted from `ShellApp::setup`, which the runner invokes before
/// `window.present()`.
pub(super) fn apply_app_identity() {
    // Both setters are write-once by contract and GLib logs a warning on a
    // second call, so re-assert only when the value is not already ours.
    // Order matters: `g_get_application_name` falls back to `prgname` while
    // unset, so prgname has to land first for the second check to be honest.
    if gtk4::glib::prgname().as_deref() != Some(APP_ID) {
        gtk4::glib::set_prgname(Some(APP_ID));
    }
    if gtk4::glib::application_name().as_deref() != Some(APP_NAME) {
        gtk4::glib::set_application_name(APP_NAME);
    }
    // `set_default_icon_name` asserts an initialized main thread, which the
    // headless test harness never has (#646) — same guard as
    // `App::find_visible_window`.
    if gtk4::is_initialized_main_thread() {
        gtk4::Window::set_default_icon_name(ICON_NAME);
    }
}

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
    let svg_path = svg_dir.join(format!("{ICON_NAME}.svg"));
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
            let png_path = png_dir.join(format!("{ICON_NAME}.png"));
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
    let desktop_path = app_dir.join(format!("{APP_ID}.desktop"));
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ICON_NAME.to_string());
    let desktop = desktop_entry(&exe);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #556: the taskbar/dock icon is resolved by matching the toplevel's
    /// app-id against an installed launcher, so the launcher's *filename*,
    /// its `StartupWMClass=` and the runtime app-id must all be the same
    /// string, and `Icon=` must name the icon that
    /// `install_icon_and_desktop` actually writes into `hicolor`.
    #[test]
    fn desktop_entry_matches_app_id_and_installed_icon() {
        let entry = desktop_entry("/usr/bin/vimcode");
        assert!(
            entry.contains(&format!("\nStartupWMClass={APP_ID}\n")),
            "launcher must declare the WM class the window reports: {entry}"
        );
        assert!(
            entry.contains(&format!("\nIcon={ICON_NAME}\n")),
            "launcher must reference the installed hicolor icon: {entry}"
        );
        assert!(entry.contains("\nExec=/usr/bin/vimcode\n"), "{entry}");
        assert!(entry.starts_with("[Desktop Entry]\n"), "{entry}");
        assert!(entry.contains(&format!("\nName={APP_NAME}\n")), "{entry}");
    }

    /// The `.desktop` basename is `{APP_ID}.desktop`, so the id must be a
    /// valid reverse-DNS freedesktop id — not the bare `vimcode` icon name.
    #[test]
    fn app_id_is_reverse_dns_and_distinct_from_icon_name() {
        assert_eq!(APP_ID, "com.vimcode.VimCode");
        assert_eq!(ICON_NAME, "vimcode");
        assert!(APP_ID.matches('.').count() >= 2, "{APP_ID}");
    }

    /// #556 regression: under the quadraui ShellApp runner the
    /// `gtk4::Application` is built with a hard-coded `org.quadraui.app` id,
    /// so vimcode must stamp its own identity onto the process. Both live GDK
    /// backends read `g_get_prgname()` for the toplevel's app-id / `WM_CLASS`,
    /// so this is the value the desktop environment matches the launcher on.
    ///
    /// Also pins the headless contract: `apply_app_identity` must not panic
    /// when GTK was never initialized (the `#[cfg(test)]` harness, #646).
    #[test]
    fn apply_app_identity_stamps_prgname_and_application_name() {
        apply_app_identity();
        assert_eq!(
            gtk4::glib::prgname().as_deref(),
            Some(APP_ID),
            "prgname is what GDK reports as the Wayland app_id / X11 WM_CLASS"
        );
        assert_eq!(
            gtk4::glib::application_name().as_deref(),
            Some(APP_NAME),
            "human-readable name shown by pagers and the session manager"
        );
        // Idempotent — `run()` and `ShellApp::setup` both call it, and both
        // GLib setters are write-once (a second `g_set_application_name`
        // logs a warning), so the repeat must be a silent no-op.
        apply_app_identity();
        assert_eq!(gtk4::glib::prgname().as_deref(), Some(APP_ID));
        assert_eq!(gtk4::glib::application_name().as_deref(), Some(APP_NAME));
    }
}
