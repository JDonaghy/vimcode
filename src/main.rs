// Relm4 view! macro generates #[name = "..."] bindings that trigger this lint.
// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional.
#![allow(unused_assignments, deprecated)]

mod core;
mod gtk;
mod icons;
mod render;
mod tui_main;

use std::path::PathBuf;

fn main() {
    // Parse CLI args to get optional file path
    let args: Vec<String> = std::env::args().collect();

    // --version / -V: print version and exit
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("VimCode {}", env!("CARGO_PKG_VERSION"));
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
