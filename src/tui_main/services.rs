//! Platform-services impl for the TUI backend.
//!
//! Stub implementations for Stage 1 of the Phase B.4 trait migration —
//! the existing TUI app currently handles clipboard via
//! [`super::setup_tui_clipboard`] writing closures into engine fields,
//! and file dialogs / notifications / URL opening are not used by the
//! TUI. As the trait migration progresses, these stubs gain real
//! bodies and the engine clipboard plumbing is deleted.

use std::path::PathBuf;

use quadraui::backend::{Clipboard, FileDialogOptions, Notification, PlatformServices};

/// TUI-side clipboard. Currently a no-op stub — the engine still owns
/// the real clipboard closures (see `setup_tui_clipboard` in
/// `tui_main/mod.rs`). Wired up in a later B.4 stage.
pub struct TuiClipboard;

impl Clipboard for TuiClipboard {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&self, _text: &str) {}
}

/// Platform-services impl for the TUI backend. Holds the clipboard;
/// file dialogs / notifications / URL opening are stubs for now.
pub struct TuiPlatformServices {
    clipboard: TuiClipboard,
}

impl TuiPlatformServices {
    pub fn new() -> Self {
        Self {
            clipboard: TuiClipboard,
        }
    }
}

impl Default for TuiPlatformServices {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformServices for TuiPlatformServices {
    fn clipboard(&self) -> &dyn Clipboard {
        &self.clipboard
    }

    fn show_file_open_dialog(&self, _opts: FileDialogOptions) -> Option<PathBuf> {
        // TUI has no native file picker — apps should provide an in-TUI
        // picker (folder picker, palette, etc.) instead.
        None
    }

    fn show_file_save_dialog(&self, _opts: FileDialogOptions) -> Option<PathBuf> {
        None
    }

    fn send_notification(&self, _n: Notification) {
        // TUI has no native notification system. Apps should surface
        // status via the engine's message line.
    }

    fn open_url(&self, _url: &str) {
        // TUI has no integrated URL handler. Apps that want to open a
        // browser shell out via std::process::Command directly.
    }

    fn platform_name(&self) -> &'static str {
        "tui"
    }
}
