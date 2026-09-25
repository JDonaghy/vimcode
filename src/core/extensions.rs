//! Extension manifest data model.
//!
//! Extensions bundle an LSP server, optional DAP adapter, and optional Lua
//! scripts into a single named package. Users install them with `:ExtInstall`.
//!
//! Manifests are fetched from a remote registry (GitHub) and cached locally.
//! There are no compiled-in extensions — the registry is the single source of
//! truth.

use serde::{Deserialize, Serialize};

// ─── Manifest deserialization ─────────────────────────────────────────────────

/// A user-configurable setting declared by an extension.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ExtSettingDef {
    /// Setting key (used in `vimcode.opt.get("extname.key")`).
    pub key: String,
    /// Human-readable label shown in the Settings UI.
    #[serde(default)]
    pub label: String,
    /// Short description shown as a tooltip or subtitle.
    #[serde(default)]
    pub description: String,
    /// Value type: `"bool"`, `"string"`, `"integer"`, or `"enum"`.
    #[serde(default = "default_setting_type")]
    pub r#type: String,
    /// Default value (as a string).
    #[serde(default)]
    pub default: String,
    /// For `"enum"` type: the list of allowed values.
    #[serde(default)]
    pub options: Vec<String>,
}

fn default_setting_type() -> String {
    "string".to_string()
}

/// Parsed contents of a `manifest.toml` (or registry JSON entry).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ExtensionManifest {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    /// File extensions this extension activates for (e.g. `[".cs"]`).
    #[serde(default)]
    pub file_extensions: Vec<String>,
    /// LSP language IDs (e.g. `["csharp"]`).
    #[serde(default)]
    pub language_ids: Vec<String>,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub dap: DapConfig,
    /// Lua script filenames bundled with this extension.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Files/directories whose presence indicates this language's project root.
    /// E.g. `["Cargo.toml"]` for Rust, `["go.mod"]` for Go.
    #[serde(default)]
    pub workspace_markers: Vec<String>,
    /// Optional comment style override for languages handled by this extension.
    #[serde(default)]
    pub comment: Option<CommentConfig>,
    /// Tree-sitter highlight query (S-expression) for this language.
    /// Overrides the built-in query in `syntax.rs` when present.
    #[serde(default)]
    pub highlights: Option<String>,
    /// User-configurable settings declared by this extension.
    #[serde(default)]
    pub settings: Vec<ExtSettingDef>,
    /// Base URL of the registry this manifest was fetched from.
    /// Derived at fetch time; not serialized to JSON/TOML.
    #[serde(skip)]
    pub registry_base_url: String,
    /// Declares this extension as a Board-panel data provider (#522).
    /// `None` means this extension doesn't provide a board.
    #[serde(default)]
    pub board: Option<BoardProviderConfig>,
    /// Declares this extension as a document provider for buffer-based
    /// authoring (#524). `None` means this extension can't open its
    /// documents as editable buffers.
    #[serde(default)]
    pub document: Option<DocumentProviderConfig>,
}

/// Comment style override specified in an extension manifest `[comment]` section.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CommentConfig {
    #[serde(default)]
    pub line: String,
    #[serde(default)]
    pub block_open: String,
    #[serde(default)]
    pub block_close: String,
}

/// Declares an extension as a Board-panel data provider (#522).
///
/// Generic on purpose — this struct names no particular provider. Any
/// extension can point `refresh_command` at an external tool that emits
/// vimcode's board JSON contract (`quadraui::BoardModel`, see
/// `crate::core::tool_client::fetch_board_model`) on stdout and get a
/// working Board panel. A pipeline-management bundle is one such
/// provider, not the only one — nothing here names any particular
/// external tool or its subcommands.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BoardProviderConfig {
    /// Argv to run for a board refresh. `refresh_command[0]` is the
    /// binary, the rest are arguments. Must emit a `quadraui::BoardModel`
    /// JSON document on stdout and exit zero.
    #[serde(default)]
    pub refresh_command: Vec<String>,
    /// Seconds between automatic background refreshes.
    #[serde(default = "default_board_poll_interval_secs")]
    pub poll_interval_secs: u64,
    /// Named, provider-declared actions a card can be dispatched through
    /// (#523) — the context menu's contents, a stage keybinding table, and
    /// (via [`Self::action_by_name`]) the target of `quadraui::BoardAction`
    /// variants like `OpenIssue`/`OpenReview` that need somewhere to
    /// dispatch to. Generic on purpose, same spirit as the rest of this
    /// struct: a name like `"assign"` or `"test-pass"` is whatever the
    /// provider calls it, never a fixed enum — vimcode's `src/core/` must
    /// never contain a specific provider's action vocabulary, see
    /// `crate::core::tool_client`'s module doc.
    #[serde(default)]
    pub actions: Vec<BoardActionDef>,
    /// Argv to run periodically as an opt-in freshness nudge (#523) — for a
    /// daemon-less provider whose pipeline only advances when some external
    /// "notify" command runs, so it doesn't stall just because vimcode is
    /// the only client with the board open. Empty (the default) means the
    /// provider doesn't need/support this; fire-and-forget either way
    /// (stdout discarded, only "did it run" matters) and gated on
    /// `Settings::board_tick_enabled`, which defaults **off** — a passive
    /// viewer must not silently dispatch metered work.
    #[serde(default)]
    pub tick_command: Vec<String>,
    /// Seconds between automatic `tick_command` runs, when enabled.
    /// Independent of `poll_interval_secs` (which governs board *reads*,
    /// always on when a provider is configured) — this is typically much
    /// longer, since it's a background nudge to an external pipeline, not
    /// a refresh.
    #[serde(default = "default_board_tick_interval_secs")]
    pub tick_interval_secs: u64,
}

fn default_board_poll_interval_secs() -> u64 {
    30
}

fn default_board_tick_interval_secs() -> u64 {
    300
}

/// One provider-declared, named board action (#523) — e.g. "dispatch this
/// card's work", "record a Test verdict", "merge". The provider names it,
/// declares which stages (columns) it's valid in, an optional single-key
/// binding, whether firing it needs confirmation, and the argv to run.
///
/// Generic on purpose, same spirit as [`BoardProviderConfig`] itself — a
/// pipeline-management bundle might declare one named `"assign"` running
/// its own dispatch command, but nothing here knows that; any provider's
/// manifest can declare any action name.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct BoardActionDef {
    /// Unique (within this provider) action name. Used as the context-menu
    /// item's identity and the dialog/status-line label fallback.
    pub name: String,
    /// Human-readable label shown in the context menu / confirmation
    /// dialog / result banner. Falls back to `name` when empty — see
    /// [`Self::display_label`].
    #[serde(default)]
    pub label: String,
    /// Argv template to run. The literal token `{id}` in any argument is
    /// replaced with the acted-on card id at dispatch time (see
    /// [`Self::resolve_argv`]). An action with an empty `command` is
    /// declared-but-not-runnable — callers no-op rather than error, same
    /// convention `DocumentProviderConfig`'s empty-template case uses.
    #[serde(default)]
    pub command: Vec<String>,
    /// Column (stage) ids this action is valid for, e.g. `["col:test"]`.
    /// Empty means "valid in every stage" — most actions (assign, drop to
    /// backlog) aren't stage-specific; a Test-only verdict action would set
    /// this.
    #[serde(default)]
    pub stages: Vec<String>,
    /// Optional single-key binding while the Board panel has focus and a
    /// card matching one of `stages` is selected — single-letter
    /// Test-verdict keys (e.g. `P`/`S`/`F`) are a common example.
    /// `None` means menu-only (no keyboard shortcut).
    #[serde(default)]
    pub key: Option<String>,
    /// Whether firing this action must be confirmed (a Yes/No dialog)
    /// before it runs — irreversible or metered actions (dispatch work,
    /// merge) should set this; read-only or idempotent ones don't need to.
    #[serde(default)]
    pub confirm: bool,
}

impl BoardActionDef {
    /// `label` if set, otherwise `name` — every action has *something*
    /// presentable without every provider having to repeat `name` as
    /// `label` verbatim.
    pub fn display_label(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }

    /// Resolve the argv to run against `card_id`, substituting `{id}` in
    /// every argument. Empty when `command` is empty — see the field's own
    /// doc for why that's "not runnable", not an error.
    pub fn resolve_argv(&self, card_id: &str) -> Vec<String> {
        self.command
            .iter()
            .map(|arg| arg.replace("{id}", card_id))
            .collect()
    }

    /// Whether this action is declared valid for `stage_id` — an empty
    /// `stages` list means "every stage".
    fn valid_for_stage(&self, stage_id: &str) -> bool {
        self.stages.is_empty() || self.stages.iter().any(|s| s == stage_id)
    }
}

/// Declares an extension as a document provider (#524, Phase 1): its
/// documents (e.g. issues) can be opened as real markdown scratch buffers
/// for editing, with edits pushed back through a write command on `:w`.
///
/// Generic on purpose, same spirit as [`BoardProviderConfig`] — names no
/// particular provider or lifecycle vocabulary. Any extension can point
/// `read_command`/`write_command` at external tools that speak vimcode's
/// document contract (`crate::core::tool_client::ToolDocument`) and get
/// working buffer-based authoring.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DocumentProviderConfig {
    /// Argv to run to fetch a document's body for editing. The literal
    /// token `{id}` in any argument is replaced with the document id at
    /// dispatch time. Must emit a `ToolDocument` JSON document on stdout
    /// and exit zero.
    #[serde(default)]
    pub read_command: Vec<String>,
    /// Argv to run to push an edited document's title/body back. `{id}` is
    /// replaced with the document id, or the empty string for a
    /// not-yet-created document (the new-document flow: this same command
    /// doubles as "create", the provider script tells the two apart by
    /// whether `{id}` came through empty). The edited `{"title", "body"}`
    /// payload is delivered on stdin as JSON, never substituted into argv
    /// (titles/bodies are free text — unsafe to splice into a command
    /// line).
    #[serde(default)]
    pub write_command: Vec<String>,
    /// Argv to run after a successful write on an *already-existing*
    /// document (never on the create path — see `write_command`'s doc).
    /// `{id}` is replaced with the document id. This is where a bundle's
    /// own lifecycle transition lives (e.g. a pipeline-management bundle
    /// flipping a "refining" document to "ready") — this struct only
    /// knows "run this argv after that one succeeds," no lifecycle
    /// vocabulary itself.
    #[serde(default)]
    pub write_follow_up: Vec<String>,
}

impl DocumentProviderConfig {
    /// Resolve the argv to run to fetch `id`'s body, substituting `{id}`
    /// in every argument. `None` if this provider declared no read
    /// command.
    pub fn read_argv(&self, id: &str) -> Option<Vec<String>> {
        Self::substitute(&self.read_command, id)
    }

    /// Resolve the argv to run to push an edit back. `id` is `None` for
    /// the new-document flow (substitutes the empty string). `None` if
    /// this provider declared no write command.
    pub fn write_argv(&self, id: Option<&str>) -> Option<Vec<String>> {
        Self::substitute(&self.write_command, id.unwrap_or(""))
    }

    /// Resolve the argv to run after a successful write on an existing
    /// document. `None` if this provider declared no follow-up command
    /// (a provider is not required to have one).
    pub fn write_follow_up_argv(&self, id: &str) -> Option<Vec<String>> {
        Self::substitute(&self.write_follow_up, id)
    }

    fn substitute(template: &[String], id: &str) -> Option<Vec<String>> {
        if template.is_empty() {
            return None;
        }
        Some(template.iter().map(|arg| arg.replace("{id}", id)).collect())
    }
}

impl BoardProviderConfig {
    /// Find a declared action by name — `None` if this provider declared
    /// none by that name (or no `[board]` actions at all).
    pub fn action_by_name(&self, name: &str) -> Option<&BoardActionDef> {
        self.actions.iter().find(|a| a.name == name)
    }

    /// Every declared action valid for `stage_id` (a column id) — the
    /// context menu's contents (#523: "listing the actions the provider
    /// declares as valid for that card's stage").
    pub fn actions_for_stage(&self, stage_id: &str) -> Vec<&BoardActionDef> {
        self.actions
            .iter()
            .filter(|a| a.valid_for_stage(stage_id))
            .collect()
    }

    /// The declared action bound to `key`, if any, and valid for
    /// `stage_id` — a stage keybinding lookup (#523's single-key verdict
    /// bindings, e.g. `P`/`S`/`F`).
    pub fn action_for_key(&self, key: &str, stage_id: &str) -> Option<&BoardActionDef> {
        self.actions
            .iter()
            .find(|a| a.key.as_deref() == Some(key) && a.valid_for_stage(stage_id))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LspConfig {
    /// Binary name that must be on PATH (e.g. `"csharp-ls"`).
    #[serde(default)]
    pub binary: String,
    /// Shell command to install the LSP server (generic / Linux fallback).
    #[serde(default)]
    pub install: String,
    /// Platform-specific install commands (override `install` when non-empty).
    #[serde(default)]
    pub install_linux: String,
    #[serde(default)]
    pub install_macos: String,
    #[serde(default)]
    pub install_windows: String,
    /// Fallback binaries tried in order when `binary` is not found on PATH.
    /// E.g. `["basedpyright-langserver", "pylsp", "jedi-language-server"]` for Python.
    #[serde(default)]
    pub fallback_binaries: Vec<String>,
    /// Command-line arguments passed to the LSP binary (default: `["--stdio"]` if needed).
    #[serde(default)]
    pub args: Vec<String>,
    /// System binaries that must be on PATH for the LSP server to work.
    /// E.g. `["dotnet"]` for .NET-based servers, `["node"]` for Node-based ones.
    /// Checked before starting the server; a helpful message is shown if missing.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Diagnostic sources whose errors should be excluded from editor display
    /// and explorer counts.  E.g. `["rust-analyzer"]` — its internal analysis
    /// produces false-positive errors; real errors come from `"rustc"` (cargo
    /// check).  Warnings from these sources are still shown.
    #[serde(default)]
    pub ignore_error_sources: Vec<String>,
    /// JSON object merged into the LSP `initialize` request's
    /// `initializationOptions`.  Allows per-server configuration (e.g.
    /// `{"diagnostics": {"enable": false}}` for rust-analyzer).
    #[serde(default)]
    pub initialization_options: Option<serde_json::Value>,
    /// Native tool acquisition (#1345): download, verify and unpack the LSP
    /// binary directly, with no shell command, `sudo`, `unzip`, or PATH
    /// edits. When present, `ext_install_from_registry` prefers this over
    /// `install`/`install_linux`/`install_macos`/`install_windows` — see
    /// `crate::core::tool_acquire`.
    #[serde(default)]
    pub acquire: Option<crate::core::tool_acquire::AcquireConfig>,
}

// ─── Target platform (testable seam, #919) ────────────────────────────────────

/// A target platform for install-command resolution. Install commands are
/// naturally platform-specific (`apt`/`brew`/`winget`, `sh -c` vs `cmd /C`);
/// this type makes "which platform" an explicit parameter passed to
/// `install_cmd_for` instead of a `cfg!` baked into the compiled binary, so a
/// single test run can assert every manifest resolves an install command on
/// *all three* platforms regardless of which one the test binary happens to
/// be compiled for. See the registry conformance gate in `tests/extensions.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Linux,
    MacOS,
    Windows,
}

impl Platform {
    /// All platforms vimcode ships a backend for, in a stable order.
    pub const ALL: [Platform; 3] = [Platform::Linux, Platform::MacOS, Platform::Windows];

    /// The platform this binary was actually compiled for — what
    /// `install_cmd_for_platform()` resolves against by default.
    pub fn host() -> Platform {
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOS
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Platform::Linux
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Platform::Linux => "linux",
            Platform::MacOS => "macos",
            Platform::Windows => "windows",
        })
    }
}

impl LspConfig {
    /// Return the install command for the current (host) platform.
    /// Prefers the platform-specific field; falls back to the generic `install` field.
    /// Applies known fixups (e.g. rust-analyzer falls back to `rustup component add`
    /// when rustup is available, since `cargo install rust-analyzer` compiles
    /// from source and is slow).
    pub fn install_cmd_for_platform(&self) -> &str {
        self.install_cmd_for(Platform::host())
    }

    /// Same as `install_cmd_for_platform`, but for an explicitly-named
    /// platform rather than the one this binary happens to be compiled for.
    /// This is the seam the #919 registry conformance test uses to check a
    /// manifest resolves an install command on all three platforms from a
    /// single (any-OS) test binary.
    pub fn install_cmd_for(&self, platform: Platform) -> &str {
        self.install_cmd_with(platform, rustup_on_path())
    }

    /// Inner helper for `install_cmd_for`, parameterised over rustup
    /// availability so the fixup can be tested without depending on the host's
    /// PATH.
    fn install_cmd_with(&self, platform: Platform, rustup_available: bool) -> &str {
        let raw = self.platform_install_cmd_raw(platform);
        // rust-analyzer: prefer `rustup component add` over `cargo install`.
        // Anyone with a Rust toolchain has rustup; the component is a
        // ~30-second binary download vs the ~10-minute source build.
        if rustup_available && self.binary == "rust-analyzer" && raw.starts_with("cargo install") {
            return "rustup component add rust-analyzer";
        }
        raw
    }

    fn platform_install_cmd_raw(&self, platform: Platform) -> &str {
        let platform_field = platform_install_field(
            platform,
            &self.install_linux,
            &self.install_macos,
            &self.install_windows,
        );
        if platform_field.is_empty() {
            &self.install
        } else {
            platform_field
        }
    }
}

/// Returns true if `rustup` is on PATH (or `rustup.exe` on Windows).
fn rustup_on_path() -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        if dir.join("rustup").exists() {
            return true;
        }
        #[cfg(target_os = "windows")]
        if dir.join("rustup.exe").exists() {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DapConfig {
    /// Adapter name matching `dap_manager`'s registry (e.g. `"netcoredbg"`).
    #[serde(default)]
    pub adapter: String,
    /// Executable to launch for the DAP adapter (e.g. `"codelldb"`, `"python"`).
    #[serde(default)]
    pub binary: String,
    /// Shell command to install the DAP adapter (generic / Linux fallback).
    #[serde(default)]
    pub install: String,
    /// Platform-specific install commands (override `install` when non-empty).
    #[serde(default)]
    pub install_linux: String,
    #[serde(default)]
    pub install_macos: String,
    #[serde(default)]
    pub install_windows: String,
    /// Transport protocol: `"stdio"` (default) or `"tcp"`.
    #[serde(default)]
    pub transport: String,
    /// Arguments passed to the DAP binary.
    #[serde(default)]
    pub args: Vec<String>,
    /// Native tool acquisition (#1345) — see `LspConfig::acquire`'s doc.
    #[serde(default)]
    pub acquire: Option<crate::core::tool_acquire::AcquireConfig>,
}

impl DapConfig {
    /// Return the install command for the current (host) platform.
    /// Prefers the platform-specific field; falls back to the generic `install` field.
    pub fn install_cmd_for_platform(&self) -> &str {
        self.install_cmd_for(Platform::host())
    }

    /// Same as `install_cmd_for_platform`, but for an explicitly-named
    /// platform. See `LspConfig::install_cmd_for` / `Platform` for why this
    /// seam exists (#919).
    pub fn install_cmd_for(&self, platform: Platform) -> &str {
        let platform_field = platform_install_field(
            platform,
            &self.install_linux,
            &self.install_macos,
            &self.install_windows,
        );
        if platform_field.is_empty() {
            &self.install
        } else {
            platform_field
        }
    }
}

/// Pick the install command string for the given platform.
fn platform_install_field<'a>(
    platform: Platform,
    linux: &'a str,
    macos: &'a str,
    windows: &'a str,
) -> &'a str {
    match platform {
        Platform::Linux => linux,
        Platform::MacOS => macos,
        Platform::Windows => windows,
    }
}

// ─── Prerequisite install guidance (#918) ─────────────────────────────────────

/// Per-platform install guidance for the handful of runtime prerequisites
/// shared across many extensions' `lsp.dependencies` (e.g. `["npm"]`).
///
/// `dependencies` is intentionally a bare `Vec<String>` of binary names —
/// extending the manifest schema so each entry could carry its own install
/// metadata would break the `dependencies = ["npm"]` shorthand already used
/// by 10+ registry manifests (it would need to accept both a bare string and
/// a `{name, install_linux, ...}` object, and every extension author would
/// need to fill it in). A small built-in table for the six prerequisites
/// actually shared across the registry today — `npm`, `dotnet`, `go`, `gem`,
/// `cargo`, `rustup` — is simpler, keeps the manifest format backward
/// compatible, and covers every affected extension without a schema change.
struct PrereqInstall {
    linux: &'static str,
    macos: &'static str,
    windows: &'static str,
}

const PREREQ_INSTALLS: &[(&str, PrereqInstall)] = &[
    (
        "npm",
        PrereqInstall {
            linux: "sudo apt install nodejs npm",
            macos: "brew install node",
            windows: "winget install OpenJS.NodeJS",
        },
    ),
    (
        "dotnet",
        PrereqInstall {
            linux: "sudo apt install dotnet-sdk-8.0",
            macos: "brew install dotnet-sdk",
            windows: "winget install Microsoft.DotNet.SDK.8",
        },
    ),
    (
        "go",
        PrereqInstall {
            linux: "sudo apt install golang-go",
            macos: "brew install go",
            windows: "winget install GoLang.Go",
        },
    ),
    (
        "gem",
        PrereqInstall {
            linux: "sudo apt install ruby",
            macos: "brew install ruby",
            windows: "winget install RubyInstallerTeam.Ruby",
        },
    ),
    (
        "cargo",
        PrereqInstall {
            linux: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
            macos: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
            windows: "winget install Rustlang.Rustup",
        },
    ),
    (
        "rustup",
        PrereqInstall {
            linux: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
            macos: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
            windows: "winget install Rustlang.Rustup",
        },
    ),
];

/// Return a runnable install command for a known prerequisite binary (e.g.
/// `"npm"`), chosen for the current platform. Returns `None` for names
/// outside the built-in table above — callers should fall back to a
/// generic "install X and try again" message in that case.
pub fn prereq_install_cmd(dep: &str) -> Option<&'static str> {
    PREREQ_INSTALLS
        .iter()
        .find(|(name, _)| *name == dep)
        .map(|(_, t)| platform_install_field(Platform::host(), t.linux, t.macos, t.windows))
}

impl ExtensionManifest {
    /// Parse a manifest from a TOML string. Returns `None` on parse failure.
    #[allow(dead_code)]
    pub fn parse(toml: &str) -> Option<Self> {
        toml::from_str(toml).ok()
    }

    /// Returns true if this extension is relevant for the given file extension
    /// (e.g. ".cs") or language ID.
    #[allow(dead_code)]
    pub fn matches_file_ext(&self, ext: &str) -> bool {
        self.file_extensions.iter().any(|e| e == ext)
    }

    pub fn matches_language_id(&self, lang: &str) -> bool {
        self.language_ids.iter().any(|l| l == lang)
    }

    /// The name to show the user: `display_name` when set, falling back to
    /// the internal `name` otherwise. Several install-failure messages
    /// (`lsp_manager.rs`'s `missing_dependency_message` and the two
    /// `ensure_server_for_language` error branches, #918) all needed this
    /// exact fallback independently — pulled out once here so it can't drift
    /// between call sites.
    pub fn display_or_name(&self) -> &str {
        if self.display_name.is_empty() {
            &self.name
        } else {
            &self.display_name
        }
    }
}

// ─── Lookup helpers (operate on a slice of manifests) ─────────────────────────

/// Find a manifest by name (case-insensitive) in a slice.
#[allow(dead_code)]
pub fn find_manifest_by_name<'a>(
    manifests: &'a [ExtensionManifest],
    name: &str,
) -> Option<&'a ExtensionManifest> {
    manifests.iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Find the first manifest whose `file_extensions` list contains `ext`.
#[allow(dead_code)]
pub fn find_manifest_for_file_ext<'a>(
    manifests: &'a [ExtensionManifest],
    ext: &str,
) -> Option<&'a ExtensionManifest> {
    manifests.iter().find(|m| m.matches_file_ext(ext))
}

/// Find the first manifest whose `language_ids` list contains `lang`.
pub fn find_manifest_for_language_id<'a>(
    manifests: &'a [ExtensionManifest],
    lang: &str,
) -> Option<&'a ExtensionManifest> {
    manifests.iter().find(|m| m.matches_language_id(lang))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifests() -> Vec<ExtensionManifest> {
        vec![
            ExtensionManifest {
                name: "rust".to_string(),
                display_name: "Rust Language Support".to_string(),
                file_extensions: vec![".rs".to_string()],
                language_ids: vec!["rust".to_string()],
                lsp: LspConfig {
                    binary: "rust-analyzer".to_string(),
                    ..Default::default()
                },
                dap: DapConfig {
                    adapter: "codelldb".to_string(),
                    binary: "codelldb".to_string(),
                    transport: "tcp".to_string(),
                    args: vec!["--port".to_string(), "0".to_string()],
                    ..Default::default()
                },
                workspace_markers: vec!["Cargo.toml".to_string()],
                ..Default::default()
            },
            ExtensionManifest {
                name: "python".to_string(),
                display_name: "Python Language Support".to_string(),
                file_extensions: vec![".py".to_string(), ".pyi".to_string()],
                language_ids: vec!["python".to_string()],
                lsp: LspConfig {
                    binary: "pyright-langserver".to_string(),
                    fallback_binaries: vec!["pylsp".to_string()],
                    args: vec!["--stdio".to_string()],
                    ..Default::default()
                },
                workspace_markers: vec!["pyproject.toml".to_string()],
                ..Default::default()
            },
            ExtensionManifest {
                name: "git-insights".to_string(),
                display_name: "Git Insights".to_string(),
                scripts: vec!["blame.lua".to_string()],
                ..Default::default()
            },
        ]
    }

    #[test]
    fn find_by_name_case_insensitive() {
        let ms = sample_manifests();
        assert!(find_manifest_by_name(&ms, "rust").is_some());
        assert!(find_manifest_by_name(&ms, "Rust").is_some());
        assert!(find_manifest_by_name(&ms, "RUST").is_some());
        assert!(find_manifest_by_name(&ms, "nonexistent").is_none());
    }

    #[test]
    fn find_by_file_ext() {
        let ms = sample_manifests();
        let m = find_manifest_for_file_ext(&ms, ".rs").expect(".rs should match rust");
        assert_eq!(m.name, "rust");
        let m = find_manifest_for_file_ext(&ms, ".py").expect(".py should match python");
        assert_eq!(m.name, "python");
        assert!(find_manifest_for_file_ext(&ms, ".xyz").is_none());
    }

    #[test]
    fn find_by_language_id() {
        let ms = sample_manifests();
        let m = find_manifest_for_language_id(&ms, "rust").expect("rust lang id");
        assert_eq!(m.name, "rust");
        let m = find_manifest_for_language_id(&ms, "python").expect("python lang id");
        assert_eq!(m.name, "python");
        assert!(find_manifest_for_language_id(&ms, "cobol").is_none());
    }

    #[test]
    fn dap_install_routes_by_language_not_shared_adapter() {
        // #212 follow-up. Both cpp and rust bundle codelldb. Previously
        // `:DapInstall rust` matched on either language OR shared adapter
        // and `.find()` returned whichever was iterated first (cpp wins
        // alphabetically), telling the user to install the wrong extension.
        // Lookup must be by language_ids exclusively.
        let ms = vec![
            ExtensionManifest {
                name: "cpp".to_string(),
                display_name: "C/C++ Language Support".to_string(),
                file_extensions: vec![".c".to_string(), ".cpp".to_string()],
                language_ids: vec!["c".to_string(), "cpp".to_string()],
                dap: DapConfig {
                    adapter: "codelldb".to_string(),
                    binary: "codelldb".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ExtensionManifest {
                name: "rust".to_string(),
                display_name: "Rust Language Support".to_string(),
                file_extensions: vec![".rs".to_string()],
                language_ids: vec!["rust".to_string()],
                dap: DapConfig {
                    adapter: "codelldb".to_string(),
                    binary: "codelldb".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            },
        ];

        let m = find_manifest_for_language_id(&ms, "rust").expect("rust must match");
        assert_eq!(
            m.name, "rust",
            ":DapInstall rust must resolve to the rust extension, not cpp \
             (both ship codelldb)",
        );

        let m = find_manifest_for_language_id(&ms, "cpp").expect("cpp must match");
        assert_eq!(m.name, "cpp");
    }

    #[test]
    fn manifest_parse_toml() {
        let toml = r#"
name = "test"
display_name = "Test Extension"
file_extensions = [".test"]
language_ids = ["test"]
[lsp]
binary = "test-lsp"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert_eq!(m.name, "test");
        assert_eq!(m.lsp.binary, "test-lsp");
    }

    #[test]
    fn manifest_serialize_deserialize_json_roundtrip() {
        let ms = sample_manifests();
        let json = serde_json::to_string(&ms).expect("serialize");
        let back: Vec<ExtensionManifest> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.len(), ms.len());
        assert_eq!(back[0].name, "rust");
        assert_eq!(back[1].lsp.fallback_binaries, vec!["pylsp"]);
    }

    #[test]
    fn rust_analyzer_uses_rustup_when_available() {
        // #436: bundled rust manifest ships `cargo install rust-analyzer`,
        // which compiles from source (~10 min silent build).  When rustup
        // is available, swap to `rustup component add rust-analyzer`
        // (~30s binary download) on every platform.
        let cfg = LspConfig {
            binary: "rust-analyzer".to_string(),
            install: "cargo install rust-analyzer".to_string(),
            ..Default::default()
        };
        assert_eq!(
            cfg.install_cmd_with(Platform::host(), true),
            "rustup component add rust-analyzer"
        );
    }

    #[test]
    fn rust_analyzer_falls_back_to_cargo_without_rustup() {
        // If rustup isn't on PATH (rare — most Rust users have rustup),
        // honour the manifest's cargo install command.
        let cfg = LspConfig {
            binary: "rust-analyzer".to_string(),
            install: "cargo install rust-analyzer".to_string(),
            ..Default::default()
        };
        assert_eq!(
            cfg.install_cmd_with(Platform::host(), false),
            "cargo install rust-analyzer"
        );
    }

    #[test]
    fn rust_analyzer_respects_platform_specific_override() {
        // If the manifest sets an explicit `install_linux` / `install_macos` /
        // `install_windows`, that wins — the fixup only fires against the
        // generic `cargo install` fallback.
        let cfg = LspConfig {
            binary: "rust-analyzer".to_string(),
            install: "cargo install rust-analyzer".to_string(),
            install_linux: "apt install rust-analyzer".to_string(),
            install_macos: "brew install rust-analyzer".to_string(),
            install_windows: "winget install rust-analyzer".to_string(),
            ..Default::default()
        };
        // The platform-specific field is selected (not the cargo command),
        // so the fixup does not match `starts_with("cargo install")`.
        assert!(!cfg
            .install_cmd_with(Platform::host(), true)
            .starts_with("rustup"));
    }

    #[test]
    fn install_fixup_only_targets_rust_analyzer() {
        // Other binaries that happen to use `cargo install` should be left alone.
        let cfg = LspConfig {
            binary: "some-other-server".to_string(),
            install: "cargo install some-other-server".to_string(),
            ..Default::default()
        };
        assert_eq!(
            cfg.install_cmd_with(Platform::host(), true),
            "cargo install some-other-server"
        );
    }

    #[test]
    fn install_cmd_for_resolves_explicit_platform_regardless_of_host() {
        // #919: the whole point of `install_cmd_for(Platform)` is that a
        // test running on any host OS can ask "what would this resolve to
        // on Windows/macOS/Linux specifically" — not just "what does it
        // resolve to on the OS I happen to be compiled for".
        let cfg = LspConfig {
            binary: "clangd".to_string(),
            install_linux: "sudo apt-get install -y clangd".to_string(),
            install_macos: "brew install llvm".to_string(),
            ..Default::default()
        };
        assert_eq!(
            cfg.install_cmd_for(Platform::Linux),
            "sudo apt-get install -y clangd"
        );
        assert_eq!(cfg.install_cmd_for(Platform::MacOS), "brew install llvm");
        // No install_windows and no generic `install` fallback set → empty.
        assert_eq!(cfg.install_cmd_for(Platform::Windows), "");
    }

    #[test]
    fn board_provider_config_defaults_when_absent_from_toml() {
        // A manifest with no [board] section should parse to `None`, not
        // an error — most extensions never touch the board panel.
        let toml = r#"
name = "rust"
display_name = "Rust Language Support"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert!(m.board.is_none());
    }

    #[test]
    fn board_provider_config_parses_from_toml() {
        let toml = r#"
name = "example-provider"
display_name = "Example Board Provider"

[board]
refresh_command = ["example-tool", "board", "--json"]
poll_interval_secs = 15

[[board.actions]]
name = "OpenIssue"
command = ["example-tool", "open", "{id}"]

[[board.actions]]
name = "merge"
label = "Merge"
command = ["example-tool", "merge", "{id}"]
stages = ["col:review"]
key = "m"
confirm = true
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        let board = m.board.expect("board provider config should be present");
        assert_eq!(
            board.refresh_command,
            vec!["example-tool", "board", "--json"]
        );
        assert_eq!(board.poll_interval_secs, 15);
        assert_eq!(
            board
                .action_by_name("OpenIssue")
                .map(|a| a.resolve_argv("card:42")),
            Some(vec![
                "example-tool".to_string(),
                "open".to_string(),
                "card:42".to_string()
            ])
        );
        assert_eq!(board.action_by_name("merge_typo"), None);

        let merge = board.action_by_name("merge").expect("merge declared");
        assert_eq!(merge.display_label(), "Merge");
        assert!(merge.confirm);
        assert_eq!(merge.key.as_deref(), Some("m"));
        assert_eq!(
            board.action_for_key("m", "col:review").map(|a| &a.name),
            Some(&"merge".to_string())
        );
        assert_eq!(board.action_for_key("m", "col:backlog"), None);
        assert_eq!(
            board
                .actions_for_stage("col:backlog")
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["OpenIssue"],
            "an action with no declared `stages` is valid everywhere"
        );
    }

    #[test]
    fn document_provider_config_defaults_when_absent_from_toml() {
        let toml = r#"
name = "rust"
display_name = "Rust Language Support"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert!(m.document.is_none());
    }

    #[test]
    fn document_provider_config_parses_from_toml() {
        let toml = r#"
name = "example-provider"
display_name = "Example Document Provider"

[document]
read_command = ["example-tool", "show", "{id}"]
write_command = ["example-tool", "write", "{id}"]
write_follow_up = ["example-tool", "ready", "{id}"]
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        let doc = m
            .document
            .expect("document provider config should be present");
        assert_eq!(
            doc.read_argv("42"),
            Some(vec![
                "example-tool".to_string(),
                "show".to_string(),
                "42".to_string()
            ])
        );
        assert_eq!(
            doc.write_argv(Some("42")),
            Some(vec![
                "example-tool".to_string(),
                "write".to_string(),
                "42".to_string()
            ])
        );
        // New-document flow: no id yet, `{id}` substitutes to empty.
        assert_eq!(
            doc.write_argv(None),
            Some(vec![
                "example-tool".to_string(),
                "write".to_string(),
                "".to_string()
            ])
        );
        assert_eq!(
            doc.write_follow_up_argv("42"),
            Some(vec![
                "example-tool".to_string(),
                "ready".to_string(),
                "42".to_string()
            ])
        );
    }

    #[test]
    fn document_provider_config_no_declared_command_is_none() {
        let doc = DocumentProviderConfig::default();
        assert_eq!(doc.read_argv("42"), None);
        assert_eq!(doc.write_argv(Some("42")), None);
        assert_eq!(doc.write_follow_up_argv("42"), None);
    }

    #[test]
    fn board_provider_config_poll_interval_defaults_when_unset() {
        let toml = r#"
name = "example-provider"
display_name = "Example Board Provider"

[board]
refresh_command = ["example-tool", "board", "--json"]
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        let board = m.board.expect("board provider config should be present");
        assert_eq!(board.poll_interval_secs, 30);
    }

    #[test]
    fn lsp_acquire_absent_from_toml_parses_to_none() {
        // #1345: every existing registry manifest has no `[lsp.acquire]` /
        // `[dap.acquire]` table — confirm they stay unaffected.
        let toml = r#"
name = "rust"
display_name = "Rust Language Support"
[lsp]
binary = "rust-analyzer"
install = "cargo install rust-analyzer"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert!(m.lsp.acquire.is_none());
        assert!(m.dap.acquire.is_none());
    }

    #[test]
    fn lsp_acquire_table_parses_from_toml() {
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
        let acquire = m.lsp.acquire.expect("acquire table should parse");
        assert_eq!(
            acquire.kind,
            crate::core::tool_acquire::AcquireKind::HashicorpRelease
        );
        assert_eq!(acquire.product, "terraform-ls");
    }

    #[test]
    fn dap_install_cmd_for_resolves_explicit_platform() {
        let cfg = DapConfig {
            adapter: "netcoredbg".to_string(),
            install_linux: "sudo apt-get install -y netcoredbg".to_string(),
            install_windows: "winget install netcoredbg".to_string(),
            ..Default::default()
        };
        assert_eq!(
            cfg.install_cmd_for(Platform::Linux),
            "sudo apt-get install -y netcoredbg"
        );
        assert_eq!(
            cfg.install_cmd_for(Platform::Windows),
            "winget install netcoredbg"
        );
        assert_eq!(cfg.install_cmd_for(Platform::MacOS), "");
    }
}
