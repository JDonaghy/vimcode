//! Native tool acquisition (#1345): download, verify and unpack an LSP
//! server / DAP adapter binary without a shell command, `sudo`, `unzip`, or
//! PATH edits.
//!
//! This is the fix for the class of bug where a per-platform shell
//! one-liner (`[lsp] install_linux = "..."`) silently rots because it
//! points at a release page that has never had assets, or needs a tool
//! (`unzip`, `tar`, a package manager) that isn't on the host. A manifest
//! can instead declare an `[lsp.acquire]` (or `[dap.acquire]`) table
//! describing *where the binary lives*; vimcode downloads it over HTTPS,
//! verifies its SHA-256 checksum whenever the upstream publishes one,
//! unpacks just the one file the manifest names, and installs it under a
//! directory vimcode owns (`paths::managed_tools_dir()`) — see that
//! module's doc comment for the on-disk layout.
//!
//! Three archive-download acquisition kinds, matching the design in #1345:
//! - `hashicorp-release`: `https://api.releases.hashicorp.com/v1/releases/<product>/<version>`
//!   returns every build's URL plus a SHA256SUMS URL — this one kind covers
//!   terraform-ls and every other HashiCorp-shipped tool.
//! - `github-release`: the GitHub "latest release" API, matching `asset`
//!   after `{version}`/`{os}`/`{arch}` placeholder substitution.
//! - `url-template`: a plain URL with the same placeholders, no API call.
//!
//! Downloads go through the `curl` binary — already a hard runtime
//! dependency of `registry.rs` (extension registry fetch/script download),
//! present on Linux, WSL, macOS and Windows 10+ — rather than adding an
//! HTTP client crate; see the PR for the tradeoff. Archive unpacking is
//! in-process (the `zip` and `tar`+`flate2` crates), never the `unzip`/`tar`
//! binaries, so there is no `sh -c` anywhere in this module.
//!
//! Five more acquisition kinds (#1346) cover the package-manager ecosystems
//! a language's LSP/DAP tooling is usually published to instead of a
//! standalone release archive: `npm`, `pip`, `go`, `cargo`, `dotnet-tool`.
//! Each installs into a **private prefix** under the same
//! `paths::managed_tool_version_dir(tool, version)` directory the archive
//! kinds unpack into — `npm install --prefix`, a fresh `python3 -m venv`,
//! `GOBIN=<dir>/bin go install`, `cargo install --root`, and
//! `dotnet tool install --tool-path` all support installing into an
//! arbitrary directory the *caller* owns, which is exactly what replaces
//! today's `npm install -g` / global `pip install` / `go install` (into
//! `~/go/bin`) / `cargo install` (into `~/.cargo/bin`) shell strings — none
//! of which need `sudo` or touch anything outside vimcode's own managed
//! tools tree. See [`package_manager_argv`] for the exact argv built per
//! kind and [`package_manager_binary_path`] for where the installed binary
//! ends up, including the Windows `Scripts\`/`.exe` variants.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::extensions::Platform;

// ─── Manifest schema ────────────────────────────────────────────────────────

/// Which upstream convention an `[lsp.acquire]` / `[dap.acquire]` table
/// describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AcquireKind {
    HashicorpRelease,
    GithubRelease,
    #[default]
    UrlTemplate,
    /// `npm install --prefix <dir> <package>@<version>` (#1346).
    Npm,
    /// `python3 -m venv <dir>` then `<dir>/bin/pip install <package>` (#1346).
    Pip,
    /// `GOBIN=<dir>/bin go install <package>@<version>` (#1346).
    Go,
    /// `cargo install --root <dir> <package>` (#1346).
    Cargo,
    /// `dotnet tool install --tool-path <dir> <package>` (#1346).
    DotnetTool,
}

/// Parsed `[lsp.acquire]` / `[dap.acquire]` manifest table. Absent from a
/// manifest → the containing `LspConfig`/`DapConfig`'s `acquire` field is
/// `None` and every existing manifest (none of which declare this table) is
/// unaffected.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct AcquireConfig {
    /// `hashicorp-release` | `github-release` | `url-template`.
    pub kind: AcquireKind,
    /// HashiCorp product name (`hashicorp-release` only), e.g. `"terraform-ls"`.
    #[serde(default)]
    pub product: String,
    /// `owner/name` GitHub repo (`github-release` only).
    #[serde(default)]
    pub repo: String,
    /// Release asset filename template, e.g.
    /// `"foo-{version}-{os}-{arch}.tar.gz"` (`github-release` only).
    #[serde(default)]
    pub asset: String,
    /// Plain download URL template (`url-template` only), e.g.
    /// `"https://example.com/{version}/foo_{os}_{arch}.zip"`.
    #[serde(default)]
    pub url: String,
    /// Package/module/crate name to install (`npm`, `pip`, `go`, `cargo`,
    /// `dotnet-tool` only, #1346), e.g. `"pyright"` (npm),
    /// `"python-lsp-server"` (pip), `"golang.org/x/tools/gopls"` (go),
    /// `"ripgrep"` (cargo).
    #[serde(default)]
    pub package: String,
    /// Version to acquire, or `"latest"`. Defaults to `"latest"`.
    #[serde(default = "default_acquire_version")]
    pub version: String,
    /// Path of the executable inside the downloaded archive
    /// (`hashicorp-release`/`github-release`/`url-template`), e.g.
    /// `"terraform-ls"`. For a package-manager kind (`npm`/`pip`/`go`/
    /// `cargo`/`dotnet-tool`, #1346) this instead names the executable
    /// the install produces inside its private prefix, e.g. the console
    /// script `npm`/`pip` write out or the binary `go`/`cargo install`
    /// build. Defaults to the tool's own binary name (the resolver's
    /// lookup key passed to `acquire_and_install`) when empty.
    #[serde(default)]
    pub binary_path: String,
    /// Overrides the default OS-name mapping (keys: `"linux"`, `"macos"`,
    /// `"windows"`) for upstreams that use odd names.
    #[serde(default)]
    pub os_map: HashMap<String, String>,
    /// Overrides the default arch-name mapping (keys: `"amd64"`, `"arm64"`)
    /// for upstreams that use odd names.
    #[serde(default)]
    pub arch_map: HashMap<String, String>,
}

fn default_acquire_version() -> String {
    "latest".to_string()
}

/// A target CPU architecture for acquisition. Explicit (not `cfg!`-derived
/// only) for the same reason `extensions::Platform` is: a single test run
/// can assert every combination resolves correctly regardless of which
/// arch the test binary happens to be compiled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Amd64,
    Arm64,
}

impl Arch {
    /// The architecture this binary was actually compiled for.
    pub fn host() -> Arch {
        #[cfg(target_arch = "aarch64")]
        {
            Arch::Arm64
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            Arch::Amd64
        }
    }
}

// ─── OS / arch name resolution + template substitution ─────────────────────

fn platform_key(p: Platform) -> &'static str {
    match p {
        Platform::Linux => "linux",
        Platform::MacOS => "macos",
        Platform::Windows => "windows",
    }
}

fn arch_key(a: Arch) -> &'static str {
    match a {
        Arch::Amd64 => "amd64",
        Arch::Arm64 => "arm64",
    }
}

/// Default OS name used in download URLs/asset names for `platform`.
/// `"darwin"` for macOS is the near-universal Go-ecosystem convention
/// (HashiCorp, and most `GOOS`-named GitHub release assets); a manifest's
/// `os_map` overrides this per-extension when an upstream uses something
/// else (e.g. `"macos"` or `"osx"`).
fn default_os_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "linux",
        Platform::MacOS => "darwin",
        Platform::Windows => "windows",
    }
}

/// Default arch name used in download URLs/asset names for `arch`.
fn default_arch_name(arch: Arch) -> &'static str {
    match arch {
        Arch::Amd64 => "amd64",
        Arch::Arm64 => "arm64",
    }
}

/// Resolve the OS-name token to substitute for `{os}`, honouring `cfg.os_map`.
pub fn resolved_os(cfg: &AcquireConfig, platform: Platform) -> String {
    cfg.os_map
        .get(platform_key(platform))
        .cloned()
        .unwrap_or_else(|| default_os_name(platform).to_string())
}

/// Resolve the arch-name token to substitute for `{arch}`, honouring `cfg.arch_map`.
pub fn resolved_arch(cfg: &AcquireConfig, arch: Arch) -> String {
    cfg.arch_map
        .get(arch_key(arch))
        .cloned()
        .unwrap_or_else(|| default_arch_name(arch).to_string())
}

/// Substitute `{version}`, `{os}`, `{arch}` placeholders in a URL/asset template.
pub fn expand_template(template: &str, version: &str, os: &str, arch: &str) -> String {
    template
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch)
}

// ─── Package-manager acquisition (#1346) ───────────────────────────────────

/// The runtime binary that must already be on PATH before `kind` can be
/// used — `None` for the three archive-download kinds, which need nothing
/// but `curl`. Returning the *name* rather than probing PATH here keeps
/// this a pure function: the caller (`Engine::ext_install_from_registry`)
/// decides how to probe (the shared `binary_on_path`, which also checks the
/// vimcode-managed tools dir and the extra tool dirs, not just PATH) and
/// what to do when it's missing — fall through to the visible-terminal
/// tier with a dependency hint instead of failing silently, per #1346's
/// acceptance criteria.
pub fn package_manager_runtime(kind: AcquireKind) -> Option<&'static str> {
    match kind {
        AcquireKind::HashicorpRelease | AcquireKind::GithubRelease | AcquireKind::UrlTemplate => {
            None
        }
        AcquireKind::Npm => Some("npm"),
        // The venv step (`python3 -m venv`) is what actually needs probing —
        // `pip` itself doesn't exist yet until the venv is created.
        AcquireKind::Pip => Some("python3"),
        AcquireKind::Go => Some("go"),
        AcquireKind::Cargo => Some("cargo"),
        AcquireKind::DotnetTool => Some("dotnet"),
    }
}

/// Is `kind` one of the five package-manager kinds (#1346), as opposed to
/// one of the three archive-download kinds from #1345?
pub fn is_package_manager_kind(kind: AcquireKind) -> bool {
    package_manager_runtime(kind).is_some()
}

/// One process to spawn as part of a package-manager install: argv (index 0
/// is the program name, resolved the same way `git::hidden_command` and
/// every other vimcode child-process spawn resolves a program name — via
/// PATH, no shell) plus any extra environment variables that invocation
/// needs (`go install` is the one kind that needs one: `GOBIN`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManagerStep {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Build the argv (no shell string, ever — #1346 replaces the old
/// `npm install -g …` / `pip install …` / `go install …` / `cargo install
/// …` shell one-liners specifically because they need a shell or a global,
/// often root-owned, prefix) for installing `cfg.package` via `kind` into
/// the private prefix `dir` (always
/// `paths::managed_tool_version_dir(tool_name, version)` in production —
/// passed in explicitly here so this stays a pure, network-free function
/// tests can drive directly against a throwaway path).
///
/// `platform` only matters for `pip`'s second step (the venv's `pip`
/// lives at `<dir>/bin/pip` everywhere except `<dir>/Scripts/pip.exe` on
/// Windows) — every other kind's argv is platform-independent.
///
/// A pure function of `(kind, cfg, dir, platform)` — no PATH probing, no
/// process spawn, no network — so every kind's argv can be asserted for
/// every platform in one test run regardless of what happens to be
/// installed on (or what OS is running) the machine running the suite
/// (mirrors `resolve_asset`'s `url-template` branch, the one archive kind
/// #1345 could already test this way).
pub fn package_manager_argv(
    kind: AcquireKind,
    cfg: &AcquireConfig,
    dir: &Path,
    platform: Platform,
) -> Result<Vec<PackageManagerStep>, AcquireError> {
    if cfg.package.is_empty() {
        return Err(AcquireError::BadConfig(format!(
            "{kind:?} requires `package`"
        )));
    }
    let pinned_version = if cfg.version.is_empty() || cfg.version == "latest" {
        None
    } else {
        Some(cfg.version.as_str())
    };
    let dir_s = dir.to_string_lossy().to_string();
    match kind {
        AcquireKind::HashicorpRelease | AcquireKind::GithubRelease | AcquireKind::UrlTemplate => {
            Err(AcquireError::BadConfig(
                "package_manager_argv called with an archive-download kind".to_string(),
            ))
        }
        AcquireKind::Npm => {
            let spec = match pinned_version {
                Some(v) => format!("{}@{v}", cfg.package),
                None => cfg.package.clone(),
            };
            Ok(vec![PackageManagerStep {
                argv: vec![
                    "npm".to_string(),
                    "install".to_string(),
                    "--prefix".to_string(),
                    dir_s,
                    spec,
                ],
                env: vec![],
            }])
        }
        AcquireKind::Pip => {
            let spec = match pinned_version {
                Some(v) => format!("{}=={v}", cfg.package),
                None => cfg.package.clone(),
            };
            let pip = venv_pip_path(dir, platform).to_string_lossy().to_string();
            Ok(vec![
                PackageManagerStep {
                    argv: vec![
                        "python3".to_string(),
                        "-m".to_string(),
                        "venv".to_string(),
                        dir_s,
                    ],
                    env: vec![],
                },
                PackageManagerStep {
                    argv: vec![pip, "install".to_string(), spec],
                    env: vec![],
                },
            ])
        }
        AcquireKind::Go => {
            let spec = format!("{}@{}", cfg.package, pinned_version.unwrap_or("latest"));
            Ok(vec![PackageManagerStep {
                argv: vec!["go".to_string(), "install".to_string(), spec],
                env: vec![(
                    "GOBIN".to_string(),
                    dir.join("bin").to_string_lossy().to_string(),
                )],
            }])
        }
        AcquireKind::Cargo => {
            let mut argv = vec![
                "cargo".to_string(),
                "install".to_string(),
                "--root".to_string(),
                dir_s,
                cfg.package.clone(),
            ];
            if let Some(v) = pinned_version {
                argv.push("--version".to_string());
                argv.push(v.to_string());
            }
            Ok(vec![PackageManagerStep { argv, env: vec![] }])
        }
        AcquireKind::DotnetTool => {
            let mut argv = vec![
                "dotnet".to_string(),
                "tool".to_string(),
                "install".to_string(),
                "--tool-path".to_string(),
                dir_s,
                cfg.package.clone(),
            ];
            if let Some(v) = pinned_version {
                argv.push("--version".to_string());
                argv.push(v.to_string());
            }
            Ok(vec![PackageManagerStep { argv, env: vec![] }])
        }
    }
}

/// Where `pip` lives inside a venv created at `dir`, per platform — the
/// Windows `Scripts\` layout vs. everywhere else's `bin/`.
fn venv_pip_path(dir: &Path, platform: Platform) -> PathBuf {
    match platform {
        Platform::Windows => dir.join("Scripts").join("pip.exe"),
        Platform::Linux | Platform::MacOS => dir.join("bin").join("pip"),
    }
}

/// Append `.exe` to `bin` on Windows (unless it already ends with one),
/// leave it bare everywhere else — the same convention
/// `paths::managed_tool_binary_path` already applies when it probes for a
/// managed tool.
fn exe_named(bin: &str, platform: Platform) -> String {
    if platform == Platform::Windows && !bin.to_ascii_lowercase().ends_with(".exe") {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    }
}

/// Where a package-manager install of `kind` puts `bin` inside its private
/// prefix `dir`, per platform (#1346 acceptance: "binary-path resolution
/// per kind, including the Windows `Scripts\`/`.exe` variants"). A pure
/// function — no filesystem access — so every kind × platform combination
/// is assertable in one test run, the same reasoning `resolved_os`/
/// `resolved_arch` above already applies to the archive kinds.
pub fn package_manager_binary_path(
    kind: AcquireKind,
    dir: &Path,
    bin: &str,
    platform: Platform,
) -> PathBuf {
    match kind {
        AcquireKind::HashicorpRelease | AcquireKind::GithubRelease | AcquireKind::UrlTemplate => {
            dir.join(bin)
        }
        // `npm install --prefix <dir>` links executables into
        // `<dir>/node_modules/.bin/`. npm's Windows shim for a binary is a
        // `.cmd` file (not `.exe`), not just `<bin>.exe` with different
        // content — name it explicitly rather than reusing `exe_named`.
        AcquireKind::Npm => {
            let name = if platform == Platform::Windows {
                format!("{bin}.cmd")
            } else {
                bin.to_string()
            };
            dir.join("node_modules").join(".bin").join(name)
        }
        AcquireKind::Pip => match platform {
            Platform::Windows => dir.join("Scripts").join(format!("{bin}.exe")),
            Platform::Linux | Platform::MacOS => dir.join("bin").join(bin),
        },
        AcquireKind::Go | AcquireKind::Cargo => dir.join("bin").join(exe_named(bin, platform)),
        // `dotnet tool install --tool-path <dir>` puts the launcher directly
        // in `<dir>` — no nested `bin/`.
        AcquireKind::DotnetTool => dir.join(exe_named(bin, platform)),
    }
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AcquireError {
    /// Manifest config was incomplete/invalid for its declared `kind`.
    BadConfig(String),
    Network(String),
    Http(String),
    Parse(String),
    NoMatchingAsset,
    ChecksumMismatch {
        expected: String,
        actual: String,
    },
    Archive(String),
    PathTraversal(String),
    Io(String),
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcquireError::BadConfig(s) => write!(f, "invalid acquire config: {s}"),
            AcquireError::Network(s) => write!(f, "network error: {s}"),
            AcquireError::Http(s) => write!(f, "download failed: {s}"),
            AcquireError::Parse(s) => write!(f, "failed to parse release metadata: {s}"),
            AcquireError::NoMatchingAsset => {
                write!(f, "no release asset matches this platform/architecture")
            }
            AcquireError::ChecksumMismatch { expected, actual } => {
                write!(f, "checksum mismatch: expected {expected}, got {actual}")
            }
            AcquireError::Archive(s) => write!(f, "archive error: {s}"),
            AcquireError::PathTraversal(s) => {
                write!(f, "refusing to extract unsafe archive path: {s}")
            }
            AcquireError::Io(s) => write!(f, "I/O error: {s}"),
        }
    }
}

impl std::error::Error for AcquireError {}

impl From<std::io::Error> for AcquireError {
    fn from(e: std::io::Error) -> Self {
        AcquireError::Io(e.to_string())
    }
}

// ─── Checksum verification ──────────────────────────────────────────────────

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Verify `path`'s SHA-256 digest matches `expected_hex` (case-insensitive).
/// On mismatch, returns `Err` without touching the file — callers are
/// responsible for deleting the partial/tampered download.
pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<(), AcquireError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = to_hex(&hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_hex.trim()) {
        Ok(())
    } else {
        Err(AcquireError::ChecksumMismatch {
            expected: expected_hex.trim().to_string(),
            actual,
        })
    }
}

/// Parse a `SHA256SUMS`-style text file (`<hex>  <filename>` per line, as
/// published by HashiCorp and many GitHub releases) and return the digest
/// for `filename`.
pub fn find_sha256_for_filename(sums_text: &str, filename: &str) -> Option<String> {
    for line in sums_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        if name == filename || name.ends_with(&format!("/{filename}")) {
            return Some(hash.to_string());
        }
    }
    None
}

// ─── Archive extraction (in-process, no `unzip`/`tar` binary) ──────────────

/// Reject absolute paths and any `..` component — the zip-slip / tar-slip
/// guard. Applied both to the manifest's own `binary_path` and to every
/// archive entry name considered as a match, so a hostile archive can't
/// escape the extraction target even if a manifest's `binary_path` is safe.
fn is_safe_relative_path(p: &str) -> bool {
    let path = Path::new(p);
    if path.is_absolute() {
        return false;
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => return false,
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return false,
        }
    }
    true
}

/// Is `name` safe to use as a **single path segment** — i.e. as the
/// directory/file name a manifest-supplied tool name is joined onto a
/// vimcode-owned directory as?
///
/// Stricter than [`is_safe_relative_path`]: that one allows nested
/// (`a/b/c`) relative paths, which is right for an archive entry but wrong
/// for a tool name, since `PathBuf::join` resolves nothing and a name with
/// separators in it escapes the tree it is supposed to be keyed inside.
/// Both `/` and `\` are rejected explicitly because a backslash is a normal
/// filename character on Unix — `Path::new("a\\b")` is one component there,
/// so `components()` alone would let a Windows-flavoured traversal through
/// on the very platform where the join would later be interpreted.
///
/// Exported (#1345 review) because every destructive operation keyed by a
/// manifest-supplied name must gate on the same predicate:
/// `tool_acquire`'s own install path does, and `Engine::ext_remove_tools`'s
/// `remove_dir_all(managed_tool_dir(bin_name))` — the mirror-image removal
/// of what that install path creates — has to as well. `binary` is
/// free-form text from a community-submitted registry manifest, so
/// `binary = "../.."` must not be able to turn tool cleanup into a
/// recursive delete of an ancestor of `~/.local/share/vimcode/tools`.
pub(crate) fn is_safe_single_segment_name(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return false;
    }
    if !is_safe_relative_path(name) {
        return false;
    }
    // Exactly one *ordinary* component. `"."` would otherwise pass every
    // check above while naming the parent directory itself — i.e.
    // `remove_dir_all(managed_tools_dir())`, every managed tool at once.
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

/// Whether archive entry `entry_name` is the file the manifest's
/// `binary_path` names — either an exact match, or (when `binary_path` is a
/// bare filename with no directory component) a basename match, since many
/// archives wrap the binary in a version-named top-level directory.
fn entry_matches(entry_name: &str, binary_path: &str) -> bool {
    if entry_name == binary_path {
        return true;
    }
    let no_dir_component = !binary_path.contains('/') && !binary_path.contains('\\');
    no_dir_component && Path::new(entry_name).file_name() == Path::new(binary_path).file_name()
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Extract just `binary_path` from `archive_path` (`.zip`, `.tar.gz`, or
/// `.tgz`, detected from the filename) to `dest`, setting the executable
/// bit on unix. Rejects a `binary_path` — or a would-be-matching archive
/// entry — containing `..` or an absolute path (zip-slip/tar-slip).
pub fn extract_binary_from_archive(
    archive_path: &Path,
    binary_path: &str,
    dest: &Path,
) -> Result<(), AcquireError> {
    if !is_safe_relative_path(binary_path) {
        return Err(AcquireError::PathTraversal(binary_path.to_string()));
    }
    let lower = archive_path.to_string_lossy().to_lowercase();
    if lower.ends_with(".zip") {
        extract_binary_from_zip(archive_path, binary_path, dest)
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        extract_binary_from_tar_gz(archive_path, binary_path, dest)
    } else {
        Err(AcquireError::Archive(format!(
            "unsupported archive format: {}",
            archive_path.display()
        )))
    }
}

fn extract_binary_from_zip(
    archive_path: &Path,
    binary_path: &str,
    dest: &Path,
) -> Result<(), AcquireError> {
    let file = std::fs::File::open(archive_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| AcquireError::Archive(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AcquireError::Archive(e.to_string()))?;
        let name = entry.name().to_string();
        if !is_safe_relative_path(&name) {
            continue;
        }
        if entry_matches(&name, binary_path) {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            set_executable(dest)?;
            return Ok(());
        }
    }
    Err(AcquireError::Archive(format!(
        "{binary_path} not found in archive"
    )))
}

fn extract_binary_from_tar_gz(
    archive_path: &Path,
    binary_path: &str,
    dest: &Path,
) -> Result<(), AcquireError> {
    let file = std::fs::File::open(archive_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| AcquireError::Archive(e.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| AcquireError::Archive(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| AcquireError::Archive(e.to_string()))?
            .to_path_buf();
        let name = path.to_string_lossy().to_string();
        if !is_safe_relative_path(&name) {
            continue;
        }
        if entry_matches(&name, binary_path) {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            set_executable(dest)?;
            return Ok(());
        }
    }
    Err(AcquireError::Archive(format!(
        "{binary_path} not found in archive"
    )))
}

// ─── Network: release resolution + download ─────────────────────────────────

/// A resolved download: the concrete URL to fetch, the concrete version it
/// corresponds to (`"latest"` resolved to a real version number), and its
/// SHA-256 checksum when the upstream publishes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAsset {
    pub url: String,
    pub version: String,
    pub sha256: Option<String>,
}

fn http_get_bytes(url: &str, timeout_secs: u32) -> Result<Vec<u8>, AcquireError> {
    let output = crate::core::git::hidden_command("curl")
        .args(["-sfL", "--max-time", &timeout_secs.to_string(), url])
        .output()
        .map_err(|e| AcquireError::Network(e.to_string()))?;
    if !output.status.success() {
        return Err(AcquireError::Http(format!("curl failed for {url}")));
    }
    Ok(output.stdout)
}

/// Env var that lets the **integration**-test crates (`tests/*.rs`, which are
/// separate compilation units where this crate's `cfg(test)` is *off*) opt
/// back into `file://` download fixtures. Deliberately not a plain runtime
/// flag: the read is itself inside `#[cfg(debug_assertions)]`, so a release
/// build — i.e. every shipped vimcode binary — has no code path that can
/// enable `file://` downloads at all, whatever the environment says.
///
/// Defined in every configuration (so `tests/extensions.rs` can name it
/// instead of hard-coding the string) but *read* in only one — see
/// [`file_url_downloads_allowed`].
pub const ALLOW_FILE_URL_DOWNLOADS_ENV: &str = "VIMCODE_TEST_ALLOW_FILE_URL_DOWNLOADS";

/// Is a `file://` download URL acceptable in *this* build?
///
/// Second review round on #1345: `file://` used to be accepted
/// unconditionally, in production code, next to `https://`. That is an
/// arbitrary-file-read primitive reachable from a community-submitted or
/// compromised registry manifest — `[lsp.acquire]`/`[dap.acquire]` with
/// `kind = "url-template"` and `url = "file:///home/user/.ssh/id_rsa"` made
/// `download_asset` shell out to `curl -sfL -o <staging> file://…` (curl
/// enables the `file` protocol by default), copying any file the vimcode
/// process can read into a predictable `temp_dir()` staging path before any
/// archive-format check runs. The allowance now exists **only** in test
/// builds:
///
/// * in-crate unit/driver tests (`cfg(test)`) — `tool_acquire.rs`'s own
///   end-to-end fixtures and `tui_main::shell_app`'s driver tests — get it
///   for free;
/// * integration-test crates opt in explicitly via
///   [`ALLOW_FILE_URL_DOWNLOADS_ENV`], and even that read is compiled out
///   of `--release` builds by the `debug_assertions` gate.
///
/// A shipped binary therefore accepts `https://` and nothing else.
fn file_url_downloads_allowed() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(all(not(test), debug_assertions))]
    {
        std::env::var_os(ALLOW_FILE_URL_DOWNLOADS_ENV).is_some_and(|v| v == "1")
    }
    #[cfg(all(not(test), not(debug_assertions)))]
    {
        false
    }
}

/// Reject any download URL that isn't `https://`. A pure, network-free check
/// so it's independently unit-testable (review finding on #1345 — the
/// acceptance criteria say "Download over https only", and neither
/// `resolve_url_template` nor the old inline check in
/// `install_resolved_asset` enforced it): a manifest (or a compromised/
/// typo'd registry entry) declaring a plain `http://` URL is rejected here
/// rather than fetched in the clear, for every acquire kind
/// (`hashicorp-release`, `github-release`, `url-template` all funnel
/// through `install_resolved_asset`, so this is the one choke point that
/// covers all three).
///
/// `file://` is accepted *only* in test builds — see
/// [`file_url_downloads_allowed`] for why that is a compile-time property
/// and not a runtime one.
fn validate_download_url(url: &str) -> Result<(), AcquireError> {
    validate_download_url_with(url, file_url_downloads_allowed())
}

/// The scheme check itself, with the `file://` allowance passed in rather
/// than read from the build configuration — so the unit tests below can
/// assert the exact behaviour of a **release** build (`allow_file_urls =
/// false`) even though they necessarily run in a `cfg(test)` one.
fn validate_download_url_with(url: &str, allow_file_urls: bool) -> Result<(), AcquireError> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if allow_file_urls && url.starts_with("file://") {
        return Ok(());
    }
    Err(AcquireError::BadConfig(format!(
        "refusing to download over a non-https URL: {url}"
    )))
}

/// Download the resolved asset archive to `dest`. Deliberately its own
/// `curl` invocation rather than reusing `registry::download_script`
/// (review finding on #1345): that helper omits `-L`, and a GitHub release
/// asset's `browser_download_url` is well known to 302-redirect through
/// `objects.githubusercontent.com` — without `-L`, `curl -f` doesn't treat
/// the redirect as an error, so the "download" can silently succeed while
/// writing a near-empty/HTML body instead of the real archive. `-sfL`
/// matches the flag set `http_get_bytes` above already uses for the JSON
/// API calls in this module. `file://` URLs (used by this module's own
/// tests, see the `tests` block below — and rejected outright in a release
/// build, see `validate_download_url`) are unaffected by `-L`.
fn download_asset(url: &str, dest: &Path) -> Result<(), AcquireError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = crate::core::git::hidden_command("curl")
        .args(["-sfL", "--max-time", "120", "-o"])
        .arg(dest)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| AcquireError::Network(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(AcquireError::Http(format!("curl failed to download {url}")))
    }
}

/// Field names below (`url_shasums`, no per-build `filename`) were
/// confirmed against the live API
/// (`https://api.releases.hashicorp.com/v1/releases/terraform-ls/latest`)
/// during development — the response shape isn't formally documented
/// anywhere vimcode can pin a schema to.
fn resolve_hashicorp_release(
    cfg: &AcquireConfig,
    platform: Platform,
    arch: Arch,
) -> Result<ResolvedAsset, AcquireError> {
    if cfg.product.is_empty() {
        return Err(AcquireError::BadConfig(
            "hashicorp-release requires `product`".to_string(),
        ));
    }
    let version_seg = if cfg.version.is_empty() || cfg.version == "latest" {
        "latest"
    } else {
        cfg.version.as_str()
    };
    let api_url = format!(
        "https://api.releases.hashicorp.com/v1/releases/{}/{}",
        cfg.product, version_seg
    );
    let bytes = http_get_bytes(&api_url, 15)?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| AcquireError::Parse(e.to_string()))?;

    let os = resolved_os(cfg, platform);
    let arch_s = resolved_arch(cfg, arch);
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or(version_seg)
        .to_string();
    let builds = json
        .get("builds")
        .and_then(|v| v.as_array())
        .ok_or(AcquireError::NoMatchingAsset)?;
    let build = builds
        .iter()
        .find(|b| {
            b.get("os").and_then(|v| v.as_str()) == Some(os.as_str())
                && b.get("arch").and_then(|v| v.as_str()) == Some(arch_s.as_str())
        })
        .ok_or(AcquireError::NoMatchingAsset)?;
    let url = build
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or(AcquireError::NoMatchingAsset)?
        .to_string();
    // The API's build objects carry no explicit filename field — derive it
    // from the download URL itself, which is what the SHA256SUMS file's own
    // entries are keyed by.
    let filename = url.rsplit('/').next().unwrap_or_default().to_string();

    // `url_shasums` (not `shasums_url`) is the real field name — see
    // `resolve_hashicorp_release`'s doc comment for how this was confirmed.
    let sha256 = json
        .get("url_shasums")
        .and_then(|v| v.as_str())
        .and_then(|shasums_url| http_get_bytes(shasums_url, 15).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|text| find_sha256_for_filename(&text, &filename));

    Ok(ResolvedAsset {
        url,
        version,
        sha256,
    })
}

fn resolve_github_release(
    cfg: &AcquireConfig,
    platform: Platform,
    arch: Arch,
) -> Result<ResolvedAsset, AcquireError> {
    if cfg.repo.is_empty() || cfg.asset.is_empty() {
        return Err(AcquireError::BadConfig(
            "github-release requires `repo` and `asset`".to_string(),
        ));
    }
    let version_seg = if cfg.version.is_empty() || cfg.version == "latest" {
        "latest".to_string()
    } else {
        format!("tags/{}", cfg.version)
    };
    let api_url = format!(
        "https://api.github.com/repos/{}/releases/{}",
        cfg.repo, version_seg
    );
    let bytes = http_get_bytes(&api_url, 15)?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| AcquireError::Parse(e.to_string()))?;

    let version = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&cfg.version)
        .to_string();
    let os = resolved_os(cfg, platform);
    let arch_s = resolved_arch(cfg, arch);
    let want_name = expand_template(&cfg.asset, &version, &os, &arch_s);

    let assets = json
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or(AcquireError::NoMatchingAsset)?;
    let asset = assets
        .iter()
        .find(|a| a.get("name").and_then(|v| v.as_str()) == Some(want_name.as_str()))
        .ok_or(AcquireError::NoMatchingAsset)?;
    let url = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .ok_or(AcquireError::NoMatchingAsset)?
        .to_string();

    // GitHub releases have no standard checksum-file location; a mismatch
    // here is silently absent rather than fabricated. Callers should still
    // prefer `hashicorp-release` when available for exactly this reason.
    //
    // Review note (#1345): this makes the "SHA-256 verification is mandatory
    // whenever the upstream publishes checksums" criterion vacuously true for
    // this kind — the upstream publishes nothing to verify against, so the
    // only integrity guarantee a `github-release` acquisition has is TLS
    // (`validate_download_url` refuses anything but `https://`). A registry
    // manifest should therefore use `hashicorp-release`, or a `url-template`
    // pointing at a checksummed artifact, whenever the project offers one,
    // and `github-release` only as a last resort. Worth restating in the
    // extension-authoring docs when they land.
    Ok(ResolvedAsset {
        url,
        version,
        sha256: None,
    })
}

fn resolve_url_template(
    cfg: &AcquireConfig,
    platform: Platform,
    arch: Arch,
) -> Result<ResolvedAsset, AcquireError> {
    if cfg.url.is_empty() {
        return Err(AcquireError::BadConfig(
            "url-template requires `url`".to_string(),
        ));
    }
    if cfg.version.is_empty() || cfg.version == "latest" {
        return Err(AcquireError::BadConfig(
            "url-template requires a pinned `version` (no release API to resolve \"latest\" against)"
                .to_string(),
        ));
    }
    let os = resolved_os(cfg, platform);
    let arch_s = resolved_arch(cfg, arch);
    let url = expand_template(&cfg.url, &cfg.version, &os, &arch_s);
    Ok(ResolvedAsset {
        url,
        version: cfg.version.clone(),
        sha256: None,
    })
}

/// Resolve `cfg` to a concrete downloadable asset for `platform`/`arch`.
/// Hits the network for `hashicorp-release`/`github-release`; pure for
/// `url-template`. Only meaningful for the three archive-download kinds —
/// `acquire_and_install_for` routes every package-manager kind (#1346) to
/// [`install_via_package_manager`] before this is ever called, so the
/// fallback arm below should be unreachable in practice.
pub fn resolve_asset(
    cfg: &AcquireConfig,
    platform: Platform,
    arch: Arch,
) -> Result<ResolvedAsset, AcquireError> {
    match cfg.kind {
        AcquireKind::HashicorpRelease => resolve_hashicorp_release(cfg, platform, arch),
        AcquireKind::GithubRelease => resolve_github_release(cfg, platform, arch),
        AcquireKind::UrlTemplate => resolve_url_template(cfg, platform, arch),
        AcquireKind::Npm
        | AcquireKind::Pip
        | AcquireKind::Go
        | AcquireKind::Cargo
        | AcquireKind::DotnetTool => Err(AcquireError::BadConfig(format!(
            "{:?} is a package-manager kind, not an archive-download kind",
            cfg.kind
        ))),
    }
}

// ─── Orchestration: download, verify, unpack, install atomically ──────────

/// Download, verify, unpack and install `cfg` for tool `tool_name` (the
/// resolver's lookup key — normally the manifest's `lsp.binary`/`dap.binary`).
/// Runs entirely off the caller's thread budget by design — the engine
/// wraps this in `std::thread::spawn`, mirroring `Engine::ext_refresh`'s
/// background-fetch pattern.
///
/// On success, installs into `paths::managed_tool_version_dir(tool_name,
/// &resolved_version)` and flips `paths::set_managed_tool_current`, then
/// returns the absolute path to the now-resolvable binary. A checksum
/// mismatch or extraction failure deletes the partial download/unpack and
/// leaves any previously-installed version untouched.
pub fn acquire_and_install(tool_name: &str, cfg: &AcquireConfig) -> Result<PathBuf, AcquireError> {
    acquire_and_install_for(tool_name, cfg, Platform::host(), Arch::host())
}

/// Same as [`acquire_and_install`], parameterised over platform/arch —
/// the seam tests use to exercise every platform/arch combination without
/// depending on the host the suite happens to run on.
pub fn acquire_and_install_for(
    tool_name: &str,
    cfg: &AcquireConfig,
    platform: Platform,
    arch: Arch,
) -> Result<PathBuf, AcquireError> {
    if is_package_manager_kind(cfg.kind) {
        return install_via_package_manager(tool_name, cfg, platform);
    }
    let asset = resolve_asset(cfg, platform, arch)?;
    let binary_path = if cfg.binary_path.is_empty() {
        tool_name.to_string()
    } else {
        cfg.binary_path.clone()
    };
    install_resolved_asset(tool_name, &asset, &binary_path)
}

/// The download/verify/unpack/atomic-install half of
/// [`acquire_and_install_for`], split out so tests can drive it directly
/// against a hand-built [`ResolvedAsset`] — including one carrying a
/// deliberately-wrong `sha256` — without needing `resolve_asset`'s network
/// calls to produce one.
fn install_resolved_asset(
    tool_name: &str,
    asset: &ResolvedAsset,
    binary_path: &str,
) -> Result<PathBuf, AcquireError> {
    if !is_safe_relative_path(tool_name) || tool_name.is_empty() {
        return Err(AcquireError::PathTraversal(tool_name.to_string()));
    }
    if !is_safe_relative_path(&asset.version) || asset.version.is_empty() {
        return Err(AcquireError::PathTraversal(asset.version.clone()));
    }
    validate_download_url(&asset.url)?;

    // Download + extract scratch space: a plain system temp dir is fine
    // here because nothing under it is ever `rename`d across a filesystem
    // boundary — only the *staging* directory below (built inside the
    // managed tools tree itself) is.
    let dl_tmp_dir = std::env::temp_dir().join(format!(
        "vimcode-acquire-dl-{tool_name}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    std::fs::create_dir_all(&dl_tmp_dir)?;
    let cleanup_dl = |dir: &Path| {
        let _ = std::fs::remove_dir_all(dir);
    };

    let archive_name = asset
        .url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download");
    let archive_path = dl_tmp_dir.join(archive_name);
    if let Err(e) = download_asset(&asset.url, &archive_path) {
        cleanup_dl(&dl_tmp_dir);
        return Err(e);
    }

    if let Some(expected) = &asset.sha256 {
        if let Err(e) = verify_sha256(&archive_path, expected) {
            cleanup_dl(&dl_tmp_dir);
            return Err(e);
        }
    }

    let extracted_name = Path::new(binary_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| tool_name.to_string());

    // Stage the unpacked binary *inside the managed tools tree* rather than
    // under the system temp dir (review finding on #1345): staging under
    // `std::env::temp_dir()` and then `rename`-ing into
    // `managed_tool_version_dir` fails with `EXDEV` whenever `/tmp` and the
    // data dir are on different filesystems/mounts — a common layout
    // (tmpfs `/tmp`, or a separate `/home` partition on many distros).
    // Staging as a sibling of the final version dir under the same tool
    // directory guarantees the swap-in `rename` is same-filesystem;
    // `rename_or_copy` below still falls back to copy+remove for the rare
    // case (e.g. a read-only bind mount split across the tool dir) where it
    // somehow isn't.
    let tool_dir = super::paths::managed_tool_dir(tool_name);
    std::fs::create_dir_all(&tool_dir)?;
    let stage_dir = tool_dir.join(format!(
        ".tmp-{}-{}-{}",
        asset.version,
        std::process::id(),
        unique_suffix()
    ));
    let cleanup_stage = |dir: &Path| {
        let _ = std::fs::remove_dir_all(dir);
    };
    if let Err(e) = std::fs::create_dir_all(&stage_dir) {
        cleanup_dl(&dl_tmp_dir);
        return Err(e.into());
    }
    let staged_binary = stage_dir.join(&extracted_name);
    if let Err(e) = extract_binary_from_archive(&archive_path, binary_path, &staged_binary) {
        cleanup_stage(&stage_dir);
        cleanup_dl(&dl_tmp_dir);
        return Err(e);
    }
    cleanup_dl(&dl_tmp_dir);

    // Atomic install: the whole version directory is built in a staging
    // location alongside it, then a single `rename` (or copy+remove
    // fallback) swaps it into place.
    let version_dir = super::paths::managed_tool_version_dir(tool_name, &asset.version);
    if version_dir.exists() {
        std::fs::remove_dir_all(&version_dir)?;
    }
    if let Err(e) = rename_or_copy(&stage_dir, &version_dir) {
        cleanup_stage(&stage_dir);
        return Err(e.into());
    }

    super::paths::set_managed_tool_current(tool_name, &asset.version)?;
    Ok(version_dir.join(&extracted_name))
}

/// Install `cfg` (one of the five #1346 package-manager kinds) for tool
/// `tool_name` by spawning `package_manager_argv`'s steps directly — no
/// shell, so there is nowhere for `cfg.package`/`cfg.version` (free-form
/// text from a community-submitted registry manifest) to break out of the
/// argument list the way it could if this built a `sh -c "npm install …"`
/// string instead.
///
/// Installs into the **same** `paths::managed_tool_version_dir(tool_name,
/// version)` the archive kinds unpack into (a private prefix vimcode owns,
/// never a global `~/.cargo/bin`/`~/go/bin`/`npm -g` prefix that can need
/// `sudo`), staged as a sibling directory first and atomically swapped in —
/// mirroring `install_resolved_asset`'s staging/rename pattern exactly.
///
/// Unlike the archive kinds, a package-manager install has no upstream API
/// to resolve `"latest"` to a concrete version number (`resolve_asset`
/// deliberately requires a pin for `url-template` for the same reason).
/// An unpinned `cfg.version` therefore installs into the literal `"latest"`
/// version directory, which a later unpinned reinstall overwrites in place.
fn install_via_package_manager(
    tool_name: &str,
    cfg: &AcquireConfig,
    platform: Platform,
) -> Result<PathBuf, AcquireError> {
    if !is_safe_relative_path(tool_name) || tool_name.is_empty() {
        return Err(AcquireError::PathTraversal(tool_name.to_string()));
    }
    let version = if cfg.version.is_empty() {
        "latest".to_string()
    } else {
        cfg.version.clone()
    };
    if !is_safe_relative_path(&version) || version.is_empty() {
        return Err(AcquireError::PathTraversal(version));
    }
    let bin_name = if cfg.binary_path.is_empty() {
        tool_name.to_string()
    } else {
        cfg.binary_path.clone()
    };

    let tool_dir = super::paths::managed_tool_dir(tool_name);
    std::fs::create_dir_all(&tool_dir)?;
    let stage_dir = tool_dir.join(format!(
        ".tmp-{version}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    let cleanup_stage = |dir: &Path| {
        let _ = std::fs::remove_dir_all(dir);
    };
    std::fs::create_dir_all(&stage_dir)?;

    let steps = match package_manager_argv(cfg.kind, cfg, &stage_dir, platform) {
        Ok(steps) => steps,
        Err(e) => {
            cleanup_stage(&stage_dir);
            return Err(e);
        }
    };
    if let Err(e) = run_package_manager_steps(&steps) {
        cleanup_stage(&stage_dir);
        return Err(e);
    }

    let installed_bin = package_manager_binary_path(cfg.kind, &stage_dir, &bin_name, platform);
    if !installed_bin.is_file() {
        cleanup_stage(&stage_dir);
        return Err(AcquireError::Archive(format!(
            "expected {} to exist after install but it does not",
            installed_bin.display()
        )));
    }

    // `paths::managed_tool_binary_path` (the resolver every other acquired
    // tool goes through) only looks for `<version_dir>/<bin>` (or, on
    // Windows, `<version_dir>/<bin>.exe`) directly — it has no idea a
    // package manager nested the real executable under `bin/` or
    // `node_modules/.bin/`. Link (unix) or copy (Windows, where an
    // unprivileged symlink needs Developer Mode) the installed binary up to
    // that top-level name so discovery agrees with every archive-kind
    // install, with no changes needed to that shared resolver.
    let link_name = match link_binary_into_prefix(&installed_bin, &stage_dir, &bin_name, platform) {
        Ok(name) => name,
        Err(e) => {
            cleanup_stage(&stage_dir);
            return Err(e);
        }
    };

    let version_dir = super::paths::managed_tool_version_dir(tool_name, &version);
    if version_dir.exists() {
        std::fs::remove_dir_all(&version_dir)?;
    }
    if let Err(e) = rename_or_copy(&stage_dir, &version_dir) {
        cleanup_stage(&stage_dir);
        return Err(e.into());
    }

    super::paths::set_managed_tool_current(tool_name, &version)?;
    Ok(version_dir.join(link_name))
}

/// Spawn every step of a package-manager install in order (argv only, never
/// a shell — see `package_manager_argv`'s doc comment), stopping at the
/// first failure. Split out of `install_via_package_manager` so a test can
/// drive it directly against a hand-built [`PackageManagerStep`] (e.g. one
/// invoking the real `false`/`cmd /c exit 1` binary) without needing a real
/// `npm`/`pip`/`go`/`cargo`/`dotnet` install to fail on cue.
fn run_package_manager_steps(steps: &[PackageManagerStep]) -> Result<(), AcquireError> {
    for step in steps {
        let Some(program) = step.argv.first() else {
            continue;
        };
        let mut command = crate::core::git::hidden_command(program);
        command.args(&step.argv[1..]);
        for (key, value) in &step.env {
            command.env(key, value);
        }
        let output = command
            .output()
            .map_err(|e| AcquireError::Network(format!("failed to spawn {program}: {e}")))?;
        if !output.status.success() {
            return Err(AcquireError::Archive(format!(
                "`{}` failed: {}",
                step.argv.join(" "),
                // #1346 acceptance: "on failure, surface the last lines of
                // stderr in the notification" — never the whole, potentially
                // huge, npm/cargo/dotnet build log.
                tail_of(&String::from_utf8_lossy(&output.stderr), 20)
            )));
        }
    }
    Ok(())
}

/// Last `n` non-empty lines of `text` — used to surface a package manager's
/// own diagnostics (the acceptance criterion: "on failure, surface the
/// last lines of stderr in the notification") without dumping a
/// potentially huge npm/cargo/dotnet build log into the status line.
fn tail_of(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Make the package-manager-installed binary at `installed_bin` (somewhere
/// under `stage_dir`, per `package_manager_binary_path`) reachable at
/// `stage_dir`'s top level under `bin_name` (plus a platform-appropriate
/// extension), and return that top-level name. See
/// `install_via_package_manager`'s doc comment for why this has to exist at
/// all.
///
/// unix: a *relative* symlink — relative because `stage_dir` is renamed
/// (not copied) into its final `version_dir` location on the common path,
/// and an absolute symlink baked with the staging path would dangle the
/// instant that rename happens. A relative link computed from two paths
/// that are both still under `stage_dir` survives the rename unchanged.
///
/// Windows: a plain copy — creating a symlink there needs Developer Mode or
/// admin rights, neither guaranteed, so this trades a few extra KB on disk
/// (npm/pip console scripts; go/cargo/dotnet-tool binaries can be tens of
/// MB, but those install directly to `bin/<name>.exe`/`<name>.exe`, exactly
/// one physical copy either way) for working unprivileged.
#[cfg(unix)]
fn link_binary_into_prefix(
    installed_bin: &Path,
    stage_dir: &Path,
    bin_name: &str,
    _platform: Platform,
) -> Result<String, AcquireError> {
    let link_path = stage_dir.join(bin_name);
    // `dotnet tool install --tool-path` already puts the launcher directly
    // at the top level (see `package_manager_binary_path`'s `DotnetTool`
    // arm) — nothing to link, and symlinking a path to itself would delete
    // the just-installed binary via the `remove_file` below then create a
    // symlink pointing at its own (now-missing) former self.
    if installed_bin == link_path {
        return Ok(bin_name.to_string());
    }
    let rel_target = installed_bin.strip_prefix(stage_dir).map_err(|_| {
        AcquireError::Archive(format!(
            "installed binary {} is not under its own staging prefix {}",
            installed_bin.display(),
            stage_dir.display()
        ))
    })?;
    if link_path.symlink_metadata().is_ok() {
        std::fs::remove_file(&link_path)?;
    }
    std::os::unix::fs::symlink(rel_target, &link_path)?;
    Ok(bin_name.to_string())
}

#[cfg(not(unix))]
fn link_binary_into_prefix(
    installed_bin: &Path,
    stage_dir: &Path,
    bin_name: &str,
    _platform: Platform,
) -> Result<String, AcquireError> {
    // Preserve `installed_bin`'s own extension rather than always forcing
    // `.exe`: npm's Windows shim for a binary is a `.cmd` file, not a PE
    // executable with a `.exe` name slapped on, and `CreateProcess` (what
    // `std::process::Command` uses) needs the real extension to run it —
    // `paths::managed_tool_binary_path`'s explicit `.exe` probe simply
    // won't match an `.cmd` shim, so it falls back to its "sole file in the
    // directory" scan, which still finds it as long as this is the only
    // top-level file (true for every one of these kinds).
    let name = match installed_bin.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{bin_name}.{ext}"),
        None => bin_name.to_string(),
    };
    let link_path = stage_dir.join(&name);
    if link_path != *installed_bin {
        std::fs::copy(installed_bin, &link_path)?;
    }
    Ok(name)
}

/// Move `src` to `dst` via `rename`, falling back to a recursive copy +
/// `remove_dir_all` of `src` when `rename` fails (e.g. `EXDEV` if `src` and
/// `dst` somehow end up on different filesystems despite `src` being staged
/// as a sibling of `dst`— a read-only bind mount split across the tool
/// directory, for instance). `install_resolved_asset` always stages `src`
/// under the same parent directory as `dst`, so the fallback path is
/// defense in depth rather than the common case.
fn rename_or_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_dir_recursive(src, dst)?;
    std::fs::remove_dir_all(src)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            // `fs::copy` already preserves the source file's permission
            // bits (including the executable bit `set_executable` set
            // during extraction) on every platform where that concept
            // exists, so no separate `set_permissions` call is needed here.
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Cheap per-call uniqueness for the scratch dir name — `std::process::id()`
/// alone collides if `acquire_and_install` runs twice in one process (e.g.
/// an LSP leg and a DAP leg of the same `:ExtInstall`, each on its own
/// background thread).
fn unique_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── manifest parse ──────────────────────────────────────────────────

    #[test]
    fn acquire_table_absent_parses_to_none() {
        use crate::core::extensions::{ExtensionManifest, LspConfig};
        let toml = r#"
name = "test"
display_name = "Test"
[lsp]
binary = "test-lsp"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert!(m.lsp.acquire.is_none());
        let _ = LspConfig::default(); // sanity: still constructible with no acquire
    }

    #[test]
    fn acquire_table_parses_hashicorp_release() {
        use crate::core::extensions::ExtensionManifest;
        let toml = r#"
name = "terraform"
display_name = "Terraform"
[lsp]
binary = "terraform-ls"
[lsp.acquire]
kind = "hashicorp-release"
product = "terraform-ls"
binary_path = "terraform-ls"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        let acquire = m.lsp.acquire.expect("acquire table should be present");
        assert_eq!(acquire.kind, AcquireKind::HashicorpRelease);
        assert_eq!(acquire.product, "terraform-ls");
        assert_eq!(acquire.version, "latest");
        assert_eq!(acquire.binary_path, "terraform-ls");
    }

    #[test]
    fn acquire_table_parses_github_release_with_os_arch_maps() {
        use crate::core::extensions::ExtensionManifest;
        let toml = r#"
name = "example"
display_name = "Example"
[lsp]
binary = "example-lsp"
[lsp.acquire]
kind = "github-release"
repo = "example/example-lsp"
asset = "example-lsp-{os}-{arch}.zip"
binary_path = "example-lsp"
[lsp.acquire.os_map]
macos = "osx"
[lsp.acquire.arch_map]
arm64 = "aarch64"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        let acquire = m.lsp.acquire.expect("acquire table should be present");
        assert_eq!(acquire.kind, AcquireKind::GithubRelease);
        assert_eq!(acquire.repo, "example/example-lsp");
        assert_eq!(acquire.os_map.get("macos"), Some(&"osx".to_string()));
        assert_eq!(acquire.arch_map.get("arm64"), Some(&"aarch64".to_string()));
    }

    #[test]
    fn dap_acquire_table_parses_independently_of_lsp() {
        use crate::core::extensions::ExtensionManifest;
        let toml = r#"
name = "example"
display_name = "Example"
[dap]
adapter = "example-dap"
binary = "example-dap"
[dap.acquire]
kind = "url-template"
url = "https://example.com/{version}/example-dap-{os}-{arch}.tar.gz"
version = "1.2.3"
binary_path = "example-dap"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert!(m.lsp.acquire.is_none());
        let acquire = m.dap.acquire.expect("dap acquire table should be present");
        assert_eq!(acquire.kind, AcquireKind::UrlTemplate);
        assert_eq!(acquire.version, "1.2.3");
    }

    // ── placeholder substitution / os-arch mapping (#1345 acceptance) ────

    #[test]
    fn expand_template_substitutes_all_placeholders() {
        assert_eq!(
            expand_template(
                "foo-{version}-{os}-{arch}.tar.gz",
                "1.2.3",
                "linux",
                "amd64"
            ),
            "foo-1.2.3-linux-amd64.tar.gz"
        );
    }

    #[test]
    fn resolved_os_defaults_cover_every_platform() {
        let cfg = AcquireConfig::default();
        assert_eq!(resolved_os(&cfg, Platform::Linux), "linux");
        assert_eq!(resolved_os(&cfg, Platform::MacOS), "darwin");
        assert_eq!(resolved_os(&cfg, Platform::Windows), "windows");
    }

    #[test]
    fn resolved_arch_defaults_cover_amd64_and_arm64() {
        let cfg = AcquireConfig::default();
        assert_eq!(resolved_arch(&cfg, Arch::Amd64), "amd64");
        assert_eq!(resolved_arch(&cfg, Arch::Arm64), "arm64");
    }

    #[test]
    fn os_map_override_wins_over_default_on_every_platform() {
        let mut cfg = AcquireConfig::default();
        cfg.os_map.insert("linux".to_string(), "lin".to_string());
        cfg.os_map.insert("macos".to_string(), "osx".to_string());
        cfg.os_map.insert("windows".to_string(), "win".to_string());
        assert_eq!(resolved_os(&cfg, Platform::Linux), "lin");
        assert_eq!(resolved_os(&cfg, Platform::MacOS), "osx");
        assert_eq!(resolved_os(&cfg, Platform::Windows), "win");
    }

    #[test]
    fn arch_map_override_wins_over_default_for_both_arches() {
        let mut cfg = AcquireConfig::default();
        cfg.arch_map
            .insert("amd64".to_string(), "x86_64".to_string());
        cfg.arch_map
            .insert("arm64".to_string(), "aarch64".to_string());
        assert_eq!(resolved_arch(&cfg, Arch::Amd64), "x86_64");
        assert_eq!(resolved_arch(&cfg, Arch::Arm64), "aarch64");
    }

    #[test]
    fn full_url_resolution_across_all_platform_arch_combinations() {
        // Exercises the acceptance criterion directly: linux/macos/windows
        // × amd64/arm64, six combinations, one template.
        let cfg = AcquireConfig {
            kind: AcquireKind::UrlTemplate,
            url: "https://example.com/tool/{version}/tool_{os}_{arch}.zip".to_string(),
            version: "9.9.9".to_string(),
            ..Default::default()
        };
        let expected = [
            (Platform::Linux, Arch::Amd64, "linux_amd64"),
            (Platform::Linux, Arch::Arm64, "linux_arm64"),
            (Platform::MacOS, Arch::Amd64, "darwin_amd64"),
            (Platform::MacOS, Arch::Arm64, "darwin_arm64"),
            (Platform::Windows, Arch::Amd64, "windows_amd64"),
            (Platform::Windows, Arch::Arm64, "windows_arm64"),
        ];
        for (platform, arch, tag) in expected {
            let asset = resolve_asset(&cfg, platform, arch).expect("url-template always resolves");
            assert_eq!(
                asset.url,
                format!("https://example.com/tool/9.9.9/tool_{tag}.zip")
            );
            assert_eq!(asset.version, "9.9.9");
            assert_eq!(asset.sha256, None);
        }
    }

    #[test]
    fn url_template_requires_a_pinned_version() {
        let cfg = AcquireConfig {
            kind: AcquireKind::UrlTemplate,
            url: "https://example.com/{version}/tool.zip".to_string(),
            ..Default::default()
        };
        let err = resolve_asset(&cfg, Platform::Linux, Arch::Amd64).unwrap_err();
        assert!(matches!(err, AcquireError::BadConfig(_)));
    }

    // ── checksum verification ───────────────────────────────────────────

    #[test]
    fn verify_sha256_accepts_matching_digest() {
        let dir = std::env::temp_dir().join(format!("vimcode_test_sha_ok_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload.bin");
        std::fs::write(&path, b"hello world").unwrap();
        let real_digest = to_hex(&Sha256::digest(b"hello world"));
        assert_eq!(real_digest.len(), 64, "sha256 hex digest is 64 chars");
        assert!(verify_sha256(&path, &real_digest).is_ok());
        // Case-insensitive too.
        assert!(verify_sha256(&path, &real_digest.to_uppercase()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_sha256_rejects_mismatched_digest() {
        let dir = std::env::temp_dir().join(format!("vimcode_test_sha_bad_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload.bin");
        std::fs::write(&path, b"hello world").unwrap();
        let bogus = "0".repeat(64);
        let err = verify_sha256(&path, &bogus).unwrap_err();
        assert!(matches!(err, AcquireError::ChecksumMismatch { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_sha256_for_filename_matches_sums_file_line() {
        let sums = "\
deadbeef00000000000000000000000000000000000000000000000000000000  terraform-ls_0.32.0_linux_amd64.zip
cafebabe00000000000000000000000000000000000000000000000000000000  terraform-ls_0.32.0_darwin_arm64.zip
";
        assert_eq!(
            find_sha256_for_filename(sums, "terraform-ls_0.32.0_linux_amd64.zip"),
            Some("deadbeef00000000000000000000000000000000000000000000000000000000".to_string())
        );
        assert_eq!(find_sha256_for_filename(sums, "nonexistent.zip"), None);
    }

    // ── archive extraction ──────────────────────────────────────────────

    fn make_zip_with_entry(dir: &Path, entry_name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join("archive.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        writer.start_file(entry_name, options).unwrap();
        std::io::Write::write_all(&mut writer, contents).unwrap();
        writer.finish().unwrap();
        path
    }

    fn make_tar_gz_with_entry(dir: &Path, entry_name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join("archive.tar.gz");
        let file = std::fs::File::create(&path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, entry_name, contents)
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        path
    }

    #[test]
    fn extracts_binary_path_from_zip() {
        let dir = std::env::temp_dir().join(format!("vimcode_test_zip_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = make_zip_with_entry(&dir, "terraform-ls", b"#!fake-binary");
        let dest = dir.join("out").join("terraform-ls");
        extract_binary_from_archive(&archive, "terraform-ls", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"#!fake-binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "extracted binary should be executable");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_binary_path_from_tar_gz() {
        let dir = std::env::temp_dir().join(format!("vimcode_test_targz_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = make_tar_gz_with_entry(&dir, "terraform-ls", b"#!fake-binary");
        let dest = dir.join("out").join("terraform-ls");
        extract_binary_from_archive(&archive, "terraform-ls", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"#!fake-binary");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extraction_matches_basename_under_a_wrapping_directory() {
        // Many archives wrap the binary in a version-named top-level dir.
        let dir = std::env::temp_dir().join(format!("vimcode_test_wrap_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = make_zip_with_entry(&dir, "terraform-ls_0.32.0/terraform-ls", b"payload");
        let dest = dir.join("out").join("terraform-ls");
        extract_binary_from_archive(&archive, "terraform-ls", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"payload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extraction_rejects_path_traversal_in_binary_path() {
        let dir = std::env::temp_dir().join(format!("vimcode_test_trav_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = make_zip_with_entry(&dir, "../x", b"payload");
        let dest = dir.join("out").join("x");
        let err = extract_binary_from_archive(&archive, "../x", &dest).unwrap_err();
        assert!(matches!(err, AcquireError::PathTraversal(_)));
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extraction_skips_traversal_entries_even_when_binary_path_is_safe() {
        // Defense in depth: even if `binary_path` itself is safe, a
        // malicious archive entry with a `..` name must never be extracted
        // as a side effect of iterating the archive.
        let dir = std::env::temp_dir().join(format!("vimcode_test_trav2_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("archive.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        writer.start_file("../evil", options.clone()).unwrap();
        std::io::Write::write_all(&mut writer, b"evil").unwrap();
        writer.start_file("terraform-ls", options).unwrap();
        std::io::Write::write_all(&mut writer, b"good").unwrap();
        writer.finish().unwrap();

        let dest = dir.join("out").join("terraform-ls");
        extract_binary_from_archive(&path, "terraform-ls", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"good");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extraction_errors_when_binary_path_not_present() {
        let dir = std::env::temp_dir().join(format!("vimcode_test_missing_{}", unique_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = make_zip_with_entry(&dir, "something-else", b"payload");
        let dest = dir.join("out").join("terraform-ls");
        let err = extract_binary_from_archive(&archive, "terraform-ls", &dest).unwrap_err();
        assert!(matches!(err, AcquireError::Archive(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── end-to-end: acquire_and_install_for against a local `file://` URL ──
    //
    // `curl` (the same binary `registry.rs` already shells out to) happily
    // fetches `file://` URLs, so the whole download → verify → unpack →
    // atomic-install pipeline can be exercised deterministically with no
    // real network access — only `resolve_asset`'s hashicorp-release/
    // github-release branches actually need the network, and those are
    // covered instead by the `#[ignore]`d live smoke test in
    // `tests/extensions.rs`.
    //
    // Locking: uses `paths::VIMCODE_TEST_DATA_HOME_LOCK` rather than a
    // module-local mutex — see that lock's doc comment for why every test
    // anywhere in the crate that mutates `VIMCODE_TEST_DATA_HOME` must
    // share one lock, not one per module.
    use super::super::paths::VIMCODE_TEST_DATA_HOME_LOCK as DATA_HOME_LOCK;

    struct DataHomeGuard {
        old: Option<std::ffi::OsString>,
        dir: PathBuf,
    }

    impl DataHomeGuard {
        fn new(tag: &str) -> Self {
            let old = std::env::var_os("VIMCODE_TEST_DATA_HOME");
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_acquire_e2e_{tag}_{}",
                unique_suffix()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("VIMCODE_TEST_DATA_HOME", &dir);
            Self { old, dir }
        }
    }

    impl Drop for DataHomeGuard {
        fn drop(&mut self) {
            match self.old.take() {
                Some(v) => std::env::set_var("VIMCODE_TEST_DATA_HOME", v),
                None => std::env::remove_var("VIMCODE_TEST_DATA_HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn acquire_and_install_end_to_end_via_file_url() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let guard = DataHomeGuard::new("ok");

        // Build a fixture zip somewhere *outside* the data-home dir the
        // guard just created, so the fixture and the install target can't
        // collide.
        let fixture_dir =
            std::env::temp_dir().join(format!("vimcode_test_acquire_fixture_{}", unique_suffix()));
        std::fs::create_dir_all(&fixture_dir).unwrap();
        let archive = make_zip_with_entry(&fixture_dir, "terraform-ls", b"#!fake-terraform-ls");

        let cfg = AcquireConfig {
            kind: AcquireKind::UrlTemplate,
            url: format!("file://{}", archive.display()),
            version: "1.2.3".to_string(),
            binary_path: "terraform-ls".to_string(),
            ..Default::default()
        };

        let installed = acquire_and_install_for("terraform-ls", &cfg, Platform::Linux, Arch::Amd64)
            .expect("end-to-end acquisition should succeed");
        assert!(installed.is_file());
        assert_eq!(std::fs::read(&installed).unwrap(), b"#!fake-terraform-ls");
        assert!(
            installed.starts_with(&guard.dir),
            "installed binary should live under the managed data dir, not {}",
            installed.display()
        );

        // `resolve_command`-equivalent: the `current` pointer + version dir
        // this installs match what `paths::managed_tool_binary_path` reads.
        assert_eq!(
            super::super::paths::managed_tool_current_version("terraform-ls"),
            Some("1.2.3".to_string())
        );
        assert_eq!(
            super::super::paths::managed_tool_binary_path("terraform-ls"),
            Some(installed)
        );

        let _ = std::fs::remove_dir_all(&fixture_dir);
    }

    #[test]
    fn acquire_and_install_checksum_mismatch_aborts_and_leaves_no_current_pointer() {
        // Drives the real orchestration abort path end-to-end (download via
        // `curl file://`, then a real checksum check that fails) via
        // `install_resolved_asset` — the internal half of
        // `acquire_and_install_for` that runs after `resolve_asset`, which
        // `url-template` itself never attaches a checksum to. This is
        // exactly what happens when a `hashicorp-release` asset's
        // SHA256SUMS line doesn't match the download.
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("checksum_mismatch");

        let fixture_dir = std::env::temp_dir().join(format!(
            "vimcode_test_acquire_fixture_bad_sum_{}",
            unique_suffix()
        ));
        std::fs::create_dir_all(&fixture_dir).unwrap();
        let archive = make_zip_with_entry(&fixture_dir, "terraform-ls", b"#!fake-terraform-ls");

        let asset = ResolvedAsset {
            url: format!("file://{}", archive.display()),
            version: "1.2.3".to_string(),
            sha256: Some("0".repeat(64)),
        };

        let err = install_resolved_asset("terraform-ls", &asset, "terraform-ls").unwrap_err();
        assert!(matches!(err, AcquireError::ChecksumMismatch { .. }));

        // Aborted before install: no version directory, no `current` pointer.
        assert_eq!(
            super::super::paths::managed_tool_current_version("terraform-ls"),
            None
        );
        assert!(!super::super::paths::managed_tool_version_dir("terraform-ls", "1.2.3").exists());

        let _ = std::fs::remove_dir_all(&fixture_dir);
    }

    // ── https-only enforcement (review finding on #1345) ───────────────

    #[test]
    fn install_resolved_asset_rejects_plain_http_url() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("http_rejected");

        let asset = ResolvedAsset {
            url: "http://example.com/terraform-ls.zip".to_string(),
            version: "1.2.3".to_string(),
            sha256: None,
        };
        let err = install_resolved_asset("terraform-ls", &asset, "terraform-ls").unwrap_err();
        assert!(
            matches!(err, AcquireError::BadConfig(_)),
            "expected a BadConfig rejection for a non-https URL, got {err:?}"
        );
        // Never even attempted a download: no version dir, no `current` pointer.
        assert_eq!(
            super::super::paths::managed_tool_current_version("terraform-ls"),
            None
        );
        assert!(!super::super::paths::managed_tool_version_dir("terraform-ls", "1.2.3").exists());
    }

    #[test]
    fn validate_download_url_accepts_https_and_rejects_every_other_scheme() {
        // Pure, network-free check (review finding on #1345) — exercised
        // directly rather than through `install_resolved_asset`, which
        // would otherwise need a real network call for any URL that gets
        // past the scheme check.
        assert!(validate_download_url("https://example.com/tool.zip").is_ok());
        for bad in [
            "http://example.com/tool.zip",
            "ftp://example.com/tool.zip",
            "example.com/tool.zip",
            "",
        ] {
            let err = validate_download_url(bad).unwrap_err();
            assert!(
                matches!(err, AcquireError::BadConfig(_)),
                "expected {bad:?} to be rejected as BadConfig, got {err:?}"
            );
        }
    }

    #[test]
    fn release_builds_reject_file_urls_outright() {
        // Second review round on #1345: `file://` used to be accepted
        // unconditionally in production code, giving a community-submitted
        // or compromised registry manifest an arbitrary-file-read primitive
        // (`url = "file:///home/user/.ssh/id_rsa"` → `curl -o <staging>`).
        // The allowance is now a *build* property, so assert the release
        // behaviour directly by passing the production value of the flag.
        //
        // RED against the previous commit: `validate_download_url` there had
        // no flag at all and returned `Ok(())` for every one of these.
        for hostile in [
            "file:///home/user/.ssh/id_rsa",
            "file:///etc/passwd",
            "file://localhost/etc/shadow",
        ] {
            let err = validate_download_url_with(hostile, false).unwrap_err();
            assert!(
                matches!(err, AcquireError::BadConfig(_)),
                "a release build must refuse {hostile:?}, got {err:?}"
            );
        }
        // https is unaffected by the flag in either direction.
        assert!(validate_download_url_with("https://example.com/t.zip", false).is_ok());
        assert!(validate_download_url_with("https://example.com/t.zip", true).is_ok());
        // …and the test-build allowance is what lets this module's own
        // end-to-end fixtures work.
        assert!(validate_download_url_with("file:///tmp/tool.zip", true).is_ok());
        assert!(
            file_url_downloads_allowed(),
            "in-crate `cfg(test)` builds must keep the `file://` fixture allowance"
        );
    }

    // ── path-traversal guard on `tool_name`/`version` (review finding) ──

    #[test]
    fn install_resolved_asset_rejects_traversal_in_version() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("traversal_version");

        let asset = ResolvedAsset {
            url: "https://example.com/terraform-ls.zip".to_string(),
            version: "../../../etc".to_string(),
            sha256: None,
        };
        let err = install_resolved_asset("terraform-ls", &asset, "terraform-ls").unwrap_err();
        assert!(matches!(err, AcquireError::PathTraversal(_)));
    }

    #[test]
    fn install_resolved_asset_rejects_traversal_in_tool_name() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("traversal_tool_name");

        let asset = ResolvedAsset {
            url: "https://example.com/terraform-ls.zip".to_string(),
            version: "1.2.3".to_string(),
            sha256: None,
        };
        let err = install_resolved_asset("../../etc", &asset, "terraform-ls").unwrap_err();
        assert!(matches!(err, AcquireError::PathTraversal(_)));
    }

    // ── atomic-install staging (EXDEV review finding) ────────────────────

    #[test]
    fn copy_dir_recursive_copies_nested_files_and_dirs() {
        let root = std::env::temp_dir().join(format!("vimcode_test_copy_dir_{}", unique_suffix()));
        let src = root.join("src");
        let dst = root.join("dst");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("top.txt"), b"top").unwrap();
        std::fs::write(src.join("nested").join("inner.txt"), b"inner").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(std::fs::read(dst.join("top.txt")).unwrap(), b"top");
        assert_eq!(
            std::fs::read(dst.join("nested").join("inner.txt")).unwrap(),
            b"inner"
        );
        // Source is left untouched — only `rename_or_copy`'s caller decides
        // whether to remove it.
        assert!(src.join("top.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_or_copy_falls_back_to_copy_when_rename_fails() {
        // `std::fs::rename` onto a non-empty directory fails on Linux/macOS
        // (`ENOTEMPTY`) the same way it would for a genuine cross-filesystem
        // `EXDEV` — both make `rename_or_copy` fall back to
        // `copy_dir_recursive` + removing `src`. This is the fallback
        // `install_resolved_asset` leans on if staging next to the version
        // dir ever still isn't same-filesystem (review finding on #1345).
        let root =
            std::env::temp_dir().join(format!("vimcode_test_rename_fallback_{}", unique_suffix()));
        let src = root.join("src");
        let dst = root.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("payload.bin"), b"payload").unwrap();
        // Make `dst` non-empty so a plain `rename` is refused.
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("existing.txt"), b"pre-existing").unwrap();

        rename_or_copy(&src, &dst).unwrap();

        assert_eq!(std::fs::read(dst.join("payload.bin")).unwrap(), b"payload");
        assert!(
            !src.exists(),
            "src should be removed after falling back to copy"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── github asset download uses -L (review finding, doc-only note) ───
    //
    // `download_asset`'s `-sfL` flags (vs. `registry::download_script`'s
    // `-sf`) aren't independently unit-testable without a real HTTP
    // redirect server; covered by inspection — see `download_asset`'s doc
    // comment for the GitHub redirect scenario this avoids.

    // ── package-manager acquisition: argv construction (#1346) ─────────
    //
    // Pure, network-free, no process spawn — `package_manager_argv` is a
    // function of `(kind, cfg, dir, platform)` alone, so every kind ×
    // platform combination is assertable in one test run regardless of the
    // suite's host OS or what's installed on it (same reasoning
    // `full_url_resolution_across_all_platform_arch_combinations` already
    // applies to the archive kinds above).

    #[test]
    fn package_manager_argv_requires_package() {
        for kind in [
            AcquireKind::Npm,
            AcquireKind::Pip,
            AcquireKind::Go,
            AcquireKind::Cargo,
            AcquireKind::DotnetTool,
        ] {
            let cfg = AcquireConfig {
                kind,
                ..Default::default()
            };
            let err =
                package_manager_argv(kind, &cfg, Path::new("/tools/x/latest"), Platform::Linux)
                    .unwrap_err();
            assert!(
                matches!(err, AcquireError::BadConfig(_)),
                "{kind:?} with no `package` should be rejected, got {err:?}"
            );
        }
    }

    #[test]
    fn package_manager_argv_npm_install_prefix_unpinned() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Npm,
            package: "pyright".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Npm,
            &cfg,
            Path::new("/tools/pyright/latest"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].argv,
            vec![
                "npm",
                "install",
                "--prefix",
                "/tools/pyright/latest",
                "pyright"
            ]
        );
        assert!(steps[0].env.is_empty());
    }

    #[test]
    fn package_manager_argv_npm_install_prefix_pinned_version() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Npm,
            package: "pyright".to_string(),
            version: "1.2.3".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Npm,
            &cfg,
            Path::new("/tools/pyright/1.2.3"),
            Platform::MacOS,
        )
        .unwrap();
        assert_eq!(
            steps[0].argv,
            vec![
                "npm",
                "install",
                "--prefix",
                "/tools/pyright/1.2.3",
                "pyright@1.2.3"
            ]
        );
    }

    #[test]
    fn package_manager_argv_pip_venv_then_install_unix() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Pip,
            package: "python-lsp-server".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Pip,
            &cfg,
            Path::new("/tools/pylsp/latest"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(
            steps[0].argv,
            vec!["python3", "-m", "venv", "/tools/pylsp/latest"]
        );
        assert_eq!(
            steps[1].argv,
            vec![
                "/tools/pylsp/latest/bin/pip",
                "install",
                "python-lsp-server"
            ]
        );
    }

    #[test]
    fn package_manager_argv_pip_venv_then_install_windows_uses_scripts_dir() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Pip,
            package: "python-lsp-server".to_string(),
            version: "1.9.0".to_string(),
            ..Default::default()
        };
        // `Path::join` always uses `/` when this test itself runs on a
        // non-Windows host (there is no simulated-Windows `Path`), so
        // assert on the logical components rather than a literal
        // backslash-separated string.
        let dir = Path::new("/tools/pylsp/1.9.0");
        let steps = package_manager_argv(AcquireKind::Pip, &cfg, dir, Platform::Windows).unwrap();
        assert_eq!(
            steps[1].argv[0],
            dir.join("Scripts").join("pip.exe").to_string_lossy(),
            "windows venvs put pip under Scripts\\, not bin/"
        );
        assert_eq!(steps[1].argv[2], "python-lsp-server==1.9.0");
    }

    #[test]
    fn package_manager_argv_go_install_sets_gobin_env() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Go,
            package: "golang.org/x/tools/gopls".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Go,
            &cfg,
            Path::new("/tools/gopls/latest"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].argv,
            vec!["go", "install", "golang.org/x/tools/gopls@latest"]
        );
        assert_eq!(
            steps[0].env,
            vec![("GOBIN".to_string(), "/tools/gopls/latest/bin".to_string())]
        );
    }

    #[test]
    fn package_manager_argv_go_install_pins_version_in_module_spec() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Go,
            package: "golang.org/x/tools/gopls".to_string(),
            version: "v0.16.1".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Go,
            &cfg,
            Path::new("/tools/gopls/v0.16.1"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(steps[0].argv[2], "golang.org/x/tools/gopls@v0.16.1");
    }

    #[test]
    fn package_manager_argv_cargo_install_root_unpinned() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Cargo,
            package: "ripgrep".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Cargo,
            &cfg,
            Path::new("/tools/rg/latest"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(
            steps[0].argv,
            vec!["cargo", "install", "--root", "/tools/rg/latest", "ripgrep"]
        );
    }

    #[test]
    fn package_manager_argv_cargo_install_root_pinned_version() {
        let cfg = AcquireConfig {
            kind: AcquireKind::Cargo,
            package: "ripgrep".to_string(),
            version: "14.1.0".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::Cargo,
            &cfg,
            Path::new("/tools/rg/14.1.0"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(
            steps[0].argv,
            vec![
                "cargo",
                "install",
                "--root",
                "/tools/rg/14.1.0",
                "ripgrep",
                "--version",
                "14.1.0"
            ]
        );
    }

    #[test]
    fn package_manager_argv_dotnet_tool_install_tool_path() {
        let cfg = AcquireConfig {
            kind: AcquireKind::DotnetTool,
            package: "csharprepl".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        let steps = package_manager_argv(
            AcquireKind::DotnetTool,
            &cfg,
            Path::new("/tools/csharprepl/1.0.0"),
            Platform::Linux,
        )
        .unwrap();
        assert_eq!(
            steps[0].argv,
            vec![
                "dotnet",
                "tool",
                "install",
                "--tool-path",
                "/tools/csharprepl/1.0.0",
                "csharprepl",
                "--version",
                "1.0.0"
            ]
        );
    }

    #[test]
    fn package_manager_argv_rejects_archive_download_kinds() {
        for kind in [
            AcquireKind::HashicorpRelease,
            AcquireKind::GithubRelease,
            AcquireKind::UrlTemplate,
        ] {
            let cfg = AcquireConfig {
                kind,
                package: "irrelevant".to_string(),
                ..Default::default()
            };
            let err =
                package_manager_argv(kind, &cfg, Path::new("/tools/x/latest"), Platform::Linux)
                    .unwrap_err();
            assert!(matches!(err, AcquireError::BadConfig(_)));
        }
    }

    // ── package-manager acquisition: runtime dependency (#1346) ─────────

    #[test]
    fn package_manager_runtime_maps_each_kind_to_its_runtime_binary() {
        assert_eq!(package_manager_runtime(AcquireKind::Npm), Some("npm"));
        assert_eq!(package_manager_runtime(AcquireKind::Pip), Some("python3"));
        assert_eq!(package_manager_runtime(AcquireKind::Go), Some("go"));
        assert_eq!(package_manager_runtime(AcquireKind::Cargo), Some("cargo"));
        assert_eq!(
            package_manager_runtime(AcquireKind::DotnetTool),
            Some("dotnet")
        );
        assert_eq!(package_manager_runtime(AcquireKind::HashicorpRelease), None);
        assert_eq!(package_manager_runtime(AcquireKind::GithubRelease), None);
        assert_eq!(package_manager_runtime(AcquireKind::UrlTemplate), None);
    }

    #[test]
    fn is_package_manager_kind_matches_runtime_presence() {
        for kind in [
            AcquireKind::Npm,
            AcquireKind::Pip,
            AcquireKind::Go,
            AcquireKind::Cargo,
            AcquireKind::DotnetTool,
        ] {
            assert!(
                is_package_manager_kind(kind),
                "{kind:?} should be a package-manager kind"
            );
        }
        for kind in [
            AcquireKind::HashicorpRelease,
            AcquireKind::GithubRelease,
            AcquireKind::UrlTemplate,
        ] {
            assert!(
                !is_package_manager_kind(kind),
                "{kind:?} should not be a package-manager kind"
            );
        }
    }

    // ── package-manager acquisition: binary-path resolution (#1346) ─────
    //
    // Pure, no filesystem access — `package_manager_binary_path` is a
    // function of `(kind, dir, bin, platform)` alone, including the
    // Windows `Scripts\`/`.exe` variants the #1346 acceptance criteria
    // call out explicitly.

    #[test]
    fn package_manager_binary_path_npm_unix_and_windows() {
        let dir = Path::new("/tools/pyright-langserver/latest");
        assert_eq!(
            package_manager_binary_path(
                AcquireKind::Npm,
                dir,
                "pyright-langserver",
                Platform::Linux
            ),
            dir.join("node_modules")
                .join(".bin")
                .join("pyright-langserver")
        );
        assert_eq!(
            package_manager_binary_path(
                AcquireKind::Npm,
                dir,
                "pyright-langserver",
                Platform::MacOS
            ),
            dir.join("node_modules")
                .join(".bin")
                .join("pyright-langserver")
        );
        assert_eq!(
            package_manager_binary_path(
                AcquireKind::Npm,
                dir,
                "pyright-langserver",
                Platform::Windows
            ),
            dir.join("node_modules")
                .join(".bin")
                .join("pyright-langserver.cmd"),
            "npm's windows shim is a .cmd file, not a bare .exe"
        );
    }

    #[test]
    fn package_manager_binary_path_pip_unix_and_windows() {
        let dir = Path::new("/tools/pylsp/latest");
        assert_eq!(
            package_manager_binary_path(AcquireKind::Pip, dir, "pylsp", Platform::Linux),
            dir.join("bin").join("pylsp")
        );
        assert_eq!(
            package_manager_binary_path(AcquireKind::Pip, dir, "pylsp", Platform::MacOS),
            dir.join("bin").join("pylsp")
        );
        assert_eq!(
            package_manager_binary_path(AcquireKind::Pip, dir, "pylsp", Platform::Windows),
            dir.join("Scripts").join("pylsp.exe"),
            "windows venvs put console scripts under Scripts\\ with a .exe extension"
        );
    }

    #[test]
    fn package_manager_binary_path_go_and_cargo_unix_and_windows() {
        let dir = Path::new("/tools/gopls/latest");
        for kind in [AcquireKind::Go, AcquireKind::Cargo] {
            assert_eq!(
                package_manager_binary_path(kind, dir, "gopls", Platform::Linux),
                dir.join("bin").join("gopls")
            );
            assert_eq!(
                package_manager_binary_path(kind, dir, "gopls", Platform::Windows),
                dir.join("bin").join("gopls.exe")
            );
        }
    }

    #[test]
    fn package_manager_binary_path_dotnet_tool_unix_and_windows() {
        let dir = Path::new("/tools/csharprepl/latest");
        assert_eq!(
            package_manager_binary_path(
                AcquireKind::DotnetTool,
                dir,
                "csharprepl",
                Platform::Linux
            ),
            dir.join("csharprepl"),
            "dotnet tool install --tool-path puts the launcher directly in <dir>, no bin/"
        );
        assert_eq!(
            package_manager_binary_path(
                AcquireKind::DotnetTool,
                dir,
                "csharprepl",
                Platform::Windows
            ),
            dir.join("csharprepl.exe")
        );
    }

    // ── package-manager acquisition: install orchestration (#1346) ──────

    #[test]
    #[cfg(unix)]
    fn link_binary_into_prefix_creates_a_relative_symlink_that_survives_rename() {
        // The correctness property the doc comment on `link_binary_into_prefix`
        // calls out: an *absolute* symlink baked with the staging path would
        // dangle the instant `install_via_package_manager`'s `rename_or_copy`
        // moves the whole staged prefix into its final `version_dir` location.
        // Reproduce that move here and confirm the link still resolves.
        let root = std::env::temp_dir().join(format!(
            "vimcode_test_link_survives_rename_{}",
            unique_suffix()
        ));
        let stage_dir = root.join("stage");
        std::fs::create_dir_all(stage_dir.join("bin")).unwrap();
        std::fs::write(stage_dir.join("bin").join("gopls"), b"#!fake-gopls").unwrap();
        let installed_bin = stage_dir.join("bin").join("gopls");

        let link_name =
            link_binary_into_prefix(&installed_bin, &stage_dir, "gopls", Platform::Linux).unwrap();
        assert_eq!(link_name, "gopls");
        assert_eq!(
            std::fs::read(stage_dir.join("gopls")).unwrap(),
            b"#!fake-gopls"
        );

        // Rename the whole staged prefix, as `install_via_package_manager`'s
        // `rename_or_copy` does on the common (same-filesystem) path.
        let version_dir = root.join("version");
        std::fs::rename(&stage_dir, &version_dir).unwrap();
        assert_eq!(
            std::fs::read(version_dir.join("gopls")).unwrap(),
            b"#!fake-gopls",
            "a relative symlink must still resolve after its containing \
             directory is renamed"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(unix)]
    fn link_binary_into_prefix_is_a_noop_for_dotnet_tool_top_level_binary() {
        // `DotnetTool`'s `package_manager_binary_path` already returns a
        // top-level path — linking it to itself must not delete it (see the
        // doc comment on the `installed_bin == link_path` guard).
        let root =
            std::env::temp_dir().join(format!("vimcode_test_link_dotnet_noop_{}", unique_suffix()));
        std::fs::create_dir_all(&root).unwrap();
        let installed_bin = root.join("csharprepl");
        std::fs::write(&installed_bin, b"#!fake-csharprepl").unwrap();

        let link_name =
            link_binary_into_prefix(&installed_bin, &root, "csharprepl", Platform::Linux).unwrap();
        assert_eq!(link_name, "csharprepl");
        assert_eq!(
            std::fs::read(root.join("csharprepl")).unwrap(),
            b"#!fake-csharprepl",
            "the installed binary must survive a would-be self-link"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_via_package_manager_rejects_traversal_in_tool_name() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("pkgmgr_traversal_tool_name");

        let cfg = AcquireConfig {
            kind: AcquireKind::Npm,
            package: "pyright".to_string(),
            ..Default::default()
        };
        let err = install_via_package_manager("../../etc", &cfg, Platform::Linux).unwrap_err();
        assert!(matches!(err, AcquireError::PathTraversal(_)));
    }

    #[test]
    fn install_via_package_manager_rejects_traversal_in_version() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("pkgmgr_traversal_version");

        let cfg = AcquireConfig {
            kind: AcquireKind::Npm,
            package: "pyright".to_string(),
            version: "../../../etc".to_string(),
            ..Default::default()
        };
        let err =
            install_via_package_manager("pyright-langserver", &cfg, Platform::Linux).unwrap_err();
        assert!(matches!(err, AcquireError::PathTraversal(_)));
    }

    #[test]
    fn tail_of_keeps_only_the_last_n_non_empty_lines() {
        let text = "line1\n\nline2\nline3\nline4\n";
        assert_eq!(tail_of(text, 2), "line3\nline4");
        assert_eq!(tail_of(text, 10), "line1\nline2\nline3\nline4");
        assert_eq!(tail_of("", 5), "");
    }

    /// #1346 acceptance criterion: "on failure, surface the last lines of
    /// stderr in the notification". Drives a real, deterministically-failing
    /// subprocess (the actual `sh` binary, not a mocked `Command`) so the
    /// spawn → non-zero exit → stderr-capture path in
    /// `run_package_manager_steps` is exercised end-to-end, with no
    /// dependency on `npm`/`pip`/`go`/`cargo`/`dotnet` actually being
    /// installed (or reachable over the network) on the machine running the
    /// suite.
    #[test]
    #[cfg(unix)]
    fn run_package_manager_steps_surfaces_stderr_tail_on_failure() {
        let steps = vec![PackageManagerStep {
            argv: vec![
                "sh".to_string(),
                "-c".to_string(),
                "echo boom-1346 >&2; exit 7".to_string(),
            ],
            env: vec![],
        }];
        let err = run_package_manager_steps(&steps).unwrap_err();
        match err {
            AcquireError::Archive(msg) => {
                assert!(
                    msg.contains("boom-1346"),
                    "failure message should carry the failing step's stderr; got: {msg}"
                );
            }
            other => panic!("expected an Archive error carrying stderr, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn run_package_manager_steps_stops_at_the_first_failing_step() {
        // The second step must never run once the first one fails — a
        // half-applied install (e.g. a venv created but never populated)
        // should never be mistaken for a complete one.
        let sentinel = std::env::temp_dir().join(format!(
            "vimcode_test_pkgmgr_steps_sentinel_{}",
            unique_suffix()
        ));
        let _ = std::fs::remove_file(&sentinel);
        let steps = vec![
            PackageManagerStep {
                argv: vec!["sh".to_string(), "-c".to_string(), "exit 1".to_string()],
                env: vec![],
            },
            PackageManagerStep {
                argv: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("touch {}", sentinel.display()),
                ],
                env: vec![],
            },
        ];
        assert!(run_package_manager_steps(&steps).is_err());
        assert!(
            !sentinel.exists(),
            "the second step must never run once the first one failed"
        );
    }

    #[test]
    fn acquire_and_install_for_routes_package_manager_kinds_around_resolve_asset() {
        // `resolve_asset` (the archive-kind resolver) must never be reached
        // for a package-manager kind — a `BadConfig` from it would be a
        // confusing "no release asset matches" message for something that
        // was never trying to download a release archive at all.
        for kind in [
            AcquireKind::Npm,
            AcquireKind::Pip,
            AcquireKind::Go,
            AcquireKind::Cargo,
            AcquireKind::DotnetTool,
        ] {
            let cfg = AcquireConfig {
                kind,
                package: "irrelevant".to_string(),
                ..Default::default()
            };
            let err = resolve_asset(&cfg, Platform::Linux, Arch::Amd64).unwrap_err();
            assert!(
                matches!(err, AcquireError::BadConfig(_)),
                "{kind:?} routed into resolve_asset instead of being dispatched around it"
            );
        }
    }

    // ── package-manager acquisition: live smoke (operator-run only) ─────
    //
    // Real network + real `npm`/`python3` on PATH — never run in CI. Verify
    // manually with:
    //   cargo test --lib tool_acquire::tests::live_npm_acquire -- --ignored
    //   cargo test --lib tool_acquire::tests::live_pip_acquire -- --ignored

    #[test]
    #[ignore]
    fn live_npm_acquire_installs_a_real_package_with_a_bin() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("live_npm");

        // `cowsay` is a small, stable npm package that ships a `bin` entry
        // named `cowsay` — exercises the full path: `npm install --prefix`,
        // `package_manager_binary_path`'s `node_modules/.bin` lookup, the
        // relative-symlink-into-the-staging-prefix step, and the atomic
        // rename into `managed_tool_version_dir`.
        let cfg = AcquireConfig {
            kind: AcquireKind::Npm,
            package: "cowsay".to_string(),
            binary_path: "cowsay".to_string(),
            ..Default::default()
        };
        let result =
            install_via_package_manager("vimcode-live-smoke-cowsay", &cfg, Platform::host());
        let path = result.expect("npm acquire of cowsay should succeed");
        assert!(
            path.is_file(),
            "resolved path {} should be a real, executable file",
            path.display()
        );
    }

    #[test]
    #[ignore]
    fn live_pip_acquire_installs_a_real_package_with_a_console_script() {
        let _lock = DATA_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("live_pip");

        // `pycowsay` ships a `pycowsay` console script — exercises the venv
        // creation, `pip install`, `bin/`/`Scripts\` binary-path resolution,
        // and the same link/rename plumbing as the npm smoke test above.
        let cfg = AcquireConfig {
            kind: AcquireKind::Pip,
            package: "pycowsay".to_string(),
            binary_path: "pycowsay".to_string(),
            ..Default::default()
        };
        let result =
            install_via_package_manager("vimcode-live-smoke-pycowsay", &cfg, Platform::host());
        let path = result.expect("pip acquire of pycowsay should succeed");
        assert!(
            path.is_file(),
            "resolved path {} should be a real, executable file",
            path.display()
        );
    }
}
