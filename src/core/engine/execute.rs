use super::*;

impl Engine {
    pub fn execute_command(&mut self, cmd: &str) -> EngineAction {
        // Save for @: repeat (before normalization, using trimmed original)
        let trimmed_cmd = cmd.trim();
        if !trimmed_cmd.is_empty() {
            self.last_ex_command = Some(trimmed_cmd.to_string());
        }

        // Handle :norm[al][!] before trimming — keys may contain significant trailing whitespace
        if let Some((range_str, keys)) = try_parse_norm(cmd.trim_start()) {
            return self.execute_norm_command(range_str, keys);
        }

        let cmd = cmd.trim();
        let normalized = normalize_ex_command(cmd);
        let cmd: &str = &normalized;

        // Handle :term / :terminal — open integrated terminal
        if cmd == "terminal" {
            return EngineAction::OpenTerminal;
        }

        // Handle workspace / folder commands (both user-typed names and menu action strings)
        if cmd == "OpenFolder" || cmd == "open_folder_dialog" {
            return EngineAction::OpenFolderDialog;
        }
        if cmd == "OpenWorkspace" || cmd == "open_workspace_dialog" {
            self.open_workspace_from_file();
            return EngineAction::OpenWorkspaceDialog;
        }
        if cmd == "SaveWorkspaceAs" || cmd == "save_workspace_as_dialog" {
            return EngineAction::SaveWorkspaceAsDialog;
        }
        if let Some(path_str) = cmd.strip_prefix("cd ").map(|s| s.trim()) {
            let path = Path::new(path_str);
            let target = if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.cwd.join(path)
            };
            self.open_folder(&target);
            return EngineAction::None;
        }

        // Handle :DapInfo — show available DAP adapters from installed extensions
        if cmd == "DapInfo" {
            let adapters: Vec<String> = self
                .ext_available_manifests()
                .into_iter()
                .filter(|m| self.extension_state.is_installed(&m.name) && !m.dap.adapter.is_empty())
                .map(|m| format!("{} ({})", m.name, m.dap.adapter))
                .collect();
            if adapters.is_empty() {
                self.message = "No DAP-capable extensions installed — use :ExtInstall".to_string();
            } else {
                self.message = format!("DAP extensions: {}", adapters.join(", "));
            }
            return EngineAction::None;
        }

        // Handle :DapWatch <expr> — add a watch expression to the debug sidebar.
        if let Some(expr) = cmd.strip_prefix("DapWatch").map(|s| s.trim()) {
            if expr.is_empty() {
                self.message = "Usage: :DapWatch <expression>".to_string();
            } else {
                self.dap_add_watch(expr.to_string());
                self.message = format!("Watch added: {expr}");
            }
            return EngineAction::None;
        }

        // Handle :DapBottomPanel terminal|output|close — switch or close the bottom panel tab.
        if let Some(panel_name) = cmd.strip_prefix("DapBottomPanel").map(|s| s.trim()) {
            match panel_name {
                "terminal" => {
                    self.bottom_panel_kind = BottomPanelKind::Terminal;
                    self.message = "Bottom panel: Terminal".to_string();
                }
                "output" => {
                    self.bottom_panel_kind = BottomPanelKind::DebugOutput;
                    self.message = "Bottom panel: Debug Output".to_string();
                }
                "close" => {
                    self.bottom_panel_open = false;
                    self.message = "Bottom panel closed".to_string();
                }
                _ => {
                    self.message = "Usage: :DapBottomPanel terminal|output|close".to_string();
                }
            }
            return EngineAction::None;
        }

        // Handle :DapEval <expr> — evaluate expression in the current frame.
        if let Some(expr) = cmd.strip_prefix("DapEval").map(|s| s.trim()) {
            if expr.is_empty() {
                self.message = "Usage: :DapEval <expression>".to_string();
            } else if self.dap_session_active && self.dap_stopped_thread.is_some() {
                self.dap_eval(expr);
            } else {
                self.message = "DapEval: program must be stopped at a breakpoint".to_string();
            }
            return EngineAction::None;
        }

        // Handle :DapExpand <var_ref> — toggle expansion of a variable node.
        if let Some(ref_str) = cmd.strip_prefix("DapExpand").map(|s| s.trim()) {
            match ref_str.parse::<u64>() {
                Ok(var_ref) if var_ref > 0 => self.dap_toggle_expand_var(var_ref),
                _ => self.message = "Usage: :DapExpand <variablesReference>".to_string(),
            }
            return EngineAction::None;
        }

        // Handle :DapCondition [expr] — set/clear condition on breakpoint at current line.
        if cmd == "DapCondition" || cmd.starts_with("DapCondition ") {
            let condition = cmd
                .strip_prefix("DapCondition")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let file = self
                .active_buffer_state()
                .file_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            let line = (self.view().cursor.line + 1) as u64;
            if let Some(file) = file {
                self.dap_set_breakpoint_condition(&file, line, condition);
            } else {
                self.message = "No file associated with this buffer".to_string();
            }
            return EngineAction::None;
        }

        // Handle :DapHitCondition [expr] — set/clear hit-count condition on breakpoint.
        if cmd == "DapHitCondition" || cmd.starts_with("DapHitCondition ") {
            let hit_cond = cmd
                .strip_prefix("DapHitCondition")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let file = self
                .active_buffer_state()
                .file_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            let line = (self.view().cursor.line + 1) as u64;
            if let Some(file) = file {
                self.dap_set_breakpoint_hit_condition(&file, line, hit_cond);
            } else {
                self.message = "No file associated with this buffer".to_string();
            }
            return EngineAction::None;
        }

        // Handle :DapLogMessage [msg] — set/clear a logpoint on the current line.
        if cmd == "DapLogMessage" || cmd.starts_with("DapLogMessage ") {
            let log_msg = cmd
                .strip_prefix("DapLogMessage")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let file = self
                .active_buffer_state()
                .file_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            let line = (self.view().cursor.line + 1) as u64;
            if let Some(file) = file {
                self.dap_set_breakpoint_log_message(&file, line, log_msg);
            } else {
                self.message = "No file associated with this buffer".to_string();
            }
            return EngineAction::None;
        }

        // Handle :DapInstall <lang> — redirect to extension system
        if let Some(lang_id) = cmd.strip_prefix("DapInstall").map(|s| s.trim()) {
            if lang_id.is_empty() {
                self.message = "Usage: :DapInstall <language>  (e.g. :DapInstall rust)".to_string();
                return EngineAction::None;
            }
            // Validate that a built-in DAP adapter exists for this language (for error message)
            match crate::core::dap_manager::DapManager::adapter_for_language(lang_id) {
                Some(info) => {
                    let adapter_name = info.name;
                    // Look up matching extension by language_id
                    let ext_name = self
                        .ext_available_manifests()
                        .into_iter()
                        .find(|m| {
                            m.language_ids.iter().any(|l| l == lang_id)
                                || m.dap.adapter == adapter_name
                        })
                        .map(|m| m.name.clone());
                    if let Some(name) = ext_name {
                        self.message =
                            format!("Use :ExtInstall {name} instead  (or open Extensions panel)");
                    } else {
                        // Fall back to direct adapter install
                        let dap_key = format!("dap:{adapter_name}");
                        if self.lsp_installing.contains(&dap_key) {
                            self.message = format!("Install already running for {adapter_name}");
                        } else if let Some(cmd_str) =
                            crate::core::dap_manager::install_cmd_for_adapter(
                                adapter_name,
                                &self.ext_available_manifests(),
                            )
                        {
                            self.ensure_lsp_manager();
                            self.lsp_installing.insert(dap_key.clone());
                            if let Some(mgr) = &self.lsp_manager {
                                mgr.run_install_command(&dap_key, &cmd_str);
                            }
                            self.message = format!("Installing {adapter_name}…");
                        } else {
                            self.message = format!("No automated installer for '{adapter_name}'");
                        }
                    }
                }
                None => {
                    self.message = format!(
                        "No built-in DAP adapter for '{lang_id}' (supported: rust, python, go, javascript, typescript, java)"
                    );
                }
            }
            return EngineAction::None;
        }

        // Handle :LspInfo — show running LSP servers (● marks active for current buffer)
        if cmd == "LspInfo" {
            let buf_lang = self
                .buffer_manager
                .get(self.active_buffer_id())
                .and_then(|s| s.lsp_language_id.clone());
            if let Some(mgr) = &self.lsp_manager {
                let servers = mgr.server_info(buf_lang.as_deref());
                self.message = servers.join(" | ");
            } else {
                self.message = "LSP manager not started".to_string();
            }
            return EngineAction::None;
        }

        // Handle :LspDebug — show binary resolution result for current language
        if cmd == "LspDebug" {
            let buf_lang = self
                .buffer_manager
                .get(self.active_buffer_id())
                .and_then(|s| s.lsp_language_id.clone());
            match buf_lang {
                None => {
                    self.message = "LspDebug: buffer has no language ID".to_string();
                }
                Some(lang) => {
                    let manifests = self.ext_available_manifests();
                    self.message = crate::core::lsp_manager::debug_resolve(&lang, &manifests);
                }
            }
            return EngineAction::None;
        }

        // Handle :LspRestart — restart server for current language
        if cmd == "LspRestart" {
            let lang = self
                .buffer_manager
                .get(self.active_buffer_id())
                .and_then(|s| s.lsp_language_id.clone());
            if let Some(lang) = lang {
                if let Some(mgr) = &mut self.lsp_manager {
                    mgr.restart_server_for_language(&lang);
                    self.message = format!("LSP server restarted for {lang}");
                }
            } else {
                self.message = "No LSP language for current buffer".to_string();
            }
            return EngineAction::None;
        }

        // Handle :LspStop — stop server for current language
        if cmd == "LspStop" {
            let lang = self
                .buffer_manager
                .get(self.active_buffer_id())
                .and_then(|s| s.lsp_language_id.clone());
            if let Some(lang) = lang {
                if let Some(mgr) = &mut self.lsp_manager {
                    mgr.stop_server_for_language(&lang);
                    self.message = format!("LSP server stopped for {lang}");
                }
            } else {
                self.message = "No LSP language for current buffer".to_string();
            }
            return EngineAction::None;
        }

        // Handle :LspInstall <language> — redirect to extension system
        if let Some(lang_id) = cmd.strip_prefix("LspInstall").map(|s| s.trim()) {
            if lang_id.is_empty() {
                self.message =
                    "Usage: :LspInstall <language>  (e.g. :LspInstall csharp)".to_string();
                return EngineAction::None;
            }
            // Look up by language_id in the merged manifest list
            let ext_name = self
                .ext_available_manifests()
                .into_iter()
                .find(|m| m.language_ids.iter().any(|l| l == lang_id))
                .map(|m| m.name.clone());
            if let Some(name) = ext_name {
                self.message =
                    format!("Use :ExtInstall {name} instead  (or open Extensions panel)");
            } else {
                self.message =
                    format!("Unknown language '{lang_id}' — try :ExtRefresh then :ExtList");
            }
            return EngineAction::None;
        }

        // Handle :Lformat — LSP format current buffer
        if cmd == "Lformat" {
            self.lsp_format_current();
            return EngineAction::None;
        }

        // Handle :Rename <newname> — LSP rename symbol at cursor
        if let Some(new_name) = cmd.strip_prefix("Rename").map(|s| s.trim()) {
            if new_name.is_empty() {
                // Pre-fill with word under cursor for interactive editing
                let word = self.word_under_cursor().unwrap_or_default();
                self.mode = crate::core::Mode::Command;
                self.command_buffer = format!("Rename {word}");
                self.command_cursor = self.command_buffer.chars().count();
            } else {
                self.lsp_request_rename(new_name);
            }
            return EngineAction::None;
        }

        // Handle :Gdiff / :Gd
        if cmd == "Gdiff" || cmd == "Gd" {
            return self.cmd_git_diff();
        }

        // Handle :Gdiffsplit / :Gds [path]
        if cmd == "Gdiffsplit" || cmd == "Gds" {
            let path = match self.file_path().map(|p| p.to_path_buf()) {
                Some(p) => p,
                None => {
                    self.message = "No file".to_string();
                    return EngineAction::Error;
                }
            };
            return self.cmd_git_diff_split(&path);
        }
        if let Some(path_str) = cmd
            .strip_prefix("Gdiffsplit ")
            .or_else(|| cmd.strip_prefix("Gds "))
        {
            let path = Path::new(path_str.trim());
            let abs_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.cwd.join(path)
            };
            return self.cmd_git_diff_split(&abs_path);
        }

        // Two-way diff commands
        if cmd == "diffthis" {
            return self.cmd_diffthis();
        }
        if cmd == "diffoff" {
            return self.cmd_diffoff();
        }
        if let Some(path_str) = cmd.strip_prefix("diffsplit ") {
            let path = Path::new(path_str.trim());
            return self.cmd_diffsplit(path);
        }
        if cmd == "diffsplit" {
            self.message = "Usage: :diffsplit <file>".to_string();
            return EngineAction::None;
        }
        if cmd == "DiffNext" {
            self.diff_jump_next();
            return EngineAction::None;
        }
        if cmd == "DiffPrev" {
            self.diff_jump_prev();
            return EngineAction::None;
        }
        if cmd == "DiffToggleContext" {
            self.diff_toggle_hide_unchanged();
            return EngineAction::None;
        }

        // Handle :Gstatus / :Gs
        if cmd == "Gstatus" || cmd == "Gs" {
            return self.cmd_git_status();
        }

        // Handle :Gadd[!] — stage current file or all
        if cmd == "Gadd" || cmd == "Ga" {
            return self.cmd_git_add(false);
        }
        if cmd == "Gadd!" || cmd == "Ga!" {
            return self.cmd_git_add(true);
        }

        // Handle :Gcommit <message> / :Gc <message>
        if let Some(msg) = cmd
            .strip_prefix("Gcommit ")
            .or_else(|| cmd.strip_prefix("Gc "))
        {
            return self.cmd_git_commit(msg.trim());
        }
        if cmd == "Gcommit" || cmd == "Gc" {
            self.message = "Usage: Gcommit <message>".to_string();
            return EngineAction::Error;
        }

        // Handle :Gpush / :Gp
        if cmd == "Gpush" || cmd == "Gp" {
            return self.cmd_git_push();
        }

        // Handle :Gblame / :Gb
        if cmd == "Gblame" || cmd == "Gb" {
            return self.cmd_git_blame();
        }

        // Handle :Ghs / :Ghunk — stage hunk under cursor
        if cmd == "Ghs" || cmd == "Ghunk" {
            return self.cmd_git_stage_hunk();
        }

        // Handle :DiffPeek — open inline diff peek popup
        if cmd == "DiffPeek" {
            self.open_diff_peek();
            return EngineAction::None;
        }

        if cmd == "ToggleBlame" || cmd == "Gib" {
            self.toggle_inline_blame();
            return EngineAction::None;
        }

        // Handle :GWorktreeAdd <branch> <path>
        if let Some(rest) = cmd.strip_prefix("GWorktreeAdd ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let dir = self.cwd.clone();
                match git::worktree_add(&dir, parts[1].trim(), parts[0].trim()) {
                    Ok(()) => {
                        self.message = format!("Created worktree at {}", parts[1].trim());
                        self.sc_refresh();
                    }
                    Err(e) => self.message = format!("GWorktreeAdd: {}", e),
                }
            } else {
                self.message = "Usage: GWorktreeAdd <branch> <path>".to_string();
            }
            return EngineAction::None;
        }

        // Handle :GWorktreeRemove <path>
        if let Some(rest) = cmd.strip_prefix("GWorktreeRemove ") {
            let dir = self.cwd.clone();
            match git::worktree_remove(&dir, rest.trim()) {
                Ok(()) => {
                    self.message = format!("Removed worktree at {}", rest.trim());
                    self.sc_refresh();
                }
                Err(e) => self.message = format!("GWorktreeRemove: {}", e),
            }
            return EngineAction::None;
        }

        // Handle :Explore / :Ex — netrw-style in-buffer file browser
        if cmd == "Explore" || cmd == "Ex" {
            return self.cmd_explore(None, None);
        }
        if let Some(arg) = cmd
            .strip_prefix("Explore ")
            .or_else(|| cmd.strip_prefix("Ex "))
        {
            return self.cmd_explore(Some(arg.trim()), None);
        }
        // Handle :Sexplore / :Sex — horizontal split + netrw
        if cmd == "Sexplore" || cmd == "Sex" {
            return self.cmd_explore(None, Some(SplitDirection::Horizontal));
        }
        if let Some(arg) = cmd
            .strip_prefix("Sexplore ")
            .or_else(|| cmd.strip_prefix("Sex "))
        {
            return self.cmd_explore(Some(arg.trim()), Some(SplitDirection::Horizontal));
        }
        // Handle :Vexplore / :Vex — vertical split + netrw
        if cmd == "Vexplore" || cmd == "Vex" {
            return self.cmd_explore(None, Some(SplitDirection::Vertical));
        }
        if let Some(arg) = cmd
            .strip_prefix("Vexplore ")
            .or_else(|| cmd.strip_prefix("Vex "))
        {
            return self.cmd_explore(Some(arg.trim()), Some(SplitDirection::Vertical));
        }

        // Handle :Gpull — git pull
        if cmd == "Gpull" {
            self.sc_pull();
            return EngineAction::None;
        }

        // Handle :Gfetch — git fetch
        if cmd == "Gfetch" {
            self.sc_fetch();
            return EngineAction::None;
        }

        // Handle :Gswitch <branch> / :Gbranch <name> — branch operations
        if let Some(branch) = cmd
            .strip_prefix("Gswitch ")
            .or_else(|| cmd.strip_prefix("GSwitch "))
            .or_else(|| cmd.strip_prefix("Gsw "))
        {
            let branch = branch.trim();
            let root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
            match git::checkout_branch(&root, branch) {
                Ok(()) => {
                    self.message = format!("Switched to {branch}");
                    self.sc_refresh();
                }
                Err(e) => self.message = format!("Switch failed: {e}"),
            }
            return EngineAction::None;
        }
        if let Some(branch) = cmd
            .strip_prefix("Gbranch ")
            .or_else(|| cmd.strip_prefix("GBranch "))
            .or_else(|| cmd.strip_prefix("Gb "))
        {
            let branch = branch.trim();
            let root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
            match git::create_branch(&root, branch) {
                Ok(()) => {
                    self.message = format!("Created and switched to {branch}");
                    self.sc_refresh();
                }
                Err(e) => self.message = format!("Create branch failed: {e}"),
            }
            return EngineAction::None;
        }

        // Handle :Gbranches — open branch picker
        if cmd == "Gbranches" || cmd == "GBranches" {
            self.open_picker(PickerSource::GitBranches);
            return EngineAction::None;
        }

        // Handle :Plugin list|reload|enable|disable
        if let Some(subcmd) = cmd.strip_prefix("Plugin").map(|s| s.trim()) {
            match subcmd {
                "list" => {
                    if let Some(ref pm) = self.plugin_manager {
                        if pm.plugins.is_empty() {
                            self.message = "No plugins loaded".to_string();
                        } else {
                            let summary: Vec<String> = pm
                                .plugins
                                .iter()
                                .map(|p| {
                                    let status = if !p.enabled {
                                        "disabled"
                                    } else if p.error.is_some() {
                                        "error"
                                    } else {
                                        "ok"
                                    };
                                    format!("{} [{}]", p.name, status)
                                })
                                .collect();
                            self.message = summary.join(", ");
                        }
                    } else {
                        self.message = "Plugin system not initialized".to_string();
                    }
                }
                "reload" => {
                    self.plugin_manager = None;
                    self.plugin_init();
                    self.message = "Plugins reloaded".to_string();
                }
                s if s.starts_with("enable ") => {
                    let name = s.trim_start_matches("enable ").trim().to_string();
                    self.settings.disabled_plugins.retain(|n| n != &name);
                    let _ = self.settings.save();
                    self.plugin_manager = None;
                    self.plugin_init();
                    self.message = format!("Plugin enabled: {name}");
                }
                s if s.starts_with("disable ") => {
                    let name = s.trim_start_matches("disable ").trim().to_string();
                    if !self.settings.disabled_plugins.contains(&name) {
                        self.settings.disabled_plugins.push(name.clone());
                        let _ = self.settings.save();
                    }
                    self.plugin_manager = None;
                    self.plugin_init();
                    self.message = format!("Plugin disabled: {name}");
                }
                _ => {
                    self.message =
                        "Usage: :Plugin list|reload|enable <name>|disable <name>".to_string();
                }
            }
            return EngineAction::None;
        }

        // ── :map / :unmap — user-defined key mappings ────────────────────────────
        if cmd == "map" {
            // :map — list all user keymaps
            if self.settings.keymaps.is_empty() {
                self.message = "No user keymaps defined".to_string();
            } else {
                self.message = self.settings.keymaps.join("  |  ");
            }
            return EngineAction::None;
        }
        if let Some(rest) = cmd.strip_prefix("map ") {
            let rest = rest.trim();
            // :map n <C-/> :Commentary → add keymap
            if parse_keymap_def(rest).is_some() {
                let entry = rest.to_string();
                if !self.settings.keymaps.contains(&entry) {
                    self.settings.keymaps.push(entry.clone());
                    let _ = self.settings.save();
                    self.rebuild_user_keymaps();
                }
                self.message = format!("Mapped: {entry}");
            } else {
                self.message =
                    "Usage: :map <mode> <keys> :<command>  (e.g. :map n <C-/> :Commentary)"
                        .to_string();
            }
            return EngineAction::None;
        }
        if cmd == "unmap" {
            self.message = "Usage: :unmap <mode> <keys>  (e.g. :unmap n <C-/>)".to_string();
            return EngineAction::None;
        }
        if let Some(rest) = cmd.strip_prefix("unmap ") {
            let rest = rest.trim();
            // Parse "n <C-/>" → find and remove matching keymap
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let mode = parts[0];
                let keys = parts[1];
                let prefix = format!("{mode} {keys} ");
                let before = self.settings.keymaps.len();
                self.settings.keymaps.retain(|s| !s.starts_with(&prefix));
                if self.settings.keymaps.len() < before {
                    let _ = self.settings.save();
                    self.rebuild_user_keymaps();
                    self.message = format!("Unmapped: {mode} {keys}");
                } else {
                    self.message = format!("No mapping found for: {mode} {keys}");
                }
            } else {
                self.message = "Usage: :unmap <mode> <keys>  (e.g. :unmap n <C-/>)".to_string();
            }
            return EngineAction::None;
        }

        // ── Extension commands (:ExtInstall / :ExtRemove / :ExtRefresh / :ExtList /
        //                        :ExtEnable / :ExtDisable) ──────────────────────────────
        if let Some(subcmd) = cmd.strip_prefix("Ext").map(|s| s.trim()) {
            if let Some(name) = subcmd.strip_prefix("Install").map(|s| s.trim()) {
                // :ExtInstall <name>
                if name.is_empty() {
                    self.message =
                        "Usage: :ExtInstall <name>  (e.g. :ExtInstall csharp)".to_string();
                    return EngineAction::None;
                }
                self.ext_install_from_registry(name);
                if let Some(cmd) = self.pending_terminal_command.take() {
                    return EngineAction::RunInTerminal(cmd);
                }
                return EngineAction::None;
            }

            if let Some(name) = subcmd.strip_prefix("Remove").map(|s| s.trim()) {
                // :ExtRemove <name>
                if name.is_empty() {
                    self.message = "Usage: :ExtRemove <name>  (e.g. :ExtRemove csharp)".to_string();
                    return EngineAction::None;
                }
                self.ext_show_remove_dialog(name);
                return EngineAction::None;
            }

            if subcmd.eq_ignore_ascii_case("Refresh") {
                // :ExtRefresh — fetch the remote registry
                self.ext_refresh();
                return EngineAction::None;
            }

            if let Some(name) = subcmd.strip_prefix("Update").map(|s| s.trim()) {
                // :ExtUpdate [name] — update one or all extensions
                if name.is_empty() {
                    // Update all installed extensions
                    self.ext_update_all();
                } else {
                    self.ext_update_one(name);
                }
                if let Some(cmd) = self.pending_terminal_command.take() {
                    return EngineAction::RunInTerminal(cmd);
                }
                return EngineAction::None;
            }

            if let Some(name) = subcmd.strip_prefix("Enable").map(|s| s.trim()) {
                // :ExtEnable <name>
                if name.is_empty() {
                    self.message = "Usage: :ExtEnable <name>".to_string();
                    return EngineAction::None;
                }
                self.extension_state.dismissed.retain(|n| n != name);
                let _ = self.extension_state.save();
                // Remove from disabled_plugins so plugin_init() will load its scripts.
                self.settings
                    .disabled_plugins
                    .retain(|n| n.as_str() != name);
                let _ = self.settings.save();
                // Reload plugin manager so the extension's hooks become active immediately.
                self.plugin_manager = None;
                self.plugin_init();
                self.message = format!("Extension '{name}' enabled");
                return EngineAction::None;
            }

            if let Some(name) = subcmd.strip_prefix("Disable").map(|s| s.trim()) {
                // :ExtDisable <name>
                if name.is_empty() {
                    self.message = "Usage: :ExtDisable <name>".to_string();
                    return EngineAction::None;
                }
                self.extension_state.mark_dismissed(name);
                let _ = self.extension_state.save();
                // Add to disabled_plugins so plugin_init() skips loading its scripts.
                if !self
                    .settings
                    .disabled_plugins
                    .iter()
                    .any(|n| n.as_str() == name)
                {
                    self.settings.disabled_plugins.push(name.to_string());
                    let _ = self.settings.save();
                }
                // Reload plugin manager so the extension's hooks are unregistered immediately.
                self.plugin_manager = None;
                self.plugin_init();
                self.message = format!("Extension '{name}' disabled");
                return EngineAction::None;
            }

            if subcmd == "List" || subcmd == "list" {
                // :ExtList — show all available extensions and install status
                let lines: Vec<String> = self
                    .ext_available_manifests()
                    .iter()
                    .map(|m| {
                        let status = if self.extension_state.is_installed(&m.name) {
                            "installed"
                        } else if self.extension_state.is_dismissed(&m.name) {
                            "dismissed"
                        } else {
                            "available"
                        };
                        format!("{} [{}]", m.name, status)
                    })
                    .collect();
                self.message = lines.join(", ");
                return EngineAction::None;
            }

            self.message =
                "Usage: :ExtInstall <name> | :ExtRemove <name> | :ExtRefresh | :ExtList | :ExtEnable <name> | :ExtDisable <name>"
                    .to_string();
            return EngineAction::None;
        }

        // :AI <message> — send a message to the AI assistant
        if let Some(msg) = cmd.strip_prefix("AI ").map(|s| s.trim()) {
            if !msg.is_empty() {
                self.ai_input = msg.to_string();
                self.ai_send_message();
                self.ai_has_focus = true;
            }
            return EngineAction::None;
        }
        if cmd == "AiClear" || cmd == "AIclear" {
            self.ai_clear();
            return EngineAction::None;
        }

        // Handle :e[dit]! — reload current file from disk (discard changes)
        if cmd == "edit!" {
            let buf_id = self.active_buffer_id();
            let state = self.buffer_manager.get_mut(buf_id).unwrap();
            match state.reload_from_disk() {
                Ok(()) => {
                    let name = state.display_name();
                    self.message = format!("\"{}\" reloaded", name);
                }
                Err(e) => {
                    self.message = format!("Error: {}", e);
                    return EngineAction::Error;
                }
            }
            return EngineAction::None;
        }
        if let Some(filename) = cmd.strip_prefix("edit! ") {
            let filename = filename.trim();
            if filename.is_empty() {
                self.message = "No file name".to_string();
                return EngineAction::Error;
            }
            return EngineAction::OpenFile(PathBuf::from(filename));
        }

        // Handle :e[dit] <filename>
        if let Some(filename) = cmd.strip_prefix("edit ") {
            let filename = filename.trim();
            if filename.is_empty() {
                self.message = "No file name".to_string();
                return EngineAction::Error;
            }
            return EngineAction::OpenFile(PathBuf::from(filename));
        }

        // Handle :b[uffer] <buffer>
        if let Some(arg) = cmd.strip_prefix("buffer ") {
            let arg = arg.trim();
            if let Ok(num) = arg.parse::<usize>() {
                self.goto_buffer(num);
            } else if let Some(id) = self.buffer_manager.find_by_path(arg) {
                let current = self.active_buffer_id();
                if id != current {
                    self.buffer_manager.alternate_buffer = Some(current);
                    self.switch_window_buffer(id);
                }
            } else {
                self.message = format!("No matching buffer for {}", arg);
            }
            return EngineAction::None;
        }

        // Handle :bd[elete][!] [N]
        if cmd == "bdelete"
            || cmd.starts_with("bdelete ")
            || cmd == "bdelete!"
            || cmd.starts_with("bdelete! ")
        {
            let force = cmd.contains('!');
            let arg = cmd
                .trim_start_matches("bdelete")
                .trim_start_matches('!')
                .trim();

            let id = if arg.is_empty() {
                self.active_buffer_id()
            } else if let Ok(num) = arg.parse::<usize>() {
                if let Some(id) = self.buffer_manager.get_by_number(num) {
                    id
                } else {
                    self.message = format!("Buffer {} does not exist", num);
                    return EngineAction::Error;
                }
            } else {
                self.message = format!("Invalid buffer: {}", arg);
                return EngineAction::Error;
            };

            match self.delete_buffer(id, force) {
                Ok(()) => {
                    self.message = "Buffer deleted".to_string();
                }
                Err(e) => {
                    self.message = e;
                    return EngineAction::Error;
                }
            }
            return EngineAction::None;
        }

        // Handle :sp[lit] [file]
        if cmd == "split" || cmd.starts_with("split ") {
            let file = cmd
                .strip_prefix("split")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            self.split_window(SplitDirection::Horizontal, file.map(Path::new));
            return EngineAction::None;
        }

        // Handle :vs[plit] [file]
        if cmd == "vsplit" || cmd.starts_with("vsplit ") {
            let file = cmd
                .strip_prefix("vsplit")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            self.split_window(SplitDirection::Vertical, file.map(Path::new));
            return EngineAction::None;
        }

        // Handle :clo[se]
        if cmd == "close" {
            self.close_window();
            return EngineAction::None;
        }

        // Handle :on[ly]
        if cmd == "only" {
            self.close_other_windows();
            return EngineAction::None;
        }

        // Handle :winc[md] {char} [count]
        if cmd == "wincmd" || cmd.starts_with("wincmd ") {
            let args = cmd.strip_prefix("wincmd").unwrap().trim();
            if args.is_empty() {
                self.message = "E471: Argument required".to_string();
                return EngineAction::None;
            }
            let mut chars = args.chars();
            let ch = chars.next().unwrap();
            let rest = chars.as_str().trim();
            let count = if rest.is_empty() {
                1
            } else {
                rest.parse::<usize>().unwrap_or(1).max(1)
            };
            return self.execute_wincmd(ch, count);
        }

        // Handle :tabnew / :tabedit [file]
        if cmd == "tabnew"
            || cmd == "tabe"
            || cmd.starts_with("tabnew ")
            || cmd.starts_with("tabe ")
        {
            let file = cmd
                .strip_prefix("tabnew")
                .or_else(|| cmd.strip_prefix("tabe"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            self.new_tab(file.map(Path::new));
            return EngineAction::None;
        }

        // Handle :tabc[lose]
        if cmd == "tabclose" || cmd.starts_with("tabclose ") {
            let arg = cmd.strip_prefix("tabclose").unwrap().trim();
            match arg {
                "others" => self.close_other_tabs(),
                "right" => self.close_tabs_to_right(),
                "left" => self.close_tabs_to_left(),
                "saved" => self.close_saved_tabs(),
                _ => {
                    self.close_tab();
                }
            }
            return EngineAction::None;
        }

        // Handle :tabn[ext]
        if cmd == "tabnext" {
            self.next_tab();
            return EngineAction::None;
        }

        // Handle :tabp[revious]
        if cmd == "tabprevious" {
            self.prev_tab();
            return EngineAction::None;
        }

        // Handle :TabSwitcher / :tabs — open MRU tab switcher popup
        if cmd == "TabSwitcher" || cmd == "tabswitcher" || cmd == "tabs" {
            self.open_tab_switcher();
            return EngineAction::None;
        }

        // Handle :set [option]
        if cmd == "set" {
            self.message = self.settings.display_all();
            return EngineAction::None;
        }
        if let Some(args) = cmd.strip_prefix("set ") {
            let trimmed = args.trim();

            // Handle :set filetype=<lang> / :set ft=<lang> — per-buffer language override
            let ft_val = trimmed
                .strip_prefix("filetype=")
                .or_else(|| trimmed.strip_prefix("ft="));
            if let Some(lang) = ft_val {
                let lang = lang.trim().to_string();
                if lang.is_empty() {
                    self.message = "filetype: value required".to_string();
                    return EngineAction::Error;
                }
                let buf_id = self.active_buffer_id();
                // Update buffer's language ID
                if let Some(state) = self.buffer_manager.get_mut(buf_id) {
                    state.lsp_language_id = Some(lang.clone());
                    // Persist to settings.language_map if buffer has a file extension
                    if let Some(ext) = state
                        .file_path
                        .as_ref()
                        .and_then(|p| p.extension())
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_string())
                    {
                        self.settings.language_map.insert(ext, lang.clone());
                        let _ = self.settings.save();
                    }
                }
                self.message = format!("filetype={lang}");
                return EngineAction::None;
            }

            // Handle :set filetype? / :set ft? — query current filetype
            if trimmed == "filetype?" || trimmed == "ft?" {
                let ft = self
                    .buffer_manager
                    .get(self.active_buffer_id())
                    .and_then(|s| s.lsp_language_id.clone())
                    .unwrap_or_else(|| "none".to_string());
                self.message = format!("filetype={ft}");
                return EngineAction::None;
            }

            match self.settings.parse_set_option(trimmed) {
                Ok(msg) => {
                    if let Err(e) = self.settings.save() {
                        self.message = format!("Setting changed but failed to save: {e}");
                    } else {
                        self.message = msg;
                    }
                }
                Err(e) => {
                    self.message = e;
                    return EngineAction::Error;
                }
            }
            // Lazy-init spell checker when spell is enabled
            if self.settings.spell {
                self.ensure_spell_checker();
            }
            return EngineAction::None;
        }

        // Handle :colorscheme [name]
        if cmd == "colorscheme" {
            let mut names: Vec<&str> = vec![
                "onedark",
                "gruvbox-dark",
                "tokyo-night",
                "solarized-dark",
                "vscode-dark",
                "vscode-light",
            ];
            let custom = list_custom_theme_names();
            let custom_strs: Vec<&str> = custom.iter().map(|s| s.as_str()).collect();
            names.extend(custom_strs);
            self.message = format!("Available themes: {}", names.join(", "));
            return EngineAction::None;
        }
        if let Some(name) = cmd.strip_prefix("colorscheme ") {
            let name = name.trim();
            // Normalize built-in aliases
            let canonical = match name {
                "gruvbox" => "gruvbox-dark",
                "tokyonight" => "tokyo-night",
                "solarized" => "solarized-dark",
                "vscode" | "dark+" => "vscode-dark",
                "light+" => "vscode-light",
                other => other,
            };
            // Verify the theme exists (built-in or custom VSCode JSON)
            let builtin = [
                "onedark",
                "gruvbox-dark",
                "tokyo-night",
                "solarized-dark",
                "vscode-dark",
                "vscode-light",
            ];
            let custom = list_custom_theme_names();
            let is_valid = builtin.contains(&canonical) || custom.iter().any(|n| n == canonical);
            if is_valid {
                self.settings.colorscheme = canonical.to_string();
                if let Err(e) = self.settings.save() {
                    self.message = format!("Theme set to '{canonical}' (save failed: {e})");
                } else {
                    self.message = format!("Theme: {canonical}");
                }
            } else {
                let mut available: Vec<String> = builtin.iter().map(|s| s.to_string()).collect();
                available.extend(custom);
                self.message = format!(
                    "Unknown theme '{name}'. Available: {}",
                    available.join(", ")
                );
                return EngineAction::Error;
            }
            return EngineAction::None;
        }

        // Handle :config reload
        // Handle :Settings — open settings.json in a new tab
        if cmd == "Settings" || cmd == "settings" {
            let path = Settings::settings_file_path();
            self.open_file_in_tab(&path);
            return EngineAction::None;
        }

        // Handle :Keymaps — open keymaps editor scratch buffer
        if cmd == "Keymaps" || cmd == "keymaps" {
            self.open_keymaps_editor();
            return EngineAction::None;
        }

        // Handle :Keybindings [vim|vscode] — open read-only keybinding reference
        if let Some(rest) = cmd
            .strip_prefix("Keybindings")
            .or_else(|| cmd.strip_prefix("keybindings"))
        {
            let arg = rest.trim();
            let force_vscode = match arg {
                "vim" => Some(false),
                "vscode" => Some(true),
                "" => None, // auto-detect from current mode
                _ => {
                    self.message = "Usage: :Keybindings [vim|vscode]".to_string();
                    return EngineAction::None;
                }
            };
            self.open_keybindings_reference_for(force_vscode);
            return EngineAction::None;
        }

        if cmd == "config reload" {
            match Settings::load_with_validation() {
                Ok(new_settings) => {
                    self.settings = new_settings;
                    self.message = "Settings reloaded successfully".to_string();
                }
                Err(e) => {
                    // Preserve current settings on error
                    self.message = format!("Error reloading settings: {}", e);
                }
            }
            return EngineAction::None;
        }

        // Handle :ls / :buffers / :files
        if cmd == "ls" || cmd == "buffers" || cmd == "files" {
            self.message = self.list_buffers();
            return EngineAction::None;
        }

        // Handle :bn[ext]
        if cmd == "bnext" {
            self.next_buffer();
            return EngineAction::None;
        }

        // Handle :bp[revious]
        if cmd == "bprevious" {
            self.prev_buffer();
            return EngineAction::None;
        }

        // Handle :buffer# (alternate buffer) — normalizer turns b# → buffer#
        if cmd == "buffer#" {
            self.alternate_buffer();
            return EngineAction::None;
        }

        // Quickfix commands
        if cmd == "copen" {
            return self.open_quickfix();
        }
        if cmd == "cclose" {
            return self.close_quickfix();
        }
        if cmd == "cnext" {
            return self.quickfix_next();
        }
        if cmd == "cprevious" || cmd == "cN" {
            return self.quickfix_prev();
        }
        if let Some(n_str) = cmd.strip_prefix("cc ") {
            if let Some(n) = n_str.trim().parse::<usize>().ok().filter(|&n| n > 0) {
                return self.quickfix_go(n - 1);
            }
        }
        if let Some(pat) = cmd
            .strip_prefix("grep ")
            .or_else(|| cmd.strip_prefix("vimgrep "))
        {
            let cwd = self.cwd.clone();
            return self.run_quickfix_grep(pat.trim(), cwd);
        }
        if cmd == "grep" || cmd == "vimgrep" {
            self.message = "Usage: :grep <pattern>".to_string();
            return EngineAction::None;
        }
        if cmd == "Buffers" {
            self.open_picker(PickerSource::Buffers);
            return EngineAction::None;
        }
        if cmd == "search_keybindings" {
            self.open_picker(PickerSource::Keybindings);
            return EngineAction::None;
        }
        if cmd == "document_outline" {
            self.open_picker(PickerSource::CommandCenter);
            self.picker_query = "@".to_string();
            self.picker_filter();
            self.picker_load_preview();
            return EngineAction::None;
        }
        if cmd == "GrepWord" {
            let word = self.word_under_cursor().unwrap_or_default();
            if word.is_empty() {
                self.message = "No word under cursor".to_string();
            } else {
                self.open_picker(PickerSource::Grep);
                self.picker_query = word;
                self.picker_filter();
                self.picker_load_preview();
            }
            return EngineAction::None;
        }

        // Handle :h[elp] [topic]
        if cmd == "help" {
            return self.cmd_help("");
        }
        if let Some(topic) = cmd.strip_prefix("help ") {
            return self.cmd_help(topic.trim());
        }

        // Substitute command: :s/pattern/replacement/flags or :%s/...
        if cmd.starts_with("s/") || cmd.starts_with("%s/") || cmd.starts_with("'<,'>s/") {
            return self.execute_substitute_command(cmd);
        }

        // :g/pat/cmd — global command (run ex cmd on lines matching pattern)
        if let Some(rest) = cmd.strip_prefix("g/") {
            return self.execute_global_command(rest, false);
        }
        // :v/pat/cmd — inverse global (run ex cmd on lines NOT matching pattern)
        if let Some(rest) = cmd.strip_prefix("v/") {
            return self.execute_global_command(rest, true);
        }

        // :sort [flags] — sort lines in buffer
        if cmd == "sort" || cmd.starts_with("sort ") {
            let flags = cmd.strip_prefix("sort").unwrap_or("").trim();
            return self.execute_sort_command(flags);
        }

        // :m[ove] {dest} — move current line to after dest
        if let Some(dest) = cmd.strip_prefix("move ") {
            return self.execute_move_command(dest.trim());
        }

        // :t {dest} / :co[py] {dest} — copy current line to after dest
        if let Some(dest) = cmd.strip_prefix("copy ").or_else(|| cmd.strip_prefix("t ")) {
            return self.execute_copy_command(dest.trim());
        }

        // Handle range filter: N,M!cmd — pipe lines through external command
        if let Some(filter_result) = self.try_execute_filter_command(cmd) {
            return filter_result;
        }

        // Handle :! {command} — run a shell command and show output
        if let Some(shell_cmd_raw) = cmd.strip_prefix('!') {
            let shell_cmd = shell_cmd_raw.trim();
            if shell_cmd.is_empty() {
                self.message = "Usage: :!command".to_string();
                return EngineAction::None;
            }
            match std::process::Command::new("sh")
                .arg("-c")
                .arg(shell_cmd)
                .output()
            {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = if stderr.is_empty() {
                        stdout.to_string()
                    } else if stdout.is_empty() {
                        stderr.to_string()
                    } else {
                        format!("{}{}", stdout, stderr)
                    };
                    // Show first line in message, rest truncated
                    let first_line = combined.lines().next().unwrap_or("(no output)");
                    let total_lines = combined.lines().count();
                    if total_lines > 1 {
                        self.message = format!("{} ({} lines)", first_line, total_lines);
                    } else {
                        self.message = first_line.to_string();
                    }
                }
                Err(e) => {
                    self.message = format!("Shell error: {}", e);
                }
            }
            return EngineAction::None;
        }

        // Handle :r[ead] {file} — read file and insert after cursor line
        if let Some(file_arg) = cmd.strip_prefix("read ").map(|s| s.trim()) {
            let path = if Path::new(file_arg).is_absolute() {
                PathBuf::from(file_arg)
            } else {
                self.cwd.join(file_arg)
            };
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let line = self.view().cursor.line;
                    let num_lines = self.buffer().len_lines();
                    let insert_pos = if line + 1 < num_lines {
                        self.buffer().line_to_char(line + 1)
                    } else {
                        let end = self.buffer().len_chars();
                        // Ensure there's a newline before inserting
                        if end > 0 && self.buffer().content.char(end - 1) != '\n' {
                            self.start_undo_group();
                            self.insert_with_undo(end, "\n");
                            self.insert_with_undo(end + 1, &content);
                            self.finish_undo_group();
                            let inserted_lines = content.lines().count();
                            self.message = format!("{} line(s) read", inserted_lines);
                            return EngineAction::None;
                        }
                        end
                    };
                    let inserted_lines = content.lines().count();
                    self.start_undo_group();
                    self.insert_with_undo(insert_pos, &content);
                    self.finish_undo_group();
                    self.message = format!("{} line(s) read", inserted_lines);
                }
                Err(e) => {
                    self.message = format!("Cannot read file: {}", e);
                }
            }
            return EngineAction::None;
        }

        // Handle :echo {text}
        if let Some(text) = cmd.strip_prefix("echo ") {
            self.message = text.trim().trim_matches('"').trim_matches('\'').to_string();
            return EngineAction::None;
        }
        if cmd == "echo" {
            self.message = String::new();
            return EngineAction::None;
        }

        // Handle :tabmove [N] — move current tab to position N (1-based, 0 = move to end)
        if cmd == "tabmove" || cmd.starts_with("tabmove ") {
            let arg = cmd.strip_prefix("tabmove").unwrap_or("").trim();
            let num_tabs = self.active_group().tabs.len();
            let current = self.active_group().active_tab;
            let dest = if arg.is_empty() {
                num_tabs.saturating_sub(1) // move to end
            } else if let Ok(n) = arg.parse::<usize>() {
                if n == 0 {
                    num_tabs.saturating_sub(1) // 0 also means end
                } else {
                    (n - 1).min(num_tabs.saturating_sub(1)) // 1-based to 0-based
                }
            } else {
                self.message = "Usage: :tabmove [N]".to_string();
                return EngineAction::None;
            };
            if current != dest && dest < num_tabs {
                let tab = self.active_group_mut().tabs.remove(current);
                self.active_group_mut().tabs.insert(dest, tab);
                self.active_group_mut().active_tab = dest;
                self.ensure_active_tab_visible();
                self.message = format!("Tab moved to position {}", dest + 1);
            }
            return EngineAction::None;
        }

        if cmd == "navback" {
            self.tab_nav_back();
            return EngineAction::None;
        }
        if cmd == "navforward" {
            self.tab_nav_forward();
            return EngineAction::None;
        }

        // Handle :sav[eas] {file} — save buffer to a new file
        if let Some(path_str) = cmd.strip_prefix("saveas ") {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                self.message = "Usage: :saveas {file}".to_string();
                return EngineAction::Error;
            }
            let path = if Path::new(path_str).is_absolute() {
                PathBuf::from(path_str)
            } else {
                self.cwd.join(path_str)
            };
            self.buffer_manager
                .get_mut(self.active_buffer_id())
                .unwrap()
                .file_path = Some(path);
            let _ = self.save_with_format(false);
            return EngineAction::None;
        }

        // Handle :ma[rk] {a-zA-Z} — set mark at cursor
        if let Some(arg) = cmd.strip_prefix("mark ") {
            let arg = arg.trim();
            if let Some(ch) = arg.chars().next() {
                if arg.len() == 1 && ch.is_ascii_alphabetic() {
                    let cursor = self.view().cursor;
                    if ch.is_ascii_lowercase() {
                        let buf_id = self.active_buffer_id();
                        self.marks.entry(buf_id).or_default().insert(ch, cursor);
                    } else {
                        let path = self.file_path().map(|p| p.to_path_buf());
                        self.global_marks
                            .insert(ch, (path, cursor.line, cursor.col));
                    }
                    self.message = format!("Mark '{ch}' set");
                    return EngineAction::None;
                }
            }
            self.message = "Usage: :mark {a-zA-Z}".to_string();
            return EngineAction::Error;
        }

        // Handle :k{a-zA-Z} — shorthand for :mark (non-alphabetic prefix: normalizer skips it)
        if cmd.starts_with('k') && cmd.len() == 2 {
            let ch = cmd.as_bytes()[1] as char;
            if ch.is_ascii_alphabetic() {
                return self.execute_command(&format!("mark {ch}"));
            }
        }

        // Handle :> — shift right
        if cmd == ">" {
            let line = self.view().cursor.line;
            let mut changed = false;
            self.indent_lines(line, 1, &mut changed);
            return EngineAction::None;
        }

        // Handle :< — shift left
        if cmd == "<" {
            let line = self.view().cursor.line;
            let mut changed = false;
            self.dedent_lines(line, 1, &mut changed);
            return EngineAction::None;
        }

        // Handle := — print line count
        if cmd == "=" {
            let count = self.buffer().len_lines();
            self.message = format!("{count}");
            return EngineAction::None;
        }

        // Handle :# — print current line with line number (alias for :number)
        if cmd == "#" {
            return self.execute_command("number");
        }

        // Handle :windo {cmd}
        if let Some(subcmd) = cmd.strip_prefix("windo ") {
            let subcmd = subcmd.trim().to_string();
            let win_ids: Vec<WindowId> = self.windows.keys().copied().collect();
            for wid in win_ids {
                self.active_tab_mut().active_window = wid;
                self.execute_command(&subcmd);
            }
            return EngineAction::None;
        }

        // Handle :bufdo {cmd}
        if let Some(subcmd) = cmd.strip_prefix("bufdo ") {
            let subcmd = subcmd.trim().to_string();
            let buf_ids: Vec<_> = self.buffer_manager.list();
            for bid in buf_ids {
                self.switch_window_buffer(bid);
                self.execute_command(&subcmd);
            }
            return EngineAction::None;
        }

        // Handle :tabdo {cmd}
        if let Some(subcmd) = cmd.strip_prefix("tabdo ") {
            let subcmd = subcmd.trim().to_string();
            let num_tabs = self.active_group().tabs.len();
            for i in 0..num_tabs {
                self.active_group_mut().active_tab = i;
                self.execute_command(&subcmd);
            }
            return EngineAction::None;
        }

        // Handle :make [args] — run build command
        if cmd == "make" || cmd.starts_with("make ") {
            let args = cmd.strip_prefix("make").unwrap_or("").trim();
            let shell_cmd = if args.is_empty() {
                "make".to_string()
            } else {
                format!("make {}", args)
            };
            return self.execute_command(&format!("!{}", shell_cmd));
        }

        // Handle :$ (jump to last line), :+N, :-N, :. (current line)
        if matches!(cmd, "$" | "." | "0") || cmd.starts_with('+') || cmd.starts_with('-') {
            let current = self.view().cursor.line;
            let total = self.buffer().len_lines();
            let target = self.parse_line_address(cmd, current, total);
            self.view_mut().cursor.line = target;
            self.view_mut().cursor.col = 0;
            self.clamp_cursor_col();
            self.ensure_cursor_visible();
            return EngineAction::None;
        }

        // Handle :N (jump to line number)
        if let Ok(line_num) = cmd.parse::<usize>() {
            let target = if line_num > 0 { line_num - 1 } else { 0 };
            let max = self.buffer().len_lines().saturating_sub(1);
            self.view_mut().cursor.line = target.min(max);
            self.view_mut().cursor.col = 0;
            self.clamp_cursor_col();
            self.ensure_cursor_visible();
            return EngineAction::None;
        }

        match cmd {
            "write" => {
                let _ = self.save_with_format(false);
                EngineAction::None
            }
            "quit" => {
                // Block if the current buffer has unsaved changes AND this is
                // the last window showing it.  If another window still displays
                // the same buffer the user can still save from there.
                if self.dirty() {
                    let buf_id = self.active_buffer_id();
                    let current_win = self.active_window_id();
                    let other_views = self
                        .windows
                        .values()
                        .any(|w| w.buffer_id == buf_id && w.id != current_win);
                    if !other_views {
                        self.message = "No write since last change (add ! to override)".to_string();
                        return EngineAction::Error;
                    }
                }
                // If this is the very last window in the very last tab of the last group: quit.
                let is_last = self.group_layout.is_single_group()
                    && self.active_group().tabs.len() == 1
                    && self.active_tab().layout.is_single_window();
                if is_last {
                    return EngineAction::Quit;
                }
                // Otherwise close the current window (and the tab if it's the last
                // window in it).  Drop the buffer if nothing else shows it so that
                // collect_session_open_files() (which filters by window-visible buffers)
                // correctly excludes explicitly-closed files from the next session.
                let buf_id = self.active_buffer_id();
                self.close_window();
                if !self.windows.values().any(|w| w.buffer_id == buf_id) {
                    let _ = self.buffer_manager.delete(buf_id, true);
                }
                EngineAction::None
            }
            "quit!" => {
                // If this is the very last window in the very last tab of the last group: quit.
                let is_last = self.group_layout.is_single_group()
                    && self.active_group().tabs.len() == 1
                    && self.active_tab().layout.is_single_window();
                if is_last {
                    return EngineAction::Quit;
                }
                // Force-close without checking dirty flag.
                let buf_id = self.active_buffer_id();
                self.close_window();
                if !self.windows.values().any(|w| w.buffer_id == buf_id) {
                    let _ = self.buffer_manager.delete(buf_id, true);
                }
                EngineAction::None
            }
            "qall" => {
                // Quit all: block if any buffer is dirty.
                let has_dirty = self
                    .buffer_manager
                    .list()
                    .iter()
                    .any(|id| self.buffer_manager.get(*id).is_some_and(|s| s.dirty));
                if has_dirty {
                    self.message = "No write since last change (add ! to override)".to_string();
                    EngineAction::Error
                } else {
                    EngineAction::Quit
                }
            }
            "qall!" => EngineAction::Quit,
            // Write all dirty buffers
            "wall" => {
                let saved = self.save_all_dirty();
                self.message = format!("{} file(s) written", saved);
                EngineAction::None
            }
            // Write all + quit
            "wqall" | "xall" => {
                let _ = self.save_all_dirty();
                EngineAction::Quit
            }
            "wqall!" => EngineAction::Quit,
            // Clear search highlight
            "nohlsearch" => {
                self.search_matches.clear();
                self.search_index = None;
                EngineAction::None
            }
            // Display registers
            "registers" | "display" => {
                let mut lines: Vec<String> = Vec::new();
                lines.push("--- Registers ---".to_string());
                let special_regs: Vec<char> = vec![
                    '"', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-', '+', '*', '.', '%',
                    '/',
                ];
                for &r in &special_regs {
                    if let Some((content, is_lw)) = self.registers.get(&r).cloned() {
                        let kind = if is_lw { "l" } else { "c" };
                        let preview: String = content.chars().take(40).collect();
                        lines.push(format!(
                            "\"{}  {}  {}",
                            r,
                            kind,
                            preview.replace('\n', "\\n")
                        ));
                    }
                }
                for c in 'a'..='z' {
                    if let Some((content, is_lw)) = self.registers.get(&c).cloned() {
                        let kind = if is_lw { "l" } else { "c" };
                        let preview: String = content.chars().take(40).collect();
                        lines.push(format!(
                            "\"{}  {}  {}",
                            c,
                            kind,
                            preview.replace('\n', "\\n")
                        ));
                    }
                }
                self.message = lines.join("\n");
                EngineAction::None
            }
            // Display marks
            "marks" => {
                let buf_id = self.active_buffer_id();
                let mut lines: Vec<String> = Vec::new();
                lines.push("mark line  col  file/text".to_string());
                if let Some(marks_map) = self.marks.get(&buf_id).cloned() {
                    let mut sorted: Vec<(char, Cursor)> = marks_map.into_iter().collect();
                    sorted.sort_by_key(|(c, _)| *c);
                    for (c, cur) in sorted {
                        lines.push(format!(" {}   {:4}  {:3}", c, cur.line + 1, cur.col));
                    }
                }
                for (c, (path, line, col)) in &self.global_marks {
                    let path_str = path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    lines.push(format!(" {}   {:4}  {:3}  {}", c, line + 1, col, path_str));
                }
                self.message = lines.join("\n");
                EngineAction::None
            }
            // Display jump list
            "jumps" => {
                let mut lines: Vec<String> = Vec::new();
                lines.push(" jump line  col  file/text".to_string());
                for (i, (path, line, col)) in self.jump_list.iter().enumerate() {
                    let marker = if i == self.jump_list_pos { ">" } else { " " };
                    let path_str = path
                        .as_ref()
                        .map(|p| {
                            p.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    lines.push(format!(
                        "{} {:4}  {:4}  {:3}  {}",
                        marker,
                        i,
                        line + 1,
                        col,
                        path_str
                    ));
                }
                self.message = lines.join("\n");
                EngineAction::None
            }
            // Display change list
            "changes" => {
                let mut lines: Vec<String> = Vec::new();
                lines.push("change line  col".to_string());
                for (i, (line, col)) in self.change_list.iter().enumerate() {
                    let marker = if i + 1 == self.change_list_pos {
                        ">"
                    } else {
                        " "
                    };
                    lines.push(format!("{} {:4}  {:4}  {:3}", marker, i, line + 1, col));
                }
                self.message = lines.join("\n");
                EngineAction::None
            }
            // Display command history
            "history" => {
                let mut lines: Vec<String> = Vec::new();
                lines.push("--- Command History ---".to_string());
                for (i, cmd) in self.history.command_history.iter().enumerate() {
                    lines.push(format!("{:4}  {}", i + 1, cmd));
                }
                self.message = lines.join("\n");
                EngineAction::None
            }
            // Menu/button "Quit" — asks UI to confirm when there are unsaved changes.
            "quit_menu" | "QuitMenu" => {
                if self.has_any_unsaved() {
                    EngineAction::QuitWithUnsaved
                } else {
                    EngineAction::Quit
                }
            }
            "wq" | "x" => {
                if self.save_with_format(true).is_ok() {
                    // If format-on-save is pending, quit will happen after
                    // the formatting response arrives (format_save_quit_ready).
                    if self.format_on_save_pending.is_some() {
                        EngineAction::None
                    } else {
                        EngineAction::SaveQuit
                    }
                } else {
                    EngineAction::Error
                }
            }
            "debug" => {
                let lang = self
                    .buffer_manager
                    .get(self.active_buffer_id())
                    .and_then(|s| s.file_path.as_ref())
                    .and_then(|p| super::lsp::language_id_from_path(p))
                    .unwrap_or_else(|| "rust".to_string());
                self.dap_start_debug(&lang);
                EngineAction::None
            }
            "continue" => {
                self.dap_continue();
                EngineAction::None
            }
            "pause" => {
                self.dap_pause();
                EngineAction::None
            }
            "stop" => {
                self.dap_stop();
                EngineAction::None
            }
            "restart" => {
                let lang = self
                    .buffer_manager
                    .get(self.active_buffer_id())
                    .and_then(|s| s.file_path.as_ref())
                    .and_then(|p| super::lsp::language_id_from_path(p))
                    .unwrap_or_else(|| "rust".to_string());
                self.dap_stop();
                self.dap_start_debug(&lang);
                EngineAction::None
            }
            "stepover" => {
                self.dap_step_over();
                EngineAction::None
            }
            "stepin" => {
                self.dap_step_into();
                EngineAction::None
            }
            "stepout" => {
                self.dap_step_out();
                EngineAction::None
            }
            "brkpt" => {
                let file = self
                    .buffer_manager
                    .get(self.active_buffer_id())
                    .and_then(|s| s.file_path.as_ref())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let line = self.cursor().line as u64 + 1; // 1-based
                self.dap_toggle_breakpoint(&file, line);
                EngineAction::None
            }
            "copy" => {
                // Bare :copy without address — show usage
                self.message = "Usage: :copy {address}".to_string();
                EngineAction::None
            }
            "clipboard_copy" => {
                // Yank current line to clipboard-style yank (palette action)
                self.execute_command("yank")
            }
            "cut" => {
                // Cut current line
                self.execute_command("dd")
            }
            "paste" => self.execute_command("p"),
            "termkill" => {
                self.terminal_close_active_tab();
                EngineAction::None
            }
            "about" => {
                self.show_dialog(
                    "about",
                    "About VimCode",
                    vec![
                        format!("VimCode {}", env!("CARGO_PKG_VERSION")),
                        String::new(),
                        "Vim-like code editor in Rust + GTK4".to_string(),
                    ],
                    vec![DialogButton {
                        label: "OK".to_string(),
                        hotkey: 'o',
                        action: "ok".to_string(),
                    }],
                );
                EngineAction::None
            }
            "openrecent" | "OpenRecent" => EngineAction::OpenRecentDialog,
            "palette" | "CommandPalette" => {
                self.open_picker(PickerSource::Commands);
                EngineAction::None
            }
            // ── Menu / palette action aliases ─────────────────────────────────
            "fuzzy" | "Picker" | "Picker files" => {
                self.open_picker(PickerSource::Files);
                EngineAction::None
            }
            "Picker commands" => {
                self.open_picker(PickerSource::Commands);
                EngineAction::None
            }
            "CommandCenter" => {
                self.open_command_center();
                EngineAction::None
            }
            "undo" => {
                self.undo();
                self.refresh_md_previews();
                EngineAction::None
            }
            "redo" => {
                self.redo();
                self.refresh_md_previews();
                EngineAction::None
            }
            "find" => {
                // Open incremental forward search (same as pressing /)
                self.mode = Mode::Search;
                self.command_buffer.clear();
                self.search_direction = SearchDirection::Forward;
                self.search_start_cursor = Some(self.view().cursor);
                self.search_word_bounded = false;
                self.count = None;
                EngineAction::None
            }
            "replace" => {
                // Open ex command mode pre-filled with %s/ for find & replace
                self.mode = Mode::Command;
                self.command_buffer = "%s/".to_string();
                self.command_cursor = self.command_buffer.chars().count();
                self.count = None;
                EngineAction::None
            }
            "sidebar" => EngineAction::ToggleSidebar,
            "zoomin" => {
                self.settings.font_size = (self.settings.font_size + 1).min(72);
                let _ = self.settings.save();
                EngineAction::None
            }
            "zoomout" => {
                self.settings.font_size = (self.settings.font_size - 1).max(6);
                let _ = self.settings.save();
                EngineAction::None
            }
            "set_wrap_toggle" => {
                self.settings.wrap = !self.settings.wrap;
                let _ = self.settings.save();
                let state = if self.settings.wrap { "wrap" } else { "nowrap" };
                self.message = format!("set {}", state);
                EngineAction::None
            }
            "goto" => {
                self.message = "Use :N to go to line N".to_string();
                EngineAction::None
            }
            "def" => {
                self.lsp_request_definition();
                EngineAction::None
            }
            "refs" => {
                self.lsp_request_references();
                EngineAction::None
            }
            "hover" => {
                self.trigger_editor_hover_at_cursor();
                EngineAction::None
            }
            "LspImpl" => {
                self.lsp_request_implementation();
                EngineAction::None
            }
            "LspTypedef" => {
                self.lsp_request_type_definition();
                EngineAction::None
            }
            "CodeAction" => {
                self.show_code_actions_popup();
                EngineAction::None
            }
            "nextdiag" => {
                self.jump_next_diagnostic();
                EngineAction::None
            }
            "prevdiag" => {
                self.jump_prev_diagnostic();
                EngineAction::None
            }
            "nexthunk" => {
                self.jump_next_hunk();
                EngineAction::None
            }
            "prevhunk" => {
                self.jump_prev_hunk();
                EngineAction::None
            }
            "back" => {
                self.jump_list_back();
                EngineAction::None
            }
            "fwd" => {
                self.jump_list_forward();
                EngineAction::None
            }
            "saveas" => {
                self.message = "Usage: :saveas {file}".to_string();
                EngineAction::None
            }
            "keys" => {
                self.message =
                    "Key ref: / search  :N line  gd def  gr refs  Ctrl+P fuzzy  Ctrl+G grep"
                        .to_string();
                EngineAction::None
            }
            "delete" => {
                // :d — delete current line (used by :g/pat/d etc.)
                let mut changed = false;
                self.delete_lines(1, &mut changed);
                EngineAction::None
            }
            // ── Editor group commands ─────────────────────────────────────────
            "EditorGroupSplit" | "egsp" => {
                self.open_editor_group(SplitDirection::Vertical);
                EngineAction::None
            }
            "EditorGroupSplitDown" | "egspd" => {
                self.open_editor_group(SplitDirection::Horizontal);
                EngineAction::None
            }
            "EditorGroupClose" | "egc" => {
                self.close_editor_group();
                EngineAction::None
            }
            "EditorGroupFocus" | "egf" => {
                self.focus_other_group();
                EngineAction::None
            }
            "EditorGroupMoveTab" | "egmt" => {
                self.move_tab_to_other_group();
                EngineAction::None
            }
            // ── Markdown Preview ─────────────────────────────────────────────
            "MarkdownPreview" | "MdPreview" => {
                let is_md = self
                    .file_path()
                    .and_then(|p| p.extension())
                    .map(|ext| ext == "md" || ext == "markdown")
                    .unwrap_or(false);
                if !is_md {
                    self.message = "Not a markdown file".to_string();
                    return EngineAction::Error;
                }
                self.open_markdown_preview_linked();
                EngineAction::None
            }
            // ── New Vim ex commands ───────────────────────────────────────────
            "join" => {
                let mut changed = false;
                self.join_lines(1, &mut changed);
                EngineAction::None
            }
            "yank" => {
                // :y[ank] [register] — yank current line
                let line = self.view().cursor.line;
                let text = self.buffer().content.line(line).chars().collect::<String>();
                self.registers.insert('"', (text.clone(), true));
                self.registers.insert('0', (text, true));
                EngineAction::None
            }
            "put" => {
                // :pu[t] — put default register after current line
                if let Some((content, _)) = self.registers.get(&'"').cloned() {
                    let line = self.view().cursor.line;
                    let num_lines = self.buffer().len_lines();
                    let insert_pos = if line + 1 < num_lines {
                        self.buffer().line_to_char(line + 1)
                    } else {
                        let end = self.buffer().len_chars();
                        if end > 0 && self.buffer().content.char(end - 1) != '\n' {
                            self.start_undo_group();
                            self.insert_with_undo(end, "\n");
                            let text = if content.ends_with('\n') {
                                content
                            } else {
                                format!("{content}\n")
                            };
                            self.insert_with_undo(end + 1, &text);
                            self.finish_undo_group();
                            return EngineAction::None;
                        }
                        end
                    };
                    let text = if content.ends_with('\n') {
                        content
                    } else {
                        format!("{content}\n")
                    };
                    self.start_undo_group();
                    self.insert_with_undo(insert_pos, &text);
                    self.finish_undo_group();
                    self.view_mut().cursor.line = line + 1;
                    self.view_mut().cursor.col = 0;
                } else {
                    self.message = "Register is empty".to_string();
                }
                EngineAction::None
            }
            "pwd" => {
                self.message = self.cwd.to_string_lossy().to_string();
                EngineAction::None
            }
            "file" => {
                let name = self
                    .file_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "[No Name]".to_string());
                let modified = if self.dirty() { " [Modified]" } else { "" };
                let total = self.buffer().len_lines();
                let cur_line = self.view().cursor.line + 1;
                let pct = if total == 0 {
                    0
                } else {
                    (cur_line * 100) / total
                };
                self.message = format!("\"{name}\"{modified} {total} lines --{pct}%--");
                EngineAction::None
            }
            "enew" => {
                let new_id = self.buffer_manager.create();
                self.switch_window_buffer(new_id);
                self.message = "New buffer".to_string();
                EngineAction::None
            }
            "update" => {
                if self.dirty() {
                    let _ = self.save_with_format(false);
                } else {
                    self.message = "(no changes)".to_string();
                }
                EngineAction::None
            }
            "version" => {
                self.message = format!("VimCode {}", env!("CARGO_PKG_VERSION"));
                EngineAction::None
            }
            "print" => {
                let line = self.view().cursor.line;
                let text = self.buffer().content.line(line).chars().collect::<String>();
                self.message = text.trim_end_matches('\n').to_string();
                EngineAction::None
            }
            "number" => {
                let line = self.view().cursor.line;
                let text = self.buffer().content.line(line).chars().collect::<String>();
                self.message = format!("{:>6}  {}", line + 1, text.trim_end_matches('\n'));
                EngineAction::None
            }
            "new" => {
                // Horizontal split + new empty buffer
                self.split_window(SplitDirection::Horizontal, None);
                let new_id = self.buffer_manager.create();
                self.switch_window_buffer(new_id);
                EngineAction::None
            }
            "vnew" => {
                // Vertical split + new empty buffer
                self.split_window(SplitDirection::Vertical, None);
                let new_id = self.buffer_manager.create();
                self.switch_window_buffer(new_id);
                EngineAction::None
            }
            "retab" => {
                let tab_size = self.settings.tabstop as usize;
                let expand = self.settings.expand_tab;
                self.start_undo_group();
                let num_lines = self.buffer().len_lines();
                for line_idx in 0..num_lines {
                    let text: String = self.buffer().content.line(line_idx).chars().collect();
                    let new_text = if expand {
                        // tabs → spaces
                        text.replace('\t', &" ".repeat(tab_size))
                    } else {
                        // Leading spaces → tabs
                        let leading: usize = text.chars().take_while(|c| *c == ' ').count();
                        if leading >= tab_size {
                            let tabs = leading / tab_size;
                            let spaces = leading % tab_size;
                            format!(
                                "{}{}{}",
                                "\t".repeat(tabs),
                                " ".repeat(spaces),
                                &text[leading..]
                            )
                        } else {
                            continue;
                        }
                    };
                    if new_text != text {
                        let start = self.buffer().line_to_char(line_idx);
                        let end = start + text.len();
                        self.delete_with_undo(start, end);
                        self.insert_with_undo(start, &new_text);
                    }
                }
                self.finish_undo_group();
                self.message = "Retabbed".to_string();
                EngineAction::None
            }
            "cquit" | "cquit!" => EngineAction::QuitWithError,
            _ => {
                // Handle :y[ank] {register} and :pu[t] {register} with args
                if let Some(arg) = cmd.strip_prefix("yank ") {
                    let arg = arg.trim();
                    let reg = arg.chars().next().unwrap_or('"');
                    let line = self.view().cursor.line;
                    let text = self.buffer().content.line(line).chars().collect::<String>();
                    self.registers.insert(reg, (text.clone(), true));
                    if reg != '"' {
                        self.registers.insert('"', (text, true));
                    }
                    return EngineAction::None;
                }
                if let Some(arg) = cmd.strip_prefix("put ") {
                    let reg = arg.trim().chars().next().unwrap_or('"');
                    if let Some((content, _)) = self.registers.get(&reg).cloned() {
                        let line = self.view().cursor.line;
                        let num_lines = self.buffer().len_lines();
                        let insert_pos = if line + 1 < num_lines {
                            self.buffer().line_to_char(line + 1)
                        } else {
                            self.buffer().len_chars()
                        };
                        let text = if content.ends_with('\n') {
                            content
                        } else {
                            format!("{content}\n")
                        };
                        self.start_undo_group();
                        self.insert_with_undo(insert_pos, &text);
                        self.finish_undo_group();
                        self.view_mut().cursor.line = line + 1;
                        self.view_mut().cursor.col = 0;
                    } else {
                        self.message = format!("Register '{reg}' is empty");
                    }
                    return EngineAction::None;
                }
                if let Some(arg) = cmd.strip_prefix("retab ") {
                    if let Ok(ts) = arg.trim().parse::<u8>() {
                        self.settings.tabstop = ts;
                    }
                    return self.execute_command("retab");
                }
                // Built-in :Comment / :Commentary command
                if cmd == "Comment"
                    || cmd.starts_with("Comment ")
                    || cmd == "Commentary"
                    || cmd.starts_with("Commentary ")
                {
                    let args = if let Some(rest) = cmd.strip_prefix("Commentary") {
                        rest.trim()
                    } else {
                        cmd.strip_prefix("Comment").unwrap().trim()
                    };
                    let count: usize = args.parse().unwrap_or(1).max(1);
                    let line = self.view().cursor.line + 1; // 1-indexed
                    self.toggle_comment(line, line + count - 1);
                    return EngineAction::None;
                }
                // Try plugin commands before giving up
                let (cmd_name, cmd_args) = cmd.split_once(' ').unwrap_or((cmd, ""));
                if self.plugin_run_command(cmd_name, cmd_args) {
                    return EngineAction::None;
                }
                self.message = format!("Not an editor command: {}", cmd);
                EngineAction::Error
            }
        }
    }

    pub(crate) fn execute_norm_command(&mut self, range_str: &str, keys: &str) -> EngineAction {
        if keys.is_empty() {
            self.message = "Usage: :norm[al][!] {keys}".to_string();
            return EngineAction::Error;
        }

        let total_lines = self.buffer().len_lines();

        // Resolve range to 0-based (start_line, end_line)
        let (start_line, end_line) = if range_str == "%" {
            (0usize, total_lines.saturating_sub(1))
        } else if range_str == "'<,'>" {
            match self.get_visual_selection_range() {
                Some((start, end)) => (start.line, end.line),
                None => {
                    self.message = "No visual selection".to_string();
                    return EngineAction::Error;
                }
            }
        } else if !range_str.is_empty() {
            // Numeric range "N,M" (1-based line numbers → 0-based)
            let mut parts = range_str.splitn(2, ',');
            let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
            let end: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(start);
            let s = start.saturating_sub(1).min(total_lines.saturating_sub(1));
            let e = end.saturating_sub(1).min(total_lines.saturating_sub(1));
            (s, e)
        } else {
            let l = self.view().cursor.line;
            (l, l)
        };

        let keys_chars: Vec<char> = keys.chars().collect();

        // Save undo stack depth so we can merge all new entries into one step
        let saved_undo_len = self.active_buffer_state_mut().undo_stack.len();

        for line_num in start_line..=end_line {
            if line_num >= self.buffer().len_lines() {
                break;
            }
            // Position cursor at start of line in Normal mode
            self.view_mut().cursor.line = line_num;
            self.view_mut().cursor.col = 0;
            self.mode = Mode::Normal;
            self.pending_key = None;
            self.count = None;

            // Execute the key sequence using a local decode loop (does not
            // disturb macro_playback_queue, safe even when called from a macro)
            let mut pos = 0;
            while pos < keys_chars.len() {
                let (key_name, unicode, ctrl, consumed) = if keys_chars[pos] == '<' {
                    // Collect up to closing '>'
                    let mut seq = String::new();
                    let mut found = false;
                    for &c in &keys_chars[pos..] {
                        seq.push(c);
                        if c == '>' {
                            found = true;
                            break;
                        }
                    }
                    let len = seq.len();
                    if found && len > 1 {
                        if let Some((kn, uc, ct)) = self.parse_key_sequence(&seq) {
                            (kn, uc, ct, len)
                        } else {
                            ("".to_string(), Some('<'), false, 1)
                        }
                    } else {
                        ("".to_string(), Some('<'), false, 1)
                    }
                } else if keys_chars[pos] == '\x1b' {
                    ("Escape".to_string(), None, false, 1)
                } else {
                    ("".to_string(), Some(keys_chars[pos]), false, 1)
                };

                pos += consumed;
                self.macro_recursion_depth += 1;
                let _ = self.handle_key(&key_name, unicode, ctrl);
                self.macro_recursion_depth -= 1;
            }

            // Ensure Normal mode after each line's key sequence
            self.mode = Mode::Normal;
            self.pending_key = None;
        }

        // Finalize the last open undo group (e.g. from trailing insert mode)
        self.active_buffer_state_mut().finish_undo_group();

        // Merge all undo entries created during :norm into a single undoable step
        let state = self.active_buffer_state_mut();
        if state.undo_stack.len() > saved_undo_len + 1 {
            let new_entries: Vec<UndoEntry> = state.undo_stack.drain(saved_undo_len..).collect();
            let cursor_before = new_entries[0].cursor_before;
            let merged_ops: Vec<_> = new_entries.into_iter().flat_map(|e| e.ops).collect();
            if !merged_ops.is_empty() {
                state.undo_stack.push(UndoEntry {
                    ops: merged_ops,
                    cursor_before,
                });
            }
        }

        let n = end_line.saturating_sub(start_line) + 1;
        self.message = format!("{} line{} affected", n, if n == 1 { "" } else { "s" });
        EngineAction::None
    }

    /// :g/pat/cmd or :v/pat/cmd — run ex cmd on matching (or non-matching) lines.
    pub(crate) fn execute_global_command(&mut self, rest: &str, invert: bool) -> EngineAction {
        // rest is "pat/cmd" — find the closing '/' after pattern
        let sep_pos = match rest.find('/') {
            Some(p) => p,
            None => {
                self.message = "Usage: :g/pattern/command".to_string();
                return EngineAction::None;
            }
        };
        let pattern = &rest[..sep_pos];
        let subcmd = rest[sep_pos + 1..].trim().to_string();
        if pattern.is_empty() {
            self.message = "Empty pattern".to_string();
            return EngineAction::None;
        }

        // Collect matching line indices BEFORE executing (avoid borrow issues)
        let num_lines = self.buffer().len_lines();
        let mut matching: Vec<usize> = Vec::new();
        for line_idx in 0..num_lines {
            let line_text: String = self.buffer().content.line(line_idx).chars().collect();
            let matches = line_text.contains(pattern);
            if matches != invert {
                matching.push(line_idx);
            }
        }

        if matching.is_empty() {
            self.message = format!("Pattern not found: {}", pattern);
            return EngineAction::None;
        }

        // Execute subcommand on each matching line in REVERSE order (preserves line numbers)
        let mut executed = 0usize;
        for &line_idx in matching.iter().rev() {
            let current_lines = self.buffer().len_lines();
            if line_idx >= current_lines {
                continue;
            }
            self.view_mut().cursor.line = line_idx;
            self.view_mut().cursor.col = 0;
            self.execute_command(&subcmd.clone());
            executed += 1;
        }

        self.message = format!(
            "{} line{} affected",
            executed,
            if executed == 1 { "" } else { "s" }
        );
        EngineAction::None
    }

    /// :sort [flags] — sort all lines, with optional flags (n=numeric, r=reverse, u=unique, i=ignorecase).
    pub(crate) fn execute_sort_command(&mut self, flags: &str) -> EngineAction {
        let numeric = flags.contains('n');
        let reverse = flags.contains('r');
        let unique = flags.contains('u');
        let ignorecase = flags.contains('i');

        let num_lines = self.buffer().len_lines();
        if num_lines == 0 {
            return EngineAction::None;
        }

        // Collect all lines (excluding trailing newline per line)
        let mut lines: Vec<String> = (0..num_lines)
            .map(|i| {
                let s: String = self.buffer().content.line(i).chars().collect();
                if s.ends_with('\n') {
                    s[..s.len() - 1].to_string()
                } else {
                    s
                }
            })
            .collect();

        // Sort
        if numeric {
            lines.sort_by(|a, b| {
                let na: i64 = a.trim().parse().unwrap_or(i64::MIN);
                let nb: i64 = b.trim().parse().unwrap_or(i64::MIN);
                let ord = na.cmp(&nb);
                if reverse {
                    ord.reverse()
                } else {
                    ord
                }
            });
        } else {
            lines.sort_by(|a, b| {
                let ka = if ignorecase {
                    a.to_lowercase()
                } else {
                    a.clone()
                };
                let kb = if ignorecase {
                    b.to_lowercase()
                } else {
                    b.clone()
                };
                let ord = ka.cmp(&kb);
                if reverse {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }

        if unique {
            lines.dedup_by(|a, b| {
                if ignorecase {
                    a.to_lowercase() == b.to_lowercase()
                } else {
                    a == b
                }
            });
        }

        // Replace buffer content
        let new_content = lines.join("\n") + "\n";
        let total_chars = self.buffer().len_chars();
        self.start_undo_group();
        self.delete_with_undo(0, total_chars);
        self.insert_with_undo(0, &new_content);
        self.finish_undo_group();
        self.view_mut().cursor.line = 0;
        self.view_mut().cursor.col = 0;
        self.message = format!("{} lines sorted", lines.len());
        EngineAction::None
    }

    /// :m[ove] {dest} — move current line to after line {dest}.
    /// dest: absolute line number (1-based), 0 = before first line, . = current, $ = last, +N/-N = relative.
    pub(crate) fn execute_move_command(&mut self, dest: &str) -> EngineAction {
        let current_line = self.view().cursor.line;
        let num_lines = self.buffer().len_lines();
        let dest_line = self.parse_line_address(dest, current_line, num_lines);

        if dest_line == current_line {
            return EngineAction::None;
        }

        // Grab the line content to move
        let line_start = self.buffer().line_to_char(current_line);
        let line_end = if current_line + 1 < num_lines {
            self.buffer().line_to_char(current_line + 1)
        } else {
            self.buffer().len_chars()
        };
        let line_text: String = self
            .buffer()
            .content
            .slice(line_start..line_end)
            .chars()
            .collect();
        let line_text = if line_text.ends_with('\n') {
            line_text
        } else {
            format!("{}\n", line_text)
        };

        self.start_undo_group();

        if dest_line > current_line {
            // Insert first (positions shift after deletion)
            let insert_after = dest_line.min(num_lines - 1);
            let insert_pos = if insert_after + 1 < num_lines {
                self.buffer().line_to_char(insert_after + 1)
            } else {
                self.buffer().len_chars()
            };
            self.insert_with_undo(insert_pos, &line_text);
            // Delete original
            let line_start2 = self.buffer().line_to_char(current_line);
            let line_end2 = self.buffer().line_to_char(current_line + 1);
            self.delete_with_undo(line_start2, line_end2);
            self.view_mut().cursor.line = dest_line;
        } else {
            // Delete first
            let del_end = if current_line < self.buffer().len_lines() {
                self.buffer().line_to_char(current_line + 1)
            } else {
                self.buffer().len_chars()
            };
            self.delete_with_undo(line_start, del_end);
            // Insert after dest_line
            let insert_pos = if dest_line < self.buffer().len_lines() {
                let after = if dest_line == 0 { 0 } else { dest_line };
                if after == 0 {
                    0usize
                } else {
                    self.buffer().line_to_char(after)
                }
            } else {
                self.buffer().len_chars()
            };
            self.insert_with_undo(insert_pos, &line_text);
            self.view_mut().cursor.line = if dest_line == 0 { 0 } else { dest_line };
        }

        self.finish_undo_group();
        let max_line = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = self.view().cursor.line.min(max_line);
        self.view_mut().cursor.col = 0;
        EngineAction::None
    }

    /// :t {dest} / :co[py] {dest} — copy current line to after line {dest}.
    pub(crate) fn execute_copy_command(&mut self, dest: &str) -> EngineAction {
        let current_line = self.view().cursor.line;
        let num_lines = self.buffer().len_lines();
        let dest_line = self.parse_line_address(dest, current_line, num_lines);

        // Grab the line content to copy
        let line_start = self.buffer().line_to_char(current_line);
        let line_end = if current_line + 1 < num_lines {
            self.buffer().line_to_char(current_line + 1)
        } else {
            self.buffer().len_chars()
        };
        let line_text: String = self
            .buffer()
            .content
            .slice(line_start..line_end)
            .chars()
            .collect();
        let line_text = if line_text.ends_with('\n') {
            line_text
        } else {
            format!("{}\n", line_text)
        };

        // Insert copy after dest_line (after the line at dest_line index)
        let insert_pos = if dest_line >= num_lines {
            self.buffer().len_chars()
        } else {
            let after = dest_line.min(num_lines - 1);
            if after + 1 < num_lines {
                self.buffer().line_to_char(after + 1)
            } else {
                self.buffer().len_chars()
            }
        };

        self.start_undo_group();
        self.insert_with_undo(insert_pos, &line_text);
        self.finish_undo_group();

        let new_line = if dest_line < current_line {
            current_line + 1
        } else {
            dest_line + 1
        };
        let max_line = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = new_line.min(max_line);
        self.view_mut().cursor.col = 0;
        EngineAction::None
    }

    /// Parse a line address string to a 0-based line index.
    /// Supports: "0", "1"-"N" (1-based absolute), ".", "$", "+N", "-N".
    pub(crate) fn parse_line_address(&self, addr: &str, current: usize, total: usize) -> usize {
        let addr = addr.trim();
        if addr == "." {
            return current;
        }
        if addr == "$" {
            return total.saturating_sub(1);
        }
        if let Some(n_str) = addr.strip_prefix('+') {
            let n: usize = n_str.parse().unwrap_or(0);
            return (current + n).min(total.saturating_sub(1));
        }
        if let Some(n_str) = addr.strip_prefix('-') {
            let n: usize = n_str.parse().unwrap_or(0);
            return current.saturating_sub(n);
        }
        if let Ok(n) = addr.parse::<usize>() {
            // 0-based absolute line index
            return n.min(total.saturating_sub(1));
        }
        current
    }

    pub(crate) fn execute_substitute_command(&mut self, cmd: &str) -> EngineAction {
        // Parse: [range]s/pattern/replacement/[flags]
        // Supported ranges: none (current line), % (all lines), '<,'> (visual selection)

        // Determine if this is :%s (all lines) or :s (current line/visual selection)
        let (range_str, rest) = if cmd.starts_with("%s/") {
            ("%", &cmd[2..]) // Skip "%s"
        } else if cmd.starts_with("s/") {
            ("", &cmd[1..]) // Skip "s"
        } else if cmd.starts_with("'<,'>s/") {
            // Visual selection range (set when entering command mode from visual)
            ("'<,'>", &cmd[6..]) // Skip "'<,'>s"
        } else {
            self.message = "Invalid substitute command".to_string();
            return EngineAction::Error;
        };

        // Parse /pattern/replacement/flags
        // rest is like "/foo/baz/" or "/foo/baz/g"
        // Splitting by '/' gives: ["", "foo", "baz", ""] or ["", "foo", "baz", "g"]
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() < 3 {
            self.message = "Usage: :s/pattern/replacement/[flags]".to_string();
            return EngineAction::Error;
        }

        let pattern = parts[1];
        let replacement = parts.get(2).unwrap_or(&"");
        let flags = parts.get(3).unwrap_or(&"");

        // Save for & repeat
        self.last_substitute = Some((
            pattern.to_string(),
            replacement.to_string(),
            flags.to_string(),
        ));

        // Determine line range
        let range = if range_str == "%" {
            // All lines
            let last = self.buffer().len_lines().saturating_sub(1);
            Some((0, last))
        } else if range_str == "'<,'>" {
            // Visual selection (if we have one)
            if let Some((start, end)) = self.get_visual_selection_range() {
                Some((start.line, end.line))
            } else {
                self.message = "No visual selection".to_string();
                return EngineAction::Error;
            }
        } else {
            // Current line only
            None
        };

        // Execute replacement
        match self.replace_in_range(range, pattern, replacement, flags) {
            Ok(count) => {
                self.message = format!(
                    "{} substitution{}",
                    count,
                    if count == 1 { "" } else { "s" }
                );
                EngineAction::None
            }
            Err(e) => {
                self.message = e;
                EngineAction::Error
            }
        }
    }

    // --- Search ---

    pub fn run_search(&mut self) {
        self.search_matches.clear();
        self.search_index = None;

        if self.search_query.is_empty() {
            return;
        }

        let text = self.buffer().to_string();
        let query_orig = self.search_query.clone();

        // Apply ignorecase / smartcase
        let case_insensitive = self.settings.ignorecase
            && !(self.settings.smartcase && query_orig.chars().any(|c| c.is_uppercase()));

        if case_insensitive {
            let text_lower = text.to_lowercase();
            let query_lower = query_orig.to_lowercase();
            let mut byte_pos = 0;
            while let Some(found) = text_lower[byte_pos..].find(&query_lower) {
                let start_byte = byte_pos + found;
                let end_byte = start_byte + query_lower.len();
                let start_char = self.buffer().content.byte_to_char(start_byte);
                let end_char = self.buffer().content.byte_to_char(end_byte);
                self.search_matches.push((start_char, end_char));
                byte_pos = start_byte + 1;
            }
        } else {
            let mut byte_pos = 0;
            while let Some(found) = text[byte_pos..].find(query_orig.as_str()) {
                let start_byte = byte_pos + found;
                let end_byte = start_byte + query_orig.len();
                let start_char = self.buffer().content.byte_to_char(start_byte);
                let end_char = self.buffer().content.byte_to_char(end_byte);
                self.search_matches.push((start_char, end_char));
                byte_pos = start_byte + 1;
            }
        }

        if self.search_matches.is_empty() {
            self.message = format!("Pattern not found: {}", self.search_query);
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            if !self.search_query.is_empty() {
                // Re-run search (matches may have been cleared by Escape/:noh)
                self.run_search();
                if self.search_matches.is_empty() {
                    self.message = format!("Pattern not found: {}", self.search_query);
                    return;
                }
            } else {
                return;
            }
        }

        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let cursor_char = self.buffer().line_to_char(line) + col;

        let next = self
            .search_matches
            .iter()
            .position(|(start, _)| *start > cursor_char);
        let idx = next.unwrap_or(0);

        self.search_index = Some(idx);
        self.jump_to_search_match(idx);
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            if !self.search_query.is_empty() {
                // Re-run search (matches may have been cleared by Escape/:noh)
                self.run_search();
                if self.search_matches.is_empty() {
                    self.message = format!("Pattern not found: {}", self.search_query);
                    return;
                }
            } else {
                return;
            }
        }

        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let cursor_char = self.buffer().line_to_char(line) + col;

        let prev = self
            .search_matches
            .iter()
            .rposition(|(start, _)| *start < cursor_char);
        let idx = prev.unwrap_or(self.search_matches.len() - 1);

        self.search_index = Some(idx);
        self.jump_to_search_match(idx);
    }

    pub(crate) fn jump_to_search_match(&mut self, idx: usize) {
        if let Some(&(start_char, _)) = self.search_matches.get(idx) {
            let line = self.buffer().content.char_to_line(start_char);
            let line_start = self.buffer().line_to_char(line);
            let col = start_char - line_start;
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = col;
            self.ensure_cursor_visible();
            // If the match landed in the bottom quarter of the viewport,
            // center it so it's not barely visible at the edge (Vim-like behavior).
            let vp = self.view().viewport_lines;
            if vp > 4 {
                let cursor_line = self.view().cursor.line;
                let scroll_top = self.view().scroll_top;
                if cursor_line > scroll_top + vp * 3 / 4 {
                    self.scroll_cursor_center();
                }
            }
            self.message = format!("match {} of {}", idx + 1, self.search_matches.len());
        }
    }

    /// Perform incremental search as user types
    pub fn perform_incremental_search(&mut self) {
        // Update search query from command buffer
        self.search_query = self.command_buffer.clone();

        if self.search_query.is_empty() {
            // Restore to start position if search is empty
            if let Some(start_cursor) = self.search_start_cursor {
                self.view_mut().cursor = start_cursor;
            }
            self.search_matches.clear();
            self.search_index = None;
            self.message.clear();
            return;
        }

        // Run the search
        self.run_search();

        // Jump to the first match from the start position
        if !self.search_matches.is_empty() {
            // Get the starting cursor position
            let start_cursor = self.search_start_cursor.unwrap_or(self.view().cursor);
            let start_char = self.buffer().line_to_char(start_cursor.line) + start_cursor.col;

            // Find the appropriate match based on search direction
            let idx = match self.search_direction {
                SearchDirection::Forward => {
                    // Find first match at or after start position
                    self.search_matches
                        .iter()
                        .position(|(start, _)| *start >= start_char)
                        .unwrap_or(0)
                }
                SearchDirection::Backward => {
                    // Find last match strictly before start position
                    self.search_matches
                        .iter()
                        .rposition(|(start, _)| *start < start_char)
                        .unwrap_or(self.search_matches.len() - 1)
                }
            };

            self.search_index = Some(idx);
            self.jump_to_search_match(idx);
        } else {
            // No matches, restore to start position
            if let Some(start_cursor) = self.search_start_cursor {
                self.view_mut().cursor = start_cursor;
            }
        }
    }

    // --- Find/Replace methods ---

    /// Replace text in a given range
    /// range: None = current line, Some((start_line, end_line)) = line range
    /// pattern: string to find (will use simple substring matching for now)
    /// replacement: string to replace with
    /// flags: "g" (all), "c" (confirm), "i" (case-insensitive)
    /// Returns: (num_replacements, modified_text_preview)
    pub fn replace_in_range(
        &mut self,
        range: Option<(usize, usize)>,
        pattern: &str,
        replacement: &str,
        flags: &str,
    ) -> Result<usize, String> {
        if pattern.is_empty() {
            return Err("Pattern cannot be empty".to_string());
        }

        let global = flags.contains('g');
        let _confirm = flags.contains('c'); // For Phase 2
        let case_insensitive = flags.contains('i');

        // Determine line range
        let (start_line, end_line) = match range {
            Some((s, e)) => (s, e),
            None => {
                let current = self.view().cursor.line;
                (current, current)
            }
        };

        let mut replacements = 0;
        self.start_undo_group();

        // Process each line in range
        for line_num in start_line..=end_line {
            if line_num >= self.buffer().len_lines() {
                break;
            }

            let line_start_char = self.buffer().line_to_char(line_num);
            let line_len = self.buffer().line_len_chars(line_num);
            let line_text: String = self
                .buffer()
                .content
                .slice(line_start_char..line_start_char + line_len)
                .chars()
                .collect();

            // Find and replace in this line
            let new_line = if global {
                self.replace_all_in_string(&line_text, pattern, replacement, case_insensitive)
            } else {
                self.replace_first_in_string(&line_text, pattern, replacement, case_insensitive)
            };

            if new_line != line_text {
                // Delete old line content and insert new
                self.delete_with_undo(line_start_char, line_start_char + line_len);
                self.insert_with_undo(line_start_char, &new_line);
                replacements += 1;
            }
        }

        self.finish_undo_group();
        Ok(replacements)
    }

    /// Helper: Replace all occurrences in a string
    pub(crate) fn replace_all_in_string(
        &self,
        text: &str,
        pattern: &str,
        replacement: &str,
        case_insensitive: bool,
    ) -> String {
        if case_insensitive {
            // Case-insensitive: convert to lowercase for comparison
            let pattern_lower = pattern.to_lowercase();
            let text_lower = text.to_lowercase();

            let mut result = String::new();
            let mut last_pos = 0;

            while let Some(pos) = text_lower[last_pos..].find(&pattern_lower) {
                let absolute_pos = last_pos + pos;
                result.push_str(&text[last_pos..absolute_pos]);
                result.push_str(replacement);
                last_pos = absolute_pos + pattern.len();
            }
            result.push_str(&text[last_pos..]);
            result
        } else {
            text.replace(pattern, replacement)
        }
    }

    /// Helper: Replace first occurrence in a string
    pub(crate) fn replace_first_in_string(
        &self,
        text: &str,
        pattern: &str,
        replacement: &str,
        case_insensitive: bool,
    ) -> String {
        if case_insensitive {
            let pattern_lower = pattern.to_lowercase();
            let text_lower = text.to_lowercase();

            if let Some(pos) = text_lower.find(&pattern_lower) {
                let mut result = String::new();
                result.push_str(&text[..pos]);
                result.push_str(replacement);
                result.push_str(&text[pos + pattern.len()..]);
                result
            } else {
                text.to_string()
            }
        } else if let Some(pos) = text.find(pattern) {
            let mut result = String::new();
            result.push_str(&text[..pos]);
            result.push_str(replacement);
            result.push_str(&text[pos + pattern.len()..]);
            result
        } else {
            text.to_string()
        }
    }

    /// Handle a click on an interactive status bar segment.
    pub fn handle_status_action(&mut self, action: &StatusAction) {
        match action {
            StatusAction::GoToLine => {
                self.open_picker(PickerSource::CommandCenter);
                self.picker_query = ":".to_string();
                self.picker_filter();
                self.picker_load_preview();
            }
            StatusAction::ChangeLanguage => {
                self.open_picker(PickerSource::Languages);
            }
            StatusAction::ChangeIndentation => {
                self.open_picker(PickerSource::Indentation);
            }
            StatusAction::ChangeLineEnding => {
                self.open_picker(PickerSource::LineEndings);
            }
            StatusAction::ChangeEncoding => {
                self.message = "Only UTF-8 encoding is supported".to_string();
            }
            StatusAction::SwitchBranch => {
                self.open_picker(PickerSource::GitBranches);
            }
            StatusAction::LspInfo => {
                let _ = self.execute_command("LspInfo");
            }
        }
    }
}
