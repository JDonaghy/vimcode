//! GTK implementation of [`quadraui::PlatformServices`].
//!
//! Stage 1 stub: methods return safe defaults. Stage 7 fills these
//! in with real GTK API calls (`gdk::Display::clipboard()`,
//! `gtk::FileDialog`, `gio::Notification`, `gtk::UriLauncher`).

use std::path::PathBuf;

use quadraui::backend::{Clipboard, FileDialogOptions, Notification};
use quadraui::PlatformServices;

/// GTK platform-services impl. Stage 7 replaces the stubs with real
/// GTK API calls. Until then, app code that depends on these (file
/// dialogs, notifications, clipboard from quadraui's perspective)
/// uses the existing GTK-direct paths in `mod.rs`; the trait surface
/// is just present so the `Backend` impl is complete.
pub struct GtkPlatformServices {
    clipboard: GtkClipboard,
}

impl GtkPlatformServices {
    pub fn new() -> Self {
        Self {
            clipboard: GtkClipboard,
        }
    }
}

impl Default for GtkPlatformServices {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformServices for GtkPlatformServices {
    fn clipboard(&self) -> &dyn Clipboard {
        &self.clipboard
    }

    fn show_file_open_dialog(&self, _opts: FileDialogOptions) -> Option<PathBuf> {
        // Stage 7: gtk::FileDialog::open. Today vimcode uses the
        // gtk4-rs FileDialog directly from mod.rs; this trait method
        // is a forward-compatible escape hatch.
        None
    }

    fn show_file_save_dialog(&self, _opts: FileDialogOptions) -> Option<PathBuf> {
        None
    }

    fn send_notification(&self, _n: Notification) {
        // Stage 7: gio::Application::send_notification.
    }

    fn open_url(&self, _url: &str) {
        // Stage 7: gtk::UriLauncher::launch.
    }

    fn platform_name(&self) -> &'static str {
        "gtk"
    }
}

/// GTK clipboard wrapper. Stage 7 hooks this up to
/// `gdk::Display::default()?.clipboard()`. Today it's a stub —
/// vimcode's existing GTK clipboard path goes through engine
/// callbacks set in `mod.rs`.
pub struct GtkClipboard;

impl Clipboard for GtkClipboard {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&self, _text: &str) {
        // Stage 7: clipboard.set_text(text).
    }
}
