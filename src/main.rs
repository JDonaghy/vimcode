//! Thin GUI-binary shim over `vimcode_core` (#657).
//!
//! Everything this binary used to declare as a `mod` — `core`, `gtk`,
//! `icons`, `render`, `tui_main` — now lives in the library crate, so that
//! integration tests under `tests/` (a separate crate, which links only
//! against `[lib] vimcode_core`) can reach the UI backends and their
//! black-box harnesses. See `src/lib.rs`.
//!
//! # Backend selection (#859)
//!
//! This target used to carry `required-features = ["gui"]` in `Cargo.toml`.
//! That list is an **AND**, so every feature set without `gui` made cargo
//! omit the whole bin — silently, with a green exit code and no diagnostic
//! (the #645 trap; see the stanza comment in `Cargo.toml` for the measured
//! incident). The requirement is now empty and the decision moved here, into
//! [`COMPILED_GUI_BACKEND`], so the target compiles in *every* feature set
//! and its disappearance can never again be mistaken for success.
//!
//! Three backends, resolved at compile time:
//!
//! | build | [`GuiBackend`] | runs |
//! |---|---|---|
//! | `--features macos` on macOS | `MacOs` | `vimcode_core::macos::run` (AppKit) |
//! | `--features gui` (the default) | `Gtk` | `vimcode_core::gtk::run` |
//! | neither | `None` | terminal UI, with a one-line stderr notice |
//!
//! `--tui` / `-t` short-circuits all of that and runs the terminal UI
//! regardless, exactly as before.

use std::path::PathBuf;
use std::process::ExitCode;

/// Which GUI backend this binary was compiled with.
///
/// Resolved from cargo features at compile time into [`COMPILED_GUI_BACKEND`];
/// the variants exist in every build so the selection logic is one ordinary
/// `match` that tests can exercise, rather than a thicket of `cfg!` at the
/// call site.
///
/// All three variants exist in every build, but only one is ever
/// *constructed* (whichever arm of the `cfg` cascade below is live), so the
/// other two would otherwise trip `dead_code` — hence the blanket allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum GuiBackend {
    /// GTK4 via `vimcode_core::gtk::run` — the `gui` feature, on by default.
    Gtk,
    /// Native AppKit via `vimcode_core::macos::run` — the `macos` feature on
    /// a macOS host (#859). See [`COMPILED_GUI_BACKEND`]'s doc comment for
    /// the precedence rule against `Gtk` and why `--features gui,macos` is
    /// untested; build the native app as `--no-default-features --features
    /// macos`, which is also the only shape macmini can build (no Homebrew/
    /// gtk4/pkg-config there by policy).
    MacOs,
    /// No GUI backend was compiled in. The binary still builds and still
    /// works — as a terminal editor, which is what the `vcd` bin is.
    None,
}

/// What this build will actually launch.
///
/// macOS wins over GTK when both are available: a `--features gui,macos`
/// build on a Mac (GTK4 from Homebrew alongside the native backend) should
/// run the *native* one, since choosing GTK there would make the native
/// backend unreachable without a rebuild.
///
/// That combination is untested and not a supported configuration —
/// `App::render_content` still has a few `#[cfg(feature = "gui")]` blocks
/// that reach for Pango (`click::build_editor_click_context`), and with
/// `gui` compiled in they would run against a `MacBackend`.
#[cfg(all(feature = "macos", target_os = "macos"))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::MacOs;
#[cfg(all(feature = "gui", not(all(feature = "macos", target_os = "macos"))))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::Gtk;
#[cfg(not(any(feature = "gui", all(feature = "macos", target_os = "macos"))))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::None;

/// The parsed command line.
///
/// Split out of `main` so the parsing rules — which are finicky enough to be
/// worth pinning down (`--debug`'s *value* must not be mistaken for the file
/// argument) — are testable without spawning a process.
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    /// `--version` / `-V`: print the version banner and exit.
    version: bool,
    /// `--tui` / `-t`: force the terminal UI.
    tui: bool,
    /// `--debug <logfile>`: write a debug log to this path.
    debug_log: Option<String>,
    /// First positional argument: the file to open.
    file_path: Option<PathBuf>,
}

impl Args {
    /// Parse an `argv`-shaped slice (element 0 is the program name).
    fn parse(argv: &[String]) -> Self {
        let version = argv.iter().any(|a| a == "--version" || a == "-V");
        let tui = argv.iter().any(|a| a == "--tui" || a == "-t");

        // --debug <logfile>: write debug log to the given file
        let debug_flag = argv.iter().position(|a| a == "--debug");
        let debug_log = debug_flag.and_then(|i| argv.get(i + 1)).cloned();

        // First positional argument (not starting with '-', not a --debug value)
        let skip_args: std::collections::HashSet<usize> = {
            let mut s = std::collections::HashSet::new();
            if let Some(i) = debug_flag {
                s.insert(i);
                s.insert(i + 1);
            }
            s
        };
        let file_path = argv
            .iter()
            .enumerate()
            .skip(1)
            .find(|(i, a)| !a.starts_with('-') && !skip_args.contains(i))
            .map(|(_, a)| PathBuf::from(a));

        Self {
            version,
            tui,
            debug_log,
            file_path,
        }
    }
}

/// The version banner, including which quadraui this binary is made of
/// (#638 — the dependency is a pinned git rev, so nothing else in the build
/// records which one was used) and which GUI backend is compiled in (#859 —
/// with three possible backends and a silent-omission history, "which one is
/// this?" needs to be answerable from the binary).
fn version_banner(backend: GuiBackend) -> String {
    let backend = match backend {
        GuiBackend::Gtk => "gtk",
        GuiBackend::MacOs => "macos",
        GuiBackend::None => "no-gui",
    };
    format!(
        "VimCode {} ({}, {})",
        env!("CARGO_PKG_VERSION"),
        vimcode_core::quadraui_pin::version_line(),
        backend,
    )
}

/// Launch the compiled-in GUI backend, or the terminal UI if there isn't one.
///
/// The `cfg` lives on three alternative definitions rather than inside the
/// body so that each build only ever *names* the modules it actually has:
/// `vimcode_core::gtk` does not exist without `gui`, and
/// `vimcode_core::macos` does not exist without `macos` on a Mac.
#[cfg(all(feature = "gui", not(all(feature = "macos", target_os = "macos"))))]
fn launch_gui(args: Args) -> ExitCode {
    vimcode_core::gtk::run(args.file_path);
    ExitCode::SUCCESS
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn launch_gui(args: Args) -> ExitCode {
    // Unlike the GTK runner, `quadraui::macos::shell_runner::run_with_shell`
    // reports the AppKit event loop's exit status, so propagate it rather
    // than flattening every run to success.
    vimcode_core::macos::run(args.file_path)
}

#[cfg(not(any(feature = "gui", all(feature = "macos", target_os = "macos"))))]
fn launch_gui(args: Args) -> ExitCode {
    eprintln!(
        "vimcode: built without a GUI backend (no `gui`, no `macos`); \
         starting the terminal UI instead. Pass --tui to skip this notice."
    );
    vimcode_core::tui_main::run(args.file_path, args.debug_log);
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args = Args::parse(&argv);

    if args.version {
        println!("{}", version_banner(COMPILED_GUI_BACKEND));
        return ExitCode::SUCCESS;
    }

    if args.tui {
        vimcode_core::tui_main::run(args.file_path, args.debug_log);
        return ExitCode::SUCCESS;
    }

    launch_gui(args)
}

#[cfg(test)]
mod arg_parsing_tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        std::iter::once("vimcode")
            .chain(items.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn bare_invocation_parses_to_defaults() {
        assert_eq!(Args::parse(&argv(&[])), Args::default());
    }

    #[test]
    fn first_positional_is_the_file_path() {
        let a = Args::parse(&argv(&["src/main.rs", "ignored.rs"]));
        assert_eq!(a.file_path, Some(PathBuf::from("src/main.rs")));
    }

    #[test]
    fn debug_value_is_not_mistaken_for_the_file_path() {
        // The regression this test pins: `--debug`'s value is a bare word, so
        // a naive "first argument not starting with '-'" scan picks it up as
        // the file to open.
        let a = Args::parse(&argv(&["--debug", "/tmp/vc.log", "notes.txt"]));
        assert_eq!(a.debug_log.as_deref(), Some("/tmp/vc.log"));
        assert_eq!(a.file_path, Some(PathBuf::from("notes.txt")));
    }

    #[test]
    fn flags_are_recognised_in_both_spellings() {
        assert!(Args::parse(&argv(&["--tui"])).tui);
        assert!(Args::parse(&argv(&["-t"])).tui);
        assert!(Args::parse(&argv(&["--version"])).version);
        assert!(Args::parse(&argv(&["-V"])).version);
    }

    #[test]
    fn dangling_debug_flag_does_not_panic() {
        let a = Args::parse(&argv(&["--debug"]));
        assert_eq!(a.debug_log, None);
        assert_eq!(a.file_path, None);
    }
}

#[cfg(test)]
mod gui_backend_tests {
    use super::*;

    /// #859/#645: the `vimcode` bin target must compile — and therefore run
    /// its tests — in *every* feature set, including the GUI-less ones. This
    /// test existing at all is most of the point: with the old
    /// `required-features = ["gui"]`, `cargo test --no-default-features` and
    /// `cargo test --no-default-features --features macos` both dropped this
    /// whole target and reported a green "2 test binaries" run. If this
    /// module stops appearing in a lane's output, the trap is back.
    #[test]
    fn the_app_bin_is_built_in_this_feature_set() {
        // Deliberately near-trivial: the real assertion is that this test
        // *ran at all*, which cargo has already proved by compiling the
        // target. The body just pins that exactly one backend is selected.
        assert!(matches!(
            COMPILED_GUI_BACKEND,
            GuiBackend::Gtk | GuiBackend::MacOs | GuiBackend::None
        ));
    }

    /// The compiled-in backend matches the feature set, and exactly one arm
    /// of the `cfg` cascade is live.
    #[test]
    fn compiled_backend_matches_the_feature_set() {
        let expected = if cfg!(all(feature = "macos", target_os = "macos")) {
            GuiBackend::MacOs
        } else if cfg!(feature = "gui") {
            GuiBackend::Gtk
        } else {
            GuiBackend::None
        };
        assert_eq!(COMPILED_GUI_BACKEND, expected);
    }

    /// The version banner names the backend, so "which vimcode is this?" is
    /// answerable from `vimcode --version` alone rather than from the build
    /// command that produced it.
    #[test]
    fn version_banner_names_version_quadraui_and_backend() {
        for (backend, tag) in [
            (GuiBackend::Gtk, "gtk"),
            (GuiBackend::MacOs, "macos"),
            (GuiBackend::None, "no-gui"),
        ] {
            let banner = version_banner(backend);
            assert!(
                banner.starts_with(&format!("VimCode {}", env!("CARGO_PKG_VERSION"))),
                "got {banner:?}"
            );
            assert!(banner.contains("quadraui "), "got {banner:?}");
            assert!(banner.ends_with(&format!(", {tag})")), "got {banner:?}");
        }
    }

    /// `--version` must win over `--tui`: printing the banner and exiting is
    /// not something you want to have to escape a terminal UI to read.
    #[test]
    fn version_takes_precedence_over_tui() {
        let a = Args::parse(
            &["vimcode", "--tui", "--version"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
        assert!(a.version && a.tui);
    }
}
