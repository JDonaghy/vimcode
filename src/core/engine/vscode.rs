use super::*;

impl Engine {
    /// True when the editor is configured in VSCode editing mode.
    pub fn is_vscode_mode(&self) -> bool {
        self.settings.editor_mode == EditorMode::Vscode
    }

    /// Human-readable mode string for the status bar.
    pub fn mode_str(&self) -> &'static str {
        if self.is_vscode_mode() {
            return match self.mode {
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock => "SELECT",
                Mode::Command => "COMMAND",
                _ => "EDIT  F1:cmd  Alt-M:vim",
            };
        }
        match self.mode {
            Mode::Normal | Mode::Search => "NORMAL",
            Mode::Command => "COMMAND",
            Mode::Insert => "INSERT",
            Mode::Replace => "REPLACE",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "VISUAL LINE",
            Mode::VisualBlock => "VISUAL BLOCK",
        }
    }

    // ── Menu bar ─────────────────────────────────────────────────────────────

    /// Toggle the VSCode-style menu bar strip on/off; clears any open dropdown.
    #[allow(dead_code)]
    pub fn toggle_menu_bar(&mut self) {
        self.menu_bar_visible = !self.menu_bar_visible;
        self.menu_open_idx = None;
    }

    /// Open the dropdown for top-level menu at `idx` (e.g. 0=File, 1=Edit, …).
    pub fn open_menu(&mut self, idx: usize) {
        self.menu_open_idx = Some(idx);
        self.menu_highlighted_item = None;
    }

    /// Close the currently open dropdown while keeping the bar visible.
    pub fn close_menu(&mut self) {
        self.menu_open_idx = None;
    }

    /// Activate the item at `item_idx` inside top-level menu `menu_idx`.
    /// `action` is the command string to dispatch (looked up from the static menu table
    /// in `render.rs` by the UI layer and passed in here).
    /// Closes the dropdown and returns the `EngineAction` so the UI layer can handle
    /// actions that require platform resources (e.g. `OpenTerminal` needs PTY size).
    pub fn menu_activate_item(
        &mut self,
        menu_idx: usize,
        item_idx: usize,
        action: &str,
    ) -> EngineAction {
        let _ = (menu_idx, item_idx); // indices are for the UI; engine just dispatches
                                      // Ensure Normal mode before executing menu actions so all Vim commands work.
                                      // VSCode mode stays in Insert (its default editing state).
        if !self.is_vscode_mode() {
            self.mode = Mode::Normal;
        }
        let result = if !action.is_empty() {
            self.execute_command(action)
        } else {
            EngineAction::None
        };
        self.close_menu();
        result
    }

    /// Move the keyboard highlight up or down within the open dropdown.
    ///
    /// `is_separator` is provided by the UI layer (derived from MENU_STRUCTURE) and indicates
    /// which items are non-selectable separator lines. The cursor wraps around and skips
    /// separators. `delta` is typically +1 (Down) or -1 (Up).
    pub fn menu_move_selection(&mut self, delta: i32, is_separator: &[bool]) {
        if self.menu_open_idx.is_none() {
            return;
        }
        let non_sep: Vec<usize> = (0..is_separator.len())
            .filter(|&i| !is_separator[i])
            .collect();
        if non_sep.is_empty() {
            return;
        }
        let cur_pos = self
            .menu_highlighted_item
            .and_then(|h| non_sep.iter().position(|&i| i == h));
        let new_pos = match cur_pos {
            None if delta >= 0 => 0,
            None => non_sep.len() - 1,
            Some(pos) => {
                let len = non_sep.len() as i32;
                ((pos as i32 + delta).rem_euclid(len)) as usize
            }
        };
        self.menu_highlighted_item = Some(non_sep[new_pos]);
    }

    /// Activate the currently highlighted item (keyboard Enter).
    ///
    /// Returns `Some((menu_idx, item_idx))` if an item was highlighted and the menu was
    /// closed. The caller must look up the action string from MENU_STRUCTURE and call
    /// `menu_activate_item` to actually dispatch it.
    /// Returns `None` if no menu is open or nothing is highlighted.
    pub fn menu_activate_highlighted(&mut self) -> Option<(usize, usize)> {
        let open_idx = self.menu_open_idx?;
        let item_idx = self.menu_highlighted_item?;
        self.close_menu();
        Some((open_idx, item_idx))
    }

    // ── Selection helpers ────────────────────────────────────────────────────

    /// Clear selection and ensure Insert mode (used in vscode mode).
    pub fn vscode_clear_selection(&mut self) {
        self.visual_anchor = None;
        self.mode = Mode::Insert;
    }

    /// Apply a named movement operation (used by selection extension).
    fn vscode_do_move(&mut self, op: &str) {
        match op {
            "Right" => self.move_right_insert(),
            "Left" => self.move_left(),
            "Up" if self.view().cursor.line > 0 => {
                self.view_mut().cursor.line -= 1;
                self.clamp_cursor_col_insert();
            }
            "Down" => {
                let max_line = self.buffer().len_lines().saturating_sub(1);
                if self.view().cursor.line < max_line {
                    self.view_mut().cursor.line += 1;
                    self.clamp_cursor_col_insert();
                }
            }
            "WordForward" => self.move_word_forward(),
            "WordBackward" => self.move_word_backward(),
            "LineEnd" => {
                let line = self.view().cursor.line;
                let max = self.get_line_len_for_insert(line);
                self.view_mut().cursor.col = max;
            }
            "SmartHome" => self.vscode_smart_home(),
            "DocStart" => {
                self.view_mut().cursor = Cursor { line: 0, col: 0 };
            }
            "DocEnd" => {
                let last = self.buffer().len_lines().saturating_sub(1);
                let last_col = self.get_line_len_for_insert(last);
                self.view_mut().cursor = Cursor {
                    line: last,
                    col: last_col,
                };
            }
            _ => {}
        }
    }

    /// Extend (or start) the visual selection by applying a move, then collapse
    /// if anchor == cursor.
    fn vscode_extend_selection(&mut self, op: &str) {
        if self.visual_anchor.is_none() {
            self.visual_anchor = Some(self.view().cursor);
            self.mode = Mode::Visual;
        }
        self.vscode_do_move(op);
        if self.visual_anchor == Some(self.view().cursor) {
            self.visual_anchor = None;
            self.mode = Mode::Insert;
        }
    }

    /// Delete the current visual selection and restore Insert mode.
    /// Uses exclusive-end semantics: selection is [anchor, cursor) (cursor not included).
    /// Relies on the caller to manage the undo group.
    fn vscode_delete_selection(&mut self, changed: &mut bool) {
        let Some(anchor) = self.visual_anchor else {
            return;
        };
        let cursor = self.view().cursor;

        // Normalize so start <= end.
        let (start, end) = if anchor.line < cursor.line
            || (anchor.line == cursor.line && anchor.col <= cursor.col)
        {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };

        let start_char = self.buffer().line_to_char(start.line) + start.col;
        let end_char = self.buffer().line_to_char(end.line) + end.col; // exclusive

        if end_char > start_char {
            self.delete_with_undo(start_char, end_char);
            self.view_mut().cursor = start;
            *changed = true;
        }

        self.visual_anchor = None;
        self.mode = Mode::Insert;
    }

    /// Delete selected word at every cursor (primary + extras) for Ctrl+D
    /// multi-cursor selections.  All selections are the same length.
    fn vscode_mc_delete_selections(&mut self, changed: &mut bool) {
        let anchor = match self.visual_anchor {
            Some(a) => a,
            None => return,
        };
        let cursor = self.view().cursor;
        let (sel_start, sel_end) = if (anchor.line, anchor.col) <= (cursor.line, cursor.col) {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        let sel_start_ci = self.buffer().line_to_char(sel_start.line) + sel_start.col;
        let sel_end_ci = self.buffer().line_to_char(sel_end.line) + sel_end.col + 1;
        let sel_len = sel_end_ci - sel_start_ci;

        // Collect char indices for start of each selection
        let extras = self.view().extra_cursors.clone();
        let mut char_indices: Vec<usize> = Vec::new();
        char_indices.push(sel_start_ci);
        for ec in &extras {
            let ec_start_col = ec.col + 1 - sel_len;
            let ci = self.buffer().line_to_char(ec.line) + ec_start_col;
            char_indices.push(ci);
        }
        // Sort descending — process rightmost/bottommost first
        char_indices.sort_unstable_by(|a, b| b.cmp(a));

        for &ci in &char_indices {
            self.delete_with_undo(ci, ci + sel_len);
        }

        // Recompute cursor positions after all deletions.
        char_indices.sort_unstable();
        let mut new_cursors: Vec<Cursor> = Vec::new();
        for (i, &ci) in char_indices.iter().enumerate() {
            // After i deletions before this one, offset is i * sel_len
            let adjusted_ci = ci - i * sel_len;
            let line = self.buffer().content.char_to_line(adjusted_ci);
            let line_start = self.buffer().line_to_char(line);
            let col = adjusted_ci - line_start;
            new_cursors.push(Cursor { line, col });
        }
        self.view_mut().cursor = new_cursors[0];
        self.view_mut().extra_cursors = new_cursors[1..].to_vec();
        self.visual_anchor = None;
        self.mode = Mode::Insert;
        *changed = true;
    }

    // ── Movement helpers ─────────────────────────────────────────────────────

    /// Smart Home: move to first non-whitespace; if already there, move to col 0.
    fn vscode_smart_home(&mut self) {
        let line = self.view().cursor.line;
        let first_non_ws = self
            .buffer()
            .content
            .line(line)
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .count();
        let cur_col = self.view().cursor.col;
        self.view_mut().cursor.col = if cur_col == first_non_ws {
            0
        } else {
            first_non_ws
        };
    }

    // ── Clipboard operations ─────────────────────────────────────────────────

    /// Ctrl-C: copy selection (or current line if no selection).
    fn vscode_copy(&mut self) {
        if self.visual_anchor.is_some() {
            if let Some((text, is_linewise)) = self.get_visual_selection_text() {
                self.set_register('+', text.clone(), is_linewise);
                self.set_register('"', text, is_linewise);
            }
            // Keep selection visible after copy.
        } else {
            // No selection: copy current line (including trailing newline).
            let line = self.view().cursor.line;
            let start = self.buffer().line_to_char(line);
            let end = if line + 1 < self.buffer().len_lines() {
                self.buffer().line_to_char(line + 1)
            } else {
                self.buffer().len_chars()
            };
            let text: String = self.buffer().content.slice(start..end).chars().collect();
            let text = if text.ends_with('\n') {
                text
            } else {
                format!("{}\n", text)
            };
            self.set_register('+', text.clone(), true);
            self.set_register('"', text, true);
        }
    }

    /// Ctrl-X: cut selection (or current line if no selection).
    fn vscode_cut(&mut self, changed: &mut bool) {
        if self.visual_anchor.is_some() {
            if let Some((text, is_linewise)) = self.get_visual_selection_text() {
                self.set_register('+', text.clone(), is_linewise);
                self.set_register('"', text, is_linewise);
            }
            self.vscode_delete_selection(changed);
        } else {
            // No selection: cut current line.
            let line = self.view().cursor.line;
            let num_lines = self.buffer().len_lines();
            let start = self.buffer().line_to_char(line);
            let end = if line + 1 < num_lines {
                self.buffer().line_to_char(line + 1)
            } else {
                self.buffer().len_chars()
            };
            let text: String = self.buffer().content.slice(start..end).chars().collect();
            let text = if text.ends_with('\n') {
                text
            } else {
                format!("{}\n", text)
            };
            self.set_register('+', text.clone(), true);
            self.set_register('"', text, true);

            self.delete_with_undo(start, end);

            let new_line = line.min(self.buffer().len_lines().saturating_sub(1));
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = 0;
            *changed = true;
        }
    }

    /// Ctrl-V: paste from `+` register at cursor (replaces selection if any).
    fn vscode_paste(&mut self, changed: &mut bool) {
        if self.visual_anchor.is_some() {
            self.vscode_delete_selection(changed);
        }
        if let Some((text, is_linewise)) = self.get_register_content('+') {
            if is_linewise {
                // Linewise: insert before current line.
                let line = self.view().cursor.line;
                let line_start = self.buffer().line_to_char(line);
                self.insert_with_undo(line_start, &text);
                // Cursor stays at beginning of pasted text.
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = 0;
            } else {
                // Character paste: insert at cursor.
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                self.insert_with_undo(char_idx, &text);
                // Advance cursor past pasted text.
                let lines: Vec<&str> = text.split('\n').collect();
                if lines.len() == 1 {
                    self.view_mut().cursor.col += text.chars().count();
                } else {
                    self.view_mut().cursor.line += lines.len() - 1;
                    self.view_mut().cursor.col = lines.last().unwrap().chars().count();
                }
            }
            *changed = true;
        }
    }

    /// Ctrl-A: select all text.
    fn vscode_select_all(&mut self) {
        self.visual_anchor = Some(Cursor { line: 0, col: 0 });
        self.mode = Mode::Visual;
        let last = self.buffer().len_lines().saturating_sub(1);
        let last_col = self.get_line_len_for_insert(last);
        self.view_mut().cursor = Cursor {
            line: last,
            col: last_col,
        };
    }

    // ── Word-level delete ────────────────────────────────────────────────────

    /// Ctrl-Delete: delete word forward from cursor.
    fn vscode_delete_word_forward(&mut self, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_chars: Vec<char> = self.buffer().content.line(line).chars().collect();
        let line_len = self.get_line_len_for_insert(line);

        if col >= line_len {
            // At EOL: delete newline to join with next line.
            let char_idx = self.buffer().line_to_char(line) + col;
            if char_idx < self.buffer().len_chars() {
                self.delete_with_undo(char_idx, char_idx + 1);
                *changed = true;
            }
            return;
        }

        let is_ws = |c: char| c == ' ' || c == '\t';
        let is_word = |c: char| c.is_alphanumeric() || c == '_';

        let mut end_col = col;
        if is_ws(line_chars[col]) {
            while end_col < line_len && line_chars.get(end_col).is_some_and(|&c| is_ws(c)) {
                end_col += 1;
            }
        } else if is_word(line_chars[col]) {
            while end_col < line_len && line_chars.get(end_col).is_some_and(|&c| is_word(c)) {
                end_col += 1;
            }
        } else {
            end_col += 1;
        }

        let start_char = self.buffer().line_to_char(line) + col;
        let end_char = self.buffer().line_to_char(line) + end_col;
        if start_char < end_char {
            self.delete_with_undo(start_char, end_char);
            *changed = true;
        }
    }

    /// Ctrl-Backspace: delete word backward from cursor.
    fn vscode_delete_word_backward(&mut self, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;

        if col == 0 {
            if line > 0 {
                // Join with previous line.
                let char_idx = self.buffer().line_to_char(line);
                if char_idx > 0 {
                    self.delete_with_undo(char_idx - 1, char_idx);
                    let new_col = self.get_line_len_for_insert(line - 1);
                    self.view_mut().cursor.line -= 1;
                    self.view_mut().cursor.col = new_col;
                    *changed = true;
                }
            }
            return;
        }

        let line_chars: Vec<char> = self.buffer().content.line(line).chars().collect();
        let is_ws = |c: char| c == ' ' || c == '\t';
        let is_word = |c: char| c.is_alphanumeric() || c == '_';

        let mut start_col = col;
        // Skip leading whitespace.
        while start_col > 0 && line_chars.get(start_col - 1).is_some_and(|&c| is_ws(c)) {
            start_col -= 1;
        }
        if start_col > 0 {
            if line_chars.get(start_col - 1).is_some_and(|&c| is_word(c)) {
                while start_col > 0 && line_chars.get(start_col - 1).is_some_and(|&c| is_word(c)) {
                    start_col -= 1;
                }
            } else {
                start_col -= 1;
            }
        }

        let start_char = self.buffer().line_to_char(line) + start_col;
        let end_char = self.buffer().line_to_char(line) + col;
        if start_char < end_char {
            self.delete_with_undo(start_char, end_char);
            self.view_mut().cursor.col = start_col;
            *changed = true;
        }
    }

    // ── Line operations (VSCode mode) ──────────────────────────────────────

    /// Alt+Up: move current line (or selected lines) up by one.
    fn vscode_move_line_up(&mut self, changed: &mut bool) {
        let (start_line, end_line) = self.vscode_affected_lines();
        if start_line == 0 {
            return;
        }
        self.start_undo_group();
        // Grab the line above and remove it
        let above = start_line - 1;
        let above_start = self.buffer().line_to_char(above);
        let above_end = self.buffer().line_to_char(above + 1);
        let above_text: String = self
            .buffer()
            .content
            .slice(above_start..above_end)
            .chars()
            .collect();
        self.delete_with_undo(above_start, above_end);
        // Insert it after the (now shifted) block
        let new_end_line = end_line - 1; // shifted because we deleted a line above
        let insert_pos = if new_end_line + 1 < self.buffer().len_lines() {
            self.buffer().line_to_char(new_end_line + 1)
        } else {
            let pos = self.buffer().len_chars();
            // Ensure there's a newline before we append
            if pos > 0 {
                let last_char_idx = pos - 1;
                let last_ch: char = self.buffer().content.char(last_char_idx);
                if last_ch != '\n' {
                    self.insert_with_undo(pos, "\n");
                }
            }
            self.buffer().len_chars()
        };
        self.insert_with_undo(insert_pos, &above_text);
        self.finish_undo_group();
        // Move cursor and visual anchor up by one
        self.view_mut().cursor.line = self.view().cursor.line.saturating_sub(1);
        if let Some(ref mut anc) = self.visual_anchor {
            anc.line = anc.line.saturating_sub(1);
        }
        *changed = true;
    }

    /// Alt+Down: move current line (or selected lines) down by one.
    fn vscode_move_line_down(&mut self, changed: &mut bool) {
        let (start_line, end_line) = self.vscode_affected_lines();
        let num_lines = self.buffer().len_lines();
        if end_line + 1 >= num_lines {
            return;
        }
        self.start_undo_group();
        // Grab the line below the block and remove it
        let below = end_line + 1;
        let below_start = self.buffer().line_to_char(below);
        let below_end = if below + 1 < self.buffer().len_lines() {
            self.buffer().line_to_char(below + 1)
        } else {
            self.buffer().len_chars()
        };
        let below_text: String = self
            .buffer()
            .content
            .slice(below_start..below_end)
            .chars()
            .collect();
        let below_text = if below_text.ends_with('\n') {
            below_text
        } else {
            format!("{}\n", below_text)
        };
        self.delete_with_undo(below_start, below_end);
        // Insert it before start_line
        let insert_pos = self.buffer().line_to_char(start_line);
        self.insert_with_undo(insert_pos, &below_text);
        self.finish_undo_group();
        // Move cursor and visual anchor down by one
        let max_line = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = (self.view().cursor.line + 1).min(max_line);
        if let Some(ref mut anc) = self.visual_anchor {
            anc.line = (anc.line + 1).min(max_line);
        }
        *changed = true;
    }

    /// Ctrl+Shift+K: delete current line.
    fn vscode_delete_line(&mut self, changed: &mut bool) {
        let num_lines = self.buffer().len_lines();
        if num_lines == 0 {
            return;
        }
        let line = self.view().cursor.line;
        let start = self.buffer().line_to_char(line);
        let end = if line + 1 < num_lines {
            self.buffer().line_to_char(line + 1)
        } else {
            self.buffer().len_chars()
        };
        if start < end {
            // If last line and there's a preceding newline, remove it too
            let actual_start = if line + 1 >= num_lines && start > 0 {
                start - 1
            } else {
                start
            };
            self.delete_with_undo(actual_start, end);
            let new_line = line.min(self.buffer().len_lines().saturating_sub(1));
            self.view_mut().cursor.line = new_line;
            self.clamp_cursor_col_insert();
            *changed = true;
        }
        self.visual_anchor = None;
    }

    /// Ctrl+Enter: insert blank line below, cursor stays on current line.
    fn vscode_insert_line_below(&mut self, changed: &mut bool) {
        let line = self.view().cursor.line;
        let insert_pos = if line + 1 < self.buffer().len_lines() {
            self.buffer().line_to_char(line + 1)
        } else {
            let pos = self.buffer().len_chars();
            // Ensure newline at end
            if pos > 0 {
                let last_ch: char = self.buffer().content.char(pos - 1);
                if last_ch != '\n' {
                    self.insert_with_undo(pos, "\n");
                }
            }
            self.buffer().len_chars()
        };
        self.insert_with_undo(insert_pos, "\n");
        *changed = true;
    }

    /// Ctrl+Shift+Enter: insert blank line above, cursor stays on current line.
    fn vscode_insert_line_above(&mut self, changed: &mut bool) {
        let line = self.view().cursor.line;
        let insert_pos = self.buffer().line_to_char(line);
        self.insert_with_undo(insert_pos, "\n");
        // Cursor is now pushed down by one, keep it there (on the original line).
        self.view_mut().cursor.line += 1;
        *changed = true;
    }

    /// Ctrl+L: select entire current line; repeat extends selection by one line.
    /// First press: anchor at line start, cursor at start of next line (selects
    /// the whole line including newline, matching VSCode).  Repeat: extend cursor
    /// down by one more line.
    fn vscode_select_line(&mut self) {
        let line = self.view().cursor.line;
        let max_line = self.buffer().len_lines().saturating_sub(1);
        if self.visual_anchor.is_some() {
            // Extend: move cursor to start of the line after the current one.
            let next = (line + 1).min(max_line);
            self.view_mut().cursor = Cursor { line: next, col: 0 };
        } else {
            // First press: anchor at line start, cursor at start of next line.
            self.visual_anchor = Some(Cursor { line, col: 0 });
            self.mode = Mode::Visual;
            if line < max_line {
                self.view_mut().cursor = Cursor {
                    line: line + 1,
                    col: 0,
                };
            } else {
                // Last line: select to end of line
                let end_col = self.get_line_len_for_insert(line);
                self.view_mut().cursor = Cursor { line, col: end_col };
            }
        }
    }

    /// Helper: get the range of lines affected by current selection or cursor.
    fn vscode_affected_lines(&self) -> (usize, usize) {
        if let Some(anchor) = self.visual_anchor {
            let cursor = self.view().cursor;
            let start = anchor.line.min(cursor.line);
            let end = anchor.line.max(cursor.line);
            (start, end)
        } else {
            let line = self.view().cursor.line;
            (line, line)
        }
    }

    // ── Multi-cursor (VSCode mode) ──────────────────────────────────────────

    /// Ctrl+D: first press selects word under cursor; subsequent presses add
    /// the next occurrence as an extra cursor.
    fn vscode_ctrl_d(&mut self) {
        if self.visual_anchor.is_none() {
            // First press: select the word under cursor (no trailing space/punctuation).
            let cursor = self.view().cursor;
            let line_chars: Vec<char> = self.buffer().content.line(cursor.line).chars().collect();
            let col = cursor.col;
            if col >= line_chars.len() {
                return;
            }
            let is_w = |c: &char| c.is_alphanumeric() || *c == '_';
            if !is_w(&line_chars[col]) {
                return;
            }
            let mut ws = col;
            while ws > 0 && line_chars.get(ws - 1).is_some_and(is_w) {
                ws -= 1;
            }
            let mut we = col;
            while we < line_chars.len() && line_chars.get(we).is_some_and(is_w) {
                we += 1;
            }
            if we > ws {
                self.visual_anchor = Some(Cursor {
                    line: cursor.line,
                    col: ws,
                });
                self.mode = Mode::Visual;
                // Cursor at last char of word (inclusive), not one past end
                self.view_mut().cursor.col = we - 1;
            }
        } else {
            // Subsequent press: extract selected text and find next occurrence.
            let anchor = match self.visual_anchor {
                Some(a) => a,
                None => return,
            };
            let cursor = self.view().cursor;
            let (start, end) = if (anchor.line, anchor.col) <= (cursor.line, cursor.col) {
                (anchor, cursor)
            } else {
                (cursor, anchor)
            };
            // Extract selected text (inclusive of end position)
            let start_idx = self.buffer().line_to_char(start.line) + start.col;
            let end_idx = self.buffer().line_to_char(end.line) + end.col + 1;
            let selected: String = self
                .buffer()
                .content
                .slice(start_idx..end_idx)
                .chars()
                .collect();
            if selected.is_empty() {
                return;
            }
            let word_len = selected.chars().count();
            // Search after the last extra cursor.  Extra cursors point at the
            // END of their match, so subtract word_len to get the match start
            // for find_next_occurrence's "after" parameter.
            let search_after = if let Some(last_ec) = self.view().extra_cursors.last().copied() {
                Cursor {
                    line: last_ec.line,
                    col: last_ec.col.saturating_sub(word_len.saturating_sub(1)),
                }
            } else {
                // First subsequent press: search after primary selection start
                start
            };
            if let Some(match_start) = self.find_next_occurrence(&selected, search_after, true) {
                // Place extra cursor at END of match (same position as primary cursor)
                let match_end = Cursor {
                    line: match_start.line,
                    col: match_start.col + word_len - 1,
                };
                let is_primary = match_end == *self.cursor();
                let already_extra = self.view().extra_cursors.contains(&match_end);
                if !is_primary && !already_extra {
                    self.view_mut().extra_cursors.push(match_end);
                    let total = self.view().extra_cursors.len() + 1;
                    self.message = format!("{} cursors ('{}')", total, selected);
                } else {
                    self.message = format!("No more occurrences of '{}'", selected);
                }
            } else {
                self.message = format!("No more occurrences of '{}'", selected);
            }
        }
    }

    /// VSCode Ctrl+Shift+L → select all occurrences of word under cursor.
    /// Like vscode_ctrl_d but selects all at once: visual_anchor at word start,
    /// primary cursor at word end, extra cursors at end of every other occurrence.
    pub fn vscode_select_all_occurrences(&mut self) {
        let cursor = self.view().cursor;
        let line_chars: Vec<char> = self.buffer().content.line(cursor.line).chars().collect();
        let col = cursor.col;
        if col >= line_chars.len() {
            return;
        }
        let is_w = |c: &char| c.is_alphanumeric() || *c == '_';
        if !is_w(&line_chars[col]) {
            return;
        }
        let mut ws = col;
        while ws > 0 && line_chars.get(ws - 1).is_some_and(is_w) {
            ws -= 1;
        }
        let mut we = col;
        while we < line_chars.len() && line_chars.get(we).is_some_and(is_w) {
            we += 1;
        }
        if we <= ws {
            return;
        }
        let word: String = line_chars[ws..we].iter().collect();
        let word_len = word.chars().count();

        // Set primary selection on current word
        self.visual_anchor = Some(Cursor {
            line: cursor.line,
            col: ws,
        });
        self.mode = Mode::Visual;
        self.view_mut().cursor.col = we - 1;

        // Find all occurrences and place extra cursors at word END
        let all_starts = self.collect_all_occurrences(&word, true);
        let primary_start = Cursor {
            line: cursor.line,
            col: ws,
        };
        let extras: Vec<Cursor> = all_starts
            .into_iter()
            .filter(|&c| c != primary_start)
            .map(|c| Cursor {
                line: c.line,
                col: c.col + word_len - 1,
            })
            .collect();
        let n = extras.len();
        self.view_mut().extra_cursors = extras;
        self.message = format!("{} cursors (all occurrences of '{}')", n + 1, word);
    }

    /// VSCode Ctrl+] → indent current line or selection.
    fn vscode_indent(&mut self, changed: &mut bool) {
        if !self.view().extra_cursors.is_empty() {
            // Collect all unique lines from primary + extra cursors
            let mut lines: Vec<usize> = vec![self.view().cursor.line];
            for ec in &self.view().extra_cursors {
                if !lines.contains(&ec.line) {
                    lines.push(ec.line);
                }
            }
            lines.sort_unstable();
            for &line in &lines {
                self.indent_lines(line, 1, changed);
            }
            // Adjust cursor columns for indent
            let indent_size = if self.settings.expand_tab {
                self.effective_shift_width()
            } else {
                1
            };
            self.view_mut().cursor.col += indent_size;
            for ec in self.view_mut().extra_cursors.iter_mut() {
                ec.col += indent_size;
            }
        } else {
            let (start_line, end_line) = self.vscode_affected_lines();
            let count = end_line - start_line + 1;
            self.indent_lines(start_line, count, changed);
        }
    }

    /// VSCode Ctrl+[ → outdent current line or selection.
    fn vscode_outdent(&mut self, changed: &mut bool) {
        if !self.view().extra_cursors.is_empty() {
            let mut lines: Vec<usize> = vec![self.view().cursor.line];
            for ec in &self.view().extra_cursors {
                if !lines.contains(&ec.line) {
                    lines.push(ec.line);
                }
            }
            lines.sort_unstable();
            // Check indent size before dedenting
            let indent_size = if self.settings.expand_tab {
                self.effective_shift_width()
            } else {
                1
            };
            for &line in &lines {
                self.dedent_lines(line, 1, changed);
            }
            // Adjust cursor columns
            self.view_mut().cursor.col = self.view().cursor.col.saturating_sub(indent_size);
            for ec in self.view_mut().extra_cursors.iter_mut() {
                ec.col = ec.col.saturating_sub(indent_size);
            }
        } else {
            let (start_line, end_line) = self.vscode_affected_lines();
            let count = end_line - start_line + 1;
            self.dedent_lines(start_line, count, changed);
        }
    }

    // ── Panel + navigation (VSCode mode) ────────────────────────────────────

    /// Ctrl+G: go to line number. Enters command mode pre-filled with ":".
    fn vscode_goto_line(&mut self) -> EngineAction {
        self.mode = Mode::Command;
        self.command_buffer.clear();
        self.message.clear();
        EngineAction::None
    }

    /// Finish any in-progress VSCode typing undo group.  Called before
    /// non-character actions so that contiguous character insertions coalesce
    /// into a single undo entry while commands, cursor jumps, etc. get their
    /// own group.
    pub(crate) fn vscode_break_undo_group(&mut self) {
        if self.vscode_undo_group_open {
            self.finish_undo_group();
            self.vscode_undo_group_open = false;
        }
    }

    // ── Ctrl+K chord (VSCode mode) ──────────────────────────────────────────

    /// Process the second key of a Ctrl+K chord. Returns true if handled.
    fn vscode_ctrl_k_dispatch(&mut self, key_name: &str, ctrl: bool, changed: &mut bool) -> bool {
        self.vscode_pending_ctrl_k = false;
        self.message.clear();
        if ctrl {
            match key_name {
                // Ctrl+K, Ctrl+C → add line comment
                "c" => {
                    let (start_line, end_line) = self.vscode_affected_lines();
                    self.toggle_comment(start_line + 1, end_line + 1);
                    *changed = true;
                    true
                }
                // Ctrl+K, Ctrl+U → remove line comment
                "u" => {
                    let (start_line, end_line) = self.vscode_affected_lines();
                    self.toggle_comment(start_line + 1, end_line + 1);
                    *changed = true;
                    true
                }
                // Ctrl+K, Ctrl+W → close all editors in group
                "w" => {
                    // Close all tabs in current group
                    let group = self.active_group;
                    let tab_count = self.editor_groups.get(&group).map_or(0, |g| g.tabs.len());
                    for _ in 0..tab_count {
                        self.close_tab();
                    }
                    true
                }
                // Ctrl+K, Ctrl+F → format document
                "f" => {
                    self.lsp_format_current();
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    // ── Main VSCode key dispatcher ───────────────────────────────────────────

    pub(crate) fn handle_vscode_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> EngineAction {
        // If the command bar is open (e.g. user pressed F1), delegate directly
        // to the command handler — no undo group needed.
        if self.mode == Mode::Command {
            return self.handle_command_key(key_name, unicode, ctrl);
        }

        // User-defined keymaps (`:map n <key> :command`) work in VSCode mode too.
        // Mode "n" maps are matched since VSCode has no modal distinction.
        if !self.user_keymaps.is_empty() {
            let mut km_changed = false;
            if let Some(km_action) = self.try_user_keymap(key_name, unicode, ctrl, &mut km_changed)
            {
                if km_changed {
                    self.set_dirty(true);
                }
                self.fire_cursor_move_hook();
                return km_action;
            }
        }

        let mut changed = false;

        // Determine whether this keystroke is a plain character insertion.
        // Plain chars reuse / extend the current undo group so consecutive
        // typing coalesces into a single undo entry.  Everything else
        // (Ctrl+* commands, cursor movement, Backspace, Return, etc.)
        // breaks the undo group first, then starts a fresh one.
        let is_plain_char = !ctrl
            && !key_name.starts_with("Shift_")
            && !key_name.starts_with("Alt_")
            && unicode.is_some()
            && !matches!(
                key_name,
                "Escape"
                    | "Right"
                    | "Left"
                    | "Up"
                    | "Down"
                    | "Home"
                    | "End"
                    | "Page_Up"
                    | "Page_Down"
                    | "BackSpace"
                    | "Delete"
                    | "Return"
                    | "Tab"
                    | "ISO_Left_Tab"
                    | "F1"
                    | "F10"
            );

        if is_plain_char {
            // Continue the existing undo group (or open a fresh one for the
            // first character in a new typing burst).  If the cursor moved
            // since the last character (e.g. mouse click), break the group
            // so the new burst becomes a separate undo entry.
            if self.vscode_undo_group_open {
                let cur = (self.view().cursor.line, self.view().cursor.col);
                if cur != self.vscode_undo_cursor {
                    self.vscode_break_undo_group();
                }
            }
            if !self.vscode_undo_group_open {
                self.start_undo_group();
                self.vscode_undo_group_open = true;
            }
        } else {
            // Non-character action: close any in-progress typing group,
            // then open a one-shot group for this action.
            self.vscode_break_undo_group();
            self.start_undo_group();
        }

        // ── Ctrl+K chord: second key dispatch ────────────────────────────
        if self.vscode_pending_ctrl_k && self.vscode_ctrl_k_dispatch(key_name, ctrl, &mut changed) {
            if changed {
                self.finish_undo_group();
                self.set_dirty(true);
                self.update_syntax();
                let active_id = self.active_buffer_id();
                self.lsp_dirty_buffers.insert(active_id, true);
                self.swap_mark_dirty();
            }
            self.ensure_cursor_visible();
            self.fire_cursor_move_hook();
            return EngineAction::None;
        }

        // ── Alt-encoded keys (sent from backends when in VSCode mode) ────
        if key_name.starts_with("Alt_") {
            match key_name {
                "Alt_Up" => self.vscode_move_line_up(&mut changed),
                "Alt_Down" => self.vscode_move_line_down(&mut changed),
                "Alt_Shift_Up" => {
                    let col = self.view().cursor.col;
                    let min_line = self
                        .view()
                        .extra_cursors
                        .iter()
                        .map(|c| c.line)
                        .min()
                        .unwrap_or(self.view().cursor.line)
                        .min(self.view().cursor.line);
                    if min_line > 0 {
                        self.add_cursor_at_pos(min_line - 1, col);
                        let n = self.view().extra_cursors.len() + 1;
                        self.message = format!("{n} cursors");
                    }
                }
                "Alt_Shift_Down" => {
                    let col = self.view().cursor.col;
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    let max_cursor_line = self
                        .view()
                        .extra_cursors
                        .iter()
                        .map(|c| c.line)
                        .max()
                        .unwrap_or(self.view().cursor.line)
                        .max(self.view().cursor.line);
                    if max_cursor_line < max_line {
                        self.add_cursor_at_pos(max_cursor_line + 1, col);
                        let n = self.view().extra_cursors.len() + 1;
                        self.message = format!("{n} cursors");
                    }
                }
                "Alt_z" => {
                    self.settings.wrap = !self.settings.wrap;
                    self.message = format!(
                        "Word wrap {}",
                        if self.settings.wrap { "on" } else { "off" }
                    );
                }
                _ => {}
            }
            if changed {
                self.finish_undo_group();
                self.set_dirty(true);
                self.update_syntax();
                let active_id = self.active_buffer_id();
                if self.preview_buffer_id == Some(active_id) {
                    self.promote_preview(active_id);
                }
                self.lsp_dirty_buffers.insert(active_id, true);
                self.swap_mark_dirty();
                if !self.search_matches.is_empty() {
                    self.run_search();
                }
            }
            self.ensure_cursor_visible();
            self.sync_scroll_binds();
            self.update_bracket_match();
            self.fire_cursor_move_hook();
            return EngineAction::None;
        }

        if ctrl {
            match key_name {
                "z" => {
                    self.vscode_clear_selection();
                    self.undo();
                    self.refresh_md_previews();
                }
                "y" => {
                    self.vscode_clear_selection();
                    self.redo();
                    self.refresh_md_previews();
                }
                "a" => {
                    self.vscode_select_all();
                }
                "c" => {
                    self.vscode_copy();
                }
                "x" => {
                    self.vscode_cut(&mut changed);
                }
                "v" => {
                    self.vscode_paste(&mut changed);
                }
                "Right" => {
                    self.vscode_clear_selection();
                    self.move_word_forward();
                }
                "Left" => {
                    self.vscode_clear_selection();
                    self.move_word_backward();
                }
                "Home" => {
                    self.vscode_clear_selection();
                    self.view_mut().cursor = Cursor { line: 0, col: 0 };
                }
                "End" => {
                    self.vscode_clear_selection();
                    self.vscode_do_move("DocEnd");
                }
                "Shift_Right" => {
                    self.vscode_extend_selection("WordForward");
                }
                "Shift_Left" => {
                    self.vscode_extend_selection("WordBackward");
                }
                "Shift_Home" => {
                    self.vscode_extend_selection("DocStart");
                }
                "Shift_End" => {
                    self.vscode_extend_selection("DocEnd");
                }
                "Delete" => {
                    self.vscode_delete_word_forward(&mut changed);
                }
                "BackSpace" => {
                    self.vscode_delete_word_backward(&mut changed);
                }
                "q" => {
                    // Ctrl+Q: quit (like VSCode)
                    self.finish_undo_group();
                    return self.execute_command("quit_menu");
                }
                "/" | "slash" => {
                    // Toggle line comment using unified comment system
                    let (start_line, end_line) = if self.visual_anchor.is_some() {
                        match self.get_visual_selection_range() {
                            Some((start, end)) => (start.line, end.line),
                            None => {
                                let l = self.view().cursor.line;
                                (l, l)
                            }
                        }
                    } else {
                        let l = self.view().cursor.line;
                        (l, l)
                    };
                    self.toggle_comment(start_line + 1, end_line + 1);
                    changed = true;
                }
                // Phase 1: Ctrl+Shift+K → delete line
                "K" => {
                    self.vscode_delete_line(&mut changed);
                }
                // Phase 1: Ctrl+Enter → insert blank line below
                "Return" => {
                    self.vscode_insert_line_below(&mut changed);
                }
                // Phase 1: Ctrl+Shift+Enter → insert blank line above
                "Shift_Return" => {
                    self.vscode_insert_line_above(&mut changed);
                }
                // Phase 1: Ctrl+L → select line
                "l" => {
                    self.vscode_select_line();
                }
                // Phase 2: Ctrl+D → select word / add next occurrence
                "d" => {
                    self.vscode_ctrl_d();
                }
                // Phase 2: Ctrl+Shift+L → select all occurrences
                "L" => {
                    self.vscode_select_all_occurrences();
                }
                // Phase 2: Ctrl+] → indent
                "bracketright" | "]" => {
                    self.vscode_indent(&mut changed);
                }
                // Phase 2: Ctrl+[ → outdent
                "bracketleft" | "[" => {
                    self.vscode_outdent(&mut changed);
                }
                // Phase 3: Ctrl+G → go to line
                "g" => {
                    self.finish_undo_group();
                    return self.vscode_goto_line();
                }
                // Phase 3: Ctrl+P → quick file open (unified picker)
                "p" => {
                    self.open_picker(PickerSource::Files);
                }
                // Phase 3: Ctrl+Shift+P → command palette
                "P" => {
                    self.open_picker(PickerSource::Commands);
                }
                // Find/Replace: Ctrl+F → find, Ctrl+H → find & replace
                "f" => {
                    self.open_find_replace();
                }
                "h" => {
                    self.open_find_replace();
                    self.find_replace_show_replace = true;
                }
                // Phase 3: Ctrl+B → toggle sidebar
                "b" => {
                    self.finish_undo_group();
                    return EngineAction::ToggleSidebar;
                }
                // Phase 3: Ctrl+J → toggle bottom panel (terminal)
                "j" => {
                    if self.terminal_panes.is_empty() {
                        return EngineAction::OpenTerminal;
                    }
                    self.toggle_terminal();
                }
                // Phase 3: Ctrl+` (backtick) → toggle terminal
                "grave" | "`" => {
                    if self.terminal_panes.is_empty() {
                        return EngineAction::OpenTerminal;
                    }
                    self.toggle_terminal();
                }
                // Phase 3: Ctrl+, → open settings
                "comma" | "," => {
                    self.settings_has_focus = true;
                }
                // Phase 3: Ctrl+K → chord prefix
                "k" => {
                    self.vscode_pending_ctrl_k = true;
                    self.message = "Ctrl+K ...".to_string();
                }
                // Phase 4: Ctrl+Shift+[ → fold region (progressive — repeated
                // presses fold increasingly larger parent blocks)
                "Shift_bracketleft" => {
                    self.cmd_fold_close_progressive();
                }
                // Phase 4: Ctrl+Shift+] → unfold region (progressive — repeated
                // presses unfold nested folds)
                "Shift_bracketright" => {
                    self.cmd_fold_open_progressive();
                }
                _ => {}
            }
        } else if key_name.starts_with("Shift_") {
            // Shift+Arrow selection extension (no ctrl).
            match key_name {
                "Shift_Right" => self.vscode_extend_selection("Right"),
                "Shift_Left" => self.vscode_extend_selection("Left"),
                "Shift_Up" => self.vscode_extend_selection("Up"),
                "Shift_Down" => self.vscode_extend_selection("Down"),
                "Shift_Home" => self.vscode_extend_selection("SmartHome"),
                "Shift_End" => self.vscode_extend_selection("LineEnd"),
                _ => {}
            }
        } else {
            // Regular (non-ctrl, non-shift) key.
            match key_name {
                "Escape" => {
                    // Dismiss completion popup if open
                    if self.completion_idx.is_some() {
                        self.dismiss_completion();
                    } else if self.lsp_completion_active {
                        self.lsp_completion_active = false;
                    } else if !self.view().extra_cursors.is_empty() {
                        // Clear multi-cursors
                        self.view_mut().extra_cursors.clear();
                    } else {
                        self.vscode_clear_selection();
                    }
                }
                "Right" => {
                    self.vscode_clear_selection();
                    self.move_right_insert();
                }
                "Left" => {
                    self.vscode_clear_selection();
                    self.move_left();
                }
                "Up" => {
                    self.vscode_clear_selection();
                    self.vscode_do_move("Up");
                }
                "Down" => {
                    self.vscode_clear_selection();
                    self.vscode_do_move("Down");
                }
                "Home" => {
                    self.vscode_clear_selection();
                    self.vscode_smart_home();
                }
                "End" => {
                    self.vscode_clear_selection();
                    let line = self.view().cursor.line;
                    let max = self.get_line_len_for_insert(line);
                    self.view_mut().cursor.col = max;
                }
                "Page_Up" => {
                    self.vscode_clear_selection();
                    let amount = self.viewport_lines();
                    self.view_mut().cursor.line = self.view().cursor.line.saturating_sub(amount);
                    self.clamp_cursor_col_insert();
                }
                "Page_Down" => {
                    self.vscode_clear_selection();
                    let amount = self.viewport_lines();
                    let max_line = self.buffer().len_lines().saturating_sub(1);
                    self.view_mut().cursor.line = (self.view().cursor.line + amount).min(max_line);
                    self.clamp_cursor_col_insert();
                }
                "F1" => {
                    // F1 opens the command palette (matches VSCode).
                    self.open_picker(PickerSource::Commands);
                }
                "F10" => {
                    // Toggle menu bar visibility
                    self.toggle_menu_bar();
                }
                "BackSpace" => {
                    if self.visual_anchor.is_some() && !self.view().extra_cursors.is_empty() {
                        // Multi-cursor with selection (Ctrl+D): delete selected word
                        // at every cursor position.
                        self.vscode_mc_delete_selections(&mut changed);
                    } else if self.visual_anchor.is_some() {
                        self.vscode_delete_selection(&mut changed);
                    } else if !self.view().extra_cursors.is_empty() {
                        // Multi-cursor backspace: delete one char before each cursor.
                        // Use char indices for correct same-line handling.
                        let primary = *self.cursor();
                        let extras = self.view().extra_cursors.clone();
                        let primary_ci = self.buffer().line_to_char(primary.line) + primary.col;
                        let mut char_indices: Vec<usize> = Vec::new();
                        char_indices.push(primary_ci);
                        for ec in &extras {
                            char_indices.push(self.buffer().line_to_char(ec.line) + ec.col);
                        }
                        // Filter out col==0 cursors and sort descending
                        char_indices.retain(|&ci| ci > 0);
                        char_indices.sort_unstable_by(|a, b| b.cmp(a));
                        for &ci in &char_indices {
                            self.delete_with_undo(ci - 1, ci);
                        }
                        // Recompute positions
                        char_indices.sort_unstable();
                        let mut new_extras: Vec<Cursor> = Vec::new();
                        for (i, &ci) in char_indices.iter().enumerate() {
                            let adjusted = ci - 1 - i; // each prior deletion shifts by 1
                            let line = self.buffer().content.char_to_line(adjusted);
                            let line_start = self.buffer().line_to_char(line);
                            let col = adjusted - line_start;
                            let cur = Cursor { line, col };
                            if ci == primary_ci {
                                self.view_mut().cursor = cur;
                            } else {
                                new_extras.push(cur);
                            }
                        }
                        self.view_mut().extra_cursors = new_extras;
                        changed = true;
                    } else {
                        let line = self.view().cursor.line;
                        let col = self.view().cursor.col;
                        let char_idx = self.buffer().line_to_char(line) + col;
                        if col > 0 {
                            // Auto-pair backspace: delete both opener and closer
                            let prev_char = self.buffer().content.char(char_idx - 1);
                            let next_char_matches = if self.settings.auto_pairs
                                && char_idx < self.buffer().len_chars()
                            {
                                let next = self.buffer().content.char(char_idx);
                                auto_pair_closer(prev_char) == Some(next)
                            } else {
                                false
                            };
                            if next_char_matches {
                                self.delete_with_undo(char_idx - 1, char_idx + 1);
                            } else {
                                self.delete_with_undo(char_idx - 1, char_idx);
                            }
                            self.view_mut().cursor.col -= 1;
                            changed = true;
                        } else if line > 0 {
                            let prev_len = self.buffer().line_len_chars(line - 1);
                            let new_col = if prev_len > 0 { prev_len - 1 } else { 0 };
                            self.delete_with_undo(char_idx - 1, char_idx);
                            self.view_mut().cursor.line -= 1;
                            self.view_mut().cursor.col = new_col;
                            changed = true;
                        }
                        if changed {
                            self.trigger_auto_completion();
                        }
                    }
                }
                "Delete" => {
                    if self.visual_anchor.is_some() {
                        self.vscode_delete_selection(&mut changed);
                    } else {
                        let line = self.view().cursor.line;
                        let col = self.view().cursor.col;
                        let char_idx = self.buffer().line_to_char(line) + col;
                        if char_idx < self.buffer().len_chars() {
                            self.delete_with_undo(char_idx, char_idx + 1);
                            changed = true;
                        }
                    }
                }
                "Return" => {
                    if self.visual_anchor.is_some() {
                        self.vscode_delete_selection(&mut changed);
                    }
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    let indent = self.smart_indent_for_newline(line);
                    let indent_len = indent.len();
                    let text = format!("\n{}", indent);
                    self.insert_with_undo(char_idx, &text);
                    self.view_mut().cursor.line += 1;
                    self.view_mut().cursor.col = indent_len;
                    changed = true;
                }
                "Tab" => {
                    if self.visual_anchor.is_some() {
                        self.vscode_delete_selection(&mut changed);
                    }
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let char_idx = self.buffer().line_to_char(line) + col;
                    if self.settings.expand_tab {
                        let n = self.settings.tabstop as usize;
                        let spaces = " ".repeat(n);
                        self.insert_with_undo(char_idx, &spaces);
                        self.view_mut().cursor.col += n;
                    } else {
                        self.insert_with_undo(char_idx, "\t");
                        self.view_mut().cursor.col += 1;
                    }
                    changed = true;
                }
                // Phase 2: Shift+Tab → outdent
                "ISO_Left_Tab" => {
                    self.vscode_outdent(&mut changed);
                }
                _ => {
                    if let Some(ch) = unicode {
                        if self.visual_anchor.is_some() && !self.view().extra_cursors.is_empty() {
                            // Multi-cursor with selection (Ctrl+D/Ctrl+Shift+L):
                            // delete selected word at every cursor, then insert typed char.
                            // Uses char indices sorted descending so edits don't shift
                            // positions of earlier occurrences (critical for same-line matches).
                            let anchor = self.visual_anchor.unwrap();
                            let cursor = self.view().cursor;
                            let (sel_start, sel_end) =
                                if (anchor.line, anchor.col) <= (cursor.line, cursor.col) {
                                    (anchor, cursor)
                                } else {
                                    (cursor, anchor)
                                };
                            let sel_start_ci =
                                self.buffer().line_to_char(sel_start.line) + sel_start.col;
                            let sel_end_ci =
                                self.buffer().line_to_char(sel_end.line) + sel_end.col + 1;
                            let sel_len = sel_end_ci - sel_start_ci;

                            // Collect char indices for start of each selection
                            let extras = self.view().extra_cursors.clone();
                            let mut char_indices: Vec<usize> = Vec::new();
                            char_indices.push(sel_start_ci);
                            for ec in &extras {
                                let ec_start_col = ec.col + 1 - sel_len;
                                let ci = self.buffer().line_to_char(ec.line) + ec_start_col;
                                char_indices.push(ci);
                            }
                            // Sort descending — process rightmost/bottommost first
                            char_indices.sort_unstable_by(|a, b| b.cmp(a));

                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            let insert_len = s.len();
                            for &ci in &char_indices {
                                self.delete_with_undo(ci, ci + sel_len);
                                self.insert_with_undo(ci, s);
                            }

                            // Recompute cursor positions after all edits.
                            // Each replacement changes length by (insert_len - sel_len).
                            // Sort ascending to compute cumulative offset.
                            char_indices.sort_unstable();
                            let delta = insert_len as isize - sel_len as isize;
                            let mut new_cursors: Vec<Cursor> = Vec::new();
                            for (i, &ci) in char_indices.iter().enumerate() {
                                // After i replacements before this one, offset is i * delta
                                let adjusted_ci =
                                    (ci as isize + i as isize * delta) as usize + insert_len;
                                let line = self.buffer().content.char_to_line(adjusted_ci);
                                let line_start = self.buffer().line_to_char(line);
                                let col = adjusted_ci - line_start;
                                new_cursors.push(Cursor { line, col });
                            }
                            // First cursor is the primary
                            self.view_mut().cursor = new_cursors[0];
                            self.view_mut().extra_cursors = new_cursors[1..].to_vec();
                            self.visual_anchor = None;
                            self.mode = Mode::Insert;
                            changed = true;
                        } else if self.visual_anchor.is_some() {
                            // Single-cursor selection: delete then fall through to insert
                            self.vscode_delete_selection(&mut changed);
                            // Fall through to single-cursor insert below
                            let line = self.view().cursor.line;
                            let col = self.view().cursor.col;
                            let char_idx = self.buffer().line_to_char(line) + col;
                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            self.insert_with_undo(char_idx, s);
                            self.view_mut().cursor.col += 1;
                            changed = true;
                        } else if !self.view().extra_cursors.is_empty() {
                            // Multi-cursor without selection: insert at all positions.
                            // Use char indices to handle same-line shifts correctly.
                            let primary = *self.cursor();
                            let extras = self.view().extra_cursors.clone();
                            let mut char_indices: Vec<usize> = Vec::new();
                            char_indices
                                .push(self.buffer().line_to_char(primary.line) + primary.col);
                            for ec in &extras {
                                char_indices.push(self.buffer().line_to_char(ec.line) + ec.col);
                            }
                            char_indices.sort_unstable_by(|a, b| b.cmp(a));
                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            let insert_len = s.len();
                            for &ci in &char_indices {
                                self.insert_with_undo(ci, s);
                            }
                            // Recompute positions accounting for cumulative shifts
                            char_indices.sort_unstable();
                            let primary_ci = self.buffer().line_to_char(primary.line) + primary.col;
                            let mut new_extras: Vec<Cursor> = Vec::new();
                            for (i, &ci) in char_indices.iter().enumerate() {
                                let adjusted = ci + (i + 1) * insert_len;
                                let line = self.buffer().content.char_to_line(adjusted);
                                let line_start = self.buffer().line_to_char(line);
                                let col = adjusted - line_start;
                                let cur = Cursor { line, col };
                                if ci == primary_ci {
                                    self.view_mut().cursor = cur;
                                } else {
                                    new_extras.push(cur);
                                }
                            }
                            self.view_mut().extra_cursors = new_extras;
                            changed = true;
                        } else {
                            // Single cursor: full auto-pairs support.
                            let line = self.view().cursor.line;
                            let col = self.view().cursor.col;
                            let char_idx = self.buffer().line_to_char(line) + col;
                            let closing_pair = auto_pair_closer(ch);
                            if self.settings.auto_pairs
                                && is_closing_pair(ch)
                                && char_idx < self.buffer().len_chars()
                                && self.buffer().content.char(char_idx) == ch
                            {
                                self.view_mut().cursor.col += 1;
                                changed = true;
                            } else if self.settings.auto_pairs && closing_pair.is_some() {
                                let closer = closing_pair.unwrap();
                                let should_pair = if is_quote_char(ch) {
                                    if char_idx == 0 {
                                        true
                                    } else {
                                        let prev = self.buffer().content.char(char_idx - 1);
                                        prev.is_whitespace()
                                            || matches!(prev, '(' | '[' | '{' | ',' | ';' | ':')
                                    }
                                } else {
                                    true
                                };
                                if should_pair {
                                    let mut buf = [0u8; 4];
                                    let open_s = ch.encode_utf8(&mut buf).to_string();
                                    let mut buf2 = [0u8; 4];
                                    let close_s = closer.encode_utf8(&mut buf2).to_string();
                                    let pair = format!("{}{}", open_s, close_s);
                                    self.insert_with_undo(char_idx, &pair);
                                    self.view_mut().cursor.col += 1;
                                    changed = true;
                                } else {
                                    let mut buf = [0u8; 4];
                                    let s = ch.encode_utf8(&mut buf);
                                    self.insert_with_undo(char_idx, s);
                                    self.view_mut().cursor.col += 1;
                                    changed = true;
                                }
                            } else {
                                let mut buf = [0u8; 4];
                                let s = ch.encode_utf8(&mut buf);
                                self.insert_with_undo(char_idx, s);
                                self.view_mut().cursor.col += 1;
                                changed = true;
                            }
                        }
                        self.trigger_auto_completion();
                    }
                }
            }
        }

        if changed {
            // For plain character insertions keep the undo group open so the
            // next typed character extends the same group.  For everything
            // else (commands, Backspace, Return, etc.) close the group now.
            if is_plain_char {
                // Record cursor so we can detect external moves before the
                // next keystroke.
                self.vscode_undo_cursor = (self.view().cursor.line, self.view().cursor.col);
            } else {
                self.finish_undo_group();
            }
            self.set_dirty(true);
            self.update_syntax();
            let active_id = self.active_buffer_id();
            if self.preview_buffer_id == Some(active_id) {
                self.promote_preview(active_id);
            }
            self.lsp_dirty_buffers.insert(active_id, true);
            self.swap_mark_dirty();
            // Refresh search highlights so they track the new buffer content.
            if !self.search_matches.is_empty() {
                self.run_search();
            }
        }

        self.ensure_cursor_visible();
        self.sync_scroll_binds();
        self.update_bracket_match();

        // Fire cursor_move plugin hook. VSCode mode is always Mode::Insert,
        // but plugins (like git-insights blame) need cursor position updates.
        // Plugins handle their own deduplication (blame.lua checks last_line).
        self.fire_cursor_move_hook();

        EngineAction::None
    }
}
