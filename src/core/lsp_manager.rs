use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use super::lsp::{
    language_id_from_path, mason_package_for_language, parse_mason_package_yaml, path_to_uri,
    LspEvent, LspServer, LspServerConfig, LspServerId, MasonPackageInfo,
};

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
        },
        // Python — ordered fallbacks (first binary found on PATH/Mason wins)
        LspServerConfig {
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec!["python".to_string()],
        },
        LspServerConfig {
            command: "basedpyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec!["python".to_string()],
        },
        LspServerConfig {
            command: "pylsp".to_string(),
            args: vec![],
            languages: vec!["python".to_string()],
        },
        LspServerConfig {
            command: "jedi-language-server".to_string(),
            args: vec![],
            languages: vec!["python".to_string()],
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
        },
        LspServerConfig {
            command: "gopls".to_string(),
            args: vec![],
            languages: vec!["go".to_string()],
        },
        LspServerConfig {
            command: "clangd".to_string(),
            args: vec![],
            languages: vec!["c".to_string(), "cpp".to_string()],
        },
        LspServerConfig {
            command: "csharp-ls".to_string(),
            args: vec![],
            languages: vec!["csharp".to_string()],
        },
        LspServerConfig {
            command: "lua-language-server".to_string(),
            args: vec![],
            languages: vec!["lua".to_string()],
        },
        LspServerConfig {
            command: "bash-language-server".to_string(),
            args: vec!["start".to_string()],
            languages: vec!["shellscript".to_string()],
        },
        LspServerConfig {
            command: "yaml-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            languages: vec!["yaml".to_string()],
        },
        LspServerConfig {
            command: "kotlin-language-server".to_string(),
            args: vec![],
            languages: vec!["kotlin".to_string()],
        },
        LspServerConfig {
            command: "zls".to_string(),
            args: vec![],
            languages: vec!["zig".to_string()],
        },
        LspServerConfig {
            command: "elixir-ls".to_string(),
            args: vec![],
            languages: vec!["elixir".to_string()],
        },
        LspServerConfig {
            command: "ruby-lsp".to_string(),
            args: vec![],
            languages: vec!["ruby".to_string()],
        },
        LspServerConfig {
            command: "terraform-ls".to_string(),
            args: vec!["serve".to_string()],
            languages: vec!["terraform".to_string()],
        },
        LspServerConfig {
            command: "marksman".to_string(),
            args: vec!["server".to_string()],
            languages: vec!["markdown".to_string()],
        },
        LspServerConfig {
            command: "taplo".to_string(),
            args: vec!["lsp".to_string(), "stdio".to_string()],
            languages: vec!["toml".to_string()],
        },
        LspServerConfig {
            command: "sourcekit-lsp".to_string(),
            args: vec![],
            languages: vec!["swift".to_string()],
        },
        LspServerConfig {
            command: "metals".to_string(),
            args: vec![],
            languages: vec!["scala".to_string()],
        },
    ]
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

    // Fall back to PATH lookup via `which`/`where`
    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    let output = std::process::Command::new(which_cmd)
        .arg(binary)
        .output()
        .ok()?;
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
    /// In-memory cache of Mason registry lookups (keyed by language ID).
    pub registry_cache: HashMap<String, MasonPackageInfo>,
}

impl LspManager {
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
            registry_cache: HashMap::new(),
        }
    }

    /// Ensure a server is running for the given language. Returns the server ID
    /// if a server is available (or was just started), None if no config exists
    /// or the binary is not on PATH/Mason bin.
    pub fn ensure_server_for_language(&mut self, language_id: &str) -> Option<LspServerId> {
        // Already running?
        if let Some(&id) = self.language_to_server.get(language_id) {
            return Some(id);
        }

        // Find a matching config where the binary is available (try all configs — first one wins).
        // Use the resolved full path so the spawn works regardless of the process's PATH.
        let (mut config, resolved) = self
            .registry
            .iter()
            .filter(|c| c.languages.iter().any(|l| l == language_id))
            .find_map(|c| resolve_command(&c.command).map(|p| (c.clone(), p)))?;
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
                eprintln!("LSP: Failed to start {} — {}", config.command, e);
                None
            }
        }
    }

    /// Add a server config to the in-memory registry (does not persist to disk).
    pub fn add_registry_entry(&mut self, config: LspServerConfig) {
        self.registry.push(config);
    }

    /// Spawn a background thread to fetch Mason registry metadata for a language.
    /// The result is sent as `LspEvent::RegistryLookup` on the shared channel.
    pub fn fetch_mason_registry_for_language(&self, lang_id: &str) {
        let pkg_name = match mason_package_for_language(lang_id) {
            Some(p) => p,
            None => {
                // No Mason mapping — send None immediately
                let _ = self.event_tx.send(LspEvent::RegistryLookup {
                    lang_id: lang_id.to_string(),
                    info: None,
                });
                return;
            }
        };
        let tx = self.event_tx.clone();
        let lang_id = lang_id.to_string();
        let pkg_name = pkg_name.to_string();
        std::thread::spawn(move || {
            let url = format!(
                "https://raw.githubusercontent.com/mason-org/mason-registry/main/packages/{pkg_name}/package.yaml"
            );
            let output = std::process::Command::new("curl")
                .args(["-sf", "--max-time", "10", &url])
                .output();
            match output {
                Ok(out) if out.status.success() => {
                    let yaml = String::from_utf8_lossy(&out.stdout);
                    let info = parse_mason_package_yaml(&yaml);
                    let _ = tx.send(LspEvent::RegistryLookup {
                        lang_id,
                        info: Some(info),
                    });
                }
                _ => {
                    let _ = tx.send(LspEvent::RegistryLookup {
                        lang_id,
                        info: None,
                    });
                }
            }
        });
    }

    /// Spawn a background thread to run an install command.
    /// The result is sent as `LspEvent::InstallComplete` on the shared channel.
    pub fn run_install_command(&self, lang_id: &str, install_cmd: &str) {
        let tx = self.event_tx.clone();
        let lang_id = lang_id.to_string();
        let install_cmd = install_cmd.to_string();
        std::thread::spawn(move || {
            // Run via shell so npm/pip/dotnet etc. resolve from user PATH
            #[cfg(target_os = "windows")]
            let result = std::process::Command::new("cmd")
                .args(["/C", &install_cmd])
                .output();
            #[cfg(not(target_os = "windows"))]
            let result = std::process::Command::new("sh")
                .args(["-c", &install_cmd])
                .output();

            match result {
                Ok(out) => {
                    let success = out.status.success();
                    let output = if success {
                        String::from_utf8_lossy(&out.stdout).into_owned()
                    } else {
                        String::from_utf8_lossy(&out.stderr).into_owned()
                    };
                    let output = output.trim().to_string();
                    let _ = tx.send(LspEvent::InstallComplete {
                        lang_id,
                        success,
                        output,
                    });
                }
                Err(e) => {
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
                    if let LspEvent::Initialized(server_id) = &event {
                        if let Some(server) = self.servers.get(*server_id) {
                            server.send_initialized();
                            self.initialized.insert(*server_id, true);
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
                // No server available — let the engine handle the registry lookup
                Err(format!("No LSP server found for {language_id}"))
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
    fn server_and_uri(&mut self, path: &Path) -> Option<(usize, String)> {
        let language_id = language_id_from_path(path)?;
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

        // Find config and restart, using the resolved full binary path.
        let mut config = self
            .registry
            .iter()
            .find(|c| c.languages.iter().any(|l| l == language_id))?
            .clone();
        let resolved = resolve_command(&config.command)?;
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

    /// Get status information about running servers.
    pub fn server_info(&self) -> Vec<String> {
        let mut info = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (lang, &server_id) in &self.language_to_server {
            if seen.insert(server_id) {
                let status = if self.initialized.get(&server_id).copied().unwrap_or(false) {
                    "running"
                } else {
                    "initializing"
                };
                let cmd = &self
                    .registry
                    .iter()
                    .find(|c| c.languages.iter().any(|l| l == lang))
                    .map(|c| c.command.as_str())
                    .unwrap_or("unknown");
                info.push(format!("{}: {} ({})", cmd, status, lang));
            }
        }
        if info.is_empty() {
            info.push("No LSP servers running".to_string());
        }
        info
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}
