//! Standalone native Windows GUI binary for VimCode.
//!
//! Build with: `cargo build --release --bin vimcode-win --features win-gui`
//!
//! Uses Win32 + Direct2D + DirectWrite — no GTK4/Relm4/Cairo dependencies.

// Suppress warnings for code shared with other backends.
#![allow(dead_code, unused_imports, unused_assignments)]
// Build as a Windows GUI application (no console window).
#![windows_subsystem = "windows"]

use std::path::PathBuf;

mod core;
mod icons;
mod render;
mod win_gui;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // --version / -V: print version and exit
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("VimCode {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // First positional argument (not starting with '-')
    let file_path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(PathBuf::from);

    win_gui::run(file_path);
}
