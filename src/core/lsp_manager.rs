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

pub fn install_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/vimcode-install.log")
    {
        let _ = writeln!(f, "{msg}");
        let _ = writeln!(f, "---");
    }
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

/// Resolve a command to an absolute path.
/// Checks Mason bin directory first (if it exists), then falls back to PATH.
fn resolve_command(cmd: &str) -> Option<PathBuf> {
    // Split on whitespace to get just the binary name
    let binary = cmd.split_whitespace().next().unwrap_or(cmd);

    // Check Mason bin directory first
    if let Some(mason_bin) = mason_bin_dir() {
        let candidate = mason_bin.join(binary);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Check common tool directories that may not be in PATH when launched
    // from a desktop environment (not a login shell).
    let home = super::paths::home_dir();
    let tool_dirs = [
        home.join(".dotnet/tools"),
        home.join(".cargo/bin"),
        home.join(".local/bin"),
        home.join("go/bin"),
        home.join(".npm-global/bin"),
    ];
    for dir in &tool_dirs {
        let candidate = dir.join(binary);
        if candidate.exists() {
            return Some(candidate);
        }
        // On Windows, also check with .exe suffix
        #[cfg(target_os = "windows")]
        if !binary.ends_with(".exe") {
            let exe = dir.join(format!("{binary}.exe"));
            if exe.exists() {
                return Some(exe);
            }
        }
    }

    // Fall back to PATH lookup via `which`/`where`
    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    let mut cmd = std::process::Command::new(which_cmd);
    cmd.arg(binary);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().ok()?;
    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout);
        // `where` on Windows may return multiple lines — take the first
        let first_line = path_str.lines().next()?.trim();
        if !first_line.is_empty() {
            return Some(PathBuf::from(first_line));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// LspManager — coordinates multiple language servers
// ---------------------------------------------------------------------------

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
    /// covered by an extension, so we don't fall back to the built-in registry for languages
    /// that have a (not-yet-installed) extension.
    all_ext_manifests: Vec<extensions::ExtensionManifest>,
    /// Servers that have returned at least one non-empty response (symbols, hover, etc.).
    /// This indicates the server has finished indexing and is truly "ready".
    server_has_responded: HashMap<LspServerId, bool>,
    /// Servers that crashed or exited (for display in :LspInfo).
    crashed_servers: Vec<String>,
    /// Last error from `ensure_server_for_language` (dependency check failure, etc.).
    /// Engine reads and clears this after calling ensure_server.
    pub last_start_error: Option<String>,
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
            crashed_servers: Vec::new(),
            last_start_error: None,
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
    }

    /// Ensure a server is running for the given language. Returns the server ID
    /// if a server is available (or was just started), None if no config exists
    /// or the binary is not on PATH/Mason bin.
    pub fn ensure_server_for_language(&mut self, language_id: &str) -> Option<LspServerId> {
        // Already running?
        if let Some(&id) = self.language_to_server.get(language_id) {
            return Some(id);
        }

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
                let name = if manifest.display_name.is_empty() {
                    &manifest.name
                } else {
                    &manifest.display_name
                };
                self.last_start_error = Some(format!(
                    "{} requires {} — install {} and try again",
                    name,
                    missing.join(", "),
                    missing.join(", "),
                ));
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
        let (mut config, resolved) = candidates
            .into_iter()
            .find_map(|c| resolve_command(&c.command).map(|p| (c, p)))?;
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
            Err(_) => None,
        }
    }

    /// Add a server config to the in-memory registry (does not persist to disk).
    pub fn add_registry_entry(&mut self, config: LspServerConfig) {
        self.registry.push(config);
    }

    /// Spawn a background thread to run an install command.
    /// The result is sent as `LspEvent::InstallComplete` on the shared channel.
    /// Full command + output is always appended to `/tmp/vimcode-install.log`.
    pub fn run_install_command(&self, lang_id: &str, install_cmd: &str) {
        let tx = self.event_tx.clone();
        let lang_id = lang_id.to_string();
        let install_cmd = install_cmd.to_string();
        std::thread::spawn(move || {
            install_log(&format!(
                "[{}] START lang_id={lang_id}\nCMD: {install_cmd}\nPATH: {}",
                timestamp(),
                std::env::var("PATH").unwrap_or_else(|_| "(unset)".into()),
            ));

            // Run via shell so npm/pip/dotnet etc. resolve from user PATH
            #[cfg(target_os = "windows")]
            let result = {
                use std::os::windows::process::CommandExt;
                std::process::Command::new("cmd")
                    .args(["/C", &install_cmd])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output()
            };
            #[cfg(not(target_os = "windows"))]
            let result = std::process::Command::new("sh")
                .args(["-c", &install_cmd])
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
