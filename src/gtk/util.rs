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

/// The single VimCode application identity: app id, `Icon=`/`StartupWMClass=`
/// value, and the stem of the installed icon files. Must match the shipped
/// `data/io.github.jdonaghy.VimCode.desktop` / `data/icons/io.github.jdonaghy.VimCode.svg`
/// and the flatpak manifest (`flatpak/io.github.jdonaghy.VimCode.yml`) exactly.
///
/// #716: this file used to write a *second*, different identity
/// (`com.vimcode.VimCode` / icon name `vimcode`) to `~/.local/share/`,
/// competing with the shipped one — whichever the desktop shell indexed
/// first determined whether the WM taskbar/alt-tab icon resolved at all.
/// `APP_ID` is now the only identity string this module writes anywhere.
pub(super) const APP_ID: &str = "io.github.jdonaghy.VimCode";

/// Edge length, in pixels, of the one-time PNG rasterisation of
/// [`crate::render::APP_ICON_SVG`] the menu row paints (#720).
///
/// Comfortably larger than any menu-bar row height (and so still crisp when a
/// HiDPI scale factor multiplies the device pixels behind that row), but small
/// enough that re-decoding it per frame is free.
const APP_ICON_RASTER_PX: u32 = 64;

/// The app icon as a `quadraui::Image`, pre-rasterised **once** to a small PNG.
///
/// quadraui's `Image` deliberately ships no caching layer ("callers own the
/// bytes/path they hand in; the backend decodes once per paint call") — so
/// handing `Backend::draw_image` the raw 1024×1024 SVG means librsvg renders a
/// megapixel canvas on *every* repaint, only to downscale it into a ~20px slot.
/// Measured on the headless GTK harness that is **+16.5 ms per frame** (4.4 ms →
/// 20.9 ms for a full `render_content`), i.e. every keystroke's repaint, which
/// is not a cost window chrome gets to impose. Rasterising to
/// [`APP_ICON_RASTER_PX`] once and re-handing those bytes puts it back in the
/// noise.
///
/// If this host has no SVG `gdk-pixbuf` loader the rasterisation fails and the
/// raw SVG is handed through unchanged: `draw_image` then reports
/// `Unsupported` and paints nothing (the icon's `fallback_text` is empty by
/// design — see [`crate::render::app_icon_image`]), which is the same visible
/// outcome as skipping the call, but keeps the "why" in one place.
pub(super) fn app_icon_image() -> quadraui::Image {
    use std::sync::OnceLock;
    static PNG: OnceLock<Option<Vec<u8>>> = OnceLock::new();

    match PNG.get_or_init(rasterise_app_icon_png) {
        Some(png) => quadraui::Image {
            source: quadraui::ImageSource::Bytes(png.clone()),
            intrinsic_size: Some((APP_ICON_RASTER_PX, APP_ICON_RASTER_PX)),
            // id / fit / fallback_text stay owned by the shared builder.
            ..crate::render::app_icon_image()
        },
        None => crate::render::app_icon_image(),
    }
}

/// Decode [`crate::render::APP_ICON_SVG`] and re-encode it as an
/// [`APP_ICON_RASTER_PX`]-square PNG. `None` if `gdk-pixbuf` cannot read the
/// SVG (no librsvg loader) or cannot write a PNG.
fn rasterise_app_icon_png() -> Option<Vec<u8>> {
    use gtk4::gdk_pixbuf::{InterpType, Pixbuf};
    let px = APP_ICON_RASTER_PX as i32;
    let svg = Pixbuf::from_read(std::io::Cursor::new(crate::render::APP_ICON_SVG)).ok()?;
    let scaled = svg.scale_simple(px, px, InterpType::Bilinear)?;
    scaled.save_to_bufferv("png", &[]).ok()
}

pub(super) fn install_icon_and_desktop() {
    use std::fs;
    use std::path::PathBuf;

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let data_dir = home.join(".local/share");
    let hicolor = data_dir.join("icons/hicolor");

    // SVG icon for scalable size (GTK/GNOME renders SVGs natively). Same
    // bytes as the shipped `data/icons/io.github.jdonaghy.VimCode.svg` —
    // deduplicated under #716, this used to be a separate `vim-code.svg`
    // copy at the repo root that could silently drift from the shipped one.
    let svg_dir = hicolor.join("scalable/apps");
    let svg_path = svg_dir.join(format!("{APP_ID}.svg"));
    // #720: the bytes now live in exactly one place (`render::APP_ICON_SVG`),
    // shared with the menu-row app icon, so the installed theme icon and the
    // one painted left of `File` can never be different artwork.
    let svg_bytes: &[u8] = crate::render::APP_ICON_SVG;
    if fs::create_dir_all(&svg_dir).is_ok() {
        let _ = fs::write(&svg_path, svg_bytes);
    }
    // #716: remove the stale pre-fix icon file installed under the old,
    // wrong name so it can't shadow the correctly-named one above.
    let _ = fs::remove_file(svg_dir.join("vimcode.svg"));

    // Render the SVG to PNG at multiple sizes so compositors and window
    // managers that don't support SVG lookup (or only read _NET_WM_ICON
    // pixel data at a fixed size) get a crisp icon in alt-tab / taskbar.
    if svg_path.exists() {
        for size in [48, 64, 128, 256, 512] {
            let png_dir = hicolor.join(format!("{size}x{size}/apps"));
            let png_path = png_dir.join(format!("{APP_ID}.png"));
            if png_path.exists() {
                // already rendered
            } else if fs::create_dir_all(&png_dir).is_ok() {
                if let Ok(pixbuf) =
                    gtk4::gdk_pixbuf::Pixbuf::from_file_at_size(&svg_path, size, size)
                {
                    let _ = pixbuf.savev(&png_path, "png", &[]);
                }
            }
            // #716: same cleanup as the SVG above, at every rendered size.
            let _ = fs::remove_file(png_dir.join("vimcode.png"));
        }
    }

    // Refresh icon theme cache so the new icons are picked up immediately.
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .arg("--force")
        .arg("--quiet")
        .arg(&hicolor)
        .output();

    // .desktop file — same identity as the shipped
    // `data/io.github.jdonaghy.VimCode.desktop`, so a non-flatpak build
    // launched from this runtime-written entry resolves to the same WM
    // identity as a flatpak install.
    let app_dir = data_dir.join("applications");
    let desktop_path = app_dir.join(format!("{APP_ID}.desktop"));
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "vimcode".to_string());
    if fs::create_dir_all(&app_dir).is_ok() {
        let _ = fs::write(&desktop_path, desktop_entry_contents(&exe));
    }
    // #716: a stale desktop entry under the old, wrong identity left behind
    // by a pre-fix install would otherwise keep shadowing the correct one
    // in some desktop-shell indexes across an upgrade.
    let _ = fs::remove_file(app_dir.join("com.vimcode.VimCode.desktop"));
}

/// Contents of the runtime-installed `.desktop` file. Factored out from
/// [`install_icon_and_desktop`] so the identity fields are unit-testable
/// without touching the filesystem (#716).
pub(super) fn desktop_entry_contents(exe: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=VimCode\n\
         Comment=Vim-like code editor with GTK4 and tree-sitter\n\
         Exec={exe}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Development;TextEditor;Utility;\n\
         StartupWMClass={APP_ID}\n"
    )
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

    /// #716: this is the one identity string vimcode should ever write —
    /// pin it so a future edit can't silently reintroduce a second one.
    #[test]
    fn app_id_matches_shipped_packaging() {
        assert_eq!(APP_ID, "io.github.jdonaghy.VimCode");
    }

    /// #716 regression test: the runtime-installed `.desktop` file used to
    /// claim `Icon=vimcode` / `StartupWMClass=com.vimcode.VimCode`, a
    /// different identity than the shipped `data/io.github.jdonaghy.VimCode.desktop`.
    /// Whichever one the desktop shell indexed first determined whether the
    /// WM taskbar/alt-tab icon resolved. Assert there is exactly one
    /// identity in the generated contents, and it's the canonical one.
    #[test]
    fn desktop_entry_uses_canonical_app_id_everywhere() {
        let contents = desktop_entry_contents("/usr/bin/vimcode");
        assert!(
            contents.contains(&format!("Icon={APP_ID}\n")),
            "missing Icon={APP_ID} in:\n{contents}"
        );
        assert!(
            contents.contains(&format!("StartupWMClass={APP_ID}\n")),
            "missing StartupWMClass={APP_ID} in:\n{contents}"
        );
        assert!(
            !contents.contains("com.vimcode") && !contents.contains("Icon=vimcode\n"),
            "found the old, wrong identity in:\n{contents}"
        );
    }

    #[test]
    fn desktop_entry_embeds_the_given_exe_path() {
        let contents = desktop_entry_contents("/opt/vimcode/bin/vimcode");
        assert!(contents.contains("Exec=/opt/vimcode/bin/vimcode\n"));
    }

    /// #720 perf guard: the icon handed to `Backend::draw_image` must be the
    /// once-rasterised **PNG**, never the raw SVG.
    ///
    /// quadraui's `Image` carries no cache by design, so whatever bytes go in
    /// here get re-decoded on every single repaint. With the 1024x1024 SVG
    /// that measured +16.5 ms per `render_content` (4.4 ms -> 20.9 ms on the
    /// headless harness); with the cached 64px PNG it is +0.35 ms. This test
    /// is the tripwire against a well-meaning "simplify" back to
    /// `render::app_icon_image()` at the paint site, which would look and test
    /// identical but quietly cap the UI's frame rate.
    #[test]
    fn painted_app_icon_is_the_rasterised_png_not_the_raw_svg() {
        let img = app_icon_image();
        let quadraui::ImageSource::Bytes(bytes) = &img.source else {
            panic!(
                "the app icon must be carried as bytes, got {:?}",
                img.source
            );
        };
        assert_ne!(
            bytes.as_slice(),
            crate::render::APP_ICON_SVG,
            "the raw SVG must not reach draw_image -- it would be re-rendered \
             through librsvg every frame"
        );
        assert_eq!(
            &bytes[..8],
            b"\x89PNG\r\n\x1a\n",
            "expected a PNG signature from the one-time rasterisation"
        );
        assert_eq!(
            img.intrinsic_size,
            Some((APP_ICON_RASTER_PX, APP_ICON_RASTER_PX)),
            "intrinsic_size must describe the rasterised bytes, not the SVG viewBox"
        );
        // Identity (which artwork / how it fits) still comes from the one
        // shared builder, so the two can't diverge.
        let shared = crate::render::app_icon_image();
        assert_eq!(img.id, shared.id);
        assert_eq!(img.fit, shared.fit);
        assert_eq!(img.fallback_text, shared.fallback_text);
    }
}
