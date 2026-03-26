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
                let end_char = self.buffer().line_to_char(end.line) + end.col;

                // Include the character at the end position (Vim-like inclusive)
                let end_char_inclusive = (end_char + 1).min(self.buffer().len_chars());

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

        // Exit visual mode
        self.mode = Mode::Normal;
        self.visual_anchor = None;
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
                    let end_char = self.buffer().line_to_char(end.line) + end.col + 1;

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
        // Change is like delete, but then enter insert mode
        self.delete_visual_selection(changed);

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
    }
}
