use super::*;

impl Engine {
    // =======================================================================
    // LSP integration
    // =======================================================================

    /// Ensure the LSP manager is initialized (lazy — created on first use).
    pub(crate) fn ensure_lsp_manager(&mut self) {
        if !self.settings.lsp_enabled || self.lsp_manager.is_some() {
            return;
        }
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut mgr = LspManager::new(root, &self.settings.lsp_servers);
        mgr.set_ext_manifests(
            self.ext_installed_manifests(),
            self.ext_available_manifests(),
        );
        self.lsp_manager = Some(mgr);
    }

    /// Ensure LSP is started for the active buffer (lazy — called on tab switch).
    /// This is idempotent: if the server is already running, didOpen is a no-op.
    pub fn lsp_ensure_active_buffer(&mut self) {
        let bid = self.active_buffer_id();
        let has_file = self
            .buffer_manager
            .get(bid)
            .and_then(|s| s.file_path.as_ref())
            .is_some();
        if has_file {
            self.lsp_did_open(bid);
        }
    }

    /// Notify LSP that a file was opened.
    pub(crate) fn lsp_did_open(&mut self, buffer_id: BufferId) {
        // Fire plugin "open" hook regardless of LSP enabled state
        if let Some(state) = self.buffer_manager.get(buffer_id) {
            if let Some(path) = state.file_path.clone() {
                let path_str = path.to_string_lossy().into_owned();
                self.plugin_event("open", &path_str);
                self.plugin_event("BufNew", &path_str);
                self.plugin_event("BufEnter", &path_str);
            }
        }
        // Fire cursor_move so position-aware plugins (e.g. git-insights blame) annotate
        // the initial cursor line immediately on file open without requiring a keypress.
        self.fire_cursor_move_hook_now();
        if !self.settings.lsp_enabled {
            return;
        }
        let (path, text, lang_id) = {
            let state = match self.buffer_manager.get(buffer_id) {
                Some(s) => s,
                None => return,
            };
            let path = match &state.file_path {
                Some(p) => p.clone(),
                None => return,
            };
            // User language_map takes priority; fall back to built-in extension table
            let lang_id = lsp::language_id_from_path_with_map(&path, &self.settings.language_map)
                .or_else(|| state.lsp_language_id.clone());
            let lang_id = match lang_id {
                Some(l) => l,
                None => return,
            };
            (path, state.buffer.to_string(), lang_id)
        };
        self.ensure_lsp_manager();
        let no_server = if let Some(mgr) = &mut self.lsp_manager {
            mgr.notify_did_open(&path, &text).err()
        } else {
            None
        };
        // Request semantic tokens after opening a file.
        self.lsp_request_semantic_tokens(&path);
        // Show extension hint based on VimCode extension state (independent of LSP binary
        // availability — ext_remove intentionally leaves the binary on disk).
        let manifests = self.ext_available_manifests();
        if let Some(manifest) =
            crate::core::extensions::find_manifest_for_language_id(&manifests, &lang_id)
        {
            let name = &manifest.name;
            if !self.extension_state.is_installed(name)
                && !self.extension_state.is_dismissed(name)
                && !self.prompted_extensions.contains(name.as_str())
            {
                self.prompted_extensions.insert(name.to_string());
                self.ext_hint_pending_name = Some(name.to_string());
                self.message = format!(
                    "No {} extension — :ExtInstall {}  (N to dismiss)",
                    manifest.display_name, name
                );
            }
        } else if let Some(err) = no_server {
            // Show dependency errors prominently; generic "no server" only as fallback.
            self.message = err;
        }
    }

    // ── Extension registry + sidebar ──────────────────────────────────────────

    /// Return the list of available extensions from the cached registry.
    /// Return manifests only for extensions that are installed.
    /// Used for LSP manager — only start servers when the extension is installed.
    pub fn ext_installed_manifests(&self) -> Vec<crate::core::extensions::ExtensionManifest> {
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| self.extension_state.is_installed(&m.name))
            .collect()
    }

    pub fn ext_available_manifests(&self) -> Vec<crate::core::extensions::ExtensionManifest> {
        let mut result: Vec<crate::core::extensions::ExtensionManifest> =
            self.ext_registry.clone().unwrap_or_default();

        // Merge local extensions: scan extensions/*/manifest.toml in config dir
        // so developers can test extensions locally before publishing to the registry.
        let ext_base = paths::vimcode_config_dir().join("extensions");
        if let Ok(entries) = std::fs::read_dir(&ext_base) {
            for entry in entries.filter_map(|e| e.ok()) {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let manifest_path = dir.join("manifest.toml");
                if let Ok(toml_str) = std::fs::read_to_string(&manifest_path) {
                    if let Some(manifest) =
                        crate::core::extensions::ExtensionManifest::parse(&toml_str)
                    {
                        // Local manifest overrides registry entry with same name
                        result.retain(|m| !m.name.eq_ignore_ascii_case(&manifest.name));
                        result.push(manifest);
                    }
                }
            }
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Spawn a background thread to fetch all configured extension registries.
    /// Result arrives via `ext_registry_rx`.
    pub fn ext_refresh(&mut self) {
        if self.ext_registry_fetching {
            return; // already in progress
        }
        let urls = self.settings.extension_registries.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut merged: Vec<crate::core::extensions::ExtensionManifest> = Vec::new();
            for url in &urls {
                if let Some(mut entries) = registry::fetch_registry(url) {
                    let base = registry::base_url_from_registry(url);
                    for m in &mut entries {
                        m.registry_base_url = base.clone();
                    }
                    // Later registries override earlier ones on name collision
                    for entry in entries {
                        merged.retain(|m| !m.name.eq_ignore_ascii_case(&entry.name));
                        merged.push(entry);
                    }
                }
            }
            let result = if merged.is_empty() && !urls.is_empty() {
                None // all fetches failed
            } else {
                Some(merged)
            };
            let _ = tx.send(result);
        });
        self.ext_registry_rx = Some(rx);
        self.ext_registry_fetching = true;
        self.message = "Fetching extension registries...".to_string();
    }

    /// Non-blocking check for a completed registry fetch.
    /// Call this from `handle_key` / `poll_lsp`.
    pub fn poll_ext_registry(&mut self) -> bool {
        let result = if let Some(rx) = &self.ext_registry_rx {
            rx.try_recv().ok()
        } else {
            return false;
        };
        if let Some(maybe_reg) = result {
            self.ext_registry_fetching = false;
            self.ext_registry_rx = None;
            match maybe_reg {
                Some(entries) => {
                    let count = entries.len();
                    registry::save_cache(&entries);
                    self.ext_registry = Some(entries);
                    // Re-filter stored diagnostics with updated ignore_error_sources.
                    self.refilter_diagnostics();
                    self.message = format!("Extension registry updated ({count} extensions)");
                }
                None => {
                    self.message = "Registry fetch failed — try again later".to_string();
                }
            }
            true
        } else {
            false
        }
    }

    /// Resolve the base URL for downloading extension files.
    /// Uses the manifest's `registry_base_url` if available, otherwise derives it
    /// from the first configured registry URL (the field is `#[serde(skip)]` so it's
    /// empty when loaded from cache).
    pub(crate) fn resolve_registry_base_url(
        &self,
        manifest: &crate::core::extensions::ExtensionManifest,
    ) -> String {
        if !manifest.registry_base_url.is_empty() {
            return manifest.registry_base_url.clone();
        }
        self.settings
            .extension_registries
            .first()
            .map(|url| registry::base_url_from_registry(url))
            .unwrap_or_default()
    }

    /// Install an extension by name: download scripts, run LSP/DAP install, mark installed.
    pub fn ext_install_from_registry(&mut self, name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));
        let manifest = match manifest {
            Some(m) => m,
            None => {
                self.message =
                    format!("Unknown extension '{name}' — try :ExtRefresh then :ExtList");
                return;
            }
        };
        let ext_name = manifest.name.clone();

        // Download scripts from the registry (skip files already on disk for local dev)
        let ext_dir = paths::vimcode_config_dir()
            .join("extensions")
            .join(&ext_name);
        let base_url = self.resolve_registry_base_url(&manifest);
        if !manifest.scripts.is_empty()
            && !base_url.is_empty()
            && std::fs::create_dir_all(&ext_dir).is_ok()
        {
            for script in &manifest.scripts {
                let dest = ext_dir.join(script);
                if !dest.exists() {
                    let url = format!("{}/{}/{}", base_url, ext_name, script);
                    let _ = registry::download_script(&url, &dest);
                }
            }
        }

        let mut status_parts: Vec<String> = Vec::new();
        let mut install_commands: Vec<String> = Vec::new();

        // ── LSP ──────────────────────────────────────────────────────────────
        // Check if any LSP binary is already on PATH (idempotent: skip install
        // if the server is already available, e.g. via `rustup component add`).
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            let found_bin = all_lsp.iter().copied().find(|b| binary_on_path(b));
            if let Some(bin) = found_bin {
                status_parts.push(format!("LSP: {bin} ✓"));
            } else if !manifest.lsp.install_cmd_for_platform().is_empty() {
                let lsp_key = format!("ext:{ext_name}:lsp");
                self.lsp_installing.insert(lsp_key.clone());
                install_commands.push(manifest.lsp.install_cmd_for_platform().to_string());
                self.pending_install_context = Some(InstallContext {
                    ext_name: ext_name.clone(),
                    install_key: lsp_key,
                });
                self.notify(
                    NotificationKind::LspInstall,
                    &format!("Installing {}…", manifest.lsp.binary),
                );
                status_parts.push(format!("LSP: installing {}…", manifest.lsp.binary));
            }
        }

        // ── DAP ──────────────────────────────────────────────────────────────
        // Check PATH first (idempotent).  Only auto-install if the manifest
        // provides an explicit dap.install command.  An empty dap.install means
        // "this adapter needs a manual/complex install" — guide the user to run
        // :DapInstall instead of silently attempting a potentially large download.
        if !manifest.dap.adapter.is_empty() {
            let dap_binary = manifest.dap.binary.as_str();
            let already_on_path = !dap_binary.is_empty() && binary_on_path(dap_binary);
            if already_on_path {
                status_parts.push(format!("DAP: {dap_binary} ✓"));
            } else if !manifest.dap.install_cmd_for_platform().is_empty() {
                let dap_key = format!("dap:{}", manifest.dap.adapter);
                self.lsp_installing.insert(dap_key.clone());
                install_commands.push(manifest.dap.install_cmd_for_platform().to_string());
                // Only set install context if LSP didn't already set it.
                if self.pending_install_context.is_none() {
                    self.pending_install_context = Some(InstallContext {
                        ext_name: ext_name.clone(),
                        install_key: dap_key,
                    });
                }
                status_parts.push(format!("DAP: installing {}…", manifest.dap.adapter));
            } else if !dap_binary.is_empty() {
                // No auto-install — guide the user to :DapInstall.
                status_parts.push(format!(
                    "DAP: run :DapInstall {ext_name} to set up {dap_binary}"
                ));
            }
        }

        // If there are install commands, combine them and store for the UI to run
        // in a visible terminal pane.
        let has_install = !install_commands.is_empty();
        if has_install {
            // Use `;` as separator — `&&` is not valid in PowerShell 5.x
            // (Windows default).  `;` works in both PowerShell and bash.
            let combined = install_commands.join(" ; ");
            let header = format!("echo '── Installing {ext_name} ──'");
            self.pending_terminal_command = Some(format!("{header} ; {combined}"));
        }

        // Mark installed with version and persist
        self.extension_state
            .mark_installed_version(&ext_name, &manifest.version);
        let _ = self.extension_state.save();

        // Reload plugins so newly extracted scripts are active
        self.plugin_manager = None;
        self.plugin_init();

        // Kick-start LSP for the current buffer if it matches this extension's languages.
        // Without this, the user would have to re-open the file to get LSP support.
        // Skip if an install is pending — the binary isn't available yet; LSP will be
        // started when the install terminal completes.
        if !has_install {
            let active_bid = self.active_buffer_id();
            if let Some(state) = self.buffer_manager.get(active_bid) {
                let buf_lang = state.lsp_language_id.clone().or_else(|| {
                    state
                        .file_path
                        .as_ref()
                        .and_then(|p| lsp::language_id_from_path(p))
                });
                let matches = buf_lang
                    .as_ref()
                    .is_some_and(|lang| manifest.language_ids.iter().any(|l| l == lang));
                if matches {
                    self.lsp_did_open(active_bid);
                }
            }
        }

        self.message = if status_parts.is_empty() {
            format!("Extension '{ext_name}' installed")
        } else {
            format!(
                "Extension '{ext_name}' installed — {}",
                status_parts.join(", ")
            )
        };
    }

    /// Open the README for the currently selected extension in the sidebar.
    /// Used by Enter and double-click.
    pub fn ext_open_selected_readme(&mut self) {
        let manifests = self.ext_available_manifests();
        let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
        let manifest = if in_installed {
            let installed = self.ext_installed_items();
            installed
                .get(idx)
                .and_then(|m| manifests.iter().find(|r| r.name == m.name))
        } else {
            let available = self.ext_available_items();
            available
                .get(idx)
                .and_then(|a| manifests.iter().find(|m| m.name == a.name))
        };
        if let Some(manifest) = manifest {
            let name = manifest.name.clone();
            let display = if manifest.display_name.is_empty() {
                name.clone()
            } else {
                manifest.display_name.clone()
            };
            let base_url = self.resolve_registry_base_url(manifest);
            let readme_path = paths::vimcode_config_dir()
                .join("extensions")
                .join(&name)
                .join("README.md");
            let content = std::fs::read_to_string(&readme_path)
                .ok()
                .or_else(|| registry::fetch_readme(&base_url, &name));
            if let Some(content) = content {
                self.open_markdown_preview_in_tab(&content, &display);
            } else {
                self.message = format!("No README available for '{name}'. Press i to install.");
            }
        }
    }

    /// Show a confirmation dialog before removing an extension.
    /// Lists the tools that would be removed and offers three choices.
    pub(crate) fn ext_show_remove_dialog(&mut self, name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));

        // Collect tool binary names that are currently installed on PATH.
        let mut tool_names: Vec<String> = Vec::new();
        if let Some(ref m) = manifest {
            if !m.lsp.binary.is_empty() && binary_on_path(&m.lsp.binary) {
                tool_names.push(m.lsp.binary.clone());
            }
            if !m.dap.binary.is_empty() && binary_on_path(&m.dap.binary) {
                // Avoid duplicates (some extensions share a binary).
                if !tool_names.contains(&m.dap.binary) {
                    tool_names.push(m.dap.binary.clone());
                }
            }
        }

        let mut body = vec![format!("Remove extension '{name}'?")];
        if tool_names.is_empty() {
            body.push("This will remove extension scripts and settings.".to_string());
        } else {
            body.push(String::new());
            body.push(format!("Installed tools: {}", tool_names.join(", ")));
        }

        let buttons = if tool_names.is_empty() {
            vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: 'c',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "Remove".into(),
                    hotkey: 'r',
                    action: "remove".into(),
                },
            ]
        } else {
            vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: 'c',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "Keep Tools".into(),
                    hotkey: 'k',
                    action: "keep_tools".into(),
                },
                DialogButton {
                    label: "Remove All".into(),
                    hotkey: 'a',
                    action: "remove_all".into(),
                },
            ]
        };

        self.pending_ext_remove = Some(name.to_string());
        self.show_dialog("ext_remove", "Remove Extension", body, buttons);
    }

    /// Remove an extension: unmark as installed, delete its Lua scripts.
    /// When `remove_tools` is true, also delete LSP/DAP binaries from PATH.
    pub fn ext_remove(&mut self, name: &str, remove_tools: bool) {
        let name = name.to_string();

        // Optionally remove installed tool binaries before clearing state.
        if remove_tools {
            self.ext_remove_tools(&name);
        }

        self.extension_state.installed.retain(|e| e.name != name);
        let _ = self.extension_state.save();

        // Remove in-memory extension settings
        self.ext_settings.remove(&name);
        self.ext_settings_collapsed.remove(&name);

        let ext_dir = paths::vimcode_config_dir().join("extensions").join(&name);
        let _ = std::fs::remove_dir_all(&ext_dir);

        // Reload plugins so removed scripts are no longer active
        self.plugin_manager = None;
        self.plugin_init();

        if remove_tools {
            self.message = format!("Extension '{name}' and its tools removed");
        } else {
            self.message = format!("Extension '{name}' removed (tools kept on PATH)");
        }

        // Keep sidebar selection in bounds.
        if self.ext_flat_item_count() == 0 {
            self.ext_sidebar_sections_expanded[1] = true;
        }
        let new_total = self.ext_flat_item_count();
        if new_total > 0 && self.ext_sidebar_selected >= new_total {
            self.ext_sidebar_selected = new_total - 1;
        }
    }

    /// Remove LSP/DAP tool binaries installed by an extension.
    /// Only removes binaries found under well-known managed directories
    /// (~/.local/bin, ~/.local/share/<name>, Mason bin dir).
    pub(crate) fn ext_remove_tools(&mut self, name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));
        let manifest = match manifest {
            Some(m) => m,
            None => return,
        };

        let mut removed: Vec<String> = Vec::new();

        // Collect all binary names to check.
        let mut bins: Vec<String> = Vec::new();
        if !manifest.lsp.binary.is_empty() {
            bins.push(manifest.lsp.binary.clone());
        }
        if !manifest.dap.binary.is_empty() && !bins.contains(&manifest.dap.binary) {
            bins.push(manifest.dap.binary.clone());
        }

        // Safe directories where we allow automatic removal.
        let home = std::env::var("HOME").unwrap_or_default();
        let mut safe_dirs: Vec<PathBuf> = vec![
            PathBuf::from(&home).join(".local/bin"),
            PathBuf::from(&home).join(".cargo/bin"),
        ];
        // Also check Mason's bin dir if it exists.
        let mason_dir = PathBuf::from(&home).join(".local/share/nvim/mason/bin");
        if mason_dir.is_dir() {
            safe_dirs.push(mason_dir);
        }

        for bin_name in &bins {
            // Remove binary from safe dirs.
            for dir in &safe_dirs {
                let path = dir.join(bin_name);
                if path.exists() && std::fs::remove_file(&path).is_ok() {
                    removed.push(format!("{}", path.display()));
                }
            }
            // Remove associated data dir (e.g. ~/.local/share/lua-language-server/).
            let data_dir = PathBuf::from(&home).join(".local/share").join(bin_name);
            if data_dir.is_dir() {
                let _ = std::fs::remove_dir_all(&data_dir);
            }
        }

        if !removed.is_empty() {
            crate::core::lsp_manager::install_log(&format!(
                "[ext-remove] Removed tools for '{name}': {}",
                removed.join(", ")
            ));
        }
    }

    /// Update a single extension: re-download scripts and update version.
    pub fn ext_update_one(&mut self, name: &str) {
        if !self.extension_state.is_installed(name) {
            self.message = format!("Extension '{name}' is not installed");
            return;
        }
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));
        let manifest = match manifest {
            Some(m) => m,
            None => {
                self.message = format!("Extension '{name}' not found in registry");
                return;
            }
        };

        let ext_name = manifest.name.clone();
        let new_version = manifest.version.clone();

        // Re-download scripts (overwrite existing files)
        let ext_dir = paths::vimcode_config_dir()
            .join("extensions")
            .join(&ext_name);
        let base_url = self.resolve_registry_base_url(&manifest);
        if !manifest.scripts.is_empty()
            && !base_url.is_empty()
            && std::fs::create_dir_all(&ext_dir).is_ok()
        {
            for script in &manifest.scripts {
                let dest = ext_dir.join(script);
                let url = format!("{}/{}/{}", base_url, ext_name, script);
                let _ = registry::download_script(&url, &dest);
            }
        }

        // Check if LSP/DAP install commands need to run (only if binaries missing)
        let mut install_commands: Vec<String> = Vec::new();
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            if all_lsp.iter().copied().all(|b| !binary_on_path(b)) {
                let cmd = manifest.lsp.install_cmd_for_platform();
                if !cmd.is_empty() {
                    install_commands.push(cmd.to_string());
                }
            }
        }
        if !manifest.dap.adapter.is_empty()
            && !manifest.dap.binary.is_empty()
            && !binary_on_path(&manifest.dap.binary)
        {
            let cmd = manifest.dap.install_cmd_for_platform();
            if !cmd.is_empty() {
                install_commands.push(cmd.to_string());
            }
        }

        if !install_commands.is_empty() {
            let combined = install_commands.join(" ; ");
            let header = format!("echo '── Updating {ext_name} ──'");
            self.pending_terminal_command = Some(format!("{header} ; {combined}"));
        }

        // Update version
        self.extension_state
            .mark_installed_version(&ext_name, &new_version);
        let _ = self.extension_state.save();

        // Reload plugins
        self.plugin_manager = None;
        self.plugin_init();

        self.message = if new_version.is_empty() {
            format!("Extension '{ext_name}' updated")
        } else {
            format!("Extension '{ext_name}' updated to v{new_version}")
        };
    }

    /// Update all installed extensions that have newer versions available.
    pub fn ext_update_all(&mut self) {
        let manifests = self.ext_available_manifests();
        let mut updated = Vec::new();
        for manifest in &manifests {
            let installed_ver = self.extension_state.installed_version(&manifest.name);
            if installed_ver.is_empty() && self.extension_state.is_installed(&manifest.name) {
                // No version tracked — always update
                updated.push(manifest.name.clone());
            } else if self.extension_state.is_installed(&manifest.name)
                && !manifest.version.is_empty()
                && manifest.version != installed_ver
            {
                updated.push(manifest.name.clone());
            }
        }
        if updated.is_empty() {
            self.message = "All extensions are up to date".to_string();
            return;
        }
        let count = updated.len();
        for name in &updated {
            // Re-download scripts for each
            if let Some(manifest) = manifests.iter().find(|m| &m.name == name) {
                let ext_dir = paths::vimcode_config_dir().join("extensions").join(name);
                let base_url = self.resolve_registry_base_url(manifest);
                if !manifest.scripts.is_empty()
                    && !base_url.is_empty()
                    && std::fs::create_dir_all(&ext_dir).is_ok()
                {
                    for script in &manifest.scripts {
                        let dest = ext_dir.join(script);
                        let url = format!("{}/{}/{}", base_url, name, script);
                        let _ = registry::download_script(&url, &dest);
                    }
                }
                self.extension_state
                    .mark_installed_version(name, &manifest.version);
            }
        }
        let _ = self.extension_state.save();
        self.plugin_manager = None;
        self.plugin_init();
        self.message = format!("{count} extension(s) updated: {}", updated.join(", "));
    }

    /// Returns true if a newer version is available for the given extension.
    pub fn ext_has_update(&self, name: &str) -> bool {
        if !self.extension_state.is_installed(name) {
            return false;
        }
        let installed_ver = self.extension_state.installed_version(name);
        if let Some(registry) = &self.ext_registry {
            if let Some(manifest) = registry.iter().find(|m| m.name == name) {
                if manifest.version.is_empty() {
                    return false;
                }
                return installed_ver.is_empty() || manifest.version != installed_ver;
            }
        }
        false
    }

    /// Get the LSP status for a specific buffer's language.
    /// Returns `LspStatus::None` if no LSP is configured or the manager isn't started.
    pub fn lsp_status_for_buffer(
        &self,
        buffer_id: crate::core::buffer::BufferId,
    ) -> crate::core::lsp_manager::LspStatus {
        use crate::core::lsp_manager::LspStatus;
        let lang = match self.buffer_manager.get(buffer_id) {
            Some(s) => match s.lsp_language_id.as_deref() {
                Some(l) => l,
                None => return LspStatus::None,
            },
            None => return LspStatus::None,
        };
        if self.lsp_installing.contains(lang) {
            return LspStatus::Installing;
        }
        match &self.lsp_manager {
            Some(mgr) => mgr.lsp_status_for_language(lang),
            None => LspStatus::None,
        }
    }
}
