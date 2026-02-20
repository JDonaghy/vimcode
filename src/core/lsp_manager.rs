use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use super::lsp::{
    language_id_from_path, path_to_uri, LspEvent, LspServer, LspServerConfig, LspServerId,
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
        LspServerConfig {
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
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
    ]
}

/// Check whether a command exists on PATH.
fn command_exists(cmd: &str) -> bool {
    // Split on whitespace to get just the binary name
    let binary = cmd.split_whitespace().next().unwrap_or(cmd);
    std::process::Command::new("which")
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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
        }
    }

    /// Ensure a server is running for the given language. Returns the server ID
    /// if a server is available (or was just started), None if no config exists
    /// or the binary is not on PATH.
    pub fn ensure_server_for_language(&mut self, language_id: &str) -> Option<LspServerId> {
        // Already running?
        if let Some(&id) = self.language_to_server.get(language_id) {
            return Some(id);
        }

        // Find a matching config
        let config = self
            .registry
            .iter()
            .find(|c| c.languages.iter().any(|l| l == language_id))?
            .clone();

        // Check if the binary exists on PATH
        if !command_exists(&config.command) {
            return None;
        }

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

        // Find a matching config
        let config = match self
            .registry
            .iter()
            .find(|c| c.languages.iter().any(|l| l == &language_id))
        {
            Some(c) => c.clone(),
            None => return Ok(()), // no config for this language
        };

        // Check if the binary exists on PATH
        if !command_exists(&config.command) {
            return Err(format!("{} not found on PATH", config.command));
        }

        // Start the server (don't send didOpen yet — wait for Initialized)
        match self.ensure_server_for_language(&language_id) {
            Some(_) => Ok(()),
            None => Err(format!("Failed to start {}", config.command)),
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

        // Find config and restart
        let config = self
            .registry
            .iter()
            .find(|c| c.languages.iter().any(|l| l == language_id))?
            .clone();
        if !command_exists(&config.command) {
            return None;
        }
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
