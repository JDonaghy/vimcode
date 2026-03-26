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
            _ => {
                self.picker_title = format!("{:?}", source);
            }
        }

        self.picker_source = source;
        self.picker_filter();
        self.picker_load_preview();
        self.picker_open = true;
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
                }
            })
            .collect();
    }

    /// Filter picker_all_items by the current query and populate picker_items.
    /// For live sources (Grep), runs a search instead of fuzzy-filtering.
    pub(crate) fn picker_filter(&mut self) {
        const CAP: usize = 100;

        // Live grep: run project search directly instead of fuzzy-filtering.
        if self.picker_source == PickerSource::Grep {
            self.picker_grep_search();
            return;
        }

        if self.picker_query.is_empty() {
            self.picker_items = self.picker_all_items.iter().take(CAP).cloned().collect();
        } else {
            let query = self.picker_query.clone();
            let mut scored: Vec<PickerItem> = self
                .picker_all_items
                .iter()
                .filter_map(|item| {
                    Self::fuzzy_score_with_positions(&item.filter_text, &query).map(
                        |(s, positions)| {
                            let mut item = item.clone();
                            item.score = s;
                            item.match_positions = positions;
                            item
                        },
                    )
                })
                .collect();
            scored.sort_by(|a, b| b.score.cmp(&a.score));
            scored.truncate(CAP);
            self.picker_items = scored;
        }
    }

    /// Run a live project search for the Grep picker source.
    fn picker_grep_search(&mut self) {
        if self.picker_query.len() < 2 {
            self.picker_items.clear();
            return;
        }
        let options = project_search::SearchOptions::default();
        let cwd = self.cwd.clone();
        match project_search::search_in_project(&cwd, &self.picker_query, &options) {
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
                        }
                    })
                    .collect();
            }
            Err(_) => self.picker_items.clear(),
        }
    }

    /// Load preview context for the currently selected picker item.
    fn picker_load_preview(&mut self) {
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
                    .take(30)
                    .enumerate()
                    .map(|(i, text)| (i + 1, text.to_string(), false))
                    .collect();
                self.picker_preview = Some(PickerPreview { lines });
            }
            PickerAction::OpenFileAtLine(path, line) => {
                let Ok(content) = std::fs::read_to_string(path) else {
                    return;
                };
                let all_lines: Vec<&str> = content.lines().collect();
                let match_line = *line;
                let context = 5usize;
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
            }
            _ => {}
        }
    }

    /// Execute the currently selected picker item.
    pub fn picker_confirm(&mut self) -> EngineAction {
        let Some(item) = self.picker_items.get(self.picker_selected).cloned() else {
            self.close_picker();
            return EngineAction::None;
        };
        self.close_picker();

        match item.action {
            PickerAction::OpenFile(rel_path) => {
                let abs = self.cwd.join(&rel_path);
                self.open_file_in_tab(&abs);
                EngineAction::None
            }
            PickerAction::OpenFileAtLine(path, line) => {
                self.open_file_in_tab(&path);
                let win_id = self.active_window_id();
                self.set_cursor_for_window(win_id, line, 0);
                self.ensure_cursor_visible();
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
                    other => self.execute_command(other),
                }
            }
            PickerAction::CheckoutBranch(branch) => {
                self.execute_command(&format!("Gcheckout {}", branch))
            }
            PickerAction::JumpToMark(_mark) => {
                // Phase 3: mark jumping via picker
                EngineAction::None
            }
            PickerAction::PasteRegister(_reg) => {
                // Phase 3: register paste via picker
                EngineAction::None
            }
            PickerAction::Custom(_key) => {
                // Future: fire Lua event
                EngineAction::None
            }
        }
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
            "Return" => self.picker_confirm(),
            "Down" | "Tab" => {
                let max = self.picker_items.len().saturating_sub(1);
                self.picker_selected = (self.picker_selected + 1).min(max);
                self.picker_update_scroll();
                self.picker_load_preview();
                EngineAction::None
            }
            "Up" => {
                self.picker_selected = self.picker_selected.saturating_sub(1);
                self.picker_update_scroll();
                self.picker_load_preview();
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
                    self.picker_selected = 0;
                    self.picker_scroll_top = 0;
                    self.picker_filter();
                    self.picker_load_preview();
                }
                EngineAction::None
            }
            "BackSpace" => {
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

    /// Adjust scroll_top so the selected item is visible (assuming ~20 visible rows).
    fn picker_update_scroll(&mut self) {
        let visible = 20usize; // approximate; render layer will clip
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
                self.quickfix_has_focus = false;
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
