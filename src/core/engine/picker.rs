use super::*;

// ─── Fuzzy score (shared utility, used by tab switcher + unified picker) ──────

impl Engine {
    /// Compute a fuzzy match score of `query` against `text`.
    /// Returns `None` if not all query characters appear as a subsequence.
    pub fn fuzzy_score(text: &str, query: &str) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }
        let text_lc = text.to_lowercase();
        let query_lc = query.to_lowercase();
        let tb = text_lc.as_bytes();
        let qb = query_lc.as_bytes();
        let mut qi = 0usize;
        let mut score = 100i32;
        let mut last_ti = 0usize;
        for ti in 0..tb.len() {
            if qi < qb.len() && tb[ti] == qb[qi] {
                if qi > 0 {
                    score -= (ti - last_ti - 1) as i32; // penalize gaps
                }
                if ti == 0 || matches!(tb[ti - 1], b'/' | b'_' | b'-' | b'.') {
                    score += 5;
                }
                last_ti = ti;
                qi += 1;
            }
        }
        if qi < qb.len() {
            None
        } else {
            Some(score - tb.len() as i32 / 20)
        }
    }
}

// ─── Unified Picker ───────────────────────────────────────────────────────────

impl Engine {
    /// Compute a fuzzy match score and record the byte positions in `text` that matched.
    /// Returns `None` if not all query characters appear as a subsequence.
    pub fn fuzzy_score_with_positions(text: &str, query: &str) -> Option<(i32, Vec<usize>)> {
        if query.is_empty() {
            return Some((0, Vec::new()));
        }
        let text_lc = text.to_lowercase();
        let query_lc = query.to_lowercase();
        let tb = text_lc.as_bytes();
        let qb = query_lc.as_bytes();
        let mut qi = 0usize;
        let mut score = 100i32;
        let mut last_ti = 0usize;
        let mut positions = Vec::with_capacity(qb.len());
        for ti in 0..tb.len() {
            if qi < qb.len() && tb[ti] == qb[qi] {
                if qi > 0 {
                    score -= (ti - last_ti - 1) as i32; // penalize gaps
                }
                if ti == 0 || matches!(tb[ti - 1], b'/' | b'_' | b'-' | b'.') {
                    score += 5;
                }
                // Map back to the original text's byte position.
                // Since to_lowercase() can change byte lengths for non-ASCII,
                // we use char-index mapping for safety, but for ASCII paths
                // the positions are identical.
                positions.push(ti);
                last_ti = ti;
                qi += 1;
            }
        }
        if qi < qb.len() {
            None
        } else {
            Some((score - tb.len() as i32 / 20, positions))
        }
    }

    /// Open the unified picker with a given source.
    pub fn open_picker(&mut self, source: PickerSource) {
        self.picker_query.clear();
        self.picker_selected = 0;
        self.picker_scroll_top = 0;
        self.picker_all_items.clear();
        self.picker_items.clear();
        self.picker_preview = None;
        self.breadcrumb_scoped_parent = None;
        self.picker_history_index = None;
        self.picker_history_typing_buffer.clear();

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
            _ => {
                self.picker_title = format!("{:?}", source);
            }
        }

        self.picker_source = source;
        self.picker_filter();
        self.picker_load_preview();
        self.picker_open = true;
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
            if path.is_dir() {
                // Directory segment: open file picker filtered to that directory
                self.open_picker(PickerSource::Files);
                let rel = path
                    .strip_prefix(&self.cwd)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                self.picker_query = if rel.is_empty() {
                    String::new()
                } else {
                    format!("{}/", rel)
                };
                self.picker_filter();
                self.picker_load_preview();
            } else {
                // File segment (the last path component): open symbol picker
                self.open_picker(PickerSource::CommandCenter);
                self.picker_query = "@".to_string();
                self.picker_filter();
                self.picker_load_preview();
            }
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

    /// Open a scoped picker for the currently selected breadcrumb segment.
    /// Path segments open the file picker for that directory.
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
        self.breadcrumb_scoped_parent = Some(seg.parent_scope.clone());
        self.open_picker(PickerSource::CommandCenter);
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
                "{:<24}:{} [mode: {}] (user remap)",
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
            let mut scored: Vec<PickerItem> = all_items
                .iter()
                .filter_map(|item| {
                    Self::fuzzy_score_with_positions(&item.filter_text, query).map(
                        |(s, positions)| {
                            let mut item = item.clone();
                            item.score = s;
                            item.match_positions = positions;
                            item
                        },
                    )
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
        let scoped = self.breadcrumb_scoped_parent.take();
        let filtered: Vec<lsp::SymbolInfo> = if let Some(ref parent_filter) = scoped {
            symbols
                .into_iter()
                .filter(|sym| sym.container.as_deref() == parent_filter.as_deref())
                .collect()
        } else {
            symbols
        };
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
            out.push(PickerItem {
                filter_text: sym.name.clone(),
                display,
                detail,
                action,
                icon: None,
                score: 0,
                match_positions: Vec::new(),
                depth,
                expandable: has_children,
                expanded: depth == 0, // Top-level items start expanded
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
    fn picker_cc_grep_search(&mut self, query: &str) {
        let options = project_search::SearchOptions::default();
        let cwd = self.cwd.clone();
        match project_search::search_in_project(&cwd, query, &options) {
            Ok(mut results) => {
                results.truncate(200);
                self.picker_items = results
                    .into_iter()
                    .map(|m| {
                        let rel = m
                            .file
                            .strip_prefix(&cwd)
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
        let configured =
            !self.settings.ai_api_key.is_empty() || self.settings.ai_provider == "ollama";

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
                        self.message = "Use :N to go to line N".to_string();
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
                        self.message = "Use :%s/old/new/g for find & replace".to_string();
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
                    self.ai_input = question;
                    self.ai_send_message();
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
                if let Some(text) = Self::clipboard_paste() {
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

// ─── Quickfix ─────────────────────────────────────────────────────────────────

impl Engine {
    /// Open the quickfix panel and give it focus.
    pub fn open_quickfix(&mut self) -> EngineAction {
        if self.quickfix_items.is_empty() {
            self.message = "Quickfix list is empty".to_string();
            return EngineAction::None;
        }
        self.quickfix_open = true;
        self.quickfix_has_focus = true;
        EngineAction::None
    }

    /// Close the quickfix panel.
    pub fn close_quickfix(&mut self) -> EngineAction {
        self.quickfix_open = false;
        self.quickfix_has_focus = false;
        EngineAction::None
    }

    /// Move to the next quickfix item and jump to it.
    pub fn quickfix_next(&mut self) -> EngineAction {
        let max = self.quickfix_items.len().saturating_sub(1);
        self.quickfix_selected = (self.quickfix_selected + 1).min(max);
        self.quickfix_jump()
    }

    /// Move to the previous quickfix item and jump to it.
    pub fn quickfix_prev(&mut self) -> EngineAction {
        self.quickfix_selected = self.quickfix_selected.saturating_sub(1);
        self.quickfix_jump()
    }

    /// Jump to a specific quickfix item by index (0-based).
    pub fn quickfix_go(&mut self, idx: usize) -> EngineAction {
        self.quickfix_selected = idx.min(self.quickfix_items.len().saturating_sub(1));
        self.quickfix_jump()
    }

    /// Jump to the currently selected quickfix item; return focus to the editor.
    pub fn quickfix_jump(&mut self) -> EngineAction {
        if let Some(m) = self.quickfix_items.get(self.quickfix_selected).cloned() {
            self.quickfix_has_focus = false;
            self.open_file_in_tab(&m.file.clone());
            let win_id = self.active_window_id();
            self.set_cursor_for_window(win_id, m.line, m.col);
            self.ensure_cursor_visible();
        }
        EngineAction::None
    }

    /// Run a grep search and populate the quickfix list.
    pub fn run_quickfix_grep(&mut self, pattern: &str, cwd: PathBuf) -> EngineAction {
        if pattern.is_empty() {
            self.message = "Usage: :grep <pattern>".to_string();
            return EngineAction::None;
        }
        let opts = SearchOptions::default();
        match project_search::search_in_project(&cwd, pattern, &opts) {
            Ok(results) => {
                let n = results.len();
                self.quickfix_items = results;
                self.quickfix_selected = 0;
                self.quickfix_open = true;
                // Focus the panel so j/k/Enter drive the result list
                // immediately — matches VimCode's other "open panel"
                // commands (:copen) and the UX convention users expect
                // after an interactive search.
                self.quickfix_has_focus = n > 0;
                self.message = format!("{} match{}", n, if n == 1 { "" } else { "es" });
            }
            Err(e) => {
                self.message = format!("grep error: {}", e.0);
            }
        }
        EngineAction::None
    }

    /// Route a key press when the quickfix panel has focus.
    pub fn handle_quickfix_key(&mut self, key_name: &str, ctrl: bool) -> EngineAction {
        match key_name {
            "Escape" | "q" => self.close_quickfix(),
            "Return" => {
                self.quickfix_jump();
                EngineAction::None
            }
            "Down" | "j" => {
                self.quickfix_selected =
                    (self.quickfix_selected + 1).min(self.quickfix_items.len().saturating_sub(1));
                EngineAction::None
            }
            "Up" | "k" => {
                self.quickfix_selected = self.quickfix_selected.saturating_sub(1);
                EngineAction::None
            }
            "n" if ctrl => {
                self.quickfix_selected =
                    (self.quickfix_selected + 1).min(self.quickfix_items.len().saturating_sub(1));
                EngineAction::None
            }
            "p" if ctrl => {
                self.quickfix_selected = self.quickfix_selected.saturating_sub(1);
                EngineAction::None
            }
            _ => EngineAction::None,
        }
    }
}
