//! Thin GUI-binary shim over `vimcode_core`.
//!
//! Everything this binary declares lives in the library crate so that
//! integration tests under `tests/` can reach the UI backends and their
//! black-box harnesses. See `src/lib.rs`.
//!
//! # Backend selection
//!
//! The decision is made at compile time in [`COMPILED_GUI_BACKEND`],
//! ensuring the target compiles in every feature set.
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
//! regardless.
//!
//! `Win`'s row reads "on Windows", not "with `win`", because unlike `macos`
//! (target-gated inside quadraui itself, so `--features macos` compiles
//! nothing on Linux), quadraui's `win` module type-checks on every host.
//! `--features win` alone therefore compiles `vimcode_core::win` everywhere,
//! but this file still only ever *launches* it on `target_os = "windows"`.
//! Launching it elsewhere would reach `quadraui::win::run::run_with`'s
//! non-Windows stub, which `todo!()`s rather than degrading gracefully the
//! way the GTK/TUI paths do.

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
    /// a macOS host. See [`COMPILED_GUI_BACKEND`]'s doc comment for
    /// the precedence rule against `Gtk` and why `--features gui,macos` is
    /// untested; build the native app as `--no-default-features --features
    /// macos`, which is also the only shape macmini can build (no Homebrew/
    /// gtk4/pkg-config there by policy).
    MacOs,
    /// Native Direct2D/Win32 via `vimcode_core::win::run` — the `win`
    /// feature on a Windows host. Same precedence posture as
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
    /// `--ext-install <name>`: headless prerequisite check and install
    /// for one extension, without touching a GUI/TUI backend.
    ext_install: Option<String>,
    /// `--json`: with `--ext-install`, print the result as one JSON object
    /// instead of plain text.
    json: bool,
    /// `--skin <FILE>`: load chrome proportions and visual tokens from this
    /// TOML skin file instead of vimcode's built-in ones.
    skin: Option<String>,
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

        // --ext-install <name>: headless prerequisite check / install
        let ext_install_flag = argv.iter().position(|a| a == "--ext-install");
        let ext_install = ext_install_flag.and_then(|i| argv.get(i + 1)).cloned();
        let json = argv.iter().any(|a| a == "--json");

        // --skin <file>: load chrome proportions from a TOML skin file
        let skin_flag = argv.iter().position(|a| a == "--skin");
        let skin = skin_flag.and_then(|i| argv.get(i + 1)).cloned();

        // First positional argument (not starting with '-', not a --debug,
        // --ext-install or --skin value)
        let skip_args: std::collections::HashSet<usize> = {
            let mut s = std::collections::HashSet::new();
            for flag in [debug_flag, ext_install_flag, skin_flag]
                .into_iter()
                .flatten()
            {
                s.insert(flag);
                s.insert(flag + 1);
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
            skin,
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
         \x20\x20    --json          With --ext-install, print the result as JSON\n\
         \x20\x20    --skin <FILE>   Load chrome proportions and visual tokens from a \
TOML skin file\n\n\
         Arguments:\n\
         \x20\x20[FILE]              File to open on startup\n",
        env!("CARGO_PKG_VERSION"),
    )
}

/// The version banner, including which quadraui this binary is made of
/// and which GUI backend is compiled in.
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
/// exists on every host under `win`, this function only ever *calls*
/// `vimcode_core::win::run` on `target_os = "windows"`.
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

/// Headless `--ext-install <name>` entry point.
///
/// Runs the same prerequisite check as `Engine::ext_install_from_registry_
/// with_runtime_check` runs before dispatching an install, with no GUI/TUI
/// backend involved. Prints a JSON (`--json`) or plain-text verdict and exits
/// non-zero the moment a prerequisite is missing, **without** dispatching
/// the doomed install.
///
/// Once every declared prerequisite is present, this runs the resolved install
/// command synchronously and reports its exit status. It does not verify the
/// installation by speaking LSP/DAP `initialize` against the freshly installed
/// server/adapter; that verification is left to external tests with the
/// per-language client fixtures to drive it.
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

    // The headless `--ext-install` entry point must refuse an incompatible
    // extension the same way the interactive path does, so the two paths
    // do not diverge.
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

    // The same gating applies before checking `lsp.dependencies`/
    // `dap.dependencies`: native `[lsp.acquire]`/`[dap.acquire]` tables have
    // their own runtime check and are not this function's concern, and there
    // is nothing to check if no install command exists at all.
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
/// could not be spawned or had no exit code). The shell choice mirrors the
/// install pane's: `sh` on Unix, Windows PowerShell on Windows.
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

    // Install the skin before any backend starts, so the first painted frame
    // already uses its proportions. One call site for every backend: the
    // shared render layer reads the active skin, so no backend knows a skin
    // file exists.
    if let Some(path) = &args.skin {
        if let Err(e) = vimcode_core::render::activate_skin_file(std::path::Path::new(path)) {
            eprintln!("vimcode: {e}");
            return ExitCode::FAILURE;
        }
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

    /// `--help` must not be mistaken for a positional file argument —
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

    /// `--ext-install <name>` takes its value, same as `--debug`
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

    /// `--skin <FILE>` takes its value, and that value — a bare path, the
    /// same shape as the file to open — must not be taken as the positional
    /// file argument.
    #[test]
    fn skin_value_is_not_mistaken_for_the_file_path() {
        let a = Args::parse(&argv(&["--skin", "compact.toml", "notes.txt"]));
        assert_eq!(a.skin.as_deref(), Some("compact.toml"));
        assert_eq!(a.file_path, Some(PathBuf::from("notes.txt")));
    }

    /// Several valued flags at once: each keeps its own value and none of
    /// them swallows another's.
    #[test]
    fn skin_parses_alongside_the_other_valued_flags() {
        let a = Args::parse(&argv(&[
            "--debug",
            "/tmp/vc.log",
            "--skin",
            "/etc/vimcode/skin.toml",
            "main.rs",
        ]));
        assert_eq!(a.debug_log.as_deref(), Some("/tmp/vc.log"));
        assert_eq!(a.skin.as_deref(), Some("/etc/vimcode/skin.toml"));
        assert_eq!(a.file_path, Some(PathBuf::from("main.rs")));
    }

    #[test]
    fn dangling_skin_flag_does_not_panic() {
        let a = Args::parse(&argv(&["--skin"]));
        assert_eq!(a.skin, None);
        assert_eq!(a.file_path, None);
    }

    /// `--skin` is advertised in the usage text, so `--help` tells a user
    /// the flag exists.
    #[test]
    fn usage_text_documents_the_skin_flag() {
        assert!(usage_text().contains("--skin <FILE>"));
    }
}

/// Black-box, backend-neutral coverage for `vimcode <file>`: driven through
/// `--tui`'s shared `vimcode_core::tui_main::testing` seam, which runs the
/// exact same `App::new_portable`/`Engine::startup` path every backend
/// (GTK, macOS, Windows, TUI) does — so this covers all of them without any
/// per-backend code.
#[cfg(test)]
mod file_argument_launch_tests {
    use std::path::PathBuf;

    use quadraui::testing::ConformanceDriver;
    use vimcode_core::icons::{TREE_CHEVRON_DOWN, TREE_CHEVRON_RIGHT};
    use vimcode_core::tui_main::testing::{tui_driver, tui_driver_with};

    /// Whether the Explorer tree's own expand/collapse chevron is painted
    /// anywhere on screen — a content-area signal driven by `Engine`'s
    /// shadow `app_shell` (sidebar visibility + active panel), unlike the
    /// chrome header text `" EXPLORER "` which only paints after a genuine
    /// click/dispatch lands on the runner's own separate `AppShell` and so
    /// never appears on a bare first frame either way. Every directory row
    /// in the tree paints one of these chevrons regardless of expansion
    /// state, so this is a reliable "is the tree painted at all" oracle
    /// that doesn't depend on which files happen to be in the tree.
    ///
    /// Checks both the Nerd Font and ASCII-fallback forms of each glyph,
    /// since `use_nerd_fonts` defaults to `true` on non-Windows TUI.
    fn explorer_tree_chevron_painted<D: ConformanceDriver>(driver: &D) -> bool {
        driver.inventory().text_runs().iter().any(|r| {
            r.text == TREE_CHEVRON_DOWN.nerd
                || r.text == TREE_CHEVRON_DOWN.fallback
                || r.text == TREE_CHEVRON_RIGHT.nerd
                || r.text == TREE_CHEVRON_RIGHT.fallback
        })
    }

    /// `vimcode <file>` must open `<file>` for editing whether it names a
    /// relative path into another directory or an absolute path. A bare
    /// `vimcode` with no file argument is unaffected and must keep today's
    /// default sidebar visibility.
    ///
    /// The "explorer starts collapsed when a file argument is given" half
    /// of this same requirement is covered separately by
    /// [`file_argument_on_launch_collapses_explorer`] below, which is
    /// `#[ignore]`d pending a quadraui capability this crate's published
    /// version doesn't have yet — see that test's doc.
    #[test]
    fn file_argument_opens_relative_and_absolute_paths_bare_launch_keeps_default() {
        // This bin's own `#[cfg(test)]` module links `vimcode_core` as an
        // ordinary dependency (no `cfg(test)` on the lib side), so the
        // `set_test_home()` thread-local sandbox is unreachable here; opt
        // out of disk writes the same way a `tests/*.rs` integration test
        // does instead.
        vimcode_core::core::session::suppress_disk_saves();

        let unique = format!(
            "vc2105_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        );

        // (a) Relative path that escapes the crate root via a leading `..`,
        // so the path is genuinely outside `cargo test`'s process CWD and
        // `adopt_cwd_for_startup_file`'s "is this path already under cwd"
        // early-return is not taken — the real cwd-adoption logic this
        // issue is about actually runs. (A `target/`-nested relative path
        // would stay under cwd and never exercise that logic.)
        let rel_scratch_root = PathBuf::from("..").join(format!("{unique}_rel"));
        let rel_dir = rel_scratch_root.join("notes");
        std::fs::create_dir_all(&rel_dir).expect("create relative scratch dir");
        const REL_MARKER: &str = "VC2105_RELATIVE_MARKER";
        std::fs::write(rel_dir.join("todo.txt"), REL_MARKER).expect("write relative scratch file");
        let rel_path = rel_dir.join("todo.txt");

        {
            let mut driver = tui_driver(Some(rel_path), 100, 30);
            driver.render();
            let screen = driver.screen();
            assert!(
                screen.contains(REL_MARKER),
                "a relative path into another directory must open that \
                 file's contents; screen:\n{screen}"
            );
        }
        let _ = std::fs::remove_dir_all(&rel_scratch_root);

        // (b) Absolute path: same assertion.
        let abs_dir = std::env::temp_dir().join(format!("{unique}_abs"));
        std::fs::create_dir_all(&abs_dir).expect("create absolute scratch dir");
        const ABS_MARKER: &str = "VC2105_ABSOLUTE_MARKER";
        let abs_path = abs_dir.join("scratch.txt");
        std::fs::write(&abs_path, ABS_MARKER).expect("write absolute scratch file");

        {
            let mut driver = tui_driver(Some(abs_path), 100, 30);
            driver.render();
            let screen = driver.screen();
            assert!(
                screen.contains(ABS_MARKER),
                "an absolute path must open that file's contents; screen:\n{screen}"
            );
        }
        let _ = std::fs::remove_dir_all(&abs_dir);

        // (c) Regression guard: a bare launch with no file argument must
        // still show the sidebar whenever settings/session say it should.
        // Forced to a known state via the `setup` hook plus the engine's
        // own public re-derivation, rather than trusting whatever happens
        // to be ambient in `~/.config/vimcode` on the machine running this
        // test.
        let mut driver = tui_driver_with(None, 100, 30, |engine| {
            engine.settings.autohide_panels = false;
            engine.settings.explorer_visible_on_startup = true;
            engine.session.explorer_visible = false;
            engine.sync_app_shell_sidebar_visibility();
        });
        // The runner's own `AppShell` (what actually paints) only learns
        // the shadow's sidebar visibility once an event has dispatched
        // through it (`App::handle`'s `sync_runner_sidebar_visibility`
        // call, gated on `menu_bar_toggleable` which TUI's `setup` always
        // sets) — a bare `driver.render()` with no prior dispatch paints
        // `quadraui::AppShell::new`'s construction-time default
        // regardless of engine state, which would make this assertion
        // pass unconditionally and catch nothing. `Escape` is a no-op in
        // Normal mode, so this dispatch doesn't itself change what should
        // be on screen.
        driver.press_named(quadraui::NamedKey::Escape);
        driver.render();
        assert!(
            explorer_tree_chevron_painted(&*driver),
            "a bare launch with no file argument must keep today's \
             default sidebar visibility (regression guard), so the \
             Explorer tree's chevrons must be painted; screen:\n{}",
            driver.screen()
        );
    }

    /// A file argument on launch must start with the explorer collapsed —
    /// matching `nvim <file>` / `code <file>` rather than showing whatever
    /// the ambient `explorer_visible_on_startup` setting would otherwise
    /// display.
    ///
    /// `#[ignore]`d: blocked on quadraui. `Engine::startup_inner` does not
    /// touch its own shadow `app_shell`'s sidebar bit on a file-argument
    /// launch either, because doing so would only ever reach the real,
    /// painting runner `AppShell` via `sync_runner_sidebar_visibility`
    /// (`src/render/menu_and_chrome_routing.rs`), which itself only runs
    /// from inside `ShellApp::handle`/`on_shell_event_ctx` — both reachable
    /// only once a `UiEvent` has already dispatched through a live
    /// `ShellContext`. The very first frame renders before any event
    /// dispatches (`quadraui::tui::run::run_with`'s loop calls `app.setup()`
    /// once, then paints, before ever calling `app.handle()`), and
    /// `quadraui::compose::app_shell::AppShell::new` hardcodes
    /// `sidebar_visible: true` with no builder or `ShellConfig` field to
    /// seed a different initial value — confirmed absent in both the
    /// published 0.6.1 this crate depends on and the newer 0.7.0. There is
    /// currently no quadraui-published mechanism for an app to seed the
    /// real `AppShell`'s initial chrome state before its first paint, so
    /// flipping the shadow alone would only desync it further from the
    /// runner without changing anything the user sees.
    #[test]
    #[ignore = "blocked on quadraui: AppShell::new has no way to seed \
                initial sidebar visibility before the first render"]
    fn file_argument_on_launch_collapses_explorer() {
        vimcode_core::core::session::suppress_disk_saves();

        let unique = format!(
            "vc2105_collapse_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join("scratch.txt");
        std::fs::write(&path, "x").expect("write scratch file");

        let mut driver = tui_driver(Some(path), 100, 30);
        driver.render();
        let screen = driver.screen();
        assert!(
            !explorer_tree_chevron_painted(&*driver),
            "a file argument on launch must start with the explorer \
             collapsed, so the Explorer tree's chevrons must not be \
             painted; screen:\n{screen}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod gui_backend_tests {
    use super::*;

    /// The `vimcode` bin target must compile — and therefore run
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
