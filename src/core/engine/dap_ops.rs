use super::*;

impl Engine {
    // ── DAP (Debug Adapter Protocol) ─────────────────────────────────────────

    /// Toggle a breakpoint on `line` (1-based) in `file`.
    /// Keeps the per-file list sorted ascending. Re-sends `setBreakpoints`
    /// to the adapter if a session is currently active.
    pub fn dap_toggle_breakpoint(&mut self, file: &str, line: u64) {
        let bps = self.dap_breakpoints.entry(file.to_string()).or_default();
        if let Some(pos) = bps.iter().position(|bp| bp.line == line) {
            bps.remove(pos);
            self.message = format!("Breakpoint removed: line {line}");
        } else {
            let insert_pos = bps.partition_point(|bp| bp.line < line);
            bps.insert(insert_pos, BreakpointInfo::new(line));
            self.message = format!("Breakpoint set: line {line}");
        }
        self.dap_send_breakpoints_for_file(file);
    }

    /// Set a condition on an existing breakpoint, or create a conditional breakpoint.
    #[allow(dead_code)]
    pub fn dap_set_breakpoint_condition(
        &mut self,
        file: &str,
        line: u64,
        condition: Option<String>,
    ) {
        let bps = self.dap_breakpoints.entry(file.to_string()).or_default();
        if let Some(bp) = bps.iter_mut().find(|bp| bp.line == line) {
            bp.condition = condition.clone();
            self.message = if condition.is_some() {
                format!("Breakpoint condition set: line {line}")
            } else {
                format!("Breakpoint condition cleared: line {line}")
            };
        } else {
            // Create a new breakpoint with the condition.
            let insert_pos = bps.partition_point(|bp| bp.line < line);
            let mut bp = BreakpointInfo::new(line);
            bp.condition = condition;
            bps.insert(insert_pos, bp);
            self.message = format!("Conditional breakpoint set: line {line}");
        }
        self.dap_send_breakpoints_for_file(file);
    }

    /// Set a hit-count condition on a breakpoint.
    #[allow(dead_code)]
    pub fn dap_set_breakpoint_hit_condition(
        &mut self,
        file: &str,
        line: u64,
        hit_condition: Option<String>,
    ) {
        let bps = self.dap_breakpoints.entry(file.to_string()).or_default();
        if let Some(bp) = bps.iter_mut().find(|bp| bp.line == line) {
            bp.hit_condition = hit_condition;
            self.message = format!("Hit condition set: line {line}");
        } else {
            let insert_pos = bps.partition_point(|bp| bp.line < line);
            let mut bp = BreakpointInfo::new(line);
            bp.hit_condition = hit_condition;
            bps.insert(insert_pos, bp);
            self.message = format!("Conditional breakpoint set: line {line}");
        }
        self.dap_send_breakpoints_for_file(file);
    }

    /// Set a log message on a breakpoint (logpoint).
    #[allow(dead_code)]
    pub fn dap_set_breakpoint_log_message(
        &mut self,
        file: &str,
        line: u64,
        log_message: Option<String>,
    ) {
        let bps = self.dap_breakpoints.entry(file.to_string()).or_default();
        if let Some(bp) = bps.iter_mut().find(|bp| bp.line == line) {
            bp.log_message = log_message;
            self.message = format!("Log message set: line {line}");
        } else {
            let insert_pos = bps.partition_point(|bp| bp.line < line);
            let mut bp = BreakpointInfo::new(line);
            bp.log_message = log_message;
            bps.insert(insert_pos, bp);
            self.message = format!("Logpoint set: line {line}");
        }
        self.dap_send_breakpoints_for_file(file);
    }

    /// Re-send breakpoints for a given file to the adapter (if session is live).
    fn dap_send_breakpoints_for_file(&mut self, file: &str) {
        let bps: Vec<BreakpointInfo> = self.dap_breakpoints.get(file).cloned().unwrap_or_default();
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.set_breakpoints(file, &bps);
            }
        }
    }

    /// Start a debug session for the given language.
    ///
    /// Launch config resolution order:
    /// 1. `.vimcode/launch.json` (our native folder)
    /// 2. `.vscode/launch.json`  (migration: copy to `.vimcode/` on first use)
    /// 3. Generate a new config and write to `.vimcode/launch.json`
    pub fn dap_start_debug(&mut self, lang: &str) {
        // Determine the workspace root — walk up from cwd until a project
        // manifest (Cargo.toml, package.json, .git, …) is found.
        let manifests = self.ext_available_manifests();
        let workspace_root = crate::core::dap_manager::find_workspace_root(&self.cwd, &manifests);
        let cwd = workspace_root.to_string_lossy().into_owned();
        let vimcode_dir = workspace_root.join(".vimcode");
        let launch_json_path = vimcode_dir.join("launch.json");

        // Migration: if .vscode/launch.json exists and .vimcode/launch.json doesn't,
        // copy it over so the user's existing VSCode config is preserved.
        let vscode_path = workspace_root.join(".vscode").join("launch.json");
        if !launch_json_path.exists() && vscode_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&vscode_path) {
                let _ = std::fs::create_dir_all(&vimcode_dir);
                let _ = std::fs::write(&launch_json_path, &content);
            }
        }

        // Migration: same for tasks.json.
        let tasks_json_path = vimcode_dir.join("tasks.json");
        let vscode_tasks_path = workspace_root.join(".vscode").join("tasks.json");
        if !tasks_json_path.exists() && vscode_tasks_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&vscode_tasks_path) {
                let _ = std::fs::create_dir_all(&vimcode_dir);
                let _ = std::fs::write(&tasks_json_path, &content);
            }
        }

        // Try to parse an existing launch.json, or generate a fresh one.
        let configs = if let Ok(content) = std::fs::read_to_string(&launch_json_path) {
            let parsed = parse_launch_json(&content, &cwd);
            if parsed.is_empty() {
                // File exists but is unparseable — generate and overwrite.
                let generated = generate_launch_json(lang, &cwd);
                let _ = std::fs::create_dir_all(&vimcode_dir);
                let _ = std::fs::write(&launch_json_path, &generated);
                parse_launch_json(&generated, &cwd)
            } else {
                parsed
            }
        } else {
            // No launch.json — generate it.
            let generated = generate_launch_json(lang, &cwd);
            let _ = std::fs::create_dir_all(&vimcode_dir);
            let _ = std::fs::write(&launch_json_path, &generated);
            parse_launch_json(&generated, &cwd)
        };

        // Store configs and select the current one.
        let cfg_idx = self
            .dap_selected_launch_config
            .min(configs.len().saturating_sub(1));
        let mut config = if configs.is_empty() {
            // Absolute fallback: synthesise a minimal config.
            LaunchConfig {
                name: "Debug".to_string(),
                adapter_type: lang.to_string(),
                request: "launch".to_string(),
                program: String::new(),
                args: Vec::new(),
                cwd: cwd.clone(),
                raw: serde_json::Value::Null,
            }
        } else {
            configs[cfg_idx].clone()
        };
        self.dap_launch_configs = configs;
        self.dap_selected_launch_config = cfg_idx;

        // Resolve ${file} to the currently-active buffer's file path.
        // This is a standard VSCode variable used by language adapters like
        // debugpy that default to debugging the file open in the editor.
        if config.program.contains("${file}") {
            if let Some(path) = self.file_path().map(|p| p.to_string_lossy().into_owned()) {
                config.program = config.program.replace("${file}", &path);
            }
        }

        // --- preLaunchTask support ---
        // If the config references a preLaunchTask and we haven't run it yet,
        // execute it via the LSP install infrastructure and return early.
        // Once the task completes, `poll_lsp` will call `dap_start_debug` again.
        let pre_launch_task = config
            .raw
            .get("preLaunchTask")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(ref task_label) = pre_launch_task {
            if !self.dap_pre_launch_done {
                // Load and parse tasks.json
                let tasks_json_path = vimcode_dir.join("tasks.json");
                let tasks = if let Ok(content) = std::fs::read_to_string(&tasks_json_path) {
                    parse_tasks_json(&content, &cwd)
                } else {
                    Vec::new()
                };

                if let Some(task) = tasks.iter().find(|t| t.label == *task_label) {
                    let cmd = if task.cwd != cwd {
                        format!("cd '{}' && {}", task.cwd, task_to_shell_command(task))
                    } else {
                        task_to_shell_command(task)
                    };
                    self.dap_deferred_lang = Some(lang.to_string());
                    self.ensure_lsp_manager();
                    let install_key = format!("dap_task:{task_label}");
                    self.lsp_installing.insert(install_key.clone());
                    if let Some(mgr) = &self.lsp_manager {
                        mgr.run_install_command(&install_key, &cmd);
                    }

                    // Set up UI state so the user sees progress.
                    self.dap_output_lines.clear();
                    self.bottom_panel_kind = BottomPanelKind::DebugOutput;
                    self.bottom_panel_open = true;
                    self.dap_wants_sidebar = true;
                    if self.session.terminal_panel_rows == 0 {
                        self.session.terminal_panel_rows = 10;
                    }
                    self.dap_session_active = true;
                    self.dap_output_lines
                        .push(format!("[dap] Running pre-launch task: {task_label}"));
                    self.dap_output_lines.push(format!("[dap] command: {cmd}"));
                    self.message = format!("Running pre-launch task: {task_label}\u{2026}");
                    return;
                } else {
                    self.dap_output_lines.push(format!(
                        "[dap] warning: preLaunchTask '{task_label}' not found in tasks.json"
                    ));
                }
            }
        }

        // Determine the adapter registry name.
        let adapter_lang = type_to_adapter(&config.adapter_type).unwrap_or(lang);

        // Ensure a DapManager exists.
        if self.dap_manager.is_none() {
            self.dap_manager = Some(DapManager::new());
        }
        let manifests = self.ext_available_manifests();
        let mgr = self.dap_manager.as_mut().unwrap();

        if let Err(e) = mgr.start_adapter(adapter_lang, &manifests) {
            self.message = format!("DAP: {e}");
            return;
        }

        // Build launch arguments: start from ALL fields in the raw config,
        // then overwrite the three path-bearing fields with substituted values.
        // VSCode sends the full launch.json entry (including `type`, `request`,
        // `name`) as the DAP `launch` arguments — some adapters (codelldb)
        // rely on fields like `type` or `request` being present.
        let mut extra = if let Some(obj) = config.raw.as_object() {
            obj.iter()
                .filter(|(k, _)| {
                    // Only skip the three fields we re-add with substituted values.
                    !matches!(k.as_str(), "program" | "cwd" | "args")
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<String, serde_json::Value>>()
        } else {
            serde_json::Map::new()
        };

        // For debugpy: tell the adapter which Python to use for running the
        // user's script.  The adapter itself runs from the debugpy-venv, but
        // the user's project venv (if active) must be used to find installed
        // packages (e.g. fastapi, django, etc.).
        // Respects any `python`/`pythonPath` already set in launch.json.
        if adapter_lang == "debugpy" {
            // Only inject `python` when neither field is already in launch.json.
            // `python` and `pythonPath` are mutually exclusive in debugpy.adapter;
            // sending both causes "pythonPath is not valid if python is specified".
            if !extra.contains_key("python") && !extra.contains_key("pythonPath") {
                let project_python =
                    crate::core::dap_manager::find_project_python_in(Some(&workspace_root))
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "python3".to_string());
                extra.insert(
                    "python".to_string(),
                    serde_json::Value::String(project_python),
                );
            }
        }

        // sourceLanguages helps codelldb map Rust source files.
        if adapter_lang == "codelldb" || config.adapter_type == "lldb" {
            extra
                .entry("sourceLanguages".to_string())
                .or_insert_with(|| serde_json::json!(["rust"]));
            // Default stdio to null so the debuggee's stdout/stderr doesn't
            // flood the Debug Output panel (especially noisy for TUI apps).
            // Users can override this in launch.json (e.g. "stdio": "/dev/pts/0").
            extra
                .entry("stdio".to_string())
                .or_insert(serde_json::Value::Null);
        }
        extra
            .entry("stopOnEntry".to_string())
            .or_insert_with(|| serde_json::json!(false));

        let mut launch_args = extra;
        launch_args.insert(
            "program".to_string(),
            serde_json::Value::String(config.program.clone()),
        );
        launch_args.insert(
            "cwd".to_string(),
            serde_json::Value::String(config.cwd.clone()),
        );
        launch_args.insert(
            "args".to_string(),
            serde_json::Value::Array(
                config
                    .args
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            ),
        );
        let launch_args = serde_json::Value::Object(launch_args);

        // Clear previous output and switch to Debug Output tab so the user
        // can see all diagnostic logs without any extra commands.
        self.dap_output_lines.clear();
        self.dap_ansi_carry.clear();
        self.bottom_panel_kind = BottomPanelKind::DebugOutput;
        // Open the bottom panel even if no terminal is running.
        // Only bump rows if completely collapsed (0) to avoid overriding user preferences.
        self.bottom_panel_open = true;
        self.dap_wants_sidebar = true;
        if self.session.terminal_panel_rows == 0 {
            self.session.terminal_panel_rows = 10;
        }

        // Diagnostic logs — visible immediately in the Debug Output tab.
        self.dap_output_lines
            .push(format!("[dap] workspace: {cwd}"));
        self.dap_output_lines
            .push(format!("[dap] launch.json: {}", launch_json_path.display()));
        self.dap_output_lines.push(format!(
            "[dap] config.program: {:?}  args: {:?}",
            config.program, config.args
        ));

        // Guard: empty program path means the launch.json is misconfigured.
        // (Attach configs don't need a program path.)
        if config.program.is_empty() && config.request != "attach" {
            self.message = format!("DAP: program not set — edit {}", launch_json_path.display());
            self.dap_session_active = false;
            self.debug_toolbar_visible = false;
            return;
        }

        // Dump the complete JSON we're about to send so mismatches are obvious.
        self.dap_output_lines
            .push(format!("[dap] {} request: {launch_args}", config.request));

        // Store launch_args for deferred sending — we must wait for the
        // `initialize` response before sending `launch`/`attach`.  Sending both
        // at once causes codelldb to process them concurrently, and it reads the
        // `launch` arguments before its LLDB session is fully initialised,
        // producing "executable doesn't exist: '(empty)'" even when `program`
        // is correctly set in the JSON we send.
        self.dap_pending_launch = Some((config.request.clone(), launch_args));
        self.dap_seq_launch = None; // will be assigned once launch is actually sent
        self.dap_seq_initialize = None;

        let adapter_name = mgr.adapter.as_deref().unwrap_or("unknown");
        if let Some(server) = mgr.server.as_mut() {
            let init_seq = server.initialize(adapter_name);
            self.dap_seq_initialize = Some(init_seq);
            self.dap_output_lines
                .push(format!("[dap] sent initialize (seq={init_seq}), waiting…"));
        }

        self.dap_session_active = true;
        self.debug_toolbar_visible = true;
        self.message = format!("DAP: starting {} debug session\u{2026}", config.name);
    }

    /// Continue execution of the stopped thread.
    pub fn dap_continue(&mut self) {
        let tid = self.dap_stopped_thread.unwrap_or(0);
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.continue_thread(tid);
                self.message = "DAP: continue".to_string();
                return;
            }
        }
        self.message = "DAP: no active session".to_string();
    }

    /// Pause all threads.
    pub fn dap_pause(&mut self) {
        let tid = self.dap_stopped_thread.unwrap_or(0);
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.pause(tid);
                self.message = "DAP: pause".to_string();
                return;
            }
        }
        self.message = "DAP: no active session".to_string();
    }

    /// Stop the debug session (disconnect adapter).
    pub fn dap_stop(&mut self) {
        if let Some(mgr) = &mut self.dap_manager {
            mgr.stop();
        }
        self.dap_session_active = false;
        self.debug_toolbar_visible = false;
        self.dap_pending_launch = None;
        self.dap_stopped_thread = None;
        self.dap_seq_initialize = None;
        self.dap_seq_launch = None;
        self.dap_active_frame = 0;
        self.dap_expanded_vars.clear();
        self.dap_child_variables.clear();
        self.dap_eval_result = None;
        self.dap_pending_vars_ref = 0;
        self.dap_primary_scope_name.clear();
        self.dap_primary_scope_ref = 0;
        self.dap_scope_groups.clear();
        self.dap_watch_values = vec![None; self.dap_watch_expressions.len()];
        self.dap_pre_launch_done = false;
        self.dap_deferred_lang = None;
        self.dap_sidebar_scroll = [0; 4];
        self.message = "DAP: session stopped".to_string();
    }

    /// Add a watch expression to the debug sidebar.
    pub fn dap_add_watch(&mut self, expr: String) {
        self.dap_watch_expressions.push(expr);
        self.dap_watch_values.push(None);
    }

    /// Remove a watch expression by index.
    #[allow(dead_code)]
    pub fn dap_remove_watch(&mut self, idx: usize) {
        if idx < self.dap_watch_expressions.len() {
            self.dap_watch_expressions.remove(idx);
            self.dap_watch_values.remove(idx);
        }
    }

    /// Select a stack frame by index, clamping to the valid range.
    /// Re-requests scopes/variables for the new frame.
    #[allow(dead_code)]
    pub fn dap_select_frame(&mut self, idx: usize) {
        let max = self.dap_stack_frames.len().saturating_sub(1);
        self.dap_active_frame = idx.min(max);
        self.dap_variables.clear();
        self.dap_child_variables.clear();
        self.dap_expanded_vars.clear();
        self.dap_primary_scope_name.clear();
        self.dap_primary_scope_ref = 0;
        self.dap_scope_groups.clear();
        let frame_id = self
            .dap_stack_frames
            .get(self.dap_active_frame)
            .map(|f| f.id)
            .unwrap_or(0);
        if frame_id > 0 {
            self.dap_pending_vars_ref = 0; // top-level fetch
            if let Some(mgr) = &mut self.dap_manager {
                if let Some(server) = &mut mgr.server {
                    server.scopes(frame_id);
                }
            }
        }
    }

    /// Toggle expansion of a variable with the given `var_ref`.
    /// If already expanded, collapses it; otherwise requests child variables.
    /// Synthetic refs (high bit set) represent the client-side "Non-Public Members"
    /// group — data is already stored locally, no server fetch needed.
    pub fn dap_toggle_expand_var(&mut self, var_ref: u64) {
        let is_synthetic = var_ref & SYNTHETIC_NON_PUBLIC_MASK != 0;
        if self.dap_expanded_vars.contains(&var_ref) {
            self.dap_expanded_vars.remove(&var_ref);
            if var_ref == self.dap_primary_scope_ref {
                // Primary scope: data lives in dap_variables, keep it.
            } else if is_synthetic {
                // Synthetic group: keep the data for re-expansion without re-fetch.
            } else {
                self.dap_child_variables.remove(&var_ref);
                // Also clean up any synthetic Non-Public Members group for this var.
                let synthetic = var_ref | SYNTHETIC_NON_PUBLIC_MASK;
                self.dap_expanded_vars.remove(&synthetic);
                self.dap_child_variables.remove(&synthetic);
            }
        } else {
            self.dap_expanded_vars.insert(var_ref);
            // Primary scope: data already in dap_variables — no re-fetch needed.
            // Synthetic ref: data already in dap_child_variables — no fetch needed.
            if var_ref != self.dap_primary_scope_ref && !is_synthetic {
                self.dap_pending_vars_ref = var_ref; // child fetch
                if let Some(mgr) = &mut self.dap_manager {
                    if let Some(server) = &mut mgr.server {
                        server.variables(var_ref);
                    }
                }
            }
        }
    }

    /// Evaluate an expression in the context of the active frame.
    /// Result is stored in `dap_eval_result` when the response arrives.
    pub fn dap_eval(&mut self, expr: &str) {
        let frame_id = self
            .dap_stack_frames
            .get(self.dap_active_frame)
            .map(|f| f.id)
            .unwrap_or(0);
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.evaluate(expr, frame_id);
                self.message = format!("DAP: evaluating `{expr}`…");
                return;
            }
        }
        self.message = "DAP: no active session".to_string();
    }

    /// Compute the number of visible items in the currently active debug sidebar section.
    pub fn dap_sidebar_section_len(&self) -> usize {
        self.dap_sidebar_section_item_count(self.dap_sidebar_section)
    }

    /// Compute the number of items in a specific debug sidebar section.
    pub fn dap_sidebar_section_item_count(&self, section: DebugSidebarSection) -> usize {
        match section {
            DebugSidebarSection::Variables => self.dap_var_flat_count(),
            DebugSidebarSection::Watch => self.dap_watch_expressions.len(),
            DebugSidebarSection::CallStack => self.dap_stack_frames.len(),
            DebugSidebarSection::Breakpoints => {
                self.dap_breakpoints.values().map(|v| v.len()).sum()
            }
        }
    }

    /// Map a `DebugSidebarSection` variant to its index (0–3).
    pub fn dap_sidebar_section_index(section: DebugSidebarSection) -> usize {
        match section {
            DebugSidebarSection::Variables => 0,
            DebugSidebarSection::Watch => 1,
            DebugSidebarSection::CallStack => 2,
            DebugSidebarSection::Breakpoints => 3,
        }
    }

    /// Adjust the scroll offset for the active section so that the selected item is visible.
    pub fn dap_sidebar_ensure_visible(&mut self) {
        let idx = Self::dap_sidebar_section_index(self.dap_sidebar_section);
        let height = self.dap_sidebar_section_heights[idx] as usize;
        if height == 0 {
            return; // not yet laid out
        }
        let scroll = &mut self.dap_sidebar_scroll[idx];
        let sel = self.dap_sidebar_selected;
        if sel < *scroll {
            *scroll = sel;
        }
        if sel >= *scroll + height {
            *scroll = sel - height + 1;
        }
    }

    /// Resize adjacent debug sidebar sections by `delta` rows.
    /// Shrinks `section_idx` and grows `section_idx + 1` (or vice versa for negative delta).
    /// Clamps each section to a minimum of 1 row and keeps the total constant.
    #[allow(dead_code)]
    pub fn dap_sidebar_resize_section(&mut self, section_idx: usize, delta: i16) {
        if section_idx >= 3 {
            return; // no next section to trade rows with
        }
        let a = self.dap_sidebar_section_heights[section_idx] as i16;
        let b = self.dap_sidebar_section_heights[section_idx + 1] as i16;
        let new_a = (a + delta).max(1).min(a + b - 1);
        let new_b = a + b - new_a;
        self.dap_sidebar_section_heights[section_idx] = new_a as u16;
        self.dap_sidebar_section_heights[section_idx + 1] = new_b as u16;
    }

    /// Count total items in the flat variable tree (including scope headers,
    /// expanded children, and additional scope groups).
    pub(crate) fn dap_var_flat_count(&self) -> usize {
        let mut count = 0;
        // Primary scope header (e.g. "Locals") if we have scope info.
        if self.dap_primary_scope_ref > 0 {
            count += 1; // header row
            if self.dap_expanded_vars.contains(&self.dap_primary_scope_ref) {
                for v in &self.dap_variables {
                    count += 1;
                    if v.var_ref > 0 && self.dap_expanded_vars.contains(&v.var_ref) {
                        count += self.dap_var_subtree_count(v.var_ref);
                    }
                }
            }
        } else {
            // No scope info (e.g. tests): show variables at root level.
            for v in &self.dap_variables {
                count += 1;
                if v.var_ref > 0 && self.dap_expanded_vars.contains(&v.var_ref) {
                    count += self.dap_var_subtree_count(v.var_ref);
                }
            }
        }
        // Additional scope groups (e.g. "Statics", "Registers").
        for &(_, var_ref) in &self.dap_scope_groups {
            count += 1; // group header row
            if self.dap_expanded_vars.contains(&var_ref) {
                count += self.dap_var_subtree_count(var_ref);
            }
        }
        count
    }

    /// Recursively count children in an expanded variable subtree.
    fn dap_var_subtree_count(&self, var_ref: u64) -> usize {
        let children = match self.dap_child_variables.get(&var_ref) {
            Some(c) => c,
            None => return 0,
        };
        let mut count = children.len();
        for child in children {
            if child.var_ref > 0 && self.dap_expanded_vars.contains(&child.var_ref) {
                count += self.dap_var_subtree_count(child.var_ref);
            }
        }
        count
    }

    /// Find the DapVariable at the given flat index in the Variables section.
    /// Returns the variable reference ID (0 if non-expandable).
    pub(crate) fn dap_var_ref_at_flat_index(&self, target: usize) -> Option<u64> {
        let mut flat = 0;
        if self.dap_primary_scope_ref > 0 {
            // Primary scope header.
            if flat == target {
                return Some(self.dap_primary_scope_ref);
            }
            flat += 1;
            if self.dap_expanded_vars.contains(&self.dap_primary_scope_ref) {
                for v in &self.dap_variables {
                    if flat == target {
                        return Some(v.var_ref);
                    }
                    flat += 1;
                    if v.var_ref > 0 && self.dap_expanded_vars.contains(&v.var_ref) {
                        if let Some(result) =
                            self.dap_var_ref_in_children(v.var_ref, target, &mut flat)
                        {
                            return Some(result);
                        }
                    }
                }
            }
        } else {
            // No scope info (e.g. tests): variables at root level.
            for v in &self.dap_variables {
                if flat == target {
                    return Some(v.var_ref);
                }
                flat += 1;
                if v.var_ref > 0 && self.dap_expanded_vars.contains(&v.var_ref) {
                    if let Some(result) = self.dap_var_ref_in_children(v.var_ref, target, &mut flat)
                    {
                        return Some(result);
                    }
                }
            }
        }
        // Additional scope groups.
        for &(_, var_ref) in &self.dap_scope_groups {
            if flat == target {
                return Some(var_ref);
            }
            flat += 1;
            if self.dap_expanded_vars.contains(&var_ref) {
                if let Some(result) = self.dap_var_ref_in_children(var_ref, target, &mut flat) {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Recursively search expanded children for a flat index.
    fn dap_var_ref_in_children(
        &self,
        parent_ref: u64,
        target: usize,
        flat: &mut usize,
    ) -> Option<u64> {
        let children = self.dap_child_variables.get(&parent_ref)?;
        for child in children {
            if *flat == target {
                return Some(child.var_ref);
            }
            *flat += 1;
            if child.var_ref > 0 && self.dap_expanded_vars.contains(&child.var_ref) {
                if let Some(result) = self.dap_var_ref_in_children(child.var_ref, target, flat) {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Resolve a flat breakpoint sidebar index into (file_path, BreakpointInfo index).
    fn dap_bp_at_flat_index(&self, target: usize) -> Option<(String, usize)> {
        let mut sorted: Vec<_> = self.dap_breakpoints.iter().collect();
        sorted.sort_by_key(|(path, _)| path.as_str());
        let mut flat = 0;
        for (path, bps) in &sorted {
            for (i, _bp) in bps.iter().enumerate() {
                if flat == target {
                    return Some(((*path).clone(), i));
                }
                flat += 1;
            }
        }
        None
    }

    /// Handle a key press directed at the debug sidebar.
    /// j/k or Up/Down navigate within the active section; Tab switches sections;
    /// Enter/Space expand/collapse variables, navigate call stack, jump to breakpoint;
    /// x/d delete watch expressions or breakpoints; q/Escape unfocus.
    pub fn handle_debug_sidebar_key(&mut self, key_name: &str, _ctrl: bool) -> EngineAction {
        let section_len = self.dap_sidebar_section_len();
        match key_name {
            "Escape" | "q" => {
                self.dap_sidebar_has_focus = false;
            }
            "Down" | "j" => {
                if section_len > 0 {
                    self.dap_sidebar_selected =
                        (self.dap_sidebar_selected + 1).min(section_len - 1);
                }
                self.dap_sidebar_ensure_visible();
            }
            "Up" | "k" => {
                self.dap_sidebar_selected = self.dap_sidebar_selected.saturating_sub(1);
                self.dap_sidebar_ensure_visible();
            }
            "Home" | "g" => {
                // Go to top of section (gg in vim).
                self.dap_sidebar_selected = 0;
                self.dap_sidebar_ensure_visible();
            }
            "End" | "G" => {
                // Go to bottom of section.
                if section_len > 0 {
                    self.dap_sidebar_selected = section_len - 1;
                }
                self.dap_sidebar_ensure_visible();
            }
            "PageDown" => {
                let idx = Self::dap_sidebar_section_index(self.dap_sidebar_section);
                let page = (self.dap_sidebar_section_heights[idx] as usize).max(1);
                if section_len > 0 {
                    self.dap_sidebar_selected =
                        (self.dap_sidebar_selected + page).min(section_len - 1);
                }
                self.dap_sidebar_ensure_visible();
            }
            "PageUp" => {
                let idx = Self::dap_sidebar_section_index(self.dap_sidebar_section);
                let page = (self.dap_sidebar_section_heights[idx] as usize).max(1);
                self.dap_sidebar_selected = self.dap_sidebar_selected.saturating_sub(page);
                self.dap_sidebar_ensure_visible();
            }
            "ScrollDown" => {
                // Scroll the active section down by 3 lines (mouse wheel).
                let idx = Self::dap_sidebar_section_index(self.dap_sidebar_section);
                let height = self.dap_sidebar_section_heights[idx] as usize;
                let max_scroll = section_len.saturating_sub(height);
                self.dap_sidebar_scroll[idx] = (self.dap_sidebar_scroll[idx] + 3).min(max_scroll);
            }
            "ScrollUp" => {
                // Scroll the active section up by 3 lines (mouse wheel).
                let idx = Self::dap_sidebar_section_index(self.dap_sidebar_section);
                self.dap_sidebar_scroll[idx] = self.dap_sidebar_scroll[idx].saturating_sub(3);
            }
            "Tab" => {
                self.dap_sidebar_section = match self.dap_sidebar_section {
                    DebugSidebarSection::Variables => DebugSidebarSection::Watch,
                    DebugSidebarSection::Watch => DebugSidebarSection::CallStack,
                    DebugSidebarSection::CallStack => DebugSidebarSection::Breakpoints,
                    DebugSidebarSection::Breakpoints => DebugSidebarSection::Variables,
                };
                self.dap_sidebar_selected = 0;
            }
            // Debugger F-keys: routed through here when the debug sidebar
            // has focus so step-over / continue / breakpoint-toggle keep
            // working without forcing the user to refocus the editor first.
            // Mirrors the F-key arms in `handle_normal_key` (#281 smoke fix).
            "F5" => {
                let _ = self.execute_command("debug");
            }
            "F6" => {
                let _ = self.execute_command("pause");
            }
            "F9" => {
                let _ = self.execute_command("brkpt");
            }
            "F10" => {
                let _ = self.execute_command("stepover");
            }
            "F11" => {
                let _ = self.execute_command("stepin");
            }
            "Return" | "Enter" | " " => {
                let sel = self.dap_sidebar_selected;
                match self.dap_sidebar_section {
                    DebugSidebarSection::Variables => {
                        // Expand/collapse the variable at the selected flat index.
                        if let Some(var_ref) = self.dap_var_ref_at_flat_index(sel) {
                            if var_ref > 0 {
                                self.dap_toggle_expand_var(var_ref);
                            }
                        }
                    }
                    DebugSidebarSection::CallStack => {
                        // Select a call-stack frame and navigate to its source.
                        self.dap_select_frame(sel);
                        // Open the source file at the frame's line.
                        if let Some(frame) = self.dap_stack_frames.get(sel).cloned() {
                            if let Some(src) = &frame.source {
                                let src_path = PathBuf::from(src);
                                self.open_file_in_tab(&src_path);
                                let target_line = (frame.line as usize).saturating_sub(1);
                                self.view_mut().cursor.line = target_line;
                                self.view_mut().cursor.col = 0;
                                self.scroll_cursor_center();
                            }
                        }
                    }
                    DebugSidebarSection::Breakpoints => {
                        // Jump to the breakpoint's file and line.
                        if let Some((path, bp_idx)) = self.dap_bp_at_flat_index(sel) {
                            let line = self
                                .dap_breakpoints
                                .get(&path)
                                .and_then(|bps| bps.get(bp_idx))
                                .map(|bp| bp.line);
                            if let Some(line) = line {
                                let bp_path = PathBuf::from(&path);
                                self.open_file_in_tab(&bp_path);
                                let target_line = (line as usize).saturating_sub(1);
                                self.view_mut().cursor.line = target_line;
                                self.view_mut().cursor.col = 0;
                                self.scroll_cursor_center();
                            }
                        }
                    }
                    DebugSidebarSection::Watch => {}
                }
            }
            "x" | "d" => {
                let sel = self.dap_sidebar_selected;
                match self.dap_sidebar_section {
                    DebugSidebarSection::Watch
                        // Remove the selected watch expression.
                        if sel < self.dap_watch_expressions.len() => {
                            self.dap_remove_watch(sel);
                            let new_len = self.dap_watch_expressions.len();
                            if self.dap_sidebar_selected >= new_len && new_len > 0 {
                                self.dap_sidebar_selected = new_len - 1;
                            }
                        }
                    DebugSidebarSection::Breakpoints => {
                        // Remove the selected breakpoint.
                        if let Some((path, bp_idx)) = self.dap_bp_at_flat_index(sel) {
                            if let Some(bps) = self.dap_breakpoints.get_mut(&path) {
                                if bp_idx < bps.len() {
                                    let line = bps[bp_idx].line;
                                    bps.remove(bp_idx);
                                    self.message = format!("Breakpoint removed: line {line}");
                                    self.dap_send_breakpoints_for_file(&path);
                                    let new_len = self.dap_sidebar_section_len();
                                    if self.dap_sidebar_selected >= new_len && new_len > 0 {
                                        self.dap_sidebar_selected = new_len - 1;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                self.dap_sidebar_ensure_visible();
            }
            _ => {}
        }
        EngineAction::None
    }

    /// Step over (next).
    pub fn dap_step_over(&mut self) {
        let tid = self.dap_stopped_thread.unwrap_or(0);
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.next(tid);
                self.message = "DAP: step over".to_string();
                return;
            }
        }
        self.message = "DAP: no active session".to_string();
    }

    /// Step into.
    pub fn dap_step_into(&mut self) {
        let tid = self.dap_stopped_thread.unwrap_or(0);
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.step_in(tid);
                self.message = "DAP: step into".to_string();
                return;
            }
        }
        self.message = "DAP: no active session".to_string();
    }

    /// Step out.
    pub fn dap_step_out(&mut self) {
        let tid = self.dap_stopped_thread.unwrap_or(0);
        if let Some(mgr) = &mut self.dap_manager {
            if let Some(server) = &mut mgr.server {
                server.step_out(tid);
                self.message = "DAP: step out".to_string();
                return;
            }
        }
        self.message = "DAP: no active session".to_string();
    }

    /// Strip ANSI escape sequences (CSI, OSC, etc.) and non-printable control
    /// characters from a string.  Used to clean DAP adapter output before
    /// storing it in the Debug Output panel.
    pub(crate) fn strip_ansi_and_control(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // ESC sequences: CSI ('['), OSC (']'), SS2 ('N'), SS3 ('O'), etc.
                match chars.peek() {
                    Some(&'[') => {
                        chars.next(); // consume '['
                                      // CSI: params (0x30-3F) + intermediates (0x20-2F) + final (0x40-7E)
                        while let Some(&c) = chars.peek() {
                            chars.next();
                            if ('@'..='~').contains(&c) {
                                break;
                            }
                        }
                    }
                    Some(&']') => {
                        chars.next(); // consume ']'
                                      // OSC: terminated by BEL (\x07) or ST (ESC \)
                        while let Some(c) = chars.next() {
                            if c == '\x07' {
                                break;
                            }
                            if c == '\x1b' && chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    Some(&c) if c.is_ascii_uppercase() || c == '(' || c == ')' || c == '#' => {
                        chars.next(); // two-byte escape (ESC + letter/paren)
                    }
                    _ => {} // bare ESC — skip it
                }
            } else if ch == '\r' || (ch.is_control() && ch != '\n' && ch != '\t') {
                // skip control chars (keep newlines and tabs)
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// If `s` ends with an incomplete (unterminated) ANSI escape sequence,
    /// return the byte offset where that sequence begins so the caller can
    /// split off the tail and carry it into the next event.  Returns `None`
    /// when no incomplete tail is found.
    pub(crate) fn ansi_incomplete_tail_start(s: &str) -> Option<usize> {
        // Find the last ESC byte (0x1B is single-byte ASCII, always a valid UTF-8 boundary).
        let esc_pos = s.as_bytes().iter().rposition(|&b| b == 0x1b)?;
        let tail = &s[esc_pos..];
        let mut chars = tail.chars().peekable();
        chars.next(); // consume \x1b
        match chars.peek().copied() {
            Some('[') => {
                chars.next(); // consume '['
                              // CSI is complete when we see a final byte in 0x40–0x7E.
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&c) {
                        return None; // complete
                    }
                }
                Some(esc_pos) // ran out of chars before final byte
            }
            Some(']') => {
                chars.next(); // consume ']'
                let mut prev = '\0';
                for c in chars {
                    if c == '\x07' {
                        return None; // BEL terminates OSC
                    }
                    if prev == '\x1b' && c == '\\' {
                        return None; // ST terminates OSC
                    }
                    prev = c;
                }
                Some(esc_pos) // unterminated OSC
            }
            Some(c) if c.is_ascii_uppercase() || c == '(' || c == ')' || c == '#' => {
                None // two-byte escape is always complete once the second char is present
            }
            None => Some(esc_pos), // bare \x1b at end of string
            _ => None,             // other: treat as complete
        }
    }

    /// Drain all pending DAP events and update engine state accordingly.
    /// Called by both UI backends every poll tick (same cadence as `poll_lsp`).
    pub fn poll_dap(&mut self) -> bool {
        let events = match &mut self.dap_manager {
            Some(mgr) => match &mut mgr.server {
                Some(server) => server.poll(),
                None => return false,
            },
            None => return false,
        };
        if events.is_empty() {
            return false;
        }
        let mut redraw = false;
        for event in events {
            match event {
                DapEvent::Initialized => {
                    // Re-send all current breakpoints then signal configuration complete.
                    let bps: Vec<(String, Vec<BreakpointInfo>)> = self
                        .dap_breakpoints
                        .iter()
                        .filter(|(_, bps)| !bps.is_empty())
                        .map(|(f, b)| (f.clone(), b.clone()))
                        .collect();
                    let bp_count: usize = bps.iter().map(|(_, b)| b.len()).sum();
                    // Diagnostic log so the user can see this in Debug Output tab.
                    self.dap_output_lines.push(format!(
                        "[dap] initialized — sending {bp_count} breakpoint(s)"
                    ));
                    if let Some(mgr) = &mut self.dap_manager {
                        if let Some(server) = &mut mgr.server {
                            for (file, file_bps) in &bps {
                                let lines: Vec<u64> = file_bps.iter().map(|bp| bp.line).collect();
                                self.dap_output_lines
                                    .push(format!("[dap] setBreakpoints: {file} lines {lines:?}"));
                                server.set_breakpoints(file, file_bps);
                            }
                            server.configuration_done();
                        }
                    }
                    self.message = if bp_count == 0 {
                        "DAP: ready (no breakpoints set — use F9 to add one)".to_string()
                    } else {
                        format!("DAP: sent {bp_count} breakpoint(s), waiting for program\u{2026}")
                    };
                    redraw = true;
                }
                DapEvent::Stopped {
                    thread_id, reason, ..
                } => {
                    self.dap_output_lines
                        .push(format!("[dap] event: Stopped reason={reason}"));
                    self.dap_stopped_thread = Some(thread_id);
                    self.message = format!("DAP: stopped ({reason})");
                    // Clear previous frame/variable state before populating new ones.
                    self.dap_stack_frames.clear();
                    self.dap_variables.clear();
                    self.dap_primary_scope_name.clear();
                    self.dap_primary_scope_ref = 0;
                    // Request stack trace so we can highlight the current line.
                    if let Some(mgr) = &mut self.dap_manager {
                        if let Some(server) = &mut mgr.server {
                            server.stack_trace(thread_id);
                        }
                    }
                    redraw = true;
                }
                DapEvent::Continued { .. } => {
                    self.dap_stopped_thread = None;
                    self.dap_current_line = None;
                    self.dap_stack_frames.clear();
                    self.dap_variables.clear();
                    self.dap_primary_scope_name.clear();
                    self.dap_primary_scope_ref = 0;
                    self.dap_child_variables.clear();
                    self.dap_expanded_vars.clear();
                    self.dap_active_frame = 0;
                    self.dap_watch_values = vec![None; self.dap_watch_expressions.len()];
                    redraw = true;
                }
                DapEvent::Exited { exit_code } => {
                    self.dap_output_lines
                        .push(format!("[dap] event: Exited code={exit_code}"));
                    self.dap_session_active = false;
                    self.debug_toolbar_visible = false;
                    self.dap_stopped_thread = None;
                    self.dap_current_line = None;
                    self.dap_stack_frames.clear();
                    self.dap_variables.clear();
                    self.dap_primary_scope_name.clear();
                    self.dap_primary_scope_ref = 0;
                    self.dap_child_variables.clear();
                    self.dap_expanded_vars.clear();
                    self.dap_active_frame = 0;
                    self.dap_watch_values = vec![None; self.dap_watch_expressions.len()];
                    self.message = format!("DAP: process exited (code {exit_code})");
                    if let Some(mgr) = &mut self.dap_manager {
                        mgr.server = None;
                    }
                    redraw = true;
                }
                DapEvent::Output { category, output } => {
                    if category != "telemetry" {
                        let trimmed = output.trim_end_matches('\n');
                        if !trimmed.is_empty() {
                            // Prepend any carry from a previously split ANSI sequence,
                            // then split off any new incomplete tail for the next event.
                            let combined = if self.dap_ansi_carry.is_empty() {
                                trimmed.to_string()
                            } else {
                                let mut s = std::mem::take(&mut self.dap_ansi_carry);
                                s.push_str(trimmed);
                                s
                            };
                            let (to_strip, carry) = if let Some(tail_start) =
                                Self::ansi_incomplete_tail_start(&combined)
                            {
                                let carry = combined[tail_start..].to_string();
                                let clean_part = combined[..tail_start].to_string();
                                (clean_part, carry)
                            } else {
                                (combined, String::new())
                            };
                            self.dap_ansi_carry = carry;
                            // Strip ANSI escape sequences and control chars that
                            // would garble the Debug Output panel.
                            let clean: String = Self::strip_ansi_and_control(&to_strip);
                            if !clean.is_empty() {
                                self.message = format!("[{category}] {clean}");
                                for line in clean.lines() {
                                    self.dap_output_lines.push(format!("[{category}] {line}"));
                                }
                                if self.dap_output_lines.len() > 1000 {
                                    let excess = self.dap_output_lines.len() - 1000;
                                    self.dap_output_lines.drain(..excess);
                                }
                            }
                        }
                    }
                    redraw = true;
                }
                DapEvent::Breakpoint { reason, breakpoint } => {
                    self.message = if breakpoint.verified {
                        format!(
                            "DAP: breakpoint {reason} (verified, line {})",
                            breakpoint.line
                        )
                    } else {
                        let detail = breakpoint
                            .message
                            .as_deref()
                            .unwrap_or("path not found by adapter");
                        format!(
                            "DAP: breakpoint unverified — {detail} (line {})",
                            breakpoint.line
                        )
                    };
                    redraw = true;
                }
                DapEvent::RequestComplete {
                    seq: req_seq,
                    command: raw_command,
                    success,
                    body,
                    error_message,
                } => {
                    // codelldb omits the `command` field from responses.
                    // Resolve the actual command via the seq→command map kept
                    // by DapServer so all downstream checks work correctly.
                    let command = if raw_command.is_empty() {
                        if let Some(mgr) = &mut self.dap_manager {
                            if let Some(server) = &mut mgr.server {
                                server.resolve_command(req_seq)
                            } else {
                                raw_command
                            }
                        } else {
                            raw_command
                        }
                    } else {
                        raw_command
                    };

                    // Log every response.
                    if !success {
                        let msg_part = error_message
                            .as_deref()
                            .map(|m| format!(" msg={m:?}"))
                            .unwrap_or_default();
                        self.dap_output_lines
                            .push(format!("[dap] response: {command} success=false{msg_part}"));
                    } else {
                        self.dap_output_lines
                            .push(format!("[dap] response: {command} success=true"));
                    }
                    // After initialize succeeds, send the deferred launch request.
                    let is_init_response = command == "initialize";
                    if is_init_response && success {
                        self.dap_seq_initialize = None;
                        if let Some((req_type, launch_args)) = self.dap_pending_launch.take() {
                            self.dap_output_lines
                                .push(format!("[dap] initialize OK — sending {req_type}"));
                            if let Some(mgr) = &mut self.dap_manager {
                                if let Some(server) = &mut mgr.server {
                                    let seq = if req_type == "attach" {
                                        server.attach(launch_args)
                                    } else {
                                        server.launch(launch_args)
                                    };
                                    self.dap_seq_launch = Some(seq);
                                }
                            }
                        }
                    }

                    if command == "launch" && !success {
                        let detail = error_message
                            .as_deref()
                            .unwrap_or("check binary path and DISPLAY env var");
                        self.message = format!("DAP: launch failed — {detail}");
                        // Don't clear dap_session_active here: some adapters (codelldb)
                        // report success=false in the launch response even when the
                        // process IS running. The Exited event will properly end the
                        // session when the process terminates.
                    }
                    if command == "setBreakpoints" {
                        // Log the full response to dap_output_lines so it's visible
                        // in the Debug Output tab for diagnostics.
                        if let Some(bps) = body.get("breakpoints").and_then(|b| b.as_array()) {
                            let total = bps.len();
                            for bp in bps {
                                let verified = bp
                                    .get("verified")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let line = bp.get("line").and_then(|l| l.as_u64()).unwrap_or(0);
                                let msg = bp.get("message").and_then(|m| m.as_str()).unwrap_or("");
                                let log = if verified {
                                    format!("[dap] BP line {line}: verified ✓")
                                } else if msg.is_empty() {
                                    format!("[dap] BP line {line}: pending (resolves on load)")
                                } else {
                                    format!("[dap] BP line {line}: UNVERIFIED — {msg}")
                                };
                                self.dap_output_lines.push(log);
                            }
                            // Surface errors in the status bar only when every BP
                            // came back with a real error (not just "pending").
                            let unverified_errors: Vec<String> = bps
                                .iter()
                                .filter(|bp| {
                                    !bp.get("verified")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false)
                                })
                                .filter_map(|bp| {
                                    let msg =
                                        bp.get("message").and_then(|m| m.as_str()).unwrap_or("");
                                    if msg.is_empty() || msg == "pending" {
                                        None
                                    } else {
                                        let line =
                                            bp.get("line").and_then(|l| l.as_u64()).unwrap_or(0);
                                        Some(format!("line {line}: {msg}"))
                                    }
                                })
                                .collect();
                            if !unverified_errors.is_empty() && unverified_errors.len() == total {
                                self.message = format!(
                                    "DAP: breakpoint(s) not found — {} (check binary is built)",
                                    unverified_errors.join("; ")
                                );
                            }
                        }
                        redraw = true;
                    } else if command == "stackTrace" && success {
                        if let Some(frames_json) =
                            body.get("stackFrames").and_then(|f| f.as_array())
                        {
                            // Parse all frames for the call-stack panel.
                            self.dap_stack_frames = frames_json
                                .iter()
                                .map(|f| StackFrame {
                                    id: f.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                                    name: f
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("?")
                                        .to_string(),
                                    source: f
                                        .get("source")
                                        .and_then(|s| s.get("path"))
                                        .and_then(|p| p.as_str())
                                        .map(|s| s.to_string()),
                                    line: f.get("line").and_then(|v| v.as_u64()).unwrap_or(0),
                                })
                                .collect();
                            // Set dap_current_line from the top frame and navigate
                            // the editor to that file+line (same as LSP go-to-definition).
                            // Clone frame data to avoid borrow conflict with &mut self.
                            let top_source = self
                                .dap_stack_frames
                                .first()
                                .and_then(|f| f.source.as_ref().map(|s| (s.clone(), f.line)));
                            if let Some((src, line)) = top_source {
                                if line > 0 {
                                    self.dap_current_line = Some((src.clone(), line));
                                    // Open the file if not already active.
                                    let src_path = std::path::PathBuf::from(&src);
                                    let already_open = self
                                        .buffer_manager
                                        .get(self.active_buffer_id())
                                        .and_then(|s| s.file_path.as_ref())
                                        .map(|p| p == &src_path)
                                        .unwrap_or(false);
                                    if !already_open {
                                        let _ = self
                                            .open_file_with_mode(&src_path, OpenMode::Permanent);
                                    }
                                    // Jump to the stopped line (1-based → 0-based) and
                                    // center it in the viewport so it's always visible.
                                    // Clamp to the actual buffer length — the file open may
                                    // have failed (leaving the old small buffer) or the
                                    // adapter's line number may exceed the file's line count.
                                    // Guard: only navigate if we have a valid active window.
                                    let active_wid = self.active_window_id();
                                    if self.windows.contains_key(&active_wid) {
                                        let target_line = (line as usize).saturating_sub(1);
                                        let max_line =
                                            self.buffer().content.len_lines().saturating_sub(1);
                                        self.view_mut().cursor.line = target_line.min(max_line);
                                        self.view_mut().cursor.col = 0;
                                        self.scroll_cursor_center();
                                    }
                                }
                            }
                            // Chain: request scopes for the top frame so we can show variables.
                            let first_id = self.dap_stack_frames.first().map(|f| f.id).unwrap_or(0);
                            if first_id > 0 {
                                if let Some(mgr) = &mut self.dap_manager {
                                    if let Some(server) = &mut mgr.server {
                                        server.scopes(first_id);
                                    }
                                }
                            }
                            redraw = true;
                        }
                    } else if command == "scopes" && success {
                        // Parse all scopes from the response.
                        let scopes = body
                            .get("scopes")
                            .and_then(|s| s.as_array())
                            .cloned()
                            .unwrap_or_default();
                        self.dap_scope_groups.clear();
                        for (i, scope) in scopes.iter().enumerate() {
                            let var_ref = scope
                                .get("variablesReference")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            if var_ref == 0 {
                                continue;
                            }
                            // Skip expensive scopes (e.g. Registers).
                            let expensive = scope
                                .get("expensive")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if expensive {
                                continue;
                            }
                            let name = scope
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("Scope")
                                .to_string();
                            if i == 0 {
                                // First scope (usually "Locals"): store name/ref and
                                // auto-expand so variables are visible by default.
                                self.dap_primary_scope_name = name;
                                self.dap_primary_scope_ref = var_ref;
                                self.dap_expanded_vars.insert(var_ref);
                                self.dap_pending_vars_ref = 0;
                                if let Some(mgr) = &mut self.dap_manager {
                                    if let Some(server) = &mut mgr.server {
                                        server.variables(var_ref);
                                    }
                                }
                            } else {
                                // Additional scopes: store as expandable groups.
                                self.dap_scope_groups.push((name, var_ref));
                            }
                        }
                    } else if command == "variables" && success {
                        let parsed: Vec<DapVariable> = body
                            .get("variables")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .map(|v| {
                                        let name =
                                            v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                                        DapVariable {
                                            name: name.to_string(),
                                            value: v
                                                .get("value")
                                                .and_then(|val| val.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            var_ref: v
                                                .get("variablesReference")
                                                .and_then(|r| r.as_u64())
                                                .unwrap_or(0),
                                            // Heuristic: netcoredbg doesn't send
                                            // presentationHint.visibility, so detect
                                            // non-public members by naming convention:
                                            // `_field` (underscore prefix) and
                                            // `<Name>k__BackingField` (compiler-generated
                                            // auto-property backing fields).
                                            is_nonpublic: name.starts_with('_')
                                                || (name.starts_with('<')
                                                    && name.contains(">k__BackingField")),
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if self.dap_pending_vars_ref == 0 {
                            // Top-level scope variables.
                            self.dap_variables = parsed;
                            // Now evaluate all watch expressions for the active frame.
                            let frame_id = self
                                .dap_stack_frames
                                .get(self.dap_active_frame)
                                .map(|f| f.id)
                                .unwrap_or(0);
                            if frame_id > 0 && !self.dap_watch_expressions.is_empty() {
                                let exprs = self.dap_watch_expressions.clone();
                                for (idx, expr) in exprs.iter().enumerate() {
                                    if let Some(mgr) = &mut self.dap_manager {
                                        if let Some(server) = &mut mgr.server {
                                            let seq = server.evaluate(expr, frame_id);
                                            self.dap_pending_watch_seqs.insert(seq, idx);
                                        }
                                    }
                                }
                            }
                        } else {
                            // Child variables for an expanded entry.
                            // Partition into public and non-public (private/protected/internal).
                            let pending_ref = self.dap_pending_vars_ref;
                            let (public_vars, nonpublic_vars): (
                                Vec<DapVariable>,
                                Vec<DapVariable>,
                            ) = parsed.into_iter().partition(|v| !v.is_nonpublic);
                            if nonpublic_vars.is_empty() {
                                self.dap_child_variables.insert(pending_ref, public_vars);
                            } else {
                                let synthetic_ref = pending_ref | SYNTHETIC_NON_PUBLIC_MASK;
                                let mut children = public_vars;
                                children.push(DapVariable {
                                    name: "Non-Public Members".to_string(),
                                    value: String::new(),
                                    var_ref: synthetic_ref,
                                    is_nonpublic: false,
                                });
                                self.dap_child_variables.insert(pending_ref, children);
                                self.dap_child_variables
                                    .insert(synthetic_ref, nonpublic_vars);
                            }
                        }
                        self.dap_pending_vars_ref = 0;
                        redraw = true;
                    } else if command == "evaluate" && success {
                        if let Some(result) = body
                            .get("result")
                            .and_then(|r| r.as_str())
                            .map(|s| s.to_string())
                        {
                            // Check if this is a watch expression response.
                            if let Some(&watch_idx) = self.dap_pending_watch_seqs.get(&req_seq) {
                                if watch_idx < self.dap_watch_values.len() {
                                    self.dap_watch_values[watch_idx] = Some(result.clone());
                                }
                                self.dap_pending_watch_seqs.remove(&req_seq);
                            } else {
                                // User-triggered eval — show in status and store result.
                                self.message = format!("= {result}");
                                self.dap_eval_result = Some(result);
                            }
                            redraw = true;
                        }
                    }
                }
            }
        }
        redraw
    }

    /// Toggle between Vim and VSCode editing modes, saving the setting.
    pub fn toggle_editor_mode(&mut self) {
        self.settings.editor_mode = match self.settings.editor_mode {
            EditorMode::Vim => EditorMode::Vscode,
            EditorMode::Vscode => EditorMode::Vim,
        };
        // Clear any selection
        self.visual_anchor = None;
        // Set appropriate base mode
        if self.is_vscode_mode() {
            self.mode = Mode::Insert;
            self.menu_bar_visible = true;
        } else {
            self.mode = Mode::Normal;
        }
        let _ = self.settings.save();
    }

    /// Handle a scroll event on the debug output panel.
    /// `delta_y`: positive = scroll down (toward newer), negative = scroll up (toward older).
    /// Uses forward-indexed convention (TextDisplay's model).
    pub fn handle_debug_output_scroll(&mut self, delta_y: f32) {
        let step = (delta_y.abs() * 3.0).ceil() as usize;
        let total = self.dap_output_lines.len();
        if delta_y > 0.0 {
            self.debug_output_scroll += step;
            if self.debug_output_scroll >= total.saturating_sub(1) {
                self.debug_output_auto_scroll = true;
            }
        } else {
            self.debug_output_scroll = self.debug_output_scroll.saturating_sub(step);
            self.debug_output_auto_scroll = false;
        }
    }
}
