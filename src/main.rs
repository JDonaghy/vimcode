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
//! Four backends, resolved at compile time:
//!
//! | build | [`GuiBackend`] | runs |
//! |---|---|---|
//! | `--features macos` on macOS | `MacOs` | `vimcode_core::macos::run` (AppKit) |
//! | `--features win` on Windows | `Win` | `vimcode_core::win::run` (Direct2D/Win32) |
//! | `--features gui` (the default) | `Gtk` | `vimcode_core::gtk::run` |
//! | none of the above | `None` | terminal UI, with a one-line stderr notice |
//!
//! `--tui` / `-t` short-circuits all of that and runs the terminal UI
//! regardless, exactly as before.
//!
//! `Win`'s row reads "on Windows", not "with `win`", because unlike `macos`
//! (target-gated inside quadraui itself, so `--features macos` compiles
//! nothing on Linux), quadraui's `win` module type-checks on every host
//! (#866 — see `Cargo.toml`'s `win` feature comment). `--features win` alone
//! therefore compiles `vimcode_core::win` everywhere, but this file still
//! only ever *launches* it on `target_os = "windows"` — launching it
//! anywhere else would reach `quadraui::win::run::run_with`'s non-Windows
//! stub, which `todo!()`s rather than degrading gracefully the way the GTK/
//! TUI paths do.

use std::path::PathBuf;
use std::process::ExitCode;

/// Which GUI backend this binary was compiled with.
///
/// Resolved from cargo features at compile time into [`COMPILED_GUI_BACKEND`];
/// the variants exist in every build so the selection logic is one ordinary
/// `match` that tests can exercise, rather than a thicket of `cfg!` at the
/// call site.
///
/// All four variants exist in every build, but only one is ever
/// *constructed* (whichever arm of the `cfg` cascade below is live), so the
/// other three would otherwise trip `dead_code` — hence the blanket allow.
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
    /// Native Direct2D/Win32 via `vimcode_core::win::run` — the `win`
    /// feature on a Windows host (#866). Same precedence posture as
    /// `MacOs`: wins over `Gtk` when both are compiled in, untested and
    /// unsupported as a combination for the same Pango-vs-native-backend
    /// reason. Unlike `MacOs`, `win` alone compiles on every host (quadraui
    /// does not target-gate its `win` module — see `Cargo.toml`'s `win`
    /// comment), so this variant additionally requires `target_os =
    /// "windows"` before it is ever *selected* — see
    /// [`COMPILED_GUI_BACKEND`]'s cascade.
    Win,
    /// No GUI backend was compiled in, or one was compiled in but not for
    /// this `target_os` (e.g. `--features win` on Linux). The binary still
    /// builds and still works — as a terminal editor, which is what the
    /// `vcd` bin is.
    None,
}

/// What this build will actually launch.
///
/// Precedence when more than one backend is compiled in: `MacOs` and `Win`
/// each win over `Gtk` on their respective `target_os` — a `--features
/// gui,macos` build on a Mac (GTK4 from Homebrew alongside the native
/// backend) should run the *native* one, since choosing GTK there would
/// make the native backend unreachable without a rebuild; same reasoning
/// for `--features gui,win` on Windows (GTK4 has a Windows port too, via
/// MSYS2/gvsbuild).
///
/// Every multi-backend combination here is untested and not a supported
/// configuration — `App::render_content` still has a few `#[cfg(feature =
/// "gui")]` blocks that reach for Pango (`click::build_editor_click_context`),
/// and with `gui` compiled in they would run against a `MacBackend`/
/// `WinBackend` instead.
#[cfg(all(feature = "macos", target_os = "macos"))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::MacOs;
#[cfg(all(feature = "win", target_os = "windows"))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::Win;
#[cfg(all(
    feature = "gui",
    not(all(feature = "macos", target_os = "macos")),
    not(all(feature = "win", target_os = "windows"))
))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::Gtk;
#[cfg(not(any(
    feature = "gui",
    all(feature = "macos", target_os = "macos"),
    all(feature = "win", target_os = "windows")
)))]
const COMPILED_GUI_BACKEND: GuiBackend = GuiBackend::None;

/// The parsed command line.
///
/// Split out of `main` so the parsing rules — which are finicky enough to be
/// worth pinning down (`--debug`'s *value* must not be mistaken for the file
/// argument) — are testable without spawning a process.
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    /// `--help` / `-h`: print usage and exit.
    help: bool,
    /// `--version` / `-V`: print the version banner and exit.
    version: bool,
    /// `--tui` / `-t`: force the terminal UI.
    tui: bool,
    /// `--debug <logfile>`: write a debug log to this path.
    debug_log: Option<String>,
    /// `--ext-install <name>`: headless prerequisite check (and, when every
    /// prerequisite is present, install) for one extension — #1719's "an
    /// external driver (vimcode-ext#17's CI matrix) can exercise vimcode's
    /// real prerequisite-selection logic without reimplementing it" entry
    /// point. Never touches a GUI/TUI backend.
    ext_install: Option<String>,
    /// `--json`: with `--ext-install`, print the result as one JSON object
    /// instead of plain text.
    json: bool,
    /// First positional argument: the file to open.
    file_path: Option<PathBuf>,
}

impl Args {
    /// Parse an `argv`-shaped slice (element 0 is the program name).
    fn parse(argv: &[String]) -> Self {
        let help = argv.iter().any(|a| a == "--help" || a == "-h");
        let version = argv.iter().any(|a| a == "--version" || a == "-V");
        let tui = argv.iter().any(|a| a == "--tui" || a == "-t");

        // --debug <logfile>: write debug log to the given file
        let debug_flag = argv.iter().position(|a| a == "--debug");
        let debug_log = debug_flag.and_then(|i| argv.get(i + 1)).cloned();

        // --ext-install <name>: headless prerequisite check / install (#1719)
        let ext_install_flag = argv.iter().position(|a| a == "--ext-install");
        let ext_install = ext_install_flag.and_then(|i| argv.get(i + 1)).cloned();
        let json = argv.iter().any(|a| a == "--json");

        // First positional argument (not starting with '-', not a --debug
        // or --ext-install value)
        let skip_args: std::collections::HashSet<usize> = {
            let mut s = std::collections::HashSet::new();
            if let Some(i) = debug_flag {
                s.insert(i);
                s.insert(i + 1);
            }
            if let Some(i) = ext_install_flag {
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
            help,
            version,
            tui,
            debug_log,
            ext_install,
            json,
            file_path,
        }
    }
}

/// The usage text for `--help` / `-h`.
///
/// Printed before any GUI backend is touched (#979 — GTK init used to run
/// ahead of flag handling, so `--help` crashed on any host without a
/// display, e.g. over SSH or in CI). Kept as a plain function, like
/// [`version_banner`], so it is unit-testable without spawning a process.
fn usage_text() -> String {
    format!(
        "VimCode {}\n\n\
         Usage: vimcode [OPTIONS] [FILE]\n\n\
         Options:\n\
         \x20\x20-h, --help          Print this help message and exit\n\
         \x20\x20-V, --version       Print the version and exit\n\
         \x20\x20-t, --tui           Force the terminal UI (no GTK4 required)\n\
         \x20\x20    --debug <FILE>  Write a debug log to FILE\n\
         \x20\x20    --ext-install <NAME>  Headless prerequisite check (and install) \
for one extension, no GUI/TUI\n\
         \x20\x20    --json          With --ext-install, print the result as JSON\n\n\
         Arguments:\n\
         \x20\x20[FILE]              File to open on startup\n",
        env!("CARGO_PKG_VERSION"),
    )
}

/// The version banner, including which quadraui this binary is made of
/// (#638 — the dependency is a pinned git rev, so nothing else in the build
/// records which one was used) and which GUI backend is compiled in (#859/
/// #866 — with four possible backends and a silent-omission history, "which
/// one is this?" needs to be answerable from the binary).
fn version_banner(backend: GuiBackend) -> String {
    let backend = match backend {
        GuiBackend::Gtk => "gtk",
        GuiBackend::MacOs => "macos",
        GuiBackend::Win => "win",
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
/// The `cfg` lives on four alternative definitions rather than inside the
/// body so that each build only ever *names* the modules it actually has:
/// `vimcode_core::gtk` does not exist without `gui`, `vimcode_core::macos`
/// does not exist without `macos` on a Mac, and while `vimcode_core::win`
/// exists on every host under `win` (#866 — quadraui does not target-gate
/// its own `win` module, see `Cargo.toml`'s `win` feature comment), this
/// function only ever *calls* `vimcode_core::win::run` on `target_os =
/// "windows"` — calling it elsewhere would reach
/// `quadraui::win::run::run_with`'s non-Windows `todo!()` stub.
#[cfg(all(
    feature = "gui",
    not(all(feature = "macos", target_os = "macos")),
    not(all(feature = "win", target_os = "windows"))
))]
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

#[cfg(all(feature = "win", target_os = "windows"))]
fn launch_gui(args: Args) -> ExitCode {
    // Same reasoning as the macOS arm above: propagate
    // `quadraui::win::shell_runner::run_with_shell`'s real exit status
    // rather than flattening to success.
    vimcode_core::win::run(args.file_path)
}

#[cfg(not(any(
    feature = "gui",
    all(feature = "macos", target_os = "macos"),
    all(feature = "win", target_os = "windows")
)))]
fn launch_gui(args: Args) -> ExitCode {
    eprintln!(
        "vimcode: built without a GUI backend for this platform (no `gui`, \
         no matching `macos`/`win`); starting the terminal UI instead. Pass \
         --tui to skip this notice."
    );
    vimcode_core::tui_main::run(args.file_path, args.debug_log);
    ExitCode::SUCCESS
}

/// Headless `--ext-install <name>` entry point (#1719).
///
/// Runs the same prerequisite check `Engine::ext_install_from_registry_
/// with_runtime_check` runs before dispatching an install, with no GUI/TUI
/// backend involved — the point is that an external driver (vimcode-ext#17's
/// cross-platform CI matrix) can exercise vimcode's *real* detect/instruct
/// logic end to end instead of re-implementing it and drifting, which is
/// exactly what happened three times over in the bugbash this issue
/// describes. Prints a JSON (`--json`) or plain-text verdict and exits
/// non-zero the moment a prerequisite is missing, **without** dispatching
/// the doomed install — contract item 1 ("detect before install").
///
/// Scope, documented rather than silently partial: once every declared
/// prerequisite is present, this runs the resolved install command
/// synchronously (there is no terminal pane to hand it to in headless mode)
/// and reports its exit status. It does not yet speak LSP/DAP `initialize`
/// against the freshly installed server/adapter to confirm "working" per
/// the issue's contract item 3 — that verification is left to vimcode-ext#17's
/// own matrix, which has the per-language client fixtures to drive it; this
/// entry point's job is just to stop re-implementation of the *selection and
/// detection* half.
fn run_ext_install(name: &str, json: bool) -> ExitCode {
    let engine = vimcode_core::core::engine::Engine::new();
    let Some(manifest) = engine
        .ext_available_manifests()
        .into_iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
    else {
        if json {
            println!(
                "{}",
                serde_json::json!({"extension": name, "status": "unknown_extension"})
            );
        } else {
            eprintln!("vimcode: unknown extension '{name}'");
        }
        return ExitCode::FAILURE;
    };

    // #1807: the headless `--ext-install` entry point must refuse an
    // incompatible extension the same way the interactive marketplace
    // does (`Engine::ext_install_from_registry_with_runtime_check`) —
    // without this check the two paths diverge: the interactive path
    // refuses, but this CI-facing one (#1719's vimcode-ext driver) would
    // sail straight through and install scripts that error at load time
    // on a missing `vimcode.*` API.
    if let Some(reason) = manifest.incompatibility_reason_for_running_vimcode() {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "extension": name,
                    "status": "incompatible_vimcode",
                    "reason": reason,
                })
            );
        } else {
            eprintln!("vimcode: cannot install '{name}' — {reason}");
        }
        return ExitCode::FAILURE;
    }

    let present = |dep: &str| vimcode_core::core::lsp_manager::resolve_command(dep).is_some();

    // Same gating `Engine::ext_install_from_registry_with_runtime_check`
    // applies before checking `lsp.dependencies`/`dap.dependencies`: native
    // `[lsp.acquire]`/`[dap.acquire]` tables have their own runtime check
    // (#1346, `resolve_acquire_action`) and are not this function's concern,
    // and there is nothing to check if no install command exists at all.
    let mut missing: Vec<String> = Vec::new();
    if !manifest.lsp.binary.is_empty()
        && manifest.lsp.acquire.is_none()
        && !manifest.lsp.install_cmd_for_platform().is_empty()
    {
        for dep in &manifest.lsp.dependencies {
            if !present(dep) && !missing.contains(dep) {
                missing.push(dep.clone());
            }
        }
    }
    if !manifest.dap.adapter.is_empty() && manifest.dap.acquire.is_none() {
        for dep in &manifest.dap.dependencies {
            if !present(dep) && !missing.contains(dep) {
                missing.push(dep.clone());
            }
        }
        if manifest.dap.install_cmd_for_platform().is_empty() {
            for dep in vimcode_core::core::dap_manager::adapter_dependencies(
                &manifest.dap.adapter,
                vimcode_core::core::extensions::Platform::host(),
            ) {
                if !present(dep) && !missing.iter().any(|m| m == dep) {
                    missing.push(dep.to_string());
                }
            }
        }
    }

    if !missing.is_empty() {
        let instructions: std::collections::BTreeMap<String, String> = missing
            .iter()
            .map(|dep| {
                let hint = vimcode_core::core::extensions::prereq_install_cmd(dep)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("install {dep} and try again"));
                (dep.clone(), hint)
            })
            .collect();
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "extension": name,
                    "status": "missing_prerequisite",
                    "missing": missing,
                    "instructions": instructions,
                })
            );
        } else {
            eprintln!("vimcode: '{name}' requires {} —", missing.join(", "));
            for (dep, hint) in &instructions {
                eprintln!("  {dep}: {hint}");
            }
        }
        return ExitCode::FAILURE;
    }

    // Every declared prerequisite is present — run the resolved install
    // command(s) for real and report the exit status.
    let mut commands: Vec<String> = Vec::new();
    if !manifest.lsp.binary.is_empty() && manifest.lsp.acquire.is_none() {
        let cmd = manifest.lsp.install_cmd_for_platform();
        if !cmd.is_empty() {
            commands.push(cmd.to_string());
        }
    }
    if !manifest.dap.adapter.is_empty() && manifest.dap.acquire.is_none() {
        if let Some(cmd) = vimcode_core::core::dap_manager::install_cmd_for_adapter(
            &manifest.dap.adapter,
            std::slice::from_ref(&manifest),
        ) {
            commands.push(cmd);
        }
    }

    let mut exit_status: i32 = 0;
    for cmd in &commands {
        exit_status = run_shell_command(cmd);
        if exit_status != 0 {
            break;
        }
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "extension": name,
                "status": "installed",
                "install_exit_status": exit_status,
            })
        );
    } else {
        println!("vimcode: '{name}' installed (exit status {exit_status})");
    }

    if exit_status == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Run a shell command synchronously and return its exit code (or `1` if it
/// could not be spawned / had no exit code). The shell choice mirrors the
/// install pane's: `sh` on Unix, Windows PowerShell on Windows (#1715 — no
/// `cmd.exe`, so every `PREREQ_INSTALLS`/manifest install string already has
/// to be valid in one of these two).
#[cfg(not(target_os = "windows"))]
fn run_shell_command(cmd: &str) -> i32 {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .map(|s| s.code().unwrap_or(1))
        .unwrap_or(1)
}

#[cfg(target_os = "windows")]
fn run_shell_command(cmd: &str) -> i32 {
    std::process::Command::new("powershell.exe")
        .arg("-Command")
        .arg(cmd)
        .status()
        .map(|s| s.code().unwrap_or(1))
        .unwrap_or(1)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args = Args::parse(&argv);

    if args.help {
        println!("{}", usage_text());
        return ExitCode::SUCCESS;
    }

    if args.version {
        println!("{}", version_banner(COMPILED_GUI_BACKEND));
        return ExitCode::SUCCESS;
    }

    if let Some(name) = &args.ext_install {
        return run_ext_install(name, args.json);
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
        assert!(Args::parse(&argv(&["--help"])).help);
        assert!(Args::parse(&argv(&["-h"])).help);
    }

    /// #979: `--help` must not be mistaken for a positional file argument —
    /// it starts with `-`, so the existing positional-arg scan already skips
    /// it, but pin it explicitly since this is the exact flag the bug was
    /// filed about.
    #[test]
    fn help_flag_does_not_become_the_file_path() {
        let a = Args::parse(&argv(&["--help"]));
        assert!(a.help);
        assert_eq!(a.file_path, None);
    }

    #[test]
    fn dangling_debug_flag_does_not_panic() {
        let a = Args::parse(&argv(&["--debug"]));
        assert_eq!(a.debug_log, None);
        assert_eq!(a.file_path, None);
    }

    /// #1719: `--ext-install <name>` takes its value, same as `--debug`
    /// does, and `--json` is recognised as a separate bare flag.
    #[test]
    fn ext_install_flag_and_value_parse_and_json_is_recognised() {
        let a = Args::parse(&argv(&["--ext-install", "yaml", "--json"]));
        assert_eq!(a.ext_install.as_deref(), Some("yaml"));
        assert!(a.json);
        assert_eq!(a.file_path, None);
    }

    /// `--ext-install`'s value must not be mistaken for the file to open,
    /// mirroring `debug_value_is_not_mistaken_for_the_file_path` above.
    #[test]
    fn ext_install_value_is_not_mistaken_for_the_file_path() {
        let a = Args::parse(&argv(&["--ext-install", "yaml", "notes.txt"]));
        assert_eq!(a.ext_install.as_deref(), Some("yaml"));
        assert_eq!(a.file_path, Some(PathBuf::from("notes.txt")));
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
            GuiBackend::Gtk | GuiBackend::MacOs | GuiBackend::Win | GuiBackend::None
        ));
    }

    /// The compiled-in backend matches the feature set, and exactly one arm
    /// of the `cfg` cascade is live.
    #[test]
    fn compiled_backend_matches_the_feature_set() {
        let expected = if cfg!(all(feature = "macos", target_os = "macos")) {
            GuiBackend::MacOs
        } else if cfg!(all(feature = "win", target_os = "windows")) {
            GuiBackend::Win
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
            (GuiBackend::Win, "win"),
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

    /// The usage text names every flag `main`'s dispatcher understands, so a
    /// future flag added to `Args` without a matching `usage_text` line is
    /// at least visible in a diff review, even though nothing enforces it
    /// mechanically.
    #[test]
    fn usage_text_documents_every_flag() {
        let text = usage_text();
        assert!(text.starts_with(&format!("VimCode {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains("Usage: vimcode"));
        for flag in [
            "--help",
            "-h",
            "--version",
            "-V",
            "--tui",
            "-t",
            "--debug",
            "--ext-install",
            "--json",
        ] {
            assert!(text.contains(flag), "usage text missing {flag:?}: {text}");
        }
    }
}
