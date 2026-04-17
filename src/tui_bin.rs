//! Standalone TUI binary for VimCode — no GTK4/Relm4/Cairo dependencies.
//!
//! Build with: `cargo build --release --bin vimcode-tui --no-default-features`

// Shared modules contain code used only by the GTK binary — suppress warnings.
#![allow(
    dead_code,
    unused_imports,
    unused_assignments,
    clippy::collapsible_match,
    clippy::explicit_counter_loop
)]

use std::collections::HashSet;
use std::path::PathBuf;

mod core;
mod icons;
mod render;
mod tui_main;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // --version / -V: print version and exit
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("VimCode {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // --debug <logfile>: write debug log to the given file
    let debug_log = args
        .iter()
        .position(|a| a == "--debug")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // First positional argument (not starting with '-', not a --debug value)
    let skip_args: HashSet<usize> = {
        let mut s = HashSet::new();
        if let Some(i) = args.iter().position(|a| a == "--debug") {
            s.insert(i);
            s.insert(i + 1);
        }
        s
    };
    let file_path = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(i, a)| !a.starts_with('-') && !skip_args.contains(i))
        .map(|(_, a)| PathBuf::from(a));

    tui_main::run(file_path, debug_log);
}
