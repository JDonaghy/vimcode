//! Standalone TUI binary for VimCode — no GTK4/Relm4/Cairo dependencies.
//!
//! Build with: `cargo build --release --bin vimcode-tui --no-default-features`

//! `core` / `icons` / `render` / `tui_main` are library modules
//! used by this binary; the `allow(dead_code, unused_imports)`
//! for `render` / `tui_main` on the no-GTK lane accommodates code
//! that is not called from this binary.

use std::collections::HashSet;
use std::path::PathBuf;

use vimcode_core::tui_main;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // --help / -h: print usage and exit
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "VimCode {} (terminal UI)\n\n\
             Usage: vcd [OPTIONS] [FILE]\n\n\
             Options:\n\
             \x20\x20-h, --help          Print this help message and exit\n\
             \x20\x20-V, --version       Print the version and exit\n\
             \x20\x20    --debug <FILE>  Write a debug log to FILE\n\n\
             Arguments:\n\
             \x20\x20[FILE]              File to open on startup\n",
            env!("CARGO_PKG_VERSION"),
        );
        return;
    }

    // --version / -V: print version and exit
    if args.iter().any(|a| a == "--version" || a == "-V") {
        // Name the quadraui this binary is made of: it is a path dep, so
        // nothing else in the build records which one was used.
        println!(
            "VimCode {} ({})",
            env!("CARGO_PKG_VERSION"),
            vimcode_core::quadraui_pin::version_line()
        );
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
