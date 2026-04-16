use super::*;

impl Engine {
    // =======================================================================
    // Visual mode helpers
    // =======================================================================

    /// Get normalized visual selection range (start, end).
    /// Start is always before or equal to end.
    pub(crate) fn get_visual_selection_range(&self) -> Option<(Cursor, Cursor)> {
        let anchor = self.visual_anchor?;
        let cursor = self.view().cursor;

        // Normalize so start <= end
        let (start, end) = if anchor.line < cursor.line
            || (anchor.line == cursor.line && anchor.col <= cursor.col)
        {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };

        Some((start, end))
    }

    /// Extract the text from the visual selection.
    /// Returns (text, is_linewise).
    pub(crate) fn get_visual_selection_text(&self) -> Option<(String, bool)> {
        let (start, end) = self.get_visual_selection_range()?;

        match self.mode {
            Mode::VisualLine => {
                // Line mode: extract full lines from start.line to end.line (inclusive)
                let start_char = self.buffer().line_to_char(start.line);
                let end_line = end.line;
                let end_char = if end_line + 1 < self.buffer().len_lines() {
                    self.buffer().line_to_char(end_line + 1)
                } else {
                    self.buffer().len_chars()
                };

                let text = self
                    .buffer()
                    .content
                    .slice(start_char..end_char)
                    .to_string();

                // Ensure it ends with newline for linewise
                let text = if text.ends_with('\n') {
                    text
                } else {
                    format!("{}\n", text)
                };

                Some((text, true))
            }
            Mode::Visual => {
                // Character mode: extract from start to end (inclusive)
                let start_char = self.buffer().line_to_char(start.line) + start.col;
                let mut end_char_inclusive = self.buffer().line_to_char(end.line) + end.col + 1;

                // When $ was used, extend through the newline (Vim curswant=MAXCOL)
                if self.visual_dollar {
                    let line_len = self.buffer().line_len_chars(end.line);
                    end_char_inclusive = self.buffer().line_to_char(end.line) + line_len;
                }

                let end_char_inclusive = end_char_inclusive.min(self.buffer().len_chars());

                let text = self
                    .buffer()
                    .content
                    .slice(start_char..end_char_inclusive)
                    .to_string();

                Some((text, false))
            }
            Mode::VisualBlock => {
                // Block mode: extract rectangular region
                // Use anchor and cursor columns directly for block selection
                let anchor = self.visual_anchor?;
                let cursor = self.view().cursor;
                let start_col = anchor.col.min(cursor.col);
                let end_col = anchor.col.max(cursor.col);

                let mut lines = Vec::new();

                for line_idx in start.line..=end.line {
                    if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                        let line_str = line.to_string();
                        let line_chars: Vec<char> = line_str.chars().collect();

                        // Extract the block portion of this line
                        let block_start = start_col.min(line_chars.len());
                        let block_end = (end_col + 1).min(line_chars.len());

                        let block_text: String = if block_start < line_chars.len() {
                            line_chars[block_start..block_end].iter().collect()
                        } else {
                            // Line is too short, just use empty string
                            String::new()
                        };

                        lines.push(block_text);
                    }
                }

                let text = lines.join("\n");
                Some((text, false))
            }
            _ => None,
        }
    }

    pub fn yank_visual_selection(&mut self) {
        // Capture the selection region for highlight before exiting visual mode
        let hl_region = self.get_visual_selection_range().map(|(start, end)| {
            let is_linewise = matches!(self.mode, Mode::VisualLine);
            (start, end, is_linewise)
        });

        if let Some((text, is_linewise)) = self.get_visual_selection_text() {
            // Store in selected register (or unnamed register)
            let reg = self.selected_register.unwrap_or('"');
            self.set_yank_register(reg, text, is_linewise);

            self.selected_register = None;
            self.message = format!("{} yanked", if is_linewise { "Line(s)" } else { "Text" });

            if let Some((start, end, lw)) = hl_region {
                self.record_yank_highlight(start, end, lw);
            }
        }

        // Vim behavior: move cursor to start of selection after yank
        if let Some((start, _end)) = self.get_visual_selection_range() {
            let is_linewise = matches!(self.mode, Mode::VisualLine);
            self.view_mut().cursor.line = start.line;
            if is_linewise {
                self.view_mut().cursor.col = 0;
            } else {
                self.view_mut().cursor.col = start.col;
            }
        }

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.visual_dollar = false;
    }

    pub fn delete_visual_selection(&mut self, changed: &mut bool) {
        if let Some((text, is_linewise)) = self.get_visual_selection_text() {
            // Store in register
            let reg = self.selected_register.unwrap_or('"');
            self.set_delete_register(reg, text, is_linewise);
            self.selected_register = None;

            // Delete the selection
            let (start, end) = self.get_visual_selection_range().unwrap();

            self.start_undo_group();

            match self.mode {
                Mode::VisualLine => {
                    // Delete full lines
                    let start_char = self.buffer().line_to_char(start.line);
                    let end_char = if end.line + 1 < self.buffer().len_lines() {
                        self.buffer().line_to_char(end.line + 1)
                    } else {
                        self.buffer().len_chars()
                    };

                    self.delete_with_undo(start_char, end_char);

                    // Position cursor at start of line
                    self.view_mut().cursor.line = start.line;
                    self.view_mut().cursor.col = 0;
                }
                Mode::Visual => {
                    // Delete characters
                    let start_char = self.buffer().line_to_char(start.line) + start.col;
                    let mut end_char = self.buffer().line_to_char(end.line) + end.col + 1;

                    // When $ was used, extend through the newline (Vim curswant=MAXCOL)
                    if self.visual_dollar {
                        let line_len = self.buffer().line_len_chars(end.line);
                        end_char = self.buffer().line_to_char(end.line) + line_len;
                    }

                    self.delete_with_undo(start_char, end_char.min(self.buffer().len_chars()));

                    // Position cursor at start
                    self.view_mut().cursor = start;
                }
                Mode::VisualBlock => {
                    // Delete rectangular block (work backwards to avoid offset issues)
                    // Use anchor and cursor columns directly for block selection
                    let anchor = self.visual_anchor.unwrap();
                    let cursor = self.view().cursor;
                    let start_col = anchor.col.min(cursor.col);
                    let end_col = anchor.col.max(cursor.col);

                    for line_idx in (start.line..=end.line).rev() {
                        let line_start_char = self.buffer().line_to_char(line_idx);
                        if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                            let line_str = line.to_string();
                            let line_len = line_str.chars().count();

                            // Only delete if the line is long enough to have characters in the block
                            if start_col < line_len {
                                let block_end = (end_col + 1).min(line_len);
                                let del_start = line_start_char + start_col;
                                let del_end = line_start_char + block_end;
                                self.delete_with_undo(del_start, del_end);
                            }
                        }
                    }

                    // Position cursor at start of block
                    self.view_mut().cursor.line = start.line;
                    self.view_mut().cursor.col = start_col;
                }
                _ => {}
            }

            self.finish_undo_group();
            *changed = true;
            self.clamp_cursor_col();
        }

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.visual_dollar = false;
    }

    /// Paste over visual selection: replace selected text with register content.
    /// The deleted selection goes into the unnamed register (Vim behavior).
    pub(crate) fn paste_visual_selection(&mut self, changed: &mut bool) {
        // 1. Read the register content BEFORE deleting (delete overwrites unnamed reg)
        let paste_reg = self.active_register();
        let (paste_content, paste_linewise) = match self.get_register_content(paste_reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                // Still delete selection even if register is empty
                self.delete_visual_selection(changed);
                return;
            }
        };

        // 2. Get the selection text and range before deleting
        let sel_linewise = matches!(self.mode, Mode::VisualLine);
        let sel_range = self.get_visual_selection_range();

        // 3. Delete the visual selection (stores deleted text in unnamed reg)
        self.delete_visual_selection(changed);

        // 4. Now paste the saved register content at the cursor position
        let start = if let Some((s, _)) = sel_range {
            s
        } else {
            return;
        };

        self.start_undo_group();

        if sel_linewise || paste_linewise {
            // Linewise paste: insert on its own line
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            // Ensure paste content ends with newline
            let content = if paste_content.ends_with('\n') {
                paste_content
            } else {
                format!("{}\n", paste_content)
            };
            self.insert_with_undo(line_start, &content);
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = 0;
        } else {
            // Characterwise paste: insert at the start of the deleted selection
            // (not cursor, which may have been clamped by delete_visual_selection)
            let line = start.line;
            let col = start.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            self.insert_with_undo(char_idx, &paste_content);
            // Position cursor at end of pasted text
            let paste_len = paste_content.chars().count();
            if paste_len > 0 {
                if paste_content.contains('\n') {
                    let lines: Vec<&str> = paste_content.split('\n').collect();
                    let last_line = lines.last().unwrap_or(&"");
                    self.view_mut().cursor.line = start.line + lines.len() - 1;
                    if lines.len() > 1 {
                        self.view_mut().cursor.col = last_line.chars().count().saturating_sub(1);
                    } else {
                        self.view_mut().cursor.col = col + paste_len - 1;
                    }
                } else {
                    self.view_mut().cursor.col = col + paste_len - 1;
                }
            }
        }

        self.finish_undo_group();
        self.clamp_cursor_col();
        self.clear_selected_register();
    }

    pub(crate) fn change_visual_selection(&mut self, changed: &mut bool) {
        let was_visual_line = self.mode == Mode::VisualLine;

        if was_visual_line {
            // In visual-line mode, "c" replaces the selected lines with an
            // empty line and enters insert mode on it (Vim behavior).  We
            // delete the line *contents* of all selected lines and collapse
            // them into a single empty line, preserving the trailing newline
            // so text typed afterwards stays on its own line.
            if let Some((text, _)) = self.get_visual_selection_text() {
                let reg = self.selected_register.unwrap_or('"');
                self.set_delete_register(reg, text, true);
                self.selected_register = None;
            }
            if let Some((start, end)) = self.get_visual_selection_range() {
                self.start_undo_group();
                let start_char = self.buffer().line_to_char(start.line);
                // Delete up to (but not including) the newline of the last selected line,
                // unless the last selected line is the very last line in the buffer.
                let end_char = if end.line + 1 < self.buffer().len_lines() {
                    // There's a line after the selection — delete through
                    // end.line's newline but stop before end.line+1's content.
                    self.buffer().line_to_char(end.line + 1)
                } else {
                    self.buffer().len_chars()
                };
                // We want to leave a single empty line.  Delete everything
                // from start_char..end_char, then re-insert a "\n" if we
                // consumed the trailing newline of the last selected line.
                let deleted_trailing_nl = end.line + 1 < self.buffer().len_lines();
                self.delete_with_undo(start_char, end_char);
                if deleted_trailing_nl {
                    // Re-insert the newline we consumed so the cursor sits
                    // on its own (empty) line.
                    self.insert_with_undo(start_char, "\n");
                }
                self.finish_undo_group();
                *changed = true;
                self.view_mut().cursor.line = start.line;
                self.view_mut().cursor.col = 0;
            }
            // Exit visual mode, enter insert mode
            self.visual_anchor = None;
            self.visual_dollar = false;
        } else {
            // Charwise / blockwise: delete selection normally
            self.delete_visual_selection(changed);
        }

        // The delete already finished the undo group and set mode to Normal
        // Now start a new undo group for the insert mode typing
        self.start_undo_group();
        self.insert_text_buffer.clear();
        self.mode = Mode::Insert;
    }

    pub(crate) fn lowercase_visual_selection(&mut self, changed: &mut bool) {
        self.transform_visual_selection(|s| s.to_lowercase(), changed);
    }

    pub(crate) fn uppercase_visual_selection(&mut self, changed: &mut bool) {
        self.transform_visual_selection(|s| s.to_uppercase(), changed);
    }

    pub(crate) fn transform_visual_selection<F>(&mut self, transform: F, changed: &mut bool)
    where
        F: Fn(&str) -> String,
    {
        let (start, end) = match self.get_visual_selection_range() {
            Some(range) => range,
            None => return,
        };

        self.start_undo_group();

        match self.mode {
            Mode::VisualLine => {
                // Transform full lines
                for line_idx in start.line..=end.line {
                    if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                        let line_str = line.to_string();
                        let transformed = transform(&line_str);

                        // Replace the line
                        let line_start_char = self.buffer().line_to_char(line_idx);
                        let line_end_char = line_start_char + line_str.chars().count();
                        self.delete_with_undo(line_start_char, line_end_char);
                        self.insert_with_undo(line_start_char, &transformed);
                    }
                }

                // Position cursor at start of first line
                self.view_mut().cursor.line = start.line;
                self.view_mut().cursor.col = 0;
            }
            Mode::Visual => {
                // Transform character selection
                if let Some((text, _)) = self.get_visual_selection_text() {
                    let transformed = transform(&text);

                    let start_char = self.buffer().line_to_char(start.line) + start.col;
                    let end_char = self.buffer().line_to_char(end.line) + end.col + 1;

                    self.delete_with_undo(start_char, end_char.min(self.buffer().len_chars()));
                    self.insert_with_undo(start_char, &transformed);

                    // Position cursor at start
                    self.view_mut().cursor = start;
                }
            }
            Mode::VisualBlock => {
                // Transform rectangular block (work backwards to maintain positions)
                let anchor = self.visual_anchor.unwrap();
                let cursor = self.view().cursor;
                let start_col = anchor.col.min(cursor.col);
                let end_col = anchor.col.max(cursor.col);

                for line_idx in (start.line..=end.line).rev() {
                    let line_start_char = self.buffer().line_to_char(line_idx);
                    if let Some(line) = self.buffer().content.lines().nth(line_idx) {
                        let line_str = line.to_string();
                        let line_chars: Vec<char> = line_str.chars().collect();

                        // Extract and transform the block portion
                        if start_col < line_chars.len() {
                            let block_end = (end_col + 1).min(line_chars.len());
                            let block_text: String =
                                line_chars[start_col..block_end].iter().collect();
                            let transformed = transform(&block_text);

                            let del_start = line_start_char + start_col;
                            let del_end = line_start_char + block_end;
                            self.delete_with_undo(del_start, del_end);
                            self.insert_with_undo(del_start, &transformed);
                        }
                    }
                }

                // Position cursor at start of block
                self.view_mut().cursor.line = start.line;
                self.view_mut().cursor.col = start_col;
            }
            _ => {}
        }

        self.finish_undo_group();
        *changed = true;
        self.clamp_cursor_col();

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.visual_dollar = false;
    }
}

// ─── Additional methods (extracted from mod.rs) ─────────────────────────

impl Engine {
    // =======================================================================
    // Multiple cursors (Alt-D)
    // =======================================================================

    /// Convert a char index in the current buffer into a `Cursor` (line, col).
    pub(crate) fn char_idx_to_cursor(&self, char_idx: usize) -> Cursor {
        let len = self.buffer().content.len_chars();
        let char_idx = char_idx.min(len);
        if len == 0 {
            return Cursor { line: 0, col: 0 };
        }
        let line = self.buffer().content.char_to_line(char_idx);
        let line_start = self.buffer().line_to_char(line);
        Cursor {
            line,
            col: char_idx - line_start,
        }
    }

    /// Convert a byte offset in the buffer text into a `Cursor`.
    pub(crate) fn byte_offset_to_cursor(&self, byte_offset: usize) -> Cursor {
        let char_idx = self.buffer().content.byte_to_char(byte_offset);
        self.char_idx_to_cursor(char_idx)
    }

    /// Search for the next occurrence of `pattern` in the buffer, starting
    /// one pattern-length past `after`.  Wraps around the document end.
    /// Returns `None` if `pattern` is not found anywhere in the buffer.
    pub(crate) fn find_next_occurrence(
        &self,
        pattern: &str,
        after: Cursor,
        word_bounded: bool,
    ) -> Option<Cursor> {
        if pattern.is_empty() {
            return None;
        }

        let text = self.buffer().to_string();
        // Start searching one pattern-length past the given cursor position.
        let after_char_idx =
            self.buffer().line_to_char(after.line) + after.col + pattern.chars().count();
        let after_byte = self
            .buffer()
            .content
            .char_to_byte(after_char_idx.min(self.buffer().content.len_chars()));

        let check_boundary = |sb: usize, eb: usize| -> bool {
            if !word_bounded {
                return true;
            }
            let before_ok =
                sb == 0 || !Self::is_word_char(text[..sb].chars().last().unwrap_or(' '));
            let after_ok =
                eb >= text.len() || !Self::is_word_char(text[eb..].chars().next().unwrap_or(' '));
            before_ok && after_ok
        };

        // Pass 1: from after_byte to end of document.
        let mut byte_pos = after_byte;
        while byte_pos < text.len() {
            match text[byte_pos..].find(pattern) {
                None => break,
                Some(found) => {
                    let sb = byte_pos + found;
                    let eb = sb + pattern.len();
                    if check_boundary(sb, eb) {
                        return Some(self.byte_offset_to_cursor(sb));
                    }
                    byte_pos = sb + 1;
                }
            }
        }

        // Pass 2: wrap around from document start to after_byte.
        byte_pos = 0;
        while byte_pos < after_byte {
            match text[byte_pos..].find(pattern) {
                None => break,
                Some(found) => {
                    let sb = byte_pos + found;
                    if sb >= after_byte {
                        break;
                    }
                    let eb = sb + pattern.len();
                    if check_boundary(sb, eb) {
                        return Some(self.byte_offset_to_cursor(sb));
                    }
                    byte_pos = sb + 1;
                }
            }
        }

        None
    }

    /// Collect all byte-offset positions of `pattern` in the current buffer,
    /// returning them as `Cursor` values.  When `word_bounded` is true only
    /// whole-word matches are returned.
    pub(crate) fn collect_all_occurrences(&self, pattern: &str, word_bounded: bool) -> Vec<Cursor> {
        if pattern.is_empty() {
            return vec![];
        }
        let text = self.buffer().to_string();
        let mut results = Vec::new();
        let mut byte_pos = 0;
        while byte_pos < text.len() {
            match text[byte_pos..].find(pattern) {
                None => break,
                Some(found) => {
                    let sb = byte_pos + found;
                    let eb = sb + pattern.len();
                    let ok = if word_bounded {
                        let before_ok = sb == 0
                            || !Self::is_word_char(text[..sb].chars().last().unwrap_or(' '));
                        let after_ok = eb >= text.len()
                            || !Self::is_word_char(text[eb..].chars().next().unwrap_or(' '));
                        before_ok && after_ok
                    } else {
                        true
                    };
                    if ok {
                        results.push(self.byte_offset_to_cursor(sb));
                    }
                    byte_pos = sb + 1;
                }
            }
        }
        results
    }

    /// Add secondary cursors at *every* occurrence of the word under the
    /// primary cursor.  Called by backends when `select_all_matches` is pressed.
    pub fn select_all_word_occurrences(&mut self) -> EngineAction {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return EngineAction::None;
            }
        };
        let all = self.collect_all_occurrences(&word, true);
        if all.is_empty() {
            self.message = format!("No occurrences of '{}'", word);
            return EngineAction::None;
        }
        let primary = *self.cursor();
        let extras: Vec<Cursor> = all.into_iter().filter(|&c| c != primary).collect();
        let n = extras.len();
        self.view_mut().extra_cursors = extras;
        self.message = format!("{} cursors (all occurrences of '{}')", n + 1, word);
        EngineAction::None
    }

    /// Add a secondary cursor at the given `(line, col)` position.
    /// Does nothing if the position equals the primary cursor or is already
    /// present in `extra_cursors`.
    pub fn add_cursor_at_pos(&mut self, line: usize, col: usize) {
        let new_cursor = Cursor { line, col };
        if new_cursor == *self.cursor() {
            return;
        }
        if self.view().extra_cursors.contains(&new_cursor) {
            return;
        }
        self.view_mut().extra_cursors.push(new_cursor);
    }

    /// Add a secondary cursor at the next occurrence of the word under the
    /// primary cursor (or after the last extra cursor if any exist).
    /// Called by backends when the configured `add_cursor` key is pressed.
    pub fn add_cursor_at_next_match(&mut self) -> EngineAction {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return EngineAction::None;
            }
        };
        let search_after = self
            .view()
            .extra_cursors
            .last()
            .copied()
            .unwrap_or_else(|| *self.cursor());
        if let Some(new_cursor) = self.find_next_occurrence(&word, search_after, true) {
            let is_primary = new_cursor == *self.cursor();
            let already_extra = self.view().extra_cursors.contains(&new_cursor);
            if !is_primary && !already_extra {
                self.view_mut().extra_cursors.push(new_cursor);
                let total = self.view().extra_cursors.len() + 1; // +1 for primary
                self.message = format!("{} cursors ('{}')", total, word);
            } else {
                self.message = format!("No more occurrences of '{}'", word);
            }
        } else {
            self.message = format!("No more occurrences of '{}'", word);
        }
        EngineAction::None
    }

    // ── Multi-cursor editing helpers ─────────────────────────────────────────

    /// Insert `text` at every cursor position (primary + extra) simultaneously.
    /// Processes in ascending char-index order with a running offset so that
    /// each subsequent insert uses the correct adjusted position.
    /// Updates primary cursor and all extra cursors to point just after their
    /// respective inserted text.
    pub(crate) fn mc_insert(&mut self, text: &str) {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;
        let extra_origs: Vec<usize> = extra
            .iter()
            .map(|c| self.buffer().line_to_char(c.line) + c.col)
            .collect();

        // All original char indices sorted ascending (safe to sort since positions are distinct).
        let mut all_origs: Vec<usize> = extra_origs.clone();
        all_origs.push(primary_orig);
        all_origs.sort_unstable();

        let insert_chars = text.chars().count();

        // Pre-compute new char indices before modifying the buffer.
        // Cursor at ascending rank i → new_cidx = orig + (rank+1)*insert_chars.
        let rank_of = |orig: usize| all_origs.iter().position(|&x| x == orig).unwrap_or(0);
        let primary_new_cidx = primary_orig + (rank_of(primary_orig) + 1) * insert_chars;
        let extra_new_cidxs: Vec<usize> = extra_origs
            .iter()
            .map(|&orig| orig + (rank_of(orig) + 1) * insert_chars)
            .collect();

        // Insert in ascending order with cumulative offset.
        let mut offset = 0usize;
        for &orig in &all_origs {
            self.insert_with_undo(orig + offset, text);
            offset += insert_chars;
        }

        // Apply updated positions (buffer is now modified; char_idx_to_cursor uses new state).
        self.view_mut().cursor = self.char_idx_to_cursor(primary_new_cidx);
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .map(|&cidx| self.char_idx_to_cursor(cidx))
            .collect();
    }

    /// Delete one char before every cursor position with col > 0.
    /// Extra cursors at col == 0 are left in place (line-merge not done in multi-cursor mode).
    /// Returns `true` if at least one deletion was performed.
    pub(crate) fn mc_backspace(&mut self) -> bool {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;

        // Pre-compute original char indices for extra cursors (before any modification).
        let extra_data: Vec<(usize, bool)> = extra
            .iter()
            .map(|c| {
                let orig = self.buffer().line_to_char(c.line) + c.col;
                (orig, c.col > 0)
            })
            .collect();

        // Collect eligible (col > 0) original char indices.
        let mut all_eligible: Vec<usize> = Vec::new();
        let primary_eligible = primary.col > 0;
        if primary_eligible {
            all_eligible.push(primary_orig);
        }
        for &(orig, eligible) in &extra_data {
            if eligible {
                all_eligible.push(orig);
            }
        }

        if all_eligible.is_empty() {
            return false;
        }

        all_eligible.sort_unstable();

        // Pre-compute new char indices before modifying the buffer.
        // Cursor at ascending rank i → new_cidx = orig - (rank+1).
        let rank_of = |orig: usize| all_eligible.iter().position(|&x| x == orig).unwrap_or(0);
        let primary_new_cidx = if primary_eligible {
            Some(primary_orig - (rank_of(primary_orig) + 1))
        } else {
            None
        };
        let extra_new_cidxs: Vec<Option<usize>> = extra_data
            .iter()
            .map(|&(orig, eligible)| {
                if eligible {
                    Some(orig - (rank_of(orig) + 1))
                } else {
                    None
                }
            })
            .collect();

        // Delete in DESCENDING order (no offset adjustment needed).
        for &orig in all_eligible.iter().rev() {
            self.delete_with_undo(orig - 1, orig);
        }

        // Apply updated positions.
        if let Some(new_cidx) = primary_new_cidx {
            self.view_mut().cursor = self.char_idx_to_cursor(new_cidx);
        }
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .zip(extra.iter())
            .map(|(&opt, ec)| {
                if let Some(new_cidx) = opt {
                    self.char_idx_to_cursor(new_cidx)
                } else {
                    *ec // unchanged (was at col == 0)
                }
            })
            .collect();

        true
    }

    /// Delete one char after every cursor position that is not at end-of-buffer.
    /// Returns `true` if at least one deletion was performed.
    pub(crate) fn mc_delete_forward(&mut self) -> bool {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        let buf_len = self.buffer().content.len_chars();
        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;

        let extra_data: Vec<(usize, bool)> = extra
            .iter()
            .map(|c| {
                let orig = self.buffer().line_to_char(c.line) + c.col;
                (orig, orig < buf_len)
            })
            .collect();

        let mut all_eligible: Vec<usize> = Vec::new();
        let primary_eligible = primary_orig < buf_len;
        if primary_eligible {
            all_eligible.push(primary_orig);
        }
        for &(orig, eligible) in &extra_data {
            if eligible {
                all_eligible.push(orig);
            }
        }

        if all_eligible.is_empty() {
            return false;
        }

        all_eligible.sort_unstable();

        // Pre-compute new char indices.
        // Delete-forward: cursor stays in place; earlier deletions shift it left.
        // Cursor at ascending rank i → new_cidx = orig - rank (not rank+1).
        let rank_of = |orig: usize| all_eligible.iter().position(|&x| x == orig).unwrap_or(0);
        let primary_new_cidx = if primary_eligible {
            Some(primary_orig - rank_of(primary_orig))
        } else {
            None
        };
        let extra_new_cidxs: Vec<Option<usize>> = extra_data
            .iter()
            .map(|&(orig, eligible)| {
                if eligible {
                    Some(orig - rank_of(orig))
                } else {
                    None
                }
            })
            .collect();

        // Delete in DESCENDING order.
        for &orig in all_eligible.iter().rev() {
            self.delete_with_undo(orig, orig + 1);
        }

        if let Some(new_cidx) = primary_new_cidx {
            self.view_mut().cursor = self.char_idx_to_cursor(new_cidx);
        }
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .zip(extra.iter())
            .map(|(&opt, ec)| {
                if let Some(new_cidx) = opt {
                    self.char_idx_to_cursor(new_cidx)
                } else {
                    *ec
                }
            })
            .collect();

        true
    }

    /// Insert a newline (+ auto-indent) at every cursor position.
    /// Each cursor gets the indent of its own line computed before any modification.
    pub(crate) fn mc_return(&mut self) {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        // Pre-compute (orig_cidx, insert_text) for every cursor, ascending.
        struct ReturnOp {
            orig_cidx: usize,
            text: String,
            is_primary: bool,
            extra_idx: usize,
        }

        let primary_indent = if self.settings.auto_indent {
            self.get_line_indent_str(primary.line)
        } else {
            String::new()
        };

        let extra_ops: Vec<(usize, String)> = extra
            .iter()
            .map(|c| {
                let orig = self.buffer().line_to_char(c.line) + c.col;
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(c.line)
                } else {
                    String::new()
                };
                (orig, format!("\n{}", indent))
            })
            .collect();

        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;
        let primary_text = format!("\n{}", primary_indent);

        let mut all_ops: Vec<ReturnOp> = extra_ops
            .iter()
            .enumerate()
            .map(|(i, (orig, text))| ReturnOp {
                orig_cidx: *orig,
                text: text.clone(),
                is_primary: false,
                extra_idx: i,
            })
            .collect();
        all_ops.push(ReturnOp {
            orig_cidx: primary_orig,
            text: primary_text,
            is_primary: true,
            extra_idx: 0,
        });
        all_ops.sort_by_key(|op| op.orig_cidx);

        // Apply inserts ascending with cumulative offset; cursor goes to end of each insert.
        let mut running_offset = 0usize;
        let mut primary_new_cidx = 0usize;
        let mut extra_new_cidxs = vec![0usize; extra.len()];

        for op in &all_ops {
            let text_chars = op.text.chars().count();
            let insert_at = op.orig_cidx + running_offset;
            self.insert_with_undo(insert_at, &op.text);
            let new_cidx = insert_at + text_chars;
            running_offset += text_chars;
            if op.is_primary {
                primary_new_cidx = new_cidx;
            } else {
                extra_new_cidxs[op.extra_idx] = new_cidx;
            }
        }

        self.view_mut().cursor = self.char_idx_to_cursor(primary_new_cidx);
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .map(|&cidx| self.char_idx_to_cursor(cidx))
            .collect();
    }
}
