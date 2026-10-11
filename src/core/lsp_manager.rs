use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use super::extensions;
use super::lsp::{
    language_id_from_path, path_to_uri, LspEvent, LspServer, LspServerConfig, LspServerId,
    SemanticTokensLegend,
};

// ---------------------------------------------------------------------------
// Install diagnostics — always written to /tmp/vimcode-install.log
// ---------------------------------------------------------------------------

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn debug_log_path() -> PathBuf {
    std::env::temp_dir().join("vimcode-lsp-debug.log")
}

pub fn install_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(debug_log_path())
    {
        let _ = writeln!(f, "[{ts}] {msg}", ts = timestamp());
    }
}

// ---------------------------------------------------------------------------
// Cargo-bin proxy validation
// ---------------------------------------------------------------------------

/// Validate that a binary in `~/.cargo/bin/` actually works.
/// Rustup installs proxy executables for components that aren't installed yet;
/// these proxies exist on disk (as symlinks to `rustup` on Linux/macOS, as
/// shim `.exe` on Windows) but exit with an error like
/// "Unknown binary 'rust-analyzer' in official toolchain …" when the
/// component isn't installed.  A quick `--version` probe catches this so
/// vimcode falls through to the install path instead of trying to spawn
/// a broken proxy as an LSP server.
pub fn cargo_bin_probe_ok(path: &Path, binary: &str) -> bool {
    // Only probe binaries found in ~/.cargo/bin/ — that's where rustup
    // proxies live.  Binaries elsewhere are trusted as-is.
    let cargo_bin = super::paths::home_dir().join(".cargo").join("bin");
    let in_cargo_bin = path.parent().map(|p| p == cargo_bin).unwrap_or(false);
    if !in_cargo_bin {
        return true;
    }

    match cargo_bin_probe_output(path) {
        Ok(()) => true,
        Err(ProbeFailure::BadExit(stderr_line)) => {
            install_log(&format!(
                "[ext-check] PROBE FAILED for {binary} at {}: {stderr_line}",
                path.display(),
            ));
            false
        }
        Err(ProbeFailure::SpawnError(e)) => {
            install_log(&format!(
                "[ext-check] PROBE ERROR for {binary} at {}: {e}",
                path.display()
            ));
            false
        }
    }
}

/// Why a `~/.cargo/bin/` probe (`<binary> --version`) failed.
enum ProbeFailure {
    /// The process ran but exited non-zero; carries the first line of stderr.
    BadExit(String),
    /// The process could not even be spawned.
    SpawnError(std::io::Error),
}

/// Run `<path> --version` and report success/failure. Shared by
/// `cargo_bin_probe_ok` (which only needs a bool) and
/// `pinned_toolchain_hint_for_binary` (which needs the stderr to recover the
/// pinned toolchain's name for a better "not found" hint).
fn cargo_bin_probe_output(path: &Path) -> Result<(), ProbeFailure> {
    let mut cmd = crate::core::git::hidden_command(path);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(ProbeFailure::BadExit(
                    stderr.lines().next().unwrap_or("(no output)").to_string(),
                ))
            }
        }
        Err(e) => Err(ProbeFailure::SpawnError(e)),
    }
}

/// Parse the pinned toolchain name out of a rustup proxy's failure stderr,
/// e.g. `error: Unknown binary 'rust-analyzer' in official toolchain
/// '1.97.1-aarch64-apple-darwin'` -> `Some("1.97.1-aarch64-apple-darwin")`.
fn parse_official_toolchain_from_stderr(stderr: &str) -> Option<String> {
    let marker = "official toolchain '";
    let start = stderr.find(marker)? + marker.len();
    let rest = &stderr[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// If `binary` resolves to a broken rustup proxy in `~/.cargo/bin/` whose
/// failure message names a pinned toolchain, return that toolchain's name.
/// Supplies the directory-independent `rustup component add --toolchain
/// <ver> …` form alongside the primary hint.
fn pinned_toolchain_hint_for_binary(binary: &str) -> Option<String> {
    let candidate = super::paths::home_dir()
        .join(".cargo")
        .join("bin")
        .join(binary);
    match cargo_bin_probe_output(&candidate) {
        Err(ProbeFailure::BadExit(stderr_line)) => {
            parse_official_toolchain_from_stderr(&stderr_line)
        }
        _ => None,
    }
}

/// Build the "<binary> not found. Run: <install_cmd>" hint, naming the
/// workspace root the command needs to run in (install commands like `rustup
/// component add` are resolved against the *current directory's* pinned
/// toolchain, not the running process's), plus — when the binary is a broken
/// rustup proxy for a pinned toolchain — an explicit `--toolchain <ver>` form
/// that works from any directory.
fn not_found_hint(binary: &str, install_cmd: &str, workspace_root: &Path) -> String {
    let root_display = workspace_root.display();
    let mut hint = format!("{binary} not found. Run in {root_display}: {install_cmd}");
    if install_cmd.trim_start().starts_with("rustup component add") {
        if let Some(toolchain) = pinned_toolchain_hint_for_binary(binary) {
            hint.push_str(&format!(
                " (or from any directory: {install_cmd} --toolchain {toolchain})"
            ));
        }
    }
    hint
}

// ---------------------------------------------------------------------------
// Built-in server registry
// ---------------------------------------------------------------------------

/// Returns a list of well-known language server configurations.
/// These are auto-discovered on PATH at startup.
pub fn default_server_registry() -> Vec<LspServerConfig> {
    vec![
        LspServerConfig {
            command: "rust-analyzer".to_string(),
            args: vec![],
            languages: vec!["rust".to_string()],
            ..Default::default()
        },
        // Python — ordered fallbacks (first binary found on PATH/Mason wins)
        LspServerConfig {
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec!["python".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "basedpyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec!["python".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "pylsp".to_string(),
            args: vec![],
            languages: vec!["python".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "jedi-language-server".to_string(),
            args: vec![],
            languages: vec!["python".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec![
                "javascript".to_string(),
                "typescript".to_string(),
                "javascriptreact".to_string(),
                "typescriptreact".to_string(),
            ],

            ..Default::default()
        },
        LspServerConfig {
            command: "gopls".to_string(),
            args: vec![],
            languages: vec!["go".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "clangd".to_string(),
            args: vec![],
            languages: vec!["c".to_string(), "cpp".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "csharp-ls".to_string(),
            args: vec![],
            languages: vec!["csharp".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "lua-language-server".to_string(),
            args: vec![],
            languages: vec!["lua".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "bash-language-server".to_string(),
            args: vec!["start".to_string()],
            languages: vec!["shellscript".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "yaml-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec!["yaml".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "kotlin-language-server".to_string(),
            args: vec![],
            languages: vec!["kotlin".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "zls".to_string(),
            args: vec![],
            languages: vec!["zig".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "elixir-ls".to_string(),
            args: vec![],
            languages: vec!["elixir".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "ruby-lsp".to_string(),
            args: vec![],
            languages: vec!["ruby".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "terraform-ls".to_string(),
            args: vec!["serve".to_string()],
            languages: vec!["terraform".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "marksman".to_string(),
            args: vec!["server".to_string()],
            languages: vec!["markdown".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "taplo".to_string(),
            args: vec!["lsp".to_string(), "stdio".to_string()],
            languages: vec!["toml".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "sourcekit-lsp".to_string(),
            args: vec![],
            languages: vec!["swift".to_string()],

            ..Default::default()
        },
        LspServerConfig {
            command: "metals".to_string(),
            args: vec![],
            languages: vec!["scala".to_string()],

            ..Default::default()
        },
    ]
}

/// Build `LspServerConfig` candidates from a bundled extension manifest for a
/// given language ID.  Returns the primary binary first, then each fallback.
fn server_configs_from_manifest(
    manifest: &extensions::ExtensionManifest,
    language_id: &str,
) -> Vec<LspServerConfig> {
    if manifest.lsp.binary.is_empty() {
        return Vec::new();
    }
    // Use the manifest's args if set; otherwise empty.
    let args = manifest.lsp.args.clone();
    // Use all language IDs from the manifest so multi-language servers (e.g.
    // typescript-language-server for js + ts) map all their languages at once.
    let languages: Vec<String> = if manifest.language_ids.is_empty() {
        vec![language_id.to_string()]
    } else {
        manifest.language_ids.clone()
    };
    let init_opts = manifest.lsp.initialization_options.clone();
    let mut configs = Vec::new();
    configs.push(LspServerConfig {
        command: manifest.lsp.binary.clone(),
        args: args.clone(),
        languages: languages.clone(),
        initialization_options: init_opts.clone(),
    });
    for fb in &manifest.lsp.fallback_binaries {
        configs.push(LspServerConfig {
            command: fb.clone(),
            args: args.clone(),
            languages: languages.clone(),
            initialization_options: init_opts.clone(),
        });
    }
    configs
}

/// Return the Mason LSP binary directory if it exists.
/// On Linux/macOS: `$HOME/.local/share/nvim/mason/bin`
/// On Windows: `%APPDATA%\nvim-data\mason\bin`
fn mason_bin_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("HOME").map(PathBuf::from)?;

    #[cfg(target_os = "windows")]
    let dir = base.join("nvim-data").join("mason").join("bin");
    #[cfg(not(target_os = "windows"))]
    let dir = base
        .join(".local")
        .join("share")
        .join("nvim")
        .join("mason")
        .join("bin");

    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Homebrew formulas that are **keg-only** — Homebrew does not symlink their
/// binaries into `<prefix>/bin`, so the binary a manifest names doesn't match
/// the formula that ships it. `clangd` ships in the `llvm` formula, which is
/// keg-only because symlinking it into the prefix would shadow the
/// system-provided `/usr/bin/clang`. Extend this list if another
/// registry extension's `install_macos` hits the same problem.
///
/// Not `#[cfg(macos)]`-gated: it's inert data, and leaving it compiled on
/// every target lets the `VIMCODE_TEST_HOMEBREW_PREFIXES` test override (see
/// `homebrew_prefixes()`) exercise the exact same keg-only probe on any host
/// OS instead of a duplicated copy.
const HOMEBREW_KEG_ONLY_FORMULAS: &[(&str, &str)] = &[("clangd", "llvm")];

/// Homebrew prefix directories to probe for LSP/DAP binaries.
///
/// A native macOS `.app` launched from Finder/Dock/launchd gets launchd's
/// minimal PATH, which contains neither Homebrew prefix — Apple Silicon
/// symlinks into `/opt/homebrew`, Intel Macs into `/usr/local`. `brew
/// --prefix` is authoritative but costs a subprocess spawn on every LSP
/// resolve; checking both fixed candidates with a stat is cheap and covers
/// both architectures (only one will ever exist on a given machine).
///
/// On non-macOS targets this returns an empty list — Windows/Linux discovery
/// order is intentionally unchanged.
///
/// The `VIMCODE_TEST_HOMEBREW_PREFIXES` environment variable overrides the
/// probed prefixes with a `PATH`-style (`:`-separated) list of directories,
/// on **every** target including macOS. That override exists solely so
/// `tests/extensions.rs` can drive this resolution logic against a fake
/// Homebrew layout without touching (or depending on the contents of) a real
/// `/opt/homebrew`; real builds never set it.
///
/// The override is deliberately *exclusive* — when set, the real prefixes are
/// not probed — so a test can assert on exactly the layout it created.
fn homebrew_prefixes() -> Vec<PathBuf> {
    if let Some(val) = std::env::var_os("VIMCODE_TEST_HOMEBREW_PREFIXES") {
        return std::env::split_paths(&val).filter(|p| p.is_dir()).collect();
    }
    #[cfg(target_os = "macos")]
    {
        ["/opt/homebrew", "/usr/local"]
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Directories `resolve_command` probes for `binary` beyond `PATH`, in probe
/// order. Pulled out of `resolve_command` so a failure message can
/// name exactly where vimcode looked without hand-duplicating the list — which
/// would silently drift the moment `resolve_command` gains or drops a directory.
fn extra_tool_dirs(binary: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // Mason bin directory, if it exists.
    dirs.extend(mason_bin_dir());
    // Common tool directories that may not be in PATH when launched from a
    // desktop environment (not a login shell).
    let home = super::paths::home_dir();
    dirs.push(home.join(".dotnet/tools"));
    dirs.push(home.join(".cargo/bin"));
    dirs.push(home.join(".local/bin"));
    dirs.push(home.join("go/bin"));
    dirs.push(home.join(".npm-global/bin"));
    // Homebrew prefixes (macOS only — see `homebrew_prefixes()`).
    for prefix in homebrew_prefixes() {
        dirs.push(prefix.join("bin"));
        // Keg-only formulas (e.g. clangd/llvm) never reach `<prefix>/bin`;
        // probe `<prefix>/opt/<formula>/bin` too.
        for (kegged_binary, formula) in HOMEBREW_KEG_ONLY_FORMULAS {
            if *kegged_binary == binary {
                dirs.push(prefix.join("opt").join(formula).join("bin"));
            }
        }
    }
    dirs
}

/// Human-readable description of the directories `resolve_command` probes
/// for `binary`, for use in "installed but still not found" messages
/// — naming where vimcode actually looked is far more actionable than just
/// saying "not found on PATH", especially since most of these directories
/// are *not* on PATH for a desktop-launched vimcode in the first place.
pub fn probed_tool_dirs_description(binary: &str) -> String {
    let mut dirs: Vec<String> = vec![super::paths::managed_tool_dir(binary).display().to_string()];
    dirs.extend(
        extra_tool_dirs(binary)
            .into_iter()
            .map(|d| d.display().to_string()),
    );
    dirs.push("PATH".to_string());
    dirs.join(", ")
}

/// Resolve a command to an absolute path.
/// Checks Mason bin directory first (if it exists), then falls back to PATH.
///
/// `pub` (rather than crate-private) specifically so `tests/extensions.rs`
/// — a separate integration-test crate — can drive it directly.
/// Also the sole lookup shared by install-time checks, install finalization,
/// and server launch — see `crate::core::engine::binary_on_path`, which
/// delegates here instead of walking `PATH` on its own.
pub fn resolve_command(cmd: &str) -> Option<PathBuf> {
    // Split on whitespace to get just the binary name
    let binary = cmd.split_whitespace().next().unwrap_or(cmd);

    // The vimcode-managed tool acquisition dir is probed first — a
    // tool vimcode downloaded, verified and unpacked itself can never
    // collide with a same-named binary elsewhere on the system, and needs
    // no PATH/env changes to be found.
    if let Some(managed) = super::paths::managed_tool_binary_path(binary) {
        return Some(managed);
    }

    // Paths already probed (and rejected) in the loop below: `which`
    // frequently resolves to the exact same path the tool-dirs loop already
    // rejected (e.g. a broken rustup proxy in `~/.cargo/bin`), and probing
    // it again doubles the cost of a slow-to-fail probe for no new information.
    let mut already_probed: Vec<PathBuf> = Vec::new();

    for dir in extra_tool_dirs(binary) {
        let candidate = dir.join(binary);
        if candidate.exists() {
            if cargo_bin_probe_ok(&candidate, binary) {
                return Some(candidate);
            }
            already_probed.push(candidate);
        }
        // On Windows, also check with .exe suffix
        #[cfg(target_os = "windows")]
        if !binary.ends_with(".exe") {
            let exe = dir.join(format!("{binary}.exe"));
            if exe.exists() {
                if cargo_bin_probe_ok(&exe, binary) {
                    return Some(exe);
                }
                already_probed.push(exe);
            }
        }
    }

    // Fall back to PATH lookup via `which`/`where`
    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    let mut cmd = crate::core::git::hidden_command(which_cmd);
    cmd.arg(binary);
    let output = cmd.output().ok()?;
    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout);
        // `where` on Windows may return multiple lines — take the first
        let first_line = path_str.lines().next()?.trim();
        if !first_line.is_empty() {
            let resolved = PathBuf::from(first_line);
            // Compare canonicalized paths to avoid probing the same path twice:
            // `which`/`where` can resolve to a differently-formatted-but-equivalent
            // path to a candidate the tool-dirs loop already rejected (a symlink,
            // a relative vs. absolute form, etc.), which a plain string comparison
            // would miss. `canonicalize()` is a cheap stat-based syscall, not a
            // spawn, so falling back to the raw path on failure (e.g. it vanished
            // between `which` returning and this call) only loses the dedupe.
            let already_seen = already_probed.contains(&resolved)
                || resolved.canonicalize().is_ok_and(|c| {
                    already_probed
                        .iter()
                        .any(|p| p.canonicalize().is_ok_and(|pc| pc == c))
                });
            if already_seen {
                return None;
            }
            // `which` happily resolves the rustup proxy in ~/.cargo/bin/
            // even after `rustup component remove`; probe to skip broken
            // proxies here too (the `tool_dirs` loop above probes only
            // when it finds the binary itself).
            if cargo_bin_probe_ok(&resolved, binary) {
                return Some(resolved);
            }
        }
    }
    None
}

/// Build the actionable "missing prerequisite" error message for a manifest,
/// given the dependency names that failed to resolve on PATH.
///
/// Pulled out of `ensure_server_for_language` as a pure function (no PATH
/// access) so tests can drive the message-building logic directly with a
/// synthetic `missing` list instead of depending on which of `npm`,
/// `dotnet`, `go`, etc. happen to be installed on the machine running the
/// test suite — mirrors the `install_cmd_with(rustup_available:
/// bool)` split already used in `extensions.rs` for the same reason.
///
/// `pub` (rather than crate-private) so `tests/extensions.rs` — a separate
/// integration-test crate — can call it directly.
pub fn missing_dependency_message(
    manifest: &extensions::ExtensionManifest,
    missing: &[&str],
) -> String {
    let name = manifest.display_or_name();
    // Naming the missing binary alone ("requires npm — install npm
    // and try again") tells the user what's missing but not what to
    // actually run. Attach a runnable, platform-specific command for the
    // prerequisites shared across the registry (npm, dotnet, go, gem,
    // cargo, rustup); fall back to the old generic phrasing for any
    // dependency name outside that table.
    let hints: Vec<String> = missing
        .iter()
        .map(|dep| match extensions::prereq_install_cmd(dep) {
            Some(cmd) => format!("{dep}: {cmd}"),
            None => format!("install {dep} and try again"),
        })
        .collect();
    format!(
        "{} requires {} — {}",
        name,
        missing.join(", "),
        hints.join("; ")
    )
}

// ---------------------------------------------------------------------------
// LspManager — coordinates multiple language servers
// ---------------------------------------------------------------------------

/// `$/progress` cooldown — `is_indexing` keeps returning true for this
/// long after the last work item ends, so the indicator doesn't flicker
/// bright during the gaps between rust-analyzer's progress phases.
/// 3 seconds is roughly the longest gap observed between Fetching → end
/// and Indexing → begin on a cold workspace start.
const PROGRESS_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(3);

pub struct LspManager {
    root_path: PathBuf,
    /// All known server configs (built-in + user overrides).
    registry: Vec<LspServerConfig>,
    /// language_id → index into `servers` vec
    language_to_server: HashMap<String, LspServerId>,
    /// Running server instances
    servers: Vec<LspServer>,
    /// Shared event channel — all servers send events here
    event_tx: Sender<LspEvent>,
    event_rx: Receiver<LspEvent>,
    /// Track which servers have completed initialization
    initialized: HashMap<LspServerId, bool>,
    /// Cached semantic tokens legends per server (extracted on initialization).
    semantic_legends: HashMap<LspServerId, SemanticTokensLegend>,
    /// Extension manifests for *installed* extensions only (used for server config lookup).
    ext_manifests: Vec<extensions::ExtensionManifest>,
    /// All extension manifests (installed + available) — used to check if a language is
    /// covered by an extension, so the built-in registry doesn't fall back for languages
    /// that have a (not-yet-installed) extension.
    all_ext_manifests: Vec<extensions::ExtensionManifest>,
    /// Servers that have returned at least one non-empty response (symbols, hover, etc.).
    /// This indicates the server has finished indexing and is truly "ready".
    server_has_responded: HashMap<LspServerId, bool>,
    /// In-flight `$/progress` tokens per server.
    /// Vec preserves begin order so `current_progress` can return the
    /// most-recently-begun work item for status-bar display. Populated
    /// by WorkProgressBegin/Report, drained by WorkProgressEnd.
    progress_data: HashMap<LspServerId, Vec<(String, LspProgress)>>,
    /// Timestamp of the last WorkProgressEnd per server. `is_indexing`
    /// keeps reporting true for `PROGRESS_COOLDOWN` after the last end
    /// so the indicator doesn't flicker bright between back-to-back
    /// progress phases (rust-analyzer fires several: Fetching → Indexing
    /// → Building proc-macros, with brief idle gaps where no token is
    /// open but the server is NOT actually ready).
    last_progress_end: HashMap<LspServerId, std::time::Instant>,
    /// Servers that crashed or exited (for display in :LspInfo).
    crashed_servers: Vec<String>,
    /// Last error from `ensure_server_for_language` (dependency check failure, etc.).
    /// Engine reads and clears this after calling ensure_server.
    pub last_start_error: Option<String>,
    /// Negative cache: languages whose server resolution has already been
    /// attempted and failed, so `ensure_server_for_language` doesn't re-run
    /// the (possibly slow — broken rustup proxy, uninstalled binary) probe
    /// on every buffer open. Maps language_id → the error message
    /// that was produced the first time, so later opens still surface it.
    ///
    /// Populated on *any* `resolve_and_start_server` failure, not only
    /// "binary could not be resolved" — a transient `LspServer::start`
    /// spawn failure on an already-resolved binary (resource exhaustion,
    /// permissions) is cached too. Cleared by `:LspRestart`,
    /// `add_registry_entry`, or when the extension set changes (`set_ext_manifests`).
    failed_language_resolutions: HashMap<String, Option<String>>,
}

/// Snapshot of a `$/progress` work item shown in the status bar.
/// Title is set at begin time (e.g. "Indexing"); message and percentage
/// are updated by interim "report" notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspProgress {
    /// Stage label from the begin payload (e.g. "Indexing", "Building").
    /// Empty when the server didn't supply one.
    pub title: String,
    /// Latest progress detail (e.g. "319/320"). None until a report
    /// arrives or if the server doesn't emit one.
    pub message: Option<String>,
    /// Latest percentage 0..=100. None when the server doesn't track it.
    pub percentage: Option<u32>,
}

/// LSP server status for a given language (used by status bar indicator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspStatus {
    /// No LSP server configured or applicable for this language.
    None,
    /// Server binary is being installed.
    Installing,
    /// Server is spawned but hasn't completed initialization handshake.
    Initializing(String),
    /// Server is running and ready. Contains the server command name.
    Running(String),
    /// Server crashed or exited unexpectedly.
    Crashed,
}

impl LspManager {
    /// Mark a server as responsive (ready for requests).
    pub fn mark_server_responded(&mut self, server_id: LspServerId) {
        self.server_has_responded.insert(server_id, true);
    }

    /// Record that a `$/progress` work item has begun on a server.
    /// Duplicate begin for the same token replaces the existing entry —
    /// servers shouldn't do this, but be defensive.
    ///
    /// Returns `true` if this actually changed the tracked snapshot (a new
    /// token, or a duplicate begin whose payload differs from what was
    /// already stored) — `false` for a byte-identical duplicate begin.
    /// The caller uses this to decide whether a redraw is warranted; a
    /// server that repeats identical payloads must not force a redraw when
    /// the on-screen status segment hasn't changed.
    #[must_use]
    pub fn work_progress_begin(
        &mut self,
        server_id: LspServerId,
        token: String,
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    ) -> bool {
        let progress = LspProgress {
            title: title.unwrap_or_default(),
            message,
            percentage,
        };
        let list = self.progress_data.entry(server_id).or_default();
        if let Some(slot) = list.iter_mut().find(|(t, _)| *t == token) {
            if slot.1 == progress {
                return false;
            }
            slot.1 = progress;
        } else {
            list.push((token, progress));
        }
        true
    }

    /// Update an in-flight work item's message/percentage from a
    /// `$/progress` "report" notification. Silently ignored if
    /// the token isn't tracked.
    ///
    /// Returns `true` only if the stored message or percentage actually
    /// changed value — a server that re-reports the same message/percentage
    /// repeatedly, or reports on a token never tracked, must not force a redraw.
    #[must_use]
    pub fn work_progress_report(
        &mut self,
        server_id: LspServerId,
        token: &str,
        message: Option<String>,
        percentage: Option<u32>,
    ) -> bool {
        let Some(list) = self.progress_data.get_mut(&server_id) else {
            return false;
        };
        let Some(slot) = list.iter_mut().find(|(t, _)| t == token) else {
            return false;
        };
        let mut changed = false;
        if message.is_some() && slot.1.message != message {
            slot.1.message = message;
            changed = true;
        }
        if percentage.is_some() && slot.1.percentage != percentage {
            slot.1.percentage = percentage;
            changed = true;
        }
        changed
    }

    /// Record that a `$/progress` work item has ended on a server.
    /// Only updates the cooldown timestamp if we actually had this token
    /// tracked — a stray `end` for an unknown token (race / bug on the
    /// server side) shouldn't extend the dim state.
    ///
    /// Returns `true` if a tracked token was actually removed — a stray `end`
    /// for a token already gone is a no-op and must not force a redraw either.
    #[must_use]
    pub fn work_progress_end(&mut self, server_id: LspServerId, token: &str) -> bool {
        let removed = self
            .progress_data
            .get_mut(&server_id)
            .map(|list| {
                let before = list.len();
                list.retain(|(t, _)| t != token);
                list.len() != before
            })
            .unwrap_or(false);
        if removed {
            self.last_progress_end
                .insert(server_id, std::time::Instant::now());
        }
        removed
    }

    /// True if the given server has any open `$/progress` work item OR
    /// finished its last progress within `PROGRESS_COOLDOWN`. The cooldown
    /// covers the brief idle gaps between rust-analyzer's progress phases
    /// (Fetching → Indexing → Building) where no token is currently open
    /// but the server hasn't truly settled.
    pub fn is_indexing(&self, server_id: LspServerId) -> bool {
        if self
            .progress_data
            .get(&server_id)
            .is_some_and(|list| !list.is_empty())
        {
            return true;
        }
        if let Some(end_time) = self.last_progress_end.get(&server_id) {
            if end_time.elapsed() < PROGRESS_COOLDOWN {
                return true;
            }
        }
        false
    }

    /// Return the most-recently-begun progress item for a server.
    /// Used by the status bar to format `name • title: message` while
    /// the server is indexing. Returns None when no progress is open.
    pub fn current_progress(&self, server_id: LspServerId) -> Option<&LspProgress> {
        self.progress_data
            .get(&server_id)
            .and_then(|list| list.last())
            .map(|(_, p)| p)
    }

    /// Look up the server handling a given language. Public accessor so
    /// the engine can correlate buffer-language → server_id for things
    /// like `is_indexing`.
    pub fn server_id_for_language(&self, lang: &str) -> Option<LspServerId> {
        self.language_to_server.get(lang).copied()
    }

    /// Get the LSP status for a given language identifier.
    pub fn lsp_status_for_language(&self, lang: &str) -> LspStatus {
        // Check if a server exists for this language
        if let Some(&server_id) = self.language_to_server.get(lang) {
            let cmd = self
                .servers
                .get(server_id)
                .map(|s| {
                    let c = s.command();
                    c.rsplit('/').next().unwrap_or(c).to_string()
                })
                .unwrap_or_default();
            let handshake_done = self.initialized.get(&server_id).copied().unwrap_or(false);
            let has_responded = self
                .server_has_responded
                .get(&server_id)
                .copied()
                .unwrap_or(false);
            if handshake_done && has_responded {
                LspStatus::Running(cmd)
            } else {
                // Still initializing (handshake pending) or indexing (no responses yet)
                LspStatus::Initializing(cmd)
            }
        } else {
            // Check if it crashed
            let crashed = self.crashed_servers.iter().any(|s| s.contains(lang));
            if crashed {
                LspStatus::Crashed
            } else {
                LspStatus::None
            }
        }
    }

    pub fn new(root_path: PathBuf, user_servers: &[LspServerConfig]) -> Self {
        let (event_tx, event_rx) = mpsc::channel();

        // Merge built-in + user configs (user configs take priority for matching languages)
        let mut registry = default_server_registry();
        for user_cfg in user_servers {
            // Remove built-in entries for languages the user overrides
            for lang in &user_cfg.languages {
                for built_in in &mut registry {
                    built_in.languages.retain(|l| l != lang);
                }
            }
            registry.push(user_cfg.clone());
        }
        // Remove empty entries
        registry.retain(|c| !c.languages.is_empty());

        Self {
            root_path,
            registry,
            language_to_server: HashMap::new(),
            servers: Vec::new(),
            event_tx,
            event_rx,
            initialized: HashMap::new(),
            semantic_legends: HashMap::new(),
            ext_manifests: Vec::new(),
            all_ext_manifests: Vec::new(),
            server_has_responded: HashMap::new(),
            progress_data: HashMap::new(),
            last_progress_end: HashMap::new(),
            crashed_servers: Vec::new(),
            last_start_error: None,
            failed_language_resolutions: HashMap::new(),
        }
    }

    /// Update the cached extension manifests (called by engine when registry changes).
    /// `installed` — only installed extensions (used for server config lookup).
    /// `all` — all available extensions (used to check if a language is covered by an
    /// extension, so the built-in registry doesn't start servers for uninstalled extensions).
    pub fn set_ext_manifests(
        &mut self,
        installed: Vec<extensions::ExtensionManifest>,
        all: Vec<extensions::ExtensionManifest>,
    ) {
        self.ext_manifests = installed;
        self.all_ext_manifests = all;
        // The extension set changing can change what
        // `ensure_server_for_language` resolves to (a newly installed
        // extension, a different manifest's dependencies) — stale negative
        // cache entries would otherwise block the new resolution forever.
        self.failed_language_resolutions.clear();
    }

    /// Ensure a server is running for the given language. Returns the server ID
    /// if a server is available (or was just started), None if no config exists
    /// or the binary is not on PATH/Mason bin.
    ///
    /// A failed resolution (e.g. a broken rustup proxy that takes
    /// 200-300ms to fail its `--version` probe) is remembered in
    /// `failed_language_resolutions` so repeat opens of files in the same
    /// language don't pay the probe cost again — a single file open already
    /// calls this twice (did-open + semantic tokens). The cache is cleared by
    /// `set_ext_manifests`, `add_registry_entry`, `restart_server_for_language`
    /// and `stop_server_for_language` — anything that could change the outcome.
    pub fn ensure_server_for_language(&mut self, language_id: &str) -> Option<LspServerId> {
        // Already running?
        if let Some(&id) = self.language_to_server.get(language_id) {
            return Some(id);
        }

        // Already tried and failed — don't re-probe.
        if let Some(cached_err) = self.failed_language_resolutions.get(language_id) {
            self.last_start_error = cached_err.clone();
            return None;
        }

        let result = self.resolve_and_start_server(language_id);
        if result.is_none() {
            self.failed_language_resolutions
                .insert(language_id.to_string(), self.last_start_error.clone());
        }
        result
    }

    /// Does the actual resolution/spawn work for `ensure_server_for_language`,
    /// uncached. Split out so the cache check/populate logic above stays a
    /// simple wrapper regardless of how many `return None` branches this
    /// grows.
    fn resolve_and_start_server(&mut self, language_id: &str) -> Option<LspServerId> {
        self.last_start_error = None;

        // Check declared dependencies from the extension manifest.
        if let Some(manifest) =
            extensions::find_manifest_for_language_id(&self.ext_manifests, language_id)
        {
            let missing: Vec<&str> = manifest
                .lsp
                .dependencies
                .iter()
                .filter(|dep| resolve_command(dep).is_none())
                .map(|s| s.as_str())
                .collect();
            if !missing.is_empty() {
                self.last_start_error = Some(missing_dependency_message(manifest, &missing));
                return None;
            }
        }

        // Build candidate list: extension manifest entries first (primary + fallbacks),
        // then the built-in registry.  First candidate with a resolvable binary wins.
        let mut candidates: Vec<LspServerConfig> = Vec::new();
        if let Some(manifest) =
            extensions::find_manifest_for_language_id(&self.ext_manifests, language_id)
        {
            candidates.extend(server_configs_from_manifest(manifest, language_id));
        }
        // Only fall back to the built-in registry for languages that have NO corresponding
        // extension at all.  If an extension exists but isn't installed, we respect that
        // choice and don't auto-start a server from a binary that happens to be on PATH.
        let has_extension =
            extensions::find_manifest_for_language_id(&self.all_ext_manifests, language_id)
                .is_some();
        if !has_extension {
            candidates.extend(
                self.registry
                    .iter()
                    .filter(|c| c.languages.iter().any(|l| l == language_id))
                    .cloned(),
            );
        }

        // Use the resolved full path so the spawn works regardless of the process's PATH.
        let (mut config, resolved) = match candidates
            .into_iter()
            .find_map(|c| resolve_command(&c.command).map(|p| (c, p)))
        {
            Some(pair) => pair,
            None => {
                // Surface an actionable hint instead of falling through
                // to the generic "No LSP server found" message.  When the
                // matching extension manifest has an install command, tell
                // the user exactly what to run.
                if let Some(manifest) =
                    extensions::find_manifest_for_language_id(&self.ext_manifests, language_id)
                {
                    let install_cmd = manifest.lsp.install_cmd_for_platform();
                    let name = manifest.display_or_name();
                    self.last_start_error = Some(if !install_cmd.is_empty() {
                        // Branch only on `install_cmd` being present, not also on
                        // `manifest.lsp.binary` being non-empty. Check whether the
                        // binary field is empty and fall back to the extension's
                        // display name for the label instead of printing an empty
                        // binary name.
                        let binary_label = if manifest.lsp.binary.is_empty() {
                            name
                        } else {
                            manifest.lsp.binary.as_str()
                        };
                        not_found_hint(binary_label, install_cmd, &self.root_path)
                    } else {
                        // When there is no install command, name the extension and
                        // say plainly that it has no installer on this platform.
                        format!(
                            "{name} extension declares no LSP install command for this \
                             platform — install its language server manually."
                        )
                    });
                }
                return None;
            }
        };
        config.command = resolved.to_string_lossy().into_owned();

        // Start the server
        let id = self.servers.len();
        match LspServer::start(id, &config, &self.root_path, self.event_tx.clone()) {
            Ok(server) => {
                // Map all languages this server handles
                for lang in &config.languages {
                    self.language_to_server.insert(lang.clone(), id);
                }
                self.initialized.insert(id, false);
                self.servers.push(server);
                Some(id)
            }
            Err(e) => {
                // Surface spawn errors so the user sees why the server failed to start,
                // rather than a generic "No LSP servers running".
                install_log(&format!(
                    "[lsp-start] FAILED for {} (lang={language_id}): {e}",
                    config.command
                ));
                self.last_start_error = Some(format!("LSP {} failed: {e}", config.command));
                None
            }
        }
    }

    /// Add a server config to the in-memory registry (does not persist to disk).
    pub fn add_registry_entry(&mut self, config: LspServerConfig) {
        // A fresh registry entry (e.g. just installed via
        // :ExtInstall) can resolve where the old candidate list couldn't —
        // drop any negative-cache entries for the languages it covers so
        // the next `ensure_server_for_language` actually re-probes.
        for lang in &config.languages {
            self.failed_language_resolutions.remove(lang);
        }
        self.registry.push(config);
    }

    /// Spawn a background thread to run an install command.
    /// The result is sent as `LspEvent::InstallComplete` on the shared channel.
    /// Full command + output is always appended to `/tmp/vimcode-install.log`.
    pub fn run_install_command(&self, lang_id: &str, install_cmd: &str) {
        let tx = self.event_tx.clone();
        let lang_id = lang_id.to_string();
        let install_cmd = install_cmd.to_string();
        // Run in the workspace root (not whatever directory the process
        // happens to be in) so a command like `rustup component add
        // rust-analyzer` picks up a pinned `rust-toolchain.toml`.
        let workspace_root = self.root_path.clone();
        std::thread::spawn(move || {
            install_log(&format!(
                "[{}] START lang_id={lang_id}\nCMD: {install_cmd}\nCWD: {}\nPATH: {}",
                timestamp(),
                workspace_root.display(),
                std::env::var("PATH").unwrap_or_else(|_| "(unset)".into()),
            ));

            // Run via shell so npm/pip/dotnet etc. resolve from user PATH.
            // `shell_cmd()` picks `sh -c` vs `cmd /C` and hides the console window
            // on Windows — the single construction point every shell spawn site goes through.
            let result = crate::core::terminal::shell_cmd(&install_cmd)
                .current_dir(&workspace_root)
                .output();

            match result {
                Ok(out) => {
                    let success = out.status.success();
                    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                    install_log(&format!(
                        "[{}] DONE lang_id={lang_id} status={} \nSTDOUT:\n{}\nSTDERR:\n{}",
                        timestamp(),
                        out.status,
                        stdout.trim(),
                        stderr.trim(),
                    ));
                    let output = if success {
                        stdout
                    } else {
                        // Combine stderr + stdout: some tools (e.g. unzip) write
                        // errors to stdout rather than stderr.
                        let combined = format!("{}\n{}", stderr.trim(), stdout.trim());
                        let combined = combined.trim().to_string();
                        if combined.is_empty() {
                            format!("process exited with {}", out.status)
                        } else {
                            combined
                        }
                    };
                    let output = output.trim().to_string();
                    let _ = tx.send(LspEvent::InstallComplete {
                        lang_id,
                        success,
                        output,
                    });
                }
                Err(e) => {
                    install_log(&format!(
                        "[{}] ERROR lang_id={lang_id} failed to spawn: {e}",
                        timestamp()
                    ));
                    let _ = tx.send(LspEvent::InstallComplete {
                        lang_id,
                        success: false,
                        output: e.to_string(),
                    });
                }
            }
        });
    }

    /// Test-only event injection: pushes a synthetic [`LspEvent`]
    /// through the exact channel a real spawned server would use, so a
    /// driver-tier test can exercise `Engine::poll_lsp`'s redraw decision
    /// against real event plumbing (`poll_events` → the event match in
    /// `Engine::poll_lsp`) instead of calling e.g. `work_progress_report`
    /// directly — which would bypass `poll_lsp` and its redraw-worthiness
    /// decision entirely.
    #[cfg(test)]
    pub(crate) fn test_send_event(&self, event: LspEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Non-blocking poll for events from all running servers.
    /// Processes at most `max_events` to avoid blocking the UI during event floods.
    pub fn poll_events(&mut self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        let max_events = 50;
        while events.len() < max_events {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    // Handle Initialized event — send the initialized notification
                    if let LspEvent::Initialized(server_id, capabilities) = &event {
                        if let Some(server) = self.servers.get_mut(*server_id) {
                            server.capabilities = capabilities.clone();
                            server.send_initialized();
                            self.initialized.insert(*server_id, true);
                            // Cache semantic tokens legend if the server supports it.
                            if let Some(legend) = server.semantic_tokens_legend() {
                                self.semantic_legends.insert(*server_id, legend);
                            }
                        }
                    }
                    events.push(event);
                }
                Err(_) => break,
            }
        }
        events
    }

    /// Notify the appropriate server that a document was opened.
    /// Returns `Ok(())` on success, `Err(message)` if the server couldn't start.
    /// If the server is still initializing, the didOpen will be sent later
    /// when the Initialized event is processed (see `Engine::poll_lsp`).
    pub fn notify_did_open(&mut self, path: &Path, text: &str) -> Result<(), String> {
        let language_id = match language_id_from_path(path) {
            Some(l) => l,
            None => return Ok(()), // unknown language, nothing to do
        };

        // Check if we already have a running server
        if let Some(&server_id) = self.language_to_server.get(&language_id) {
            // Only send didOpen if server has completed initialization
            if self.initialized.get(&server_id).copied().unwrap_or(false) {
                let uri = path_to_uri(path);
                self.servers[server_id].did_open(&uri, &language_id, text);
            }
            // If still initializing, poll_lsp will re-send didOpen on Initialized event
            return Ok(());
        }

        // Try to start any configured server for this language
        match self.ensure_server_for_language(&language_id) {
            Some(_) => Ok(()),
            None => {
                // Return specific dependency error if available, otherwise generic
                let msg = self
                    .last_start_error
                    .take()
                    .unwrap_or_else(|| format!("No LSP server found for {language_id}"));
                Err(msg)
            }
        }
    }

    /// Notify the appropriate server that a document changed.
    pub fn notify_did_change(&mut self, path: &Path, text: &str) {
        let language_id = match language_id_from_path(path) {
            Some(l) => l,
            None => return,
        };
        if let Some(&server_id) = self.language_to_server.get(&language_id) {
            if !self.initialized.get(&server_id).copied().unwrap_or(false) {
                return;
            }
            let uri = path_to_uri(path);
            self.servers[server_id].did_change(&uri, text);
        }
    }

    /// Notify the appropriate server that a document was saved.
    pub fn notify_did_save(&mut self, path: &Path, text: &str) {
        let language_id = match language_id_from_path(path) {
            Some(l) => l,
            None => return,
        };
        if let Some(&server_id) = self.language_to_server.get(&language_id) {
            if !self.initialized.get(&server_id).copied().unwrap_or(false) {
                return;
            }
            let uri = path_to_uri(path);
            self.servers[server_id].did_save(&uri, text);
        }
    }

    /// Notify the appropriate server that a document was closed.
    pub fn notify_did_close(&mut self, path: &Path) {
        let language_id = match language_id_from_path(path) {
            Some(l) => l,
            None => return,
        };
        if let Some(&server_id) = self.language_to_server.get(&language_id) {
            if !self.initialized.get(&server_id).copied().unwrap_or(false) {
                return;
            }
            let uri = path_to_uri(path);
            self.servers[server_id].did_close(&uri);
        }
    }

    /// Request completions from the appropriate server.
    pub fn request_completion(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        let language_id = language_id_from_path(path)?;
        let server_id = *self.language_to_server.get(&language_id)?;
        if !self.initialized.get(&server_id).copied().unwrap_or(false) {
            return None;
        }
        let uri = path_to_uri(path);
        Some(self.servers[server_id].request_completion(&uri, line, character))
    }

    /// Helper: look up server for a path; returns (server_id, uri) if ready.
    /// If no server is running yet, attempts to start one.
    fn server_and_uri(&mut self, path: &Path) -> Option<(usize, String)> {
        let language_id = language_id_from_path(path)?;
        if !self.language_to_server.contains_key(&language_id) {
            // No server running — try to start one.
            self.ensure_server_for_language(&language_id);
        }
        let server_id = *self.language_to_server.get(&language_id)?;
        if !self.initialized.get(&server_id).copied().unwrap_or(false) {
            return None;
        }
        Some((server_id, path_to_uri(path)))
    }

    /// Check whether a server exists for the given path but is still initializing.
    pub fn is_server_initializing(&self, path: &Path) -> bool {
        let language_id = match language_id_from_path(path) {
            Some(l) => l,
            None => return false,
        };
        if let Some(&server_id) = self.language_to_server.get(&language_id) {
            !self.initialized.get(&server_id).copied().unwrap_or(false)
        } else {
            false
        }
    }

    /// Request go-to-definition from the appropriate server.
    pub fn request_definition(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        let language_id = language_id_from_path(path)?;
        let server_id = *self.language_to_server.get(&language_id)?;
        if !self.initialized.get(&server_id).copied().unwrap_or(false) {
            return None;
        }
        let uri = path_to_uri(path);
        Some(self.servers[server_id].request_definition(&uri, line, character))
    }

    /// Request hover info from the appropriate server.
    pub fn request_hover(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        let language_id = language_id_from_path(path)?;
        let server_id = *self.language_to_server.get(&language_id)?;
        if !self.initialized.get(&server_id).copied().unwrap_or(false) {
            return None;
        }
        let uri = path_to_uri(path);
        Some(self.servers[server_id].request_hover(&uri, line, character))
    }

    /// Request all references from the appropriate server.
    pub fn request_references(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_references(&uri, line, character))
    }

    /// Request go-to-implementation from the appropriate server.
    pub fn request_implementation(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_implementation(&uri, line, character))
    }

    /// Request go-to-type-definition from the appropriate server.
    pub fn request_type_definition(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_type_definition(&uri, line, character))
    }

    /// Request document symbols (outline) from the appropriate server.
    pub fn request_document_symbols(&mut self, path: &Path) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_document_symbols(&uri))
    }

    /// Request workspace symbols matching a query from the appropriate server.
    pub fn request_workspace_symbols(&mut self, path: &Path, query: &str) -> Option<i64> {
        let (sid, _uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_workspace_symbols(query))
    }

    /// Request signature help from the appropriate server.
    pub fn request_signature_help(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_signature_help(&uri, line, character))
    }

    /// Request code actions for a line from the appropriate server.
    pub fn request_code_action(
        &mut self,
        path: &Path,
        line: u32,
        col: u32,
        diagnostics_json: serde_json::Value,
    ) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_code_action(&uri, line, col, diagnostics_json))
    }

    /// Request whole-file formatting from the appropriate server.
    pub fn request_formatting(
        &mut self,
        path: &Path,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_formatting(&uri, tab_size, insert_spaces))
    }

    /// Check if the server for the given file supports document formatting.
    #[allow(dead_code)]
    pub fn server_supports_formatting(&self, path: &Path) -> bool {
        let language_id = match language_id_from_path(path) {
            Some(l) => l,
            None => return false,
        };
        if let Some(&server_id) = self.language_to_server.get(&language_id) {
            if self.initialized.get(&server_id).copied().unwrap_or(false) {
                return self.servers[server_id].supports_formatting();
            }
        }
        false
    }

    /// Request rename from the appropriate server.
    pub fn request_rename(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        Some(self.servers[sid].request_rename(&uri, line, character, new_name))
    }

    /// Request full semantic tokens for a file. Returns the request ID if the server supports it.
    pub fn request_semantic_tokens(&mut self, path: &Path) -> Option<i64> {
        let (sid, uri) = self.server_and_uri(path)?;
        if !self.servers[sid].supports_semantic_tokens() {
            return None;
        }
        Some(self.servers[sid].request_semantic_tokens_full(&uri))
    }

    /// Get the cached semantic tokens legend for a server.
    pub fn semantic_legend_for_server(
        &self,
        server_id: LspServerId,
    ) -> Option<&SemanticTokensLegend> {
        self.semantic_legends.get(&server_id)
    }

    /// Find which server is handling a given file path.
    #[allow(dead_code)]
    pub fn server_id_for_path(&self, path: &Path) -> Option<LspServerId> {
        let language_id = language_id_from_path(path)?;
        self.language_to_server.get(&language_id).copied()
    }

    /// Check if the server for a given file supports a specific LSP capability.
    pub fn server_supports(&self, path: &Path, capability: &str) -> bool {
        let Some(server_id) = self.server_id_for_path(path) else {
            return false;
        };
        if let Some(server) = self.servers.get(server_id) {
            let v = &server.capabilities[capability];
            v.as_bool().unwrap_or(false) || v.is_object()
        } else {
            false
        }
    }

    /// Shutdown all running servers.
    pub fn shutdown_all(&mut self) {
        for server in &mut self.servers {
            server.shutdown();
        }
    }

    /// Shutdown and restart the server for a given language.
    pub fn restart_server_for_language(&mut self, language_id: &str) -> Option<LspServerId> {
        // An explicit restart is a request to re-probe, even if a
        // prior resolution attempt failed and was cached.
        self.failed_language_resolutions.remove(language_id);
        // Shutdown existing
        if let Some(&server_id) = self.language_to_server.get(language_id) {
            self.servers[server_id].shutdown();
        }
        // Remove all language mappings pointing to this server
        let old_id = self.language_to_server.remove(language_id);
        if let Some(id) = old_id {
            self.language_to_server.retain(|_, v| *v != id);
            self.initialized.remove(&id);
        }

        // Find config and restart: manifest first, then registry.
        let mut candidates: Vec<LspServerConfig> = Vec::new();
        if let Some(manifest) =
            extensions::find_manifest_for_language_id(&self.ext_manifests, language_id)
        {
            candidates.extend(server_configs_from_manifest(manifest, language_id));
        }
        let has_extension =
            extensions::find_manifest_for_language_id(&self.all_ext_manifests, language_id)
                .is_some();
        if !has_extension {
            candidates.extend(
                self.registry
                    .iter()
                    .filter(|c| c.languages.iter().any(|l| l == language_id))
                    .cloned(),
            );
        }
        let (mut config, resolved) = candidates
            .into_iter()
            .find_map(|c| resolve_command(&c.command).map(|p| (c, p)))?;
        config.command = resolved.to_string_lossy().into_owned();
        let new_id = self.servers.len();
        match LspServer::start(new_id, &config, &self.root_path, self.event_tx.clone()) {
            Ok(server) => {
                for lang in &config.languages {
                    self.language_to_server.insert(lang.clone(), new_id);
                }
                self.initialized.insert(new_id, false);
                self.servers.push(server);
                Some(new_id)
            }
            Err(_) => None,
        }
    }

    /// Stop the server for a given language.
    pub fn stop_server_for_language(&mut self, language_id: &str) {
        // Also let a subsequent open re-probe rather than reusing a
        // stale negative-cache entry from before the server was started.
        self.failed_language_resolutions.remove(language_id);
        if let Some(&server_id) = self.language_to_server.get(language_id) {
            self.servers[server_id].shutdown();
        }
        let old_id = self.language_to_server.remove(language_id);
        if let Some(id) = old_id {
            self.language_to_server.retain(|_, v| *v != id);
            self.initialized.remove(&id);
        }
    }

    /// Clean up a server that exited or crashed. Returns a description string
    /// (command + languages) for use in the user-facing message.
    pub fn handle_server_exited(&mut self, server_id: LspServerId) -> String {
        let cmd = self
            .servers
            .get(server_id)
            .map(|s| s.command().to_string())
            .unwrap_or_else(|| format!("server {}", server_id));

        let langs: Vec<String> = self
            .language_to_server
            .iter()
            .filter(|(_, &id)| id == server_id)
            .map(|(lang, _)| lang.clone())
            .collect();

        self.language_to_server.retain(|_, &mut id| id != server_id);
        self.initialized.remove(&server_id);

        let desc = if langs.is_empty() {
            cmd
        } else {
            format!("{} ({})", cmd, langs.join(", "))
        };
        self.crashed_servers.push(desc.clone());
        desc
    }

    /// Get status information about running servers.
    /// If `current_lang` is provided, marks the server handling that language with ●.
    pub fn server_info(&self, current_lang: Option<&str>) -> Vec<String> {
        let mut info = Vec::new();
        // Group languages by server ID
        let mut server_langs: std::collections::HashMap<usize, Vec<&str>> =
            std::collections::HashMap::new();
        for (lang, &server_id) in &self.language_to_server {
            server_langs.entry(server_id).or_default().push(lang);
        }
        let mut server_ids: Vec<usize> = server_langs.keys().copied().collect();
        server_ids.sort();
        let active_server_id = current_lang.and_then(|l| self.language_to_server.get(l).copied());
        for server_id in server_ids {
            let langs = &server_langs[&server_id];
            let status = if self.initialized.get(&server_id).copied().unwrap_or(false) {
                "running"
            } else {
                "initializing"
            };
            let cmd = self
                .servers
                .get(server_id)
                .map(|s| s.command())
                .unwrap_or("unknown");
            let mut sorted_langs: Vec<&str> = langs.to_vec();
            sorted_langs.sort();
            let lang_list = sorted_langs.join(", ");
            let marker = if active_server_id == Some(server_id) {
                "● "
            } else {
                "  "
            };
            info.push(format!("{marker}{cmd}: {status} ({lang_list})"));
        }
        for entry in &self.crashed_servers {
            info.push(format!("  {}: crashed", entry));
        }
        if info.is_empty() {
            info.push("No LSP servers running".to_string());
        }
        info
    }
}

/// Diagnostic helper: try to resolve binaries for all servers matching `lang_id`.
/// Checks extension manifests first, then the default registry.
pub fn debug_resolve(lang_id: &str, ext_manifests: &[extensions::ExtensionManifest]) -> String {
    let mut candidates: Vec<LspServerConfig> = Vec::new();
    if let Some(manifest) = extensions::find_manifest_for_language_id(ext_manifests, lang_id) {
        candidates.extend(server_configs_from_manifest(manifest, lang_id));
    }
    let registry = default_server_registry();
    candidates.extend(
        registry
            .into_iter()
            .filter(|c| c.languages.iter().any(|l| l == lang_id)),
    );
    if candidates.is_empty() {
        return format!("LspDebug: no registry entries for '{lang_id}'");
    }
    let results: Vec<String> = candidates
        .iter()
        .map(|c| match resolve_command(&c.command) {
            Some(p) => format!("{} -> {}", c.command, p.display()),
            None => format!("{} -> NOT FOUND", c.command),
        })
        .collect();
    format!("LspDebug[{lang_id}]: {}", results.join("; "))
}

impl Drop for LspManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Progress_data lifecycle. Without a real LSP server we
    // drive the helpers directly — that's all the indicator's gate
    // consults.
    #[test]
    fn work_progress_begin_marks_indexing() {
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        assert!(!mgr.is_indexing(sid), "no progress → not indexing");

        let _ = mgr.work_progress_begin(sid, "rustAnalyzer/Indexing".to_string(), None, None, None);
        assert!(mgr.is_indexing(sid), "begin → indexing");
    }

    #[test]
    fn work_progress_end_keeps_indexing_during_cooldown() {
        // After all tokens end, is_indexing stays true for PROGRESS_COOLDOWN
        // to smooth flicker between back-to-back progress phases (e.g.
        // rust-analyzer's Fetching → gap → Indexing).
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let _ = mgr.work_progress_begin(sid, "t1".to_string(), None, None, None);
        let _ = mgr.work_progress_end(sid, "t1");
        assert!(
            mgr.is_indexing(sid),
            "still indexing within cooldown after last end"
        );
        // Underlying progress_data IS empty — just the cooldown extends it.
        assert!(mgr
            .progress_data
            .get(&sid)
            .is_none_or(|list| list.is_empty()));
    }

    #[test]
    fn work_progress_multiple_tokens_open_keep_indexing() {
        // rust-analyzer fires several overlapping progress tokens during
        // workspace load (Indexing, Roots Scanned, Building, etc.). The
        // indicator should stay dim until they all close — which still
        // works directly (cooldown is irrelevant while a token is open).
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let _ = mgr.work_progress_begin(sid, "Indexing".to_string(), None, None, None);
        let _ = mgr.work_progress_begin(sid, "Roots Scanned".to_string(), None, None, None);
        assert!(mgr.is_indexing(sid));

        let _ = mgr.work_progress_end(sid, "Indexing");
        assert!(
            mgr.is_indexing(sid),
            "still indexing while one token remains open"
        );
        // Other token still in flight — cooldown isn't even consulted yet.
        assert_eq!(mgr.progress_data[&sid].len(), 1);
    }

    #[test]
    fn work_progress_unknown_end_does_not_arm_cooldown() {
        // Defensive: an `end` for a token we never saw `begin` for must
        // not crash AND must not extend the dim state via the cooldown.
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let _ = mgr.work_progress_end(sid, "never-began"); // no panic
        assert!(
            !mgr.is_indexing(sid),
            "stray end (no matching begin) must not flip state"
        );
        assert!(
            mgr.last_progress_end.get(&sid).is_none(),
            "stray end must not arm the cooldown"
        );
    }

    // ─── LspProgress accessors ──────────────────────────────────────
    #[test]
    fn current_progress_is_none_when_idle() {
        let mgr = LspManager::new(PathBuf::from("."), &[]);
        assert!(mgr.current_progress(0).is_none());
    }

    #[test]
    fn current_progress_returns_begin_payload() {
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let _ = mgr.work_progress_begin(
            sid,
            "rustAnalyzer/Indexing".to_string(),
            Some("Indexing".to_string()),
            Some("0/319".to_string()),
            Some(0),
        );
        let p = mgr.current_progress(sid).expect("progress exists");
        assert_eq!(p.title, "Indexing");
        assert_eq!(p.message.as_deref(), Some("0/319"));
        assert_eq!(p.percentage, Some(0));
    }

    #[test]
    fn report_updates_message_and_percentage() {
        // rust-analyzer fires begin with no detail, then a stream of
        // report notifications with `319/320`-style messages. Each report
        // must overwrite the previous message + percentage.
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let tok = "tok".to_string();
        let _ = mgr.work_progress_begin(sid, tok.clone(), Some("Indexing".to_string()), None, None);
        let _ = mgr.work_progress_report(sid, &tok, Some("1/320".to_string()), Some(0));
        let _ = mgr.work_progress_report(sid, &tok, Some("319/320".to_string()), Some(99));
        let p = mgr.current_progress(sid).expect("progress exists");
        assert_eq!(p.title, "Indexing");
        assert_eq!(p.message.as_deref(), Some("319/320"));
        assert_eq!(p.percentage, Some(99));
    }

    #[test]
    fn report_for_unknown_token_is_noop() {
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        // No begin for "ghost" — report must not create an entry.
        let _ = mgr.work_progress_report(sid, "ghost", Some("x".into()), Some(50));
        assert!(mgr.current_progress(sid).is_none());
    }

    // ─── Idempotent progress must not report "changed" ──────────────
    //
    // A chatty server resends the exact same $/progress payload repeatedly.
    // `Engine::poll_lsp` uses these `bool` returns to decide whether a redraw
    // is warranted. Every notification must not force one if the stored snapshot
    // (the only thing a redraw could possibly repaint differently) hasn't changed.

    #[test]
    fn work_progress_begin_reports_changed_only_on_first_call() {
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        assert!(
            mgr.work_progress_begin(
                sid,
                "tok".to_string(),
                Some("Indexing".to_string()),
                None,
                None
            ),
            "first begin for a token is always a change"
        );
        assert!(
            !mgr.work_progress_begin(
                sid,
                "tok".to_string(),
                Some("Indexing".to_string()),
                None,
                None
            ),
            "a byte-identical duplicate begin changes nothing"
        );
        assert!(
            mgr.work_progress_begin(
                sid,
                "tok".to_string(),
                Some("Indexing".to_string()),
                Some("1/10".to_string()),
                None
            ),
            "a begin with a genuinely different payload IS a change"
        );
    }

    #[test]
    fn work_progress_report_reports_changed_only_when_values_differ() {
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let tok = "tok".to_string();
        let _ = mgr.work_progress_begin(sid, tok.clone(), Some("Indexing".to_string()), None, None);

        assert!(
            mgr.work_progress_report(sid, &tok, Some("1/320".to_string()), Some(0)),
            "first report with real values is a change"
        );
        assert!(
            !mgr.work_progress_report(sid, &tok, Some("1/320".to_string()), Some(0)),
            "an identical repeat of the same message+percentage is not a change"
        );
        assert!(
            mgr.work_progress_report(sid, &tok, Some("2/320".to_string()), Some(0)),
            "message changing alone is a change"
        );
        assert!(
            mgr.work_progress_report(sid, &tok, Some("2/320".to_string()), Some(1)),
            "percentage changing alone is a change"
        );
        assert!(
            !mgr.work_progress_report(sid, "ghost", Some("x".into()), Some(50)),
            "a report for an untracked token is not a change"
        );
    }

    #[test]
    fn work_progress_end_reports_changed_only_when_a_token_was_actually_removed() {
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let _ = mgr.work_progress_begin(sid, "tok".to_string(), None, None, None);
        assert!(
            mgr.work_progress_end(sid, "tok"),
            "ending a tracked token is a change"
        );
        assert!(
            !mgr.work_progress_end(sid, "tok"),
            "ending it again (already gone) is not a change"
        );
        assert!(
            !mgr.work_progress_end(sid, "never-began"),
            "ending a token that was never tracked is not a change"
        );
    }

    #[test]
    fn current_progress_returns_most_recently_begun() {
        // When multiple tokens are open (rust-analyzer's overlapping
        // phases) the indicator surfaces the latest one.
        let mut mgr = LspManager::new(PathBuf::from("."), &[]);
        let sid: LspServerId = 0;
        let _ = mgr.work_progress_begin(sid, "a".into(), Some("Fetching".into()), None, None);
        let _ = mgr.work_progress_begin(sid, "b".into(), Some("Indexing".into()), None, None);
        let p = mgr.current_progress(sid).expect("progress exists");
        assert_eq!(p.title, "Indexing");

        // After the most recent ends, fall back to the older still-open one.
        let _ = mgr.work_progress_end(sid, "b");
        let p = mgr.current_progress(sid).expect("progress exists");
        assert_eq!(p.title, "Fetching");
    }

    // ─── Negative-cache / probe-dedup coverage ──────────────────────
    //
    // A rustup proxy for an uninstalled component exists on disk in
    // `~/.cargo/bin` but takes real wall-clock time (200-300ms) to fail its
    // `--version` probe. Before caching, `ensure_server_for_language` re-ran
    // that probe on every call — one file open already calls it twice (did-open +
    // semantic tokens) — so N opens cost 2N subprocess spawns instead of one.
    // These tests fake the proxy with a script that records each invocation to a
    // counter file and exits non-zero, so assertions are on invocation count.

    /// Build `<home>/.cargo/bin/<binary_name>` as a script that appends a
    /// line to `counter_file` on every invocation and exits non-zero —
    /// simulating a broken rustup proxy. Returns the fake home directory;
    /// caller is responsible for cleanup (and for calling
    /// `crate::core::paths::set_test_home` with it).
    #[cfg(unix)]
    fn fake_cargo_bin_broken_proxy(tag: &str, binary_name: &str, counter_file: &Path) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_1386_home_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cargo_bin = home.join(".cargo").join("bin");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&cargo_bin).unwrap();
        let binary_path = cargo_bin.join(binary_name);
        std::fs::write(
            &binary_path,
            format!(
                "#!/bin/sh\necho probe >> {}\nexit 1\n",
                counter_file.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Ensure the script is executable before testing — a freshly written
        // executable can transiently fail to `exec` on Linux with `ETXTBSY` when
        // other processes have the file open for writing. Run it here until it
        // genuinely execs, then truncate the counter file so later probes start
        // counting from zero.
        let mut execd = false;
        for _ in 0..200 {
            if std::process::Command::new(&binary_path)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok()
            {
                execd = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            execd,
            "fake broken proxy at {} never became executable",
            binary_path.display()
        );
        std::fs::write(counter_file, "").unwrap();

        home
    }

    #[cfg(unix)]
    fn probe_count(counter_file: &Path) -> usize {
        std::fs::read_to_string(counter_file)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .count()
    }

    /// **Verified RED against unfixed `develop`:** without
    /// `failed_language_resolutions`, each of the 6 `ensure_server_for_
    /// language` calls below re-runs the full probe, so `probe_count` reads
    /// 6 (or more, before the `resolve_command` dedup) instead of 1.
    #[test]
    #[cfg(unix)]
    fn ensure_server_for_language_caches_failed_resolution_across_opens() {
        let binary_name = "vimcode-test-1386-cache-proxy";
        let lang = "vimcode-test-lang-1386-cache";
        let counter_dir = std::env::temp_dir().join(format!(
            "vimcode_test_1386_cache_counter_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&counter_dir).unwrap();
        let counter_file = counter_dir.join("probes.log");

        let home = fake_cargo_bin_broken_proxy("cache", binary_name, &counter_file);
        // Thread-local home override — see `core::paths::TEST_HOME_OVERRIDE`.
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut mgr = LspManager::new(
            PathBuf::from("."),
            &[LspServerConfig {
                command: binary_name.to_string(),
                args: vec![],
                languages: vec![lang.to_string()],
                ..Default::default()
            }],
        );

        // Mirrors 3 file opens, each of which calls
        // `ensure_server_for_language` twice (did-open + semantic tokens —
        // see `Engine::lsp_did_open` / `LspManager::server_and_uri`).
        for _ in 0..3 {
            assert!(mgr.ensure_server_for_language(lang).is_none());
            assert!(mgr.ensure_server_for_language(lang).is_none());
        }
        assert_eq!(
            probe_count(&counter_file),
            1,
            "a failed resolution must be cached — 6 calls across 3 \
             simulated opens must probe the broken proxy at most once"
        );

        // :LspRestart-equivalent must clear the cache so a later attempt
        // re-probes instead of replaying the stale cached failure forever.
        mgr.restart_server_for_language(lang);
        assert_eq!(
            probe_count(&counter_file),
            2,
            "restart_server_for_language probes directly (uncached)"
        );
        mgr.ensure_server_for_language(lang);
        assert_eq!(
            probe_count(&counter_file),
            3,
            "ensure_server_for_language must re-probe after a restart \
             cleared the negative cache, not reuse the pre-restart cached \
             failure"
        );

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&counter_dir);
    }

    /// `resolve_command` must not probe the same path twice within
    /// one call — once via the `extra_tool_dirs` loop, again via `which`
    /// resolving to that identical path (which is exactly what happens for
    /// a real rustup proxy: it lives in `~/.cargo/bin` *and* is normally on
    /// `PATH`). This appends (never replaces) the fake proxy's directory to
    /// the real `PATH` so it can't affect any other concurrently-running
    /// `--lib` test's resolution of a real binary — only this test's
    /// uniquely-named fake binary becomes resolvable.
    ///
    /// **Verified RED against unfixed `develop`:** removing the
    /// `already_probed` check makes `probe_count` read 2 instead of 1.
    #[test]
    #[cfg(unix)]
    fn resolve_command_probes_cargo_bin_candidate_only_once_even_when_which_finds_it_too() {
        static PATH_APPEND_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _lock = PATH_APPEND_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let binary_name = "vimcode-test-1386-dedupe-proxy";
        let counter_dir = std::env::temp_dir().join(format!(
            "vimcode_test_1386_dedupe_counter_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&counter_dir).unwrap();
        let counter_file = counter_dir.join("probes.log");

        let home = fake_cargo_bin_broken_proxy("dedupe", binary_name, &counter_file);
        let cargo_bin = home.join(".cargo").join("bin");
        let _home_guard = crate::core::paths::set_test_home(&home);

        let old_path = std::env::var_os("PATH");
        let mut dirs: Vec<PathBuf> = old_path
            .as_ref()
            .map(std::env::split_paths)
            .into_iter()
            .flatten()
            .collect();
        dirs.push(cargo_bin);
        std::env::set_var("PATH", std::env::join_paths(&dirs).unwrap());

        let resolved = resolve_command(binary_name);

        match old_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }

        assert_eq!(resolved, None, "a broken proxy must still fail to resolve");
        assert_eq!(
            probe_count(&counter_file),
            1,
            "resolve_command must not probe the same rejected path twice \
             within a single call"
        );

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&counter_dir);
    }

    // ─── "not found" hint names the workspace root + toolchain ──

    /// Build `<home>/.cargo/bin/<binary_name>` as a script that always exits
    /// non-zero and prints `stderr_msg` to stderr — simulating a rustup
    /// proxy for a pinned toolchain that's missing the component. Returns
    /// the fake home directory; caller cleans up and calls
    /// `crate::core::paths::set_test_home` with it.
    #[cfg(unix)]
    fn fake_cargo_bin_broken_proxy_with_stderr(
        tag: &str,
        binary_name: &str,
        stderr_msg: &str,
    ) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_1905_home_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cargo_bin = home.join(".cargo").join("bin");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&cargo_bin).unwrap();
        let binary_path = cargo_bin.join(binary_name);
        std::fs::write(
            &binary_path,
            format!("#!/bin/sh\necho \"{stderr_msg}\" >&2\nexit 1\n"),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Same executability warm-up as `fake_cargo_bin_broken_proxy` — see
        // its comment for why this loop exists.
        let mut execd = false;
        for _ in 0..200 {
            if std::process::Command::new(&binary_path)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok()
            {
                execd = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            execd,
            "fake broken proxy at {} never became executable",
            binary_path.display()
        );
        home
    }

    #[test]
    fn parse_official_toolchain_from_stderr_extracts_pinned_version() {
        let stderr = "error: Unknown binary 'rust-analyzer' in official toolchain \
                       '1.97.1-aarch64-apple-darwin'\n";
        assert_eq!(
            parse_official_toolchain_from_stderr(stderr),
            Some("1.97.1-aarch64-apple-darwin".to_string())
        );
    }

    #[test]
    fn parse_official_toolchain_from_stderr_returns_none_without_marker() {
        assert_eq!(
            parse_official_toolchain_from_stderr("some unrelated error\n"),
            None
        );
    }

    #[test]
    fn not_found_hint_names_workspace_root() {
        // Sandbox `$HOME` to an empty dir with no `.cargo/bin/` at all, so
        // this assertion can't be swayed by whatever rust-analyzer proxy (or
        // lack thereof) happens to exist on the machine actually running the
        // test — `pinned_toolchain_hint_for_binary` must find nothing to
        // probe and the hint stays exactly the base form.
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_1905_nohome_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let _home_guard = crate::core::paths::set_test_home(&home);

        let root = Path::new("/workspace/my-project");
        let hint = not_found_hint("rust-analyzer", "rustup component add rust-analyzer", root);
        assert_eq!(
            hint,
            "rust-analyzer not found. Run in /workspace/my-project: \
             rustup component add rust-analyzer"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// The hint must say *where* to run the install command: from any
    /// directory other than the pinned project's root it installs into the
    /// wrong toolchain.
    #[test]
    #[cfg(unix)]
    fn resolve_and_start_server_hint_names_root_and_toolchain() {
        let binary_name = "vimcode-test-1905-ra-proxy";
        let stderr_msg = "error: Unknown binary 'vimcode-test-1905-ra-proxy' in official \
                           toolchain '1.97.1-aarch64-apple-darwin'";
        let home = fake_cargo_bin_broken_proxy_with_stderr("hint", binary_name, stderr_msg);
        let _home_guard = crate::core::paths::set_test_home(&home);

        let root = std::env::temp_dir().join(format!(
            "vimcode_test_1905_root_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let mut mgr = LspManager::new(root.clone(), &[]);
        let manifest = extensions::ExtensionManifest {
            name: "rust".to_string(),
            display_name: "Rust Language Support".to_string(),
            language_ids: vec!["vimcode-test-lang-1905".to_string()],
            lsp: extensions::LspConfig {
                binary: binary_name.to_string(),
                install: "rustup component add rust-analyzer".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        mgr.set_ext_manifests(vec![manifest.clone()], vec![manifest]);

        assert!(mgr
            .ensure_server_for_language("vimcode-test-lang-1905")
            .is_none());
        let err = mgr
            .last_start_error
            .as_deref()
            .expect("resolution failure must set last_start_error");
        assert!(
            err.contains(&format!("Run in {}: ", root.display())),
            "hint must name the workspace root, got: {err}"
        );
        assert!(
            err.contains("--toolchain 1.97.1-aarch64-apple-darwin"),
            "hint must offer the explicit --toolchain form parsed from the \
             proxy's stderr, got: {err}"
        );

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `run_install_command` must run in the workspace root, or it misses a
    /// pinned `rust-toolchain.toml` there.
    #[test]
    #[cfg(unix)]
    fn run_install_command_runs_in_workspace_root() {
        let root = std::env::temp_dir().join(format!(
            "vimcode_test_1905_install_cwd_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let out_file = root.join("pwd.out");

        let mgr = LspManager::new(root.clone(), &[]);
        let install_cmd = format!("pwd > {}", out_file.display());
        mgr.run_install_command("vimcode-test-lang-1905-cwd", &install_cmd);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut seen = String::new();
        while std::time::Instant::now() < deadline {
            if let Ok(contents) = std::fs::read_to_string(&out_file) {
                if !contents.trim().is_empty() {
                    seen = contents;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let pwd_canonical = Path::new(seen.trim()).canonicalize().unwrap_or_default();
        assert_eq!(
            pwd_canonical, canonical_root,
            "run_install_command must run with current_dir set to the \
             workspace root, got pwd={seen:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
