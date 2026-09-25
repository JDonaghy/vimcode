// `open_url`/`install_bundled_icon_font` moved to the backend-neutral
// `crate::app_support` (#862) — neither named a `gtk4`/`gio` type. Nothing
// under `crate::gtk` referenced them by this path (only `crate::app` did, and
// it now imports `crate::app_support` directly), so no re-export is needed.

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

/// Whether this host's `gdk-pixbuf` can decode an SVG at all (i.e. has the
/// `librsvg2-common`/similar loader installed).
///
/// Test-only (`#[cfg(test)]`): gates two things — (1) the tests below that
/// exercise [`install_icon_and_desktop_at`]'s per-size PNG rendering, which
/// needs the same loader via `Pixbuf::from_file_at_size`; (2) the #720 GTK
/// pixel probe in `gtk::testing`'s `app_icon` tests, since without the
/// loader `Backend::draw_image` reports `Unsupported` for the menu-row app
/// icon and paints nothing there — an environment gap (`librsvg2-common` is
/// only a `Recommends` of `libgtk-4-1` on Ubuntu, so a
/// `--no-install-recommends` install can legitimately lack it; CI installs
/// it explicitly, see `.github/workflows/ci.yml`), not a regression in this
/// code.
///
/// Before #1102 this same probe doubled as vimcode's own once-per-run
/// pre-rasteriser for the *painted* icon (`app_icon_image`/
/// `cached_app_icon_png`/`rasterise_app_icon_png`, deleted here): quadraui's
/// `Image` shipped no caching layer, so handing `Backend::draw_image` the raw
/// 1024×1024 SVG meant librsvg rendered a megapixel canvas on *every* repaint,
/// measured at +16.5 ms/frame on the headless GTK harness. quadraui#1014 added
/// a decode cache inside `GtkBackend::draw_image` itself, so every backend now
/// hands the same [`crate::render::app_icon_image`] straight through — see
/// `app_icon_image_for_paint` in `crate::app`, which no longer forks on
/// `#[cfg(feature = "gui")]`.
#[cfg(test)]
pub(super) fn host_has_svg_loader() -> bool {
    use std::sync::OnceLock;
    static HAS_LOADER: OnceLock<bool> = OnceLock::new();
    *HAS_LOADER.get_or_init(|| {
        gtk4::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(crate::render::APP_ICON_SVG))
            .is_ok()
    })
}

/// Sizes (in pixels) the SVG is rasterised to for compositors/WMs that only
/// read fixed-size `_NET_WM_ICON` pixel data instead of looking up the
/// scalable SVG. Shared between the installer and the up-to-date check below
/// so the two can never drift out of sync.
const ICON_PNG_SIZES: [u32; 5] = [48, 64, 128, 256, 512];

/// Name of the stamp file [`install_icon_and_desktop_at`] writes after a
/// successful install, holding the `CARGO_PKG_VERSION` it installed.
///
/// #1106: before this stamp existed, the installer — writes to
/// `~/.local/share/icons/hicolor`, a `.desktop` file, and a
/// `gtk-update-icon-cache` subprocess spawn — ran unconditionally on *every*
/// launch, even though the files it writes never change between runs of the
/// same build. The stamp lets [`install_icon_and_desktop_at`] recognise "this
/// version is already installed" and skip straight to returning.
///
/// #1106 review (nit): lives under [`ICON_INSTALL_STAMP_DIR`] — a
/// vimcode-specific subdirectory of `data_dir` — rather than directly in the
/// shared XDG data root, so it can't be mistaken for someone else's stray
/// dotfile at that level the way `hicolor`/`applications` (namespaced by
/// `APP_ID` within themselves) never are.
const ICON_INSTALL_STAMP_FILE: &str = "icon-install-version";

/// App-specific subdirectory of `data_dir` the install stamp lives under —
/// `~/.local/share/vimcode/`, not `~/.local/share/` directly.
const ICON_INSTALL_STAMP_DIR: &str = "vimcode";

/// Whether a previous [`install_icon_and_desktop_at`] call already installed
/// `current_version` and every file it wrote is still present — i.e. whether
/// this call can skip all filesystem writes and the `gtk-update-icon-cache`
/// spawn.
///
/// Split out as a pure function (#1106) so the decision is unit-testable
/// without touching a filesystem or spawning a subprocess: given the stamp
/// file's contents (if any) and whether the installed files are still there,
/// decide once, the same way [`install_icon_and_desktop_at`] and its test
/// both need to.
fn icon_install_up_to_date(
    stamp_contents: Option<&str>,
    current_version: &str,
    files_present: bool,
) -> bool {
    files_present && stamp_contents.map(str::trim) == Some(current_version)
}

/// Whether every file [`install_icon_and_desktop_at`] writes is present
/// under `data_dir` — the SVG, every rasterised PNG size, and the `.desktop`
/// entry. If any is missing (a partial prior install, or a user/package
/// manager having removed one) a re-install is needed even if the version
/// stamp still matches.
fn icon_install_files_present(hicolor: &std::path::Path, app_dir: &std::path::Path) -> bool {
    let svg_present = hicolor
        .join("scalable/apps")
        .join(format!("{APP_ID}.svg"))
        .exists();
    let pngs_present = ICON_PNG_SIZES.iter().all(|size| {
        hicolor
            .join(format!("{size}x{size}/apps"))
            .join(format!("{APP_ID}.png"))
            .exists()
    });
    let desktop_present = app_dir.join(format!("{APP_ID}.desktop")).exists();
    svg_present && pngs_present && desktop_present
}

pub(super) fn install_icon_and_desktop() {
    use std::path::PathBuf;

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    install_icon_and_desktop_at(&home.join(".local/share"));
}

/// Does the actual writing, parameterised on the `~/.local/share`-equivalent
/// directory so tests can point it at a scratch directory instead of the
/// real one (#1106). [`install_icon_and_desktop`] is the real entry point;
/// this is `pub(super)` only so `super::tests` below can drive it directly.
pub(super) fn install_icon_and_desktop_at(data_dir: &std::path::Path) {
    use std::fs;

    let hicolor = data_dir.join("icons/hicolor");
    let app_dir = data_dir.join("applications");
    let stamp_dir = data_dir.join(ICON_INSTALL_STAMP_DIR);
    let stamp_path = stamp_dir.join(ICON_INSTALL_STAMP_FILE);
    let current_version = env!("CARGO_PKG_VERSION");

    let stamp_contents = fs::read_to_string(&stamp_path).ok();
    if icon_install_up_to_date(
        stamp_contents.as_deref(),
        current_version,
        icon_install_files_present(&hicolor, &app_dir),
    ) {
        // #1106: this version is already installed and every file it wrote
        // is still there — skip the writes and the `gtk-update-icon-cache`
        // subprocess spawn below entirely, rather than redoing packaging
        // work on every single launch.
        return;
    }

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
    //
    // #1106 review: this used to skip re-rendering a size whose PNG already
    // existed on disk, regardless of whether its bytes matched the SVG this
    // call just wrote. That was harmless when the function ran on every
    // launch (an existing PNG was always current, since nothing else changes
    // the artwork), but the version-stamp gate above means reaching this
    // point at all now means "the version changed or a file went missing" —
    // exactly the case where a stale PNG from a previous version's artwork
    // must NOT be left in place. So: unconditionally (re)render every size
    // whenever we've decided a reinstall is needed at all, matching the SVG
    // and `.desktop` file below, which already do the same.
    if svg_path.exists() {
        for size in ICON_PNG_SIZES {
            let png_dir = hicolor.join(format!("{size}x{size}/apps"));
            let png_path = png_dir.join(format!("{APP_ID}.png"));
            if fs::create_dir_all(&png_dir).is_ok() {
                let size = size as i32;
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

    // #1106: record what we just installed so the next launch (same
    // version, files intact) can skip straight past the check above.
    if fs::create_dir_all(&stamp_dir).is_ok() {
        let _ = fs::write(&stamp_path, current_version);
    }
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

    /// #1106: the gate that lets a second launch skip the install entirely.
    /// Pure-logic coverage of [`icon_install_up_to_date`] — no filesystem,
    /// no subprocess — for every combination the real callsite can hit.
    #[test]
    fn icon_install_up_to_date_requires_matching_version_and_present_files() {
        let current = "1.2.3";

        // No stamp at all (first-ever launch): never up to date.
        assert!(!icon_install_up_to_date(None, current, true));
        assert!(!icon_install_up_to_date(None, current, false));

        // Stamp matches, but a file went missing (e.g. deleted underneath
        // us): still needs a re-install.
        assert!(!icon_install_up_to_date(Some(current), current, false));

        // Stamp is a different (older or newer) version: re-install even
        // though the files are all present, so a version bump's changed
        // artwork/`.desktop` contents actually land (#716's symptom).
        assert!(!icon_install_up_to_date(Some("1.2.2"), current, true));

        // A trailing newline (as `fs::read_to_string` would hand back from a
        // file written with a newline) must not defeat the match.
        assert!(icon_install_up_to_date(
            Some(&format!("{current}\n")),
            current,
            true
        ));

        // The one case that should actually skip the install: same version,
        // every file still there.
        assert!(icon_install_up_to_date(Some(current), current, true));
    }

    /// RAII guard around a [`scratch_data_dir`] temp directory: removes it on
    /// drop, including via unwinding, so a panicking `assert_eq!` partway
    /// through a test (#1106 review nit) can't leak the directory the way a
    /// plain `let _ = std::fs::remove_dir_all(&data_dir);` at the *end* of
    /// the test body would — that line is simply never reached if an earlier
    /// assertion panics first. `Deref<Target = Path>` lets call sites keep
    /// using it exactly like the `PathBuf` it used to be.
    struct ScratchDataDir(std::path::PathBuf);

    impl std::ops::Deref for ScratchDataDir {
        type Target = std::path::Path;
        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for ScratchDataDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Scratch directory under the OS temp dir, unique per call so parallel
    /// `cargo test` threads (and repeated runs) never collide. Not a
    /// dependency addition (`tempfile`) — this file has no prior fixture
    /// pattern to match, and one bespoke helper is cheaper than a new crate
    /// for a single test.
    fn scratch_data_dir(tag: &str) -> ScratchDataDir {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("vimcode-test-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch data dir");
        ScratchDataDir(dir)
    }

    /// #1106 regression test: a second call with an unchanged version must
    /// perform **no** filesystem writes and spawn **no**
    /// `gtk-update-icon-cache` — both gated by the same early return, so
    /// proving the writes didn't happen proves the spawn didn't either.
    ///
    /// Observed RED against unfixed `develop` (which had no gate at all):
    /// with the early-return removed, the second call always rewrites
    /// `desktop_path`, so the sentinel content this test plants gets
    /// clobbered and the final assertion fails.
    #[test]
    fn second_install_with_unchanged_version_is_a_no_op() {
        if !host_has_svg_loader() {
            // Same environment gap `host_has_svg_loader` documents: no SVG
            // loader means the first install's per-size PNG rendering never
            // succeeds, so `icon_install_files_present` can never see a
            // complete install and this test can't reach its "up to date"
            // branch.
            eprintln!(
                "skipping second_install_with_unchanged_version_is_a_no_op: \
                 no gdk-pixbuf SVG loader on this host"
            );
            return;
        }

        let data_dir = scratch_data_dir("icon-install-noop");

        // First call: real install, creates everything including the stamp.
        install_icon_and_desktop_at(&data_dir);

        let desktop_path = data_dir
            .join("applications")
            .join(format!("{APP_ID}.desktop"));
        assert!(
            desktop_path.exists(),
            "first call should have written the .desktop file"
        );

        // Plant a sentinel so a second, unwanted write is observable.
        let sentinel = "SENTINEL: should not be overwritten by a no-op install\n";
        std::fs::write(&desktop_path, sentinel).unwrap();
        let stamp_path = data_dir
            .join(ICON_INSTALL_STAMP_DIR)
            .join(ICON_INSTALL_STAMP_FILE);
        let stamp_mtime_before = std::fs::metadata(&stamp_path).unwrap().modified().unwrap();

        // Second call, same version, files all still present: must skip.
        install_icon_and_desktop_at(&data_dir);

        assert_eq!(
            std::fs::read_to_string(&desktop_path).unwrap(),
            sentinel,
            "second install must not rewrite the .desktop file when the \
             version and files are unchanged"
        );
        let stamp_mtime_after = std::fs::metadata(&stamp_path).unwrap().modified().unwrap();
        assert_eq!(
            stamp_mtime_before, stamp_mtime_after,
            "second install must not rewrite the stamp file either"
        );
    }

    /// #1106: a version bump must still trigger a real re-install (#716's
    /// symptom — a stale icon in the WM app bar/alt-tab — must not return).
    #[test]
    fn install_after_version_bump_still_reinstalls() {
        if !host_has_svg_loader() {
            eprintln!(
                "skipping install_after_version_bump_still_reinstalls: \
                 no gdk-pixbuf SVG loader on this host"
            );
            return;
        }

        let data_dir = scratch_data_dir("icon-install-version-bump");
        install_icon_and_desktop_at(&data_dir);

        let desktop_path = data_dir
            .join("applications")
            .join(format!("{APP_ID}.desktop"));
        std::fs::write(&desktop_path, "stale contents from an old version\n").unwrap();
        // Simulate "the running binary is a newer version than what's
        // installed" by rewinding the stamp instead.
        std::fs::write(
            data_dir
                .join(ICON_INSTALL_STAMP_DIR)
                .join(ICON_INSTALL_STAMP_FILE),
            "0.0.0-older",
        )
        .unwrap();

        install_icon_and_desktop_at(&data_dir);

        assert_eq!(
            std::fs::read_to_string(&desktop_path).unwrap(),
            desktop_entry_contents(
                &std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "vimcode".to_string())
            ),
            "a version bump must reinstall the .desktop file, not leave the stale one"
        );
        assert_eq!(
            std::fs::read_to_string(
                data_dir
                    .join(ICON_INSTALL_STAMP_DIR)
                    .join(ICON_INSTALL_STAMP_FILE)
            )
            .unwrap(),
            env!("CARGO_PKG_VERSION"),
            "the stamp must be updated to the currently-running version"
        );
    }
}
