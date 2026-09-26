use super::*;

// ─── Unified Picker ───────────────────────────────────────────────────────────

impl Engine {
    /// Open the unified picker with a given source.
    pub fn open_picker(&mut self, source: PickerSource) {
        // Opening a picker is a "user is now focused on this modal"
        // event — dismiss any passive overlays (LSP hover) so they
        // don't render behind the picker (#247).
        self.dismiss_editor_hover();

        self.picker_query.clear();
        self.picker_selected = 0;
        self.picker_scroll_top = 0;
        self.picker_all_items.clear();
        self.picker_items.clear();
        self.picker_preview = None;
        self.breadcrumb_scoped_parent = None;
        self.breadcrumb_scoped_parent_line = None;
        self.picker_history_index = None;
        self.picker_history_typing_buffer.clear();
        self.picker_grep_scope = None;

        match source {
            PickerSource::Files => {
                self.picker_title = "Find Files".to_string();
                self.picker_populate_files();
            }
            PickerSource::Commands => {
                self.picker_title = "Command Palette".to_string();
                self.picker_populate_commands();
            }
            PickerSource::Grep => {
                self.picker_title = "Live Grep".to_string();
                // Grep is a live source — no pre-populate, search runs per keystroke.
            }
            PickerSource::CommandCenter => {
                self.picker_title = "Search".to_string();
                // Default mode: files. Prefix routing handled in picker_filter_command_center.
                self.picker_populate_files();
            }
            PickerSource::Buffers => {
                self.picker_title = "Open Buffers".to_string();
                self.picker_populate_buffers();
            }
            PickerSource::Keybindings => {
                self.picker_title = "Key Bindings".to_string();
                self.picker_populate_keybindings();
            }
            PickerSource::GitBranches => {
                self.picker_title = "Switch Branch".to_string();
                self.picker_populate_branches();
            }
            PickerSource::Languages => {
                self.picker_title = "Select Language Mode".to_string();
                self.picker_populate_languages();
            }
            PickerSource::Indentation => {
                self.picker_title = "Select Indentation".to_string();
                self.picker_populate_indentation();
            }
            PickerSource::LineEndings => {
                self.picker_title = "Select Line Ending Sequence".to_string();
                self.picker_populate_line_endings();
            }
            PickerSource::RecentWorkspaces => {
                self.picker_title = "Open Recent Workspace".to_string();
                self.picker_populate_recent_workspaces();
            }
            _ => {
                self.picker_title = format!("{:?}", source);
            }
        }

        self.picker_source = source;
        self.picker_filter();
        self.picker_load_preview();
        self.picker_open = true;
    }

    /// Open the Grep picker scoped to `dir` — the explorer context menu's
    /// "Find in Folder..." action (#1418/#1438). Search results are
    /// restricted to `dir` (via [`Self::picker_grep_scope`]) instead of the
    /// whole workspace, matching the menu label. `open_picker` already
    /// reset `picker_grep_scope` to `None`, so this only needs to set it
    /// afterwards. The picker title is switched from the plain "Live Grep"
    /// to `"Grep in <dir relative to cwd>/"` so the scope is visible in the
    /// UI, not just in search behavior (#1438).
    pub fn open_grep_picker_scoped(&mut self, dir: &Path) {
        self.open_picker(PickerSource::Grep);
        self.picker_grep_scope = Some(dir.to_path_buf());
        let rel = dir.strip_prefix(&self.cwd).unwrap_or(dir);
        let rel_str = rel.to_string_lossy();
        self.picker_title = if rel_str.is_empty() {
            "Live Grep".to_string()
        } else {
            format!("Grep in {}/", rel_str.trim_end_matches(['/', '\\']))
        };
    }

    /// Open the Command Center picker (called from menu bar search box click).
    pub fn open_command_center(&mut self) {
        self.open_picker(PickerSource::CommandCenter);
    }

    /// Handle a click on a breadcrumb segment.
    /// `is_symbol`: true if this is a symbol segment (opens `@` symbol picker).
    /// `path_prefix`: for path segments, the accumulated directory path up to this segment.
    pub fn breadcrumb_click(&mut self, is_symbol: bool, path_prefix: Option<&std::path::Path>) {
        if is_symbol {
            // Open document symbol picker
            self.open_picker(PickerSource::CommandCenter);
            self.picker_query = "@".to_string();
            self.picker_filter();
            self.picker_load_preview();
        } else if let Some(path) = path_prefix {
            // Path segment: show peers (sibling entries in the parent directory).
            let parent = path.parent().unwrap_or(path);
            self.open_picker(PickerSource::Files);
            let rel = parent
                .strip_prefix(&self.cwd)
                .unwrap_or(parent)
                .to_string_lossy()
                .to_string();
            self.picker_query = if rel.is_empty() {
                String::new()
            } else {
                format!("{}/", rel)
            };
            self.picker_filter();
            self.picker_load_preview();
        }
    }

    /// Handle a double-click on a breadcrumb segment.
    /// Symbols: jump directly to the symbol's definition line.
    /// Path segments: same as single click (open picker).
    pub fn breadcrumb_double_click(
        &mut self,
        is_symbol: bool,
        path_prefix: Option<&std::path::Path>,
        symbol_line: Option<usize>,
    ) {
        if is_symbol {
            if let Some(line) = symbol_line {
                self.push_jump_location();
                let win_id = self.active_window_id();
                self.set_cursor_for_window(win_id, line, 0);
                self.ensure_cursor_visible();
            } else {
                // No position info — fall back to symbol picker
                self.breadcrumb_click(is_symbol, path_prefix);
            }
        } else {
            // Path segments: same as single click
            self.breadcrumb_click(is_symbol, path_prefix);
        }
    }

    /// Close the unified picker and clear all state.
    pub fn close_picker(&mut self) {
        self.picker_open = false;
        self.picker_query.clear();
        self.picker_all_items.clear();
        self.picker_items.clear();
        self.picker_selected = 0;
        self.picker_scroll_top = 0;
        self.picker_preview = None;
        self.breadcrumb_scoped_parent = None;
        self.breadcrumb_scoped_parent_line = None;
    }

    /// Rebuild the cached breadcrumb segments from the active group's state.
    /// Called when entering breadcrumb focus mode.
    pub(crate) fn rebuild_breadcrumb_segments(&mut self) {
        self.breadcrumb_segments.clear();
        let buf_state = match self.buffer_manager.get(self.active_buffer_id()) {
            Some(s) => s,
            None => return,
        };

        // Path segments
        if let Some(ref file_path) = buf_state.file_path {
            let display = if let Ok(rel) = file_path.strip_prefix(&self.cwd) {
                rel.to_string_lossy().to_string()
            } else {
                file_path.to_string_lossy().to_string()
            };
            let mut accumulated = self.cwd.clone();
            for part in display.split(std::path::MAIN_SEPARATOR) {
                accumulated = accumulated.join(part);
                self.breadcrumb_segments.push(BreadcrumbSegmentInfo {
                    label: part.to_string(),
                    is_symbol: false,
                    path_prefix: Some(accumulated.clone()),
                    symbol_line: None,
                    parent_scope: None,
                });
            }
        }

        // Symbol segments from tree-sitter
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let text = buf_state.buffer.to_string();
        let scopes = if let Some(ref syn) = buf_state.syntax {
            syn.enclosing_scopes(&text, cursor_line, cursor_col)
        } else {
            Vec::new()
        };
        let mut prev_scope_name: Option<String> = None;
        for scope in &scopes {
            self.breadcrumb_segments.push(BreadcrumbSegmentInfo {
                label: scope.name.clone(),
                is_symbol: true,
                path_prefix: None,
                symbol_line: Some(scope.line),
                parent_scope: prev_scope_name.clone(),
            });
            prev_scope_name = Some(scope.name.clone());
        }

        // Clamp selection
        if !self.breadcrumb_segments.is_empty() {
            self.breadcrumb_selected = self
                .breadcrumb_selected
                .min(self.breadcrumb_segments.len() - 1);
        }
    }

    /// Focus `group_id` if it exists, so the breadcrumb helpers below (which
    /// all read the *active* group's buffer and cursor) resolve against the
    /// bar the user actually clicked (#555).
    fn focus_breadcrumb_group(&mut self, group_id: GroupId) {
        if self.active_group != group_id && self.editor_groups.contains_key(&group_id) {
            self.active_group = group_id;
        }
    }

    /// Open a scoped picker for the currently selected breadcrumb segment.
    /// Path segments open the file picker for that directory.
    /// Handle a breadcrumb segment click from either backend.
    /// Focuses the clicked group, rebuilds its segments, selects the clicked
    /// index, and opens scoped.
    ///
    /// `group_id` comes from `render::BreadcrumbClickResult::Hit` — it is the
    /// group whose *bar* was clicked, which is not necessarily the focused one
    /// in a split. Resolving the index against the focused group instead was
    /// the #555 "clicks do nothing" bug: an index valid for the clicked bar
    /// could be out of range for the focused group's shorter segment list, and
    /// `breadcrumb_open_scoped` would silently bail.
    pub fn handle_breadcrumb_click(&mut self, group_id: GroupId, idx: usize) {
        self.focus_breadcrumb_group(group_id);
        self.rebuild_breadcrumb_segments();
        self.breadcrumb_selected = idx;
        self.breadcrumb_open_scoped();
    }

    /// Handle a breadcrumb segment *double*-click from either backend.
    ///
    /// Same group-resolution contract as [`Self::handle_breadcrumb_click`].
    /// Symbol segments jump straight to the definition; path segments fall
    /// back to the single-click behaviour (open the scoped picker).
    pub fn handle_breadcrumb_double_click(&mut self, group_id: GroupId, idx: usize) {
        self.focus_breadcrumb_group(group_id);
        self.rebuild_breadcrumb_segments();
        let seg = match self.breadcrumb_segments.get(idx) {
            Some(s) => s.clone(),
            None => return,
        };
        self.breadcrumb_selected = idx;
        self.breadcrumb_double_click(seg.is_symbol, seg.path_prefix.as_deref(), seg.symbol_line);
    }

    /// Symbol segments open the `@` symbol picker filtered to siblings
    /// within the parent scope.
    pub(crate) fn breadcrumb_open_scoped(&mut self) {
        let seg = match self.breadcrumb_segments.get(self.breadcrumb_selected) {
            Some(s) => s.clone(),
            None => return,
        };

        if !seg.is_symbol {
            self.breadcrumb_click(false, seg.path_prefix.as_deref());
            return;
        }

        // Symbol segment: show siblings at the same level.
        // Filter to symbols whose container matches this segment's parent,
        // matching VSCode behavior (clicking a function shows sibling functions).
        //
        // #465: `open_picker` resets the scope filter as part of its state-clear
        // pass, so both assignments happen AFTER the open call — otherwise the
        // filter is wiped before the async LSP response can read it.
        //
        // The parent's *line* is recorded alongside its name because tree-sitter
        // and LSP disagree on naming for impl blocks: tree-sitter's name is the
        // type being implemented (e.g. `VsplitLayout`), while rust-analyzer's
        // hierarchical `DocumentSymbol` names the impl block fully (e.g. `impl
        // WindowGroupLayout for VsplitLayout`) and sets that on children's
        // `container`. The line uniquely identifies the parent in either form
        // without depending on name-string compatibility.
        let parent_line = self
            .breadcrumb_segments
            .iter()
            .take(self.breadcrumb_selected)
            .rev()
            .find(|s| s.is_symbol)
            .and_then(|s| s.symbol_line);
        self.open_picker(PickerSource::CommandCenter);
        self.breadcrumb_scoped_parent = Some(seg.parent_scope.clone());
        self.breadcrumb_scoped_parent_line = parent_line;
        self.picker_query = "@".to_string();
        self.picker_filter();
        self.picker_load_preview();
    }

    /// Populate picker_all_items with files from the project using the ignore crate.
    fn picker_populate_files(&mut self) {
        let cwd = self.cwd.clone();
        let show_hidden = self.settings.show_hidden_files;
        let walker = ignore::WalkBuilder::new(&cwd)
            .hidden(!show_hidden)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        let mut items: Vec<PickerItem> = Vec::new();
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let rel = match path.strip_prefix(&cwd) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            };
            let display = rel.to_string_lossy().into_owned();
            items.push(PickerItem {
                filter_text: display.clone(),
                display,
                detail: None,
                action: PickerAction::OpenFile(rel),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            });
        }
        items.sort_by(|a, b| a.display.cmp(&b.display));
        self.picker_all_items = items;
    }

    /// Populate picker_all_items with command palette entries.
    fn picker_populate_commands(&mut self) {
        let use_vscode = self.is_vscode_mode();
        self.picker_all_items = PALETTE_COMMANDS
            .iter()
            .map(|cmd| {
                let sc = if use_vscode && !cmd.vscode_shortcut.is_empty() {
                    cmd.vscode_shortcut.to_string()
                } else {
                    cmd.shortcut.to_string()
                };
                PickerItem {
                    display: cmd.label.to_string(),
                    filter_text: format!("{} {}", cmd.label, cmd.action),
                    detail: if sc.is_empty() { None } else { Some(sc) },
                    action: PickerAction::ExecuteCommand(cmd.action.to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    /// Populate picker_all_items with git branches.
    fn picker_populate_buffers(&mut self) {
        let active_id = self.active_buffer_id();
        let ids = self.buffer_manager.list();
        self.picker_all_items = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let state = self.buffer_manager.get(id).unwrap();
                let buf_num = i + 1;
                let name = state.display_name();
                let mut flags = String::new();
                if id == active_id {
                    flags.push_str("%a ");
                }
                if state.dirty {
                    flags.push('+');
                }
                let detail = if flags.is_empty() {
                    None
                } else {
                    Some(flags.trim().to_string())
                };
                let action = if let Some(ref p) = state.file_path {
                    PickerAction::OpenFile(p.clone())
                } else {
                    PickerAction::ExecuteCommand(format!("buffer {}", buf_num))
                };
                let icon = state
                    .file_path
                    .as_ref()
                    .and_then(|p| p.extension())
                    .and_then(|e| e.to_str())
                    .map(|ext| crate::icons::file_icon(ext).to_string());
                PickerItem {
                    display: name,
                    filter_text: state
                        .file_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    detail,
                    action,
                    icon,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    fn picker_populate_keybindings(&mut self) {
        let is_vscode = self.is_vscode_mode();
        let content = if is_vscode {
            super::keybindings_reference_vscode()
        } else {
            super::keybindings_reference_vim()
        };
        let mut section = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            // Section headers: "── Foo ──"
            if trimmed.starts_with("──") {
                // Extract section name between ── markers
                section = trimmed
                    .trim_start_matches('─')
                    .trim_end_matches('─')
                    .trim()
                    .to_string();
                continue;
            }
            // Skip empty, title, or decoration lines
            if trimmed.is_empty()
                || trimmed.starts_with('=')
                || trimmed.starts_with("VimCode")
                || trimmed.starts_with("Use ")
                || trimmed.starts_with("Remap ")
                || trimmed.starts_with("Commands shown")
            {
                continue;
            }
            // Parse "key(s)   description   [:command]"
            // Split at first run of 2+ spaces
            if let Some(idx) = trimmed.find("  ") {
                let keys = trimmed[..idx].trim();
                let desc = trimmed[idx..].trim();
                if !keys.is_empty() && !desc.is_empty() {
                    let detail = if section.is_empty() {
                        None
                    } else {
                        Some(section.clone())
                    };
                    // Check for user remaps
                    let display = format!("{:<24}{}", keys, desc);
                    self.picker_all_items.push(PickerItem {
                        display,
                        filter_text: format!("{} {} {}", keys, desc, section),
                        detail,
                        action: PickerAction::ExecuteCommand("nop".to_string()),
                        icon: None,
                        score: 0,
                        match_positions: Vec::new(),
                        depth: 0,
                        expandable: false,
                        expanded: false,
                    });
                }
            }
        }

        // Append configurable panel keys with their actual values
        let pk = &self.settings.panel_keys;
        let panel_bindings: &[(&str, &str, &str)] = &[
            (&pk.toggle_sidebar, "Toggle sidebar", "Panel"),
            (&pk.focus_explorer, "Focus explorer", "Panel"),
            (&pk.focus_search, "Focus search panel", "Panel"),
            (&pk.fuzzy_finder, "Fuzzy file finder", "Panel"),
            (&pk.live_grep, "Live grep", "Panel"),
            (&pk.command_palette, "Command palette", "Panel"),
            (&pk.open_terminal, "Toggle terminal", "Panel"),
            (&pk.add_cursor, "Add cursor at next match", "Panel"),
            (&pk.select_all_matches, "Select all occurrences", "Panel"),
            (&pk.nav_back, "Navigate back in history", "Panel"),
            (&pk.nav_forward, "Navigate forward in history", "Panel"),
        ];
        for &(key, desc, cat) in panel_bindings {
            if key.is_empty() {
                continue;
            }
            let display = format!("{:<24}{} (configurable)", key, desc);
            self.picker_all_items.push(PickerItem {
                display,
                filter_text: format!("{} {} {} configurable", key, desc, cat),
                detail: Some(cat.to_string()),
                action: PickerAction::ExecuteCommand("nop".to_string()),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            });
        }

        // Append user keymaps (`:map` remaps) with a marker
        for km in &self.user_keymaps {
            let keys_str = km.keys.join("");
            let display = format!(
                "{:<24}{} [mode: {}] (user remap)",
                keys_str, km.action, km.mode
            );
            self.picker_all_items.push(PickerItem {
                display,
                filter_text: format!("{} {} {} user remap", keys_str, km.action, km.mode),
                detail: Some("User Keymaps".to_string()),
                action: PickerAction::ExecuteCommand("nop".to_string()),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            });
        }
    }

    fn picker_populate_branches(&mut self) {
        let branches = crate::core::git::list_branches(&self.cwd);
        self.picker_all_items = branches
            .into_iter()
            .map(|b| {
                let mut detail_parts = Vec::new();
                if b.is_current {
                    detail_parts.push("● current".to_string());
                }
                if let Some(ref ab) = b.ahead_behind {
                    detail_parts.push(ab.clone());
                }
                if let Some(ref up) = b.upstream {
                    detail_parts.push(format!("→ {}", up));
                }
                let detail = if detail_parts.is_empty() {
                    None
                } else {
                    Some(detail_parts.join("  "))
                };
                PickerItem {
                    display: b.name.clone(),
                    filter_text: b.name.clone(),
                    detail,
                    action: PickerAction::CheckoutBranch(b.name),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    fn picker_populate_languages(&mut self) {
        let current = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.lsp_language_id.as_deref())
            .unwrap_or("");
        self.picker_all_items = crate::core::lsp::all_known_language_ids()
            .into_iter()
            .map(|lang| {
                let detail = if lang == current {
                    Some("● current".to_string())
                } else {
                    None
                };
                PickerItem {
                    display: lang.to_string(),
                    filter_text: lang.to_string(),
                    detail,
                    action: PickerAction::SetLanguage(lang.to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    fn picker_populate_indentation(&mut self) {
        let et = self.settings.expand_tab;
        let ts = self.settings.tabstop;
        let items = [
            ("Spaces: 2", true, 2u8),
            ("Spaces: 4", true, 4),
            ("Spaces: 8", true, 8),
            ("Tabs (width 2)", false, 2),
            ("Tabs (width 4)", false, 4),
            ("Tabs (width 8)", false, 8),
        ];
        self.picker_all_items = items
            .iter()
            .map(|(label, expand, width)| {
                let is_current = *expand == et && *width == ts;
                PickerItem {
                    display: label.to_string(),
                    filter_text: label.to_string(),
                    detail: if is_current {
                        Some("● current".to_string())
                    } else {
                        None
                    },
                    action: PickerAction::SetIndentation(*expand, *width),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    fn picker_populate_line_endings(&mut self) {
        use crate::core::buffer_manager::LineEnding;
        let current = self
            .buffer_manager
            .get(self.active_buffer_id())
            .map(|s| s.line_ending)
            .unwrap_or(LineEnding::LF);
        let items = [
            ("LF", false),  // is_crlf = false
            ("CRLF", true), // is_crlf = true
        ];
        self.picker_all_items = items
            .iter()
            .map(|(label, is_crlf)| {
                let le = if *is_crlf {
                    LineEnding::Crlf
                } else {
                    LineEnding::LF
                };
                PickerItem {
                    display: label.to_string(),
                    filter_text: label.to_string(),
                    detail: if le == current {
                        Some("● current".to_string())
                    } else {
                        None
                    },
                    action: PickerAction::SetLineEnding(*is_crlf),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    /// Populate the picker with recent workspace paths from the session.
    /// Most-recent first (the session stores them oldest-first). #274.
    fn picker_populate_recent_workspaces(&mut self) {
        let current = self.workspace_root.clone();
        self.picker_all_items = self
            .session
            .recent_workspaces
            .iter()
            .rev()
            .map(|path| {
                let display = path.display().to_string();
                let detail = if current.as_ref() == Some(path) {
                    Some("● current".to_string())
                } else {
                    None
                };
                PickerItem {
                    display: display.clone(),
                    filter_text: display,
                    detail,
                    action: PickerAction::OpenWorkspace(path.clone()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
    }

    /// Filter picker_all_items by the current query and populate picker_items.
    /// For live sources (Grep), runs a search instead of fuzzy-filtering.
    /// For CommandCenter, delegates to prefix-aware routing.
    pub(crate) fn picker_filter(&mut self) {
        const CAP: usize = 100;

        // Live grep: run project search directly instead of fuzzy-filtering.
        if self.picker_source == PickerSource::Grep {
            self.picker_grep_search();
            return;
        }

        // Command Center: dynamic prefix routing.
        if self.picker_source == PickerSource::CommandCenter {
            self.picker_filter_command_center();
            return;
        }

        Self::fuzzy_filter_items(
            &self.picker_all_items,
            &self.picker_query,
            CAP,
            &mut self.picker_items,
        );
    }

    /// Shared fuzzy filter: score `all_items` against `query`, populate `out`.
    fn fuzzy_filter_items(
        all_items: &[PickerItem],
        query: &str,
        cap: usize,
        out: &mut Vec<PickerItem>,
    ) {
        if query.is_empty() {
            *out = all_items.iter().take(cap).cloned().collect();
        } else {
            let query_lc = query.to_lowercase();
            let mut scored: Vec<PickerItem> = all_items
                .iter()
                .filter_map(|item| {
                    quadraui::text_util::fuzzy_score(&item.filter_text.to_lowercase(), &query_lc)
                        .map(|(s, positions)| {
                            let mut item = item.clone();
                            item.score = s;
                            item.match_positions = positions;
                            item
                        })
                })
                .collect();
            scored.sort_by_key(|b| std::cmp::Reverse(b.score));
            scored.truncate(cap);
            *out = scored;
        }
    }

    /// Detect the prefix in `picker_query` and route to the appropriate mode.
    fn picker_filter_command_center(&mut self) {
        const CAP: usize = 100;
        let query = self.picker_query.clone();

        if let Some(rest) = query.strip_prefix('>') {
            // Command palette mode
            self.picker_title = "Commands".to_string();
            // Re-populate commands if all_items aren't command items
            if self.picker_all_items.is_empty()
                || !matches!(
                    self.picker_all_items.first().map(|i| &i.action),
                    Some(PickerAction::ExecuteCommand(_))
                )
            {
                self.picker_populate_commands();
            }
            let sub_query = rest.trim_start().to_string();
            Self::fuzzy_filter_items(
                &self.picker_all_items,
                &sub_query,
                CAP,
                &mut self.picker_items,
            );
        } else if let Some(rest) = query.strip_prefix('@') {
            // Document symbols mode (LSP)
            self.picker_title = "Go to Symbol in File".to_string();
            let sub_query = rest.trim_start().to_string();
            // Clear file/command items and request symbols if we haven't already
            let has_symbol_items = matches!(
                self.picker_all_items.first().map(|i| &i.action),
                Some(PickerAction::GotoSymbol(..))
            );
            if !has_symbol_items {
                self.picker_all_items.clear();
            }
            if self.lsp_pending_document_symbols.is_none() && self.picker_all_items.is_empty() {
                self.picker_request_document_symbols();
            }
            // Tree view when no query; flat fuzzy filter when typing
            if sub_query.is_empty() {
                self.picker_rebuild_visible_tree();
            } else {
                Self::fuzzy_filter_items(
                    &self.picker_all_items,
                    &sub_query,
                    CAP,
                    &mut self.picker_items,
                );
                // Reset depth on filtered items so they display flat
                for item in &mut self.picker_items {
                    item.depth = 0;
                    item.expandable = false;
                }
            }
            if self.picker_items.is_empty() && self.lsp_pending_document_symbols.is_some() {
                self.picker_items = vec![PickerItem {
                    display: "Loading symbols...".to_string(),
                    filter_text: String::new(),
                    detail: None,
                    action: PickerAction::Custom("loading".to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }];
            }
        } else if let Some(rest) = query.strip_prefix('#') {
            // Workspace symbols mode (LSP)
            self.picker_title = "Go to Symbol in Workspace".to_string();
            let sub_query = rest.trim_start().to_string();
            if sub_query.len() >= 2 {
                self.picker_request_workspace_symbols(&sub_query);
            } else if sub_query.is_empty() {
                self.picker_items = vec![PickerItem {
                    display: "Type at least 2 characters to search workspace symbols..."
                        .to_string(),
                    filter_text: String::new(),
                    detail: None,
                    action: PickerAction::Custom("hint".to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }];
            }
        } else if let Some(rest) = query.strip_prefix(':') {
            // Go to line mode
            self.picker_title = "Go to Line".to_string();
            let trimmed = rest.trim();
            if let Ok(line_num) = trimmed.parse::<usize>() {
                let line_count = self.buffer().content.len_lines();
                let clamped = line_num.clamp(1, line_count);
                self.picker_items = vec![PickerItem {
                    display: format!("Go to line {}", clamped),
                    filter_text: String::new(),
                    detail: Some(format!("of {}", line_count)),
                    action: PickerAction::GotoLine(clamped.saturating_sub(1)),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }];
            } else {
                self.picker_items = vec![PickerItem {
                    display: "Type a line number...".to_string(),
                    filter_text: String::new(),
                    detail: Some(format!("1–{}", self.buffer().content.len_lines())),
                    action: PickerAction::Custom("hint".to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }];
            }
        } else if let Some(rest) = query.strip_prefix('%') {
            // Live grep mode (search for text in project)
            self.picker_title = "Search for Text".to_string();
            let sub_query = rest.trim_start().to_string();
            if sub_query.len() < 2 {
                self.picker_items = vec![PickerItem {
                    display: "Type at least 2 characters to search project...".to_string(),
                    filter_text: String::new(),
                    detail: None,
                    action: PickerAction::Custom("hint".to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }];
            } else {
                // Reuse the grep search logic with sub_query
                self.picker_cc_grep_search(&sub_query);
            }
        } else if query == "debug" || query.starts_with("debug ") {
            // Start Debugging mode — show launch configurations
            self.picker_title = "Start Debugging".to_string();
            let sub_query = query
                .strip_prefix("debug")
                .unwrap_or("")
                .trim_start()
                .to_string();
            self.picker_populate_debug_configs(&sub_query);
        } else if query == "task" || query.starts_with("task ") {
            // Run Task mode — show tasks from tasks.json
            self.picker_title = "Run Task".to_string();
            let sub_query = query
                .strip_prefix("task")
                .unwrap_or("")
                .trim_start()
                .to_string();
            self.picker_populate_tasks(&sub_query);
        } else if query == "chat" || query.starts_with("chat ") {
            // AI Chat mode — open AI panel or send a message
            self.picker_title = "AI Chat".to_string();
            let sub_query = query
                .strip_prefix("chat")
                .unwrap_or("")
                .trim_start()
                .to_string();
            self.picker_populate_chat(&sub_query);
        } else if query == "?" {
            // Help mode: show available prefixes
            self.picker_title = "Help: Prefix Modes".to_string();
            self.picker_items = vec![
                Self::help_item("", "Search files by name (default)"),
                Self::help_item(">", "Show and run commands"),
                Self::help_item("@", "Go to symbol in current file (LSP)"),
                Self::help_item("#", "Go to symbol in workspace (LSP)"),
                Self::help_item(":", "Go to line number"),
                Self::help_item("%", "Search for text in project"),
                Self::help_item("debug", "Start debugging (launch configurations)"),
                Self::help_item("task", "Run a task (from tasks.json)"),
                Self::help_item("chat", "Ask the AI assistant"),
                Self::help_item("?", "Show this help"),
            ];
        } else if query.is_empty() {
            // Placeholder hints: show available modes when query is empty
            self.picker_title = "Search".to_string();
            self.picker_items = vec![
                Self::hint_item("Go to File", "", "Type a file name"),
                Self::hint_item("Show and Run Commands", ">", "Ctrl+Shift+P"),
                Self::hint_item("Go to Symbol in Editor", "@", ""),
                Self::hint_item("Go to Symbol in Workspace", "#", ""),
                Self::hint_item("Go to Line", ":", "Ctrl+G"),
                Self::hint_item("Search for Text", "%", "Ctrl+G (grep)"),
                Self::hint_item("Start Debugging", "debug", "F5"),
                Self::hint_item("Run Task", "task", ""),
                Self::hint_item("Ask AI", "chat", ":AI"),
                Self::hint_item("More Help", "?", ""),
            ];
        } else {
            // Default: file search
            self.picker_title = "Search".to_string();
            // Re-populate files if all_items aren't file items
            if self.picker_all_items.is_empty()
                || matches!(
                    self.picker_all_items.first().map(|i| &i.action),
                    Some(PickerAction::ExecuteCommand(_))
                )
            {
                self.picker_populate_files();
            }
            Self::fuzzy_filter_items(&self.picker_all_items, &query, CAP, &mut self.picker_items);
        }
    }

    /// Create a placeholder hint item for the empty-query Command Center dropdown.
    /// `label` is the mode name, `prefix` is the prefix to set, `shortcut` is the keyboard shortcut hint.
    fn hint_item(label: &str, prefix: &str, shortcut: &str) -> PickerItem {
        let action_prefix = if prefix.is_empty() {
            // "Go to File" — just clear the query (stay in file mode)
            String::new()
        } else {
            prefix.to_string()
        };
        PickerItem {
            display: label.to_string(),
            filter_text: String::new(),
            detail: if shortcut.is_empty() {
                Some(prefix.to_string())
            } else {
                Some(format!("{}  {}", prefix, shortcut))
            },
            action: PickerAction::Custom(format!("prefix:{}", action_prefix)),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
            depth: 0,
            expandable: false,
            expanded: false,
        }
    }

    /// Create a help item for the "?" prefix mode.
    fn help_item(prefix: &str, description: &str) -> PickerItem {
        PickerItem {
            display: if prefix.is_empty() {
                "(no prefix)".to_string()
            } else {
                prefix.to_string()
            },
            filter_text: String::new(),
            detail: Some(description.to_string()),
            action: PickerAction::Custom(format!("prefix:{}", prefix)),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
            depth: 0,
            expandable: false,
            expanded: false,
        }
    }

    /// Request document symbols from LSP for the current buffer.
    fn picker_request_document_symbols(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let path = match self.active_buffer_path() {
            Some(p) => p,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_document_symbols(&path) {
                self.lsp_pending_document_symbols = Some(id);
            }
        }
    }

    /// Populate picker items from a document symbol LSP response.
    /// Builds a tree structure: `picker_all_items` holds the full depth-first tree,
    /// `picker_items` holds only visible items (respecting expand/collapse state).
    /// When a filter query is active, the tree is flattened for fuzzy matching.
    pub(crate) fn picker_populate_document_symbols(&mut self, symbols: Vec<lsp::SymbolInfo>) {
        // Apply scoped parent filter if set (from breadcrumb navigation).
        //
        // Two filter keys can be set by `breadcrumb_open_scoped`:
        //  * `breadcrumb_scoped_parent_line` — start line of the parent scope
        //    in the buffer. Primary key. Set when the click came via the
        //    breadcrumb bar (where each segment has a line).
        //  * `breadcrumb_scoped_parent` — parent name. Fallback for callers
        //    that don't have a line (and the existing test surface).
        //
        // Line-based lookup wins when available because tree-sitter and LSP
        // disagree on naming for impl blocks (#465): tree-sitter returns just
        // the implemented type (`VsplitLayout`), while rust-analyzer names the
        // impl `impl WindowGroupLayout for VsplitLayout`. Lines are unique and
        // language-server-agnostic.
        let scoped = self.breadcrumb_scoped_parent.take();
        let scoped_line = self.breadcrumb_scoped_parent_line.take();
        let is_scoped = scoped.is_some();
        let mut filtered: Vec<lsp::SymbolInfo> = if let Some(parent_line) = scoped_line {
            // Walk the hierarchical tree to find the parent symbol at the given
            // line, then use its children as the sibling list. Falls through to
            // name-based filtering if no such symbol is found (defensive).
            match Self::find_symbol_at_line(&symbols, parent_line) {
                Some(parent) if !parent.children.is_empty() => parent.children.clone(),
                Some(parent) => {
                    // Flat SymbolInformation case: parent has no children, so
                    // siblings have to be identified via the `container` field
                    // using the parent's own LSP name (not tree-sitter's).
                    let parent_name = parent.name.clone();
                    symbols
                        .into_iter()
                        .filter(|s| s.container.as_deref() == Some(parent_name.as_str()))
                        .collect()
                }
                None => symbols, // parent not in LSP response — best-effort: show all
            }
        } else if let Some(ref parent_filter) = scoped {
            symbols
                .into_iter()
                .filter(|sym| sym.container.as_deref() == parent_filter.as_deref())
                .collect()
        } else {
            symbols
        };
        // In scoped mode the picker shows ONE level (the siblings of the
        // clicked segment). Strip nested children so `build_symbol_tree_items`
        // doesn't recurse and flatten the whole subtree below each sibling —
        // before this, clicking a top-level `impl` segment expanded every
        // impl's methods inline and the picker showed all ~92 symbols in the
        // file instead of just the sibling impls/structs/free functions.
        if is_scoped {
            for sym in &mut filtered {
                sym.children.clear();
            }
        }
        let path = self.active_buffer_path().unwrap_or_default();

        // Check if the symbols already have hierarchy (DocumentSymbol format)
        // or need reconstruction from the `container` field (SymbolInformation format).
        // Skip reconstruction when showing scoped siblings (breadcrumb filter active).
        let has_hierarchy = filtered.iter().any(|s| !s.children.is_empty());
        let tree_symbols = if scoped.is_some() || has_hierarchy {
            filtered
        } else {
            Self::rebuild_tree_from_containers(filtered)
        };

        // Build tree items depth-first, sorted by kind then name at each level
        self.picker_all_items.clear();
        Self::build_symbol_tree_items(&tree_symbols, &path, 0, &mut self.picker_all_items);

        // Re-run filter with current query
        let sub_query = self
            .picker_query
            .strip_prefix('@')
            .unwrap_or("")
            .trim_start()
            .to_string();
        if sub_query.is_empty() {
            // No query: show tree view with expand/collapse
            self.picker_rebuild_visible_tree();
        } else {
            // With query: flatten tree, fuzzy-filter all items
            Self::fuzzy_filter_items(
                &self.picker_all_items,
                &sub_query,
                100,
                &mut self.picker_items,
            );
            // Reset depth on filtered items so they display flat
            for item in &mut self.picker_items {
                item.depth = 0;
                item.expandable = false;
            }
        }
        // Pre-select the symbol closest to (and at or before) the cursor line,
        // matching VSCode's behavior of highlighting the current function.
        let cursor_line = self.view().cursor.line;
        let mut best_idx = 0usize;
        let mut best_line: Option<usize> = None;
        for (i, item) in self.picker_items.iter().enumerate() {
            if let PickerAction::GotoSymbol(_, line, _) = &item.action {
                if *line <= cursor_line && (best_line.is_none() || *line > best_line.unwrap()) {
                    best_line = Some(*line);
                    best_idx = i;
                }
            }
        }
        self.picker_selected = best_idx;
        self.picker_scroll_top = 0;
        self.picker_update_scroll();
        self.picker_load_preview();
    }

    /// Recursively walk a hierarchical document-symbol tree and return the
    /// first symbol whose start line matches `target_line`. Used by the
    /// breadcrumb scope filter to locate the clicked-on parent by line —
    /// language-server-agnostic, unlike name matching which breaks for impl
    /// blocks where tree-sitter and LSP disagree on naming (#465).
    fn find_symbol_at_line(
        symbols: &[lsp::SymbolInfo],
        target_line: usize,
    ) -> Option<&lsp::SymbolInfo> {
        for sym in symbols {
            if sym.line as usize == target_line {
                return Some(sym);
            }
            if let Some(found) = Self::find_symbol_at_line(&sym.children, target_line) {
                return Some(found);
            }
        }
        None
    }

    /// Reconstruct a tree from a flat symbol list using the `container` field.
    /// Groups symbols by their container name, creating parent SymbolInfo nodes
    /// with children populated. Symbols without a container stay at the top level.
    fn rebuild_tree_from_containers(flat: Vec<lsp::SymbolInfo>) -> Vec<lsp::SymbolInfo> {
        use std::collections::HashMap;

        // Collect children grouped by container name
        let mut children_map: HashMap<String, Vec<lsp::SymbolInfo>> = HashMap::new();
        let mut top_level: Vec<lsp::SymbolInfo> = Vec::new();

        for sym in &flat {
            if let Some(ref container) = sym.container {
                children_map
                    .entry(container.clone())
                    .or_default()
                    .push(sym.clone());
            }
        }

        // Build top-level: symbols that are containers (have children grouped under them)
        // or have no container themselves.
        let mut seen_containers: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for sym in flat {
            if sym.container.is_none() {
                // Top-level symbol — check if it's also a container for other symbols
                let mut s = sym;
                if let Some(kids) = children_map.remove(&s.name) {
                    s.children = kids;
                    seen_containers.insert(s.name.clone());
                }
                top_level.push(s);
            }
        }

        // Any remaining containers that weren't found as top-level symbols:
        // create synthetic parent nodes for them.
        for (container_name, kids) in children_map {
            if seen_containers.contains(&container_name) {
                continue;
            }
            // Find the first child to infer a reasonable line/kind for the synthetic parent
            let first = kids.first().cloned();
            let (line, character) = first
                .as_ref()
                .map(|k| (k.line.saturating_sub(1), 0))
                .unwrap_or((0, 0));
            top_level.push(lsp::SymbolInfo {
                name: container_name,
                kind: lsp::SymbolKind::Class, // Best guess for a container
                detail: None,
                container: None,
                path: first.and_then(|f| f.path),
                line,
                character,
                children: kids,
            });
        }

        top_level
    }

    /// Recursively build picker items from hierarchical symbols in depth-first order.
    /// Sorts children by (kind.sort_order(), name) at each level.
    fn build_symbol_tree_items(
        symbols: &[lsp::SymbolInfo],
        path: &std::path::Path,
        depth: usize,
        out: &mut Vec<PickerItem>,
    ) {
        // Sort by kind then name
        let mut sorted: Vec<&lsp::SymbolInfo> = symbols.iter().collect();
        sorted.sort_by(|a, b| {
            a.kind
                .sort_order()
                .cmp(&b.kind.sort_order())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        for sym in sorted {
            let has_children = !sym.children.is_empty();
            let display = format!("{} {}", sym.kind.icon(), sym.name);
            let detail = Some(sym.kind.label().to_string());
            let action = PickerAction::GotoSymbol(
                path.to_path_buf(),
                sym.line as usize,
                sym.character as usize,
            );
            // #262: symbol items are never expandable. Every row is a
            // jumpable destination (GotoSymbol). Children are emitted
            // flat at increasing depth so the indent still shows the
            // hierarchy, but Enter / click always jumps via picker_confirm
            // instead of taking the toggle-expand branch keyed off
            // `expandable`. VSCode breadcrumb picker has the same shape.
            out.push(PickerItem {
                filter_text: sym.name.clone(),
                display,
                detail,
                action,
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth,
                expandable: false,
                expanded: true,
            });
            if has_children {
                Self::build_symbol_tree_items(&sym.children, path, depth + 1, out);
            }
        }
    }

    /// Rebuild `picker_items` from `picker_all_items` respecting expand/collapse state.
    /// Only shows items whose ancestors are all expanded.
    pub(crate) fn picker_rebuild_visible_tree(&mut self) {
        self.picker_items.clear();
        let mut skip_depth: Option<usize> = None;
        for item in &self.picker_all_items {
            // If we're skipping collapsed children, check if we've exited the scope
            if let Some(sd) = skip_depth {
                if item.depth > sd {
                    continue; // Still inside collapsed parent
                } else {
                    skip_depth = None; // Exited collapsed parent's scope
                }
            }
            self.picker_items.push(item.clone());
            // If this item is expandable but not expanded, skip its children
            if item.expandable && !item.expanded {
                skip_depth = Some(item.depth);
            }
        }
    }

    /// Toggle expand/collapse on the currently selected picker item.
    /// Returns true if the item was expandable and was toggled.
    pub(crate) fn picker_toggle_expand(&mut self) -> bool {
        let sel = self.picker_selected;
        if sel >= self.picker_items.len() {
            return false;
        }
        let item = &self.picker_items[sel];
        if !item.expandable {
            return false;
        }

        // Find this item in picker_all_items and toggle its expanded state
        let target_display = item.display.clone();
        let target_depth = item.depth;
        let target_line = match &item.action {
            PickerAction::GotoSymbol(_, line, _) => Some(*line),
            _ => None,
        };
        for all_item in &mut self.picker_all_items {
            if all_item.display == target_display
                && all_item.depth == target_depth
                && matches!(&all_item.action, PickerAction::GotoSymbol(_, l, _) if Some(*l) == target_line)
            {
                all_item.expanded = !all_item.expanded;
                break;
            }
        }

        // Rebuild visible items
        self.picker_rebuild_visible_tree();
        // Try to keep selection on the same item
        self.picker_selected = self
            .picker_items
            .iter()
            .position(|i| {
                i.display == target_display
                    && i.depth == target_depth
                    && matches!(&i.action, PickerAction::GotoSymbol(_, l, _) if Some(*l) == target_line)
            })
            .unwrap_or(sel.min(self.picker_items.len().saturating_sub(1)));
        self.picker_update_scroll();
        true
    }

    /// Request workspace symbols from LSP.
    fn picker_request_workspace_symbols(&mut self, query: &str) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let path = match self.active_buffer_path() {
            Some(p) => p,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_workspace_symbols(&path, query) {
                self.lsp_pending_workspace_symbols = Some(id);
            }
        }
    }

    /// Populate picker items from a workspace symbol LSP response.
    pub(crate) fn picker_populate_workspace_symbols(&mut self, symbols: Vec<lsp::SymbolInfo>) {
        let cwd = self.cwd.clone();
        self.picker_items = symbols
            .into_iter()
            .take(100)
            .map(|sym| {
                let file_hint = sym
                    .path
                    .as_ref()
                    .and_then(|p| p.strip_prefix(&cwd).ok())
                    .map(|r| r.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let container_str = sym
                    .container
                    .as_ref()
                    .map(|c| format!("  ({})", c))
                    .unwrap_or_default();
                let display = format!("{} {}{}", sym.kind.icon(), sym.name, container_str);
                let detail = if file_hint.is_empty() {
                    Some(sym.kind.label().to_string())
                } else {
                    Some(format!("{} · {}", sym.kind.label(), file_hint))
                };
                let action = PickerAction::GotoSymbol(
                    sym.path.unwrap_or_default(),
                    sym.line as usize,
                    sym.character as usize,
                );
                PickerItem {
                    filter_text: sym.name.clone(),
                    display,
                    detail,
                    action,
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();
        self.picker_selected = 0;
        self.picker_scroll_top = 0;
        self.picker_load_preview();
    }

    /// Run a live project search for the Grep picker source.
    fn picker_grep_search(&mut self) {
        if self.picker_query.len() < 2 {
            self.picker_items.clear();
            return;
        }
        self.picker_cc_grep_search(&self.picker_query.clone());
    }

    /// Run a project grep search with a given query string and populate picker_items.
    /// Shared between the standalone Grep picker source and Command Center `%` prefix.
    ///
    /// Searches under [`Self::picker_grep_scope`] when set (the "Find in
    /// Folder..." context-menu action, #1418/#1438) — [`Self::cwd`]
    /// otherwise. Displayed paths always stay relative to [`Self::cwd`]
    /// (the workspace root), even when the search itself is scoped to a
    /// subfolder, so opening a result behaves exactly as it does for the
    /// unscoped picker (#1438).
    fn picker_cc_grep_search(&mut self, query: &str) {
        let options = project_search::SearchOptions::default();
        let search_root = self
            .picker_grep_scope
            .clone()
            .unwrap_or_else(|| self.cwd.clone());
        let display_root = self.cwd.clone();
        match project_search::search_in_project(&search_root, query, &options) {
            Ok(mut results) => {
                results.truncate(200);
                self.picker_items = results
                    .into_iter()
                    .map(|m| {
                        let rel = m
                            .file
                            .strip_prefix(&display_root)
                            .unwrap_or(&m.file)
                            .to_string_lossy()
                            .into_owned();
                        let display = format!("{}:{}: {}", rel, m.line + 1, m.line_text.trim());
                        PickerItem {
                            filter_text: display.clone(),
                            display,
                            detail: None,
                            action: PickerAction::OpenFileAtLine(m.file.clone(), m.line),
                            icon: None,
                            score: 0,
                            match_positions: Vec::new(),
                            depth: 0,
                            expandable: false,
                            expanded: false,
                        }
                    })
                    .collect();
            }
            Err(_) => self.picker_items.clear(),
        }
    }

    /// Populate picker items with launch configurations from `.vimcode/launch.json`.
    /// If no launch.json exists, offers a "Create launch.json..." option.
    fn picker_populate_debug_configs(&mut self, filter_query: &str) {
        use crate::core::dap_manager::{find_workspace_root, parse_launch_json};

        let manifests = self.ext_available_manifests();
        let workspace_root = find_workspace_root(&self.cwd, &manifests);
        let cwd_str = workspace_root.to_string_lossy().into_owned();

        // Try .vimcode/launch.json first, then .vscode/launch.json
        let vimcode_path = workspace_root.join(".vimcode").join("launch.json");
        let vscode_path = workspace_root.join(".vscode").join("launch.json");

        let configs = if let Ok(content) = std::fs::read_to_string(&vimcode_path) {
            parse_launch_json(&content, &cwd_str)
        } else if let Ok(content) = std::fs::read_to_string(&vscode_path) {
            parse_launch_json(&content, &cwd_str)
        } else {
            Vec::new()
        };

        if configs.is_empty() {
            self.picker_items = vec![PickerItem {
                display: "Create launch.json...".to_string(),
                filter_text: "create launch.json".to_string(),
                detail: Some("No launch configurations found".to_string()),
                action: PickerAction::Custom("create_launch_json".to_string()),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            }];
            return;
        }

        let all_items: Vec<PickerItem> = configs
            .iter()
            .enumerate()
            .map(|(idx, cfg)| {
                let detail = if cfg.program.is_empty() {
                    cfg.adapter_type.clone()
                } else {
                    format!("{} — {}", cfg.adapter_type, cfg.program)
                };
                PickerItem {
                    display: cfg.name.clone(),
                    filter_text: format!("{} {} {}", cfg.name, cfg.adapter_type, cfg.program),
                    detail: Some(detail),
                    action: PickerAction::Custom(format!("debug_config:{}", idx)),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();

        if filter_query.is_empty() {
            self.picker_items = all_items;
        } else {
            Self::fuzzy_filter_items(&all_items, filter_query, 100, &mut self.picker_items);
        }
    }

    /// Populate picker items with tasks from `.vimcode/tasks.json`.
    /// If no tasks.json exists, offers a "Configure Tasks..." option.
    fn picker_populate_tasks(&mut self, filter_query: &str) {
        use crate::core::dap_manager::{
            find_workspace_root, parse_tasks_json, task_to_shell_command,
        };

        let manifests = self.ext_available_manifests();
        let workspace_root = find_workspace_root(&self.cwd, &manifests);
        let cwd_str = workspace_root.to_string_lossy().into_owned();

        // Try .vimcode/tasks.json first, then .vscode/tasks.json
        let vimcode_path = workspace_root.join(".vimcode").join("tasks.json");
        let vscode_path = workspace_root.join(".vscode").join("tasks.json");

        let tasks = if let Ok(content) = std::fs::read_to_string(&vimcode_path) {
            parse_tasks_json(&content, &cwd_str)
        } else if let Ok(content) = std::fs::read_to_string(&vscode_path) {
            parse_tasks_json(&content, &cwd_str)
        } else {
            Vec::new()
        };

        if tasks.is_empty() {
            self.picker_items = vec![PickerItem {
                display: "Configure Tasks...".to_string(),
                filter_text: "configure tasks".to_string(),
                detail: Some("No tasks found — create tasks.json".to_string()),
                action: PickerAction::Custom("create_tasks_json".to_string()),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            }];
            return;
        }

        let all_items: Vec<PickerItem> = tasks
            .iter()
            .map(|task| {
                let cmd = task_to_shell_command(task);
                PickerItem {
                    display: task.label.clone(),
                    filter_text: format!("{} {}", task.label, cmd),
                    detail: Some(cmd.clone()),
                    action: PickerAction::Custom(format!("task_run:{}", cmd)),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                }
            })
            .collect();

        if filter_query.is_empty() {
            self.picker_items = all_items;
        } else {
            Self::fuzzy_filter_items(&all_items, filter_query, 100, &mut self.picker_items);
        }
    }

    /// Create a default tasks.json in `.vimcode/` and open it in a buffer.
    fn create_and_open_tasks_json(&mut self) {
        use crate::core::dap_manager::find_workspace_root;

        let manifests = self.ext_available_manifests();
        let workspace_root = find_workspace_root(&self.cwd, &manifests);
        let vimcode_dir = workspace_root.join(".vimcode");
        let tasks_path = vimcode_dir.join("tasks.json");

        if !tasks_path.exists() {
            let _ = std::fs::create_dir_all(&vimcode_dir);
            let template = r#"{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "build",
      "type": "shell",
      "command": "cargo build"
    },
    {
      "label": "test",
      "type": "shell",
      "command": "cargo test"
    }
  ]
}"#;
            let _ = std::fs::write(&tasks_path, template);
        }

        self.open_file_in_tab(&tasks_path);
    }

    /// Populate picker items for AI chat mode.
    /// If a question is provided, shows a "Send to AI" item.
    /// Otherwise shows "Open AI Panel".
    fn picker_populate_chat(&mut self, question: &str) {
        // An ACP agent (`acp_agent_command`) is a third valid transport
        // alongside a direct-provider API key or Ollama (#952 ACP-1) —
        // `ai_send_message` already routes to it, so the palette's gate
        // must recognise it too, or an ACP-only user (no `ai_api_key`,
        // default `ai_provider`) hits "Configure AI provider first" and
        // never reaches the `chat_send:` action.
        let configured = !self.settings.ai_api_key.is_empty()
            || self.settings.ai_provider == "ollama"
            || !self.settings.acp_agent_command.trim().is_empty();

        if !configured {
            self.picker_items = vec![PickerItem {
                display: "Configure AI provider first".to_string(),
                filter_text: String::new(),
                detail: Some(":set ai_provider=anthropic  :set ai_api_key=sk-...".to_string()),
                action: PickerAction::Custom("chat_configure".to_string()),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            }];
            return;
        }

        if question.is_empty() {
            self.picker_items = vec![
                PickerItem {
                    display: "Open AI Panel".to_string(),
                    filter_text: "open ai panel chat".to_string(),
                    detail: Some("Focus the AI chat sidebar".to_string()),
                    action: PickerAction::Custom("chat_open".to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                },
                PickerItem {
                    display: "Type a question after 'chat '...".to_string(),
                    filter_text: String::new(),
                    detail: Some("e.g. chat explain this function".to_string()),
                    action: PickerAction::Custom("hint".to_string()),
                    icon: None,
                    score: 0,
                    match_positions: Vec::new(),
                    depth: 0,
                    expandable: false,
                    expanded: false,
                },
            ];
        } else {
            self.picker_items = vec![PickerItem {
                display: format!("Ask AI: {}", question),
                filter_text: question.to_string(),
                detail: Some(format!("Send to {} AI", self.settings.ai_provider)),
                action: PickerAction::Custom(format!("chat_send:{}", question)),
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth: 0,
                expandable: false,
                expanded: false,
            }];
        }
    }

    /// Scroll the picker selection by `delta` rows (positive = down).
    /// `visible_rows` is the number of visible result rows in the popup.
    pub fn picker_scroll(&mut self, delta: isize, visible_rows: usize) {
        let max = self.picker_items.len().saturating_sub(1);
        if delta > 0 {
            self.picker_selected = (self.picker_selected + delta as usize).min(max);
        } else {
            self.picker_selected = self.picker_selected.saturating_sub((-delta) as usize);
        }
        if self.picker_selected >= self.picker_scroll_top + visible_rows {
            self.picker_scroll_top = self.picker_selected + 1 - visible_rows;
        }
        if self.picker_selected < self.picker_scroll_top {
            self.picker_scroll_top = self.picker_selected;
        }
        self.picker_load_preview();
    }

    /// Load preview context for the currently selected picker item.
    pub(crate) fn picker_load_preview(&mut self) {
        self.picker_preview = None;
        let Some(item) = self.picker_items.get(self.picker_selected) else {
            return;
        };
        match &item.action {
            PickerAction::OpenFile(rel_path) => {
                let abs = self.cwd.join(rel_path);
                let Ok(content) = std::fs::read_to_string(&abs) else {
                    return;
                };
                let lines: Vec<(usize, String, bool)> = content
                    .lines()
                    .take(500)
                    .enumerate()
                    .map(|(i, text)| (i + 1, text.to_string(), false))
                    .collect();
                self.picker_preview = Some(PickerPreview { lines });
                self.picker_preview_scroll = 0;
            }
            PickerAction::OpenFileAtLine(path, line) => {
                let Ok(content) = std::fs::read_to_string(path) else {
                    return;
                };
                let all_lines: Vec<&str> = content.lines().collect();
                let match_line = *line;
                // Show enough context for meaningful scrolling in the preview pane.
                let context = 50usize;
                let start = match_line.saturating_sub(context);
                let end = (match_line + context + 1).min(all_lines.len());
                let lines: Vec<(usize, String, bool)> = all_lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, text)| {
                        let lineno = start + i + 1;
                        let is_match = (start + i) == match_line;
                        (lineno, text.to_string(), is_match)
                    })
                    .collect();
                self.picker_preview = Some(PickerPreview { lines });
                // Scroll so the match line is visible near the top of the preview.
                let match_offset = match_line.saturating_sub(start);
                self.picker_preview_scroll = match_offset.saturating_sub(3);
            }
            _ => {}
        }
    }

    /// Execute the currently selected picker item.
    pub fn picker_confirm(&mut self) -> EngineAction {
        self.picker_push_history();
        let Some(item) = self.picker_items.get(self.picker_selected).cloned() else {
            self.close_picker();
            return EngineAction::None;
        };
        self.close_picker();

        match item.action {
            PickerAction::OpenFile(rel_path) => {
                self.push_jump_location();
                let abs = self.cwd.join(&rel_path);
                self.open_file_in_tab(&abs);
                EngineAction::None
            }
            PickerAction::OpenFileAtLine(path, line) => {
                self.push_jump_location();
                self.open_file_in_tab(&path);
                let win_id = self.active_window_id();
                self.set_cursor_for_window(win_id, line, 0);
                self.scroll_cursor_center();
                EngineAction::None
            }
            PickerAction::ExecuteCommand(action) => {
                // Same logic as palette_confirm for special actions
                if !self.is_vscode_mode() {
                    self.mode = Mode::Normal;
                }
                match action.as_str() {
                    "fuzzy" => {
                        self.open_picker(PickerSource::Files);
                        EngineAction::None
                    }
                    "grep" => {
                        self.open_picker(PickerSource::Grep);
                        EngineAction::None
                    }
                    "goto_line" => {
                        // #193: open Command mode so the user can type a
                        // line number directly. Previously printed a
                        // status message suggesting `:N` — unhelpful as
                        // a first-class palette action.
                        self.mode = Mode::Command;
                        self.command_buffer.clear();
                        self.command_cursor = 0;
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
                    "substitute" => {
                        // #193: open the find/replace overlay directly
                        // (same as the Ctrl+H binding) rather than
                        // printing a tip about :%s/...
                        self.open_find_replace();
                        // Ensure the replace row is visible so "Find and
                        // Replace" actually lands on a replace-capable UI
                        // and not just the find-only overlay.
                        self.find_replace_show_replace = true;
                        EngineAction::None
                    }
                    "jump_back" => {
                        self.jump_list_back();
                        EngineAction::None
                    }
                    "lsp_definition" => {
                        self.lsp_request_definition();
                        EngineAction::None
                    }
                    "lsp_references" => {
                        self.lsp_request_references();
                        EngineAction::None
                    }
                    "set_wrap_toggle" => {
                        self.settings.wrap = !self.settings.wrap;
                        let _ = self.settings.save();
                        let state = if self.settings.wrap { "wrap" } else { "nowrap" };
                        self.message = format!("set {}", state);
                        EngineAction::None
                    }
                    "set_number_toggle" => {
                        use crate::core::settings::LineNumberMode;
                        self.settings.line_numbers = match self.settings.line_numbers {
                            LineNumberMode::None | LineNumberMode::Relative => {
                                LineNumberMode::Absolute
                            }
                            LineNumberMode::Absolute | LineNumberMode::Hybrid => {
                                LineNumberMode::None
                            }
                        };
                        let _ = self.settings.save();
                        EngineAction::None
                    }
                    "set_rnu_toggle" => {
                        use crate::core::settings::LineNumberMode;
                        self.settings.line_numbers = match self.settings.line_numbers {
                            LineNumberMode::None | LineNumberMode::Absolute => {
                                LineNumberMode::Relative
                            }
                            LineNumberMode::Relative | LineNumberMode::Hybrid => {
                                LineNumberMode::None
                            }
                        };
                        let _ = self.settings.save();
                        EngineAction::None
                    }
                    "toggle_spell" => {
                        self.settings.spell = !self.settings.spell;
                        if self.settings.spell {
                            self.ensure_spell_checker();
                            self.message = "Spell checking enabled".to_string();
                        } else {
                            self.message = "Spell checking disabled".to_string();
                        }
                        let _ = self.settings.save();
                        EngineAction::None
                    }
                    "nop" => EngineAction::None,
                    other => self.execute_command(other),
                }
            }
            PickerAction::CheckoutBranch(branch) => {
                self.execute_command(&format!("Gswitch {}", branch))
            }
            PickerAction::SetLanguage(lang) => {
                // Set the language ID on the active buffer and re-run syntax
                let bid = self.active_buffer_id();
                if let Some(state) = self.buffer_manager.get_mut(bid) {
                    state.lsp_language_id = Some(lang.clone());
                    // Update syntax parser for the new language
                    state.syntax = crate::core::syntax::Syntax::new_from_language_id_with_overrides(
                        &lang,
                        Some(&self.highlight_overrides),
                    );
                    state.update_syntax();
                }
                self.message = format!("Language mode: {}", lang);
                EngineAction::None
            }
            PickerAction::SetIndentation(expand, width) => {
                self.settings.expand_tab = expand;
                self.settings.tabstop = width;
                self.settings.shift_width = width;
                let _ = self.settings.save();
                self.message = if expand {
                    format!("Spaces: {}", width)
                } else {
                    format!("Tab Size: {}", width)
                };
                EngineAction::None
            }
            PickerAction::SetLineEnding(is_crlf) => {
                use crate::core::buffer_manager::LineEnding;
                let new = if is_crlf {
                    LineEnding::Crlf
                } else {
                    LineEnding::LF
                };
                let bid = self.active_buffer_id();
                if let Some(state) = self.buffer_manager.get_mut(bid) {
                    state.set_line_ending(new);
                }
                self.message = format!("Line endings: {}", new.as_str());
                EngineAction::None
            }
            PickerAction::OpenWorkspace(path) => {
                // #274: recent-workspaces picker confirm. Switch workspace,
                // focus the explorer (so the active file's row renders with
                // the focused selection bg — surfaces visibly in GTK whose
                // inactive_selected_bg is too close to surface_bg to read),
                // re-reveal the active file (open_file_in_tab already did
                // it once but explorer_needs_refresh triggers a backend
                // rebuild that can desync the row idx), then signal the
                // backends to refresh.
                self.open_folder(&path);
                self.explorer_has_focus = true;
                self.explorer_reveal_active_file();
                self.explorer_needs_refresh = true;
                EngineAction::None
            }
            PickerAction::JumpToMark(_mark) => {
                // Phase 3: mark jumping via picker
                EngineAction::None
            }
            PickerAction::PasteRegister(_reg) => {
                // Phase 3: register paste via picker
                EngineAction::None
            }
            PickerAction::GotoLine(line) => {
                self.push_jump_location();
                let win_id = self.active_window_id();
                self.set_cursor_for_window(win_id, line, 0);
                self.scroll_cursor_center();
                EngineAction::None
            }
            PickerAction::GotoSymbol(path, line, _col) => {
                self.push_jump_location();
                if !path.as_os_str().is_empty() {
                    // Check if it's a different file than the current buffer
                    let cur_path = self
                        .buffer_manager
                        .get(self.active_buffer_id())
                        .and_then(|s| s.file_path.clone())
                        .unwrap_or_default();
                    if path != cur_path {
                        self.open_file_in_tab(&path);
                    }
                }
                let win_id = self.active_window_id();
                self.set_cursor_for_window(win_id, line, 0);
                self.scroll_cursor_center();
                EngineAction::None
            }
            PickerAction::Custom(key) => {
                // Handle prefix selection from help mode
                if let Some(prefix) = key.strip_prefix("prefix:") {
                    self.open_command_center();
                    if prefix.is_empty() {
                        // "Go to File" — stay in file search mode with empty query
                        // (open_command_center already set up files + hints)
                        // Force file mode by setting a no-op query state
                        self.picker_populate_files();
                        self.picker_items =
                            self.picker_all_items.iter().take(100).cloned().collect();
                    } else {
                        self.picker_query = if prefix.contains(char::is_alphabetic) {
                            format!("{} ", prefix)
                        } else {
                            prefix.to_string()
                        };
                        self.picker_selected = 0;
                        self.picker_scroll_top = 0;
                        self.picker_filter();
                        self.picker_load_preview();
                    }
                    EngineAction::None
                } else if let Some(idx_str) = key.strip_prefix("debug_config:") {
                    // Launch a debug configuration by index
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        self.dap_selected_launch_config = idx;
                        self.close_picker();
                        let _ = self.execute_command("debug");
                    }
                    EngineAction::None
                } else if key == "create_launch_json" {
                    // Generate launch.json and start debugging
                    self.close_picker();
                    let _ = self.execute_command("debug");
                    EngineAction::None
                } else if let Some(cmd) = key.strip_prefix("task_run:") {
                    // Run a task command in the integrated terminal
                    let cmd = cmd.to_string();
                    self.close_picker();
                    EngineAction::RunInTerminal(cmd)
                } else if key == "create_tasks_json" {
                    // Open .vimcode/tasks.json for editing
                    self.close_picker();
                    self.create_and_open_tasks_json();
                    EngineAction::None
                } else if key == "chat_open" {
                    // Focus the AI chat panel
                    self.close_picker();
                    self.ai_has_focus = true;
                    EngineAction::None
                } else if let Some(question) = key.strip_prefix("chat_send:") {
                    // Send a question to the AI provider
                    let question = question.to_string();
                    self.close_picker();
                    self.ai_send_message(question);
                    self.ai_has_focus = true;
                    EngineAction::None
                } else if key == "chat_configure" {
                    // Open settings to configure AI
                    self.close_picker();
                    self.settings_has_focus = true;
                    EngineAction::None
                } else {
                    EngineAction::None
                }
            }
        }
    }

    /// Save the current picker query to per-source history (dedup consecutive).
    fn picker_push_history(&mut self) {
        let q = self.picker_query.trim().to_string();
        if q.is_empty() {
            return;
        }
        let hist = self
            .picker_history
            .entry(self.picker_source.clone())
            .or_default();
        if hist.last().is_none_or(|last| *last != q) {
            hist.push(q);
            // Cap at 100 entries.
            if hist.len() > 100 {
                hist.remove(0);
            }
        }
    }

    /// Exit history browsing mode, resetting the index.
    fn picker_exit_history(&mut self) {
        self.picker_history_index = None;
        self.picker_history_typing_buffer.clear();
    }

    /// Route a key press when the unified picker is open.
    pub fn handle_picker_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> EngineAction {
        match key_name {
            "Escape" => {
                self.close_picker();
                EngineAction::None
            }
            "Return" => {
                // In symbol tree mode (@), Enter on expandable items toggles expand
                if self.picker_source == PickerSource::CommandCenter
                    && self.picker_query.starts_with('@')
                {
                    let sub_query = self.picker_query.strip_prefix('@').unwrap_or("").trim();
                    if sub_query.is_empty() {
                        // Tree view active — check if selected item is expandable
                        if self.picker_toggle_expand() {
                            self.picker_load_preview();
                            return EngineAction::None;
                        }
                    }
                }
                self.picker_confirm()
            }
            "Right" => {
                // In symbol tree mode, Right expands collapsed item
                if self.picker_source == PickerSource::CommandCenter && self.picker_query == "@" {
                    if let Some(item) = self.picker_items.get(self.picker_selected) {
                        if item.expandable && !item.expanded {
                            self.picker_toggle_expand();
                            self.picker_load_preview();
                            return EngineAction::None;
                        }
                    }
                }
                EngineAction::None
            }
            "Left" => {
                // In symbol tree mode, Left collapses expanded item
                if self.picker_source == PickerSource::CommandCenter && self.picker_query == "@" {
                    if let Some(item) = self.picker_items.get(self.picker_selected) {
                        if item.expandable && item.expanded {
                            self.picker_toggle_expand();
                            self.picker_load_preview();
                            return EngineAction::None;
                        }
                    }
                }
                EngineAction::None
            }
            "Down" | "Tab" => {
                if self.picker_history_index.is_some() {
                    // Navigate forward in history or exit history mode.
                    let hist = self
                        .picker_history
                        .get(&self.picker_source)
                        .cloned()
                        .unwrap_or_default();
                    let idx = self.picker_history_index.unwrap();
                    if idx + 1 < hist.len() {
                        self.picker_history_index = Some(idx + 1);
                        self.picker_query = hist[idx + 1].clone();
                    } else {
                        // Past newest entry — restore the original typed query.
                        self.picker_query = std::mem::take(&mut self.picker_history_typing_buffer);
                        self.picker_history_index = None;
                    }
                    self.picker_selected = 0;
                    self.picker_scroll_top = 0;
                    self.picker_filter();
                    self.picker_load_preview();
                } else {
                    let max = self.picker_items.len().saturating_sub(1);
                    self.picker_selected = (self.picker_selected + 1).min(max);
                    self.picker_update_scroll();
                    self.picker_load_preview();
                }
                EngineAction::None
            }
            "Up" => {
                if self.picker_selected == 0 {
                    // At top of results — enter or continue history browsing.
                    let hist_len = self
                        .picker_history
                        .get(&self.picker_source)
                        .map_or(0, |h| h.len());
                    if hist_len > 0 {
                        let hist = &self.picker_history[&self.picker_source];
                        let new_idx = match self.picker_history_index {
                            None => {
                                // Enter history mode — save current query.
                                self.picker_history_typing_buffer = self.picker_query.clone();
                                hist_len - 1
                            }
                            Some(idx) => idx.saturating_sub(1),
                        };
                        self.picker_history_index = Some(new_idx);
                        self.picker_query = hist[new_idx].clone();
                        self.picker_selected = 0;
                        self.picker_scroll_top = 0;
                        self.picker_filter();
                        self.picker_load_preview();
                    }
                } else {
                    self.picker_selected = self.picker_selected.saturating_sub(1);
                    self.picker_update_scroll();
                    self.picker_load_preview();
                }
                EngineAction::None
            }
            "n" if ctrl => {
                let max = self.picker_items.len().saturating_sub(1);
                self.picker_selected = (self.picker_selected + 1).min(max);
                self.picker_update_scroll();
                self.picker_load_preview();
                EngineAction::None
            }
            "p" if ctrl => {
                self.picker_selected = self.picker_selected.saturating_sub(1);
                self.picker_update_scroll();
                self.picker_load_preview();
                EngineAction::None
            }
            "v" if ctrl => {
                // Paste clipboard into picker query
                if let Some(text) = self.clipboard_read.as_ref().and_then(|cb| cb().ok()) {
                    // Take first line only, strip control chars
                    let line = text.lines().next().unwrap_or("");
                    for c in line.chars() {
                        if !c.is_control() {
                            self.picker_query.push(c);
                        }
                    }
                    self.picker_exit_history();
                    self.picker_selected = 0;
                    self.picker_scroll_top = 0;
                    self.picker_filter();
                    self.picker_load_preview();
                }
                EngineAction::None
            }
            "BackSpace" => {
                self.picker_exit_history();
                self.picker_query.pop();
                self.picker_selected = 0;
                self.picker_scroll_top = 0;
                self.picker_filter();
                self.picker_load_preview();
                EngineAction::None
            }
            _ => {
                if !ctrl {
                    if let Some(c) = unicode {
                        if !c.is_control() {
                            self.picker_exit_history();
                            self.picker_query.push(c);
                            self.picker_selected = 0;
                            self.picker_scroll_top = 0;
                            self.picker_filter();
                            self.picker_load_preview();
                        }
                    }
                }
                EngineAction::None
            }
        }
    }

    /// Adjust scroll_top so the selected item is visible.
    ///
    /// The engine doesn't know the actual renderer row count, so this uses
    /// a conservative heuristic. Renderers are the authoritative source of
    /// truth: `quadraui_tui::draw_palette` and `quadraui_gtk::draw_palette`
    /// both clamp `scroll_offset` at render time to guarantee the selected
    /// item is always visible regardless of the engine's estimate.
    fn picker_update_scroll(&mut self) {
        // Small enough that narrow terminals don't leave the selection
        // off-screen via the engine's scroll state. Renderer clamp catches
        // the rest.
        let visible = 8usize;
        if self.picker_selected < self.picker_scroll_top {
            self.picker_scroll_top = self.picker_selected;
        } else if self.picker_selected >= self.picker_scroll_top + visible {
            self.picker_scroll_top = self.picker_selected + 1 - visible;
        }
    }
}

// ─── Quickfix / location list ──────────────────────────────────────────────
//
// Vim's location list is a per-window twin of the global quickfix list —
// same shape, same navigation, same panel, just scoped to a window instead
// of shared globally. Rather than writing the open/close/navigate/jump
// logic twice, every `qf_*` method below is generic over *which* list it
// targets via `win: Option<WindowId>` (`None` = the global quickfix list,
// `Some(id)` = window `id`'s location list) and is the single
// implementation behind both the `:c*` and `:l*` ex-command families
// (`src/core/engine/execute.rs`) (#1155).

impl Engine {
    /// Borrow the target list mutably, creating an empty location list on
    /// first use (mirrors how the global quickfix list starts out empty
    /// rather than absent).
    pub fn qf_get_mut(&mut self, win: Option<WindowId>) -> &mut QuickfixList {
        match win {
            None => &mut self.quickfix,
            Some(w) => self.location_lists.entry(w).or_default(),
        }
    }

    /// Borrow the target list read-only, without creating a location list
    /// that doesn't exist yet.
    pub fn qf_get(&self, win: Option<WindowId>) -> Option<&QuickfixList> {
        match win {
            None => Some(&self.quickfix),
            Some(w) => self.location_lists.get(&w),
        }
    }

    /// The "list is empty" message for the target, matching Vim's distinct
    /// quickfix (`E42`) vs location-list (`E776`) wording.
    ///
    /// #1283: confirmed by hand against `nvim --headless -u NONE` — the
    /// global-list message is `E42: No Errors` verbatim (capital `E` in
    /// `Errors`), not the "Quickfix list is empty" prose this used to read.
    /// The wrong casing/wording was invisible before #1283 because nothing
    /// in the oracle corpus compared it: the buffer/cursor-only `Case`
    /// harness `ex:cc on empty quickfix list` (#1154) can't see `message` at
    /// all, and its paired engine-level test asserted the same wrong text
    /// back (`.contains("No errors")`) — see that test's #1283 update.
    pub(super) fn qf_empty_msg(win: Option<WindowId>) -> String {
        if win.is_some() {
            "E776: No location list".to_string()
        } else {
            "E42: No Errors".to_string()
        }
    }

    /// Replace the target list's contents (used by `:grep`/`:vimgrep`/
    /// `:lgrep`/`:lvimgrep` and by LSP producers such as "Find References").
    /// Replacing the *global* list snapshots the new contents onto
    /// `quickfix_stack` for `:colder`/`:cnewer`.
    pub fn qf_set_list(&mut self, win: Option<WindowId>, items: Vec<ProjectMatch>) {
        let n = items.len();
        let list = self.qf_get_mut(win);
        list.items = items;
        list.selected = 0;
        list.open = true;
        list.has_focus = n > 0;
        if win.is_none() {
            self.qf_push_history();
        }
        // #1307: this flips `open` unconditionally (existing behaviour,
        // unrelated to #1307) but deliberately does *not* call
        // `qf_ensure_panel_window` — a plain `:grep`/`:vimgrep` re-run must
        // not conjure a real split window out of nowhere; only `:copen`/
        // `:lopen`/`:cwindow`/`:lwindow` do that. If one is *already* open
        // (from an earlier `:copen`), keep it live instead of leaving it
        // showing stale entries.
        self.qf_refresh_panel_if_open(win);
    }

    /// Snapshot the (just-updated) global quickfix list onto
    /// `quickfix_stack`, truncating any "newer" entries beyond the current
    /// browse position first (creating a new list while parked on an older
    /// one via `:colder` discards the ones ahead, like an undo tree branch)
    /// and capping the stack at 10 entries — Vim's default quickfix-stack
    /// depth. `quickfix_stack[quickfix_stack_pos]` always equals
    /// `self.quickfix` immediately after this call.
    fn qf_push_history(&mut self) {
        const MAX_QF_HISTORY: usize = 10;
        self.quickfix_stack.truncate(self.quickfix_stack_pos + 1);
        self.quickfix_stack.push(self.quickfix.clone());
        if self.quickfix_stack.len() > MAX_QF_HISTORY {
            self.quickfix_stack.remove(0);
        }
        self.quickfix_stack_pos = self.quickfix_stack.len() - 1;
    }

    /// `:colder [count]` — move `count` (default 1) steps back through the
    /// quickfix stack.
    pub fn qf_colder(&mut self, count: usize) -> EngineAction {
        let count = count.max(1);
        if self.quickfix_stack.is_empty() || self.quickfix_stack_pos == 0 {
            self.message = "E380: At bottom of quickfix stack".to_string();
            return EngineAction::None;
        }
        self.quickfix_stack_pos = self.quickfix_stack_pos.saturating_sub(count);
        self.quickfix = self.quickfix_stack[self.quickfix_stack_pos].clone();
        self.qf_refresh_panel_if_open(None);
        self.message = format!(
            "list {} of {}",
            self.quickfix_stack_pos + 1,
            self.quickfix_stack.len()
        );
        EngineAction::None
    }

    /// `:cnewer [count]` — move `count` (default 1) steps forward through
    /// the quickfix stack.
    pub fn qf_newer(&mut self, count: usize) -> EngineAction {
        let count = count.max(1);
        if self.quickfix_stack.is_empty()
            || self.quickfix_stack_pos + 1 >= self.quickfix_stack.len()
        {
            self.message = "E381: At top of quickfix stack".to_string();
            return EngineAction::None;
        }
        self.quickfix_stack_pos =
            (self.quickfix_stack_pos + count).min(self.quickfix_stack.len() - 1);
        self.quickfix = self.quickfix_stack[self.quickfix_stack_pos].clone();
        self.qf_refresh_panel_if_open(None);
        self.message = format!(
            "list {} of {}",
            self.quickfix_stack_pos + 1,
            self.quickfix_stack.len()
        );
        EngineAction::None
    }

    /// Format one entry the way a real panel window's buffer displays it:
    /// `"file.rs:12: line text"` — no directory, matching real Neovim's
    /// quickfix buffer lines (`:h quickfix-window-function`'s default
    /// formatter).
    fn qf_format_item(m: &ProjectMatch) -> String {
        let f = m.file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let snippet: String = m.line_text.trim().chars().take(80).collect();
        format!("{}:{}: {}", f, m.line + 1, snippet)
    }

    /// Format every entry in `list` (see [`Self::qf_format_item`]).
    fn qf_format_items(list: &QuickfixList) -> Vec<String> {
        list.items.iter().map(Self::qf_format_item).collect()
    }

    /// If a real panel window is currently open for `target` (#1307),
    /// refresh its buffer text from the list's current items and move its
    /// cursor to the list's current `selected` index — call after anything
    /// that mutates a `QuickfixList`'s `items`/`selected` so an already-open
    /// panel window stays live, the way Neovim's own quickfix buffer does.
    /// A no-op when no real window is open for `target` (the common case for
    /// `qf_set_list`'s implicit `:grep`/`:vimgrep` auto-open, which only
    /// ever flips the legacy overlay flags — see that method's own doc).
    fn qf_refresh_panel_if_open(&mut self, target: Option<WindowId>) {
        let Some(&win_id) = self.qf_panel_windows.get(&target) else {
            return;
        };
        if !self.windows.contains_key(&win_id) {
            return;
        }
        let (lines, selected) = match self.qf_get(target) {
            Some(list) => (Self::qf_format_items(list), list.selected),
            None => return,
        };
        self.qf_refresh_panel_window(win_id, &lines);
        self.qf_set_panel_cursor(win_id, selected);
    }

    /// Does `target` have a real `WindowLayout` leaf open for it right now
    /// (#1307)? `render.rs`'s legacy overlay-band renderer
    /// (`quickfix_panel_rows`, `compute_editor_layout`, `build_screen_
    /// layout_with_breadcrumb_row`'s `screen.quickfix` builder) calls this
    /// to self-suppress for a target that already has a real window —
    /// painting both at once would double up the same content in two
    /// places. Any caller that still drives `QuickfixList::open`/
    /// `has_focus` directly (most of this codebase's own rendering tests,
    /// and `qf_set_list`'s implicit `:grep` auto-open) never populates
    /// `qf_panel_windows`, so this stays `false` for them and the overlay
    /// keeps rendering exactly as it always has.
    pub fn qf_has_real_window(&self, target: Option<WindowId>) -> bool {
        self.qf_panel_windows
            .get(&target)
            .is_some_and(|id| self.windows.contains_key(id))
    }

    /// The window id `execute_command`'s `:l*` family should pass as
    /// `qf_get`'s `win` — ordinarily just the active window, *except* when
    /// the active window is itself a location-list panel window (#1307): a
    /// real Neovim location-list window keeps a hidden reference to the
    /// window it was opened for (`getloclist(0, {'filewinid': 0})`), so
    /// `:lclose`/`:lnext`/... typed *while inside* the panel still target
    /// that owner, not the panel window itself. Every `:l*` ex-command
    /// handler in `execute.rs` calls this instead of `active_window_id()`
    /// directly.
    pub(crate) fn qf_loc_target_window(&self) -> WindowId {
        match self.qf_panel_target(self.active_window_id()) {
            Some(Some(owner)) => owner,
            _ => self.active_window_id(),
        }
    }

    /// If `id` is a quickfix/location-list panel window (#1307), the list it
    /// targets (`qf_get`'s `win` shape: `None` = global, `Some(owner)` =
    /// window `owner`'s location list). `None` if `id` isn't a panel window.
    pub(crate) fn qf_panel_target(&self, id: WindowId) -> Option<Option<WindowId>> {
        self.qf_panel_windows
            .iter()
            .find(|(_, &w)| w == id)
            .map(|(&target, _)| target)
    }

    /// Undo `qf_open`'s bookkeeping when `window_id` — possibly a quickfix/
    /// location-list panel window (#1307) — is removed from `self.windows`
    /// through *any* window-close path, not just `qf_close`/
    /// `qf_close_panel_window` (`CTRL-W q`/`CTRL-W c` on a focused panel
    /// window, `:only`, closing its owning tab/group, a diff-pair cleanup,
    /// ...). Every one of those already removes the window correctly on its
    /// own — a panel window is now an entirely ordinary `Window`/
    /// `WindowLayout` leaf — so this only keeps `QuickfixList::open`/
    /// `has_focus` from lying about a window that no longer exists, and
    /// drops the now-stale `qf_panel_windows` entry. A no-op for any id that
    /// isn't a panel window (the overwhelmingly common case, since
    /// `qf_panel_windows` rarely holds more than one or two entries).
    pub(crate) fn forget_closed_panel_window(&mut self, window_id: WindowId) {
        if let Some(target) = self.qf_panel_target(window_id) {
            self.qf_panel_windows.remove(&target);
            let list = self.qf_get_mut(target);
            list.open = false;
            list.has_focus = false;
        }
    }

    /// Open the target panel and give it focus.
    ///
    /// #1283 finding, confirmed by hand against `nvim --headless -u NONE`:
    /// `:copen` opens the quickfix window **unconditionally** — even on a
    /// totally empty list (`winnr('$')` goes 1 -> 2, no error) — so the
    /// global list (`win == None`, which always exists per `qf_get`) must
    /// never refuse here. `:lopen` only refuses when the *window has never
    /// had a location list at all* (`E776: No location list`); once one
    /// exists — even populated with zero items via `setloclist(0, [])` —
    /// `:lopen` opens it exactly like `:copen`. `qf_get` (read-only, no
    /// autovivify) is what distinguishes "never created" from "empty",
    /// unlike `qf_get_mut`'s `entry(..).or_default()` this used to check
    /// against, which collapsed both into the same wrong refusal.
    ///
    /// #1307: also opens (or reuses) the panel's real `WindowLayout` leaf
    /// (`qf_ensure_panel_window`) — the reason `:copen`/`:lopen` are the
    /// entry points that get a real window at all, unlike `qf_set_list`'s
    /// implicit auto-open, which only flips the flags below.
    pub fn qf_open(&mut self, win: Option<WindowId>) -> EngineAction {
        if win.is_some() && self.qf_get(win).is_none() {
            self.message = Self::qf_empty_msg(win);
            return EngineAction::None;
        }
        let lines = Self::qf_format_items(self.qf_get(win).expect("checked above"));
        let scratch_name = if win.is_some() {
            "[Location List]"
        } else {
            "[Quickfix List]"
        };
        self.qf_ensure_panel_window(win, &lines, scratch_name);
        let list = self.qf_get_mut(win);
        list.open = true;
        list.has_focus = true;
        EngineAction::None
    }

    /// Close the target panel, including its real `WindowLayout` leaf if one
    /// is open (#1307).
    pub fn qf_close(&mut self, win: Option<WindowId>) -> EngineAction {
        self.qf_close_panel_window(win);
        let list = self.qf_get_mut(win);
        list.open = false;
        list.has_focus = false;
        EngineAction::None
    }

    /// `:cwindow`/`:lwindow` — open the panel only if the target list is
    /// non-empty; close it (if open) otherwise.
    ///
    /// #1283 finding: `:cwindow` never errors (the global list always
    /// exists), matching the pre-existing behaviour below. But `:lwindow`
    /// on a window with **no location list at all** does error (`E776: No
    /// location list`, confirmed against a live oracle) rather than silently
    /// no-op like it does once a list exists (even an empty one, via
    /// `setloclist(0, [])`) — the same "never created" vs "empty" split as
    /// `qf_open` above, so it needs the same `qf_get` (not `qf_get_mut`)
    /// existence check first.
    pub fn qf_window(&mut self, win: Option<WindowId>) -> EngineAction {
        if win.is_some() && self.qf_get(win).is_none() {
            self.message = Self::qf_empty_msg(win);
            return EngineAction::None;
        }
        let empty = self.qf_get_mut(win).items.is_empty();
        if empty {
            self.qf_close_panel_window(win);
            let list = self.qf_get_mut(win);
            list.open = false;
            list.has_focus = false;
        } else {
            let lines = Self::qf_format_items(self.qf_get(win).expect("checked above"));
            let scratch_name = if win.is_some() {
                "[Location List]"
            } else {
                "[Quickfix List]"
            };
            self.qf_ensure_panel_window(win, &lines, scratch_name);
            let list = self.qf_get_mut(win);
            list.open = true;
            list.has_focus = true;
        }
        EngineAction::None
    }

    /// Move to the next entry and jump to it.
    ///
    /// #1283 finding: on an empty (or, for a location list, altogether
    /// absent) target, real Neovim's `:cnext`/`:lnext` refuse with an
    /// explicit error (`E42: No Errors` / `E776: No location list`) rather
    /// than silently doing nothing. Before this fix `qf_jump` still declined
    /// to move (there is no item at index 0 of an empty `Vec`), so the
    /// buffer/cursor-only oracle `Case` harness could not see the gap — only
    /// `engine.message` can, which is why this is paired with an
    /// engine-level test rather than relying on a `Case` alone (same
    /// precedent as `ex:cc on empty quickfix list`, #1154).
    pub fn qf_next(&mut self, win: Option<WindowId>) -> EngineAction {
        if self.qf_get(win).is_none_or(|l| l.items.is_empty()) {
            self.message = Self::qf_empty_msg(win);
            return EngineAction::None;
        }
        let list = self.qf_get_mut(win);
        let max = list.items.len().saturating_sub(1);
        list.selected = (list.selected + 1).min(max);
        self.qf_jump(win)
    }

    /// Move to the previous entry and jump to it. See [`Engine::qf_next`]'s
    /// #1283 doc comment — same empty/absent-list refusal.
    pub fn qf_prev(&mut self, win: Option<WindowId>) -> EngineAction {
        if self.qf_get(win).is_none_or(|l| l.items.is_empty()) {
            self.message = Self::qf_empty_msg(win);
            return EngineAction::None;
        }
        let list = self.qf_get_mut(win);
        list.selected = list.selected.saturating_sub(1);
        self.qf_jump(win)
    }

    /// Jump to a specific entry by index (0-based, clamped to the list).
    pub fn qf_go(&mut self, win: Option<WindowId>, idx: usize) -> EngineAction {
        let list = self.qf_get_mut(win);
        list.selected = idx.min(list.items.len().saturating_sub(1));
        self.qf_jump(win)
    }

    /// Jump to the currently selected entry; return focus to the editor.
    ///
    /// The global quickfix list opens each entry in a new tab
    /// (`open_file_in_tab`) since it isn't bound to any one window. A
    /// location list *is* bound to a window, so navigating it must reuse
    /// that window (`open_file_in_window`) — otherwise each jump would open
    /// a fresh tab/window and `self.active_window_id()` would drift away
    /// from the window the list actually belongs to (#1155).
    pub fn qf_jump(&mut self, win: Option<WindowId>) -> EngineAction {
        let item = {
            let list = self.qf_get_mut(win);
            list.items.get(list.selected).cloned()
        };
        if let Some(m) = item {
            self.qf_get_mut(win).has_focus = false;
            let win_id = match win {
                None => {
                    self.open_file_in_tab(&m.file.clone());
                    self.active_window_id()
                }
                Some(w) => {
                    self.open_file_in_window(w, &m.file.clone());
                    w
                }
            };
            self.set_cursor_for_window(win_id, m.line, m.col);
            self.ensure_cursor_visible();
        }
        // #1307: the panel window (if open) stays open after a jump — real
        // Neovim leaves the quickfix window in place after `<CR>` too — but
        // its own cursor should still land on the entry that was just
        // jumped to, so tabbing back into it later starts on the right row.
        self.qf_refresh_panel_if_open(win);
        EngineAction::None
    }

    /// Run a grep search and populate the target list (`:grep`/`:vimgrep`
    /// for the global list, `:lgrep`/`:lvimgrep` for the active window's
    /// location list).
    pub fn qf_run_grep(
        &mut self,
        win: Option<WindowId>,
        pattern: &str,
        cwd: PathBuf,
    ) -> EngineAction {
        if pattern.is_empty() {
            self.message = "Usage: :grep <pattern>".to_string();
            return EngineAction::None;
        }
        let opts = SearchOptions::default();
        match project_search::search_in_project(&cwd, pattern, &opts) {
            Ok(results) => {
                let n = results.len();
                self.qf_set_list(win, results);
                self.message = format!("{} match{}", n, if n == 1 { "" } else { "es" });
            }
            Err(e) => {
                self.message = format!("grep error: {}", e.0);
            }
        }
        EngineAction::None
    }

    /// `:clist`/`:llist` — print every entry in the target list, marking
    /// the currently selected one.
    pub fn qf_list_cmd(&mut self, win: Option<WindowId>) -> EngineAction {
        let list = self.qf_get_mut(win);
        if list.items.is_empty() {
            self.message = Self::qf_empty_msg(win);
            return EngineAction::None;
        }
        let lines: Vec<String> = list
            .items
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let marker = if i == list.selected { ">" } else { " " };
                let file = m.file.display();
                let text = m.line_text.trim();
                if text.is_empty() {
                    format!("{marker}{:>3} {file}:{}:{}", i + 1, m.line + 1, m.col + 1)
                } else {
                    format!(
                        "{marker}{:>3} {file}:{}:{}: {text}",
                        i + 1,
                        m.line + 1,
                        m.col + 1
                    )
                }
            })
            .collect();
        self.message = lines.join("\n");
        EngineAction::None
    }

    /// `:cdo`/`:cfdo`/`:ldo`/`:lfdo` — run an ex command once per entry
    /// (`per_file = false`) or once per distinct file, in first-seen order
    /// (`per_file = true`), jumping to each one first like `:cc`/`:ll`.
    pub fn qf_do(&mut self, win: Option<WindowId>, cmd: &str, per_file: bool) -> EngineAction {
        if cmd.trim().is_empty() {
            self.message = "E471: Argument required".to_string();
            return EngineAction::None;
        }
        // #1308: unlike `:cc`/`:cnext`/`:clist`/`:cfirst`/`:clast`, real
        // Neovim's `:cdo`/`:cfdo`/`:ldo`/`:lfdo` are a silent no-op on an
        // empty list — confirmed against a live `nvim --headless -u NONE`
        // oracle. `pcall` succeeds and `v:errmsg` stays empty, so this must
        // NOT set `Self::qf_empty_msg` like the other list-empty checks do.
        let items = match self.qf_get(win) {
            Some(list) if !list.items.is_empty() => list.items.clone(),
            _ => {
                return EngineAction::None;
            }
        };
        let mut seen_files = std::collections::HashSet::new();
        let mut count = 0usize;
        for (idx, item) in items.iter().enumerate() {
            if per_file && !seen_files.insert(item.file.clone()) {
                continue;
            }
            self.qf_get_mut(win).selected = idx;
            self.qf_jump(win);
            self.execute_command(cmd);
            count += 1;
        }
        self.message = format!(
            "{count} {}",
            if per_file {
                "file(s)"
            } else {
                "quickfix entries"
            }
        );
        EngineAction::None
    }

    /// Route a key press when the target panel has keyboard focus.
    pub fn qf_handle_key(
        &mut self,
        win: Option<WindowId>,
        key_name: &str,
        ctrl: bool,
    ) -> EngineAction {
        match key_name {
            "Escape" | "q" => self.qf_close(win),
            "Return" => {
                self.qf_jump(win);
                EngineAction::None
            }
            "Down" | "j" => {
                let list = self.qf_get_mut(win);
                list.selected = (list.selected + 1).min(list.items.len().saturating_sub(1));
                self.qf_refresh_panel_if_open(win);
                EngineAction::None
            }
            "Up" | "k" => {
                let list = self.qf_get_mut(win);
                list.selected = list.selected.saturating_sub(1);
                self.qf_refresh_panel_if_open(win);
                EngineAction::None
            }
            "n" if ctrl => {
                let list = self.qf_get_mut(win);
                list.selected = (list.selected + 1).min(list.items.len().saturating_sub(1));
                self.qf_refresh_panel_if_open(win);
                EngineAction::None
            }
            "p" if ctrl => {
                let list = self.qf_get_mut(win);
                list.selected = list.selected.saturating_sub(1);
                self.qf_refresh_panel_if_open(win);
                EngineAction::None
            }
            _ => EngineAction::None,
        }
    }
}

#[cfg(test)]
mod grep_scope_tests {
    use super::*;

    /// Marker token shared by files both inside and outside the scoped
    /// folder, so a query matching it alone cannot tell scoped from
    /// unscoped — the per-file suffix below is what a scope must filter
    /// on (mirrors `src/harness.rs`'s
    /// `issue_1418_explorer_context_menu::SCOPED_GREP_COMMON` fixture).
    const MARKER: &str = "zqxw1438grepscope";

    /// Build an engine rooted at a fresh temp workspace containing
    /// `dir/in_scope.txt` (matching `MARKER` + `"in"`) and a sibling
    /// `other/out_of_scope.txt` (matching `MARKER` + `"out"`). Returns
    /// `(engine, dir)`.
    fn engine_with_scoped_and_unscoped_files(tag: &str) -> (Engine, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "vimcode_test_1438_grep_scope_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("dir");
        let other = root.join("other");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(dir.join("in_scope.txt"), format!("{MARKER}in")).unwrap();
        std::fs::write(other.join("out_of_scope.txt"), format!("{MARKER}out")).unwrap();

        let mut engine = Engine::new_for_test();
        engine.cwd = root;
        (engine, dir)
    }

    /// `open_grep_picker_scoped(dir)` + a query matching files both inside
    /// and outside `dir` must return only the match under `dir` (#1438).
    #[test]
    fn open_grep_picker_scoped_only_returns_matches_under_dir() {
        let (mut engine, dir) = engine_with_scoped_and_unscoped_files("scoped_only");

        engine.open_grep_picker_scoped(&dir);
        engine.picker_query = MARKER.to_string();
        engine.picker_filter();

        assert_eq!(
            engine.picker_items.len(),
            1,
            "expected exactly one match under the scoped folder, got: {:?}",
            engine
                .picker_items
                .iter()
                .map(|i| &i.display)
                .collect::<Vec<_>>()
        );
        assert!(
            engine.picker_items[0].display.contains("in_scope.txt"),
            "the scoped folder's own match must appear: {}",
            engine.picker_items[0].display
        );
    }

    /// The scoped picker's title must show the folder being searched
    /// (#1438's "Show the scope in the picker title" requirement).
    #[test]
    fn open_grep_picker_scoped_shows_folder_in_title() {
        let (mut engine, dir) = engine_with_scoped_and_unscoped_files("title");

        engine.open_grep_picker_scoped(&dir);

        assert_eq!(engine.picker_title, "Grep in dir/");
    }

    /// The ordinary (unscoped) Grep picker must keep searching the whole
    /// workspace — matches both inside and outside `dir` must appear.
    #[test]
    fn unscoped_grep_picker_still_returns_matches_from_everywhere() {
        let (mut engine, _dir) = engine_with_scoped_and_unscoped_files("unscoped");

        engine.open_picker(PickerSource::Grep);
        engine.picker_query = MARKER.to_string();
        engine.picker_filter();

        assert_eq!(
            engine.picker_items.len(),
            2,
            "expected matches from both folders when unscoped, got: {:?}",
            engine
                .picker_items
                .iter()
                .map(|i| &i.display)
                .collect::<Vec<_>>()
        );
        assert!(engine
            .picker_items
            .iter()
            .any(|i| i.display.contains("in_scope.txt")));
        assert!(engine
            .picker_items
            .iter()
            .any(|i| i.display.contains("out_of_scope.txt")));
    }

    /// Right-clicking a *file* (not a folder) must scope the grep picker
    /// to the file's parent directory, per #1438's acceptance criteria —
    /// `apply_explorer_context_action`'s `"find_in_folder"` arm resolves
    /// this via `explorer_ctx_action_dir` before calling
    /// `open_grep_picker_scoped`.
    #[test]
    fn find_in_folder_on_a_file_target_scopes_to_its_parent_directory() {
        let (mut engine, dir) = engine_with_scoped_and_unscoped_files("file_target");
        let file_target = dir.join("in_scope.txt");

        let mut host = NoopHost;
        crate::render::apply_explorer_context_action(
            &mut engine,
            "find_in_folder",
            &file_target,
            false,
            &mut host,
        );
        engine.picker_query = MARKER.to_string();
        engine.picker_filter();

        assert_eq!(
            engine.picker_items.len(),
            1,
            "right-clicking a file must scope the search to its parent \
             directory, not the whole workspace: {:?}",
            engine
                .picker_items
                .iter()
                .map(|i| &i.display)
                .collect::<Vec<_>>()
        );
        assert!(engine.picker_items[0].display.contains("in_scope.txt"));
    }

    struct NoopHost;
    impl crate::render::ExplorerContextHost for NoopHost {
        fn open_terminal_at(&mut self, _engine: &mut Engine, _dir: std::path::PathBuf) {}
    }
}
