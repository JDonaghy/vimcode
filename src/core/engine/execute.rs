use super::*;

impl Engine {
    pub fn execute_command(&mut self, cmd: &str) -> EngineAction {
        // Save for @: repeat (before normalization, using trimmed original)
        let trimmed_cmd = cmd.trim();
        if !trimmed_cmd.is_empty() {
            self.last_ex_command = Some(trimmed_cmd.to_string());
        }

        // Handle :norm[al][!] before trimming — keys may contain significant trailing whitespace
        if let Some(action) = self.try_execute_norm(cmd.trim_start()) {
            return action;
        }

        let cmd = cmd.trim();
        let normalized = normalize_ex_command(cmd);
        let cmd: &str = &normalized;

        // Handle `:{range}{cmd}` — the line-oriented ex commands that take a
        // general address range (`:2d`, `:'a,'by`, `:.,+1j`, `:2,3>`, `:/foo/`,
        // `:*d` — `*` is `'<,'>`, the last visual selection).
        if cmd
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_digit() || b".$'%*+-/?<>,;".contains(b))
            || is_ranged_ex_name(cmd)
        {
            if let Some(action) = self.try_execute_ranged_command(cmd) {
                return action;
            }
        }

        // Handle :%{cmd} — whole-buffer range prefix (e.g. :%join, :%d)
        if let Some(rest) = cmd.strip_prefix('%') {
            let rest = rest.trim();
            let rest_normalized = normalize_ex_command(rest);
            match rest_normalized.as_ref() {
                "join" => {
                    let total_lines = self.buffer().len_lines();
                    if total_lines > 1 {
                        self.view_mut().cursor.line = 0;
                        self.view_mut().cursor.col = 0;
                        let mut changed = false;
                        self.join_lines(total_lines, &mut changed);
                    }
                    return EngineAction::None;
                }
                "delete" | "d" => {
                    let total_lines = self.buffer().len_lines();
                    self.view_mut().cursor.line = 0;
                    self.view_mut().cursor.col = 0;
                    let mut changed = false;
                    self.start_undo_group();
                    self.delete_lines(total_lines, &mut changed);
                    self.finish_undo_group();
                    return EngineAction::None;
                }
                "yank" | "y" => {
                    let total_lines = self.buffer().len_lines();
                    let saved = self.view().cursor;
                    self.view_mut().cursor.line = 0;
                    self.yank_lines(total_lines);
                    self.view_mut().cursor = saved;
                    return EngineAction::None;
                }
                _ => {
                    // Fall through for commands that handle % themselves (s/, norm, etc.)
                }
            }
        }

        // Handle :term / :terminal — open integrated terminal
        if cmd == "terminal" {
            return EngineAction::OpenTerminal;
        }

        // Handle :TerminalMaximize / :TerminalMax — toggle maximize on the
        // terminal panel. Backend supplies the available-row count.
        if cmd == "TerminalMaximize" || cmd == "TerminalMax" {
            return EngineAction::ToggleTerminalMaximize;
        }

        // Handle :CheckNerdFonts (issue #999) — TUI-discoverability command:
        // paint a sample row of nerd-font glyphs next to their ASCII
        // fallbacks and let the user say which one actually rendered,
        // persisting the answer as an explicit `use_nerd_fonts` override.
        // Platform-neutral: the dialog it opens is the same generic
        // `Engine::dialog` system every other confirm dialog uses (both
        // backends already render/key-handle it), so there is nothing
        // backend-specific to add here or on either backend's side.
        if cmd == "CheckNerdFonts" {
            self.show_check_nerd_fonts_dialog();
            return EngineAction::None;
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
                    // Look up matching extension by language_id ONLY. The old
                    // logic also matched on `m.dap.adapter == adapter_name`,
                    // but multiple extensions can ship the same adapter
                    // binary (codelldb is bundled by both `rust` and `cpp`),
                    // so `:DapInstall rust` would route to whichever
                    // extension was iterated first — usually `cpp`. That's a
                    // bug — when the user types a language, the extension
                    // claiming that language is the correct answer.
                    let manifests = self.ext_available_manifests();
                    let ext_name =
                        crate::core::extensions::find_manifest_for_language_id(&manifests, lang_id)
                            .map(|m| m.name.clone());
                    if let Some(name) = ext_name {
                        // Short-circuit the message-then-second-command chain:
                        // the user said install, just install. Mirrors
                        // `:ExtInstall <name>` exactly (including the
                        // pending_terminal_command handoff for the install
                        // command to run in a visible terminal pane).
                        self.ext_install_from_registry(&name);
                        if let Some(cmd) = self.pending_terminal_command.take() {
                            return EngineAction::RunInTerminal(cmd);
                        }
                        return EngineAction::None;
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

        // :Toast <text> — push a test toast. Diagnostic for #450 toast
        // wiring; verifies the render path independent of LSP events.
        if let Some(body) = cmd.strip_prefix("Toast ").map(|s| s.trim()) {
            self.push_toast("Test toast", body, quadraui::ToastSeverity::Info);
            return EngineAction::None;
        }
        if cmd == "Toast" {
            self.push_toast(
                "Test toast",
                "hello from :Toast",
                quadraui::ToastSeverity::Info,
            );
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

        // ── :map family — vim's per-mode key mapping commands (#1151) ─────────────
        // Before #1151, vimcode's only mapping command was its own invention,
        // `:map <mode> <keys> :<excmd>` — mode as a positional argument, target
        // always an ex command. Neither matches vim: vim selects the mode from
        // the *command name* (`:nnoremap`, `:imap`, …) and `{rhs}` can be a raw
        // key sequence, not just `:excmd`. This table drives that per-mode
        // dispatch; `build_keymap_entries` materializes it into the same
        // persisted-string format `settings.json` already used (so existing
        // `keymaps` entries, and code that reads them, keep working unmigrated).
        //
        // "x"/"s" are vim's Visual-only / Select-only letters. vimcode has no
        // separate Select mode, so `x` matches during `try_user_keymap`
        // alongside `v` and `s` is accepted here (parses, lists, unmaps) but
        // never active — see `Engine::active_keymap_modes`.
        const MAP_DEFINE_CMDS: &[(&str, &[&str], bool)] = &[
            ("nnoremap", &["n"], true),
            ("nmap", &["n"], false),
            ("vnoremap", &["v"], true),
            ("vmap", &["v"], false),
            ("xnoremap", &["x"], true),
            ("xmap", &["x"], false),
            ("onoremap", &["o"], true),
            ("omap", &["o"], false),
            ("inoremap", &["i"], true),
            ("imap", &["i"], false),
            ("cnoremap", &["c"], true),
            ("cmap", &["c"], false),
            ("snoremap", &["s"], true),
            ("smap", &["s"], false),
            // Bang forms target Insert+Command-line; bare forms target vim's
            // combined Normal+Visual+Select+Operator-pending.
            ("noremap!", &["i", "c"], true),
            ("map!", &["i", "c"], false),
            ("noremap", &["n", "v", "s", "o"], true),
            ("map", &["n", "v", "s", "o"], false),
        ];
        const MAP_UNMAP_CMDS: &[(&str, &[&str])] = &[
            ("nunmap", &["n"]),
            ("vunmap", &["v"]),
            ("xunmap", &["x"]),
            ("ounmap", &["o"]),
            ("iunmap", &["i"]),
            ("cunmap", &["c"]),
            ("sunmap", &["s"]),
            ("unmap!", &["i", "c"]),
            ("unmap", &["n", "v", "s", "o"]),
        ];
        const MAP_CLEAR_CMDS: &[(&str, &[&str])] = &[
            ("nmapclear", &["n"]),
            ("vmapclear", &["v"]),
            ("xmapclear", &["x"]),
            ("omapclear", &["o"]),
            ("imapclear", &["i"]),
            ("cmapclear", &["c"]),
            ("smapclear", &["s"]),
            // Bare `:mapclear` matches bare `:map`'s scope (n,v,s,o); `!`
            // matches `:map!`'s (i,c) — it does not mean "clear everything".
            ("mapclear!", &["i", "c"]),
            ("mapclear", &["n", "v", "s", "o"]),
        ];

        let (map_word, map_rest) = match cmd.find(' ') {
            Some(idx) => (&cmd[..idx], Some(cmd[idx + 1..].trim_start())),
            None => (cmd, None),
        };

        if let Some(&(_, modes, noremap)) =
            MAP_DEFINE_CMDS.iter().find(|(name, ..)| *name == map_word)
        {
            match map_rest {
                None | Some("") => {
                    // Bare command (any of the family) — list all user keymaps,
                    // same as pre-#1151 bare `:map`.
                    if self.settings.keymaps.is_empty() {
                        self.message = "No user keymaps defined".to_string();
                    } else {
                        self.message = self.settings.keymaps.join("  |  ");
                    }
                }
                Some(rest) => {
                    let mut parts = rest.splitn(2, ' ');
                    let lhs = parts.next().unwrap_or("");
                    let rhs = parts.next().unwrap_or("").trim();
                    match build_keymap_entries(lhs, rhs, modes, noremap) {
                        Some(entries) => {
                            let mut added = false;
                            for entry in entries {
                                if !self.settings.keymaps.contains(&entry) {
                                    self.settings.keymaps.push(entry);
                                    added = true;
                                }
                            }
                            if added {
                                let _ = self.settings.save();
                                self.rebuild_user_keymaps();
                            }
                            self.message = format!("Mapped: {lhs} -> {rhs}");
                        }
                        None => {
                            self.message = format!(
                                "Usage: :{map_word} {{lhs}} {{rhs}}  (e.g. :{map_word} jk <Esc>)"
                            );
                        }
                    }
                }
            }
            return EngineAction::None;
        }

        if let Some(&(_, modes)) = MAP_UNMAP_CMDS.iter().find(|(name, _)| *name == map_word) {
            match map_rest {
                None | Some("") => {
                    self.message = format!("Usage: :{map_word} {{lhs}}  (e.g. :{map_word} jk)");
                }
                Some(lhs) => {
                    let leader = self.settings.leader.to_string();
                    let target_keys = expand_leader_tokens(parse_key_sequence(lhs), &leader);
                    let before = self.settings.keymaps.len();
                    self.settings.keymaps.retain(|s| match parse_keymap_def(s) {
                        Some(km) => {
                            let km_keys = expand_leader_tokens(km.keys, &leader);
                            !(modes.contains(&km.mode.as_str()) && km_keys == target_keys)
                        }
                        None => true,
                    });
                    if self.settings.keymaps.len() < before {
                        let _ = self.settings.save();
                        self.rebuild_user_keymaps();
                        self.message = format!("Unmapped: {lhs}");
                    } else {
                        self.message = format!("No mapping found for: {lhs}");
                    }
                }
            }
            return EngineAction::None;
        }

        if let Some(&(_, modes)) = MAP_CLEAR_CMDS.iter().find(|(name, _)| *name == map_word) {
            // Buffer-local (`<buffer>`) mapclear is not modeled — vimcode has
            // no buffer-local keymaps to distinguish from global ones.
            let before = self.settings.keymaps.len();
            self.settings.keymaps.retain(|s| match parse_keymap_def(s) {
                Some(km) => !modes.contains(&km.mode.as_str()),
                None => true,
            });
            let removed = before - self.settings.keymaps.len();
            if removed > 0 {
                let _ = self.settings.save();
                self.rebuild_user_keymaps();
            }
            self.message = format!("Cleared {removed} mapping(s)");
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
                self.ai_send_message(msg.to_string());
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
                    // #222: re-sync the buffer with the LSP server so
                    // semantic_tokens (which override tree-sitter for
                    // Rust) are refreshed against the new content.
                    self.lsp_dirty_buffers.insert(buf_id, true);
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

            // Vim's `:set` takes several options at once (`:set ic scs`,
            // `:set noet ts=4`). Apply each in turn; a backslash escapes a
            // space inside a value (`:h :set`).
            if split_set_args(trimmed).len() > 1 {
                let mut last_err = None;
                for opt in split_set_args(trimmed) {
                    if let Err(e) = self.settings.parse_set_option(&opt) {
                        last_err = Some(e);
                    }
                }
                let _ = self.settings.save();
                if self.settings.spell {
                    self.ensure_spell_checker();
                }
                self.update_syntax();
                // `foldmethod=indent`/`foldlevel=N` (#1153) — recompute the
                // indent-fold hierarchy against the new level immediately,
                // mirroring `apply_foldlevel`'s own doc ("processing
                // deepest-first") rather than waiting for the next `z`
                // command. Idempotent (a plain re-close/re-define pass), so
                // it's safe to re-run whenever either option was actually
                // touched on this `:set` line — gated on that (rather than
                // unconditionally on every `:set`) so an unrelated option
                // like `:set ic` doesn't pay to recompute fold state on
                // large files (#1153 review).
                if self.settings.foldmethod == "indent"
                    && split_set_args(trimmed)
                        .iter()
                        .any(|a| set_arg_touches_folding(a))
                {
                    self.apply_foldlevel(self.settings.foldlevel);
                }
                return match last_err {
                    Some(e) => {
                        self.message = e;
                        EngineAction::Error
                    }
                    None => {
                        self.message = trimmed.to_string();
                        EngineAction::None
                    }
                };
            }

            let prev_syntax_max_lines = self.settings.syntax_max_lines;
            // Queries (`:set foo?`) don't mutate, so skip the disk save —
            // both to avoid the unnecessary I/O and so the TUI mtime watcher
            // doesn't trigger and clobber the query's result message.
            let is_query = trimmed.ends_with('?');
            match self.settings.parse_set_option(trimmed) {
                Ok(msg) => {
                    if !is_query {
                        if let Err(e) = self.settings.save() {
                            self.message = format!("Setting changed but failed to save: {e}");
                        } else {
                            self.message = msg;
                        }
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
            // If the syntax-highlighting threshold changed, re-parse the
            // active buffer so the new limit takes effect immediately.
            // (The atomic was synced inside `set_value_str`; this picks up
            // newly-enabled or newly-disabled highlighting on the visible
            // buffer.)
            if self.settings.syntax_max_lines != prev_syntax_max_lines {
                self.update_syntax();
            }
            // See the matching comment in the multi-option branch above
            // (#1153).
            if self.settings.foldmethod == "indent" && set_arg_touches_folding(trimmed) {
                self.apply_foldlevel(self.settings.foldlevel);
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

        // `[range]:g[!]/pat/cmd` and `[range]:v/pat/cmd` — global commands.
        if let Some(action) = self.try_execute_global(cmd) {
            return action;
        }

        // `[range]s/pattern/replacement/flags`, `:&`, `:&&`, `:~`.
        if let Some(action) = self.try_execute_substitute(cmd) {
            return action;
        }

        // :sort [flags] — sort lines in buffer.
        // Accept both ":sort" with space-separated flags and the Vim ":sort!" bang.
        // `!` reverses the sort direction; it is a separate axis from the `r`
        // letter flag (which selects the pattern *match* as the sort key
        // instead of the text after it) — the two used to be conflated here,
        // which broke `:sort /pat/ r` (#879).
        if cmd == "sort" || cmd == "sort!" || cmd.starts_with("sort ") || cmd.starts_with("sort!") {
            // Normalize: strip "sort" and an optional '!'.
            let after = cmd.strip_prefix("sort").unwrap_or("");
            let (bang, after) = if let Some(rest) = after.strip_prefix('!') {
                (true, rest)
            } else {
                (false, after)
            };
            return self.execute_sort_command(None, bang, after.trim());
        }

        // :m[ove] {dest} / :t {dest} / :co[py] {dest} — operate on current line.
        // Accept both the space-separated form (":move 3") and the concatenated
        // digit-suffix form (":m3", ":t3", ":co3") that Vim supports.
        if let Some(dest) = split_cmd_and_arg(cmd, "move").or_else(|| split_cmd_and_arg(cmd, "m")) {
            return self.execute_move_command(dest);
        }
        if let Some(dest) = split_cmd_and_arg(cmd, "copy")
            .or_else(|| split_cmd_and_arg(cmd, "co"))
            .or_else(|| split_cmd_and_arg(cmd, "t"))
        {
            return self.execute_copy_command(dest);
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
            let (shell, flag) = shell_command();
            match std::process::Command::new(shell)
                .arg(flag)
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

        // Handle :r[ead] {file} / :r[ead] !{cmd} — read a file, or the stdout
        // of a shell command, and insert it after the cursor line (#879).
        if let Some(arg) = cmd.strip_prefix("read ").map(|s| s.trim()) {
            if let Some(shell_cmd) = arg.strip_prefix('!') {
                let shell_cmd = shell_cmd.trim();
                if shell_cmd.is_empty() {
                    self.message = "Usage: :r !command".to_string();
                    return EngineAction::None;
                }
                let (shell, flag) = shell_command();
                match std::process::Command::new(shell)
                    .arg(flag)
                    .arg(shell_cmd)
                    .output()
                {
                    Ok(output) => {
                        let content = String::from_utf8_lossy(&output.stdout).to_string();
                        let inserted_lines = self.insert_read_content(&content);
                        self.message = format!("{} line(s) read", inserted_lines);
                    }
                    Err(e) => {
                        self.message = format!("Cannot run shell command: {}", e);
                    }
                }
                return EngineAction::None;
            }
            let path = if Path::new(arg).is_absolute() {
                PathBuf::from(arg)
            } else {
                self.cwd.join(arg)
            };
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let inserted_lines = self.insert_read_content(&content);
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
                    return self.set_ex_mark(ch, cursor);
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
            self.indent_lines(line, 1, &mut changed, true);
            return EngineAction::None;
        }

        // Handle :< — shift left
        if cmd == "<" {
            let line = self.view().cursor.line;
            let mut changed = false;
            self.dedent_lines(line, 1, &mut changed, true);
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
                    if !self.buffer_has_other_views(buf_id, current_win) {
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
                    if let Some((content, ty)) = self.registers.get(&r).cloned() {
                        let kind = reg_type_letter(ty);
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
                    if let Some((content, ty)) = self.registers.get(&c).cloned() {
                        let kind = reg_type_letter(ty);
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
                lines.push(" jump line  col  tab  file/text".to_string());
                for (i, entry) in self.jump_list.iter().enumerate() {
                    let marker = if i == self.jump_list_pos { ">" } else { " " };
                    let path_str = entry
                        .file
                        .as_ref()
                        .map(|p| {
                            p.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    // "tab" column: the recorded pane's TabId when it still
                    // exists (i.e. `Ctrl-O`/`Ctrl-I` would switch to it),
                    // or "x" when that tab/split has since been closed and
                    // this entry would fall back to reopening `file` (#674).
                    let tab_str = if self
                        .locate_jump_pane(entry.group_id, entry.tab_id, entry.window_id)
                        .is_some()
                    {
                        entry.tab_id.0.to_string()
                    } else {
                        "x".to_string()
                    };
                    lines.push(format!(
                        "{} {:4}  {:4}  {:3}  {:>3}  {}",
                        marker,
                        i,
                        entry.line + 1,
                        entry.col,
                        tab_str,
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
                // Open the unified find/replace overlay
                self.open_find_replace();
                EngineAction::None
            }
            "replace" => {
                // Open the unified find/replace overlay with replace row visible
                self.open_find_replace();
                self.find_replace_show_replace = true;
                EngineAction::None
            }
            "sidebar" => {
                self.toggle_sidebar();
                EngineAction::None
            }
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
                // No-argument invocation — GTK/TUI menu "File: Save As…" and
                // the command palette both route here via
                // dispatch_menu_action/PickerAction::ExecuteCommand. Rather
                // than a per-backend native dialog, pre-fill the command
                // line with the current path for interactive editing, same
                // platform-neutral pattern as `:Rename` with no argument
                // above (#585).
                let current = self
                    .file_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.mode = Mode::Command;
                self.command_buffer = format!("saveas {current}");
                self.command_cursor = self.command_buffer.chars().count();
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
                self.registers
                    .insert('"', (text.clone(), RegType::Linewise));
                self.registers.insert('0', (text, RegType::Linewise));
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
                let pct = (cur_line * 100).checked_div(total).unwrap_or(0);
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
            "cquit" | "cquit!" => EngineAction::QuitWithError,
            _ => {
                // Handle :y[ank] {register} and :pu[t] {register} with args
                if let Some(arg) = cmd.strip_prefix("yank ") {
                    let arg = arg.trim();
                    let reg = arg.chars().next().unwrap_or('"');
                    let line = self.view().cursor.line;
                    let text = self.buffer().content.line(line).chars().collect::<String>();
                    self.registers
                        .insert(reg, (text.clone(), RegType::Linewise));
                    if reg != '"' {
                        self.registers.insert('"', (text, RegType::Linewise));
                    }
                    return EngineAction::None;
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

    /// `:[range]norm[al][!] {keys}` — the range is a full ex range, so
    /// `:2normal $`, `:%normal Ax` and `:'a,'bnormal .` all work.
    pub(crate) fn try_execute_norm(&mut self, cmd: &str) -> Option<EngineAction> {
        let chars: Vec<char> = cmd.chars().collect();
        let (range, consumed) = self.parse_ex_range(&chars);
        let rest: String = chars[consumed..].iter().collect();
        let keys = rest
            .strip_prefix("normal! ")
            .or_else(|| rest.strip_prefix("normal "))
            .or_else(|| rest.strip_prefix("norm! "))
            .or_else(|| rest.strip_prefix("norm "))?
            .to_string();
        let last = self.buffer().len_lines().saturating_sub(1);
        let (start, end) = match range {
            Some((a, b)) => ((a.max(0) as usize).min(last), (b.max(0) as usize).min(last)),
            None => {
                let l = self.view().cursor.line;
                (l, l)
            }
        };
        Some(self.execute_norm_range(start, end, &keys))
    }

    pub(crate) fn execute_norm_range(
        &mut self,
        start_line: usize,
        end_line: usize,
        keys: &str,
    ) -> EngineAction {
        if keys.is_empty() {
            self.message = "Usage: :norm[al][!] {keys}".to_string();
            return EngineAction::Error;
        }

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

            // `:normal @a` must play the macro back *now*, on this line — the
            // UI normally pumps the queue between keystrokes, and there is no
            // pump inside an ex command.
            self.drain_macro_queue();

            // Vim ends an unterminated `:normal` insert as if <Esc> were typed,
            // which shifts the cursor one column left (`:normal Ax` → col 2).
            if self.mode != Mode::Normal {
                self.handle_key("Escape", None, false);
            }
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
    /// Try to run `cmd` as `[range]g[!]/pat/cmd` or `[range]v/pat/cmd`.
    ///
    /// Returns `None` when `cmd` is not a global command at all.
    pub(crate) fn try_execute_global(&mut self, cmd: &str) -> Option<EngineAction> {
        let chars: Vec<char> = cmd.chars().collect();
        let (range, consumed) = self.parse_ex_range(&chars);
        let rest: String = chars[consumed..].iter().collect();

        // `g`, `gl`, ... `global` (with an optional `!`), or `v` / `vglobal`.
        let (invert, after) = match rest.strip_prefix("g!") {
            Some(a) => (true, a),
            None => match strip_command_name(&rest, "global") {
                Some(a) => (false, a),
                None => (true, strip_command_name(&rest, "vglobal")?),
            },
        };

        let delim = after.chars().next()?;
        if delim.is_alphanumeric() || matches!(delim, '\\' | '"' | '|' | ' ') {
            return None;
        }
        Some(self.execute_global_command(range, after, delim, invert))
    }

    /// `:[range]g/pat/cmd` — run an ex command on every matching line.
    ///
    /// Vim marks the matching lines first and then executes in **forward**
    /// order, which is what makes `:g/^/m0` reverse the buffer. Because we have
    /// no per-line marks that survive edits, the remaining line numbers are
    /// shifted by the net line-count delta of each sub-command — enough for
    /// `d`, `m`, `t`, `j`, `s` and `normal`, which is what `:g` is used for.
    pub(crate) fn execute_global_command(
        &mut self,
        range: Option<(isize, isize)>,
        after: &str,
        delim: char,
        invert: bool,
    ) -> EngineAction {
        // Split `/pat/cmd` on the first unescaped delimiter after the pattern.
        let chars: Vec<char> = after.chars().collect();
        let mut i = 1;
        let mut pattern = String::new();
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                pattern.push(chars[i]);
                pattern.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if chars[i] == delim {
                i += 1;
                break;
            }
            pattern.push(chars[i]);
            i += 1;
        }
        let subcmd: String = chars[i.min(chars.len())..].iter().collect();
        let subcmd = subcmd.trim().to_string();
        let subcmd = if subcmd.is_empty() {
            "p".to_string()
        } else {
            subcmd
        };

        // An empty pattern reuses the last search pattern; a non-empty one
        // *becomes* it, so `:g/a/s//x/` works.
        let pattern = if pattern.is_empty() {
            self.search_query.clone()
        } else {
            self.search_query = pattern.clone();
            self.search_smartcase_applies = true;
            pattern
        };
        if pattern.is_empty() {
            self.message = "E35: No previous regular expression".to_string();
            return EngineAction::Error;
        }
        let compiled = match self.compile_vim_pattern(&pattern, true) {
            Ok(c) => c,
            Err(e) => {
                self.message = e;
                return EngineAction::Error;
            }
        };

        let num_lines = self.buffer().len_lines();
        let (first, last) = match range {
            Some((a, b)) => (
                a.max(0) as usize,
                (b.max(0) as usize).min(num_lines.saturating_sub(1)),
            ),
            None => (0, num_lines.saturating_sub(1)),
        };

        let mut matching: Vec<usize> = Vec::new();
        for line_idx in first..=last {
            if line_idx >= num_lines {
                break;
            }
            let line_text: String = self.buffer().content.line(line_idx).chars().collect();
            let line_text = line_text.trim_end_matches('\n');
            let matches = compiled.is_match(line_text);
            if matches != invert {
                matching.push(line_idx);
            }
        }

        if matching.is_empty() {
            self.message = format!("E486: Pattern not found: {pattern}");
            return EngineAction::Error;
        }

        let mut executed = 0usize;
        let mut pending: Vec<isize> = matching.iter().map(|&l| l as isize).collect();
        let mut idx = 0usize;
        // `:g` is one undoable step in Vim, however many lines it touches —
        // save the undo depth so the per-line sub-command entries below can
        // be merged into a single one (#886). A bare `start_undo_group()`
        // here doesn't work: the first sub-command that itself calls
        // `start_undo_group()` (nearly all of them — `d`, `s`, `normal`, …)
        // immediately finishes this outer group (empty, so it's discarded)
        // and starts its own, so without the merge below `u` only reverts
        // the *last* matching line, and the buffer + cursor are both wrong.
        let saved_undo_len = self.active_buffer_state_mut().undo_stack.len();
        while idx < pending.len() {
            let line = pending[idx];
            idx += 1;
            if line < 0 {
                continue;
            }
            let line = line as usize;
            let before = self.buffer().len_lines();
            if line >= before {
                continue;
            }
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = 0;
            self.execute_command(&subcmd);
            executed += 1;
            let delta = self.buffer().len_lines() as isize - before as isize;
            if delta != 0 {
                for l in pending[idx..].iter_mut() {
                    if *l > line as isize {
                        *l += delta;
                    }
                }
            }
        }
        // Finalize the last open undo group (e.g. from a trailing insert-mode
        // sub-command).
        self.active_buffer_state_mut().finish_undo_group();

        // Merge every undo entry created by the sub-commands above into a
        // single step, so `u` reverts all of `:g`'s edits at once and lands
        // on the position of the *first* one (#886).
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

        let max_line = self.buffer().len_lines().saturating_sub(1);
        if self.view().cursor.line > max_line {
            self.view_mut().cursor.line = max_line;
        }
        self.clamp_cursor_col();

        self.message = format!(
            "{} line{} affected",
            executed,
            if executed == 1 { "" } else { "s" }
        );
        EngineAction::None
    }

    /// `:[range]sor[t][!] [i][u][r][n] [/{pattern}/]` — sort lines.
    ///
    /// `range` is a 0-based inclusive `(start, end)` line span, or `None` for
    /// the whole buffer. `bang` reverses the sort direction (Vim's `:sort!`);
    /// it is a separate axis from the `r` letter flag inside `spec`, which
    /// (only meaningful together with a `/pattern/`) selects the *matched*
    /// text as the sort key instead of the text following the match — mixing
    /// the two up is what made `:sort /pat/ r` sort on whole lines (#879).
    pub(crate) fn execute_sort_command(
        &mut self,
        range: Option<(usize, usize)>,
        bang: bool,
        spec: &str,
    ) -> EngineAction {
        let (letter_flags, pattern) = parse_sort_spec(spec);
        let numeric = letter_flags.contains('n');
        let unique = letter_flags.contains('u');
        let ignorecase = letter_flags.contains('i');
        let use_match = letter_flags.contains('r');
        let reverse = bang;

        let num_lines = self.buffer().len_lines();
        if num_lines == 0 {
            return EngineAction::None;
        }
        let (start, end) = match range {
            Some((s, e)) => (s.min(num_lines - 1), e.min(num_lines - 1)),
            None => (0, num_lines - 1),
        };
        if start > end {
            return EngineAction::None;
        }

        let compiled = match pattern.as_deref() {
            Some(pat) if !pat.is_empty() => match self.compile_vim_pattern(pat, true) {
                Ok(c) => Some(c),
                Err(e) => {
                    self.message = e;
                    return EngineAction::None;
                }
            },
            _ => None,
        };

        // Collect the lines in range (excluding trailing newline per line).
        let mut lines: Vec<String> = (start..=end)
            .map(|i| {
                let s: String = self.buffer().content.line(i).chars().collect();
                if s.ends_with('\n') {
                    s[..s.len() - 1].to_string()
                } else {
                    s
                }
            })
            .collect();

        // The sort key is the whole line, unless `/pattern/` narrows it to
        // either the matched text (`r`) or the text after the match. A line
        // the pattern doesn't match sorts as the empty key.
        let key_of = |line: &str| -> String {
            match &compiled {
                Some(compiled) => match compiled.captures(line) {
                    Some(caps) => {
                        let (s, e) = compiled.span(&caps);
                        if use_match {
                            line[s..e].to_string()
                        } else {
                            line[e..].to_string()
                        }
                    }
                    None => String::new(),
                },
                None => line.to_string(),
            }
        };

        if numeric {
            lines.sort_by(|a, b| {
                let na: i64 = key_of(a).trim().parse().unwrap_or(i64::MIN);
                let nb: i64 = key_of(b).trim().parse().unwrap_or(i64::MIN);
                let ord = na.cmp(&nb);
                if reverse {
                    ord.reverse()
                } else {
                    ord
                }
            });
        } else {
            lines.sort_by(|a, b| {
                let (ka, kb) = (key_of(a), key_of(b));
                let (ka, kb) = if ignorecase {
                    (ka.to_lowercase(), kb.to_lowercase())
                } else {
                    (ka, kb)
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
                let (ka, kb) = (key_of(a), key_of(b));
                if ignorecase {
                    ka.to_lowercase() == kb.to_lowercase()
                } else {
                    ka == kb
                }
            });
        }

        // Replace only the sorted range, leaving the rest of the buffer
        // untouched — `lines.join("\n")` reproduces every interior newline;
        // only the trailing one (present unless `end` is the buffer's final,
        // newline-less line) needs to be re-added explicitly.
        let last_raw: String = self.buffer().content.line(end).chars().collect();
        let trailing = if last_raw.ends_with('\n') { "\n" } else { "" };
        let replacement = format!("{}{trailing}", lines.join("\n"));

        let range_start_char = self.buffer().line_to_char(start);
        let range_end_char = if end + 1 < num_lines {
            self.buffer().line_to_char(end + 1)
        } else {
            self.buffer().len_chars()
        };
        self.start_undo_group();
        if range_end_char > range_start_char {
            self.delete_with_undo(range_start_char, range_end_char);
        }
        self.insert_with_undo(range_start_char, &replacement);
        self.finish_undo_group();
        self.view_mut().cursor.line = start;
        self.view_mut().cursor.col = 0;
        self.message = format!("{} lines sorted", lines.len());
        EngineAction::None
    }

    /// Insert `content` (a file's contents, or a shell command's stdout) after
    /// the cursor line, the way `:r[ead]` does, and leave the cursor on the
    /// *last* inserted line at column 0 — the same place `:put` lands, per
    /// Vim (`:h :read` cursor behaviour differs from a plain paste only in
    /// having no register to consult). Returns the number of lines inserted.
    pub(crate) fn insert_read_content(&mut self, content: &str) -> usize {
        if content.is_empty() {
            return 0;
        }
        let line = self.view().cursor.line;
        let num_lines = self.buffer().len_lines();
        let inserted_lines = content.lines().count().max(1);
        self.start_undo_group();
        let first_new_line = if line + 1 < num_lines {
            let insert_pos = self.buffer().line_to_char(line + 1);
            self.insert_with_undo(insert_pos, content);
            line + 1
        } else {
            let end = self.buffer().len_chars();
            // Ensure there's a newline before inserting, so the new text
            // lands on its own line(s) instead of extending the last one.
            if end > 0 && self.buffer().content.char(end - 1) != '\n' {
                self.insert_with_undo(end, "\n");
                self.insert_with_undo(end + 1, content);
            } else {
                self.insert_with_undo(end, content);
            }
            num_lines
        };
        self.finish_undo_group();
        let new_last = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = (first_new_line + inserted_lines - 1).min(new_last);
        self.view_mut().cursor.col = 0;
        inserted_lines
    }

    /// :m[ove] {dest} — move current line to after line {dest}.
    /// dest: absolute line number (1-based), 0 = before first line, . = current, $ = last, +N/-N = relative.
    /// `:[range]pu[t][!] [x]` — put register `x` linewise after the 0-based
    /// line `target` (or before it, with `bang`). `target == -1` is Vim's
    /// address `0`, "before the first line" (`:0put`).
    pub(crate) fn execute_put(&mut self, target: isize, bang: bool, reg: char) -> EngineAction {
        let Some((content, _)) = self.registers.get(&reg).cloned() else {
            self.message = if reg == '"' {
                "Register is empty".to_string()
            } else {
                format!("Register '{reg}' is empty")
            };
            return EngineAction::None;
        };
        let text = if content.ends_with('\n') {
            content
        } else {
            format!("{content}\n")
        };
        let n_lines = text.matches('\n').count().max(1);
        let num_lines = self.buffer().len_lines();
        let last_line = num_lines.saturating_sub(1);

        self.start_undo_group();
        let first_new_line = if bang {
            // `:put!` — insert *before* `target` (clamped to the top line).
            let base = target.max(0) as usize;
            let insert_pos = self.buffer().line_to_char(base.min(last_line));
            self.insert_with_undo(insert_pos, &text);
            base
        } else {
            // `:put` — insert *after* `target`; `-1` means "before line 1".
            let base = (target + 1).max(0) as usize;
            if base < num_lines {
                let insert_pos = self.buffer().line_to_char(base);
                self.insert_with_undo(insert_pos, &text);
            } else {
                // Appending past the true end of the buffer: if the last
                // char isn't a newline, insert one first so the put text
                // lands on its own line(s) instead of extending the last one.
                let end = self.buffer().len_chars();
                if end > 0 && self.buffer().content.char(end - 1) != '\n' {
                    self.insert_with_undo(end, "\n");
                    self.insert_with_undo(end + 1, &text);
                } else {
                    self.insert_with_undo(end, &text);
                }
            }
            base
        };
        self.finish_undo_group();
        let new_last = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = (first_new_line + n_lines - 1).min(new_last);
        self.view_mut().cursor.col = 0;
        EngineAction::None
    }

    /// Set mark `ch` to `cursor` — shared by the plain `:mark {a-zA-Z}` /
    /// `:k{a-zA-Z}` form (mark at the cursor, full column) and the ranged
    /// form (`:{addr}mark {a-zA-Z}` / `:{addr}k{a-zA-Z}`, mark at `{addr}`
    /// column 0).
    pub(crate) fn set_ex_mark(&mut self, ch: char, cursor: Cursor) -> EngineAction {
        let line = cursor.line.min(self.buffer().len_lines().saturating_sub(1));
        let cursor = Cursor { line, ..cursor };
        if ch.is_ascii_lowercase() {
            let buf_id = self.active_buffer_id();
            self.marks.entry(buf_id).or_default().insert(ch, cursor);
        } else {
            let path = self.file_path().map(|p| p.to_path_buf());
            self.global_marks
                .insert(ch, (path, cursor.line, cursor.col));
        }
        self.message = format!("Mark '{ch}' set");
        EngineAction::None
    }

    /// `:[range]m[ove] {addr}` — move lines to after `{addr}`.
    pub(crate) fn execute_move_command(&mut self, dest: &str) -> EngineAction {
        let cur = self.view().cursor.line;
        self.ex_copy_move(cur, cur, dest, true)
    }

    /// `:[range]t` / `:[range]co[py] {addr}` — copy lines to after `{addr}`.
    pub(crate) fn execute_copy_command(&mut self, dest: &str) -> EngineAction {
        let cur = self.view().cursor.line;
        self.ex_copy_move(cur, cur, dest, false)
    }

    pub(crate) fn execute_move_range(
        &mut self,
        start: usize,
        end: usize,
        dest: &str,
    ) -> EngineAction {
        self.ex_copy_move(start, end, dest, true)
    }

    pub(crate) fn execute_copy_range(
        &mut self,
        start: usize,
        end: usize,
        dest: &str,
    ) -> EngineAction {
        self.ex_copy_move(start, end, dest, false)
    }

    /// Shared implementation of `:t` / `:co[py]` and `:m[ove]`.
    ///
    /// `dest` is a full ex address, so `$`, `.`, `+N`, `-N`, `'a` and `/pat/`
    /// all work. Vim's address `0` means "before the first line", which the old
    /// 0-based-usize address helper could not express — hence the `isize` here,
    /// where `-1` is that "before line 1" position.
    fn ex_copy_move(
        &mut self,
        start: usize,
        end: usize,
        dest: &str,
        is_move: bool,
    ) -> EngineAction {
        let n = self.buffer().len_lines();
        if n == 0 || start > end || start >= n {
            return EngineAction::None;
        }
        let end = end.min(n - 1);

        let dest_chars: Vec<char> = dest.chars().collect();
        let mut di = 0usize;
        let cur = self.view().cursor.line;
        let Some(mut dest_line) = self.parse_ex_address(&dest_chars, &mut di, cur) else {
            self.message = format!("E14: Invalid address: {dest}");
            return EngineAction::Error;
        };

        if is_move && dest_line >= start as isize && dest_line < end as isize {
            self.message = "E134: Cannot move a range of lines into itself".to_string();
            return EngineAction::Error;
        }

        let mut lines: Vec<String> = (0..n)
            .map(|i| {
                let s: String = self.buffer().content.line(i).chars().collect();
                s.trim_end_matches('\n').to_string()
            })
            .collect();
        let block: Vec<String> = lines[start..=end].to_vec();

        if is_move {
            lines.drain(start..=end);
            if dest_line > end as isize {
                dest_line -= (end - start + 1) as isize;
            }
        }

        // `dest_line` is the line the block goes *after*; `-1` is the very top.
        let insert_at = (dest_line + 1).max(0) as usize;
        let insert_at = insert_at.min(lines.len());
        let block_len = block.len();
        for (k, line) in block.into_iter().enumerate() {
            lines.insert(insert_at + k, line);
        }

        let had_trailing_newline = {
            let len = self.buffer().len_chars();
            len == 0 || self.buffer().content.char(len - 1) == '\n'
        };
        let mut new_text = lines.join("\n");
        if had_trailing_newline {
            new_text.push('\n');
        }
        self.splice_buffer_text(&new_text);

        // Vim leaves the cursor on the last copied/moved line.
        let target = (insert_at + block_len - 1).min(self.buffer().len_lines().saturating_sub(1));
        self.view_mut().cursor.line = target;
        self.view_mut().cursor.col = self.first_non_blank_col(target);
        self.clamp_cursor_col();
        EngineAction::None
    }

    /// Parse a line address string to a 0-based line index.
    ///
    /// Bare numeric addresses are **1-based** (matches Vim's convention): `"1"`
    /// → index 0, `"3"` → index 2. The special address `"0"` means "before line 1"
    /// — callers that use the result as an insert point should treat 0 as "insert
    /// at the very top". For callers that use the result as a cursor line,
    /// clamping to 0 is already safe.
    ///
    /// Supports: `"0"` (before first line), `"1"`-`"N"` (1-based absolute),
    /// `"."` (current), `"$"` (last), `"+N"`/`"-N"` (relative to current).
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
            // 1-based absolute line index. "0" maps to index 0 and is treated
            // specially by copy/move callers as "before line 1".
            if n == 0 {
                return 0;
            }
            return (n - 1).min(total.saturating_sub(1));
        }
        current
    }

    // --- Ex address / range parsing (`:h :range`) -------------------------

    /// Parse one ex address starting at `*i`, returning a **0-based** line.
    ///
    /// `-1` is Vim's line `0` ("before the first line"), which `:m` / `:t` /
    /// `:put` treat as "insert at the very top".
    ///
    /// Handles `N`, `.`, `$`, `'m`, `'<`, `'>`, `/pat[/]`, `?pat[?]`, `\/`,
    /// `\?`, each optionally followed by any number of `+N` / `-N` offsets.
    /// Returns `None` when there is no address here at all.
    pub(crate) fn parse_ex_address(
        &self,
        chars: &[char],
        i: &mut usize,
        current: usize,
    ) -> Option<isize> {
        let last = self.buffer().len_lines().saturating_sub(1) as isize;
        let skip_ws = |i: &mut usize| {
            while chars.get(*i) == Some(&' ') {
                *i += 1;
            }
        };
        skip_ws(i);

        let mut base: Option<isize> = match chars.get(*i) {
            Some(c) if c.is_ascii_digit() => {
                let mut n = 0usize;
                while let Some(d) = chars.get(*i).and_then(|c| c.to_digit(10)) {
                    n = n * 10 + d as usize;
                    *i += 1;
                }
                Some(n as isize - 1)
            }
            Some('.') => {
                *i += 1;
                Some(current as isize)
            }
            Some('$') => {
                *i += 1;
                Some(last)
            }
            Some('\'') => {
                let m = *chars.get(*i + 1)?;
                let line = self.ex_mark_line(m)?;
                *i += 2;
                Some(line as isize)
            }
            Some('/') | Some('?') => {
                let delim = chars[*i];
                *i += 1;
                let mut pat = String::new();
                while *i < chars.len() {
                    if chars[*i] == '\\' && *i + 1 < chars.len() {
                        pat.push(chars[*i]);
                        pat.push(chars[*i + 1]);
                        *i += 2;
                        continue;
                    }
                    if chars[*i] == delim {
                        *i += 1;
                        break;
                    }
                    pat.push(chars[*i]);
                    *i += 1;
                }
                Some(self.ex_search_address(&pat, delim == '/', current)? as isize)
            }
            Some('\\') if matches!(chars.get(*i + 1), Some('/') | Some('?')) => {
                let forward = chars[*i + 1] == '/';
                *i += 2;
                Some(self.ex_search_address("", forward, current)? as isize)
            }
            _ => None,
        };

        // Trailing `+N` / `-N` offsets, which may appear with no base at all
        // (`:+2`, `:-1`) in which case they are relative to the current line.
        loop {
            skip_ws(i);
            match chars.get(*i) {
                Some(&sign @ ('+' | '-')) => {
                    *i += 1;
                    let mut n = 0usize;
                    let mut saw_digit = false;
                    while let Some(d) = chars.get(*i).and_then(|c| c.to_digit(10)) {
                        n = n * 10 + d as usize;
                        saw_digit = true;
                        *i += 1;
                    }
                    let step = if saw_digit { n as isize } else { 1 };
                    let from = base.unwrap_or(current as isize);
                    base = Some(if sign == '+' {
                        from + step
                    } else {
                        from - step
                    });
                }
                _ => break,
            }
        }

        base.map(|b| b.clamp(-1, last.max(0)))
    }

    /// Line of an ex mark reference (`'a`, `'<`, `'>`), 0-based.
    fn ex_mark_line(&self, m: char) -> Option<usize> {
        match m {
            '<' => self
                .visual_mark_start
                .map(|(l, _)| l)
                .or_else(|| self.get_visual_selection_range().map(|(s, _)| s.line)),
            '>' => self
                .visual_mark_end
                .map(|(l, _)| l)
                .or_else(|| self.get_visual_selection_range().map(|(_, e)| e.line)),
            '\'' => self.last_jump_pos.map(|(l, _)| l),
            c if c.is_ascii_uppercase() => self.global_marks.get(&c).map(|&(_, l, _)| l),
            c => self
                .marks
                .get(&self.active_window().buffer_id)
                .and_then(|m| m.get(&c))
                .map(|c| c.line),
        }
        .map(|l| l.min(self.buffer().len_lines().saturating_sub(1)))
    }

    /// Resolve a `/pat/` or `?pat?` ex address to a 0-based line, wrapping.
    fn ex_search_address(&self, pat: &str, forward: bool, current: usize) -> Option<usize> {
        let pat = if pat.is_empty() {
            self.search_query.clone()
        } else {
            pat.to_string()
        };
        let compiled = self.compile_vim_pattern(&pat, true).ok()?;
        let text = self.buffer().to_string();
        let spans = Self::collect_match_spans(&compiled, &text);
        let lines: Vec<usize> = spans
            .iter()
            .map(|&(b, _)| {
                self.buffer()
                    .content
                    .char_to_line(self.buffer().content.byte_to_char(b))
            })
            .collect();
        if forward {
            lines
                .iter()
                .find(|&&l| l > current)
                .copied()
                .or_else(|| lines.first().copied())
        } else {
            lines
                .iter()
                .rev()
                .find(|&&l| l < current)
                .copied()
                .or_else(|| lines.last().copied())
        }
    }

    /// Parse a leading ex range, returning the 0-based inclusive line range and
    /// the number of characters consumed.
    ///
    /// `%` is the whole buffer; `a,b` and `a;b` are two addresses (with `;`
    /// moving the current line to `a` before `b` is parsed, per `:h :;`).
    pub(crate) fn parse_ex_range(&self, chars: &[char]) -> (Option<(isize, isize)>, usize) {
        let mut i = 0usize;
        while chars.get(i) == Some(&' ') {
            i += 1;
        }
        if chars.get(i) == Some(&'%') {
            let last = self.buffer().len_lines().saturating_sub(1) as isize;
            return (Some((0, last)), i + 1);
        }
        // `*` is shorthand for `'<,'>` — the last visual selection — and,
        // like `%`, stands for a whole range rather than a single address
        // (`:h :star`).
        if chars.get(i) == Some(&'*') {
            return match (self.ex_mark_line('<'), self.ex_mark_line('>')) {
                (Some(s), Some(e)) => {
                    let (s, e) = if s <= e { (s, e) } else { (e, s) };
                    (Some((s as isize, e as isize)), i + 1)
                }
                _ => (None, 0),
            };
        }
        let current = self.view().cursor.line;
        let Some(first) = self.parse_ex_address(chars, &mut i, current) else {
            return (None, 0);
        };
        let mut start = first;
        let mut end = first;
        while matches!(chars.get(i), Some(',') | Some(';')) {
            let semi = chars[i] == ';';
            i += 1;
            let base = if semi {
                end.max(0) as usize
            } else {
                self.view().cursor.line
            };
            match self.parse_ex_address(chars, &mut i, base) {
                Some(next) => {
                    start = end;
                    end = next;
                }
                None => {
                    start = end;
                    end = base as isize;
                }
            }
        }
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        (Some((start, end)), i)
    }

    // --- :substitute ------------------------------------------------------

    /// Try to run `cmd` as `[range]s/pat/repl/[flags] [count]`, `:&`, `:&&` or
    /// `:~`. Returns `None` when `cmd` is some other ex command entirely, so
    /// the caller can keep dispatching.
    pub(crate) fn try_execute_substitute(&mut self, cmd: &str) -> Option<EngineAction> {
        let chars: Vec<char> = cmd.chars().collect();
        let (range, consumed) = self.parse_ex_range(&chars);
        let rest: String = chars[consumed..].iter().collect();

        // `:&`, `:&&` and `:~` repeat the previous substitution.
        if let Some(tail) = rest.strip_prefix('&') {
            let (keep_flags, tail) = match tail.strip_prefix('&') {
                Some(t) => (true, t),
                None => (false, tail),
            };
            return Some(self.repeat_last_substitute(range, keep_flags, tail.trim()));
        }
        if let Some(tail) = rest.strip_prefix('~') {
            return Some(self.repeat_last_substitute(range, false, tail.trim()));
        }

        // Longest prefix of `rest` that is also a prefix of "substitute".
        let name_len = "substitute"
            .char_indices()
            .take_while(|&(k, c)| rest.chars().nth(k) == Some(c))
            .count();
        if name_len == 0 {
            return None;
        }
        let after: String = rest.chars().skip(name_len).collect();
        let delim = after.chars().next();

        match delim {
            // `:s` / `:s 3` / `:s g` — repeat with the previous pattern.
            None => Some(self.repeat_last_substitute(range, false, "")),
            Some(' ') => Some(self.repeat_last_substitute(range, false, after.trim())),
            // A delimiter is any non-alphanumeric char except `\`, `"` and `|`.
            Some(d) if !d.is_alphanumeric() && !matches!(d, '\\' | '"' | '|') => {
                let (pattern, replacement, flags) = split_substitute_args(&after, d);
                Some(self.run_substitute(range, &pattern, Some(&replacement), &flags))
            }
            _ => None,
        }
    }

    /// `:&`, `:&&`, `:~` and normal-mode `&` / `g&`.
    fn repeat_last_substitute(
        &mut self,
        range: Option<(isize, isize)>,
        keep_flags: bool,
        extra_flags: &str,
    ) -> EngineAction {
        let Some((pattern, replacement, flags)) = self.last_substitute.clone() else {
            self.message = "E33: No previous substitute regular expression".to_string();
            return EngineAction::Error;
        };
        let mut f = if keep_flags { flags } else { String::new() };
        if !extra_flags.is_empty() {
            f.push_str(extra_flags);
        }
        self.run_substitute(range, &pattern, Some(&replacement), &f)
    }

    /// The `:substitute` implementation.
    ///
    /// `replacement` is `None` only when the caller has no replacement text at
    /// all (`:s/pat`), which Vim treats as an empty replacement.
    pub(crate) fn run_substitute(
        &mut self,
        range: Option<(isize, isize)>,
        pattern: &str,
        replacement: Option<&str>,
        flags_and_count: &str,
    ) -> EngineAction {
        // --- flags + trailing count (+ a `|`-chained follow-up command) ---
        let mut flags = String::new();
        let mut count: Option<usize> = None;
        let chained: Option<String>;
        {
            let mut it = flags_and_count.chars().peekable();
            while let Some(&c) = it.peek() {
                if c.is_ascii_digit() || c == ' ' || c == '|' {
                    break;
                }
                flags.push(c);
                it.next();
            }
            let tail: String = it.collect();
            // `:s/a/x/|s/b/y/` — an unescaped `|` separates ex commands.
            let (tail, rest) = match tail.split_once('|') {
                Some((t, r)) => (t.to_string(), Some(r.to_string())),
                None => (tail, None),
            };
            chained = rest;
            let tail = tail.trim();
            if !tail.is_empty() {
                match tail.parse::<usize>() {
                    Ok(n) if n > 0 => count = Some(n),
                    _ => {
                        self.message = format!("E488: Trailing characters: {tail}");
                        return EngineAction::Error;
                    }
                }
            }
        }
        // The `&` flag means "reuse the flags of the previous :s".
        if flags.contains('&') {
            if let Some((_, _, prev)) = self.last_substitute.clone() {
                for c in prev.chars() {
                    if c != '&' && !flags.contains(c) {
                        flags.push(c);
                    }
                }
            }
        }
        // #1031 (#801 Phase 2): the confirm loop is entered further down,
        // once the pattern/replacement/range are all resolved — see
        // `confirm && !report_only` below.
        let confirm = flags.contains('c');
        // `:h 'gdefault'`: when set, the meaning of the `g` flag is
        // inverted — every match on a line is replaced by default, and a
        // `g` flag toggles that off (first match per line only).
        let global = flags.contains('g') ^ self.settings.gdefault;
        let report_only = flags.contains('n');
        let quiet = flags.contains('e');

        // --- pattern ---
        let pattern = if pattern.is_empty() {
            self.search_query.clone()
        } else {
            self.search_query = pattern.to_string();
            self.search_smartcase_applies = true;
            pattern.to_string()
        };
        if pattern.is_empty() {
            self.message = "E35: No previous regular expression".to_string();
            return EngineAction::Error;
        }
        let effective_pattern = if flags.contains('I') {
            format!("\\C{pattern}")
        } else if flags.contains('i') {
            format!("\\c{pattern}")
        } else {
            pattern.clone()
        };
        let compiled = match self.compile_vim_pattern(&effective_pattern, true) {
            Ok(c) => c,
            Err(e) => {
                self.message = e;
                return EngineAction::Error;
            }
        };

        // --- replacement ---
        let raw_repl = replacement.unwrap_or("");
        if raw_repl.starts_with("\\=") {
            self.message =
                "E-vimcode: \\= (Vimscript expression) in :s is not implemented".to_string();
            return EngineAction::Error;
        }
        let repl = expand_replacement_tilde(raw_repl, &self.last_sub_replacement);

        self.last_substitute = Some((pattern.clone(), raw_repl.to_string(), flags.clone()));
        self.last_sub_replacement = repl.clone();

        // --- resolve the line range ---
        let full = self.buffer().to_string();
        let (body, trailing) = match full.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (full.as_str(), ""),
        };
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(body.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let n_lines = line_starts.len();
        let line_of = |b: usize| match line_starts.binary_search(&b) {
            Ok(i) => i,
            Err(i) => i - 1,
        };

        let cur = self.view().cursor.line;
        let (mut first_line, mut last_line) = match range {
            Some((a, b)) => (a.max(0) as usize, b.max(0) as usize),
            None => (cur, cur),
        };
        if let Some(n) = count {
            // `:s/a/b/ N` acts on N lines starting at the range's last line.
            first_line = last_line;
            last_line = last_line + n - 1;
        }
        first_line = first_line.min(n_lines.saturating_sub(1));
        last_line = last_line.min(n_lines.saturating_sub(1));

        // --- `:s///c` confirm loop (#1031, #801 Phase 2) ---
        //
        // `n` ("report only, don't substitute") wins over `c` if both are
        // given — there is nothing to confirm if nothing is ever applied —
        // so that combination falls through to the ordinary scan below
        // exactly like a plain `:s///n` would.
        if confirm && !report_only {
            let candidates = collect_confirm_candidates(
                body,
                &line_starts,
                first_line,
                last_line,
                &compiled,
                global,
                &repl,
            );
            if candidates.is_empty() {
                if !quiet {
                    self.message = format!("E486: Pattern not found: {pattern}");
                    return EngineAction::Error;
                }
                self.message.clear();
                if let Some(next) = chained {
                    let next = next.trim().to_string();
                    if !next.is_empty() {
                        return self.execute_command(&next);
                    }
                }
                return EngineAction::None;
            }
            self.confirm_sub = Some(ConfirmSubState {
                body: body.to_string(),
                matches: candidates,
                idx: 0,
                copied: 0,
                out: String::new(),
                n_subs: 0,
                done_lines: Vec::new(),
                last_end_in_out: None,
                last_was_multiline: false,
                cur,
                chained,
            });
            return self.begin_confirm_sub_prompt();
        }

        // --- single left-to-right pass over the buffer text ---
        let mut out = String::new();
        let mut copied = 0usize;
        let mut at = line_starts[first_line];
        let mut done_lines: Vec<usize> = Vec::new();
        let mut n_subs = 0usize;
        let mut last_end_in_out: Option<usize> = None;
        let mut last_was_multiline = false;
        // Byte offset (into `body`, i.e. pre-substitution) of the very first
        // match — Vim's `u` restores the cursor here, not to the cursor
        // position when `:s` was invoked (#886).
        let mut first_change_pos: Option<usize> = None;

        while at <= body.len() {
            let Some(caps) = compiled.captures_at(body, at) else {
                break;
            };
            let whole = caps.get(0).expect("group 0 always matches");
            // `\zs` / `\ze` trim the *replaced* span without changing where the
            // scan resumes, so `:s/foo\zsbar/X/` on "foobar" yields "fooX".
            let (mstart, mend) = compiled.span(&caps);
            let sline = line_of(mstart);
            if sline > last_line {
                break;
            }
            let skip_to_next_line = |at: &mut usize| -> bool {
                match line_starts.get(sline + 1) {
                    Some(&next) => {
                        *at = next;
                        true
                    }
                    None => false,
                }
            };
            if !global && done_lines.last() == Some(&sline) {
                // Only the first match on each line without the `g` flag.
                if skip_to_next_line(&mut at) {
                    continue;
                }
                break;
            }
            // Vim stops the `g` loop when a *subsequent* empty match lands on
            // the end of the line (ex_cmds.c: `sub_firstline[matchcol] == NUL`),
            // so `:s/x*/-/g` on "abc" gives "-a-b-c", not "-a-b-c-".
            let at_eol = mend == body.len() || body.as_bytes()[mend] == b'\n';
            if mstart == mend && at_eol && done_lines.last() == Some(&sline) {
                if skip_to_next_line(&mut at) {
                    continue;
                }
                break;
            }

            let rendered =
                expand_replacement(&repl, &caps, &compiled.group_map, &body[mstart..mend]);
            out.push_str(&body[copied..mstart]);
            out.push_str(&rendered);
            last_end_in_out = Some(out.len());
            copied = mend;
            n_subs += 1;
            if first_change_pos.is_none() {
                first_change_pos = Some(mstart);
            }

            let eline = line_of(mend);
            last_was_multiline = eline > sline;
            if eline > sline {
                // A match that swallowed a line break merges those lines, and
                // Vim re-scans the merged line (`lnum -= nmatch_tl`) — which is
                // why `:%s/\n//` collapses the whole buffer into one line.
                last_line = last_line.saturating_sub(eline - sline);
            } else if done_lines.last() != Some(&sline) {
                done_lines.push(sline);
            }

            at = if whole.end() > whole.start() {
                whole.end().max(mend)
            } else {
                let from = whole.end().max(mend);
                match body[from..].chars().next() {
                    Some(c) => from + c.len_utf8(),
                    None => from + 1,
                }
            };
            if !global && eline == sline {
                if let Some(&next) = line_starts.get(eline + 1) {
                    at = at.max(next);
                } else {
                    break;
                }
            }
        }
        out.push_str(&body[copied..]);

        if n_subs == 0 {
            if !quiet {
                self.message = format!("E486: Pattern not found: {pattern}");
                return EngineAction::Error;
            }
            self.message.clear();
            if let Some(next) = chained {
                let next = next.trim().to_string();
                if !next.is_empty() {
                    return self.execute_command(&next);
                }
            }
            return EngineAction::None;
        }

        if report_only {
            self.message = format!(
                "{} match{} on {} line{}",
                n_subs,
                if n_subs == 1 { "" } else { "es" },
                done_lines.len(),
                if done_lines.len() == 1 { "" } else { "s" }
            );
            return EngineAction::None;
        }

        // Cursor lands on the line holding the end of the *last* substitution,
        // at its first non-blank column (`:h :s`).
        let target_line = last_end_in_out
            .map(|b| out[..b].matches('\n').count())
            .unwrap_or(cur);
        // A substitution that swallowed a line break leaves the cursor on the
        // join column rather than the first non-blank (`:%s/\n//`).
        let target_col = if last_was_multiline {
            last_end_in_out.map(|b| {
                let line_start = out[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
                out[line_start..b].chars().count()
            })
        } else {
            None
        };

        // `u` restores the cursor here — Vim uses the position of the first
        // substitution, not wherever the cursor was when `:s` ran (#886).
        let first_change_cursor = first_change_pos
            .map(|pos| {
                let line = line_of(pos);
                let line_start = line_starts[line];
                let col = body[line_start..pos].chars().count();
                Cursor { line, col }
            })
            .unwrap_or(Cursor { line: cur, col: 0 });

        let new_full = format!("{out}{trailing}");
        self.splice_buffer_text_at(&new_full, first_change_cursor);

        let max_line = self.buffer().len_lines().saturating_sub(1);
        let target_line = target_line.min(max_line);
        self.view_mut().cursor.line = target_line;
        self.view_mut().cursor.col =
            target_col.unwrap_or_else(|| self.first_non_blank_col(target_line));
        self.clamp_cursor_col();

        self.message = format!(
            "{} substitution{} on {} line{}",
            n_subs,
            if n_subs == 1 { "" } else { "s" },
            done_lines.len(),
            if done_lines.len() == 1 { "" } else { "s" }
        );
        if let Some(next) = chained {
            let next = next.trim().to_string();
            if !next.is_empty() {
                return self.execute_command(&next);
            }
        }
        EngineAction::None
    }

    /// Replace the buffer's text with `new_text` as a single undo step,
    /// touching only the region that actually differs so undo stays tight.
    ///
    /// `cursor_before` is what `u` restores the cursor to; Vim uses the
    /// position of the *first* change, which is not always wherever the
    /// (real) cursor happens to be at the time of the call (#886).
    pub(crate) fn splice_buffer_text_at(&mut self, new_text: &str, cursor_before: Cursor) {
        let old: Vec<char> = self.buffer().to_string().chars().collect();
        let new: Vec<char> = new_text.chars().collect();
        if old == new {
            return;
        }
        let mut prefix = 0usize;
        while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
            prefix += 1;
        }
        let mut suffix = 0usize;
        while suffix < old.len() - prefix
            && suffix < new.len() - prefix
            && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
        {
            suffix += 1;
        }
        let inserted: String = new[prefix..new.len() - suffix].iter().collect();
        self.start_undo_group_at(cursor_before);
        if old.len() - suffix > prefix {
            self.delete_with_undo(prefix, old.len() - suffix);
        }
        if !inserted.is_empty() {
            self.insert_with_undo(prefix, &inserted);
        }
        self.finish_undo_group();
    }

    /// [`splice_buffer_text_at`](Self::splice_buffer_text_at) using the
    /// engine's current cursor as `cursor_before`.
    pub(crate) fn splice_buffer_text(&mut self, new_text: &str) {
        let cursor_before = *self.cursor();
        self.splice_buffer_text_at(new_text, cursor_before);
    }

    // --- `:s///c` confirm loop (#1031, #801 Phase 2) ---
    //
    // `run_substitute` precomputes every candidate match up front (see
    // `collect_confirm_candidates`) using the *original*, unmodified buffer
    // text — the same one-pass scan the non-confirm path already used,
    // just not applied yet. The interactive loop below only ever decides,
    // per candidate in order, whether to fold its rendered replacement into
    // an `out` string being built incrementally; the real buffer is left
    // untouched (`self.confirm_sub` holds all of this state) until the loop
    // ends, at which point one `splice_buffer_text_at` applies the result —
    // exactly mirroring what a single non-interactive `:s///g` would have
    // spliced, just gated per-match by the user's answer. This keeps
    // candidate positions stable across answers (no later match ever shifts
    // because an earlier one was replaced), which is safe because, like the
    // non-confirm path, no candidate's rendered text is re-scanned for
    // further matches.
    //
    // Verified against a real `nvim --headless --listen` session (v0.12.5,
    // driven interactively over its msgpack `--remote-send`, not the
    // non-interactive `-es` batch mode, which short-circuits `:s///c`
    // entirely): the pending match's *start* (not its line's first
    // non-blank) is where the cursor sits while a prompt is up; quitting
    // (`q`/`<Esc>`) or `l` ("last") freezes the cursor there and prints no
    // report line even if earlier answers replaced something; only running
    // off the end of the candidate list (individually or via `a`) both
    // reports and re-lands the cursor the same place the non-confirm path
    // would (last substitution's line, first non-blank unless the last
    // substitution was multiline).

    /// Show the prompt for the confirm loop's current candidate, or finish
    /// the loop (as a "ran off the end" completion) if there isn't one.
    ///
    /// The candidate's line/col is *not* trusted from its frozen (original,
    /// pre-edit) `sline`/`scol` here -- every earlier confirmed answer has
    /// already been spliced into the real buffer (see `confirm_sub_apply_current`),
    /// which can shift both the line count (a multiline match, or a `\r` in
    /// the replacement, changes how many newlines precede this candidate)
    /// and the column. Instead this asks the *live* buffer where this
    /// candidate's match now actually sits, via `confirm_sub_live_char_pos`.
    fn begin_confirm_sub_prompt(&mut self) -> EngineAction {
        let Some(state) = self.confirm_sub.as_ref() else {
            return EngineAction::None;
        };
        let Some(m) = state.matches.get(state.idx) else {
            return self.finish_confirm_sub(true, true);
        };
        let rendered = m.rendered.clone();
        let live_pos = confirm_sub_live_char_pos(state, m.mstart);
        let line = self.buffer().content.char_to_line(live_pos);
        let line_start = self.buffer().line_to_char(line);
        let col = live_pos - line_start;
        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = col;
        self.clamp_cursor_col();
        self.message = format!(
            "replace with {rendered}? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)"
        );
        EngineAction::None
    }

    /// Route one keystroke to the confirm loop. Called from `handle_key`
    /// while `self.confirm_sub.is_some()`, ahead of all other dispatch.
    pub(crate) fn handle_confirm_sub_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> EngineAction {
        // `<C-c>`/`<C-[>` aren't in `:h :s_c`'s documented answer set, but
        // they're the same "get me out of here" aliases for `<Esc>` that
        // `handle_insert_key` already recognizes (#804) -- without this a
        // user's habitual escape hatch would silently re-prompt instead of
        // quitting the loop like every other Escape-shaped key in this
        // codebase does.
        if ctrl && matches!(key_name, "c" | "bracketleft" | "[") {
            return self.finish_confirm_sub(false, false);
        }
        if ctrl && (unicode == Some('e') || key_name == "e") {
            self.scroll_viewport_with_cursor(1, 1);
            return EngineAction::None;
        }
        if ctrl && (unicode == Some('y') || key_name == "y") {
            self.scroll_viewport_with_cursor(-1, 1);
            return EngineAction::None;
        }
        if key_name == "Escape" {
            return self.finish_confirm_sub(false, false);
        }
        match unicode {
            Some('y') => {
                self.confirm_sub_apply_current();
                self.confirm_sub_advance()
            }
            Some('n') => self.confirm_sub_advance(),
            Some('a') => {
                loop {
                    self.confirm_sub_apply_current();
                    let Some(state) = self.confirm_sub.as_mut() else {
                        return EngineAction::None;
                    };
                    state.idx += 1;
                    if state.idx >= state.matches.len() {
                        break;
                    }
                }
                self.finish_confirm_sub(true, true)
            }
            // "Last" -- verified against real Neovim: unlike `q`/`<Esc>`,
            // `l` *does* re-land the cursor the same way a natural
            // completion would (first non-blank of the line the applied
            // replacement landed on), it just prints no report line.
            Some('l') => {
                self.confirm_sub_apply_current();
                self.finish_confirm_sub(true, false)
            }
            Some('q') => self.finish_confirm_sub(false, false),
            // Any other key is simply ignored -- verified against real
            // Neovim: the prompt stays up for the same candidate (mode
            // stays 'r', cursor doesn't move), it is not treated as `n`.
            _ => EngineAction::None,
        }
    }

    /// Fold the current candidate's rendered replacement into the
    /// in-progress `out` string (mirroring exactly what the non-confirm
    /// scan does per match -- `run_substitute`'s own comment on the
    /// equivalent code explains the bookkeeping) *and* splice that same
    /// change into the real, live buffer right now.
    ///
    /// #1031 review: the confirm loop used to only ever touch `state.out`,
    /// leaving the visible buffer frozen until the whole loop ended (`a`,
    /// `q`/`<Esc>`, or running off the end) at which point `finish_confirm_sub`
    /// applied every decided candidate in one shot. That's backwards from
    /// the entire point of an interactive confirm prompt -- a user answering
    /// `y` should watch that match change in place before deciding on the
    /// next one. This now performs the live edit immediately, as part of a
    /// single undo group spanning the whole loop (opened here, lazily, on
    /// the first applied answer; closed once in `finish_confirm_sub`) so `u`
    /// still undoes the entire `:s///c` invocation in one step rather than
    /// one keystroke at a time.
    fn confirm_sub_apply_current(&mut self) {
        let Some(mut state) = self.confirm_sub.take() else {
            return;
        };
        let Some(m) = state.matches.get(state.idx).cloned() else {
            self.confirm_sub = Some(state);
            return;
        };

        // Where this candidate's match currently sits in the *live* buffer
        // -- everything up to `state.copied` has already been folded into
        // the live buffer exactly as `state.out` records it (verbatim
        // copies and any earlier confirmed replacements alike), so this is
        // `state.out`'s length plus however much *unchanged* original text
        // sits between `state.copied` and this match's start.
        let live_start = confirm_sub_live_char_pos(&state, m.mstart);
        let matched_chars = state.body[m.mstart..m.mend].chars().count();
        let is_first_apply = state.n_subs == 0;
        let first_cursor = Cursor {
            line: m.sline,
            col: m.scol,
        };

        state.out.push_str(&state.body[state.copied..m.mstart]);
        state.out.push_str(&m.rendered);
        state.copied = m.mend;
        state.n_subs += 1;
        state.last_end_in_out = Some(state.out.len());
        state.last_was_multiline = m.eline > m.sline;
        // Mirrors the non-confirm scan: a multiline match's start line is
        // deliberately *not* added to `done_lines` there either (it shrinks
        // `last_line` instead) -- kept identical here for the same report
        // count.
        if m.eline == m.sline && state.done_lines.last() != Some(&m.sline) {
            state.done_lines.push(m.sline);
        }

        if is_first_apply {
            self.start_undo_group_at(first_cursor);
        }
        if matched_chars > 0 {
            self.delete_with_undo(live_start, live_start + matched_chars);
        }
        if !m.rendered.is_empty() {
            self.insert_with_undo(live_start, &m.rendered);
        }

        self.confirm_sub = Some(state);
    }

    /// Move to the next candidate (without deciding anything about it),
    /// showing its prompt, or finish the loop if that was the last one.
    fn confirm_sub_advance(&mut self) -> EngineAction {
        let Some(state) = self.confirm_sub.as_mut() else {
            return EngineAction::None;
        };
        state.idx += 1;
        if state.idx >= state.matches.len() {
            self.finish_confirm_sub(true, true)
        } else {
            self.begin_confirm_sub_prompt()
        }
    }

    /// End the confirm loop: close out the undo group spanning whatever got
    /// decided (each answer already spliced its own change into the real
    /// buffer live, in `confirm_sub_apply_current` -- there is nothing left
    /// to apply here), then handle the cursor and message independently —
    /// verified against real Neovim (see this section's own doc above),
    /// the two don't always travel together:
    ///
    /// * `reposition` — land the cursor the same way the non-confirm path
    ///   would (last substitution's line, first non-blank unless
    ///   multiline). True for a natural "ran off the end" completion
    ///   (individually or via `a`) *and* for `l`. False only for `q`/
    ///   `<Esc>`, which instead freeze the cursor exactly where the last
    ///   prompt already left it (the pending, undecided candidate).
    /// * `report` — print "N substitutions on M lines". True only for the
    ///   natural completion; `l`, `q` and `<Esc>` all stay silent even if
    ///   an earlier answer replaced something.
    fn finish_confirm_sub(&mut self, reposition: bool, report: bool) -> EngineAction {
        let Some(state) = self.confirm_sub.take() else {
            return EngineAction::None;
        };
        // `state.out` + the still-untouched tail of `state.body` from
        // `state.copied` onward is, by construction, exactly what the live
        // buffer already holds at this point (every earlier answer kept
        // this invariant true via its own live splice) -- rebuilt here
        // purely to derive the report cursor below, not to be written back.
        let mut out = state.out;
        out.push_str(&state.body[state.copied..]);

        if state.n_subs > 0 {
            self.finish_undo_group();
        }

        if reposition && state.n_subs > 0 {
            let target_line = state
                .last_end_in_out
                .map(|b| out[..b].matches('\n').count())
                .unwrap_or(state.cur);
            let target_col = if state.last_was_multiline {
                state.last_end_in_out.map(|b| {
                    let line_start = out[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    out[line_start..b].chars().count()
                })
            } else {
                None
            };
            let max_line = self.buffer().len_lines().saturating_sub(1);
            let target_line = target_line.min(max_line);
            self.view_mut().cursor.line = target_line;
            self.view_mut().cursor.col =
                target_col.unwrap_or_else(|| self.first_non_blank_col(target_line));
            self.clamp_cursor_col();
        }
        // Quitting (`q`/`<Esc>`) leaves the cursor exactly where the last
        // prompt left it (the pending candidate) -- `reposition` is false
        // in that case, so the block above is simply skipped.

        if report && state.n_subs > 0 {
            self.message = format!(
                "{} substitution{} on {} line{}",
                state.n_subs,
                if state.n_subs == 1 { "" } else { "s" },
                state.done_lines.len(),
                if state.done_lines.len() == 1 { "" } else { "s" }
            );
        } else {
            self.message.clear();
        }

        if let Some(next) = state.chained {
            let next = next.trim().to_string();
            if !next.is_empty() {
                return self.execute_command(&next);
            }
        }
        EngineAction::None
    }

    // --- Search ---

    /// Compile a Vim pattern against the current `'ignorecase'` / `'smartcase'`
    /// settings, or return the Vim-style error message.
    ///
    /// #801: a pattern that fails to translate is **rejected**, never silently
    /// downgraded to a literal substring match.
    pub(crate) fn compile_vim_pattern(
        &self,
        pattern: &str,
        smartcase_applies: bool,
    ) -> Result<vim_regex::Compiled, String> {
        vim_regex::compile(
            pattern,
            self.settings.ignorecase,
            self.settings.smartcase,
            smartcase_applies,
            &self.last_sub_replacement,
        )
    }

    /// Collect every match of `re` in `text`.
    ///
    /// Vim enumerates matches **non-overlapping**: `searchit()` restarts its
    /// scan at `endpos.col`, so `/o\+` over `fooo` is one match, not three
    /// (`[1/1]` in the search-count indicator). `captures_iter` has exactly
    /// that semantics, including the advance-one-char rule for empty matches.
    pub(crate) fn collect_match_spans(re: &vim_regex::Compiled, text: &str) -> Vec<(usize, usize)> {
        re.match_spans(text)
    }

    pub fn run_search(&mut self) {
        self.search_matches.clear();
        self.search_index = None;

        if self.search_query.is_empty() {
            return;
        }

        let query_orig = self.search_query.clone();
        let compiled = match self.compile_vim_pattern(&query_orig, self.search_smartcase_applies) {
            Ok(c) => c,
            Err(e) => {
                self.message = e;
                return;
            }
        };

        let text = self.buffer().to_string();
        for (start_byte, end_byte) in Self::collect_match_spans(&compiled, &text) {
            let start_char = self.buffer().content.byte_to_char(start_byte);
            let end_char = self.buffer().content.byte_to_char(end_byte);
            self.search_matches.push((start_char, end_char));
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
        let idx = match next {
            Some(i) => i,
            None if self.settings.wrapscan => {
                self.message = "search hit BOTTOM, continuing at TOP".to_string();
                0
            }
            None => {
                // `:h 'wrapscan'`: off, and no match after the cursor —
                // stay put rather than wrapping (#1153).
                self.message = format!(
                    "E385: search hit BOTTOM without match for: {}",
                    self.search_query
                );
                return;
            }
        };

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
        let idx = match prev {
            Some(i) => i,
            None if self.settings.wrapscan => {
                self.message = "search hit TOP, continuing at BOTTOM".to_string();
                self.search_matches.len() - 1
            }
            None => {
                // See `search_next`'s matching `'wrapscan'` comment (#1153).
                self.message = format!(
                    "E384: search hit TOP without match for: {}",
                    self.search_query
                );
                return;
            }
        };

        self.search_index = Some(idx);
        self.jump_to_search_match(idx);
    }

    /// Is the active search offset a *line* offset (`/pat/+1`, `/pat/-1`,
    /// `/pat/0`)? Those make the search linewise for an operator (`:h
    /// search-offset`).
    /// (Seam for the operator-pending `d/pat` work in the next issue of the
    /// #801 chain — an operator over a search motion is linewise exactly when
    /// the offset is a line offset.)
    pub fn search_offset_is_linewise(&self) -> bool {
        let off = self.search_offset.trim();
        !off.is_empty() && !off.starts_with(['e', 's', 'b'])
    }

    /// Resolve `self.search_offset` against a match span into a final cursor.
    ///
    /// Returns `None` when there is no offset, so the caller lands on the match
    /// start as usual.
    fn offset_cursor(&self, start_char: usize, end_char: usize) -> Option<Cursor> {
        let off = self.search_offset.trim();
        if off.is_empty() {
            return None;
        }
        let (kind, num_str) = match off.chars().next() {
            Some(c @ ('e' | 's' | 'b')) => (c, &off[1..]),
            _ => ('l', off),
        };
        let num: isize = if num_str.is_empty() {
            0
        } else if num_str == "+" {
            1
        } else if num_str == "-" {
            -1
        } else {
            let cleaned = num_str.strip_prefix('+').unwrap_or(num_str);
            cleaned.parse::<isize>().unwrap_or(0)
        };

        if kind == 'l' {
            // Line offset: N lines from the match start, first non-blank.
            let base = self.buffer().content.char_to_line(start_char) as isize;
            let max = self.buffer().len_lines().saturating_sub(1) as isize;
            let line = (base + num).clamp(0, max.max(0)) as usize;
            return Some(Cursor {
                line,
                col: self.first_non_blank_col(line),
            });
        }

        let base = if kind == 'e' {
            end_char.saturating_sub(1).max(start_char)
        } else {
            start_char
        };
        let total = self.buffer().len_chars();
        let target = (base as isize + num).clamp(0, total as isize) as usize;
        let line = self.buffer().content.char_to_line(target);
        let line_start = self.buffer().line_to_char(line);
        Some(Cursor {
            line,
            col: target - line_start,
        })
    }

    pub(crate) fn jump_to_search_match(&mut self, idx: usize) {
        if let Some(&(start_char, end_char)) = self.search_matches.get(idx) {
            let (line, col) = match self.offset_cursor(start_char, end_char) {
                Some(c) => (c.line, c.col),
                None => {
                    let line = self.buffer().content.char_to_line(start_char);
                    let line_start = self.buffer().line_to_char(line);
                    (line, start_char - line_start)
                }
            };
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = col;
            // `/$` and `/\n` match *past* the last character; normal mode keeps
            // the cursor on the last character instead.
            self.clamp_cursor_col();
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

    /// Run the search the user just typed on the `/` or `?` command line.
    ///
    /// `raw` is everything after the leading `/` / `?`, so it may carry a
    /// closing delimiter, a search offset and a `;`-chained second search
    /// (`:h search-offset`, `:h //;`).
    pub fn submit_search(&mut self, raw: &str, count: usize) {
        let delim = match self.search_direction {
            SearchDirection::Forward => '/',
            SearchDirection::Backward => '?',
        };
        let parsed = split_search_cmdline(raw, delim);

        let pattern = if parsed.pattern.is_empty() {
            // `//` and a bare `/<CR>` reuse the last pattern.
            self.search_query.clone()
        } else {
            parsed.pattern
        };
        if pattern.is_empty() {
            self.message = "E35: No previous regular expression".to_string();
            return;
        }

        self.search_query = pattern;
        self.search_offset = parsed.offset;
        self.search_smartcase_applies = true;
        self.run_search();
        if self.search_matches.is_empty() {
            return;
        }
        for _ in 0..count.max(1) {
            match self.search_direction {
                SearchDirection::Forward => self.search_next(),
                SearchDirection::Backward => self.search_prev(),
            }
        }

        if let Some((dir, rest)) = parsed.chained {
            self.search_direction = dir;
            self.submit_search(&rest, 1);
        }
    }

    /// Perform incremental search as user types
    pub fn perform_incremental_search(&mut self) {
        // Update search query from command buffer. Strip any offset / chained
        // search so that typing `/foo/e` still highlights `foo` as you type.
        let delim = match self.search_direction {
            SearchDirection::Forward => '/',
            SearchDirection::Backward => '?',
        };
        let typed = split_search_cmdline(&self.command_buffer, delim).pattern;
        if self.command_buffer.is_empty() {
            self.search_query.clear();
        } else if !typed.is_empty() {
            // An empty `typed` with a non-empty command line is `//` — Vim
            // reuses the previous pattern, so leave `search_query` alone.
            self.search_query = typed;
            self.search_smartcase_applies = true;
        }

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

            // Find the appropriate match based on search direction. `:h
            // 'wrapscan'`: off, and no match in the requested direction from
            // the start position — the live preview stays put rather than
            // previewing a wrapped-around match (#1153 review; mirrors
            // `search_next`/`search_prev`).
            let idx = match self.search_direction {
                SearchDirection::Forward => {
                    // Find first match at or after start position
                    self.search_matches
                        .iter()
                        .position(|(start, _)| *start >= start_char)
                }
                SearchDirection::Backward => {
                    // Find last match strictly before start position
                    self.search_matches
                        .iter()
                        .rposition(|(start, _)| *start < start_char)
                }
            };

            let idx = match idx {
                Some(i) => Some(i),
                None if self.settings.wrapscan => Some(match self.search_direction {
                    SearchDirection::Forward => 0,
                    SearchDirection::Backward => self.search_matches.len() - 1,
                }),
                None => None,
            };

            if let Some(idx) = idx {
                self.search_index = Some(idx);
                self.jump_to_search_match(idx);
            } else if let Some(start_cursor) = self.search_start_cursor {
                self.view_mut().cursor = start_cursor;
            }
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
    /// flags: "g" (all), "i" (case-insensitive) -- `c` (confirm) is handled
    /// by `run_substitute`'s own `:s///c` loop before this function is ever
    /// reached; this legacy per-line path's only caller (`search.rs`'s
    /// `find_replace_replace_all`) never passes it.
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
    /// Handle a status bar segment click. Returns an `EngineAction` if the
    /// caller (backend) must perform it (e.g. sidebar toggle lives on the UI).
    pub fn handle_status_action(&mut self, action: &StatusAction) -> Option<EngineAction> {
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
            StatusAction::ToggleSidebar => {
                self.toggle_sidebar();
            }
            StatusAction::TogglePanel => {
                if self.terminal_panes.is_empty() {
                    return Some(EngineAction::OpenTerminal);
                }
                self.toggle_terminal();
            }
            StatusAction::ToggleMenuBar => {
                self.toggle_menu_bar();
            }
            StatusAction::DismissNotifications => {
                self.dismiss_done_notifications();
            }
        }
        None
    }

    /// Try to parse and execute a ranged ex command like `:2d`, `:3,5d`, `:10y`.
    /// Returns `Some(action)` if it handled the command, `None` otherwise.
    /// `:[range]{cmd}` for the line-oriented ex commands that take a general
    /// address range: `:d`, `:y`, `:j`, `:>`, `:<`, `:t` / `:co`, `:m`, plus a
    /// bare range (`:5`, `:$`, `:/foo/`) which just moves the cursor.
    ///
    /// Returns `None` for anything it does not recognise so the rest of
    /// `execute_command`'s dispatch is unaffected.
    fn try_execute_ranged_command(&mut self, cmd: &str) -> Option<EngineAction> {
        let chars: Vec<char> = cmd.chars().collect();
        let (range, consumed) = self.parse_ex_range(&chars);
        let rest: String = chars[consumed..].iter().collect();
        let rest = rest.trim().to_string();
        let last_line = self.buffer().len_lines().saturating_sub(1);

        // A range on its own moves the cursor to its last line.
        if rest.is_empty() {
            let (_, end) = range?;
            let target = (end.max(0) as usize).min(last_line);
            // Neovim leaves the cursor in column 1 and does *not* push a jump
            // for a bare `:{address}` (`jump:C-o after :5` pins both).
            self.view_mut().cursor.line = target;
            self.view_mut().cursor.col = 0;
            self.clamp_cursor_col();
            return Some(EngineAction::None);
        }

        // `>`, `>>`, `<<` … — one shift level per repeated character.
        if rest.starts_with('>') || rest.starts_with('<') {
            let shift_char = rest.chars().next()?;
            let levels = rest.chars().take_while(|&c| c == shift_char).count();
            let args = rest[levels..].trim();
            let count = if args.is_empty() {
                None
            } else {
                Some(args.parse::<usize>().ok()?)
            };
            let (start, end) = self.range_with_count(range, count, last_line);
            let n = end - start + 1;
            let mut changed = false;
            self.view_mut().cursor.line = start;
            for _ in 0..levels {
                if shift_char == '>' {
                    self.indent_lines(start, n, &mut changed, true);
                } else {
                    self.dedent_lines(start, n, &mut changed, true);
                }
            }
            self.view_mut().cursor.line = end.min(self.buffer().len_lines().saturating_sub(1));
            let line = self.view().cursor.line;
            self.view_mut().cursor.col = self.first_non_blank_col(line);
            self.clamp_cursor_col();
            return Some(EngineAction::None);
        }

        let (name, args) = split_ex_name(&rest);
        let (name, bang) = match name.strip_suffix('!') {
            Some(n) => (n, true),
            None => (name, false),
        };
        let is = |canonical: &str, min: usize| {
            name.len() >= min && name.len() <= canonical.len() && canonical.starts_with(name)
        };

        // `:[range]sor[t][!] [flags] [/pattern/]` — sort just the given range.
        // `sor` is Vim's minimum abbreviation (`so` is `:source`).
        if is("sort", 3) {
            let (start, end) = self.range_with_count(range, None, last_line);
            return Some(self.execute_sort_command(Some((start, end)), bang, args));
        }

        if is("delete", 1) {
            let (reg, count) = parse_reg_and_count(args)?;
            let (start, end) = self.range_with_count(range, count, last_line);
            self.view_mut().cursor.line = start;
            self.view_mut().cursor.col = 0;
            let mut changed = false;
            // Route the register through `delete_lines`'s own
            // `set_delete_register` (via `active_register`) instead of
            // deleting into the unnamed register and copying afterward —
            // the copy step can't express "don't write anywhere", so `:d _`
            // was clobbering "" like a plain `:d` (#806, "ex:d _").
            if let Some(r) = reg {
                self.selected_register = Some(r);
            }
            self.start_undo_group();
            self.delete_lines(end - start + 1, &mut changed);
            self.finish_undo_group();
            let line = self.view().cursor.line;
            self.view_mut().cursor.col = self.first_non_blank_col(line);
            self.clamp_cursor_col();
            return Some(EngineAction::None);
        }

        if is("yank", 1) {
            let (reg, count) = parse_reg_and_count(args)?;
            let (start, end) = self.range_with_count(range, count, last_line);
            let saved = self.view().cursor;
            self.view_mut().cursor.line = start;
            if let Some(r) = reg {
                self.selected_register = Some(r);
            }
            self.yank_lines(end - start + 1);
            self.view_mut().cursor = saved;
            return Some(EngineAction::None);
        }

        if is("join", 1) {
            let count = if args.is_empty() {
                None
            } else {
                Some(args.parse::<usize>().ok()?)
            };
            let (start, end) = match (range, count) {
                // `:j 3` joins 3 lines starting at the range's last line.
                (_, Some(n)) => {
                    let base = range
                        .map(|(_, e)| e.max(0) as usize)
                        .unwrap_or(self.view().cursor.line);
                    (base, (base + n - 1).min(last_line))
                }
                // A single-line range joins that line with the next one.
                (Some((a, b)), None) if a == b => {
                    let a = a.max(0) as usize;
                    (a, (a + 1).min(last_line))
                }
                (Some((a, b)), None) => (a.max(0) as usize, (b.max(0) as usize).min(last_line)),
                (None, None) => {
                    let c = self.view().cursor.line;
                    (c, (c + 1).min(last_line))
                }
            };
            self.view_mut().cursor.line = start;
            let mut changed = false;
            self.start_undo_group();
            if bang {
                self.join_lines_no_space(end - start + 1, &mut changed);
            } else {
                self.join_lines(end - start + 1, &mut changed);
            }
            self.finish_undo_group();
            // Ex `:join` ends on the first non-blank of the joined line, unlike
            // normal-mode `J` which parks the cursor on the join point.
            let line = start.min(self.buffer().len_lines().saturating_sub(1));
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = self.first_non_blank_col(line);
            self.clamp_cursor_col();
            return Some(EngineAction::None);
        }

        // `:[range]pu[t][!] [x]` — put register `x` after `[range]`'s last
        // address (or before it, with `!`). Default address is the cursor
        // line; address `0` (`-1` in our 0-based/-1 addressing) means "before
        // the first line", which is what makes `:0put` legal (`:h :put`).
        if is("put", 2) {
            let reg = if args.is_empty() {
                '"'
            } else {
                let (r, count) = parse_reg_and_count(args)?;
                if count.is_some() {
                    return None;
                }
                r.unwrap_or('"')
            };
            let target = range
                .map(|(_, end)| end)
                .unwrap_or(self.view().cursor.line as isize);
            return Some(self.execute_put(target, bang, reg));
        }

        // `:[range]ma[rk] {a-zA-Z}` and `:[range]k{a-zA-Z}` (the `k` spelling
        // takes its mark letter directly, with no space) — set mark `x` on
        // `[range]`'s last address rather than the cursor line.
        let mark_arg = if is("mark", 2) && args.chars().count() == 1 {
            args.chars().next()
        } else if rest.len() == 2 && rest.starts_with('k') {
            rest.chars().nth(1)
        } else {
            None
        };
        if let Some(ch) = mark_arg {
            if ch.is_ascii_alphabetic() {
                let target = range
                    .map(|(_, end)| end)
                    .unwrap_or(self.view().cursor.line as isize);
                let line = target.max(0) as usize;
                return Some(self.set_ex_mark(ch, Cursor { line, col: 0 }));
            }
        }

        // `:t`, `:co[py]` and `:m[ove]` take a destination address.
        let dest_kind = if name == "t" || is("copy", 2) {
            Some(false)
        } else if is("move", 1) {
            Some(true)
        } else {
            None
        };
        if let Some(is_move) = dest_kind {
            if args.is_empty() {
                return None;
            }
            let (start, end) = self.range_with_count(range, None, last_line);
            return Some(if is_move {
                self.execute_move_range(start, end, args)
            } else {
                self.execute_copy_range(start, end, args)
            });
        }

        // `:[range]ret[ab][!] [new_tabstop]` — unlike the other commands
        // above, an *omitted* range means the whole buffer (`:h :retab`),
        // not the current line.
        if is("retab", 3) {
            let new_tabstop = if args.is_empty() {
                None
            } else {
                match args.parse::<usize>() {
                    Ok(n) => Some(n),
                    Err(_) => return None,
                }
            };
            let (start, end) = match range {
                Some((a, b)) => (a.max(0) as usize, (b.max(0) as usize).min(last_line)),
                None => (0, last_line),
            };
            return Some(self.execute_retab(start, end, bang, new_tabstop));
        }

        // `:[range]le[ft] [indent]`, `:[range]ri[ght] [width]`,
        // `:[range]ce[nter] [width]` — re-indent the range. The numeric
        // argument is an indent/width, not a `:d`-style trailing count, and
        // the default range is the current line (`:h :left`).
        let reindent_kind = if is("left", 2) {
            Some(0)
        } else if is("right", 2) {
            Some(1)
        } else if is("center", 2) {
            Some(2)
        } else {
            None
        };
        if let Some(kind) = reindent_kind {
            let arg = if args.is_empty() {
                None
            } else {
                match args.parse::<usize>() {
                    Ok(n) => Some(n),
                    Err(_) => return None,
                }
            };
            let (start, end) = match range {
                Some((a, b)) => (a.max(0) as usize, (b.max(0) as usize).min(last_line)),
                None => {
                    let cur = self.view().cursor.line;
                    (cur, cur)
                }
            };
            return Some(match kind {
                0 => self.execute_left(start, end, arg),
                1 => self.execute_right(start, end, arg),
                _ => self.execute_center(start, end, arg),
            });
        }

        None
    }

    /// Resolve a parsed range plus an optional trailing count into 0-based
    /// inclusive line bounds. A count makes the range "N lines starting at the
    /// range's last line", which is Vim's rule for `:d 2`, `:y 3`, `:> 2`.
    fn range_with_count(
        &self,
        range: Option<(isize, isize)>,
        count: Option<usize>,
        last_line: usize,
    ) -> (usize, usize) {
        let cur = self.view().cursor.line;
        let (a, b) = match range {
            Some((a, b)) => (a.max(0) as usize, b.max(0) as usize),
            None => (cur, cur),
        };
        match count {
            Some(n) if n > 0 => (b.min(last_line), (b + n - 1).min(last_line)),
            _ => (a.min(last_line), b.min(last_line)),
        }
    }

    /// `:[range]ret[ab][!] [new_tabstop]` (`:h :retab`).
    ///
    /// Without `!`, only whitespace runs that contain a <Tab> are touched:
    /// with `'expandtab'` they become spaces, without it they are
    /// re-expressed using the (possibly new) `'tabstop'`. With `!`, runs of
    /// plain spaces are considered too, which only matters with
    /// `'noexpandtab'` (with `'expandtab'` a plain-space run's replacement is
    /// always itself). The existing `'tabstop'` — not the new one — is
    /// always used to measure the *current* width of a run; the new value
    /// only controls how that width is re-emitted.
    fn execute_retab(
        &mut self,
        start: usize,
        end: usize,
        bang: bool,
        new_tabstop: Option<usize>,
    ) -> EngineAction {
        let old_ts = (self.settings.tabstop as usize).max(1);
        let new_ts = new_tabstop.filter(|&n| n > 0).unwrap_or(old_ts);
        let expand = self.settings.expand_tab;
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let mut new_cursor_col = None;

        self.start_undo_group();
        let total = self.buffer().len_lines();
        for line_idx in start..=end.min(total.saturating_sub(1)) {
            let chars: Vec<char> = self.buffer().content.line(line_idx).chars().collect();
            // Drop a trailing line terminator before processing — it is never
            // whitespace `retab` should touch, and re-appending it verbatim
            // keeps `\r\n` intact.
            let eol_len = chars
                .iter()
                .rev()
                .take_while(|c| **c == '\n' || **c == '\r')
                .count();
            let body = &chars[..chars.len() - eol_len];
            let (new_body, runs) = retab_line(body, old_ts, new_ts, expand, bang);
            if runs.is_empty() {
                continue;
            }
            if line_idx == cursor_line {
                new_cursor_col = Some(retab_adjust_col(&runs, cursor_col));
            }
            let line_start = self.buffer().line_to_char(line_idx);
            self.delete_with_undo(line_start, line_start + body.len());
            self.insert_with_undo(line_start, &new_body);
        }
        self.finish_undo_group();
        self.settings.tabstop = new_ts.min(u8::MAX as usize) as u8;
        if let Some(col) = new_cursor_col {
            self.view_mut().cursor.col = col;
            self.clamp_cursor_col();
        }
        self.message = "Retabbed".to_string();
        EngineAction::None
    }

    /// `:[range]le[ft] [indent]` — strip existing leading white space and
    /// replace it with exactly `indent` columns (0 when omitted).
    fn execute_left(
        &mut self,
        start: usize,
        end: usize,
        indent_arg: Option<usize>,
    ) -> EngineAction {
        let ts = (self.settings.tabstop as usize).max(1);
        let expand = self.settings.expand_tab;
        let indent_cols = indent_arg.unwrap_or(0);
        let new_indent = make_indent_string(indent_cols, ts, expand);

        self.start_undo_group();
        let total = self.buffer().len_lines();
        let last = end.min(total.saturating_sub(1));
        for line_idx in start..=last {
            let chars: Vec<char> = self.buffer().content.line(line_idx).chars().collect();
            let eol_len = chars
                .iter()
                .rev()
                .take_while(|c| **c == '\n' || **c == '\r')
                .count();
            let body = &chars[..chars.len() - eol_len];
            let leading = body
                .iter()
                .take_while(|c| **c == ' ' || **c == '\t')
                .count();
            let rest: String = body[leading..].iter().collect();
            let new_body = format!("{new_indent}{rest}");
            if new_body.chars().eq(body.iter().copied()) {
                continue;
            }
            let line_start = self.buffer().line_to_char(line_idx);
            self.delete_with_undo(line_start, line_start + body.len());
            self.insert_with_undo(line_start, &new_body);
        }
        self.finish_undo_group();
        self.finish_reindent_cursor(last);
        EngineAction::None
    }

    /// `:[range]ri[ght] [width]` — right-align the trimmed line content so it
    /// ends at column `width` (`'textwidth'`, or 80 when that is 0).
    fn execute_right(
        &mut self,
        start: usize,
        end: usize,
        width_arg: Option<usize>,
    ) -> EngineAction {
        let width = width_arg.unwrap_or_else(|| self.reindent_default_width());
        self.reindent_range(start, end, |content_width| {
            width.saturating_sub(content_width)
        });
        EngineAction::None
    }

    /// `:[range]ce[nter] [width]` — center the trimmed line content within a
    /// field `width` columns wide (`'textwidth'`, or 80 when that is 0).
    fn execute_center(
        &mut self,
        start: usize,
        end: usize,
        width_arg: Option<usize>,
    ) -> EngineAction {
        let width = width_arg.unwrap_or_else(|| self.reindent_default_width());
        self.reindent_range(start, end, |content_width| {
            width.saturating_sub(content_width) / 2
        });
        EngineAction::None
    }

    fn reindent_default_width(&self) -> usize {
        if self.settings.textwidth > 0 {
            self.settings.textwidth
        } else {
            80
        }
    }

    /// Shared body of `:right` and `:center`: trim each line in the range and
    /// re-pad its left side with `pad_for(content_width)` columns of
    /// indentation. Blank lines are left untouched, matching `:h :left`'s
    /// treatment of an empty indent.
    fn reindent_range(&mut self, start: usize, end: usize, pad_for: impl Fn(usize) -> usize) {
        let ts = (self.settings.tabstop as usize).max(1);
        let expand = self.settings.expand_tab;

        self.start_undo_group();
        let total = self.buffer().len_lines();
        let last = end.min(total.saturating_sub(1));
        for line_idx in start..=last {
            let chars: Vec<char> = self.buffer().content.line(line_idx).chars().collect();
            let eol_len = chars
                .iter()
                .rev()
                .take_while(|c| **c == '\n' || **c == '\r')
                .count();
            let body = &chars[..chars.len() - eol_len];
            let trimmed_start = body
                .iter()
                .take_while(|c| **c == ' ' || **c == '\t')
                .count();
            let trimmed_end = body.len()
                - body
                    .iter()
                    .rev()
                    .take_while(|c| **c == ' ' || **c == '\t')
                    .count();
            if trimmed_start >= trimmed_end {
                continue; // blank line — leave it as-is
            }
            let content: String = body[trimmed_start..trimmed_end].iter().collect();
            let pad = pad_for(content.chars().count());
            let new_body = format!("{}{}", make_indent_string(pad, ts, expand), content);
            if new_body.chars().eq(body.iter().copied()) {
                continue;
            }
            let line_start = self.buffer().line_to_char(line_idx);
            self.delete_with_undo(line_start, line_start + body.len());
            self.insert_with_undo(line_start, &new_body);
        }
        self.finish_undo_group();
        self.finish_reindent_cursor(last);
    }

    /// `:left`/`:right`/`:center` leave the cursor on the range's last line,
    /// at its first non-blank (`:h :left` and friends; verified against
    /// Neovim — unlike `:retab`, which tracks the edited run instead).
    fn finish_reindent_cursor(&mut self, last_line: usize) {
        let line = last_line.min(self.buffer().len_lines().saturating_sub(1));
        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = self.first_non_blank_col(line);
        self.clamp_cursor_col();
    }
}

/// One candidate match `:s///c` offers to confirm — everything about it is
/// derived from the *original*, unmodified buffer text once, up front (see
/// `collect_confirm_candidates`), so answering earlier candidates never
/// shifts a later one's position.
#[derive(Clone)]
pub(crate) struct ConfirmSubMatch {
    /// Byte offset of the match's start in the frozen `body` (honours
    /// `\zs`/`\ze` like the non-confirm scan does).
    mstart: usize,
    /// Byte offset just past the match's end in the frozen `body`.
    mend: usize,
    /// 0-indexed line the match starts on.
    sline: usize,
    /// 0-indexed line the match ends on (`> sline` for a match that
    /// swallowed a line break, e.g. `:%s/\n//`).
    eline: usize,
    /// 0-indexed char column of `mstart` within `sline` — where the cursor
    /// sits while this candidate's prompt is up (verified against real
    /// Neovim: the match's start, not the line's first non-blank).
    scol: usize,
    /// This candidate's replacement text, already expanded against its own
    /// captures (`\1`, `\U`, `&`, …).
    rendered: String,
}

/// State for an in-progress `:s///c` confirm loop — lives in
/// `Engine::confirm_sub` between keystrokes; see that field's doc and the
/// "confirm loop" section of `impl Engine` in this file for how it's driven.
pub(crate) struct ConfirmSubState {
    /// The buffer's text at the moment `:s///c` was invoked, sans its
    /// trailing `\n` (that trailing newline, if any, is never touched by
    /// the confirm loop -- every match lives inside `body`, and each
    /// confirmed answer is spliced live into the real buffer in place, so
    /// there is no need to track or ever reassemble the suffix separately)
    /// — frozen for the whole loop; matches' offsets are only ever valid
    /// against this copy, not the live buffer.
    body: String,
    /// Every match `:s///c` will offer to confirm, in order, precomputed
    /// against `body`.
    matches: Vec<ConfirmSubMatch>,
    /// Index into `matches` of the candidate currently being prompted for.
    idx: usize,
    /// Byte offset into `body` up to which `out` already accounts for
    /// (either copied verbatim or replaced) — mirrors the non-confirm
    /// scan's `copied`.
    copied: usize,
    /// The substitution result being built incrementally as candidates are
    /// decided — mirrors the non-confirm scan's `out`.
    out: String,
    /// Count of candidates actually replaced (`y`/`a`/`l`) — *not* the
    /// number of candidates offered; a `n`-answered candidate must not
    /// count toward the post-loop report (#1031 deliverable 2).
    n_subs: usize,
    /// Distinct 0-indexed lines an actual replacement landed on, in the
    /// order they were applied — feeds "N substitutions on M lines".
    /// Mirrors the non-confirm scan's own `done_lines` quirk: a multiline
    /// match's start line is never pushed here (see `confirm_sub_apply_current`).
    done_lines: Vec<usize>,
    /// Byte offset into `out` just past the most recently applied
    /// replacement — used to compute the final cursor line/col exactly like
    /// the non-confirm path's `last_end_in_out`.
    last_end_in_out: Option<usize>,
    /// Whether the most recently applied replacement swallowed a line
    /// break, same meaning as the non-confirm path's `last_was_multiline`.
    last_was_multiline: bool,
    /// The cursor's line when `:s///c` was invoked — the fallback used if
    /// nothing ever gets applied (mirrors the non-confirm path's `cur`).
    cur: usize,
    /// A `|`-chained follow-up ex command (`:s/a/x/|s/b/y/c`), run once the
    /// loop ends, same as the non-confirm path.
    chained: Option<String>,
}

/// Char position, in the *live* buffer as it currently stands, that a byte
/// offset into the frozen `body` corresponds to.
///
/// Everything up to `state.copied` has already been folded into the live
/// buffer exactly as `state.out` records it (verbatim copies of
/// not-yet-decided text and any earlier confirmed replacements alike -- see
/// `Engine::confirm_sub_apply_current`'s doc for why that invariant holds),
/// so a later offset's live position is `state.out`'s length plus however
/// much *unchanged* original text sits between `state.copied` and it.
/// `body_byte_offset` must be `>= state.copied` (true of every candidate's
/// `mstart`/`mend`, since candidates are processed strictly in document
/// order and `state.copied` only ever advances to a just-applied match's
/// `mend`).
fn confirm_sub_live_char_pos(state: &ConfirmSubState, body_byte_offset: usize) -> usize {
    state.out.chars().count() + state.body[state.copied..body_byte_offset].chars().count()
}

/// Scan `body` for every match `:s///c` should offer to confirm, applying
/// the exact same global/same-line-dedup/multiline/empty-match-at-eol rules
/// the non-confirm scan in `run_substitute` uses to decide which matches
/// are candidates at all — the two scans *must* agree, since `:s///gc` and
/// `:s///g` differ only in whether each candidate is applied unconditionally
/// or interactively. Unlike that scan, this one never mutates an `out`
/// string; it only records each candidate's span, line/col and rendered
/// replacement text so the confirm loop (`Engine::confirm_sub_apply_current`
/// et al.) can decide, one keystroke at a time, which candidates actually
/// get folded into the result.
#[allow(clippy::too_many_arguments)]
fn collect_confirm_candidates(
    body: &str,
    line_starts: &[usize],
    first_line: usize,
    last_line: usize,
    compiled: &vim_regex::Compiled,
    global: bool,
    repl: &str,
) -> Vec<ConfirmSubMatch> {
    let line_of = |b: usize| match line_starts.binary_search(&b) {
        Ok(i) => i,
        Err(i) => i - 1,
    };
    let mut matches = Vec::new();
    let mut at = line_starts[first_line];
    let mut done_lines: Vec<usize> = Vec::new();
    let mut last_line = last_line;
    while at <= body.len() {
        let Some(caps) = compiled.captures_at(body, at) else {
            break;
        };
        let whole = caps.get(0).expect("group 0 always matches");
        let (mstart, mend) = compiled.span(&caps);
        let sline = line_of(mstart);
        if sline > last_line {
            break;
        }
        let skip_to_next_line = |at: &mut usize| -> bool {
            match line_starts.get(sline + 1) {
                Some(&next) => {
                    *at = next;
                    true
                }
                None => false,
            }
        };
        if !global && done_lines.last() == Some(&sline) {
            if skip_to_next_line(&mut at) {
                continue;
            }
            break;
        }
        let at_eol = mend == body.len() || body.as_bytes()[mend] == b'\n';
        if mstart == mend && at_eol && done_lines.last() == Some(&sline) {
            if skip_to_next_line(&mut at) {
                continue;
            }
            break;
        }

        let rendered = expand_replacement(repl, &caps, &compiled.group_map, &body[mstart..mend]);
        let eline = line_of(mend);
        if eline > sline {
            last_line = last_line.saturating_sub(eline - sline);
        } else if done_lines.last() != Some(&sline) {
            done_lines.push(sline);
        }
        let line_start = line_starts[sline];
        let scol = body[line_start..mstart].chars().count();
        matches.push(ConfirmSubMatch {
            mstart,
            mend,
            sline,
            eline,
            scol,
            rendered,
        });

        at = if whole.end() > whole.start() {
            whole.end().max(mend)
        } else {
            let from = whole.end().max(mend);
            match body[from..].chars().next() {
                Some(c) => from + c.len_utf8(),
                None => from + 1,
            }
        };
        if !global && eline == sline {
            if let Some(&next) = line_starts.get(eline + 1) {
                at = at.max(next);
            } else {
                break;
            }
        }
    }
    matches
}

/// One whitespace run `:retab` rewrote on a line, in char offsets — used to
/// re-derive where the cursor should land afterward.
struct RetabRun {
    old_start: usize,
    old_len: usize,
    new_start: usize,
    new_len: usize,
}

/// The column just past `col` on the current line (`:h 'tabstop'`).
fn next_tabstop(col: usize, ts: usize) -> usize {
    (col / ts + 1) * ts
}

/// Rewrite the whitespace runs of one line for `:retab`, mirroring Vim's
/// `do_retab`: each run's *old* width is measured with `old_ts` (a tab
/// advances to the next `old_ts` stop; a space always costs 1), then
/// re-emitted at that same width — as spaces when `expand`, else as the
/// fewest tabs-then-spaces `new_ts` allows. A run is only rewritten when it
/// contains a tab, or (with `bang`) unconditionally; a plain-space run under
/// `expand` always re-emits itself, so `bang` only has visible effect with
/// `'noexpandtab'`. All non-whitespace characters are treated as one column
/// wide (matches this codebase's existing tab/indent handling elsewhere).
fn retab_line(
    chars: &[char],
    old_ts: usize,
    new_ts: usize,
    expand: bool,
    bang: bool,
) -> (String, Vec<RetabRun>) {
    let old_ts = old_ts.max(1);
    let new_ts = new_ts.max(1);
    let mut out = String::new();
    let mut runs = Vec::new();
    let mut i = 0;
    let mut vcol = 0usize;
    let n = chars.len();
    while i < n {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            let run_start = i;
            let run_start_vcol = vcol;
            let mut has_tab = false;
            let mut j = i;
            while j < n && (chars[j] == ' ' || chars[j] == '\t') {
                if chars[j] == '\t' {
                    has_tab = true;
                    vcol = next_tabstop(vcol, old_ts);
                } else {
                    vcol += 1;
                }
                j += 1;
            }
            let run_len = j - run_start;
            let end_vcol = vcol;
            if has_tab || bang {
                let width = end_vcol - run_start_vcol;
                let new_text = if expand {
                    " ".repeat(width)
                } else {
                    let mut cur = run_start_vcol;
                    let mut tabs = 0usize;
                    loop {
                        let nt = next_tabstop(cur, new_ts);
                        if nt <= end_vcol {
                            tabs += 1;
                            cur = nt;
                        } else {
                            break;
                        }
                    }
                    format!("{}{}", "\t".repeat(tabs), " ".repeat(end_vcol - cur))
                };
                let new_start = out.chars().count();
                let new_len = new_text.chars().count();
                if !new_text.chars().eq(chars[run_start..j].iter().copied()) {
                    runs.push(RetabRun {
                        old_start: run_start,
                        old_len: run_len,
                        new_start,
                        new_len,
                    });
                }
                out.push_str(&new_text);
            } else {
                out.extend(&chars[run_start..j]);
            }
            i = j;
        } else {
            vcol += 1;
            out.push(c);
            i += 1;
        }
    }
    (out, runs)
}

/// Re-derive a line's cursor column after `retab_line` rewrote it, mirroring
/// `do_retab`'s own cursor tracking: a column strictly before a rewritten run
/// is unaffected by it; one at or inside the run's old span lands at the end
/// of that run's replacement; one strictly after is shifted by the run's net
/// length change. Verified against Neovim (`ex:retab`, `ex:retab!`): typing
/// `:retab<CR>` through the command line moves the cursor this way even
/// though the equivalent scripted `nvim_cmd` call does not — a real
/// command-line-only quirk, not a harness artifact, so vimcode (which has no
/// separate scripted path) always applies it.
fn retab_adjust_col(runs: &[RetabRun], old_col: usize) -> usize {
    let mut col = old_col as isize;
    for run in runs {
        let old_end = run.old_start + run.old_len;
        if old_col < run.old_start {
            continue;
        } else if old_col < old_end {
            col = (run.new_start + run.new_len).saturating_sub(1) as isize;
            break;
        } else {
            col += run.new_len as isize - run.old_len as isize;
        }
    }
    col.max(0) as usize
}

/// Build an indent string of exactly `cols` display columns, using tabs where
/// `'noexpandtab'` allows a whole `ts`-wide stop and spaces for the
/// remainder — the same representation `>>`/`<<` use (`:h :left`, `:h :right`,
/// `:h :center` all delegate to this for their leading white space).
fn make_indent_string(cols: usize, ts: usize, expand: bool) -> String {
    let ts = ts.max(1);
    if expand {
        " ".repeat(cols)
    } else {
        format!("{}{}", "\t".repeat(cols / ts), " ".repeat(cols % ts))
    }
}

/// Split `after` — everything from the `:s` delimiter onwards — into
/// `(pattern, replacement, flags)`.
///
/// The delimiter may be any character Vim allows (`:s#a#b#`), and a
/// backslash-escaped delimiter (`\/`) does not terminate a field, which is what
/// made the old `cmd.split('/')` parse wrong for `:s/\//-/`.
pub(crate) fn split_substitute_args(after: &str, delim: char) -> (String, String, String) {
    let chars: Vec<char> = after.chars().collect();
    let mut i = 1; // skip the opening delimiter
    let mut fields: Vec<String> = Vec::new();
    let mut cur = String::new();
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            cur.push(chars[i]);
            cur.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == delim {
            fields.push(std::mem::take(&mut cur));
            i += 1;
            if fields.len() == 2 {
                break;
            }
            continue;
        }
        cur.push(chars[i]);
        i += 1;
    }
    if fields.len() < 2 {
        fields.push(std::mem::take(&mut cur));
    }
    let pattern = fields.first().cloned().unwrap_or_default();
    let replacement = fields.get(1).cloned().unwrap_or_default();
    let flags: String = chars[i.min(chars.len())..].iter().collect();
    (pattern, replacement, flags)
}

/// Expand an unescaped `~` in a `:s` replacement to the previous replacement
/// string (`:h sub-replace-special`). `\~` stays literal for the per-match pass.
pub(crate) fn expand_replacement_tilde(repl: &str, previous: &str) -> String {
    let chars: Vec<char> = repl.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '~' {
            out.push_str(previous);
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Case-folding state driven by `\u`, `\l`, `\U`, `\L`, `\E` and `\e`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CaseMode {
    None,
    Upper,
    Lower,
}

/// Expand one match's replacement text per `:h sub-replace-special`.
///
/// `group_map` maps a Vim group number (`\1`) onto the Rust group number, which
/// differs when `\zs` / `\ze` injected a capture group ahead of it.
pub(crate) fn expand_replacement<'a>(
    repl: &str,
    caps: &vim_regex::Captures<'a>,
    group_map: &[usize],
    whole: &'a str,
) -> String {
    let group = |vim_n: usize| -> &str {
        if vim_n == 0 {
            // `&` and `\0` are the *reported* match, i.e. the `\zs`/`\ze` span.
            return whole;
        }
        let rust_n = group_map.get(vim_n).copied().unwrap_or(vim_n);
        caps.get(rust_n).map(|m| m.as_str()).unwrap_or("")
    };

    let mut out = String::new();
    let mut run = CaseMode::None;
    let mut one = CaseMode::None;
    let push = |out: &mut String, s: &str, run: &mut CaseMode, one: &mut CaseMode| {
        for c in s.chars() {
            let mapped = match (*one, *run) {
                (CaseMode::Upper, _) => {
                    *one = CaseMode::None;
                    c.to_uppercase().collect::<String>()
                }
                (CaseMode::Lower, _) => {
                    *one = CaseMode::None;
                    c.to_lowercase().collect::<String>()
                }
                (_, CaseMode::Upper) => c.to_uppercase().collect::<String>(),
                (_, CaseMode::Lower) => c.to_lowercase().collect::<String>(),
                _ => c.to_string(),
            };
            out.push_str(&mapped);
        }
    };

    let chars: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c == '&' {
            push(&mut out, group(0), &mut run, &mut one);
            continue;
        }
        if c != '\\' {
            push(&mut out, &c.to_string(), &mut run, &mut one);
            continue;
        }
        let Some(&n) = chars.get(i) else {
            out.push('\\');
            break;
        };
        i += 1;
        match n {
            '0'..='9' => {
                let idx = n as usize - '0' as usize;
                push(&mut out, group(idx), &mut run, &mut one);
            }
            'u' => one = CaseMode::Upper,
            'l' => one = CaseMode::Lower,
            'U' => run = CaseMode::Upper,
            'L' => run = CaseMode::Lower,
            'e' | 'E' => {
                run = CaseMode::None;
                one = CaseMode::None;
            }
            'r' => out.push('\n'),
            // Vim's `\n` in a replacement inserts a <NUL>, not a line break.
            'n' => out.push('\0'),
            't' => out.push('\t'),
            '\\' => push(&mut out, "\\", &mut run, &mut one),
            other => push(&mut out, &other.to_string(), &mut run, &mut one),
        }
    }
    out
}

/// Does `cmd` start with one of the ex commands that `try_execute_ranged_command`
/// handles, so it is worth parsing a range for even without a leading address?
pub(crate) fn is_ranged_ex_name(cmd: &str) -> bool {
    let (name, _) = split_ex_name(cmd);
    let name = name.strip_suffix('!').unwrap_or(name);
    if name.is_empty() {
        return false;
    }
    [
        "delete", "yank", "join", "copy", "move", "put", "retab", "left", "right", "center",
    ]
    .iter()
    .any(|full| full.starts_with(name))
        || name == "t"
}

/// Split a `:sort` argument string into its letter flags and optional
/// `/{pattern}/`.
///
/// Vim lets the pattern and the letter flags appear in either order and
/// interleaved (`:sort /pat/ r` puts `r` *after* the pattern), so this scans
/// the whole string rather than splitting once: any ASCII letter is a flag,
/// and any other non-blank character opens a delimited pattern that runs
/// (honouring `\`-escapes) to the next unescaped occurrence of that same
/// delimiter.
pub(crate) fn parse_sort_spec(spec: &str) -> (String, Option<String>) {
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut flags = String::new();
    let mut pattern: Option<String> = None;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch.is_ascii_alphabetic() {
            flags.push(ch);
            i += 1;
            continue;
        }
        let delim = ch;
        i += 1;
        let mut pat = String::new();
        while i < chars.len() && chars[i] != delim {
            if chars[i] == '\\' && i + 1 < chars.len() {
                pat.push(chars[i]);
                pat.push(chars[i + 1]);
                i += 2;
                continue;
            }
            pat.push(chars[i]);
            i += 1;
        }
        if i < chars.len() {
            i += 1; // skip the closing delimiter
        }
        pattern = Some(pat);
    }
    (flags, pattern)
}

/// Split `rest` into an ex command name and its argument.
///
/// The name runs to the first character that cannot be part of one — a space,
/// a digit, or one of the address characters `. $ ' + - / ?` that start a
/// destination (`:t$`, `:m+1`, `:co0`). A trailing `!` stays with the name.
pub(crate) fn split_ex_name(rest: &str) -> (&str, &str) {
    let mut end = 0usize;
    for (i, c) in rest.char_indices() {
        if c.is_ascii_alphabetic() {
            end = i + c.len_utf8();
            continue;
        }
        if c == '!' && i > 0 {
            end = i + c.len_utf8();
        }
        break;
    }
    (&rest[..end], rest[end..].trim())
}

/// Parse the optional `[register] [count]` argument shared by `:d` and `:y`.
///
/// Returns `None` when the argument is neither, so the caller can decline the
/// command rather than silently doing the wrong thing.
#[allow(clippy::type_complexity)]
pub(crate) fn parse_reg_and_count(args: &str) -> Option<(Option<char>, Option<usize>)> {
    let mut reg = None;
    let mut count = None;
    for tok in args.split_whitespace() {
        if let Ok(n) = tok.parse::<usize>() {
            if n == 0 {
                return None;
            }
            count = Some(n);
        } else if tok.chars().count() == 1
            && tok
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            reg = Some(tok.chars().next()?);
        } else {
            return None;
        }
    }
    Some((reg, count))
}

/// Strip an abbreviated ex command name from `rest`.
///
/// Vim lets any unambiguous prefix stand in for the full name (`:g`, `:gl`,
/// `:global`), optionally followed by `!`. Returns the remainder after the name
/// (and after the `!`), or `None` when `rest` does not start with the command.
pub(crate) fn strip_command_name<'a>(rest: &'a str, full: &str) -> Option<&'a str> {
    let matched = full
        .char_indices()
        .take_while(|&(k, c)| rest.chars().nth(k) == Some(c))
        .count();
    if matched == 0 {
        return None;
    }
    let after = &rest[matched..];
    Some(after.strip_prefix('!').unwrap_or(after))
}

/// Split a `:set` argument list into individual options.
///
/// Vim allows several options per `:set` (`:set ic scs`, `:set noet ts=4`); a
/// backslash escapes a space that belongs to a value.
pub(crate) fn split_set_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = args.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Does a single `:set` argument (one entry from [`split_set_args`], or a
/// whole single-option `:set` line) name `'foldmethod'`/`'foldlevel'`
/// (either full name or abbreviation)? Used to skip the
/// `apply_foldlevel` recompute on `:set` lines that have nothing to do with
/// folding — see the two call sites in `handle_ex_command` (#1153 review:
/// re-running the indent-fold pass on every `:set ic` etc. was a needless
/// cost on large files).
fn set_arg_touches_folding(arg: &str) -> bool {
    let arg = arg.trim();
    let arg = arg.strip_suffix('?').unwrap_or(arg);
    let arg = arg.strip_suffix('!').unwrap_or(arg);
    let name = arg.split('=').next().unwrap_or(arg);
    matches!(name, "foldmethod" | "fdm" | "foldlevel" | "fdl")
}

/// One parsed `/` or `?` command line.
pub(crate) struct SearchCmdline {
    /// The Vim pattern, empty when the user typed `//` or a bare `/`.
    pub pattern: String,
    /// Search offset (`e`, `e+1`, `b+2`, `+1`, `-1`, `0`), empty when absent.
    pub offset: String,
    /// A `;`-chained follow-up search: its direction and its own raw command line.
    pub chained: Option<(SearchDirection, String)>,
}

/// Split a search command line into pattern, offset and `;`-chained follow-up.
///
/// `raw` is the text after the leading `/` or `?`; `delim` is that same
/// character, which terminates the pattern unless backslash-escaped.
pub(crate) fn split_search_cmdline(raw: &str, delim: char) -> SearchCmdline {
    let chars: Vec<char> = raw.chars().collect();
    let mut pattern = String::new();
    let mut i = 0;
    let mut closed = false;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            pattern.push(chars[i]);
            pattern.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == delim {
            closed = true;
            i += 1;
            break;
        }
        pattern.push(chars[i]);
        i += 1;
    }

    let mut offset = String::new();
    if closed {
        while i < chars.len() && matches!(chars[i], 'e' | 's' | 'b' | '+' | '-' | '0'..='9') {
            offset.push(chars[i]);
            i += 1;
        }
    }

    let mut chained = None;
    if i < chars.len() && chars[i] == ';' {
        if let Some(&next) = chars.get(i + 1) {
            let dir = match next {
                '/' => Some(SearchDirection::Forward),
                '?' => Some(SearchDirection::Backward),
                _ => None,
            };
            if let Some(dir) = dir {
                chained = Some((dir, chars[i + 2..].iter().collect::<String>()));
            }
        }
    }

    SearchCmdline {
        pattern,
        offset,
        chained,
    }
}

/// Matches `rest` against command name `name` followed by a valid argument
/// separator (space, digit, `+`, `-`, `.`, `$`). Returns the trimmed argument
/// if matched, else None.
///
/// Examples:
/// - split_cmd_and_arg("t3", "t") → Some("3")
/// - split_cmd_and_arg("move 5", "move") → Some("5")
/// - split_cmd_and_arg("term", "t") → None (char 'e' is not a valid separator)
fn split_cmd_and_arg<'a>(rest: &'a str, name: &str) -> Option<&'a str> {
    let rest = rest.strip_prefix(name)?;
    if rest.is_empty() {
        return None;
    }
    let first = rest.chars().next()?;
    if first == ' '
        || first.is_ascii_digit()
        || first == '+'
        || first == '-'
        || first == '.'
        || first == '$'
    {
        Some(rest.trim())
    } else {
        None
    }
}

/// `:registers` type column — Vim prints `c`, `l` or `b` (`:h :registers`).
fn reg_type_letter(ty: RegType) -> &'static str {
    match ty {
        RegType::Charwise => "c",
        RegType::Linewise => "l",
        RegType::Blockwise => "b",
    }
}

/// Validate and materialize the persisted-string keymap entries for a
/// vim-style `:{cmd} {lhs} {rhs}` definition, one per targeted mode.
///
/// Returns `None` if `lhs`/`rhs` don't form a valid mapping (empty, or an
/// `{rhs}` that fails key-notation parsing). Reuses [`parse_keymap_def`] as
/// the single source of truth for validity, so a `:nnoremap` definition and a
/// hand-edited `settings.json` line can never disagree about what's valid
/// (#1151).
fn build_keymap_entries(
    lhs: &str,
    rhs: &str,
    modes: &[&str],
    noremap: bool,
) -> Option<Vec<String>> {
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }
    let bang = if noremap { "!" } else { "" };
    let mut entries = Vec::with_capacity(modes.len());
    for m in modes {
        let entry = format!("{m}{bang} {lhs} {rhs}");
        parse_keymap_def(&entry)?;
        entries.push(entry);
    }
    Some(entries)
}
