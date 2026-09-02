//! Standalone TUI binary for VimCode — no GTK4/Relm4/Cairo dependencies.
//!
//! Build with: `cargo build --release --bin vimcode-tui --no-default-features`

//! #657: `core` / `icons` / `render` / `tui_main` were promoted into
//! `vimcode_core`, so this binary no longer re-compiles them as private
//! modules — it just calls into the library. The crate-wide
//! `allow(dead_code, unused_imports)` this file used to carry (because the
//! shared modules contain GTK-only code with no caller on this lane) moved
//! with them, narrowed to `render` / `tui_main` on the no-GTK lane only.

use std::collections::HashSet;
use std::path::PathBuf;

use vimcode_core::tui_main;

fn main() {
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
