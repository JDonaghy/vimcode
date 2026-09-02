//! Thin GTK-binary shim over `vimcode_core` (#657).
//!
//! Everything this binary used to declare as a `mod` — `core`, `gtk`,
//! `icons`, `render`, `tui_main` — now lives in the library crate, so that
//! integration tests under `tests/` (a separate crate, which links only
//! against `[lib] vimcode_core`) can reach the UI backends and their
//! black-box harnesses. See `src/lib.rs`.

use vimcode_core::{gtk, tui_main};

use std::path::PathBuf;

fn main() {
    // Parse CLI args to get optional file path
    let args: Vec<String> = std::env::args().collect();

    // --version / -V: print version and exit
    if args.iter().any(|a| a == "--version" || a == "-V") {
        // Name the quadraui this binary is made of (#638): it is a path dep, so
        // nothing else in the build records which one was used.
        println!(
            "VimCode {} ({})",
            env!("CARGO_PKG_VERSION"),
            vimcode_core::quadraui_pin::version_line()
        );
        return;
    }

    // --tui / -t flag: launch the terminal UI instead of GTK
    let tui_mode = args.iter().any(|a| a == "--tui" || a == "-t");

    // --debug <logfile>: write debug log to the given file
    let debug_log = args
        .iter()
        .position(|a| a == "--debug")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // First positional argument (not starting with '-', not a --debug value)
    let skip_args: std::collections::HashSet<usize> = {
        let mut s = std::collections::HashSet::new();
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

    if tui_mode {
        tui_main::run(file_path, debug_log);
        return;
    }

    gtk::run(file_path);
}
